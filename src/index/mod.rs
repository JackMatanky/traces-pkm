//! `FileIndex`: a persisted cache of [`FileRecord`]s and [`Note`]s for files
//! under a trusted project root.
//!
//! Persistence is redb-backed (see [`store`]) but that detail stays behind
//! [`FileIndex`] — callers (`cli`, later `template`) only ever see
//! build/persist/load/enumerate.

#![cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "crate-internal API surface for FileIndex note metadata, \
                  consumed by later tickets (#04 lazy query refresh)"
    )
)]

mod error;
mod file;
mod note;
mod scan;
mod store;

use std::{fs, path::Path};

pub(crate) use error::FileIndexError;
#[expect(
    unused_imports,
    reason = "domain types exported for index module callers"
)]
pub(crate) use file::{FileFormat, FileRecord, Timestamp};
use note::parse_markdown;
#[expect(
    unused_imports,
    reason = "domain types exported for index module callers"
)]
pub(crate) use note::{
    CodeRegion, FieldSource, FieldValue, Frontmatter, InlineFieldForm,
    LinkType, List, ListItem, MetadataField, Note, Outlink, RawFrontmatter,
    Tag, TaskStatus,
};
use store::IndexStore;

/// The persisted `FileIndex` database's path, relative to a project root.
const INDEX_FILE: &str = ".traces/index.redb";

/// A persisted cache of File Records and Note Metadata for files under a
/// project root. Two tiers: every file gets a [`FileRecord`]
/// ([`Self::records`]); markdown files additionally get a [`Note`]
/// ([`Self::notes`], [`Self::note`]).
#[derive(Debug)]
pub(crate) struct FileIndex {
    records: Vec<FileRecord>,
    notes: Vec<Note>,
}

impl FileIndex {
    /// Scans `root` and builds a `FileIndex` in memory, extracting [`Note`]
    /// metadata for markdown files. Does not persist — call [`Self::persist`]
    /// to write it to disk.
    ///
    /// # Errors
    ///
    /// Returns [`FileIndexError::Io`] if a directory cannot be read, a file's
    /// metadata cannot be inspected, or a markdown file cannot be read.
    pub(crate) fn build(root: &Path) -> Result<Self, FileIndexError> {
        let records = scan::scan_root(root)?;
        let mut notes = Vec::new();

        for record in &records {
            if record.format() == FileFormat::Note {
                let full_path = root.join(record.path());
                let content =
                    fs::read_to_string(&full_path).map_err(|source| {
                        FileIndexError::Io {
                            path: full_path,
                            source,
                        }
                    })?;
                let note = parse_markdown(record.path(), &content);
                notes.push(note);
            }
        }
        notes.sort_by(|a, b| a.path().cmp(b.path()));

        Ok(Self {
            records,
            notes,
        })
    }

    /// Persists this `FileIndex`'s File Records and Note Records to `root`'s
    /// index database, replacing any previously persisted contents.
    ///
    /// # Errors
    ///
    /// - [`FileIndexError::Io`] if the database's parent directory cannot be
    ///   created
    /// - [`FileIndexError::Store`] if the database transaction fails
    /// - [`FileIndexError::Serialize`] if a record cannot be encoded
    #[inline]
    pub(crate) fn persist(&self, root: &Path) -> Result<(), FileIndexError> {
        IndexStore::open(root)?.replace_all(&self.records, &self.notes)
    }

    /// Loads the `FileIndex` previously persisted for `root`. Returns an empty
    /// `FileIndex` if none was ever persisted.
    ///
    /// # Errors
    ///
    /// - [`FileIndexError::Store`] if the database cannot be read
    /// - [`FileIndexError::Corrupt`] if stored bytes aren't valid UTF-8
    /// - [`FileIndexError::Deserialize`] if stored text isn't a valid record
    #[inline]
    pub(crate) fn load(root: &Path) -> Result<Self, FileIndexError> {
        let (records, notes) = IndexStore::open(root)?.load_all()?;
        Ok(Self {
            records,
            notes,
        })
    }

    /// Every indexed File Record, sorted by path.
    #[inline]
    #[must_use]
    pub(crate) fn records(&self) -> &[FileRecord] {
        &self.records
    }

    /// Every indexed Note Record, sorted by path.
    #[inline]
    #[must_use]
    pub(crate) fn notes(&self) -> &[Note] {
        &self.notes
    }

    /// Returns the [`Note`] for the note at `path`, if indexed.
    ///
    /// # Performance
    ///
    /// O(log n) — [`Self::notes`] is kept sorted by path, so this binary
    /// searches rather than scanning.
    #[must_use]
    pub(crate) fn note(&self, path: &Path) -> Option<&Note> {
        self.notes
            .binary_search_by(|n| n.path().cmp(path))
            .ok()
            .and_then(|idx| self.notes.get(idx))
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

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
                loaded_note.outlinks().first().map(Outlink::target),
                Some("other_note")
            );
            assert_eq!(loaded_note.tasks().count(), 1);
        }

        #[rstest]
        #[case::body(
            "Status:: Draft",
            "Status",
            "Draft",
            InlineFieldForm::Body
        )]
        #[case::visible_key(
            "[Status:: Draft]",
            "Status",
            "Draft",
            InlineFieldForm::VisibleKey
        )]
        #[case::hidden_key(
            "(Status:: Draft)",
            "Status",
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
            assert_eq!(field.key(), expected_key);
            assert_eq!(field.value().as_str(), Some(expected_value));
            assert_eq!(field.form(), Some(expected_form));
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
        fn returns_an_empty_index_when_the_root_was_never_persisted() {
            let temp = tempfile::tempdir().expect("create temp dir");

            let index = FileIndex::load(temp.path()).expect("load index");

            assert_eq!(index.records().len(), 0);
            assert_eq!(index.notes().len(), 0);
        }

        #[test]
        fn rebuilds_rather_than_appends_when_persisted_again() {
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
}
