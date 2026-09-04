//! Index lifecycle service.
//!
//! [`IndexerService`] owns a project root and drives the [`super::FileIndex`]
//! lifecycle through four operations:
//!
//! - [`build`](IndexerService::build): scan and parse all files into a fresh
//!   in-memory index.
//! - [`persist`](IndexerService::persist): write the index to a redb database
//!   at `.traces/index.redb`.
//! - [`load`](IndexerService::load): read a previously-persisted index from
//!   disk.
//! - [`refresh`](IndexerService::refresh): re-scan, diff against persisted
//!   state, and atomically write only changed rows.
//!
//! All disk interaction flows through [`super::store::IndexStore`]; this module
//! owns service-level orchestration, not table-level read/write mechanics.

use std::path::PathBuf;

use super::{
    FileIndex, INDEX_FILE, IndexResult, builder, cache, delta::IndexDelta,
    entry, error::IndexBuilderError, store::IndexStore,
};
use crate::{
    Config, DirTree, DirTreeError, TaskConfig, config::FrontmatterConfig,
    file::FileBase, note::ListRecord,
};

/// Drives the [`FileIndex`] lifecycle for one project root: build, persist,
/// load, and refresh.
///
/// # Lifecycle
///
/// 1. Build a fresh in-memory index: [`Self::build`]
/// 2. Persist to disk: [`Self::persist`]
/// 3. On subsequent runs, load from disk: [`Self::load`]
/// 4. Keep the index current: [`Self::refresh`] (re-scans and persists
///    atomically, best-effort on persist failure)
///
/// # Errors
///
/// All methods return `IndexError`. [`Self::refresh`] also logs a
/// `tracing::warn!` on persist failure without propagating it.

#[derive(Clone, Debug)]
pub struct IndexerService {
    root: PathBuf,
    tasks: TaskConfig,
    frontmatter: FrontmatterConfig,
}

impl IndexerService {
    /// Creates a service scoped to `root`.
    #[inline]
    #[must_use]
    pub fn new<P: Into<PathBuf>>(root: P) -> Self {
        Self {
            root: root.into(),
            tasks: TaskConfig::default(),
            frontmatter: FrontmatterConfig::default(),
        }
    }

    /// Attaches resolved [`Config`] settings for task and frontmatter
    /// classification.
    #[inline]
    #[must_use]
    pub fn with_config(mut self, config: &Config) -> Self {
        self.tasks = config.tasks().clone();
        self.frontmatter = config.frontmatter().clone();
        self
    }

    /// Scans this service's root and builds a [`FileIndex`] in memory.
    ///
    /// # Errors
    ///
    /// - `IndexError::Builder` if a directory cannot be read, a file's metadata
    ///   cannot be inspected, or a Markdown file cannot be parsed.
    #[inline]
    pub fn build(&self) -> IndexResult<FileIndex> {
        let files = self.scan()?;
        Ok(builder::IndexBuilder::new(files)
            .with_tasks(self.tasks.clone())
            .with_frontmatter(self.frontmatter.clone())
            .build(&self.root)?)
    }

    /// Refreshes the persisted index for this service's root against current
    /// filesystem state, persisting the fresh result before returning
    /// (best-effort: a persist failure is logged via `tracing::warn!` and does
    /// not fail this call).
    ///
    /// Re-scans the root and diffs against the previously persisted index:
    ///
    /// - Unchanged Markdown Notes reuse their parsed [`crate::Note`].
    /// - Added or changed Markdown Notes are parsed from disk.
    /// - Deleted files disappear because they are absent from the fresh scan.
    ///
    /// Derived inlinks are recomputed in full when a Note is added or removed,
    /// or when a changed Note's outlink targets actually differ from its
    /// previously-persisted value (backdating skips the recompute otherwise;
    /// see `RefreshCache::reconcile_note`). A full recompute (not a per-note
    /// patch) is required because link target resolution considers every
    /// indexed Note: an unedited Note's *resolved* target can change when an
    /// unrelated Note is added or removed. For example, a wikilink that was
    /// ambiguous becomes resolvable once one of the ambiguous candidates is
    /// deleted.
    ///
    /// # Errors
    ///
    /// - `IndexError::Builder` if a directory cannot be read, a file's metadata
    ///   cannot be inspected, a Markdown file cannot be parsed, or an unchanged
    ///   Note's previous value cannot be recalled.
    /// - `IndexError::Store` if the previously persisted index cannot be
    ///   loaded.
    #[inline]
    pub fn refresh(&self) -> IndexResult<FileIndex> {
        let store = IndexStore::open(&self.root)?;
        let index = {
            let read_txn = store.begin_read()?;
            let cache = cache::RefreshCache::load(&store, &read_txn)?;
            let files = self.scan()?;
            builder::IndexBuilder::new(files)
                .with_cache(cache)
                .with_tasks(self.tasks.clone())
                .with_frontmatter(self.frontmatter.clone())
                .build(&self.root)?
        };
        // read_txn (and cache, which borrows it) drop here, before
        // persist_index opens a write transaction
        if let Err(source) = store.persist_index(&index) {
            tracing::warn!(%source, "failed to persist refreshed index");
        }
        Ok(index)
    }

    /// Persists `index` to this service's root, replacing any existing index.
    ///
    /// # Errors
    ///
    /// - `IndexError::Store` if the database's parent directory cannot be
    ///   created, the transaction fails, or a record cannot be encoded.
    #[inline]
    pub fn persist(&self, index: &FileIndex) -> IndexResult<()> {
        IndexStore::open(&self.root)?.persist_index(index)
    }

    /// Loads the index previously persisted for this service's root, or an
    /// empty [`FileIndex`] if none exists.
    ///
    /// # Errors
    ///
    /// - `IndexError::Store` if the database cannot be read or stored bytes are
    ///   not a valid record.
    #[inline]
    #[cfg_attr(
        not(any(test, feature = "test-utils")),
        expect(
            dead_code,
            reason = "no current caller in cli; kept for IndexerService \
                      lifecycle symmetry and tests"
        )
    )]
    pub fn load(&self) -> IndexResult<FileIndex> {
        let (files, notes, inlinks) =
            IndexStore::open(&self.root)?.read_all()?;
        Ok(FileIndex::new(
            entry::assemble_entries(files, notes, inlinks),
            IndexDelta::Full,
        ))
    }

    /// Reads all persisted [`ListRecord`]s from the `LISTS` table.
    ///
    /// # Errors
    ///
    /// - [`IndexError::Db`] if the database cannot be opened or read.
    #[cfg(any(test, feature = "test-utils"))]
    pub fn read_lists(&self) -> IndexResult<Vec<ListRecord>> {
        let store = IndexStore::open(&self.root)?;
        Ok(store.read_all_lists()?)
    }

    /// Recursively scans this service's root for regular files, skipping `.git`
    /// directories, the index database, and symlinks.
    ///
    /// # Errors
    ///
    /// - [`IndexBuilderError::Scan`] if a directory cannot be read or a file's
    ///   metadata cannot be inspected.
    #[inline]
    pub(super) fn scan(&self) -> Result<Vec<FileBase>, IndexBuilderError> {
        let index_db = self.root.join(INDEX_FILE);
        let mut files = Vec::new();
        let nodes = DirTree::descendants(&self.root)
            .filter(|node| {
                node.file_name() != ".traces"
                    && crate::env_vars::is_ignored_dir(node.file_name())
            })
            .sorted_by(|a, b| a.file_name().cmp(b.file_name()));
        for node in nodes {
            let node = node.map_err(scan_error)?;
            let path = node.path();
            if !node.file_type().is_file() || path == index_db {
                continue;
            }
            let metadata = node.metadata().map_err(scan_error)?;
            files.push(
                FileBase::from_metadata(path, &self.root, &metadata).map_err(
                    |source| IndexBuilderError::Scan {
                        path: path.to_path_buf(),
                        source,
                    },
                )?,
            );
        }
        Ok(files)
    }
}

/// Converts a [`DirTreeError`] into the builder's scan error variant.
fn scan_error(error: DirTreeError) -> IndexBuilderError {
    let (path, source) = error.into_parts();
    IndexBuilderError::Scan {
        path,
        source,
    }
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::{Path, PathBuf},
        sync::Arc,
    };

    use super::{super::IndexError, *};
    use crate::{
        Note,
        file::FileBase,
        index::FileEntry,
        query::{QueryBuilder, QueryService, QuerySet, SourceSelector},
    };

    /// Runs a page-level query via [`QueryService`].
    fn query_pages(
        index: &Arc<FileIndex>,
        source: &SourceSelector,
    ) -> QuerySet {
        QueryService::new("class")
            .run(index, QueryBuilder::pages(source.clone()))
    }

    /// Finds a [`Note`] by project-relative path in `index`.
    fn find_note<'a>(index: &'a FileIndex, path: &str) -> Option<&'a Note> {
        index
            .entries()
            .iter()
            .find(|entry| entry.file().path() == Path::new(path))
            .and_then(FileEntry::note)
    }

    mod build {
        use pretty_assertions::assert_eq;

        use super::*;
        use crate::index::error::IndexBuilderError;

        #[test]
        fn extracts_note_metadata_only_for_markdown_files() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::create_dir_all(temp.path().join("notes")).expect("mkdir notes");
            fs::write(temp.path().join("notes/todo.md"), "- [ ] task 1")
                .expect("write note");
            fs::write(temp.path().join("readme.txt"), "text content")
                .expect("write txt");

            let index =
                IndexerService::new(temp.path()).build().expect("build index");

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
                    .find_map(FileEntry::note)
                    .map(Note::path),
                Some(Path::new("notes/todo.md"))
            );
        }

        #[test]
        fn includes_frontmatter_fields_in_the_indexed_note() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("todo.md"), "---\ntitle: Todo\n---")
                .expect("write note");

            let index =
                IndexerService::new(temp.path()).build().expect("build index");

            assert_eq!(
                find_note(&index, "todo.md")
                    .and_then(Note::frontmatter)
                    .map(|fm| fm.fields().len()),
                Some(1)
            );
        }

        #[test]
        fn includes_tasks_in_the_indexed_note() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("todo.md"), "- [ ] task 1")
                .expect("write note");

            let index =
                IndexerService::new(temp.path()).build().expect("build index");

            assert_eq!(
                find_note(&index, "todo.md")
                    .map(Note::tasks)
                    .map(Iterator::count),
                Some(1)
            );
        }

        #[test]
        fn indexing_never_rewrites_the_source_markdown_file() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let original = "Status:: Draft #urgent\n\n- Reviewer:: Jane \
                            #book\n\n# Heading #chapter\n";
            fs::write(temp.path().join("note.md"), original)
                .expect("write note");

            IndexerService::new(temp.path()).build().expect("build index");

            let after = fs::read_to_string(temp.path().join("note.md"))
                .expect("read note back");
            assert_eq!(after, original);
        }

        #[test]
        fn returns_io_error_when_markdown_file_is_not_utf8() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("bad.md"), [0xFF, 0xFE])
                .expect("write invalid utf8");

            let result = IndexerService::new(temp.path()).build();

            assert!(matches!(
                result,
                Err(IndexError::Builder(IndexBuilderError::NoteParse { .. }))
            ));
        }

        #[test]
        fn sorts_indexed_notes_by_path() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("b.md"), "# B").expect("write b");
            fs::write(temp.path().join("a.md"), "# A").expect("write a");
            let index =
                IndexerService::new(temp.path()).build().expect("build index");

            let paths: Vec<&Path> = index
                .entries()
                .iter()
                .filter_map(FileEntry::note)
                .map(Note::path)
                .collect();
            assert_eq!(paths, [Path::new("a.md"), Path::new("b.md")]);
        }
    }

    mod scan {
        use std::fs;

        use pretty_assertions::assert_eq;

        use super::*;
        #[cfg(unix)]
        use crate::index::tests::fixtures::RestorePermissions;

        fn names(files: &[FileBase]) -> Vec<&Path> {
            files.iter().map(FileBase::path).collect()
        }

        #[test]
        fn scans_nested_files_in_sorted_order() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let root = temp.path();
            fs::create_dir_all(root.join("b")).expect("mkdir b");
            fs::write(root.join("b/one.md"), "1").expect("write b/one.md");
            fs::write(root.join("a.md"), "2").expect("write a.md");

            let files = IndexerService::new(root).scan().expect("scan");

            assert_eq!(names(&files), vec![
                Path::new("a.md"),
                Path::new("b/one.md")
            ]);
        }

        #[test]
        fn orders_sibling_files_and_directories_by_relative_path() {
            // Arrange — `b.txt` sorts AFTER anything inside `b` under
            // component-wise path comparison (`b` < `b.txt`), which matches
            // the walk's name-ordered depth-first traversal.
            let temp = tempfile::tempdir().expect("create temp dir");
            let root = temp.path();
            fs::create_dir_all(root.join("b")).expect("mkdir b");
            fs::write(root.join("b/one.md"), "1").expect("write b/one.md");
            fs::write(root.join("b.txt"), "2").expect("write b.txt");

            let files = IndexerService::new(root).scan().expect("scan");

            assert_eq!(names(&files), vec![
                Path::new("b/one.md"),
                Path::new("b.txt")
            ]);
        }

        #[test]
        fn skips_git_directories() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let root = temp.path();
            fs::create_dir_all(root.join(".git")).expect("mkdir .git");
            fs::write(root.join(".git/HEAD"), "ref: refs/heads/main")
                .expect("write .git/HEAD");
            fs::write(root.join("note.md"), "content").expect("write note.md");

            let files = IndexerService::new(root).scan().expect("scan");

            assert_eq!(names(&files), vec![Path::new("note.md")]);
        }

        #[test]
        fn skips_its_own_index_database_file() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let root = temp.path();
            fs::create_dir_all(root.join(".traces")).expect("mkdir .traces");
            fs::write(root.join(INDEX_FILE), b"redb-bytes")
                .expect("write index db");
            fs::write(root.join("note.md"), "content").expect("write note.md");

            let files = IndexerService::new(root).scan().expect("scan");

            assert_eq!(names(&files), vec![Path::new("note.md")]);
        }

        #[cfg(unix)]
        #[test]
        fn skips_symlinks_entirely() {
            use std::os::unix::fs::symlink;

            let temp = tempfile::tempdir().expect("create temp dir");
            let outside = tempfile::tempdir().expect("create outside dir");
            let root = temp.path();
            let target = outside.path().join("outside.md");
            fs::write(&target, "content").expect("write link target");
            symlink(&target, root.join("link.md")).expect("create symlink");
            fs::write(root.join("note.md"), "content").expect("write note.md");

            let files = IndexerService::new(root).scan().expect("scan");

            assert_eq!(names(&files), vec![Path::new("note.md")]);
        }

        #[test]
        fn empty_root_yields_no_records() {
            let temp = tempfile::tempdir().expect("create temp dir");

            let files = IndexerService::new(temp.path()).scan().expect("scan");

            assert_eq!(files.len(), 0);
        }

        #[cfg(unix)]
        #[test]
        fn returns_an_io_error_when_a_directory_is_unreadable() {
            use std::os::unix::fs::PermissionsExt as _;

            let temp = tempfile::tempdir().expect("create temp dir");
            let root = temp.path();
            let locked = root.join("locked");
            fs::create_dir(&locked).expect("create locked dir");
            fs::set_permissions(&locked, fs::Permissions::from_mode(0o000))
                .expect("revoke read permission");
            let _restore = RestorePermissions(&locked);

            let error = IndexerService::new(root)
                .scan()
                .expect_err("unreadable dir fails");

            assert!(matches!(error, IndexBuilderError::Scan { .. }));
        }
    }

    mod persistence {
        use pretty_assertions::assert_eq;
        use rstest::rstest;

        use super::*;
        use crate::{
            Tag,
            note::{Frontmatter, Link, LinkType, NoteFieldValue},
        };

        /// The `upserted`/`deleted`/link path lists extracted from an
        /// [`IndexDelta::Incremental`]. Distinct from production's
        /// [`delta::IncrementalDelta`]: this borrows the same fields these
        /// tests assert on.
        struct IncrementalPaths<'a> {
            upserted: &'a [PathBuf],
            deleted: &'a [PathBuf],
            links_upserted: Option<&'a [PathBuf]>,
            links_deleted: &'a [PathBuf],
        }

        /// Extracts [`IncrementalPaths`] from an [`IndexDelta::Incremental`],
        /// or `None` for [`IndexDelta::Full`].
        fn incremental_paths(
            delta: &IndexDelta,
        ) -> Option<IncrementalPaths<'_>> {
            match delta {
                IndexDelta::Incremental(delta) => Some(IncrementalPaths {
                    upserted: &delta.upserted,
                    deleted: &delta.deleted,
                    links_upserted: delta.links_upserted.as_deref(),
                    links_deleted: &delta.links_deleted,
                }),
                IndexDelta::Full => None,
            }
        }

        /// Writes three notes (`a.md`/`b.md`/`c.md`) under `root`, builds
        /// and persists the index, and returns the scoped service plus the
        /// initial build's `a`/`c` notes, used to assert they persist
        /// byte-identical after an incremental refresh that only changes
        /// `b.md`.
        fn seed_three_notes(root: &Path) -> (IndexerService, Note, Note) {
            fs::write(root.join("a.md"), "---\ntitle: A\n---\nBody A.")
                .expect("write a");
            fs::write(root.join("b.md"), "---\ntitle: B\n---\nBody B.")
                .expect("write b");
            fs::write(root.join("c.md"), "---\ntitle: C\n---\nBody C.")
                .expect("write c");
            let indexer = IndexerService::new(root);
            let built = indexer.build().expect("build index");
            indexer.persist(&built).expect("persist index");
            let a = find_note(&built, "a.md").expect("note a").clone();
            let c = find_note(&built, "c.md").expect("note c").clone();
            (indexer, a, c)
        }

        #[test]
        fn round_trips_records() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(
                temp.path().join("note.md"),
                "---\ntitle: Hello\n---\n[[other_note]]\n- [x] done",
            )
            .expect("write note");
            let indexer = IndexerService::new(temp.path());
            let built = indexer.build().expect("build index");
            indexer.persist(&built).expect("persist index");
            let loaded = indexer.load().expect("load index");

            assert_eq!(loaded.entries(), built.entries());
        }

        #[test]
        fn round_trips_notes_with_outlinks() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(
                temp.path().join("note.md"),
                "---\ntitle: Hello\n---\n[[other_note]]\n- [x] done",
            )
            .expect("write note");
            let indexer = IndexerService::new(temp.path());
            let built = indexer.build().expect("build index");
            indexer.persist(&built).expect("persist index");
            let loaded = indexer.load().expect("load index");

            assert_eq!(loaded.entries(), built.entries());

            let loaded_note =
                find_note(&loaded, "note.md").expect("loaded note");
            assert_eq!(loaded_note.outlinks().len(), 1);
            assert_eq!(
                loaded_note.outlinks().first().map(Link::target),
                Some("other_note")
            );
        }

        #[test]
        fn round_trips_task_count() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(
                temp.path().join("note.md"),
                "---\ntitle: Hello\n---\n[[other_note]]\n- [x] done",
            )
            .expect("write note");
            let indexer = IndexerService::new(temp.path());
            let built = indexer.build().expect("build index");
            indexer.persist(&built).expect("persist index");
            let loaded = indexer.load().expect("load index");

            let loaded_note =
                find_note(&loaded, "note.md").expect("loaded note");
            assert_eq!(loaded_note.tasks().count(), 1);
        }

        #[test]
        fn persist_then_load_recovers_frontmatter_link_fields() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(
                temp.path().join("note.md"),
                "---\nrelated: \"[[Project Alpha|Alpha]]\"\n---\nBody text.",
            )
            .expect("write note");
            let indexer = IndexerService::new(temp.path());
            let built = indexer.build().expect("build index");
            indexer.persist(&built).expect("persist index");
            let loaded = indexer.load().expect("load index");

            let field = find_note(&loaded, "note.md")
                .and_then(Note::frontmatter)
                .into_iter()
                .flat_map(Frontmatter::fields)
                .find(|(k, _)| k.is_canonical_match("related"))
                .expect("related field");
            assert_eq!(
                field.1,
                &NoteFieldValue::Link(Link::new(
                    "Project Alpha",
                    "Alpha",
                    LinkType::Wikilink
                ))
            );
        }

        #[rstest]
        #[case::body("Status:: Draft", "status", "Draft")]
        #[case::visible_key("[Status:: Draft]", "status", "Draft")]
        #[case::hidden_key("(Status:: Draft)", "status", "Draft")]
        fn persist_then_load_recovers_inline_fields(
            #[case] source: &str,
            #[case] expected_key: &str,
            #[case] expected_value: &str,
        ) {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("note.md"), source).expect("write note");
            let indexer = IndexerService::new(temp.path());
            let built = indexer.build().expect("build index");
            indexer.persist(&built).expect("persist index");
            let loaded = indexer.load().expect("load index");

            let loaded_note =
                find_note(&loaded, "note.md").expect("loaded note");
            let built_note = find_note(&built, "note.md").expect("built note");
            assert_eq!(loaded_note.inline_fields(), built_note.inline_fields());
            let (key, values) = loaded_note
                .inline_fields()
                .iter()
                .next()
                .expect("inline field present");
            assert!(key.is_canonical_match(expected_key));
            assert_eq!(
                values.first().and_then(|v| v.as_str()),
                Some(expected_value)
            );
        }

        #[test]
        fn persist_then_load_recovers_typed_inline_field_values() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(
                temp.path().join("note.md"),
                "[duration:: 7 hours]\n[values:: 1, 2]",
            )
            .expect("write note");
            let indexer = IndexerService::new(temp.path());
            let built = indexer.build().expect("build index");
            indexer.persist(&built).expect("persist index");
            let loaded = indexer.load().expect("load index");

            let loaded_note =
                find_note(&loaded, "note.md").expect("loaded note");
            let built_note = find_note(&built, "note.md").expect("built note");
            assert_eq!(loaded_note.inline_fields(), built_note.inline_fields());
            let values: Vec<&NoteFieldValue> = loaded_note
                .inline_fields()
                .values()
                .flat_map(|vals| vals.iter())
                .collect();
            assert_eq!(values, [
                &NoteFieldValue::Duration("7 hours".to_owned()),
                &NoteFieldValue::List(vec![
                    NoteFieldValue::Number(1.0),
                    NoteFieldValue::Number(2.0)
                ])
            ]);
        }

        #[test]
        fn persist_then_load_recovers_tags() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("note.md"), "Filed under #book today.")
                .expect("write note");
            let indexer = IndexerService::new(temp.path());
            let built = indexer.build().expect("build index");
            indexer.persist(&built).expect("persist index");
            let loaded = indexer.load().expect("load index");

            let loaded_note =
                find_note(&loaded, "note.md").expect("loaded note");
            let built_note = find_note(&built, "note.md").expect("built note");
            assert_eq!(loaded_note.tags(), built_note.tags());
            assert_eq!(loaded_note.tags(), [Tag::parse("#book").unwrap()]);
        }

        #[test]
        fn returns_empty_when_nothing_persisted() {
            let temp = tempfile::tempdir().expect("create temp dir");

            let index =
                IndexerService::new(temp.path()).load().expect("load index");

            assert_eq!(index.entries().len(), 0);
            assert_eq!(
                index
                    .entries()
                    .iter()
                    .filter(|entry| entry.note().is_some())
                    .count(),
                0
            );
        }

        #[test]
        fn persists_rebuilds_rather_than_appends() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("first.md"), "- [ ] first")
                .expect("write first");
            let indexer = IndexerService::new(temp.path());
            indexer
                .persist(&indexer.build().expect("build first index"))
                .expect("persist first index");
            fs::remove_file(temp.path().join("first.md"))
                .expect("remove first");
            fs::write(temp.path().join("second.md"), "- [x] second")
                .expect("write second");

            indexer
                .persist(&indexer.build().expect("build second index"))
                .expect("persist second index");
            let loaded = indexer.load().expect("load index");

            assert_eq!(loaded.entries().len(), 1);
            assert_eq!(
                loaded
                    .entries()
                    .iter()
                    .filter(|entry| entry.note().is_some())
                    .count(),
                1
            );
            assert_eq!(
                loaded
                    .entries()
                    .first()
                    .map(FileEntry::file)
                    .map(FileBase::path),
                Some(Path::new("second.md"))
            );
            assert_eq!(
                loaded
                    .entries()
                    .iter()
                    .find_map(FileEntry::note)
                    .map(Note::path),
                Some(Path::new("second.md"))
            );
        }

        #[test]
        fn incremental_refresh_persist_actually_removes_a_deleted_notes_row_from_disk()
         {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("keep.md"), "# Keep")
                .expect("write keep");
            fs::write(temp.path().join("gone.md"), "# Gone")
                .expect("write gone");
            let indexer = IndexerService::new(temp.path());
            indexer
                .persist(&indexer.build().expect("build index"))
                .expect("persist index");

            fs::remove_file(temp.path().join("gone.md")).expect("delete gone");
            indexer.refresh().expect("refresh persists internally");

            let loaded = indexer.load().expect("load index");
            assert_eq!(loaded.entries().len(), 1);
            assert_eq!(
                loaded
                    .entries()
                    .iter()
                    .filter(|entry| entry.note().is_some())
                    .count(),
                1
            );
            assert!(find_note(&loaded, "gone.md").is_none());
            assert_eq!(
                loaded
                    .entries()
                    .first()
                    .map(FileEntry::file)
                    .map(FileBase::path),
                Some(Path::new("keep.md"))
            );
        }

        #[test]
        fn refresh_delta_names_only_the_changed_path() {
            // Arrange
            let temp = tempfile::tempdir().expect("create temp dir");
            let (indexer, ..) = seed_three_notes(temp.path());
            fs::write(
                temp.path().join("b.md"),
                "---\ntitle: B\n---\nBody B changed.",
            )
            .expect("rewrite b");

            // Act
            let refreshed = indexer.refresh().expect("refresh index");

            // Assert: the delta names only the changed path — proves the
            // refresh plans a row-level write, not a full rewrite.
            let delta = incremental_paths(refreshed.delta())
                .expect("refresh after a persisted build must be incremental");
            assert_eq!(delta.upserted, &[PathBuf::from("b.md")]);
            assert!(delta.deleted.is_empty());
        }

        #[test]
        fn persist_incremental_preserves_unchanged_notes_and_updates_changed_note()
         {
            // Arrange
            let temp = tempfile::tempdir().expect("create temp dir");
            let (indexer, original_a, original_c) =
                seed_three_notes(temp.path());
            fs::write(
                temp.path().join("b.md"),
                "---\ntitle: B\n---\nBody B changed.",
            )
            .expect("rewrite b");
            let refreshed = indexer.refresh().expect("refresh index");

            // Act
            indexer.persist(&refreshed).expect("persist refreshed index");
            let loaded = indexer.load().expect("load index");

            // Assert: the two untouched notes persisted byte-identical to
            // their original write — the incremental write path never
            // touched their rows.
            assert_eq!(find_note(&loaded, "a.md"), Some(&original_a));
            assert_eq!(find_note(&loaded, "c.md"), Some(&original_c));

            // The changed note reflects the rewrite.
            let loaded_b = find_note(&loaded, "b.md").expect("note b");
            assert_eq!(
                loaded_b.frontmatter().and_then(|fm| fm.get("title").cloned()),
                Some(crate::NoteFieldValue::String("B".to_owned()))
            );
        }

        #[test]
        fn noop_refresh_after_incremental_persist_reports_empty_delta() {
            // Arrange
            let temp = tempfile::tempdir().expect("create temp dir");
            let (indexer, ..) = seed_three_notes(temp.path());
            fs::write(
                temp.path().join("b.md"),
                "---\ntitle: B\n---\nBody B changed.",
            )
            .expect("rewrite b");
            let refreshed = indexer.refresh().expect("refresh index");
            indexer.persist(&refreshed).expect("persist refreshed index");

            // Act
            let noop_refresh = indexer.refresh().expect("noop refresh");

            // Assert: a refresh against the now-persisted, unchanged
            // filesystem state reports no further changes.
            let delta = incremental_paths(noop_refresh.delta())
                .expect("refresh after a persisted build must be incremental");
            assert!(delta.upserted.is_empty());
            assert!(delta.deleted.is_empty());
        }

        #[test]
        fn refresh_after_corruption_recovery_reports_every_file_upserted_and_nothing_deleted()
         {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("a.md"), "# A").expect("write a");
            fs::write(temp.path().join("b.md"), "# B").expect("write b");
            let indexer = IndexerService::new(temp.path());
            indexer
                .persist(&indexer.build().expect("build index"))
                .expect("persist index");

            let db_path = temp.path().join(".traces/index.redb");
            // Preserve redb's 9-byte magic number (`page_store::header::
            // MAGICNUMBER`) so opening reaches checksum verification and
            // reports `StorageError::Corrupted`, not the earlier
            // magic-number mismatch path (`StorageError::Io`) a
            // completely-foreign byte sequence would hit instead.
            let mut corrupted = fs::read(&db_path).expect("read valid db");
            corrupted
                .get_mut(9..)
                .expect("db file longer than the 9-byte magic number")
                .fill(0xFF);
            fs::write(&db_path, &corrupted).expect("corrupt the database file");

            let refreshed =
                indexer.refresh().expect("refresh recovers from corruption");

            let delta = incremental_paths(refreshed.delta()).expect(
                "post-recovery refresh must still report an incremental delta",
            );
            let mut upserted = delta.upserted.to_vec();
            upserted.sort();
            assert_eq!(upserted, [
                PathBuf::from("a.md"),
                PathBuf::from("b.md")
            ]);
            assert!(delta.deleted.is_empty());
        }

        #[test]
        fn content_change_without_outlink_change_backdates_and_skips_inlink_recompute()
         {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("target.md"), "# Target")
                .expect("write target");
            fs::write(temp.path().join("linker.md"), "[[target]]\n- [ ] task")
                .expect("write linker");
            let indexer = IndexerService::new(temp.path());
            indexer
                .persist(&indexer.build().expect("build index"))
                .expect("persist index");

            fs::write(temp.path().join("linker.md"), "[[target]]\n- [x] task")
                .expect("rewrite linker: task checked, same outlink");

            let refreshed = indexer.refresh().expect("refresh index");

            let delta = incremental_paths(refreshed.delta())
                .expect("refresh after a persisted build must be incremental");
            assert_eq!(delta.upserted, &[PathBuf::from("linker.md")]);
            assert!(
                delta.links_upserted.is_none(),
                "backdating must skip the inlink recompute when outlinks are \
                 unchanged"
            );
        }

        #[test]
        fn outlink_change_is_not_backdated_and_recomputes_inlinks() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("old-target.md"), "# Old")
                .expect("write old target");
            fs::write(temp.path().join("new-target.md"), "# New")
                .expect("write new target");
            fs::write(temp.path().join("linker.md"), "[[old-target]]")
                .expect("write linker");
            let indexer = IndexerService::new(temp.path());
            indexer
                .persist(&indexer.build().expect("build index"))
                .expect("persist index");

            fs::write(temp.path().join("linker.md"), "[[new-target]]")
                .expect("retarget linker");

            let refreshed = indexer.refresh().expect("refresh index");
            let delta = incremental_paths(refreshed.delta())
                .expect("refresh after a persisted build must be incremental");
            let links_upserted = delta
                .links_upserted
                .expect("outlink change must recompute inlinks");
            assert!(links_upserted.contains(&PathBuf::from("new-target.md")));
            assert!(
                delta.links_deleted.contains(&PathBuf::from("old-target.md"))
            );
        }

        #[test]
        fn reordered_or_relabeled_outlinks_with_same_targets_still_backdates() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("a.md"), "# A").expect("write a");
            fs::write(temp.path().join("b.md"), "# B").expect("write b");
            fs::write(temp.path().join("linker.md"), "[[a]]\n[[b]]")
                .expect("write linker");
            let indexer = IndexerService::new(temp.path());
            indexer
                .persist(&indexer.build().expect("build index"))
                .expect("persist index");

            fs::write(temp.path().join("linker.md"), "[[b|Bee]]\n[[a]]")
                .expect("reorder links and relabel display text, same targets");

            let refreshed = indexer.refresh().expect("refresh index");
            let delta = incremental_paths(refreshed.delta())
                .expect("refresh after a persisted build must be incremental");
            assert!(
                delta.links_upserted.is_none(),
                "same target set in different order/display text must still \
                 backdate"
            );
        }

        #[test]
        fn brand_new_note_always_contributes_to_staleness() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("a.md"), "# A").expect("write a");
            let indexer = IndexerService::new(temp.path());
            indexer
                .persist(&indexer.build().expect("build index"))
                .expect("persist index");

            fs::write(temp.path().join("b.md"), "# B, no links")
                .expect("write new note");

            let refreshed = indexer.refresh().expect("refresh index");
            let delta = incremental_paths(refreshed.delta())
                .expect("refresh after a persisted build must be incremental");
            assert!(
                delta.links_upserted.is_some(),
                "a brand-new note has nothing to backdate against and must \
                 force a recompute"
            );
        }
    }

    mod refresh {
        use std::path::PathBuf;

        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn reparses_a_note_whose_content_and_size_changed() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("note.md"), "- [ ] task")
                .expect("write note");
            let indexer = IndexerService::new(temp.path());
            indexer
                .persist(&indexer.build().expect("build index"))
                .expect("persist index");

            fs::write(temp.path().join("note.md"), "- [ ] task\n- [x] second")
                .expect("rewrite note");

            let refreshed = indexer.refresh().expect("refresh index");
            assert_eq!(
                find_note(&refreshed, "note.md")
                    .map(|note| note.tasks().count()),
                Some(2)
            );
        }

        #[test]
        #[expect(
            clippy::disallowed_methods,
            reason = "not async code; this test waits for the filesystem \
                      mtime to advance after rewriting a file with the same \
                      byte length; tokio::time::sleep does not apply here and \
                      filetime is not worth the dep"
        )]
        fn reparses_a_note_when_modified_timestamp_changes_with_same_file_size()
        {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("note.md"), "Status:: Draft")
                .expect("write note");
            let indexer = IndexerService::new(temp.path());
            indexer
                .persist(&indexer.build().expect("build index"))
                .expect("persist index");

            std::thread::sleep(std::time::Duration::from_millis(15));
            fs::write(temp.path().join("note.md"), "Status:: Final")
                .expect("rewrite note with same byte length");

            let refreshed = indexer.refresh().expect("refresh index");
            let value = find_note(&refreshed, "note.md")
                .and_then(|n| n.inline_fields().iter().next())
                .and_then(|(_, vals)| vals.first())
                .and_then(|v| v.as_str());
            assert_eq!(value, Some("Final"));
        }

        #[test]
        fn includes_newly_added_note() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("first.md"), "# First")
                .expect("write first");
            let indexer = IndexerService::new(temp.path());
            indexer
                .persist(&indexer.build().expect("build index"))
                .expect("persist index");

            fs::write(temp.path().join("second.md"), "# Second")
                .expect("write second");

            let refreshed = indexer.refresh().expect("refresh index");
            assert_eq!(
                refreshed
                    .entries()
                    .iter()
                    .filter(|entry| entry.note().is_some())
                    .count(),
                2
            );
        }

        #[test]
        fn excludes_deleted_note() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("gone.md"), "# Gone")
                .expect("write note");
            let indexer = IndexerService::new(temp.path());
            indexer
                .persist(&indexer.build().expect("build index"))
                .expect("persist index");

            fs::remove_file(temp.path().join("gone.md")).expect("delete note");

            let refreshed = indexer.refresh().expect("refresh index");
            assert_eq!(
                refreshed
                    .entries()
                    .iter()
                    .filter(|entry| entry.note().is_some())
                    .count(),
                0
            );
            assert_eq!(refreshed.entries().len(), 0);
        }

        #[test]
        fn excludes_inlink_after_linker_deletion() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("target.md"), "# Target")
                .expect("write target");
            fs::write(temp.path().join("linker.md"), "[[target]]")
                .expect("write linker");
            let indexer = IndexerService::new(temp.path());
            indexer
                .persist(&indexer.build().expect("build index"))
                .expect("persist index");

            fs::remove_file(temp.path().join("linker.md"))
                .expect("delete linker");

            let refreshed = Arc::new(indexer.refresh().expect("refresh index"));
            let outcome = query_pages(&refreshed, &SourceSelector::All);
            let target = outcome.iter().next().expect("target record");

            assert_eq!(target.file().path(), Path::new("target.md"));
            assert!(target.inlinks().is_empty());
        }

        #[test]
        fn moves_inlink_when_linker_retargets() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("old-target.md"), "# Old")
                .expect("write old target");
            fs::write(temp.path().join("new-target.md"), "# New")
                .expect("write new target");
            fs::write(temp.path().join("linker.md"), "[[old-target]]")
                .expect("write linker");
            let indexer = IndexerService::new(temp.path());
            indexer
                .persist(&indexer.build().expect("build index"))
                .expect("persist index");

            fs::write(temp.path().join("linker.md"), "[[new-target]]")
                .expect("repoint linker");

            let refreshed = Arc::new(indexer.refresh().expect("refresh index"));
            let outcome = query_pages(&refreshed, &SourceSelector::All);
            let old_target = outcome
                .iter()
                .find(|record| {
                    record.file().path() == Path::new("old-target.md")
                })
                .expect("old target record");
            let new_target = outcome
                .iter()
                .find(|record| {
                    record.file().path() == Path::new("new-target.md")
                })
                .expect("new target record");

            assert!(old_target.inlinks().is_empty());
            assert_eq!(new_target.inlinks(), [PathBuf::from("linker.md")]);
        }

        #[test]
        fn persists_refreshed_changes_survive_load() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("note.md"), "# Draft")
                .expect("write note");
            let indexer = IndexerService::new(temp.path());
            indexer
                .persist(&indexer.build().expect("build index"))
                .expect("persist index");

            fs::write(temp.path().join("extra.md"), "# Extra")
                .expect("write extra");
            indexer
                .persist(&indexer.refresh().expect("refresh index"))
                .expect("persist index");

            let loaded = indexer.load().expect("load index");
            assert_eq!(
                loaded
                    .entries()
                    .iter()
                    .filter(|entry| entry.note().is_some())
                    .count(),
                2
            );
        }

        #[test]
        fn builds_an_index_when_nothing_was_persisted_yet() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("note.md"), "# Note")
                .expect("write note");

            let refreshed = IndexerService::new(temp.path())
                .refresh()
                .expect("refresh index");
            assert_eq!(
                refreshed
                    .entries()
                    .iter()
                    .filter(|entry| entry.note().is_some())
                    .count(),
                1
            );
        }

        #[test]
        fn preserves_inlinks_after_noop_refresh() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("target.md"), "# Target")
                .expect("write target");
            fs::write(temp.path().join("linker.md"), "[[target]]")
                .expect("write linker");
            let indexer = IndexerService::new(temp.path());
            indexer
                .persist(&indexer.build().expect("build index"))
                .expect("persist index");

            let refreshed = Arc::new(indexer.refresh().expect("refresh index"));
            let outcome = query_pages(&refreshed, &SourceSelector::All);
            let target = outcome
                .iter()
                .find(|record| record.file().path() == Path::new("target.md"))
                .expect("target record");

            assert_eq!(target.inlinks(), [PathBuf::from("linker.md")]);
        }

        #[test]
        fn refresh_persists_so_a_fresh_load_reflects_the_change() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("note.md"), "---\ntitle: Draft\n---")
                .expect("write note");
            let indexer = IndexerService::new(temp.path());
            indexer
                .persist(&indexer.build().expect("build index"))
                .expect("persist index");

            fs::write(temp.path().join("note.md"), "# Revised")
                .expect("rewrite note");

            let refreshed = indexer.refresh().expect("refresh index");

            // The refreshed index reflects the new content...
            assert_eq!(
                find_note(&refreshed, "note.md")
                    .and_then(Note::frontmatter)
                    .and_then(|fm| fm.fields().values().next())
                    .and_then(|v| v.as_str()),
                None // "# Revised" has no frontmatter
            );
            // ...and refresh() persists internally, so a fresh load from
            // disk reflects the same revised content without a separate
            // `persist()` call.
            let loaded = indexer.load().expect("load index");
            assert_eq!(
                find_note(&loaded, "note.md")
                    .and_then(Note::frontmatter)
                    .and_then(|fm| fm.fields().values().next())
                    .and_then(|v| v.as_str()),
                None // "# Revised" content, persisted by refresh() itself
            );
        }

        #[test]
        fn resolves_stale_ambiguous_wikilink_after_unrelated_deletion() {
            // `a.md`'s own bytes never change in this test. Its `[[foo]]`
            // link starts ambiguous (two Notes named `foo`) and later
            // becomes resolvable purely because a *different* Note is
            // deleted. `refresh`'s per-file staleness check would mark
            // `a.md` "unchanged, reused" and skip re-parsing it — proving
            // inlinks must come from a full recompute over every indexed
            // Note (deleting a Note always forces one, per
            // `RefreshCache::diff_files`), not a patch limited to the
            // notes `refresh` actually re-parsed.
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::create_dir_all(temp.path().join("notes")).expect("mkdir notes");
            fs::create_dir_all(temp.path().join("archive"))
                .expect("mkdir archive");
            fs::write(temp.path().join("notes/foo.md"), "# Foo")
                .expect("write notes/foo.md");
            fs::write(temp.path().join("archive/foo.md"), "# Old Foo")
                .expect("write archive/foo.md");
            fs::write(temp.path().join("a.md"), "[[foo]]").expect("write a");
            let indexer = IndexerService::new(temp.path());
            indexer
                .persist(&indexer.build().expect("build index"))
                .expect("persist index");

            fs::remove_file(temp.path().join("archive/foo.md"))
                .expect("delete archive/foo.md");

            let refreshed = Arc::new(indexer.refresh().expect("refresh index"));
            let outcome = query_pages(&refreshed, &SourceSelector::All);
            let target = outcome
                .iter()
                .find(|record| {
                    record.file().path() == Path::new("notes/foo.md")
                })
                .expect("notes/foo.md record");

            assert_eq!(target.inlinks(), [PathBuf::from("a.md")]);
        }
    }
}
