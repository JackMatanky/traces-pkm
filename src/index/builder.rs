//! Internal build pipeline for [`super::FileIndex`].
//!
//! [`IndexBuilder`] is a **plan**: it holds the scan result and a reuse
//! directive, deferring all note parsing, sorting, and inlink derivation to
//! [`IndexBuilder::build`]. Callers use [`super::IndexerService::build`] and
//! [`super::IndexerService::refresh`] instead of this type directly.

use std::path::{Path, PathBuf};

use super::{
    FileFormat,
    error::{IndexBuilderError, IndexError},
    inlinks::{InlinkMap, derive_inlinks},
    scan,
};
use crate::{file::FileBase, note::parse_markdown};

/// Per-path persistence plan produced by [`IndexBuilder::build`].
///
/// [`super::store::IndexStore::persist_index`] reads this to choose between a
/// full [`super::store::IndexStore::replace_all`] rewrite (fresh build — no
/// previous persisted state to diff against) and a row-level incremental write
/// (refresh — only paths that actually changed since the last persist).
///
/// [`Incremental`]'s payload is boxed: `IndexDelta` is a field of
/// [`super::FileIndex`], and every [`Full`] build (the common case for a
/// first-time index) would otherwise pay for the largest variant's four
/// inline `Vec`s regardless of which variant is active. Boxing shrinks
/// `IndexDelta` from 96 bytes to 8.
///
/// [`Incremental`]: IndexDelta::Incremental
/// [`Full`]: IndexDelta::Full
///
/// `Full` and `Incremental` are not interchangeable, even when an
/// `Incremental` diff would come out empty: `Full` (`replace_all`)
/// unconditionally wipes all three tables before rewriting, so it never
/// needs to know what was deleted. `Incremental` (`persist_incremental`)
/// only deletes paths its diff explicitly names, which is only correct
/// because that diff is always computed against a `RefreshCache` loaded
/// from the real, currently-persisted store (via `RefreshCache::load`,
/// the only constructor — private fields make this a type-level
/// guarantee). A `Full`-built `FileIndex` retagged `Incremental` against
/// a fabricated empty previous state would silently orphan any row for a
/// file deleted since the last persist, because an empty diff can never
/// produce a deletion.
#[derive(Clone, Debug)]
pub(crate) enum IndexDelta {
    /// Produced by a fresh build: no previous state exists to diff against.
    Full,
    /// Produced by a refresh that reused a previous index.
    Incremental(Box<IncrementalDelta>),
}

/// The changed-path plan behind [`IndexDelta::Incremental`].
#[derive(Clone, Debug)]
pub(crate) struct IncrementalDelta {
    /// Paths whose `FileBase` (and `Note`, if applicable) must be upserted
    /// into `FILES`/`NOTES` — added or metadata-changed since the last
    /// persist.
    pub(crate) upserted: Vec<PathBuf>,
    /// Paths removed since the last persist — must be deleted from
    /// `FILES`/`NOTES`.
    pub(crate) deleted: Vec<PathBuf>,
    /// Target paths in `LINKS` whose source set is new or changed — `None`
    /// when inlinks were reused unchanged (nothing to write).
    pub(crate) links_upserted: Option<Vec<PathBuf>>,
    /// Target paths removed from `LINKS` — always present alongside
    /// `links_upserted` (both `None`/both populated; empty `Vec` is a valid
    /// "no removals" case, distinct from `None`'s "inlinks unchanged,
    /// nothing computed").
    pub(crate) links_deleted: Vec<PathBuf>,
}

impl IncrementalDelta {
    /// True if this delta names no changes at all — persisting it would
    /// open a write transaction only to commit nothing.
    pub(crate) fn is_empty(&self) -> bool {
        self.upserted.is_empty()
            && self.deleted.is_empty()
            && self.links_deleted.is_empty()
            && self.links_upserted.as_ref().is_none_or(Vec::is_empty)
    }
}

/// Build plan for a [`super::FileIndex`].
///
/// Stores the fresh scan result and (optionally) moved notes from a previous
/// index. All heavy work (note parsing, sorting, inlink derivation) happens
/// once in [`Self::build`], not across intermediate steps.
///
/// # Invariants
///
/// - [`Self::from_scan`] always produces records sorted by path (guaranteed by
///   [`scan::scan_root`]).
/// - [`Self::reuse_unchanged`] consumes the previous index, moving its notes
///   and inlinks into the plan.
/// - [`Self::build`] produces a [`super::FileIndex`] with sorted records and
///   notes, and correctly derived inlinks (reused when nothing changed,
///   recomputed otherwise).
pub(crate) struct IndexBuilder<'a> {
    bases: Vec<FileBase>,
    /// `None` = fresh build (parse all notes at build time). `Some(cache)`
    /// = refresh (reuse `cache`'s previously-persisted state for
    /// unchanged records, parse only changed ones at build time).
    cache: Option<Box<RefreshCache<'a>>>,
}

impl<'a> IndexBuilder<'a> {
    /// Scans `root` for regular files. Parsing is deferred to [`Self::build`].
    ///
    /// # Errors
    ///
    /// - [`super::error::IndexBuilderError::Scan`] if a directory cannot be
    ///   read or a file's metadata cannot be inspected.
    pub(super) fn from_scan(root: &Path) -> Result<Self, IndexBuilderError> {
        let bases = scan::scan_root(root)?;
        Ok(Self {
            bases,
            cache: None,
        })
    }

    /// Consumes `cache` (already loaded via
    /// [`RefreshCache::load`]) to plan reuse of unchanged Notes without
    /// loading every persisted Note upfront.
    ///
    /// Parsing of changed or newly added notes, and recalling unchanged notes
    /// via point lookup, are both deferred to [`Self::build`].
    pub(super) fn reuse_unchanged(self, cache: RefreshCache<'a>) -> Self {
        Self {
            bases: self.bases,
            cache: Some(Box::new(cache)),
        }
    }

    /// Consumes the plan and produces a [`super::FileIndex`].
    ///
    /// - **Fresh build** (`cache: None`): parses every markdown record from
    ///   disk, sorts notes, derives inlinks.
    /// - **Refresh** (`cache: Some`): for each markdown record, reuses the
    ///   moved note if the file metadata is unchanged, otherwise parses from
    ///   disk. Sorts notes, then derives inlinks only if any note was reparsed
    ///   or the record set changed.
    ///
    /// # Errors
    ///
    /// - [`IndexBuilderError::NoteParse`] if a markdown file cannot be read.
    /// - [`IndexBuilderError::MissingNote`] if a matched record's note is
    ///   absent from the moved notes map (indicates a logic bug).
    pub(super) fn build(
        self,
        root: &Path,
    ) -> Result<super::FileIndex, IndexBuilderError> {
        let Self {
            bases,
            cache,
        } = self;
        match cache {
            None => Self::build_fresh(bases, root),
            Some(cache) => Self::build_with_reuse(bases, root, *cache),
        }
    }

    fn build_fresh(
        bases: Vec<FileBase>,
        root: &Path,
    ) -> Result<super::FileIndex, IndexBuilderError> {
        let mut notes = Vec::new();
        for base in &bases {
            if base.format() == FileFormat::Note {
                notes.push(parse_note(root, base)?);
            }
        }
        debug_assert!(
            notes.windows(2).all(|pair| {
                let [a, b] = pair else {
                    return true;
                };
                a.path() <= b.path()
            }),
            "notes must already be sorted by path: scan_root sorts records, \
             and this loop preserves that order while filtering to \
             Note-format entries"
        );
        let inlinks = derive_inlinks(&notes);
        Ok(super::FileIndex::new(bases, notes, inlinks, IndexDelta::Full))
    }

    /// Diffs two path-sorted `FileBase` slices via a two-pointer merge,
    /// returning current-side paths that are new or changed (`upserted`) and
    /// previous-side paths absent from `current` (`deleted`).
    fn diff_bases(
        current: &[FileBase],
        previous: &[FileBase],
    ) -> (Vec<PathBuf>, Vec<PathBuf>) {
        let mut upserted = Vec::new();
        let mut deleted = Vec::new();
        let mut cur = current.iter().peekable();
        let mut prev = previous.iter().peekable();
        loop {
            match (cur.peek(), prev.peek()) {
                (Some(c), Some(p)) => match c.path().cmp(p.path()) {
                    std::cmp::Ordering::Less => {
                        upserted.push(c.path().to_path_buf());
                        cur.next();
                    }
                    std::cmp::Ordering::Greater => {
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
                    deleted.push(p.path().to_path_buf());
                    prev.next();
                }
                (None, None) => break,
            }
        }
        (upserted, deleted)
    }

    /// Diffs two target-keyed inlink maps by source-set membership (order
    /// independent — [`derive_inlinks`]'s output and a redb-loaded map are not
    /// guaranteed to list one target's sources in the same order even when the
    /// set is identical). Returns target paths whose source set is new or
    /// changed (`upserted`) and target paths removed entirely (`deleted`).
    fn diff_inlinks(
        previous: &InlinkMap,
        current: &InlinkMap,
    ) -> (Vec<PathBuf>, Vec<PathBuf>) {
        let mut upserted = Vec::new();
        for (target, sources) in current {
            let changed = match previous.get(target) {
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
        let deleted = previous
            .keys()
            .filter(|target| !current.contains_key(*target))
            .cloned()
            .collect();
        (upserted, deleted)
    }

    fn build_with_reuse(
        bases: Vec<FileBase>,
        root: &Path,
        reuse: RefreshCache<'a>,
    ) -> Result<super::FileIndex, IndexBuilderError> {
        let (upserted, deleted) = Self::diff_bases(&bases, &reuse.previous);
        let mut notes = Vec::with_capacity(bases.len());
        let mut dirty = false;
        // Precondition: records and reuse.previous are path-sorted
        // (guaranteed by scan_root).
        let mut prev_iter = reuse.previous.iter().peekable();

        for base in &bases {
            dirty |= Self::has_deleted_note(&mut prev_iter, base.path());

            if base.format() != FileFormat::Note {
                continue;
            }

            let (note, reparsed) = Self::reconcile_note(
                base,
                &mut prev_iter,
                reuse.store,
                reuse.read_txn,
                root,
            )?;
            dirty |= reparsed;
            notes.push(note);
        }

        // Any previous entries left unconsumed sort after every current
        // record — trailing deletions. Same Note-only rule as
        // has_deleted_note.
        dirty |= prev_iter.any(|p| p.format() == FileFormat::Note);

        debug_assert!(
            notes.windows(2).all(|pair| {
                let [a, b] = pair else {
                    return true;
                };
                a.path() <= b.path()
            }),
            "notes must already be sorted by path: scan_root sorts records, \
             and this loop preserves that order while filtering to \
             Note-format entries"
        );

        // Inlinks depend on every Note's outlinks (ambiguous link resolution
        // considers the full set). Recompute only when a Note was added,
        // removed, or reparsed; non-Markdown file changes never affect the
        // link graph, so they must not force a full recompute.
        let new_inlinks_if_dirty = if dirty {
            Some(derive_inlinks(&notes))
        } else {
            None
        };
        let (links_upserted, links_deleted) = match &new_inlinks_if_dirty {
            Some(new_map) => {
                let (u, d) = Self::diff_inlinks(&reuse.inlinks, new_map);
                (Some(u), d)
            }
            None => (None, Vec::new()),
        };
        let inlinks = new_inlinks_if_dirty.unwrap_or(reuse.inlinks);
        let delta = IndexDelta::Incremental(Box::new(IncrementalDelta {
            upserted,
            deleted,
            links_upserted,
            links_deleted,
        }));

        Ok(super::FileIndex::new(bases, notes, inlinks, delta))
    }

    /// Advances `prev_iter` past every previously-indexed record with a path
    /// strictly less than `current_path` (records deleted since the last
    /// index). Returns `true` if any skipped record was a Note — only a
    /// deleted Note changes the inbound-link graph; a deleted non-Markdown
    /// file (image, PDF, ...) never contributed outlinks.
    fn has_deleted_note(
        prev_iter: &mut std::iter::Peekable<std::slice::Iter<'_, FileBase>>,
        current_path: &Path,
    ) -> bool {
        let mut deleted_note = false;
        while prev_iter.peek().is_some_and(|p| p.path() < current_path) {
            if prev_iter.next().is_some_and(|p| p.format() == FileFormat::Note)
            {
                deleted_note = true;
            }
        }
        deleted_note
    }

    /// Reuses `record`'s previously-parsed Note if a previously-indexed
    /// record at the same path has unchanged metadata, otherwise parses it
    /// fresh from disk. Returns the resolved Note and whether it was
    /// reparsed (`true`) or reused unchanged (`false`).
    ///
    /// Consumes `prev_iter`'s peeked entry whenever its path matches
    /// `record`'s path (whether reused or superseded) so the entry is never
    /// also counted as a deletion by a later `has_deleted_note` call or the
    /// trailing-deletion check — the previous version of this logic only
    /// peeked and never consumed a matched entry, so every matched Note was
    /// spuriously counted as deleted on the next call, forcing an
    /// unnecessary `derive_inlinks` recompute on almost every refresh.
    fn reconcile_note(
        base: &FileBase,
        prev_iter: &mut std::iter::Peekable<std::slice::Iter<'_, FileBase>>,
        store: &super::store::IndexStore,
        read_txn: &redb::ReadTransaction,
        root: &Path,
    ) -> Result<(crate::note::Note, bool), IndexBuilderError> {
        let previous_matches_path =
            prev_iter.peek().is_some_and(|p| p.path() == base.path());
        let unchanged = previous_matches_path
            && prev_iter.peek().is_some_and(|p| **p == *base);

        if previous_matches_path {
            prev_iter.next();
        }

        if unchanged {
            let note = store
                .load_note(read_txn, base.path())
                .map_err(|source| IndexBuilderError::NoteLookup {
                    path: base.path().to_path_buf(),
                    source: Box::new(source),
                })?
                .ok_or_else(|| IndexBuilderError::MissingNote {
                    path: base.path().to_path_buf(),
                })?;
            Ok((note, false))
        } else {
            Ok((parse_note(root, base)?, true))
        }
    }
}

/// State carried across [`IndexBuilder::build_with_reuse`], borrowed
/// from the caller's own [`super::store::IndexStore`]/`ReadTransaction`
/// rather than owned — owning them here would force
/// [`super::IndexerService::refresh`] to reopen the store a second time
/// to persist afterward.
///
/// `read_txn` stays open for the entire call — every `parse_note` disk read
/// and the full merge-join, not just the
/// [`super::store::IndexStore::load_note`] point lookups it backs. Per redb's
/// own docs, "read-only transactions may exist concurrently with writes", so
/// this never blocks a concurrent writer; it does pin the transaction's MVCC
/// snapshot for the duration. [`super::IndexerService::refresh`] scopes this
/// transaction's lifetime to end before it opens its own write transaction
/// to persist, so the two never overlap.
pub(super) struct RefreshCache<'a> {
    previous: Vec<FileBase>,
    inlinks: InlinkMap,
    store: &'a super::store::IndexStore,
    read_txn: &'a redb::ReadTransaction,
}

impl<'a> RefreshCache<'a> {
    /// Loads `previous`/`inlinks` via `store` through `read_txn` — the
    /// only way to construct a `RefreshCache`; fields stay private.
    ///
    /// # Errors
    ///
    /// - [`IndexError::Store`] if the previously persisted `FILES`/`LINKS`
    ///   tables cannot be read.
    pub(super) fn load(
        store: &'a super::store::IndexStore,
        read_txn: &'a redb::ReadTransaction,
    ) -> Result<Self, IndexError> {
        let (previous, inlinks) = store.load_bases_and_links_via(read_txn)?;
        Ok(Self {
            previous,
            inlinks,
            store,
            read_txn,
        })
    }
}

/// Reads and parses the markdown file for `record`.
fn parse_note(
    root: &Path,
    base: &FileBase,
) -> Result<crate::note::Note, IndexBuilderError> {
    let full_path = root.join(base.path());
    let content = std::fs::read_to_string(&full_path).map_err(|source| {
        IndexBuilderError::NoteParse {
            path: full_path,
            source,
        }
    })?;
    Ok(parse_markdown(base.path(), &content))
}

#[cfg(test)]
mod tests {
    use std::{fs, path::Path};

    use super::*;
    use crate::{
        file::FileBase,
        index::{FileIndex, IndexerService, store::IndexStore},
        note::Note,
    };

    /// Persists `previous` to `root` and reopens the store, mirroring what
    /// `IndexerService::refresh` does in production before reconciliation.
    fn persist_previous(previous: &FileIndex, root: &Path) -> IndexStore {
        let store = IndexStore::open(root).expect("open store");
        store
            .replace_all(previous.bases(), previous.notes(), previous.inlinks())
            .expect("persist previous index");
        store
    }

    mod constructor {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn from_scan_produces_sorted_records() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("b.md"), "# B").expect("write b");
            fs::write(temp.path().join("a.md"), "# A").expect("write a");

            let index = IndexBuilder::from_scan(temp.path())
                .expect("scan")
                .build(temp.path())
                .expect("build");

            assert_eq!(
                index.bases().iter().map(FileBase::path).collect::<Vec<_>>(),
                [Path::new("a.md"), Path::new("b.md")]
            );
        }

        #[test]
        fn from_scan_parses_markdown_notes() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("note.md"), "- [ ] task")
                .expect("write note");
            fs::write(temp.path().join("readme.txt"), "text")
                .expect("write txt");

            let index = IndexBuilder::from_scan(temp.path())
                .expect("scan")
                .build(temp.path())
                .expect("build");

            assert_eq!(index.bases().len(), 2);
            assert_eq!(index.notes().len(), 1);
            assert_eq!(
                index.note(Path::new("note.md")).map(Note::path),
                Some(Path::new("note.md"))
            );
        }
    }

    mod inlink_reuse {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn reused_index_preserves_inlinks_when_nothing_changes() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(
                temp.path().join("a.md"),
                "---\ntitle: A\n---\nLink to [[b]].",
            )
            .expect("write a");
            fs::write(temp.path().join("b.md"), "---\ntitle: B\n---\nBody.")
                .expect("write b");

            let first = IndexBuilder::from_scan(temp.path())
                .expect("scan")
                .build(temp.path())
                .expect("build");
            let first_inlinks = first.inlinks().len();

            let store = persist_previous(&first, temp.path());
            let read_txn = store.begin_read().expect("begin read");
            let cache =
                RefreshCache::load(&store, &read_txn).expect("load cache");
            let second = IndexBuilder::from_scan(temp.path())
                .expect("scan")
                .reuse_unchanged(cache)
                .build(temp.path())
                .expect("build");

            assert_eq!(
                second.inlinks().len(),
                first_inlinks,
                "inlinks must be reused when nothing changed"
            );
        }

        #[test]
        fn deleted_non_note_file_does_not_mark_dirty() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(
                temp.path().join("note.md"),
                "---\ntitle: Note\n---\nBody.",
            )
            .expect("write note");
            fs::write(temp.path().join("image.png"), "fake")
                .expect("write image");

            let first = IndexBuilder::from_scan(temp.path())
                .expect("scan")
                .build(temp.path())
                .expect("build");
            let first_inlinks = first.inlinks().len();

            fs::remove_file(temp.path().join("image.png"))
                .expect("delete image");

            let store = persist_previous(&first, temp.path());
            let read_txn = store.begin_read().expect("begin read");
            let cache =
                RefreshCache::load(&store, &read_txn).expect("load cache");
            let second = IndexBuilder::from_scan(temp.path())
                .expect("scan")
                .reuse_unchanged(cache)
                .build(temp.path())
                .expect("build");

            assert_eq!(
                second.inlinks().len(),
                first_inlinks,
                "deleting non-note file must not recompute inlinks"
            );
        }
    }

    mod reuse {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn skips_parse_for_unchanged_records() {
            // Arrange
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(
                temp.path().join("note.md"),
                "---\ntitle: Test\n---\nBody.",
            )
            .expect("write note");

            let first = IndexBuilder::from_scan(temp.path())
                .expect("scan")
                .build(temp.path())
                .expect("build");
            let first_len = first.notes().len();

            // Act
            let store = persist_previous(&first, temp.path());
            let read_txn = store.begin_read().expect("begin read");
            let cache =
                RefreshCache::load(&store, &read_txn).expect("load cache");
            let second = IndexBuilder::from_scan(temp.path())
                .expect("scan")
                .reuse_unchanged(cache)
                .build(temp.path())
                .expect("build");

            // Assert
            assert_eq!(first_len, second.notes().len());
        }

        #[test]
        fn reparse_when_record_content_changes() {
            // Arrange
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(
                temp.path().join("note.md"),
                "---\ntitle: V1\n---\nBody.",
            )
            .expect("write note");

            let first = IndexBuilder::from_scan(temp.path())
                .expect("scan")
                .build(temp.path())
                .expect("build");

            fs::write(
                temp.path().join("note.md"),
                "---\ntitle: V2\n---\nBody.",
            )
            .expect("rewrite note");

            // Act
            let store = persist_previous(&first, temp.path());
            let read_txn = store.begin_read().expect("begin read");
            let cache =
                RefreshCache::load(&store, &read_txn).expect("load cache");
            let second = IndexBuilder::from_scan(temp.path())
                .expect("scan")
                .reuse_unchanged(cache)
                .build(temp.path())
                .expect("build");

            // Assert
            let title = second
                .notes()
                .first()
                .expect("note must exist")
                .frontmatter()
                .and_then(|fm| fm.get("title").cloned());
            assert_eq!(
                title,
                Some(crate::note::NoteFieldValue::String("V2".to_owned()))
            );
        }

        #[test]
        fn removes_deleted_notes_from_index() {
            // Arrange
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(
                temp.path().join("note.md"),
                "---\ntitle: Test\n---\nBody.",
            )
            .expect("write note");

            let first = IndexBuilder::from_scan(temp.path())
                .expect("scan")
                .build(temp.path())
                .expect("build");
            assert_eq!(first.notes().len(), 1);

            fs::remove_file(temp.path().join("note.md")).expect("delete note");

            // Act
            let store = persist_previous(&first, temp.path());
            let read_txn = store.begin_read().expect("begin read");
            let cache =
                RefreshCache::load(&store, &read_txn).expect("load cache");
            let second = IndexBuilder::from_scan(temp.path())
                .expect("scan")
                .reuse_unchanged(cache)
                .build(temp.path())
                .expect("build");

            // Assert
            assert_eq!(second.notes().len(), 0, "deleted note must be removed");
        }

        #[test]
        fn includes_newly_added_notes() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("a.md"), "---\ntitle: A\n---\nBody.")
                .expect("write a");

            let first = IndexBuilder::from_scan(temp.path())
                .expect("scan")
                .build(temp.path())
                .expect("build");
            assert_eq!(first.notes().len(), 1);

            fs::write(temp.path().join("b.md"), "---\ntitle: B\n---\nBody.")
                .expect("write b");

            let store = persist_previous(&first, temp.path());
            let read_txn = store.begin_read().expect("begin read");
            let cache =
                RefreshCache::load(&store, &read_txn).expect("load cache");
            let second = IndexBuilder::from_scan(temp.path())
                .expect("scan")
                .reuse_unchanged(cache)
                .build(temp.path())
                .expect("build");

            assert_eq!(second.notes().len(), 2, "new note must be included");
        }

        #[test]
        fn preserves_task_content_for_unchanged_records() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("note.md"), "- [ ] task")
                .expect("write note");
            let built =
                IndexerService::new(temp.path()).build().expect("build index");

            let store = persist_previous(&built, temp.path());
            let read_txn = store.begin_read().expect("begin read");
            let cache =
                RefreshCache::load(&store, &read_txn).expect("load cache");
            let index = IndexBuilder::from_scan(temp.path())
                .expect("scan")
                .reuse_unchanged(cache)
                .build(temp.path())
                .expect("build");

            assert_eq!(
                index
                    .note(Path::new("note.md"))
                    .map(Note::tasks)
                    .map(Iterator::count),
                Some(1)
            );
        }

        #[test]
        fn reparses_task_content_when_record_changes() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("note.md"), "- [ ] task")
                .expect("write note");
            let built =
                IndexerService::new(temp.path()).build().expect("build index");

            fs::write(temp.path().join("note.md"), "- [ ] task\n- [x] done")
                .expect("rewrite note");

            let store = persist_previous(&built, temp.path());
            let read_txn = store.begin_read().expect("begin read");
            let cache =
                RefreshCache::load(&store, &read_txn).expect("load cache");
            let index = IndexBuilder::from_scan(temp.path())
                .expect("scan")
                .reuse_unchanged(cache)
                .build(temp.path())
                .expect("build");

            assert_eq!(
                index
                    .note(Path::new("note.md"))
                    .map(Note::tasks)
                    .map(Iterator::count),
                Some(2)
            );
        }
    }

    mod reconcile_note {
        use super::*;

        #[test]
        fn consumes_the_matched_previous_entry_so_it_is_not_double_counted() {
            // Arrange: one previously-indexed Note, unchanged in the fresh
            // scan.
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("a.md"), "content").expect("write note");
            let previous = scan::scan_root(temp.path()).expect("scan root");
            let record = previous.first().expect("one record");
            let mut prev_iter = previous.iter().peekable();
            let note = crate::note::parse_markdown("a.md", "content");
            let store = IndexStore::open(temp.path()).expect("open store");
            store
                .replace_all(&previous, &[note], &InlinkMap::new())
                .expect("persist note");
            let read_txn = store.begin_read().expect("begin read");

            // Act
            let (_, reparsed) = IndexBuilder::reconcile_note(
                record,
                &mut prev_iter,
                &store,
                &read_txn,
                temp.path(),
            )
            .expect("reconcile succeeds");

            // Assert: unchanged, and the matched entry is consumed, not left
            // for the next has_deleted_note/trailing check to miscount as
            // deleted.
            assert!(!reparsed);
            assert!(prev_iter.peek().is_none());
        }

        #[test]
        fn consumes_the_matched_previous_entry_even_when_the_record_changed() {
            // Arrange: previously-indexed Note whose content (and thus size)
            // differs in the fresh scan, at the same path.
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("a.md"), "content").expect("write note");
            let previous = scan::scan_root(temp.path()).expect("scan root");

            fs::write(temp.path().join("a.md"), "different content")
                .expect("rewrite note");
            let current = scan::scan_root(temp.path()).expect("rescan root");
            let record = current.first().expect("one record");
            let mut prev_iter = previous.iter().peekable();
            let store = IndexStore::open(temp.path()).expect("open store");
            let read_txn = store.begin_read().expect("begin read");

            // Act
            let (_, reparsed) = IndexBuilder::reconcile_note(
                record,
                &mut prev_iter,
                &store,
                &read_txn,
                temp.path(),
            )
            .expect("reconcile succeeds");

            // Assert: reparsed, and the matched (now-stale) previous entry
            // is still consumed — the doc comment's "whether reused or
            // superseded" claim, exercised on the superseded branch.
            assert!(reparsed);
            assert!(prev_iter.peek().is_none());
        }
    }
}
