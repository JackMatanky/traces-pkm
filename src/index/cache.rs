//! Refresh-time cache of the previously persisted index state.
//!
//! [`RefreshCache`] loads the previous scan and inbound-link map through a
//! caller-supplied read transaction, then diffs a fresh scan against them
//! (two-pointer merges over path-sorted state) and resolves each record's
//! [`crate::note::Note`] via point lookup or backdated reparse. Used
//! exclusively by [`super::builder::IndexBuilder::build_with_cache`].

use std::path::{Path, PathBuf};

use super::{
    FileFormat,
    builder::parse_note,
    error::{IndexBuilderError, IndexError},
    inlinks::InlinkMap,
};
use crate::file::FileBase;

/// Whether a record's previously-persisted Note is still valid, decided by
/// [`RefreshCache::diff_files`]'s merge-join — replaces a bare
/// `is_upserted: bool` so [`RefreshCache::reconcile_note`] reads as a
/// decision, not a flag.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum NoteCacheState {
    /// New or metadata-changed since the last persist — the cached Note (if
    /// any) is outdated; reparse from disk and backdate against its
    /// outlinks.
    Upserted,
    /// Unchanged since the last persist — the cached Note can be reused via
    /// point lookup.
    Fresh,
}

/// State carried across [`super::builder::IndexBuilder::build_with_cache`],
/// borrowed from the caller's own [`super::store::IndexStore`]/
/// `ReadTransaction` rather than owned — owning them here would force
/// [`super::IndexerService::refresh`] to reopen the store a second time to
/// persist afterward.
///
/// `txn` stays open for the entire call — every `parse_note` disk read and
/// the full merge-join, not just the [`super::store::IndexStore::read_note`]
/// point lookups it backs. Per redb's own docs, "read-only transactions may
/// exist concurrently with writes", so this never blocks a concurrent
/// writer; it does pin the transaction's MVCC snapshot for the duration.
/// [`super::IndexerService::refresh`] scopes this transaction's lifetime to
/// end before it opens its own write transaction to persist, so the two
/// never overlap.
pub(super) struct RefreshCache<'a> {
    files: Vec<FileBase>,
    inlinks: InlinkMap,
    store: &'a super::store::IndexStore,
    txn: &'a redb::ReadTransaction,
}

impl<'a> RefreshCache<'a> {
    /// Loads `files`/`inlinks` via `store` through `txn` — the only way to
    /// construct a `RefreshCache`; fields stay private.
    ///
    /// # Errors
    ///
    /// - [`IndexError::Store`] if the previously persisted `FILES`/`LINKS`
    ///   tables cannot be read.
    pub(super) fn load(
        store: &'a super::store::IndexStore,
        txn: &'a redb::ReadTransaction,
    ) -> Result<Self, IndexError> {
        let (files, inlinks) = store.read_files_and_links_via(txn)?;
        Ok(Self {
            files,
            inlinks,
            store,
            txn,
        })
    }

    /// Diffs two path-sorted `FileBase` slices via a two-pointer merge,
    /// returning current-side paths that are new or changed (`upserted`),
    /// previous-side paths absent from `current` (`deleted`), and whether
    /// any deleted entry was a Note (the trailing `bool`) — a deleted Note
    /// always forces an inlink recompute; an upserted Note's contribution to
    /// staleness depends on its outlinks, which needs Note content this
    /// function structurally doesn't have, so it is deliberately not folded
    /// in here (see [`Self::reconcile_note`]'s backdating).
    pub(super) fn diff_files(
        &self,
        current: &[FileBase],
    ) -> (Vec<PathBuf>, Vec<PathBuf>, bool) {
        let mut upserted = Vec::new();
        let mut deleted = Vec::new();
        let mut has_deleted_note = false;
        let mut cur = current.iter().peekable();
        let mut prev = self.files.iter().peekable();
        loop {
            match (cur.peek(), prev.peek()) {
                (Some(c), Some(p)) => match c.path().cmp(p.path()) {
                    std::cmp::Ordering::Less => {
                        upserted.push(c.path().to_path_buf());
                        cur.next();
                    }
                    std::cmp::Ordering::Greater => {
                        if p.format() == FileFormat::Note {
                            has_deleted_note = true;
                        }
                        deleted.push(p.path().to_path_buf());
                        prev.next();
                    }
                    std::cmp::Ordering::Equal => {
                        if *c != *p {
                            upserted.push(c.path().to_path_buf());
                        }
                        cur.next();
                        prev.next();
                    }
                },
                (Some(c), None) => {
                    upserted.push(c.path().to_path_buf());
                    cur.next();
                }
                (None, Some(p)) => {
                    if p.format() == FileFormat::Note {
                        has_deleted_note = true;
                    }
                    deleted.push(p.path().to_path_buf());
                    prev.next();
                }
                (None, None) => break,
            }
        }
        (upserted, deleted, has_deleted_note)
    }

    /// Reuses `file`'s Note via point lookup when
    /// [`NoteCacheState::Fresh`];
    /// otherwise reparses from disk and backdates by comparing the reparsed
    /// Note's outlink targets against the previously persisted Note's (if
    /// any) — see the module-level backdating note on
    /// [`super::builder::IndexBuilder::build_with_cache`]. Returns the
    /// resolved Note and whether its outlinks actually changed (forcing an
    /// inlink recompute).
    ///
    /// Backdating's point lookup fails open: if it errors for any reason
    /// other than "no previous Note at this path" (a corrupted or
    /// undeserializable stored row), it is logged via `tracing::debug!` and
    /// treated as "outlinks changed" — never propagated as a hard error.
    /// Backdating is a pure optimization layered on top of the reparsed
    /// Note's own already-successful parse; its failure must never fail
    /// `refresh()`.
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
    /// lookup — the [`NoteCacheState::Fresh`] branch of
    /// [`Self::reconcile_note`].
    ///
    /// # Errors
    ///
    /// - [`IndexBuilderError::NoteLookup`] if the previously-persisted Note
    ///   cannot be read.
    /// - [`IndexBuilderError::MissingNote`] if no Note was persisted at
    ///   `file`'s path (logic-bug guard).
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
    /// previously-persisted Note — the [`NoteCacheState::Upserted`] branch of
    /// [`Self::reconcile_note`].
    ///
    /// # Errors
    ///
    /// - [`IndexBuilderError::NoteParse`] if `file`'s markdown file cannot be
    ///   read.
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

    /// Diffs two target-keyed inlink maps by source-set membership (order
    /// independent — [`derive_inlinks`](super::inlinks::derive_inlinks)'s
    /// output and a redb-loaded map are not guaranteed to list one target's
    /// sources in the same order even when the set is identical). Returns
    /// target paths whose source set is new or changed (`upserted`) and
    /// target paths removed entirely (`deleted`).
    pub(super) fn diff_links(
        &self,
        current: &InlinkMap,
    ) -> (Vec<PathBuf>, Vec<PathBuf>) {
        let mut upserted = Vec::new();
        for (target, sources) in current {
            let changed = match self.inlinks.get(target) {
                Some(prev_sources) => {
                    let mut prev_sorted: Vec<_> = prev_sources.iter().collect();
                    let mut cur_sorted: Vec<_> = sources.iter().collect();
                    prev_sorted.sort_unstable();
                    cur_sorted.sort_unstable();
                    prev_sorted != cur_sorted
                }
                None => true,
            };
            if changed {
                upserted.push(target.clone());
            }
        }
        let deleted = self
            .inlinks
            .keys()
            .filter(|target| !current.contains_key(*target))
            .cloned()
            .collect();
        (upserted, deleted)
    }

    /// Consumed when nothing was stale: hands back the previous inlink map
    /// unchanged.
    pub(super) fn into_inlinks(self) -> InlinkMap {
        self.inlinks
    }
}

/// Logs a backdating point-lookup failure at debug level. Extracted (and
/// marked cold/never-inline) so [`RefreshCache::reconcile_note`]'s hot path
/// doesn't pay for `tracing`'s format-argument machinery in its own stack
/// frame — this error path is rare (a corrupted or undeserializable stored
/// row) and never propagated as a hard error.
#[cold]
#[inline(never)]
fn log_backdating_lookup_failure(path: &Path, source: &IndexError) {
    tracing::debug!(
        path = %path.display(),
        %source,
        "failed to load previous note for backdating; assuming outlinks changed"
    );
}

/// Deduplicated, sorted outlink targets for backdating comparison — order-
/// and duplicate-insensitive, matching `derive_inlinks`'s own "duplicate
/// outlinks to the same target within one Note ... collapse to a single
/// edge" behavior. `Link::text` (display text) is deliberately excluded:
/// comparing it would force a recompute on the common case of a user
/// renaming a wikilink's display text without moving its target.
fn outlink_targets(note: &crate::note::Note) -> Vec<&str> {
    let mut targets: Vec<&str> =
        note.outlinks().iter().map(crate::note::Link::target).collect();
    targets.sort_unstable();
    targets.dedup();
    targets
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, fs};

    use super::*;
    use crate::index::{IndexerService, store::IndexStore};

    /// Persists `files`/`inlinks` and loads a [`RefreshCache`] against them
    /// — the only way to construct one outside
    /// [`super::super::builder::IndexBuilder::build_with_cache`].
    fn load_cache<'a>(
        store: &'a IndexStore,
        txn: &'a redb::ReadTransaction,
    ) -> RefreshCache<'a> {
        RefreshCache::load(store, txn).expect("load cache")
    }

    mod diff_files {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn deleted_note_sets_has_deleted_note() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("a.md"), "content").expect("write a");
            let previous =
                IndexerService::new(temp.path()).scan().expect("scan");
            let store = IndexStore::open(temp.path()).expect("open store");
            store
                .replace_all(&previous, &[], &InlinkMap::new())
                .expect("persist previous");
            fs::remove_file(temp.path().join("a.md")).expect("delete a");
            let current =
                IndexerService::new(temp.path()).scan().expect("scan");

            let txn = store.begin_read().expect("begin read");
            let cache = load_cache(&store, &txn);
            let (_, deleted, has_deleted_note) = cache.diff_files(&current);

            assert_eq!(deleted, [PathBuf::from("a.md")]);
            assert!(has_deleted_note);
        }

        #[test]
        fn deleted_non_note_file_does_not_set_has_deleted_note() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("image.png"), "fake")
                .expect("write image");
            let previous =
                IndexerService::new(temp.path()).scan().expect("scan");
            let store = IndexStore::open(temp.path()).expect("open store");
            store
                .replace_all(&previous, &[], &InlinkMap::new())
                .expect("persist previous");
            fs::remove_file(temp.path().join("image.png"))
                .expect("delete image");
            let current =
                IndexerService::new(temp.path()).scan().expect("scan");

            let txn = store.begin_read().expect("begin read");
            let cache = load_cache(&store, &txn);
            let (_, deleted, has_deleted_note) = cache.diff_files(&current);

            assert_eq!(deleted, [PathBuf::from("image.png")]);
            assert!(!has_deleted_note);
        }

        #[test]
        fn upserted_note_does_not_set_has_deleted_note() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("a.md"), "v1").expect("write a");
            let previous =
                IndexerService::new(temp.path()).scan().expect("scan");
            let store = IndexStore::open(temp.path()).expect("open store");
            store
                .replace_all(&previous, &[], &InlinkMap::new())
                .expect("persist previous");
            fs::write(temp.path().join("a.md"), "v2, longer content")
                .expect("rewrite a");
            let current =
                IndexerService::new(temp.path()).scan().expect("scan");

            let txn = store.begin_read().expect("begin read");
            let cache = load_cache(&store, &txn);
            let (upserted, deleted, has_deleted_note) =
                cache.diff_files(&current);

            assert_eq!(upserted, [PathBuf::from("a.md")]);
            assert!(deleted.is_empty());
            assert!(
                !has_deleted_note,
                "upserted-Note staleness is decided elsewhere (backdating), \
                 not by diff_files"
            );
        }
    }

    mod diff_links {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn detects_new_and_removed_targets() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let previous: InlinkMap = HashMap::from([(
                PathBuf::from("a.md"),
                vec![PathBuf::from("x.md")],
            )]);
            let store = IndexStore::open(temp.path()).expect("open store");
            store
                .replace_all(&[], &[], &previous)
                .expect("persist previous inlinks");

            let txn = store.begin_read().expect("begin read");
            let cache = load_cache(&store, &txn);
            let current: InlinkMap = HashMap::from([(
                PathBuf::from("b.md"),
                vec![PathBuf::from("x.md")],
            )]);
            let (upserted, deleted) = cache.diff_links(&current);

            assert_eq!(upserted, [PathBuf::from("b.md")]);
            assert_eq!(deleted, [PathBuf::from("a.md")]);
        }

        #[test]
        fn ignores_source_order_differences() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let previous: InlinkMap =
                HashMap::from([(PathBuf::from("a.md"), vec![
                    PathBuf::from("x.md"),
                    PathBuf::from("y.md"),
                ])]);
            let store = IndexStore::open(temp.path()).expect("open store");
            store
                .replace_all(&[], &[], &previous)
                .expect("persist previous inlinks");

            let txn = store.begin_read().expect("begin read");
            let cache = load_cache(&store, &txn);
            let current: InlinkMap =
                HashMap::from([(PathBuf::from("a.md"), vec![
                    PathBuf::from("y.md"),
                    PathBuf::from("x.md"),
                ])]);
            let (upserted, deleted) = cache.diff_links(&current);

            assert!(upserted.is_empty());
            assert!(deleted.is_empty());
        }
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
                .replace_all(&files, &[note], &InlinkMap::new())
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
