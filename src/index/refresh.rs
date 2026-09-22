//! Incremental index refresh planning and typed update application.
//!
//! [`RefreshPass`] models the refresh lifecycle: unchanged scans can
//! materialize directly from the store, while reconciled scans must pass
//! through [`PendingApply`] before callers can receive a persisted store. A
//! [`PersistFailed`] keeps the pending pass so [`IndexerService::refresh`] can
//! still materialize a fail-open in-memory index from the same reconciliation.
//!
//! [`IndexerService::refresh`]: super::service::IndexerService::refresh

use std::{cmp::Ordering, collections::HashSet, path::Path};

use super::{
    IndexError, IndexResult, WorkspaceIndex,
    delta::{FileDelta, InlinkDelta},
    inlinks::{self, InlinkMap},
    service::IndexerService,
    sort::SortedByPath,
    store::{IndexAxes, IndexStore, PersistRequest},
};
use crate::{FileBase, Note};

/// Changed-row counts from an incremental refresh.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub struct RefreshReport {
    upserted_count: usize,
    deleted_count: usize,
    links_modified_count: usize,
}

impl RefreshReport {
    /// Creates a report from changed-row counts.
    #[inline]
    #[must_use]
    pub const fn new(
        upserted_count: usize,
        deleted_count: usize,
        links_modified_count: usize,
    ) -> Self {
        Self {
            upserted_count,
            deleted_count,
            links_modified_count,
        }
    }

    /// Files inserted or updated.
    #[inline]
    #[must_use]
    pub const fn upserted_count(self) -> usize {
        self.upserted_count
    }

    /// Files deleted from the index.
    #[inline]
    #[must_use]
    pub const fn deleted_count(self) -> usize {
        self.deleted_count
    }

    /// Inbound link edges updated.
    #[inline]
    #[must_use]
    pub const fn links_modified_count(self) -> usize {
        self.links_modified_count
    }
}

/// Result of one scanned refresh pass before optional persistence.
pub(super) enum RefreshPass {
    /// No file metadata changed; the opened store remains current.
    Unchanged {
        store: IndexStore,
        files: SortedByPath<FileBase>,
        links: InlinkMap,
    },
    /// File metadata changed and is reconciled but not yet persisted.
    Reconciled(PendingApply),
}

/// Reconciled refresh data waiting for a store write.
pub(super) struct PendingApply {
    store: IndexStore,
    update: IndexUpdate,
}

impl PendingApply {
    /// Returns the changed-row report without applying the update.
    #[inline]
    pub(super) fn report(&self) -> RefreshReport {
        self.update.report()
    }

    /// Persists this pass through [`IndexStore::persist`].
    ///
    /// The update remains owned by the returned state in both success and error
    /// cases, so a failed apply can still materialize an in-memory index from
    /// exactly the rows that failed to persist.
    pub(super) fn apply(
        self,
        axes: IndexAxes,
    ) -> Result<Persisted, PersistFailed> {
        let result = {
            let notes = self.update.notes_to_upsert();
            self.store.persist(&PersistRequest::incremental(
                axes,
                self.update.delta(),
                &notes,
                self.update.inlink_delta(),
            ))
        };
        match result {
            Ok(()) => Ok(Persisted {
                pass: self,
            }),
            Err(source) => Err(PersistFailed {
                source,
                pass: Box::new(self),
            }),
        }
    }

    /// Materializes this pass into an in-memory [`WorkspaceIndex`].
    ///
    /// # Errors
    ///
    /// - [`IndexError::Store`] if content-only materialization cannot read
    ///   persisted notes.
    pub(super) fn into_index(self) -> IndexResult<WorkspaceIndex> {
        let Self {
            store,
            update,
        } = self;
        update.into_index(&store)
    }
}

/// Successfully persisted refresh pass.
pub(super) struct Persisted {
    pass: PendingApply,
}

impl Persisted {
    /// Returns the current store after a successful apply.
    #[inline]
    pub(super) fn into_store(self) -> IndexStore {
        self.pass.store
    }

    /// Materializes the persisted pass into an in-memory [`WorkspaceIndex`].
    ///
    /// # Errors
    ///
    /// - [`IndexError::Store`] if content-only materialization cannot read
    ///   persisted notes.
    #[inline]
    pub(super) fn into_index(self) -> IndexResult<WorkspaceIndex> {
        self.pass.into_index()
    }
}

/// Failed persistence plus the pass needed for fail-open materialization.
///
/// The pass is boxed to keep this error type small: it is returned through
/// [`PendingApply::apply`]'s `Result`, and refresh call sites handle it inline.
pub(super) struct PersistFailed {
    source: IndexError,
    pass: Box<PendingApply>,
}

impl PersistFailed {
    /// Returns the persistence error while preserving the pass.
    #[inline]
    pub(super) const fn source(&self) -> &IndexError {
        &self.source
    }

    /// Consumes the failure into its persistence error.
    #[inline]
    pub(super) fn into_error(self) -> IndexError {
        self.source
    }

    /// Materializes the unpersisted pass into an in-memory [`WorkspaceIndex`].
    ///
    /// # Errors
    ///
    /// - [`IndexError::Store`] if content-only materialization cannot read
    ///   persisted notes.
    #[inline]
    pub(super) fn into_index(self) -> IndexResult<WorkspaceIndex> {
        (*self.pass).into_index()
    }
}

/// Which notes are authoritative for the reconciled inlink map.
pub(super) struct InlinkReconciliation {
    links: InlinkMap,
    notes: NoteScope,
}

/// Notes needed to persist and materialize a refresh pass.
pub(super) enum NoteScope {
    /// Only modified notes were parsed; unchanged notes still live in the
    /// store.
    Modified(Vec<Note>),
    /// The complete note set has already been rebuilt and path-sorted.
    Complete(Vec<Note>),
}

impl NoteScope {
    /// Returns note rows to upsert for this scope.
    ///
    /// `Modified` returns every parsed note because all of them came from
    /// changed files. `Complete` searches the rebuilt, path-sorted set and
    /// returns only the files identified by the file delta.
    #[must_use]
    pub(super) fn to_upsert(&self, upserted: &[FileBase]) -> Vec<&Note> {
        match self {
            Self::Modified(notes) => notes.iter().collect(),
            Self::Complete(notes) => upserted
                .iter()
                .filter_map(|file| {
                    notes
                        .binary_search_by(|note| note.path().cmp(file.path()))
                        .ok()
                        .and_then(|idx| notes.get(idx))
                })
                .collect(),
        }
    }

    /// Resolves this scope into a complete, path-sorted note set.
    ///
    /// # Errors
    ///
    /// - [`IndexError::Store`] if modified-only materialization cannot read
    ///   persisted notes.
    pub(super) fn resolve(
        self,
        store: &IndexStore,
        delta: &FileDelta,
    ) -> IndexResult<SortedByPath<Note>> {
        match self {
            Self::Modified(notes) => merge_refreshed_notes(store, delta, notes),
            Self::Complete(notes) => Ok(SortedByPath::assumed_sorted(notes)),
        }
    }
}

/// Scanned-and-diffed state of one index root, before note parsing.
pub(super) struct RefreshPlan {
    store: IndexStore,
    current_files: SortedByPath<FileBase>,
    persisted_files: SortedByPath<FileBase>,
    prev_links: InlinkMap,
    delta: FileDelta,
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
    /// - [`IndexError::Walk`] if a directory cannot be read.
    /// - [`IndexError::Inspect`] if file metadata cannot be inspected.
    /// - [`IndexError::Path`] if a walked file cannot be derived as a safe
    ///   project-relative path.
    /// - [`IndexError::Store`] if opening or reading the store fails.
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
        let current_files = SortedByPath::assumed_sorted(scanned?);
        let delta = FileDelta::compute(&current_files, &persisted_files);
        Ok(Self {
            store,
            current_files,
            persisted_files,
            prev_links,
            delta,
        })
    }

    /// Reports whether the scan produced no changes.
    #[inline]
    pub(super) fn is_empty(&self) -> bool {
        self.delta.is_empty()
    }

    /// Returns the files this pass would upsert.
    #[inline]
    pub(super) fn upserted_files(&self) -> &[FileBase] {
        self.delta.upserted()
    }

    /// Consumes the unchanged plan into a refresh pass.
    #[inline]
    pub(super) fn into_unchanged(self) -> RefreshPass {
        RefreshPass::Unchanged {
            store: self.store,
            files: self.current_files,
            links: self.prev_links,
        }
    }

    /// Reconciles reparsed notes with persisted state.
    ///
    /// # Errors
    ///
    /// - [`IndexError::Store`] if a path-set rebuild cannot read persisted
    ///   notes.
    pub(super) fn reconcile(
        self,
        modified_notes: Vec<Note>,
    ) -> IndexResult<PendingApply> {
        let (inlinks, inlink_delta) =
            if Self::is_paths_unchanged(&self.delta, &self.persisted_files) {
                let links = Self::patch_links(
                    &self.prev_links,
                    &modified_notes,
                    self.current_files.as_slice(),
                );
                let inlink_delta =
                    InlinkDelta::compute(&links, &self.prev_links);
                (
                    InlinkReconciliation {
                        links,
                        notes: NoteScope::Modified(modified_notes),
                    },
                    inlink_delta,
                )
            } else {
                let notes = merge_refreshed_notes(
                    &self.store,
                    &self.delta,
                    modified_notes,
                )?;
                let links = InlinkMap::new(
                    notes.as_slice(),
                    self.current_files.as_slice(),
                );
                let inlink_delta =
                    InlinkDelta::compute(&links, &self.prev_links);
                (
                    InlinkReconciliation {
                        links,
                        notes: NoteScope::Complete(notes.into_vec()),
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
        Ok(PendingApply {
            store: self.store,
            update,
        })
    }

    /// Reports whether `delta` changed only content at previously indexed
    /// paths.
    ///
    /// Only this case leaves link resolution for unedited notes invariant.
    fn is_paths_unchanged(
        delta: &FileDelta,
        persisted_files: &SortedByPath<FileBase>,
    ) -> bool {
        delta.deleted().is_empty()
            && delta.upserted().iter().all(|file| {
                persisted_files.binary_search_by_path(file.path()).is_ok()
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
    current_files: SortedByPath<FileBase>,
    delta: FileDelta,
    inlinks: InlinkReconciliation,
    inlink_delta: InlinkDelta,
}

impl IndexUpdate {
    /// Returns changed-row counts for this update.
    pub(super) fn report(&self) -> RefreshReport {
        RefreshReport::new(
            self.delta.upserted().len(),
            self.delta.deleted().len(),
            self.inlink_delta
                .upserted()
                .len()
                .saturating_add(self.inlink_delta.deleted().len()),
        )
    }

    #[inline]
    fn delta(&self) -> &FileDelta {
        &self.delta
    }

    #[inline]
    fn inlink_delta(&self) -> &InlinkDelta {
        &self.inlink_delta
    }

    #[inline]
    fn notes_to_upsert(&self) -> Vec<&Note> {
        self.inlinks.notes.to_upsert(self.delta.upserted())
    }

    /// Materializes this update into an in-memory [`WorkspaceIndex`].
    ///
    /// # Errors
    ///
    /// - [`IndexError::Store`] if content-only materialization cannot read
    ///   persisted notes.
    pub(super) fn into_index(
        self,
        store: &IndexStore,
    ) -> IndexResult<WorkspaceIndex> {
        let InlinkReconciliation {
            links,
            notes,
        } = self.inlinks;
        let notes = notes.resolve(store, &self.delta)?;
        Ok(WorkspaceIndex::assemble(self.current_files, notes, links))
    }
}

/// Merges persisted notes with deletions and reparsed notes for a full
/// recompute.
///
/// Consumes `modified_notes`, moving rather than cloning each note.
///
/// Bulk-reads all persisted notes because this fallback needs the full note
/// set. Deleted paths are filtered first, then one merge pass over the two
/// path-ascending inputs folds modified notes in place of persisted rows, so
/// the result is born sorted.
///
/// # Errors
///
/// - [`IndexError::Store`] if persisted notes cannot be read or decoded.
fn merge_refreshed_notes(
    store: &IndexStore,
    delta: &FileDelta,
    modified_notes: Vec<Note>,
) -> IndexResult<SortedByPath<Note>> {
    let mut all_notes = store.read_all_notes()?.into_vec();

    if !delta.deleted().is_empty() {
        let deleted: HashSet<&Path> =
            delta.deleted().iter().map(FileBase::path).collect();
        all_notes.retain(|n| !deleted.contains(n.path()));
    }

    // Both inputs are path-ascending: `read_all_notes` returns sorted rows and
    // `parse_notes` preserves the sorted upsert order, so one merge pass
    // produces the union with modified notes replacing persisted rows.
    let mut merged = Vec::with_capacity(
        all_notes.len().saturating_add(modified_notes.len()),
    );
    let mut persisted = all_notes.into_iter().peekable();
    let mut modified = modified_notes.into_iter().peekable();
    loop {
        match (persisted.peek(), modified.peek()) {
            (None, None) => break,
            (Some(_), None) => merged.extend(persisted.by_ref()),
            (None, Some(_)) => merged.extend(modified.by_ref()),
            (Some(old), Some(new)) => match old.path().cmp(new.path()) {
                Ordering::Less => merged.extend(persisted.by_ref().take(1)),
                Ordering::Greater => merged.extend(modified.by_ref().take(1)),
                Ordering::Equal => {
                    persisted.next();
                    merged.extend(modified.by_ref().take(1));
                }
            },
        }
    }
    Ok(SortedByPath::assumed_sorted(merged))
}

#[cfg(test)]
mod tests {
    use super::*;

    mod report {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn counts_upserts_deletes_and_link_edges() {
            let delta = FileDelta::compute(
                &SortedByPath::sorted(vec![FileBase::note_for_test("a.md")]),
                &SortedByPath::sorted(vec![FileBase::note_for_test("b.md")]),
            );
            let inlink_delta = InlinkDelta::compute(
                &InlinkMap::default(),
                &InlinkMap::default(),
            );
            let update = IndexUpdate {
                current_files: SortedByPath::default(),
                delta,
                inlinks: InlinkReconciliation {
                    links: InlinkMap::default(),
                    notes: NoteScope::Modified(vec![]),
                },
                inlink_delta,
            };

            assert_eq!(update.report(), RefreshReport::new(1, 1, 0));
        }
    }

    mod note_scope {
        use std::path::Path;

        use pretty_assertions::assert_eq;

        use super::*;
        use crate::parse_note;

        #[test]
        fn returns_all_modified_notes_for_upsert() {
            let notes =
                vec![parse_note("a.md", "# A"), parse_note("b.md", "# B")];
            let scope = NoteScope::Modified(notes);
            let upserted = [FileBase::note_for_test("a.md")];

            let paths: Vec<_> = scope
                .to_upsert(&upserted)
                .into_iter()
                .map(Note::path)
                .collect();

            assert_eq!(paths, [Path::new("a.md"), Path::new("b.md")]);
        }

        #[test]
        fn returns_complete_notes_matching_upserted_paths() {
            let notes =
                vec![parse_note("a.md", "# A"), parse_note("c.md", "# C")];
            let scope = NoteScope::Complete(notes);
            let upserted = [
                FileBase::note_for_test("a.md"),
                FileBase::note_for_test("b.md"),
            ];

            let paths: Vec<_> = scope
                .to_upsert(&upserted)
                .into_iter()
                .map(Note::path)
                .collect();

            assert_eq!(paths, [Path::new("a.md")]);
        }

        #[test]
        fn returns_no_complete_notes_when_upserted_paths_miss() {
            let notes = vec![parse_note("a.md", "# A")];
            let scope = NoteScope::Complete(notes);
            let upserted = [FileBase::note_for_test("b.md")];

            assert_eq!(scope.to_upsert(&upserted), Vec::<&Note>::new());
        }
    }
}
