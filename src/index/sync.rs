//! Incremental index refresh planning and update application.
//!
//! [`RefreshPlan`] collects one filesystem/store snapshot. [`IndexUpdate`]
//! holds reconciled changes before persistence or materialization.

use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};

use super::{
    FileIndex, IndexResult,
    delta::{IndexDelta, InlinkDelta},
    inlinks::{self, InlinkMap},
    service::IndexerService,
    store::IndexStore,
};
use crate::{FileBase, Note};

/// Changed-row counts from an incremental synchronization.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub struct SyncReport {
    upserted: usize,
    deleted: usize,
    links_modified: usize,
}

impl SyncReport {
    /// Creates a report from changed-row counts.
    #[inline]
    #[must_use]
    pub const fn new(
        upserted: usize,
        deleted: usize,
        links_modified: usize,
    ) -> Self {
        Self {
            upserted,
            deleted,
            links_modified,
        }
    }

    /// Files inserted or updated.
    #[inline]
    #[must_use]
    pub const fn upserted(self) -> usize {
        self.upserted
    }

    /// Files deleted from the index.
    #[inline]
    #[must_use]
    pub const fn deleted(self) -> usize {
        self.deleted
    }

    /// Inbound link edges updated.
    #[inline]
    #[must_use]
    pub const fn links_modified(self) -> usize {
        self.links_modified
    }
}

/// Which inlink recomputation one pass performed; owned by `IndexUpdate`.
pub(crate) enum InlinkReconciliation {
    /// Only modified sources were re-resolved; candidate paths unchanged.
    ContentOnlyPatch {
        links: InlinkMap,
        modified_notes: Vec<Note>,
    },
    /// Candidate paths changed; the whole graph was rebuilt and the full note
    /// set is already merged.
    PathSetRebuild {
        links: InlinkMap,
        notes: Vec<Note>,
    },
}

/// Scanned-and-diffed state of one index root, before note parsing.
pub(super) struct RefreshPlan {
    store: IndexStore,
    current_files: Vec<FileBase>,
    persisted_files: Vec<FileBase>,
    prev_links: InlinkMap,
    delta: IndexDelta,
}

impl RefreshPlan {
    /// Opens `IndexStore`, scans the filesystem, and reads persisted files and
    /// inlinks.
    ///
    /// The filesystem walk runs concurrently with `IndexStore::open` and the
    /// store read. Only the read waits on open; the scan continues
    /// independently.
    ///
    /// # Errors
    ///
    /// - `IndexError::Walk` if a directory cannot be read.
    /// - `IndexError::NoteParse` if file metadata cannot be inspected.
    /// - `IndexError::Path` if a walked file cannot be derived as a safe
    ///   project-relative path.
    /// - `IndexError::Store` if opening or reading the store fails.
    pub(super) fn collect(root: &Path) -> IndexResult<Self> {
        let (opened, scanned) = rayon::join(
            || -> IndexResult<_> {
                let store = IndexStore::open(root)?;
                let (persisted_files, prev_links) =
                    store.read_files_and_links()?;
                Ok((store, persisted_files, prev_links))
            },
            || IndexerService::scan(root),
        );
        let (store, persisted_files, prev_links) = opened?;
        let current_files = scanned?;
        let delta = IndexDelta::compute(&current_files, &persisted_files);
        Ok(Self {
            store,
            current_files,
            persisted_files,
            prev_links,
            delta,
        })
    }

    #[inline]
    pub(super) fn is_empty(&self) -> bool {
        self.delta.is_empty()
    }

    #[inline]
    pub(super) fn upserted_files(&self) -> &[FileBase] {
        self.delta.upserted()
    }

    #[inline]
    pub(super) fn into_store(self) -> IndexStore {
        self.store
    }

    /// Reconciles reparsed notes with persisted state.
    ///
    /// # Errors
    ///
    /// - `IndexError::Store` if a path-set rebuild cannot read persisted notes.
    pub(super) fn reconcile(
        self,
        modified_notes: Vec<Note>,
    ) -> IndexResult<(IndexStore, IndexUpdate)> {
        let (inlinks, inlink_delta) =
            if Self::is_paths_unchanged(&self.delta, &self.persisted_files) {
                let links = Self::patch_links(
                    &self.prev_links,
                    &modified_notes,
                    &self.current_files,
                );
                let inlink_delta =
                    InlinkDelta::compute(&links, &self.prev_links);
                (
                    InlinkReconciliation::ContentOnlyPatch {
                        links,
                        modified_notes,
                    },
                    inlink_delta,
                )
            } else {
                let notes = merge_refreshed_notes(
                    &self.store,
                    &self.delta,
                    modified_notes,
                )?;
                let links = InlinkMap::new(&notes, &self.current_files);
                let inlink_delta =
                    InlinkDelta::compute(&links, &self.prev_links);
                (
                    InlinkReconciliation::PathSetRebuild {
                        links,
                        notes,
                    },
                    inlink_delta,
                )
            };
        let update = IndexUpdate {
            current_files: self.current_files,
            delta: self.delta,
            inlinks,
            inlink_delta,
        };
        Ok((self.store, update))
    }

    /// Reports whether `delta` changed only content at previously indexed
    /// paths.
    ///
    /// Only this case leaves link resolution for unedited notes invariant.
    fn is_paths_unchanged(
        delta: &IndexDelta,
        persisted_files: &[FileBase],
    ) -> bool {
        delta.deleted().is_empty()
            && delta.upserted().iter().all(|file| {
                persisted_files
                    .binary_search_by(|p| p.path().cmp(file.path()))
                    .is_ok()
            })
    }

    /// Replaces inbound edges sourced by modified notes after re-resolving
    /// their current outlinks.
    ///
    /// Sound only when [`Self::is_paths_unchanged`] holds.
    fn patch_links(
        prev_links: &InlinkMap,
        modified_notes: &[Note],
        current_files: &[FileBase],
    ) -> InlinkMap {
        let edited: HashSet<&Path> =
            modified_notes.iter().map(Note::path).collect();
        let new_edges =
            inlinks::resolve_edges_for(modified_notes, current_files);
        prev_links.without_sources(&edited).with_edges(new_edges)
    }
}

/// Computed facts from one reconciliation pass, not yet applied. Pure data: no
/// store handle, constructible and assertable without a database.
pub(super) struct IndexUpdate {
    current_files: Vec<FileBase>,
    delta: IndexDelta,
    inlinks: InlinkReconciliation,
    inlink_delta: InlinkDelta,
}

impl IndexUpdate {
    pub(super) fn report(&self) -> SyncReport {
        SyncReport::new(
            self.delta.upserted().len(),
            self.delta.deleted().len(),
            self.inlink_delta
                .upserted()
                .len()
                .saturating_add(self.inlink_delta.deleted().len()),
        )
    }

    /// Persists this update's row-level changes.
    ///
    /// # Errors
    ///
    /// - `IndexError::Store` if incremental persistence fails.
    pub(super) fn persist(&self, store: &IndexStore) -> IndexResult<()> {
        match &self.inlinks {
            InlinkReconciliation::ContentOnlyPatch {
                modified_notes,
                ..
            } => store.persist_incremental(
                &self.delta,
                modified_notes,
                &self.inlink_delta,
            ),
            InlinkReconciliation::PathSetRebuild {
                notes,
                ..
            } => store.persist_incremental_rebuilt_notes(
                &self.delta,
                notes,
                &self.inlink_delta,
            ),
        }
    }

    /// Materializes this update into an in-memory [`FileIndex`].
    ///
    /// # Errors
    ///
    /// - `IndexError::Store` if content-only materialization cannot read
    ///   persisted notes.
    pub(super) fn into_index(
        self,
        store: &IndexStore,
    ) -> IndexResult<FileIndex> {
        let (links, notes) = match self.inlinks {
            InlinkReconciliation::ContentOnlyPatch {
                links,
                modified_notes,
            } => {
                let notes =
                    merge_refreshed_notes(store, &self.delta, modified_notes)?;
                (links, notes)
            }
            InlinkReconciliation::PathSetRebuild {
                links,
                notes,
            } => (links, notes),
        };
        Ok(FileIndex::assemble(self.current_files, notes, links))
    }
}

/// Merges persisted notes with deletions and reparsed notes for a full
/// recompute.
///
/// Bulk-reads all persisted notes because this fallback needs the full note
/// set. Path-indexed deletes and replacements avoid an `O((deleted + modified)
/// * n)` scan over the persisted note count `n`.
/// # Errors
///
/// - `IndexError::Store` if persisted notes cannot be read or decoded.
fn merge_refreshed_notes(
    store: &IndexStore,
    delta: &IndexDelta,
    modified_notes: Vec<Note>,
) -> IndexResult<Vec<Note>> {
    let mut all_notes = store.read_all_notes()?;

    if !delta.deleted().is_empty() {
        let deleted: HashSet<&Path> =
            delta.deleted().iter().map(FileBase::path).collect();
        all_notes.retain(|n| !deleted.contains(n.path()));
    }

    let mut index_of_path: HashMap<PathBuf, usize> = all_notes
        .iter()
        .enumerate()
        .map(|(idx, note)| (note.path().to_path_buf(), idx))
        .collect();
    for new_note in modified_notes {
        if let Some(&idx) = index_of_path.get(new_note.path())
            && let Some(target) = all_notes.get_mut(idx)
        {
            *target = new_note;
        } else {
            index_of_path
                .insert(new_note.path().to_path_buf(), all_notes.len());
            all_notes.push(new_note);
        }
    }
    all_notes.sort_by(|a, b| a.path().cmp(b.path()));
    Ok(all_notes)
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn report_counts_upserts_deletes_and_link_edges() {
        let delta = IndexDelta::compute(
            &[FileBase::new_note_test("a.md".into(), "".into())],
            &[FileBase::new_note_test("b.md".into(), "".into())],
        );
        let inlink_delta =
            InlinkDelta::compute(&InlinkMap::default(), &InlinkMap::default());
        let update = IndexUpdate {
            current_files: vec![],
            delta,
            inlinks: InlinkReconciliation::ContentOnlyPatch {
                links: InlinkMap::default(),
                modified_notes: vec![],
            },
            inlink_delta,
        };

        assert_eq!(update.report(), SyncReport::new(1, 1, 0));
    }
}
