//! Borrowed entry view over a [`FileIndex`].

use std::path::PathBuf;

use super::{FileIndex, InlinkMap};
use crate::{file::FileRecord, note::Note};

/// Borrowed file row paired with optional parsed Note data and inbound links.
#[derive(Copy, Clone)]
pub(crate) struct FileIndexEntry<'a> {
    pub(crate) file: &'a FileRecord,
    pub(crate) note: Option<&'a Note>,
    pub(crate) inlinks: &'a [PathBuf],
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
