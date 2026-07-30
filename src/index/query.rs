//! Page-level query source selection over indexed markdown Notes.
//!
//! [`Source`] selects which Notes a page-level query includes. [`IndexRecord`]
//! pairs a [`FileRecord`] with its [`Note`] so callers can read both
//! `file.*` fields and Note Metadata from one value. [`QueryOutcome`] is the
//! iterable collection [`super::FileIndex::query`] returns.

use std::path::PathBuf;

use super::file::FileRecord;
use crate::note::Note;

/// Selects which markdown Notes a page-level query includes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Source {
    /// Every indexed markdown Note.
    All,
    /// Notes tagged with a markdown tag, or a sub-tag nested under it, e.g.
    /// `#book` or `#projects` (which also matches `#projects/active`).
    Tag(String),
    /// Notes whose [`FileRecord::folder`] is the requested project-relative
    /// folder, or a folder nested under it.
    Folder(PathBuf),
}

impl Source {
    /// Whether `file` and its parsed `note` belong to this source.
    #[inline]
    #[must_use]
    pub(super) fn is_match(&self, file: &FileRecord, note: &Note) -> bool {
        match self {
            Self::All => true,
            Self::Tag(tag) => {
                note.tags().iter().any(|t| t.is_nested_under(tag))
            }
            Self::Folder(folder) => file.folder().starts_with(folder),
        }
    }
}

/// One page-level query result: a [`FileRecord`] paired with its [`Note`].
///
/// Exposes both `file.*` fields and Note Metadata (frontmatter, inline
/// fields, tags) through one value for Template and CLI callers.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct IndexRecord {
    file: FileRecord,
    note: Note,
}

impl IndexRecord {
    /// Pairs `file` with its parsed `note`.
    pub(super) fn new(file: FileRecord, note: Note) -> Self {
        Self {
            file,
            note,
        }
    }

    /// The indexed file's general metadata.
    #[inline]
    #[must_use]
    pub(crate) fn file(&self) -> &FileRecord {
        &self.file
    }

    /// The indexed file's parsed Note Metadata.
    #[inline]
    #[must_use]
    pub(crate) fn note(&self) -> &Note {
        &self.note
    }
}

/// Iterable, page-level collection of [`IndexRecord`] values returned by
/// [`super::FileIndex::query`].
///
/// Ready for the filtering, ordering, and Template/CLI integration added by
/// later tickets (#05 `QueryOutcome` filtering, #06 `QueryOps` namespace).
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct QueryOutcome {
    records: Vec<IndexRecord>,
}

impl QueryOutcome {
    /// Wraps `records` as a page-level query result.
    pub(super) fn new(records: Vec<IndexRecord>) -> Self {
        Self {
            records,
        }
    }

    /// The number of [`IndexRecord`]s in this outcome.
    #[inline]
    #[must_use]
    pub(crate) fn len(&self) -> usize {
        self.records.len()
    }

    /// Whether this outcome has no [`IndexRecord`]s.
    #[inline]
    #[must_use]
    pub(crate) fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// The [`IndexRecord`] at `index`, if present.
    #[inline]
    #[must_use]
    pub(crate) fn get(&self, index: usize) -> Option<&IndexRecord> {
        self.records.get(index)
    }

    /// Iterates over the contained [`IndexRecord`]s by reference.
    #[inline]
    pub(crate) fn iter(&self) -> std::slice::Iter<'_, IndexRecord> {
        self.records.iter()
    }
}

impl IntoIterator for QueryOutcome {
    type IntoIter = std::vec::IntoIter<Self::Item>;
    type Item = IndexRecord;

    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        self.records.into_iter()
    }
}

impl<'a> IntoIterator for &'a QueryOutcome {
    type IntoIter = std::slice::Iter<'a, IndexRecord>;
    type Item = &'a IndexRecord;

    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        self.records.iter()
    }
}
#[cfg(test)]
mod tests {
    use std::{fs, path::Path};

    use super::*;
    use crate::index::FileIndex;

    mod source_is_match {

        use super::*;

        #[test]
        fn returns_true_for_all_source() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("a.md"), "# A").expect("write file");
            let index = FileIndex::build(temp.path()).expect("build index");
            let record = index.record(Path::new("a.md")).expect("record");
            let note = index.note(Path::new("a.md")).expect("note");

            assert!(Source::All.is_match(record, note));
        }

        #[test]
        fn returns_true_when_note_has_matching_or_sub_tag() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("a.md"), "Tracked in #projects/active.")
                .expect("write file");
            let index = FileIndex::build(temp.path()).expect("build index");
            let record = index.record(Path::new("a.md")).expect("record");
            let note = index.note(Path::new("a.md")).expect("note");

            assert!(Source::Tag("#projects".to_owned()).is_match(record, note));
            assert!(!Source::Tag("#books".to_owned()).is_match(record, note));
        }

        #[test]
        fn returns_true_when_file_is_under_folder_source() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::create_dir_all(temp.path().join("projects/active"))
                .expect("mkdir");
            fs::write(temp.path().join("projects/active/task.md"), "# Task")
                .expect("write file");
            let index = FileIndex::build(temp.path()).expect("build index");
            let record = index
                .record(Path::new("projects/active/task.md"))
                .expect("record");
            let note =
                index.note(Path::new("projects/active/task.md")).expect("note");

            assert!(
                Source::Folder(PathBuf::from("projects"))
                    .is_match(record, note)
            );
            assert!(
                !Source::Folder(PathBuf::from("archive"))
                    .is_match(record, note)
            );
        }
    }

    mod index_record {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn accessors_return_file_record_and_note() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("a.md"), "Filed under #tag.")
                .expect("write file");
            let index = FileIndex::build(temp.path()).expect("build index");
            let file = index.record(Path::new("a.md")).expect("record").clone();
            let note = index.note(Path::new("a.md")).expect("note").clone();

            let record = IndexRecord::new(file.clone(), note.clone());

            assert_eq!(record.file(), &file);
            assert_eq!(record.note(), &note);
        }
    }

    mod query_outcome {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn reports_len_and_is_empty() {
            let empty = QueryOutcome::default();
            assert!(empty.is_empty());
            assert_eq!(empty.len(), 0);

            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("a.md"), "# A").expect("write file");
            let index = FileIndex::build(temp.path()).expect("build index");
            let outcome = index.query(&Source::All);

            assert!(!outcome.is_empty());
            assert_eq!(outcome.len(), 1);
        }

        #[test]
        fn get_returns_record_or_none() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("a.md"), "# A").expect("write file");
            let index = FileIndex::build(temp.path()).expect("build index");
            let outcome = index.query(&Source::All);

            assert!(outcome.get(0).is_some());
            assert_eq!(outcome.get(1), None);
        }

        #[test]
        fn iter_and_into_iterator_yield_records() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("a.md"), "# A").expect("write file");
            let index = FileIndex::build(temp.path()).expect("build index");
            let outcome = index.query(&Source::All);

            let via_iter: Vec<&IndexRecord> = outcome.iter().collect();
            assert_eq!(via_iter.len(), 1);

            let via_ref_into: Vec<&IndexRecord> =
                (&outcome).into_iter().collect();
            assert_eq!(via_ref_into.len(), 1);

            let via_into: Vec<IndexRecord> = outcome.into_iter().collect();
            assert_eq!(via_into.len(), 1);
        }
    }
}
