//! Scan, persist, load, and refresh a file index over a project root.
//!
//! [`IndexerService`] owns a project root and drives the index lifecycle:
//! build, persist, load, and refresh. [`FileIndex`] is the value it
//! produces — a snapshot of every indexed [`FileRecord`] (from
//! [`crate::file`]), each Markdown file's parsed [`Note`], and derived
//! inbound links. `FileIndex` carries no `&Path` of its own; construction and
//! persistence flow entirely through [`IndexerService`].
//!
//! Query execution lives in [`crate::query`]: `QueryService` consumes a
//! [`FileIndex`]'s decomposed parts ([`FileIndex::into_parts`]) rather than
//! `FileIndex` depending on the query domain, keeping `index` and `query`
//! free of a mutual dependency.
//!
//! Persistence uses a redb-backed database managed by the [`store`]
//! submodule; callers use [`IndexerService`]'s methods instead of touching
//! redb tables directly.
//!
//! Inbound links between Notes are derived from outlinks during build and
//! refresh, then persisted alongside them; see [`inlinks`].
//!
//! The build pipeline is composed internally by [`builder::IndexBuilder`],
//! which holds a scan result and reuse directive, deferring note parsing,
//! sorting, and inlink derivation to build time.
//!
//! # Lifecycle
//!
//! - Build a fresh index: [`IndexerService::build`]
//! - Persist to disk: [`IndexerService::persist`]
//! - Load from disk: [`IndexerService::load`]
//! - Refresh against the filesystem: [`IndexerService::refresh`]
//!
//! # Inspecting a [`FileIndex`]
//!
//! - [`FileIndex::records`] and [`FileIndex::notes`] expose sorted indexed data
//!   for direct inspection.
//! - [`FileIndex::into_parts`] decomposes a `FileIndex` into its records,
//!   notes, and inbound-link map for `crate::query::QueryService`.
//!
//! [`store`]: mod@store
//! [`inlinks`]: mod@inlinks
//! [`builder::IndexBuilder`]: mod@builder

mod builder;
mod error;
mod inlinks;
mod scan;
mod store;

use std::path::{Path, PathBuf};

#[allow(unused_imports, reason = "re-exported for downstream callers")]
pub use error::{FileIndexError, IndexBuilderError};
pub(crate) use inlinks::InlinkMap;
use store::IndexStore;

pub(crate) use crate::file::FileFormat;
pub use crate::file::FileRecord;
use crate::note::Note;

/// Project-relative path of the persisted [`FileIndex`] database.
const INDEX_FILE: &str = ".traces/index.redb";

/// Drives the [`FileIndex`] lifecycle for one project root: build, persist,
/// load, and refresh.
///
/// Mirrors `ConfigService`/`SchemaService`: a fixed-configuration service
/// (here, the project root) with methods that read or write against it,
/// rather than a bare `root: &Path` parameter repeated at every call site.
///
/// # Examples
///
/// ```ignore
/// # use traces_pkm::index::IndexerService;
/// let indexer = IndexerService::new("/path/to/project");
/// let index = indexer.build().expect("build index");
/// indexer.persist(&index).expect("persist index");
/// ```
#[derive(Clone, Debug)]
pub struct IndexerService {
    root: PathBuf,
}

impl IndexerService {
    /// Creates a service scoped to `root`.
    #[inline]
    #[must_use]
    pub fn new<P: Into<PathBuf>>(root: P) -> Self {
        Self {
            root: root.into(),
        }
    }

    /// Scans this service's root and builds a [`FileIndex`] in memory.
    ///
    /// Markdown files are parsed into [`Note`] records. The index is not
    /// persisted until [`Self::persist`] is called.
    ///
    /// # Errors
    ///
    /// - [`FileIndexError::Io`] if a directory cannot be read, a file's
    ///   metadata cannot be inspected, or a markdown file cannot be read.
    #[inline]
    pub fn build(&self) -> Result<FileIndex, FileIndexError> {
        Ok(builder::IndexBuilder::from_scan(&self.root)?.build(&self.root)?)
    }

    /// Refreshes the persisted index for this service's root against current
    /// filesystem state.
    ///
    /// Re-scans the root and compares each current file's `(created_at,
    /// modified_at, size)` tuple against the previously persisted
    /// [`FileRecord`]:
    ///
    /// - Unchanged markdown Notes reuse their parsed [`Note`].
    /// - Added or changed markdown Notes are parsed from disk.
    /// - Deleted files disappear because they are absent from the fresh scan.
    ///
    /// Returns the fresh [`FileIndex`] without persisting. Call
    /// [`Self::persist`] to write the result to disk.
    ///
    /// Derived inlinks are recomputed in full whenever a Note's content or
    /// metadata changed since the last persist. A full recompute (not a
    /// per-note patch) is required because link target resolution considers
    /// every indexed Note: an unedited Note's *resolved* target can change
    /// when an unrelated Note is added or removed. For example, a wikilink
    /// that was ambiguous becomes resolvable once one of the ambiguous
    /// candidates is deleted.
    ///
    /// # Errors
    ///
    /// - [`FileIndexError::Io`] if a directory cannot be read, a file's
    ///   metadata cannot be inspected, or a markdown file cannot be read.
    /// - [`FileIndexError::Store`] or [`FileIndexError::Deserialize`] if the
    ///   previous index cannot be loaded.
    #[inline]
    pub fn refresh(&self) -> Result<FileIndex, FileIndexError> {
        let previous = self.load()?;
        Ok(builder::IndexBuilder::from_scan(&self.root)?
            .reuse_unchanged(previous)
            .build(&self.root)?)
    }

    /// Persists `index` to this service's root, replacing any existing index
    /// contents.
    ///
    /// [`FileRecord`], [`Note`], and derived inlink records are all written
    /// atomically.
    ///
    /// # Errors
    ///
    /// - [`FileIndexError::Io`] if the database's parent directory cannot be
    ///   created.
    /// - [`FileIndexError::Store`] if the database transaction fails.
    /// - [`FileIndexError::Serialize`] if a record cannot be encoded.
    #[inline]
    pub fn persist(&self, index: &FileIndex) -> Result<(), FileIndexError> {
        IndexStore::open(&self.root)?.replace_all(
            &index.records,
            &index.notes,
            &index.inlinks,
        )
    }

    /// Loads the index previously persisted for this service's root.
    ///
    /// Returns an empty [`FileIndex`] if no index has been persisted yet.
    ///
    /// # Errors
    ///
    /// - [`FileIndexError::Store`] if the database cannot be read.
    /// - [`FileIndexError::Deserialize`] if stored bytes are not a valid
    ///   record.
    #[inline]
    pub fn load(&self) -> Result<FileIndex, FileIndexError> {
        let (records, notes, inlinks) =
            IndexStore::open(&self.root)?.load_all()?;
        Ok(FileIndex {
            records,
            notes,
            inlinks,
        })
    }
}

/// Persisted cache of file records, parsed Note metadata, and derived inbound
/// links.
///
/// Every regular file under the project root contributes a [`FileRecord`].
/// Markdown files also contribute a [`Note`], accessible through
/// [`Self::notes`]. A pure value type: [`IndexerService`] produces, persists,
/// and loads it; `FileIndex` itself carries no `&Path`.
#[derive(Clone, Debug)]
pub struct FileIndex {
    records: Vec<FileRecord>,
    notes: Vec<Note>,
    /// Inbound links, keyed by target path; see [`inlinks::derive_inlinks`].
    ///
    /// Recomputed in full whenever [`IndexerService::refresh`] finds changed
    /// content. Reused unchanged from the last persisted computation
    /// otherwise.
    inlinks: InlinkMap,
}

impl FileIndex {
    /// Returns indexed [`FileRecord`]s, sorted by path.
    ///
    /// Every regular file under the project root contributes one record.
    /// Markdown files also have a corresponding [`Note`] accessible via
    /// [`Self::notes`].
    #[inline]
    #[must_use]
    pub fn records(&self) -> &[FileRecord] {
        &self.records
    }

    /// Returns indexed [`Note`] records, sorted by path.
    ///
    /// Only markdown files produce notes. Non-markdown files appear in
    /// [`Self::records`] but not here.
    #[inline]
    #[must_use]
    #[cfg_attr(
        not(any(test, feature = "test-utils")),
        expect(
            dead_code,
            reason = "no current caller outside tests; CLI exposes \
                      FileIndex::records but not the parsed Note view yet"
        )
    )]
    pub fn notes(&self) -> &[Note] {
        &self.notes
    }

    /// Returns the [`Note`] for the note at `path`, if indexed.
    ///
    /// # Performance
    ///
    /// O(log n): [`Self::notes`] is kept sorted by path, so this binary
    /// searches rather than scanning.
    #[inline]
    #[must_use]
    #[cfg(test)]
    pub(crate) fn note(&self, path: &Path) -> Option<&Note> {
        find_by_path(&self.notes, path)
    }

    /// Consumes this index and returns its inner components: sorted
    /// [`FileRecord`]s, sorted [`Note`]s, and the derived inbound-link map.
    ///
    /// `crate::query::QueryService` consumes these directly instead of
    /// depending on `FileIndex`, keeping `index` and `query` free of a mutual
    /// dependency.
    #[inline]
    #[must_use]
    pub fn into_parts(self) -> (Vec<FileRecord>, Vec<Note>, InlinkMap) {
        (self.records, self.notes, self.inlinks)
    }
}

/// Binary-searches path-sorted `notes` for an exact path match.
///
/// Shared by the [`inlinks`] submodule, which needs the same search over a
/// bare `&[Note]` slice while resolving link targets during
/// [`IndexerService::build`]/[`IndexerService::refresh`].
///
/// [`inlinks`]: mod@inlinks
fn find_by_path<'a>(notes: &'a [Note], path: &Path) -> Option<&'a Note> {
    let idx = notes.binary_search_by(|note| note.path().cmp(path)).ok()?;
    notes.get(idx)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::query::{
        QueryRecord, QueryRecordSet, QueryService, QuerySource,
    };

    /// Runs a page-level query via [`QueryService`].
    fn query_all(index: FileIndex, source: &QuerySource) -> QueryRecordSet {
        let (records, notes, inlinks) = index.into_parts();
        QueryService::new("class").query(records, notes, inlinks, source)
    }

    /// Task-level counterpart to [`query_all`].
    fn query_tasks_all(
        index: FileIndex,
        source: &QuerySource,
    ) -> QueryRecordSet {
        let (records, notes, inlinks) = index.into_parts();
        QueryService::new("class").query_tasks(records, notes, inlinks, source)
    }
    /// Shared test fixtures live here so `scan.rs` and `store.rs` tests can
    /// import them without duplicating the definitions.
    pub(crate) mod fixtures {
        use std::{fs, path::Path};

        /// Restores a locked directory's permissions on drop, even if the
        /// test panics. Otherwise, a `0o000` or `0o500` directory blocks the
        /// tempdir's own cleanup.
        #[cfg(unix)]
        pub struct RestorePermissions<'a>(pub &'a Path);

        #[cfg(unix)]
        impl Drop for RestorePermissions<'_> {
            fn drop(&mut self) {
                use std::os::unix::fs::PermissionsExt as _;

                let _ = fs::set_permissions(
                    self.0,
                    fs::Permissions::from_mode(0o700),
                );
            }
        }
    }

    mod build {
        use pretty_assertions::assert_eq;

        use super::*;

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

            assert_eq!(index.records().len(), 2);
            assert_eq!(index.notes().len(), 1);
            assert_eq!(
                index.notes().first().map(Note::path),
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
                index
                    .note(Path::new("todo.md"))
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
                index
                    .note(Path::new("todo.md"))
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

            assert!(matches!(result, Err(FileIndexError::Io { .. })));
        }

        #[test]
        fn sorts_indexed_notes_by_path() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("b.md"), "# B").expect("write b");
            fs::write(temp.path().join("a.md"), "# A").expect("write a");
            let index =
                IndexerService::new(temp.path()).build().expect("build index");

            let paths: Vec<&Path> =
                index.notes().iter().map(Note::path).collect();
            assert_eq!(paths, [Path::new("a.md"), Path::new("b.md")]);
        }
    }

    mod persistence {
        use pretty_assertions::assert_eq;
        use rstest::rstest;

        use super::*;
        use crate::note::{Frontmatter, Link, LinkType, NoteFieldValue, Tag};

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

            assert_eq!(loaded.records(), built.records());
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

            assert_eq!(loaded.notes(), built.notes());

            let loaded_note =
                loaded.note(Path::new("note.md")).expect("loaded note");
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
                loaded.note(Path::new("note.md")).expect("loaded note");
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

            let field = loaded
                .note(Path::new("note.md"))
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
                loaded.note(Path::new("note.md")).expect("loaded note");
            let built_note =
                built.note(Path::new("note.md")).expect("built note");
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
                loaded.note(Path::new("note.md")).expect("loaded note");
            let built_note =
                built.note(Path::new("note.md")).expect("built note");
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
                loaded.note(Path::new("note.md")).expect("loaded note");
            let built_note =
                built.note(Path::new("note.md")).expect("built note");
            assert_eq!(loaded_note.tags(), built_note.tags());
            assert_eq!(loaded_note.tags(), [Tag::new("#book")]);
        }

        #[test]
        fn returns_empty_when_nothing_persisted() {
            let temp = tempfile::tempdir().expect("create temp dir");

            let index =
                IndexerService::new(temp.path()).load().expect("load index");

            assert_eq!(index.records().len(), 0);
            assert_eq!(index.notes().len(), 0);
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

            assert_eq!(loaded.records().len(), 1);
            assert_eq!(loaded.notes().len(), 1);
            assert_eq!(
                loaded.records().first().map(FileRecord::path),
                Some(Path::new("second.md"))
            );
            assert_eq!(
                loaded.notes().first().map(Note::path),
                Some(Path::new("second.md"))
            );
        }
    }

    mod lookup {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn returns_none_when_note_path_is_not_indexed() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let index =
                IndexerService::new(temp.path()).build().expect("build index");

            assert_eq!(index.note(Path::new("nonexistent.md")), None);
        }

        #[test]
        fn returns_the_matching_note_when_path_is_indexed() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("a.md"), "# A").expect("write a");
            fs::write(temp.path().join("b.md"), "# B").expect("write b");
            fs::write(temp.path().join("c.md"), "# C").expect("write c");
            let index =
                IndexerService::new(temp.path()).build().expect("build index");

            assert_eq!(
                index.note(Path::new("b.md")).map(Note::path),
                Some(Path::new("b.md"))
            );
        }
    }

    mod builder {
        use pretty_assertions::assert_eq;

        use super::{super::builder::IndexBuilder, *};

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
                index
                    .records()
                    .iter()
                    .map(FileRecord::path)
                    .collect::<Vec<_>>(),
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

            assert_eq!(index.records().len(), 2);
            assert_eq!(index.notes().len(), 1);
            assert_eq!(
                index.note(Path::new("note.md")).map(Note::path),
                Some(Path::new("note.md"))
            );
        }

        #[test]
        fn reuse_unchanged_skips_reparsing() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("note.md"), "- [ ] task")
                .expect("write note");
            let built =
                IndexerService::new(temp.path()).build().expect("build index");

            let index = IndexBuilder::from_scan(temp.path())
                .expect("scan")
                .reuse_unchanged(built)
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
        fn reuse_unchanged_reparses_changed_notes() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("note.md"), "- [ ] task")
                .expect("write note");
            let built =
                IndexerService::new(temp.path()).build().expect("build index");

            fs::write(temp.path().join("note.md"), "- [ ] task\n- [x] done")
                .expect("rewrite note");

            let index = IndexBuilder::from_scan(temp.path())
                .expect("scan")
                .reuse_unchanged(built)
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
                refreshed
                    .note(Path::new("note.md"))
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
            let value = refreshed
                .note(Path::new("note.md"))
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
            assert_eq!(refreshed.notes().len(), 2);
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
            assert_eq!(refreshed.notes().len(), 0);
            assert_eq!(refreshed.records().len(), 0);
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

            let refreshed = indexer.refresh().expect("refresh index");
            let outcome = query_all(refreshed, &QuerySource::All);
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

            let refreshed = indexer.refresh().expect("refresh index");
            let outcome = query_all(refreshed, &QuerySource::All);
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
            assert_eq!(loaded.notes().len(), 2);
        }

        #[test]
        fn builds_an_index_when_nothing_was_persisted_yet() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("note.md"), "# Note")
                .expect("write note");

            let refreshed = IndexerService::new(temp.path())
                .refresh()
                .expect("refresh index");
            assert_eq!(refreshed.notes().len(), 1);
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

            let refreshed = indexer.refresh().expect("refresh index");
            let outcome = query_all(refreshed, &QuerySource::All);
            let target = outcome
                .iter()
                .find(|record| record.file().path() == Path::new("target.md"))
                .expect("target record");

            assert_eq!(target.inlinks(), [PathBuf::from("linker.md")]);
        }

        #[test]
        fn returns_unpersisted_index_from_refresh() {
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
                refreshed
                    .note(Path::new("note.md"))
                    .and_then(Note::frontmatter)
                    .and_then(|fm| fm.fields().values().next())
                    .and_then(|v| v.as_str()),
                None // "# Revised" has no frontmatter
            );
            // ...but a fresh load from disk still shows the OLD content,
            // because refresh did not persist.
            let loaded = indexer.load().expect("load index");
            assert_eq!(
                loaded
                    .note(Path::new("note.md"))
                    .and_then(Note::frontmatter)
                    .and_then(|fm| fm.fields().values().next())
                    .and_then(|v| v.as_str()),
                Some("Draft") // OLD frontmatter, not the revised content
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
            // Note (gated on whether *anything* changed), not a patch
            // limited to the notes `refresh` actually re-parsed.
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

            let refreshed = indexer.refresh().expect("refresh index");
            let outcome = query_all(refreshed, &QuerySource::All);
            let target = outcome
                .iter()
                .find(|record| {
                    record.file().path() == Path::new("notes/foo.md")
                })
                .expect("notes/foo.md record");

            assert_eq!(target.inlinks(), [PathBuf::from("a.md")]);
        }
    }

    mod query {
        use std::path::PathBuf;

        use pretty_assertions::assert_eq;

        use super::*;
        use crate::note::Tag;

        fn note_paths(outcome: &QueryRecordSet) -> Vec<&Path> {
            outcome
                .iter()
                .filter_map(|record| record.note().map(Note::path))
                .collect()
        }

        fn build_book_index() -> FileIndex {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(
                temp.path().join("book.md"),
                "---\ntitle: Dune\n---\nGenre:: Sci-fi\n\nShelved as #book.",
            )
            .expect("write note");
            IndexerService::new(temp.path()).build().expect("build index")
        }

        #[test]
        fn returns_all_files_in_sorted_order() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("a.md"), "# A").expect("write a");
            fs::write(temp.path().join("b.md"), "# B").expect("write b");
            fs::write(temp.path().join("readme.txt"), "text")
                .expect("write txt");
            let index =
                IndexerService::new(temp.path()).build().expect("build index");
            let outcome = query_all(index, &QuerySource::All);

            assert_eq!(outcome.len(), 3);
            assert_eq!(
                outcome.get(0).map(|r| r.file().path()),
                Some(Path::new("a.md"))
            );
            assert_eq!(
                outcome.get(1).map(|r| r.file().path()),
                Some(Path::new("b.md"))
            );
            assert_eq!(
                outcome.get(2).map(|r| r.file().path()),
                Some(Path::new("readme.txt"))
            );
            assert!(outcome.get(3).is_none());
        }

        #[test]
        fn excludes_non_markdown_files_from_note_results() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("a.md"), "# A").expect("write a");
            fs::write(temp.path().join("readme.txt"), "text")
                .expect("write txt");
            let index =
                IndexerService::new(temp.path()).build().expect("build index");
            let outcome = query_all(index, &QuerySource::All);

            assert_eq!(note_paths(&outcome), [Path::new("a.md")]);
            assert_eq!(outcome.get(1).and_then(|r| r.note()), None);
        }

        #[test]
        fn returns_empty_when_no_notes_match_tag() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("readme.txt"), "text")
                .expect("write txt");
            let index =
                IndexerService::new(temp.path()).build().expect("build index");
            let outcome = query_all(
                index,
                &QuerySource::parse("#missing").expect("valid source"),
            );

            assert_eq!(outcome.len(), 0);
        }

        #[test]
        fn returns_matching_note_when_tag_source_is_exact() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("book.md"), "Filed under #book.")
                .expect("write book");
            fs::write(temp.path().join("other.md"), "No tags here.")
                .expect("write other");
            let index =
                IndexerService::new(temp.path()).build().expect("build index");
            let outcome = query_all(
                index,
                &QuerySource::parse("#book").expect("valid source"),
            );

            assert_eq!(note_paths(&outcome), [Path::new("book.md")]);
        }

        #[test]
        fn returns_matching_note_when_tag_source_is_nested() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(
                temp.path().join("project.md"),
                "Tracked in #projects/active.",
            )
            .expect("write project");
            fs::write(temp.path().join("other.md"), "No tags here.")
                .expect("write other");
            let index =
                IndexerService::new(temp.path()).build().expect("build index");
            let exact = query_all(
                index.clone(),
                &QuerySource::parse("#projects/active").expect("valid source"),
            );
            let parent = query_all(
                index,
                &QuerySource::parse("#projects").expect("valid source"),
            );

            assert_eq!(note_paths(&exact), [Path::new("project.md")]);
            assert_eq!(note_paths(&parent), [Path::new("project.md")]);
        }

        #[test]
        fn returns_empty_when_tag_query_is_too_specific() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("project.md"), "Tracked in #projects.")
                .expect("write project");
            let index =
                IndexerService::new(temp.path()).build().expect("build index");
            let outcome = query_all(
                index,
                &QuerySource::parse("#projects/active").expect("valid source"),
            );

            assert!(outcome.is_empty());
        }

        #[test]
        fn returns_notes_at_and_under_folder_when_source_is_folder() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::create_dir_all(temp.path().join("books/fiction"))
                .expect("mkdir books/fiction");
            fs::write(temp.path().join("books/dune.md"), "# Dune")
                .expect("write dune");
            fs::write(temp.path().join("books/fiction/hobbit.md"), "# Hobbit")
                .expect("write hobbit");
            fs::write(temp.path().join("other.md"), "# Other")
                .expect("write other");
            let index =
                IndexerService::new(temp.path()).build().expect("build index");
            let outcome = query_all(
                index,
                &QuerySource::parse("books/").expect("valid source"),
            );

            assert_eq!(note_paths(&outcome), [
                Path::new("books/dune.md"),
                Path::new("books/fiction/hobbit.md")
            ]);
        }

        #[test]
        fn returns_file_path_for_each_record() {
            let index = build_book_index();

            let outcome = query_all(index, &QuerySource::All);
            let record = outcome.iter().next().expect("one record");

            assert_eq!(record.file().path(), Path::new("book.md"));
        }

        #[test]
        fn includes_frontmatter_fields_in_note() {
            let index = build_book_index();

            let outcome = query_all(index, &QuerySource::All);
            let note = outcome
                .iter()
                .next()
                .expect("one record")
                .note()
                .expect("note");

            assert_eq!(note.frontmatter().map(|fm| fm.fields().len()), Some(1));
        }

        #[test]
        fn includes_inline_field_keys() {
            let index = build_book_index();

            let outcome = query_all(index, &QuerySource::All);
            let note = outcome
                .iter()
                .next()
                .expect("one record")
                .note()
                .expect("note");

            assert_eq!(
                note.inline_fields()
                    .iter()
                    .map(|(key, _)| key.canonical())
                    .collect::<Vec<_>>(),
                ["genre"]
            );
        }

        #[test]
        fn includes_note_tags() {
            let index = build_book_index();

            let outcome = query_all(index, &QuerySource::All);
            let note = outcome
                .iter()
                .next()
                .expect("one record")
                .note()
                .expect("note");

            assert_eq!(note.tags(), [Tag::new("#book")]);
        }

        #[test]
        fn derives_inlinks_from_multiple_notes_linking_to_the_same_target() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("target.md"), "# Target")
                .expect("write target");
            fs::write(temp.path().join("a.md"), "[[target]]").expect("write a");
            fs::write(temp.path().join("b.md"), "[[target]]").expect("write b");
            let index =
                IndexerService::new(temp.path()).build().expect("build index");
            let outcome = query_all(index, &QuerySource::All);
            let target = outcome
                .iter()
                .find(|record| record.file().path() == Path::new("target.md"))
                .expect("target record");

            assert_eq!(target.inlinks(), [
                PathBuf::from("a.md"),
                PathBuf::from("b.md")
            ]);
        }

        #[test]
        fn includes_a_linking_note_outside_the_source_in_the_targets_inlinks() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("target.md"), "#book\n")
                .expect("write target");
            fs::write(temp.path().join("linker.md"), "[[target]]")
                .expect("write linker");
            let index =
                IndexerService::new(temp.path()).build().expect("build index");
            let outcome = query_all(
                index,
                &QuerySource::parse("#book").expect("valid source"),
            );
            let target = outcome.iter().next().expect("target record");

            assert_eq!(target.file().path(), Path::new("target.md"));
            assert_eq!(target.inlinks(), [PathBuf::from("linker.md")]);
        }

        #[test]
        fn deduplicates_outlinks_from_same_source() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("target.md"), "# Target")
                .expect("write target");
            fs::write(
                temp.path().join("a.md"),
                "[[target]] and [[target]] again",
            )
            .expect("write a");
            let index =
                IndexerService::new(temp.path()).build().expect("build index");
            let outcome = query_all(index, &QuerySource::All);
            let target = outcome
                .iter()
                .find(|record| record.file().path() == Path::new("target.md"))
                .expect("target record");

            assert_eq!(target.inlinks(), [PathBuf::from("a.md")]);
        }

        #[test]
        fn preserves_a_self_linking_notes_own_inlink() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("b.md"), "[[b]]").expect("write b");
            let index =
                IndexerService::new(temp.path()).build().expect("build index");
            let outcome = query_all(index, &QuerySource::All);
            let source = outcome
                .iter()
                .find(|record| record.file().path() == Path::new("b.md"))
                .expect("self-linking record");

            assert_eq!(source.inlinks(), [PathBuf::from("b.md")]);
        }

        #[test]
        fn derives_inlinks_from_outlinks() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("target.md"), "# Target")
                .expect("write target");
            fs::write(temp.path().join("linker.md"), "[[target]]")
                .expect("write linker");

            let index =
                IndexerService::new(temp.path()).build().expect("build index");
            let outcome = query_all(index, &QuerySource::All);
            let target = outcome
                .iter()
                .find(|r| r.file().path() == Path::new("target.md"))
                .expect("target record");

            assert_eq!(target.inlinks(), [PathBuf::from("linker.md")]);
        }
    }

    mod query_tasks {
        use std::path::PathBuf;

        use pretty_assertions::assert_eq;

        use super::*;

        /// `(completed, text)` pairs for every row in `outcome`, in order.
        fn task_rows(outcome: &QueryRecordSet) -> Vec<(Option<bool>, &str)> {
            outcome
                .iter()
                .map(|record| {
                    (
                        record.task_completed(),
                        record.task_text().unwrap_or_default(),
                    )
                })
                .collect()
        }

        #[test]
        fn contributes_no_rows_when_note_has_no_tasks() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("no-tasks.md"), "Just prose, no tasks.")
                .expect("write note");
            fs::write(temp.path().join("todo.md"), "- [ ] buy milk\n")
                .expect("write note");
            let index =
                IndexerService::new(temp.path()).build().expect("build index");
            let outcome = query_tasks_all(index, &QuerySource::All);

            assert_eq!(outcome.len(), 1);
            assert_eq!(
                outcome.iter().next().and_then(QueryRecord::task_text),
                Some("buy milk")
            );
        }

        #[test]
        fn returns_empty_outcome_when_no_notes_match_source() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("readme.txt"), "text")
                .expect("write txt");
            let index =
                IndexerService::new(temp.path()).build().expect("build index");
            let outcome = query_tasks_all(index, &QuerySource::All);

            assert!(outcome.is_empty());
        }

        #[test]
        fn retains_file_path() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(
                temp.path().join("project.md"),
                "---\ntitle: Launch\n---\nFiled under #projects.\n\n- [ ] \
                 ship it\n",
            )
            .expect("write note");
            let index =
                IndexerService::new(temp.path()).build().expect("build index");
            let outcome = query_tasks_all(index, &QuerySource::All);
            let record = outcome.iter().next().expect("one task row");

            assert_eq!(record.file().path(), Path::new("project.md"));
        }

        #[test]
        fn retains_frontmatter_field() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(
                temp.path().join("project.md"),
                "---\ntitle: Launch\n---\nFiled under #projects.\n\n- [ ] \
                 ship it\n",
            )
            .expect("write note");
            let index =
                IndexerService::new(temp.path()).build().expect("build index");
            let outcome = query_tasks_all(index, &QuerySource::All);
            let record = outcome.iter().next().expect("one task row");

            assert_eq!(
                record.field("title"),
                Ok(crate::note::NoteFieldValue::String("Launch".to_owned()))
            );
        }

        #[test]
        fn retains_tag_field() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(
                temp.path().join("project.md"),
                "---\ntitle: Launch\n---\nFiled under #projects.\n\n- [ ] \
                 ship it\n",
            )
            .expect("write note");
            let index =
                IndexerService::new(temp.path()).build().expect("build index");
            let outcome = query_tasks_all(index, &QuerySource::All);
            let record = outcome.iter().next().expect("one task row");

            assert_eq!(
                record.field("tags"),
                Ok(crate::note::NoteFieldValue::List(vec![
                    crate::note::NoteFieldValue::String("#projects".to_owned())
                ]))
            );
        }

        #[test]
        fn retains_the_parent_notes_inlinks() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("target.md"), "- [ ] ship it\n")
                .expect("write target");
            fs::write(temp.path().join("linker.md"), "[[target]]")
                .expect("write linker");
            let index =
                IndexerService::new(temp.path()).build().expect("build index");
            let outcome = query_tasks_all(index, &QuerySource::All);
            let task = outcome.iter().next().expect("one task row");

            assert_eq!(task.file().path(), Path::new("target.md"));
            assert_eq!(task.inlinks(), [PathBuf::from("linker.md")]);
        }

        #[test]
        fn returns_only_tasks_from_notes_matching_the_tag_source() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(
                temp.path().join("a.md"),
                "#projects\n- [ ] project task\n",
            )
            .expect("write a");
            fs::write(temp.path().join("b.md"), "#books\n- [ ] book task\n")
                .expect("write b");
            let index =
                IndexerService::new(temp.path()).build().expect("build index");
            let outcome = query_tasks_all(
                index,
                &QuerySource::parse("#projects").expect("valid source"),
            );

            assert_eq!(task_rows(&outcome), [(Some(false), "project task")]);
        }

        #[test]
        fn returns_only_tasks_from_notes_under_the_folder_source() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::create_dir_all(temp.path().join("projects")).expect("mkdir");
            fs::write(
                temp.path().join("projects/a.md"),
                "- [ ] project task\n",
            )
            .expect("write a");
            fs::write(temp.path().join("b.md"), "- [ ] other task\n")
                .expect("write b");
            let index =
                IndexerService::new(temp.path()).build().expect("build index");
            let outcome = query_tasks_all(
                index,
                &QuerySource::parse("projects/").expect("valid source"),
            );

            assert_eq!(task_rows(&outcome), [(Some(false), "project task")]);
        }

        #[test]
        fn filters_tasks_by_completion_status() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(
                temp.path().join("todo.md"),
                "- [ ] buy milk\n- [x] pay rent\n",
            )
            .expect("write note");
            let index =
                IndexerService::new(temp.path()).build().expect("build index");
            let outcome = query_tasks_all(index, &QuerySource::All)
                .filter("task.completed == true")
                .expect("valid filter");

            // The Note has one complete and one incomplete task: filtering
            // must keep only the matching task row, not both rows from the
            // one Note that has at least one match.
            assert_eq!(task_rows(&outcome), [(Some(true), "pay rent")]);
        }
    }
}
