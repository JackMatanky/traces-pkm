//! Refresh-time cache of the previously persisted index state.
//!
//! [`RefreshCache`] loads the previous scan and inbound-link map through a
//! caller-supplied read transaction, then diffs a fresh scan against them
//! (delegating the two-pointer merge algorithms to [`super::delta`]) and
//! resolves each record's [`crate::note::Note`] via point lookup or
//! backdated reparse. Used exclusively by
//! [`super::builder::IndexBuilder::build_with_cache`].

use std::path::{Path, PathBuf};

use super::{
    builder::parse_note,
    delta,
    error::{IndexBuilderError, IndexError, IndexResult},
    inlinks::InlinkMap,
};
use crate::file::FileBase;

/// Whether a record's previously-persisted Note is still valid, decided by
/// [`RefreshCache::diff_files`]'s merge-join. Replaces a bare `is_upserted:
/// bool` so [`RefreshCache::reconcile_note`] reads as a decision, not a flag.
///
/// [`RefreshCache::diff_files`]: RefreshCache::diff_files
/// [`RefreshCache::reconcile_note`]: RefreshCache::reconcile_note
#[derive(Clone, Copy, Debug)]
pub(super) enum NoteCacheState {
    /// New or metadata-changed since the last persist; the cached Note (if any)
    /// is outdated; reparse from disk and backdate against its outlinks.
    Upserted,
    /// Unchanged since the last persist; the cached Note can be reused via
    /// point lookup.
    Fresh,
}

/// State carried across [`super::builder::IndexBuilder::build_with_cache`],
/// borrowed from the caller's own
/// [`super::store::IndexStore`]/`ReadTransaction` rather than owned. Owning
/// them here would force [`super::IndexerService::refresh`] to reopen the store
/// a second time to persist afterward.
///
/// `txn` stays open for the entire call, every `parse_note` disk read and the
/// full merge-join, not just the [`super::store::IndexStore::read_note`] point
/// lookups it backs. Per redb's own docs, "read-only transactions may exist
/// concurrently with writes", so this never blocks a concurrent writer; it does
/// pin the transaction's MVCC snapshot for the duration.
/// [`super::IndexerService::refresh`] scopes this transaction's lifetime to end
/// before it opens its own write transaction to persist, so the two never
/// overlap.
pub(super) struct RefreshCache<'a> {
    files: Vec<FileBase>,
    inlinks: InlinkMap,
    store: &'a super::store::IndexStore,
    txn: &'a redb::ReadTransaction,
}

impl<'a> RefreshCache<'a> {
    /// Loads `files`/`inlinks` via `store` through `txn`, the only way to
    /// construct a `RefreshCache`; fields stay private.
    ///
    /// # Errors
    ///
    /// - [`IndexError::Store`] if the previously persisted `FILES`/`LINKS`
    ///   tables cannot be read.
    pub(super) fn load(
        store: &'a super::store::IndexStore,
        txn: &'a redb::ReadTransaction,
    ) -> IndexResult<Self> {
        let (files, inlinks) = store.read_files_and_links_via(txn)?;
        Ok(Self {
            files,
            inlinks,
            store,
            txn,
        })
    }

    /// Diffs `current` against the previous scan via [`delta::diff_files`].
    /// See that function for the merge algorithm and the
    /// `has_deleted_note` return value's semantics.
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
        &self,
        file: &FileBase,
        state: NoteCacheState,
        root: &Path,
    ) -> Result<(crate::note::Note, bool), IndexBuilderError> {
        match state {
            NoteCacheState::Upserted => self.reparse_and_backdate(file, root),
            NoteCacheState::Fresh => {
                self.recall_unchanged_note(file).map(|note| (note, false))
            }
        }
    }

    /// Recalls an unchanged record's previously-persisted Note via point
    /// lookup, the [`NoteCacheState::Fresh`] branch of
    /// [`Self::reconcile_note`].
    ///
    /// # Errors
    ///
    /// - [`IndexBuilderError::NoteLookup`] if the previously-persisted Note
    ///   cannot be read.
    /// - [`IndexBuilderError::MissingNote`] if no Note was persisted at
    ///   `file`'s path (logic-bug guard).
    ///
    /// [`Self::reconcile_note`]: RefreshCache::reconcile_note
    fn recall_unchanged_note(
        &self,
        file: &FileBase,
    ) -> Result<crate::note::Note, IndexBuilderError> {
        self.store
            .read_note(self.txn, file.path())
            .map_err(|source| IndexBuilderError::NoteLookup {
                path: file.path().to_path_buf(),
                source: Box::new(source),
            })?
            .ok_or_else(|| IndexBuilderError::MissingNote {
                path: file.path().to_path_buf(),
            })
    }

    /// Reparses an upserted record from disk and backdates it against its
    /// previously-persisted Note, the [`NoteCacheState::Upserted`] branch of
    /// [`Self::reconcile_note`].
    ///
    /// # Errors
    ///
    /// - [`IndexBuilderError::NoteParse`] if `file`'s markdown file cannot be
    ///   read.
    ///
    /// [`Self::reconcile_note`]: RefreshCache::reconcile_note
    fn reparse_and_backdate(
        &self,
        file: &FileBase,
        root: &Path,
    ) -> Result<(crate::note::Note, bool), IndexBuilderError> {
        let note = parse_note(root, file)?;
        let outlinks_changed = match self.store.read_note(self.txn, file.path())
        {
            Ok(Some(previous)) => {
                outlink_targets(&note) != outlink_targets(&previous)
            }
            Ok(None) => true,
            Err(source) => {
                log_backdating_lookup_failure(file.path(), &source);
                true
            }
        };
        Ok((note, outlinks_changed))
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

/// Logs a backdating point-lookup failure at debug level. Extracted (and marked
/// cold/never-inline) so [`RefreshCache::reconcile_note`]'s hot path doesn't
/// pay for `tracing`'s format-argument machinery in its own stack frame; this
/// error path is rare (a corrupted or undeserializable stored row) and never
/// propagated as a hard error.
#[cold]
#[inline(never)]
fn log_backdating_lookup_failure(path: &Path, source: &IndexError) {
    tracing::debug!(
        path = %path.display(),
        %source,
        "failed to load previous note for backdating; assuming outlinks changed"
    );
}

/// Deduplicated, sorted outlink targets for backdating comparison. Order-and
/// duplicate-insensitive, matching `derive_inlinks`'s own "duplicate outlinks
/// to the same target within one Note ... collapse to a single edge" behavior.
/// `Link::text` (display text) is deliberately excluded: comparing it would
/// force a recompute on the common case of a user renaming a wikilink's display
/// text without moving its target.
fn outlink_targets(note: &crate::note::Note) -> Vec<&str> {
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
    fn load_cache<'a>(
        store: &'a IndexStore,
        txn: &'a redb::ReadTransaction,
    ) -> RefreshCache<'a> {
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
            let note = crate::note::parse_markdown("a.md", "content");
            let store = IndexStore::open(temp.path()).expect("open store");
            store
                .write_all(&files, &[note], &InlinkMap::new())
                .expect("persist");
            let txn = store.begin_read().expect("begin read");
            let cache = load_cache(&store, &txn);
            let file = files.first().expect("one record");

            let (_, outlinks_changed) = cache
                .reconcile_note(file, NoteCacheState::Fresh, temp.path())
                .expect("reconcile succeeds");

            assert!(!outlinks_changed);
        }

        #[test]
        fn reparses_and_has_nothing_to_backdate_against_when_upserted_and_new()
        {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("a.md"), "content").expect("write note");
            let files = IndexerService::new(temp.path()).scan().expect("scan");
            let store = IndexStore::open(temp.path()).expect("open store");
            let txn = store.begin_read().expect("begin read");
            let cache = load_cache(&store, &txn);
            let file = files.first().expect("one record");

            let (note, outlinks_changed) = cache
                .reconcile_note(file, NoteCacheState::Upserted, temp.path())
                .expect("reconcile succeeds");

            assert_eq!(note.path(), Path::new("a.md"));
            assert!(outlinks_changed);
        }
    }
}
