//! Internal build pipeline for [`super::FileIndex`].
//!
//! [`IndexBuilder`] holds a scan result and optional cache, deferring note
//! parsing, sorting, and inlink derivation to [`IndexBuilder::build`]. Callers
//! use [`super::IndexerService::build`] and [`super::IndexerService::refresh`]
//! directly; this module is not part of the public API.

use std::path::Path;

use super::{
    FileFormat,
    cache::{NoteCacheState, RefreshCache},
    delta::{IncrementalDelta, IndexDelta},
    entry,
    error::IndexBuilderError,
    inlinks::derive_inlinks,
};
use crate::{
    config::{FrontmatterConfig, TaskConfig},
    file::FileBase,
    note::{MarkdownParserInput, parse_markdown},
};

/// Build plan for a [`super::FileIndex`].
///
/// Holds an already-scanned set of records and an optional [`RefreshCache`].
/// Note parsing, sorting, and inlink derivation run in [`Self::build`].
/// Scanning lives in [`super::IndexerService::scan`]; `IndexBuilder` performs
/// data assembly without I/O.
///
/// # Invariants
///
/// - `files` must be sorted by path, guaranteed by
///   [`super::IndexerService::scan`].
/// - [`Self::with_cache`] consumes the previous index's cache for reuse.
/// - [`Self::build`] produces a [`super::FileIndex`] with sorted records and
///   correctly derived inlinks.
/// - The returned delta is [`IndexDelta::Full`] for fresh builds and
///   [`IndexDelta::Incremental`] for refreshes.
///
/// [`RefreshCache`]: super::cache::RefreshCache
/// [`IndexDelta::Full`]: super::delta::IndexDelta::Full
/// [`IndexDelta::Incremental`]: super::delta::IndexDelta::Incremental
pub(crate) struct IndexBuilder<'a> {
    files: Vec<FileBase>,
    /// `None` = fresh build (parse all notes).
    /// `Some(cache)` = refresh (reuse `cache`'s previously-persisted state
    /// for unchanged records, parse only changed ones).
    cache: Option<Box<RefreshCache<'a>>>,
    tasks: TaskConfig,
    frontmatter: FrontmatterConfig,
}

impl<'a> IndexBuilder<'a> {
    /// Wraps an already-scanned, path-sorted set of records. Parsing is
    /// deferred to [`Self::build`].
    pub(super) fn new(files: Vec<FileBase>) -> Self {
        Self {
            files,
            cache: None,
            tasks: TaskConfig::default(),
            frontmatter: FrontmatterConfig::default(),
        }
    }

    /// Attaches resolved [`TaskConfig`] settings for task classification.
    #[inline]
    #[must_use]
    pub(super) fn with_tasks(mut self, tasks: TaskConfig) -> Self {
        self.tasks = tasks;
        self
    }

    /// Attaches resolved [`FrontmatterConfig`] settings.
    #[inline]
    #[must_use]
    pub(super) fn with_frontmatter(
        mut self,
        frontmatter: FrontmatterConfig,
    ) -> Self {
        self.frontmatter = frontmatter;
        self
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
    /// - **Fresh build** (`cache: None`): parses every note from disk, sorts,
    ///   and derives inlinks.
    /// - **Refresh** (`cache: Some`): reuses unchanged notes via point lookup,
    ///   reparsing only modified files. Recomputes inlinks only when notes or
    ///   outlinks change.
    ///
    /// # Errors
    ///
    /// - [`IndexBuilderError::NoteParse`] if a markdown file cannot be read.
    /// - [`IndexBuilderError::MissingNote`] if a matched record's note is
    ///   absent from the persisted index.
    pub(super) fn build(
        self,
        root: &Path,
    ) -> Result<super::FileIndex, IndexBuilderError> {
        let Self {
            files,
            cache,
            tasks,
            frontmatter,
        } = self;
        match cache {
            None => Self::build_fresh(files, root, &tasks, &frontmatter),
            Some(cache) => Self::build_with_cache(
                files,
                root,
                *cache,
                &tasks,
                &frontmatter,
            ),
        }
    }

    fn build_fresh(
        files: Vec<FileBase>,
        root: &Path,
        tasks: &TaskConfig,
        frontmatter: &FrontmatterConfig,
    ) -> Result<super::FileIndex, IndexBuilderError> {
        let mut entries = Vec::with_capacity(files.len());
        for file in files {
            let note = if file.format() == FileFormat::Note {
                Some(parse_note(root, &file, tasks, frontmatter)?)
            } else {
                None
            };
            entries.push(entry::FileEntry::new(file, note));
        }
        debug_assert!(
            entries.windows(2).all(|pair| {
                let [a, b] = pair else {
                    return true;
                };
                a.file().path() <= b.file().path()
            }),
            "entries must already be sorted by path: IndexerService::scan \
             sorts records, and this loop preserves that order while building \
             FileEntry values"
        );
        let notes_view: Vec<&crate::note::Note> =
            entries.iter().filter_map(entry::FileEntry::note).collect();
        let inlinks = derive_inlinks(&notes_view);
        entry::redistribute_inlinks(&mut entries, inlinks);
        Ok(super::FileIndex::new(entries.into_boxed_slice(), IndexDelta::Full))
    }

    fn build_with_cache(
        files: Vec<FileBase>,
        root: &Path,
        cache: RefreshCache<'a>,
        tasks: &TaskConfig,
        frontmatter: &FrontmatterConfig,
    ) -> Result<super::FileIndex, IndexBuilderError> {
        let (upserted, deleted, mut stale) = cache.diff_files(&files);
        let mut upserted_iter = upserted.iter().peekable();
        let mut entries = Vec::with_capacity(files.len());

        for file in files {
            let cache_state = if upserted_iter
                .next_if(|p| p.as_path() == file.path())
                .is_some()
            {
                NoteCacheState::Upserted
            } else {
                NoteCacheState::Fresh
            };
            let note = if file.format() == FileFormat::Note {
                let (note, outlinks_changed) = cache.reconcile_note(
                    &file,
                    cache_state,
                    root,
                    (tasks, frontmatter),
                )?;
                stale |= outlinks_changed;
                Some(note)
            } else {
                None
            };
            entries.push(entry::FileEntry::new(file, note));
        }

        debug_assert!(
            entries.windows(2).all(|pair| {
                let [a, b] = pair else {
                    return true;
                };
                a.file().path() <= b.file().path()
            }),
            "entries must already be sorted by path: IndexerService::scan \
             sorts records, and this loop preserves that order while building \
             FileEntry values"
        );

        let new_inlinks_if_stale = stale.then(|| {
            let notes_view: Vec<&crate::note::Note> =
                entries.iter().filter_map(entry::FileEntry::note).collect();
            derive_inlinks(&notes_view)
        });
        let (links_upserted, links_deleted) = match &new_inlinks_if_stale {
            Some(new_map) => {
                let (u, d) = cache.diff_links(new_map);
                (Some(u), d)
            }
            None => (None, Vec::new()),
        };
        let inlinks =
            new_inlinks_if_stale.unwrap_or_else(|| cache.into_inlinks());
        entry::redistribute_inlinks(&mut entries, inlinks);

        let delta = IndexDelta::Incremental(Box::new(IncrementalDelta {
            upserted,
            deleted,
            links_upserted,
            links_deleted,
        }));
        Ok(super::FileIndex::new(entries.into_boxed_slice(), delta))
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
    tasks: &TaskConfig,
    frontmatter: &FrontmatterConfig,
) -> Result<crate::note::Note, IndexBuilderError> {
    let full_path = root.join(file.path());
    let content = std::fs::read_to_string(&full_path).map_err(|source| {
        IndexBuilderError::NoteParse {
            path: full_path,
            source,
        }
    })?;
    let input =
        MarkdownParserInput::new(file.path(), &content, tasks, frontmatter);
    Ok(parse_markdown(&input))
}

#[cfg(test)]
mod tests {
    use std::{fs, path::Path};

    use super::*;
    use crate::{
        file::FileBase,
        index::{FileEntry, FileIndex, IndexerService, store::IndexStore},
        note::Note,
    };

    /// Persists `previous` to `root` and reopens the store, mirroring what
    /// `IndexerService::refresh` does in production before reconciliation.
    fn persist_previous(previous: &FileIndex, root: &Path) -> IndexStore {
        let store = IndexStore::open(root).expect("open store");
        store.write_all(previous.entries()).expect("persist previous index");
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
                index
                    .entries()
                    .iter()
                    .map(FileEntry::file)
                    .map(FileBase::path)
                    .collect::<Vec<_>>(),
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

            assert_eq!(index.entries().len(), 2);
            assert_eq!(
                index
                    .entries()
                    .iter()
                    .filter(|entry| entry.note().is_some())
                    .count(),
                1
            );
            assert_eq!(
                index
                    .entries()
                    .iter()
                    .find(|entry| entry.file().path() == Path::new("note.md"))
                    .and_then(FileEntry::note)
                    .map(Note::path),
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
            let first_inlinks = first
                .entries()
                .iter()
                .filter(|entry| !entry.inlinks().is_empty())
                .count();

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
                second
                    .entries()
                    .iter()
                    .filter(|entry| !entry.inlinks().is_empty())
                    .count(),
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
            let first_inlinks = first
                .entries()
                .iter()
                .filter(|entry| !entry.inlinks().is_empty())
                .count();

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
                second
                    .entries()
                    .iter()
                    .filter(|entry| !entry.inlinks().is_empty())
                    .count(),
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
            let first_len = first
                .entries()
                .iter()
                .filter(|entry| entry.note().is_some())
                .count();

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
            assert_eq!(
                first_len,
                second
                    .entries()
                    .iter()
                    .filter(|entry| entry.note().is_some())
                    .count()
            );
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
                .entries()
                .iter()
                .find_map(FileEntry::note)
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
            assert_eq!(
                first
                    .entries()
                    .iter()
                    .filter(|entry| entry.note().is_some())
                    .count(),
                1
            );

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
            assert_eq!(
                second
                    .entries()
                    .iter()
                    .filter(|entry| entry.note().is_some())
                    .count(),
                0,
                "deleted note must be removed"
            );
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
            assert_eq!(
                first
                    .entries()
                    .iter()
                    .filter(|entry| entry.note().is_some())
                    .count(),
                1
            );

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

            assert_eq!(
                second
                    .entries()
                    .iter()
                    .filter(|entry| entry.note().is_some())
                    .count(),
                2,
                "new note must be included"
            );
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
                    .entries()
                    .iter()
                    .find(|entry| entry.file().path() == Path::new("note.md"))
                    .and_then(FileEntry::note)
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
                    .entries()
                    .iter()
                    .find(|entry| entry.file().path() == Path::new("note.md"))
                    .and_then(FileEntry::note)
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
            assert_eq!(
                second
                    .entries()
                    .iter()
                    .filter(|entry| entry.note().is_some())
                    .count(),
                2
            );
        }
    }
}
