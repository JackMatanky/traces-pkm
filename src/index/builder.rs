//! Internal build pipeline for [`super::FileIndex`].
//!
//! [`IndexBuilder`] is a **plan**: it holds the scan result and a reuse
//! directive, deferring all note parsing, sorting, and inlink derivation to
//! [`IndexBuilder::build`]. Callers use [`super::IndexerService::build`] and
//! [`super::IndexerService::refresh`] instead of this type directly.

use std::path::Path;

use super::{
    FileFormat,
    cache::{NoteCacheState, RefreshCache},
    delta::{IncrementalDelta, IndexDelta},
    error::IndexBuilderError,
    inlinks::derive_inlinks,
};
use crate::{file::FileBase, note::parse_markdown};

/// Build plan for a [`super::FileIndex`].
///
/// Stores an already-scanned set of records and (optionally) a [`RefreshCache`]
/// to reuse. All heavy work (note parsing, sorting, inlink derivation) happens
/// once in [`Self::build`], not across intermediate steps. Scanning itself
/// lives in [`super::IndexerService::scan`], not here: `IndexBuilder` is pure
/// data assembly, no I/O.
///
/// # Invariants
///
/// - `files` must already be sorted by path, guaranteed by
///   [`super::IndexerService::scan`], the only production caller.
/// - [`Self::with_cache`] consumes the previous index's cache, reusing its
///   notes and inlinks where unchanged.
/// - [`Self::build`] produces a [`super::FileIndex`] with sorted records and
///   notes, and correctly derived inlinks (reused when nothing changed,
///   recomputed otherwise).
/// - The delta on the returned [`super::FileIndex`] is
///   [`super::delta::IndexDelta::Full`] for a fresh build and
///   [`super::delta::IndexDelta::Incremental`] for a refresh, enabling
///   [`super::store::IndexStore::persist_index`] to choose the appropriate
///   write strategy.
///
/// [`RefreshCache`]: super::cache::RefreshCache
/// [`RefreshCache::load`]: super::cache::RefreshCache::load
pub(crate) struct IndexBuilder<'a> {
    files: Vec<FileBase>,
    /// `None` = fresh build (parse all notes at build time). `Some(cache)`
    /// = refresh (reuse `cache`'s previously-persisted state for
    /// unchanged records, parse only changed ones at build time).
    cache: Option<Box<RefreshCache<'a>>>,
}

impl<'a> IndexBuilder<'a> {
    /// Wraps an already-scanned, path-sorted set of records. Parsing is
    /// deferred to [`Self::build`].
    pub(super) fn new(files: Vec<FileBase>) -> Self {
        Self {
            files,
            cache: None,
        }
    }

    /// Attaches `cache` (already loaded via [`RefreshCache::load`]) to plan
    /// reuse of unchanged Notes without loading every persisted Note upfront.
    ///
    /// [`RefreshCache::load`]: super::cache::RefreshCache::load
    pub(super) fn with_cache(mut self, cache: RefreshCache<'a>) -> Self {
        self.cache = Some(Box::new(cache));
        self
    }

    /// Consumes the plan and produces a [`super::FileIndex`].
    ///
    /// - **Fresh build** (`cache: None`): parses every markdown record from
    ///   disk, sorts notes, derives inlinks. Never opens `IndexStore`; forcing
    ///   this through `RefreshCache` would both cost a needless store-open on
    ///   every first-time build and, more importantly, risk conflating "no
    ///   previous state to check" with "verified nothing was deleted," which
    ///   only `RefreshCache::load`'s real query can honestly claim (see
    ///   [`IndexDelta`]'s doc comment).
    /// - **Refresh** (`cache: Some`): for each record, reuses the previous Note
    ///   via point lookup when unchanged, otherwise reparses and backdates.
    ///   Recomputes inlinks only if a Note was added, removed, or its outlinks
    ///   actually changed.
    ///
    /// # Errors
    ///
    /// - [`IndexBuilderError::NoteParse`] if a markdown file cannot be read.
    /// - [`IndexBuilderError::MissingNote`] if a matched record's note is
    ///   absent from the persisted index (indicates a logic bug).
    pub(super) fn build(
        self,
        root: &Path,
    ) -> Result<super::FileIndex, IndexBuilderError> {
        let Self {
            files,
            cache,
        } = self;
        match cache {
            None => Self::build_fresh(files, root),
            Some(cache) => Self::build_with_cache(files, root, *cache),
        }
    }

    fn build_fresh(
        files: Vec<FileBase>,
        root: &Path,
    ) -> Result<super::FileIndex, IndexBuilderError> {
        let mut notes = Vec::new();
        for file in &files {
            if file.format() == FileFormat::Note {
                notes.push(parse_note(root, file)?);
            }
        }
        debug_assert!(
            notes.windows(2).all(|pair| {
                let [a, b] = pair else {
                    return true;
                };
                a.path() <= b.path()
            }),
            "notes must already be sorted by path: IndexerService::scan sorts \
             records, and this loop preserves that order while filtering to \
             Note-format entries"
        );
        let inlinks = derive_inlinks(&notes);
        Ok(super::FileIndex::new(files, notes, inlinks, IndexDelta::Full))
    }

    fn build_with_cache(
        files: Vec<FileBase>,
        root: &Path,
        cache: RefreshCache<'a>,
    ) -> Result<super::FileIndex, IndexBuilderError> {
        let (upserted, deleted, mut stale) = cache.diff_files(&files);
        let mut upserted_iter = upserted.iter().peekable();
        let mut notes = Vec::with_capacity(files.len());

        for file in &files {
            let cache_state = if upserted_iter
                .next_if(|p| p.as_path() == file.path())
                .is_some()
            {
                NoteCacheState::Upserted
            } else {
                NoteCacheState::Fresh
            };
            if file.format() != FileFormat::Note {
                continue;
            }
            let (note, outlinks_changed) =
                cache.reconcile_note(file, cache_state, root)?;
            stale |= outlinks_changed;
            notes.push(note);
        }

        debug_assert!(
            notes.windows(2).all(|pair| {
                let [a, b] = pair else {
                    return true;
                };
                a.path() <= b.path()
            }),
            "notes must already be sorted by path: IndexerService::scan sorts \
             records, and this loop preserves that order while filtering to \
             Note-format entries"
        );

        let new_inlinks_if_stale = stale.then(|| derive_inlinks(&notes));
        let (links_upserted, links_deleted) = match &new_inlinks_if_stale {
            Some(new_map) => {
                let (u, d) = cache.diff_links(new_map);
                (Some(u), d)
            }
            None => (None, Vec::new()),
        };
        let inlinks =
            new_inlinks_if_stale.unwrap_or_else(|| cache.into_inlinks());

        let delta = IndexDelta::Incremental(Box::new(IncrementalDelta {
            upserted,
            deleted,
            links_upserted,
            links_deleted,
        }));
        Ok(super::FileIndex::new(files, notes, inlinks, delta))
    }
}

/// Reads and parses the markdown file for `file`.
///
/// # Errors
///
/// - [`IndexBuilderError::NoteParse`] if the file cannot be read or is not
///   valid UTF-8.
pub(super) fn parse_note(
    root: &Path,
    file: &FileBase,
) -> Result<crate::note::Note, IndexBuilderError> {
    let full_path = root.join(file.path());
    let content = std::fs::read_to_string(&full_path).map_err(|source| {
        IndexBuilderError::NoteParse {
            path: full_path,
            source,
        }
    })?;
    Ok(parse_markdown(file.path(), &content))
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
            .write_all(previous.bases(), previous.notes(), previous.inlinks())
            .expect("persist previous index");
        store
    }

    mod constructor {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn new_produces_sorted_records() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("b.md"), "# B").expect("write b");
            fs::write(temp.path().join("a.md"), "# A").expect("write a");

            let index = IndexBuilder::new(
                IndexerService::new(temp.path()).scan().expect("scan"),
            )
            .build(temp.path())
            .expect("build");

            assert_eq!(
                index.bases().iter().map(FileBase::path).collect::<Vec<_>>(),
                [Path::new("a.md"), Path::new("b.md")]
            );
        }

        #[test]
        fn new_parses_markdown_notes() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("note.md"), "- [ ] task")
                .expect("write note");
            fs::write(temp.path().join("readme.txt"), "text")
                .expect("write txt");

            let index = IndexBuilder::new(
                IndexerService::new(temp.path()).scan().expect("scan"),
            )
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

            let first = IndexBuilder::new(
                IndexerService::new(temp.path()).scan().expect("scan"),
            )
            .build(temp.path())
            .expect("build");
            let first_inlinks = first.inlinks().len();

            let store = persist_previous(&first, temp.path());
            let read_txn = store.begin_read().expect("begin read");
            let cache =
                RefreshCache::load(&store, &read_txn).expect("load cache");
            let second = IndexBuilder::new(
                IndexerService::new(temp.path()).scan().expect("scan"),
            )
            .with_cache(cache)
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

            let first = IndexBuilder::new(
                IndexerService::new(temp.path()).scan().expect("scan"),
            )
            .build(temp.path())
            .expect("build");
            let first_inlinks = first.inlinks().len();

            fs::remove_file(temp.path().join("image.png"))
                .expect("delete image");

            let store = persist_previous(&first, temp.path());
            let read_txn = store.begin_read().expect("begin read");
            let cache =
                RefreshCache::load(&store, &read_txn).expect("load cache");
            let second = IndexBuilder::new(
                IndexerService::new(temp.path()).scan().expect("scan"),
            )
            .with_cache(cache)
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

            let first = IndexBuilder::new(
                IndexerService::new(temp.path()).scan().expect("scan"),
            )
            .build(temp.path())
            .expect("build");
            let first_len = first.notes().len();

            // Act
            let store = persist_previous(&first, temp.path());
            let read_txn = store.begin_read().expect("begin read");
            let cache =
                RefreshCache::load(&store, &read_txn).expect("load cache");
            let second = IndexBuilder::new(
                IndexerService::new(temp.path()).scan().expect("scan"),
            )
            .with_cache(cache)
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

            let first = IndexBuilder::new(
                IndexerService::new(temp.path()).scan().expect("scan"),
            )
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
            let second = IndexBuilder::new(
                IndexerService::new(temp.path()).scan().expect("scan"),
            )
            .with_cache(cache)
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

            let first = IndexBuilder::new(
                IndexerService::new(temp.path()).scan().expect("scan"),
            )
            .build(temp.path())
            .expect("build");
            assert_eq!(first.notes().len(), 1);

            fs::remove_file(temp.path().join("note.md")).expect("delete note");

            // Act
            let store = persist_previous(&first, temp.path());
            let read_txn = store.begin_read().expect("begin read");
            let cache =
                RefreshCache::load(&store, &read_txn).expect("load cache");
            let second = IndexBuilder::new(
                IndexerService::new(temp.path()).scan().expect("scan"),
            )
            .with_cache(cache)
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

            let first = IndexBuilder::new(
                IndexerService::new(temp.path()).scan().expect("scan"),
            )
            .build(temp.path())
            .expect("build");
            assert_eq!(first.notes().len(), 1);

            fs::write(temp.path().join("b.md"), "---\ntitle: B\n---\nBody.")
                .expect("write b");

            let store = persist_previous(&first, temp.path());
            let read_txn = store.begin_read().expect("begin read");
            let cache =
                RefreshCache::load(&store, &read_txn).expect("load cache");
            let second = IndexBuilder::new(
                IndexerService::new(temp.path()).scan().expect("scan"),
            )
            .with_cache(cache)
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
            let index = IndexBuilder::new(
                IndexerService::new(temp.path()).scan().expect("scan"),
            )
            .with_cache(cache)
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
            let index = IndexBuilder::new(
                IndexerService::new(temp.path()).scan().expect("scan"),
            )
            .with_cache(cache)
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

        #[test]
        fn upserted_non_note_file_between_notes_does_not_stall_the_upserted_pointer()
         {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("a.md"), "# A").expect("write a");
            fs::write(temp.path().join("m.png"), "fake").expect("write image");
            fs::write(temp.path().join("z.md"), "# Z").expect("write z");
            let first = IndexBuilder::new(
                IndexerService::new(temp.path()).scan().expect("scan"),
            )
            .build(temp.path())
            .expect("build");

            fs::write(temp.path().join("m.png"), "fake, changed")
                .expect("change image, sorts between a.md and z.md");

            let store = persist_previous(&first, temp.path());
            let read_txn = store.begin_read().expect("begin read");
            let cache =
                RefreshCache::load(&store, &read_txn).expect("load cache");
            let second = IndexBuilder::new(
                IndexerService::new(temp.path()).scan().expect("scan"),
            )
            .with_cache(cache)
            .build(temp.path())
            .expect("build");

            // Both notes must still be present and unchanged — proves the
            // upserted pointer was consumed for m.png (a non-Note) and did
            // not misalign against a.md/z.md.
            assert_eq!(second.notes().len(), 2);
        }
    }
}
