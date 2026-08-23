//! [`FileIndex`] data structure and its borrowed entry view.

use std::path::{Path, PathBuf};

use super::InlinkMap;
use crate::{file::FileRecord, note::Note};

/// Persisted cache of file records, parsed Note metadata, and derived inbound
/// links.
///
/// Every regular file under the project root contributes a [`FileRecord`].
/// Markdown files also contribute a [`Note`], accessible through
/// [`Self::notes`]. A pure value type: [`super::IndexerService`] produces,
/// persists, and loads it; `FileIndex` itself carries no `&Path`.
#[derive(Clone, Debug)]
pub struct FileIndex {
    pub(super) records: Vec<FileRecord>,
    pub(super) notes: Vec<Note>,
    pub(super) inlinks: InlinkMap,
}

impl FileIndex {
    /// Creates an index from its constituent parts.
    pub(crate) fn new(
        records: Vec<FileRecord>,
        notes: Vec<Note>,
        inlinks: InlinkMap,
    ) -> Self {
        Self {
            records,
            notes,
            inlinks,
        }
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
    pub fn notes(&self) -> &[Note] {
        &self.notes
    }

    /// Returns the inbound link map.
    #[inline]
    #[must_use]
    pub(crate) fn inlinks(&self) -> &InlinkMap {
        &self.inlinks
    }

    /// Returns the [`Note`] for the note at `path`, if indexed.
    #[inline]
    #[must_use]
    #[cfg(test)]
    pub(crate) fn note(&self, path: &Path) -> Option<&Note> {
        find_by_path(&self.notes, path)
    }

    /// Returns borrowed entries pairing each file record with its Note and
    /// inbound links.
    #[inline]
    pub(crate) fn entries(
        &self,
    ) -> impl Iterator<Item = FileIndexEntry<'_>> + '_ {
        FileIndexEntryIter::new(self)
    }
}

/// Binary-searches path-sorted `notes` for an exact path match.
///
/// Shared by the [`super::inlinks`] submodule, which needs the same search
/// over a bare `&[Note]` slice while resolving link targets during
/// [`super::IndexerService::build`]/[`super::IndexerService::refresh`].
pub(super) fn find_by_path<'a>(
    notes: &'a [Note],
    path: &Path,
) -> Option<&'a Note> {
    let idx = notes.binary_search_by(|note| note.path().cmp(path)).ok()?;
    notes.get(idx)
}

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
    fn new(index: &'a FileIndex) -> Self {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::service::IndexerService;

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
            std::fs::write(temp.path().join("a.md"), "# A").expect("write a");
            std::fs::write(temp.path().join("b.md"), "# B").expect("write b");
            std::fs::write(temp.path().join("c.md"), "# C").expect("write c");
            let index =
                IndexerService::new(temp.path()).build().expect("build index");

            assert_eq!(
                index.note(Path::new("b.md")).map(Note::path),
                Some(Path::new("b.md"))
            );
        }
    }
}
