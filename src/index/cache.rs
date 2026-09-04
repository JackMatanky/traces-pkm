//! Refresh-time cache of the previously persisted index state.
//!
//! [`RefreshCache`] loads the previous scan and inbound-link map through a
//! caller-supplied read transaction, then diffs a fresh scan against them
//! (delegating the two-pointer merge algorithms to [`super::delta`]) and
//! resolves each record's [`crate::Note`] via point lookup or backdated
//! reparse. Used exclusively by
//! [`super::builder::IndexBuilder::build_with_cache`].

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use rayon::prelude::*;

use super::{
    builder::parse_note,
    delta,
    error::{IndexBuilderError, IndexResult},
    inlinks::InlinkMap,
    store::{IndexStore, NOTES},
};
use crate::{
    FileBase, Note,
    config::{FrontmatterConfig, TaskConfig},
};

/// Whether a record's previously-persisted Note is still valid, decided by
/// [`RefreshCache::diff_files`]'s merge-join. Replaces a bare `is_upserted:
/// bool` so [`RefreshCache::reconcile_note`] reads as a decision, not a flag.
///
/// [`RefreshCache::diff_files`]: RefreshCache::diff_files
/// [`RefreshCache::reconcile_note`]: RefreshCache::reconcile_note
#[derive(Copy, Clone, Debug)]
pub(super) enum NoteCacheState {
    /// New or metadata-changed since the last persist; the cached Note (if any)
    /// is outdated; reparse from disk and backdate against its outlinks.
    Upserted,
    /// Unchanged since the last persist; the cached Note can be reused via
    /// point lookup.
    Fresh,
}

/// State carried across [`super::builder::IndexBuilder::build_with_cache`],
/// borrowed from the caller's [`super::store::IndexStore`] and read transaction
/// to avoid reopening the store for persistence afterward.
///
/// The transaction stays open for the entire refresh: disk reads, merge-join,
/// and point lookups. Per redb's docs, read-only transactions may exist
/// concurrently with writes, so this never blocks a writer; it does pin the
/// MVCC snapshot for the duration. [`super::IndexerService::refresh`] scopes
/// the transaction to end before persisting, so the two never overlap.
pub(super) struct RefreshCache {
    files: Vec<FileBase>,
    inlinks: InlinkMap,
    notes: HashMap<PathBuf, Note>,
}

impl RefreshCache {
    /// Loads `files`/`inlinks` via `store` through `txn`, the only way to
    /// construct a `RefreshCache`; fields stay private.
    ///
    /// # Errors
    ///
    /// - [`IndexError::Store`] if the previously persisted `FILES`/`LINKS`
    ///   tables cannot be read.
    pub(super) fn load(
        store: &IndexStore,
        txn: &redb::ReadTransaction,
    ) -> IndexResult<Self> {
        let (files, inlinks) = store.read_files_and_links_via(txn)?;
        let table = match txn.open_table(NOTES) {
            Ok(table) => Some(table),
            Err(redb::TableError::TableDoesNotExist(_)) => None,
            Err(source) => return Err(store.raise_source_error(source).into()),
        };
        let chunks = if let Some(table) = &table {
            store.read_notes_chunked(table)?
        } else {
            Vec::new()
        };
        let notes = decode_notes_parallel(chunks);
        Ok(Self {
            files,
            inlinks,
            notes,
        })
    }

    /// Diffs `current` against the previous scan via [`delta::diff_files`].
    /// See that function for the merge algorithm and the `has_deleted_note`
    /// return value's semantics.
    pub(super) fn diff_files(
        &self,
        current: &[FileBase],
    ) -> (Vec<PathBuf>, Vec<PathBuf>, bool) {
        delta::diff_files(current, &self.files)
    }

    /// Reuses `file`'s Note via point lookup when [`NoteCacheState::Fresh`];
    /// otherwise reparses from disk and backdates by comparing the reparsed
    /// Note's outlink targets against the previously persisted Note's (if any),
    /// see the module-level backdating note on
    /// [`IndexBuilder::build_with_cache`]. Returns the resolved Note and
    /// whether its outlinks actually changed (forcing an inlink recompute).
    ///
    /// Backdating's point lookup fails open: if it errors for any reason other
    /// than "no previous Note at this path" (a corrupted or undeserializable
    /// stored row), it is logged via `tracing::debug!` and treated as "outlinks
    /// changed", never propagated as a hard error. Backdating is a pure
    /// optimization layered on top of the reparsed Note's own
    /// already-successful parse; its failure must never fail `refresh()`.
    ///
    /// # Errors
    ///
    /// - [`IndexBuilderError::NoteParse`] if `state` is
    ///   [`NoteCacheState::Upserted`] and `file`'s markdown cannot be read.
    /// - [`IndexBuilderError::NoteLookup`] if `state` is
    ///   [`NoteCacheState::Fresh`] but its previously-persisted Note cannot be
    ///   read.
    /// - [`IndexBuilderError::MissingNote`] if `state` is
    ///   [`NoteCacheState::Fresh`] but no Note was persisted at its path
    ///   (logic-bug guard).
    ///
    /// [`NoteCacheState::Fresh`]: NoteCacheState::Fresh
    /// [`Self::reconcile_note`]: RefreshCache::reconcile_note
    /// [`IndexBuilder::build_with_cache`]: super::builder::IndexBuilder::build_with_cache
    pub(super) fn reconcile_note(
        &mut self,
        file: &FileBase,
        state: NoteCacheState,
        root: &Path,
        (tasks, frontmatter): (&TaskConfig, &FrontmatterConfig),
    ) -> Result<(Note, bool), IndexBuilderError> {
        match state {
            NoteCacheState::Upserted => {
                let note = parse_note(root, file, tasks, frontmatter)?;
                let outlinks_changed = match self.notes.get(file.path()) {
                    Some(previous) => {
                        outlink_targets(&note) != outlink_targets(previous)
                    }
                    None => true,
                };
                Ok((note, outlinks_changed))
            }
            NoteCacheState::Fresh => {
                let note = self.notes.remove(file.path()).ok_or_else(|| {
                    IndexBuilderError::MissingNote {
                        path: file.path().to_path_buf(),
                    }
                })?;
                Ok((note, false))
            }
        }
    }

    /// Diffs the previous inlink map against a freshly recomputed one via
    /// [`delta::diff_inlinks`].
    pub(super) fn diff_links(
        &self,
        current: &InlinkMap,
    ) -> (Vec<PathBuf>, Vec<PathBuf>) {
        delta::diff_inlinks(&self.inlinks, current)
    }

    /// Consumed when nothing was stale: hands back the previous inlink map
    /// unchanged.
    pub(super) fn into_inlinks(self) -> InlinkMap {
        self.inlinks
    }
}

fn decode_notes_parallel(
    chunks: Vec<super::store::ChunkBuffer<'_>>,
) -> HashMap<PathBuf, Note> {
    let decoded: Vec<(PathBuf, Note)> = chunks
        .into_par_iter()
        .flat_map(|chunk| {
            chunk
                .into_par_iter()
                .filter_map(|(path, guard)| match postcard::from_bytes(guard.value()) {
                    Ok(note) => Some((path, note)),
                    Err(source) => {
                        tracing::debug!(
                            path = %path.display(),
                            %source,
                            "failed to deserialize cached note; treating as missing"
                        );
                        None
                    }
                })
        })
        .collect();
    let mut notes = HashMap::with_capacity(decoded.len());
    for (path, note) in decoded {
        notes.insert(path, note);
    }
    notes
}

/// Deduplicated, sorted outlink targets for backdating comparison. Order-and
/// duplicate-insensitive, matching `derive_inlinks`'s own "duplicate outlinks
/// to the same target within one Note ... collapse to a single edge" behavior.
/// `Link::text` (display text) is deliberately excluded: comparing it would
/// force a recompute on the common case of a user renaming a wikilink's display
/// text without moving its target.
fn outlink_targets(note: &crate::Note) -> Vec<&str> {
    let mut targets: Vec<&str> =
        note.outlinks().iter().map(crate::note::Link::target).collect();
    targets.sort_unstable();
    targets.dedup();
    targets
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::index::{IndexerService, store::IndexStore};

    /// Persists `files`/`inlinks` and loads a [`RefreshCache`] against them,
    /// the only way to construct one outside
    /// [`super::super::builder::IndexBuilder::build_with_cache`].
    fn load_cache(
        store: &IndexStore,
        txn: &redb::ReadTransaction,
    ) -> RefreshCache {
        RefreshCache::load(store, txn).expect("load cache")
    }

    mod reconcile_note {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn reuses_via_point_lookup_when_fresh() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("a.md"), "content").expect("write note");
            let files = IndexerService::new(temp.path()).scan().expect("scan");
            let input = crate::note::MarkdownParserInput::for_test(
                Path::new("a.md"),
                "content",
            );
            let note = crate::note::parse_markdown(&input);
            let store = IndexStore::open(temp.path()).expect("open store");
            let entries = crate::index::entry::assemble_entries(
                files.clone(),
                vec![note],
                InlinkMap::new(),
            );
            store.write_all(&entries).expect("persist");
            let file = files.first().expect("file");
            let txn = store.begin_read().expect("begin read");
            let mut cache = load_cache(&store, &txn);
            let tasks = crate::TaskConfig::default();
            let frontmatter = crate::config::FrontmatterConfig::default();
            let (_, outlinks_changed) = cache
                .reconcile_note(
                    file,
                    NoteCacheState::Fresh,
                    temp.path(),
                    (&tasks, &frontmatter),
                )
                .expect("reconcile succeeds");

            assert!(!outlinks_changed);
        }

        #[test]
        fn reparses_and_has_nothing_to_backdate_against_when_upserted_and_new()
        {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("a.md"), "content").expect("write note");
            let files = IndexerService::new(temp.path()).scan().expect("scan");
            let file = files.first().expect("file");
            let store = IndexStore::open(temp.path()).expect("open store");
            let txn = store.begin_read().expect("begin read");
            let mut cache = load_cache(&store, &txn);

            let tasks = crate::TaskConfig::default();
            let frontmatter = crate::config::FrontmatterConfig::default();
            let (note, outlinks_changed) = cache
                .reconcile_note(
                    file,
                    NoteCacheState::Upserted,
                    temp.path(),
                    (&tasks, &frontmatter),
                )
                .expect("reconcile succeeds");

            assert_eq!(note.path(), Path::new("a.md"));
            assert!(outlinks_changed);
        }
    }
}
