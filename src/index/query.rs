//! Page-level query source selection over indexed markdown Notes.
//!
//! [`Source`] selects which Notes a page-level query includes. [`IndexRecord`]
//! pairs a [`FileRecord`] with its [`Note`] so callers can read both
//! `file.*` fields and Note Metadata from one value. [`QueryOutcome`] is the
//! iterable collection [`super::FileIndex::query`] returns.
//!
//! Covered by [`super`]'s tests through the [`super::FileIndex::query`] seam
//! rather than in isolation here, per the project's `FileIndex` testing
//! decisions.

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
#[derive(Clone, Debug)]
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
#[derive(Clone, Debug, Default)]
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
