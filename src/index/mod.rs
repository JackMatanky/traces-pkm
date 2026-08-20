//! Build, persist, load, and query a file index over a project root.
//!
//! [`FileIndex`] is the main entry point. It stores a sorted [`FileRecord`]
//! (from [`crate::file`]) for every regular file under a project root. Markdown
//! files also contribute parsed [`Note`] metadata. Persistence uses a
//! redb-backed database managed by the [`store`] submodule; callers use
//! [`FileIndex`]'s methods instead of touching redb tables directly.
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
//! - Build the index: [`FileIndex::build`]
//! - Persist to disk: [`FileIndex::persist`]
//! - Load from disk: [`FileIndex::load`]
//! - Refresh against the filesystem: [`FileIndex::refresh`]
//!
//! # Querying
//!
//! - [`FileIndex::query`] runs a page-level query (one row per Note).
//! - [`FileIndex::query_tasks`] runs a task-level query (one row per task
//!   item).
//! - [`FileIndex::records`] and [`FileIndex::notes`] expose sorted indexed data
//!   for direct inspection.
//!
//! [`store`]: mod@store
//! [`inlinks`]: mod@inlinks
//! [`builder::IndexBuilder`]: mod@builder

mod builder;
mod error;
mod inlinks;
mod scan;
mod store;

use std::path::Path;

#[allow(unused_imports, reason = "re-exported for downstream callers")]
pub use error::{FileIndexError, IndexBuilderError};
use inlinks::InlinkMap;
use store::IndexStore;

pub(crate) use crate::file::FileFormat;
pub use crate::file::FileRecord;
#[cfg(test)]
use crate::query::IndexRecord;
use crate::{
    note::Note,
    query::{QueryOutcome, QuerySource},
};

/// Project-relative path of the persisted [`FileIndex`] database.
const INDEX_FILE: &str = ".traces/index.redb";

/// Persisted cache of file records, parsed Note metadata, and derived inbound
/// links.
///
/// Every regular file under the project root contributes a [`FileRecord`].
/// Markdown files also contribute a [`Note`], accessible through
/// [`Self::notes`] or [`Self::note`]. Use [`Self::build`] to create an index
/// from scratch, [`Self::persist`] to save it, [`Self::load`] to reload it, or
/// [`Self::refresh`] to update it against the current filesystem state.
#[derive(Clone, Debug)]
pub struct FileIndex {
    records: Vec<FileRecord>,
    notes: Vec<Note>,
    /// Inbound links, keyed by target path; see [`inlinks::derive_inlinks`].
    ///
    /// Recomputed in full whenever [`Self::refresh`] finds changed content.
    /// Reused unchanged from the last persisted computation otherwise.
    inlinks: InlinkMap,
}

impl FileIndex {
    /// Scans `root` and builds a [`FileIndex`] in memory.
    ///
    /// Markdown files are parsed into [`Note`] records. The index is not
    /// persisted until [`Self::persist`] is called.
    ///
    /// # Errors
    ///
    /// - [`FileIndexError::Io`] if a directory cannot be read, a file's
    ///   metadata cannot be inspected, or a markdown file cannot be read.
    #[inline]
    pub fn build(root: &Path) -> Result<Self, FileIndexError> {
        Ok(builder::IndexBuilder::from_scan(root)?.build(root)?)
    }

    /// Refreshes the persisted index for `root` against current filesystem
    /// state.
    ///
    /// Re-scans `root` and compares each current file's `(created_at,
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
    /// Derived inlinks are recomputed in full whenever any file's content or
    /// metadata changed since the last persist. A full recompute (not a
    /// per-note patch) is required because link target resolution considers
    /// every indexed Note: an unedited Note's *resolved* target can change when
    /// an unrelated Note is added or removed. For example, a wikilink that was
    /// ambiguous becomes resolvable once one of the ambiguous candidates is
    /// deleted.
    ///
    /// # Errors
    ///
    /// - [`FileIndexError::Io`] if a directory cannot be read, a file's
    ///   metadata cannot be inspected, or a markdown file cannot be read.
    /// - [`FileIndexError::Store`] or [`FileIndexError::Deserialize`] if the
    ///   previous index cannot be loaded.
    #[inline]
    pub fn refresh(root: &Path) -> Result<Self, FileIndexError> {
        let previous = Self::load(root)?;
        Ok(builder::IndexBuilder::from_scan(root)?
            .reuse_unchanged(previous)
            .build(root)?)
    }

    /// Persists this index to `root`, replacing any existing index contents.
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
    pub fn persist(&self, root: &Path) -> Result<(), FileIndexError> {
        IndexStore::open(root)?.replace_all(
            &self.records,
            &self.notes,
            &self.inlinks,
        )
    }

    /// Loads the index previously persisted for `root`.
    ///
    /// Returns an empty [`FileIndex`] if no index has been persisted yet.
    ///
    /// # Errors
    ///
    /// - [`FileIndexError::Store`] if the database cannot be read.
    /// - [`FileIndexError::Deserialize`] if stored bytes are not a valid
    ///   record.
    #[inline]
    pub fn load(root: &Path) -> Result<Self, FileIndexError> {
        let (records, notes, inlinks) = IndexStore::open(root)?.load_all()?;
        Ok(Self {
            records,
            notes,
            inlinks,
        })
    }

    /// Executes a page-level query over `source`, consuming this index.
    ///
    /// Call [`Self::refresh`] first so results reflect the current filesystem.
    /// Every markdown Note has a matching [`FileRecord`] by construction (both
    /// [`Self::build`] and [`Self::refresh`] add one for every parsed Note), so
    /// a Note found without one is skipped rather than causing a panic.
    ///
    /// Every matched [`IndexRecord`]'s `inlinks` reflects every indexed Note,
    /// not just Notes matching `source`: a Note outside `source` can still link
    /// to one inside it.
    ///
    /// # Performance
    ///
    /// O(n + m): [`Self::refresh`]/[`Self::load`] already produced
    /// `self.inlinks`, so this is just the single-pass iterator merge-join
    /// across `records` and `notes`, looking each matched Note's inlinks up by
    /// moving them out of the map instead of cloning.
    #[inline]
    #[cfg_attr(
        not(any(test, feature = "test-utils")),
        expect(
            dead_code,
            reason = "read-side exit is exported only with the test-utils API"
        )
    )]
    #[must_use]
    pub fn query(self, source: &QuerySource) -> QueryOutcome {
        crate::query::query(self, source, "class")
    }

    /// Executes a task-level query over `source`, consuming this index.
    ///
    /// Selects the same Notes as [`Self::query`], then expands each matched
    /// Note into one [`IndexRecord`] per markdown task item (`- [ ]` or `-
    /// [x]`). Notes without tasks contribute no rows.
    ///
    /// Each task row keeps its parent Note's `file.*`, frontmatter,
    /// inline-field, tag, and inlinks metadata for filtering and display
    /// through `IndexRecord::field`. It also exposes
    /// [`IndexRecord::task_completed`] and `IndexRecord::task_text`.
    ///
    /// Call [`Self::refresh`] first so results reflect the current filesystem.
    ///
    /// # Performance
    ///
    /// - O(n + m + t), where `t` is the total task-item count across matched
    ///   Notes. [`Self::refresh`]/[`Self::load`] already produced
    ///   `self.inlinks`.
    /// - The task iterator is peeked to identify its final item, so only
    ///   earlier rows clone the base record.
    /// - The final row moves the shared [`IndexRecord`] base. Earlier clones
    ///   remain O(1) because [`IndexRecord`]'s `note` field is an [`Arc`], not
    ///   a deep clone.
    ///
    /// [`Arc`]: std::sync::Arc
    #[inline]
    #[cfg_attr(
        not(any(test, feature = "test-utils")),
        expect(
            dead_code,
            reason = "read-side exit is exported only with the test-utils API"
        )
    )]
    #[must_use]
    pub fn query_tasks(self, source: &QuerySource) -> QueryOutcome {
        crate::query::query_tasks(self, source, "class")
    }

    /// Returns indexed [`FileRecord`]s, sorted by path.
    #[inline]
    #[must_use]
    pub fn records(&self) -> &[FileRecord] {
        &self.records
    }

    /// Returns indexed [`Note`] records, sorted by path.
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

    /// Returns the [`FileRecord`] for the file at `path`, if indexed.
    ///
    /// # Performance
    ///
    /// O(log n): [`Self::records`] is kept sorted by path, so this binary
    /// searches rather than scanning.
    #[inline]
    #[must_use]
    #[cfg_attr(
        not(any(test, feature = "test-utils")),
        expect(
            dead_code,
            reason = "used in query module tests for direct record inspection"
        )
    )]
    pub(crate) fn record(&self, path: &Path) -> Option<&FileRecord> {
        self.records
            .binary_search_by(|r| r.path().cmp(path))
            .ok()
            .and_then(|idx| self.records.get(idx))
    }

    /// Consumes this index and returns its inner components.
    ///
    /// Used by the query module to pair records with notes and resolve inlinks
    /// without exposing `FileIndex`'s internal layout.
    pub(crate) fn into_parts(self) -> (Vec<FileRecord>, Vec<Note>, InlinkMap) {
        (self.records, self.notes, self.inlinks)
    }
}

/// Binary-searches path-sorted `notes` for an exact path match.
///
/// Shared by [`FileIndex::note`], which does this lookup once `self` exists,
/// and the [`inlinks`] submodule, which needs the same search over a bare
/// `&[Note]` slice while resolving link targets during
/// [`FileIndex::build`]/[`FileIndex::refresh`].
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

            let index = FileIndex::build(temp.path()).expect("build index");

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

            let index = FileIndex::build(temp.path()).expect("build index");

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

            let index = FileIndex::build(temp.path()).expect("build index");

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

            FileIndex::build(temp.path()).expect("build index");

            let after = fs::read_to_string(temp.path().join("note.md"))
                .expect("read note back");
            assert_eq!(after, original);
        }

        #[test]
        fn returns_io_error_when_markdown_file_is_not_utf8() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("bad.md"), [0xFF, 0xFE])
                .expect("write invalid utf8");

            let result = FileIndex::build(temp.path());

            assert!(matches!(result, Err(FileIndexError::Io { .. })));
        }

        #[test]
        fn sorts_indexed_notes_by_path() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("b.md"), "# B").expect("write b");
            fs::write(temp.path().join("a.md"), "# A").expect("write a");

            let index = FileIndex::build(temp.path()).expect("build index");

            let paths: Vec<&Path> =
                index.notes().iter().map(Note::path).collect();
            assert_eq!(paths, [Path::new("a.md"), Path::new("b.md")]);
        }
    }

    mod persistence {
        use pretty_assertions::assert_eq;
        use rstest::rstest;

        use super::*;
        use crate::note::{
            FieldValue, Frontmatter, InlineField, InlineFieldForm, Link,
            LinkType, Tag,
        };

        #[test]
        fn persist_then_load_recovers_the_same_records_and_notes() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(
                temp.path().join("note.md"),
                "---\ntitle: Hello\n---\n[[other_note]]\n- [x] done",
            )
            .expect("write note");
            let built = FileIndex::build(temp.path()).expect("build index");
            built.persist(temp.path()).expect("persist index");

            let loaded = FileIndex::load(temp.path()).expect("load index");

            assert_eq!(loaded.records(), built.records());
            assert_eq!(loaded.notes(), built.notes());

            let loaded_note =
                loaded.note(Path::new("note.md")).expect("loaded note");
            assert_eq!(loaded_note.outlinks().len(), 1);
            assert_eq!(
                loaded_note.outlinks().first().map(Link::target),
                Some("other_note")
            );
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
            let built = FileIndex::build(temp.path()).expect("build index");
            built.persist(temp.path()).expect("persist index");

            let loaded = FileIndex::load(temp.path()).expect("load index");

            let field = loaded
                .note(Path::new("note.md"))
                .and_then(Note::frontmatter)
                .into_iter()
                .flat_map(Frontmatter::fields)
                .find(|field| field.key().is_canonical_match("related"))
                .expect("related field");
            assert_eq!(
                field.value(),
                &FieldValue::Link(Link::new(
                    "Project Alpha",
                    "Alpha",
                    LinkType::Wikilink
                ))
            );
        }

        #[rstest]
        #[case::body(
            "Status:: Draft",
            "status",
            "Draft",
            InlineFieldForm::Body
        )]
        #[case::visible_key(
            "[Status:: Draft]",
            "status",
            "Draft",
            InlineFieldForm::VisibleKey
        )]
        #[case::hidden_key(
            "(Status:: Draft)",
            "status",
            "Draft",
            InlineFieldForm::HiddenKey
        )]
        fn persist_then_load_recovers_inline_fields(
            #[case] source: &str,
            #[case] expected_key: &str,
            #[case] expected_value: &str,
            #[case] expected_form: InlineFieldForm,
        ) {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("note.md"), source).expect("write note");
            let built = FileIndex::build(temp.path()).expect("build index");
            built.persist(temp.path()).expect("persist index");

            let loaded = FileIndex::load(temp.path()).expect("load index");

            let loaded_note =
                loaded.note(Path::new("note.md")).expect("loaded note");
            let built_note =
                built.note(Path::new("note.md")).expect("built note");
            assert_eq!(loaded_note.inline_fields(), built_note.inline_fields());
            let field = loaded_note
                .inline_fields()
                .first()
                .expect("inline field present");
            assert!(field.key().is_canonical_match(expected_key));
            assert_eq!(field.value().as_str(), Some(expected_value));
            assert_eq!(field.form(), expected_form);
        }

        #[test]
        fn persist_then_load_recovers_typed_inline_field_values() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(
                temp.path().join("note.md"),
                "[duration:: 7 hours]\n[values:: 1, 2]",
            )
            .expect("write note");
            let built = FileIndex::build(temp.path()).expect("build index");
            built.persist(temp.path()).expect("persist index");

            let loaded = FileIndex::load(temp.path()).expect("load index");

            let loaded_note =
                loaded.note(Path::new("note.md")).expect("loaded note");
            let built_note =
                built.note(Path::new("note.md")).expect("built note");
            assert_eq!(loaded_note.inline_fields(), built_note.inline_fields());
            let values: Vec<&FieldValue> = loaded_note
                .inline_fields()
                .iter()
                .map(InlineField::value)
                .collect();
            assert_eq!(values, [
                &FieldValue::Duration("7 hours".to_owned()),
                &FieldValue::List(vec![
                    FieldValue::Number(1.0),
                    FieldValue::Number(2.0)
                ])
            ]);
        }

        #[test]
        fn persist_then_load_recovers_tags() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("note.md"), "Filed under #book today.")
                .expect("write note");
            let built = FileIndex::build(temp.path()).expect("build index");
            built.persist(temp.path()).expect("persist index");

            let loaded = FileIndex::load(temp.path()).expect("load index");

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

            let index = FileIndex::load(temp.path()).expect("load index");

            assert_eq!(index.records().len(), 0);
            assert_eq!(index.notes().len(), 0);
        }

        #[test]
        fn persists_rebuilds_rather_than_appends() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("first.md"), "- [ ] first")
                .expect("write first");
            FileIndex::build(temp.path())
                .expect("build first index")
                .persist(temp.path())
                .expect("persist first index");
            fs::remove_file(temp.path().join("first.md"))
                .expect("remove first");
            fs::write(temp.path().join("second.md"), "- [x] second")
                .expect("write second");

            FileIndex::build(temp.path())
                .expect("build second index")
                .persist(temp.path())
                .expect("persist second index");
            let loaded = FileIndex::load(temp.path()).expect("load index");

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
            let index = FileIndex::build(temp.path()).expect("build index");

            assert_eq!(index.note(Path::new("nonexistent.md")), None);
        }

        #[test]
        fn returns_the_matching_note_when_path_is_indexed() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("a.md"), "# A").expect("write a");
            fs::write(temp.path().join("b.md"), "# B").expect("write b");
            fs::write(temp.path().join("c.md"), "# C").expect("write c");
            let index = FileIndex::build(temp.path()).expect("build index");

            assert_eq!(
                index.note(Path::new("b.md")).map(Note::path),
                Some(Path::new("b.md"))
            );
        }
    }

    mod builder {
        use std::path::PathBuf;

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
        }

        #[test]
        fn reuse_unchanged_skips_reparsing() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("note.md"), "- [ ] task")
                .expect("write note");
            let built = FileIndex::build(temp.path()).expect("build index");

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
            let built = FileIndex::build(temp.path()).expect("build index");

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

        #[test]
        fn derives_inlinks_from_outlinks() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("target.md"), "# Target")
                .expect("write target");
            fs::write(temp.path().join("linker.md"), "[[target]]")
                .expect("write linker");

            let index = IndexBuilder::from_scan(temp.path())
                .expect("scan")
                .build(temp.path())
                .expect("build");

            let outcome = index.query(&QuerySource::All);
            let target = outcome
                .iter()
                .find(|r| r.file().path() == Path::new("target.md"))
                .expect("target record");

            assert_eq!(target.inlinks(), [PathBuf::from("linker.md")]);
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
            FileIndex::build(temp.path())
                .expect("build index")
                .persist(temp.path())
                .expect("persist index");

            fs::write(temp.path().join("note.md"), "- [ ] task\n- [x] second")
                .expect("rewrite note");

            let refreshed =
                FileIndex::refresh(temp.path()).expect("refresh index");

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
            FileIndex::build(temp.path())
                .expect("build index")
                .persist(temp.path())
                .expect("persist index");

            std::thread::sleep(std::time::Duration::from_millis(15));
            fs::write(temp.path().join("note.md"), "Status:: Final")
                .expect("rewrite note with same byte length");

            let refreshed =
                FileIndex::refresh(temp.path()).expect("refresh index");

            let value = refreshed
                .note(Path::new("note.md"))
                .and_then(|n| n.inline_fields().first())
                .and_then(|f| f.value().as_str());
            assert_eq!(value, Some("Final"));
        }

        #[test]
        fn includes_newly_added_note() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("first.md"), "# First")
                .expect("write first");
            FileIndex::build(temp.path())
                .expect("build index")
                .persist(temp.path())
                .expect("persist index");

            fs::write(temp.path().join("second.md"), "# Second")
                .expect("write second");

            let refreshed =
                FileIndex::refresh(temp.path()).expect("refresh index");

            assert_eq!(refreshed.notes().len(), 2);
        }

        #[test]
        fn excludes_deleted_note() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("gone.md"), "# Gone")
                .expect("write note");
            FileIndex::build(temp.path())
                .expect("build index")
                .persist(temp.path())
                .expect("persist index");

            fs::remove_file(temp.path().join("gone.md")).expect("delete note");

            let refreshed =
                FileIndex::refresh(temp.path()).expect("refresh index");

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
            FileIndex::build(temp.path())
                .expect("build index")
                .persist(temp.path())
                .expect("persist index");

            fs::remove_file(temp.path().join("linker.md"))
                .expect("delete linker");

            let refreshed =
                FileIndex::refresh(temp.path()).expect("refresh index");
            let outcome = refreshed.query(&QuerySource::All);
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
            FileIndex::build(temp.path())
                .expect("build index")
                .persist(temp.path())
                .expect("persist index");

            fs::write(temp.path().join("linker.md"), "[[new-target]]")
                .expect("repoint linker");

            let refreshed =
                FileIndex::refresh(temp.path()).expect("refresh index");
            let outcome = refreshed.query(&QuerySource::All);
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
            FileIndex::build(temp.path())
                .expect("build index")
                .persist(temp.path())
                .expect("persist index");

            fs::write(temp.path().join("extra.md"), "# Extra")
                .expect("write extra");
            FileIndex::refresh(temp.path())
                .expect("refresh index")
                .persist(temp.path())
                .expect("persist index");

            let loaded = FileIndex::load(temp.path()).expect("load index");
            assert_eq!(loaded.notes().len(), 2);
        }

        #[test]
        fn builds_an_index_when_nothing_was_persisted_yet() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("note.md"), "# Note")
                .expect("write note");

            let refreshed =
                FileIndex::refresh(temp.path()).expect("refresh index");

            assert_eq!(refreshed.notes().len(), 1);
        }

        #[test]
        fn preserves_inlinks_after_noop_refresh() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("target.md"), "# Target")
                .expect("write target");
            fs::write(temp.path().join("linker.md"), "[[target]]")
                .expect("write linker");
            FileIndex::build(temp.path())
                .expect("build index")
                .persist(temp.path())
                .expect("persist index");

            let refreshed =
                FileIndex::refresh(temp.path()).expect("refresh index");
            let outcome = refreshed.query(&QuerySource::All);
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
            FileIndex::build(temp.path())
                .expect("build index")
                .persist(temp.path())
                .expect("persist index");

            fs::write(temp.path().join("note.md"), "# Revised")
                .expect("rewrite note");

            let refreshed =
                FileIndex::refresh(temp.path()).expect("refresh index");

            // The refreshed index reflects the new content...
            assert_eq!(
                refreshed
                    .note(Path::new("note.md"))
                    .and_then(Note::frontmatter)
                    .and_then(|fm| fm.fields().first())
                    .and_then(|f| f.value().as_str()),
                None // "# Revised" has no frontmatter
            );
            // ...but a fresh load from disk still shows the OLD content,
            // because refresh did not persist.
            let loaded = FileIndex::load(temp.path()).expect("load index");
            assert_eq!(
                loaded
                    .note(Path::new("note.md"))
                    .and_then(Note::frontmatter)
                    .and_then(|fm| fm.fields().first())
                    .and_then(|f| f.value().as_str()),
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
            FileIndex::build(temp.path())
                .expect("build index")
                .persist(temp.path())
                .expect("persist index");

            fs::remove_file(temp.path().join("archive/foo.md"))
                .expect("delete archive/foo.md");

            let refreshed =
                FileIndex::refresh(temp.path()).expect("refresh index");
            let outcome = refreshed.query(&QuerySource::All);
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

        fn note_paths(outcome: &QueryOutcome) -> Vec<&Path> {
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
            FileIndex::build(temp.path()).expect("build index")
        }

        #[test]
        fn returns_all_files_in_sorted_order() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("a.md"), "# A").expect("write a");
            fs::write(temp.path().join("b.md"), "# B").expect("write b");
            fs::write(temp.path().join("readme.txt"), "text")
                .expect("write txt");
            let index = FileIndex::build(temp.path()).expect("build index");

            let outcome = index.query(&QuerySource::All);

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
            let index = FileIndex::build(temp.path()).expect("build index");

            let outcome = index.query(&QuerySource::All);

            assert_eq!(note_paths(&outcome), [Path::new("a.md")]);
            assert_eq!(outcome.get(1).and_then(|r| r.note()), None);
        }

        #[test]
        fn returns_empty_when_no_notes_match_tag() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("readme.txt"), "text")
                .expect("write txt");
            let index = FileIndex::build(temp.path()).expect("build index");

            let outcome = index
                .query(&QuerySource::parse("#missing").expect("valid source"));

            assert_eq!(outcome.len(), 0);
        }

        #[test]
        fn returns_matching_note_when_tag_source_is_exact() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("book.md"), "Filed under #book.")
                .expect("write book");
            fs::write(temp.path().join("other.md"), "No tags here.")
                .expect("write other");
            let index = FileIndex::build(temp.path()).expect("build index");

            let outcome = index
                .query(&QuerySource::parse("#book").expect("valid source"));

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
            let index = FileIndex::build(temp.path()).expect("build index");

            let exact = index.clone().query(
                &QuerySource::parse("#projects/active").expect("valid source"),
            );
            let parent = index
                .query(&QuerySource::parse("#projects").expect("valid source"));

            assert_eq!(note_paths(&exact), [Path::new("project.md")]);
            assert_eq!(note_paths(&parent), [Path::new("project.md")]);
        }

        #[test]
        fn returns_empty_when_tag_query_is_too_specific() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("project.md"), "Tracked in #projects.")
                .expect("write project");
            let index = FileIndex::build(temp.path()).expect("build index");

            let outcome = index.query(
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
            let index = FileIndex::build(temp.path()).expect("build index");

            let outcome = index
                .query(&QuerySource::parse("books/").expect("valid source"));

            assert_eq!(note_paths(&outcome), [
                Path::new("books/dune.md"),
                Path::new("books/fiction/hobbit.md")
            ]);
        }

        #[test]
        fn returns_file_path_for_each_record() {
            let index = build_book_index();

            let outcome = index.query(&QuerySource::All);
            let record = outcome.iter().next().expect("one record");

            assert_eq!(record.file().path(), Path::new("book.md"));
        }

        #[test]
        fn includes_frontmatter_fields_in_note() {
            let index = build_book_index();

            let outcome = index.query(&QuerySource::All);
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

            let outcome = index.query(&QuerySource::All);
            let note = outcome
                .iter()
                .next()
                .expect("one record")
                .note()
                .expect("note");

            assert_eq!(
                note.inline_fields()
                    .iter()
                    .map(|field| field.key().canonical())
                    .collect::<Vec<_>>(),
                ["genre"]
            );
        }

        #[test]
        fn includes_note_tags() {
            let index = build_book_index();

            let outcome = index.query(&QuerySource::All);
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
            let index = FileIndex::build(temp.path()).expect("build index");

            let outcome = index.query(&QuerySource::All);
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
            let index = FileIndex::build(temp.path()).expect("build index");

            // `linker.md` has no `#book` tag, so it is excluded from this
            // tag-scoped query; its outlink to `target.md` must still show
            // up in the target's inlinks.
            let outcome = index
                .query(&QuerySource::parse("#book").expect("valid source"));
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
            let index = FileIndex::build(temp.path()).expect("build index");

            let outcome = index.query(&QuerySource::All);
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
            let index = FileIndex::build(temp.path()).expect("build index");

            let outcome = index.query(&QuerySource::All);
            let source = outcome
                .iter()
                .find(|record| record.file().path() == Path::new("b.md"))
                .expect("self-linking record");

            assert_eq!(source.inlinks(), [PathBuf::from("b.md")]);
        }
    }

    mod query_tasks {
        use std::path::PathBuf;

        use pretty_assertions::assert_eq;

        use super::*;

        /// `(completed, text)` pairs for every row in `outcome`, in order.
        fn task_rows(outcome: &QueryOutcome) -> Vec<(Option<bool>, &str)> {
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
            let index = FileIndex::build(temp.path()).expect("build index");

            let outcome = index.query_tasks(&QuerySource::All);

            assert_eq!(outcome.len(), 1);
            assert_eq!(
                outcome.iter().next().and_then(IndexRecord::task_text),
                Some("buy milk")
            );
        }

        #[test]
        fn returns_empty_outcome_when_no_notes_match_source() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("readme.txt"), "text")
                .expect("write txt");
            let index = FileIndex::build(temp.path()).expect("build index");

            let outcome = index.query_tasks(&QuerySource::All);

            assert!(outcome.is_empty());
        }

        #[test]
        fn retains_parent_note_metadata_for_filtering_and_display() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(
                temp.path().join("project.md"),
                "---\ntitle: Launch\n---\nFiled under #projects.\n\n- [ ] \
                 ship it\n",
            )
            .expect("write note");
            let index = FileIndex::build(temp.path()).expect("build index");

            let outcome = index.query_tasks(&QuerySource::All);
            let record = outcome.iter().next().expect("one task row");

            assert_eq!(record.file().path(), Path::new("project.md"));
            assert_eq!(
                record.field("title"),
                Ok(crate::note::FieldValue::String("Launch".to_owned()))
            );
            assert_eq!(
                record.field("tags"),
                Ok(crate::note::FieldValue::List(vec![
                    crate::note::FieldValue::String("#projects".to_owned())
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
            let index = FileIndex::build(temp.path()).expect("build index");

            let outcome = index.query_tasks(&QuerySource::All);
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
            let index = FileIndex::build(temp.path()).expect("build index");

            let outcome = index.query_tasks(
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
            let index = FileIndex::build(temp.path()).expect("build index");

            let outcome = index.query_tasks(
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
            let index = FileIndex::build(temp.path()).expect("build index");

            let outcome = index
                .query_tasks(&QuerySource::All)
                .filter("task.completed == true")
                .expect("valid filter");

            // The Note has one complete and one incomplete task: filtering
            // must keep only the matching task row, not both rows from the
            // one Note that has at least one match.
            assert_eq!(task_rows(&outcome), [(Some(true), "pay rent")]);
        }
    }
}
