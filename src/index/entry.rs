//! Borrowed entry view over a [`FileIndex`].

use std::path::PathBuf;

use super::{FileIndex, InlinkMap};
use crate::{file::FileRecord, note::Note};

/// Borrowed file row paired with optional parsed Note data and inbound links.
#[derive(Copy, Clone)]
pub(crate) struct FileIndexEntry<'a> {
    file: &'a FileRecord,
    note: Option<&'a Note>,
    inlinks: &'a [PathBuf],
}

impl<'a> FileIndexEntry<'a> {
    /// Returns the indexed file record.
    #[inline]
    pub(crate) const fn file(&self) -> &'a FileRecord {
        self.file
    }

    /// Returns parsed Note data when this entry is a Markdown file.
    #[inline]
    pub(crate) const fn note(&self) -> Option<&'a Note> {
        self.note
    }

    /// Returns project-relative paths for Notes linking to this entry.
    #[inline]
    pub(crate) const fn inlinks(&self) -> &'a [PathBuf] {
        self.inlinks
    }
}

/// Iterator over [`FileIndexEntry`] values.
struct FileIndexEntryIter<'a> {
    records: std::slice::Iter<'a, FileRecord>,
    notes: std::iter::Peekable<std::slice::Iter<'a, Note>>,
    inlinks: &'a InlinkMap,
}

impl<'a> FileIndexEntryIter<'a> {
    pub(super) fn new(index: &'a FileIndex) -> Self {
        Self {
            records: index.records.iter(),
            notes: index.notes.iter().peekable(),
            inlinks: &index.inlinks,
        }
    }
}

impl<'a> Iterator for FileIndexEntryIter<'a> {
    type Item = FileIndexEntry<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        let file = self.records.next()?;
        while self.notes.peek().is_some_and(|note| note.path() < file.path()) {
            self.notes.next();
        }
        let note = self.notes.next_if(|note| note.path() == file.path());
        let inlinks = self
            .inlinks
            .get(file.path())
            .map(Vec::as_slice)
            .unwrap_or_default();
        Some(FileIndexEntry {
            file,
            note,
            inlinks,
        })
    }
}

impl FileIndex {
    /// Returns borrowed entries pairing each file record with its Note and
    /// inbound links.
    #[inline]
    pub(crate) fn entries(
        &self,
    ) -> impl Iterator<Item = FileIndexEntry<'_>> + '_ {
        FileIndexEntryIter::new(self)
    }
}
