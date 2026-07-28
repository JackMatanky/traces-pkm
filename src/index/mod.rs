//! `FileIndex`: a persisted cache of [`FileRecord`]s and [`NoteRecord`]s for
//! files under a trusted project root.
//!
//! Persistence is redb-backed (see [`store`]) but that detail stays behind
//! [`FileIndex`] — callers (`cli`, later `template`) only ever see
//! build/persist/load/enumerate.

#![cfg_attr(
    not(test),
    expect(
        clippy::missing_inline_in_public_items,
        dead_code,
        reason = "crate-internal API surface for FileIndex note metadata"
    )
)]

use std::{fs, path::Path};

pub(crate) use error::FileIndexError;
#[allow(
    unused_imports,
    reason = "domain types exported for index module callers"
)]
pub(crate) use file::{FileFormat, FileRecord, Timestamp};
use markdown::parse_markdown;
#[allow(
    unused_imports,
    reason = "domain types exported for index module callers"
)]
pub(crate) use markdown::{
    CodeRegion, Frontmatter, LinkType, List, ListItem, Note, NoteRecord,
    Outlink, TaskStatus,
};
use store::IndexStore;

mod error;
mod file;
mod markdown;
mod scan;
mod store;

/// The persisted `FileIndex` database's path, relative to a project root.
const INDEX_FILE: &str = ".traces/index.redb";

/// A persisted cache of File Records and Note Metadata for files under a
/// project root.
#[derive(Debug)]
pub(crate) struct FileIndex {
    records: Vec<FileRecord>,
    notes: Vec<NoteRecord>,
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
                let note = parse_markdown(&content);
                notes.push(NoteRecord::new(record.path().to_path_buf(), note));
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
    pub(crate) fn notes(&self) -> &[NoteRecord] {
        &self.notes
    }

    /// Returns the [`Note`] for the note at `path`, if indexed.
    #[must_use]
    pub(crate) fn note(&self, path: &Path) -> Option<&Note> {
        self.notes
            .binary_search_by(|r| r.path().cmp(path))
            .ok()
            .and_then(|idx| self.notes.get(idx))
            .map(NoteRecord::note)
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
        fn finds_every_file_under_root_and_extracts_notes() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::create_dir_all(temp.path().join("notes")).expect("mkdir notes");
            fs::write(
                temp.path().join("notes/todo.md"),
                "---\ntitle: Todo\n---\n- [ ] task 1",
            )
            .expect("write note");
            fs::write(temp.path().join("readme.txt"), "text content")
                .expect("write txt");

            let index = FileIndex::build(temp.path()).expect("build index");

            assert_eq!(index.records().len(), 2);
            assert_eq!(index.notes().len(), 1);

            let note_rec = index.notes().first().expect("note present");
            assert_eq!(note_rec.path(), Path::new("notes/todo.md"));

            let note =
                index.note(Path::new("notes/todo.md")).expect("note lookup");
            assert_eq!(
                note.frontmatter().map(Frontmatter::raw),
                Some("title: Todo\n")
            );
            assert_eq!(note.tasks().count(), 1);
        }
    }

    mod persistence {
        use pretty_assertions::assert_eq;

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
                loaded.notes().first().map(NoteRecord::path),
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
        fn returns_note_when_path_is_indexed() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("a.md"), "# Title").expect("write file");
            let index = FileIndex::build(temp.path()).expect("build index");

            assert_eq!(index.note(Path::new("a.md")).is_some(), true);
        }
    }
}
