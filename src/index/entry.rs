//! [`FileIndex`] data structure and its borrowed entry view.

use std::path::{Path, PathBuf};

use super::InlinkMap;
use crate::{file::FileBase, note::Note};

/// Persisted cache of file records, parsed Note metadata, and derived inbound
/// links.
///
/// Every regular file under the project root contributes a [`FileBase`].
/// Markdown files also contribute a [`Note`], accessible through
/// [`Self::notes`]. A pure value type: [`super::IndexerService`] produces,
/// persists, and loads it; `FileIndex` itself carries no `&Path`.
///
/// Construction always flows through [`super::IndexerService`]'s
/// [`build`](super::IndexerService::build),
/// [`load`](super::IndexerService::load), or
/// [`refresh`](super::IndexerService::refresh) methods, never directly.
#[derive(Clone, Debug)]
pub struct FileIndex {
    pub(super) bases: Vec<FileBase>,
    pub(super) notes: Vec<Note>,
    pub(super) inlinks: InlinkMap,
    pub(super) delta: super::delta::IndexDelta,
}

impl FileIndex {
    /// Creates an index from its constituent parts.
    ///
    /// Used exclusively by [`super::builder::IndexBuilder`] after scanning,
    /// parsing, and inlink derivation are complete.
    pub(crate) fn new(
        bases: Vec<FileBase>,
        notes: Vec<Note>,
        inlinks: InlinkMap,
        delta: super::delta::IndexDelta,
    ) -> Self {
        Self {
            bases,
            notes,
            inlinks,
            delta,
        }
    }

    /// Returns indexed [`FileBase`]s, sorted by path.
    #[inline]
    #[must_use]
    pub fn bases(&self) -> &[FileBase] {
        &self.bases
    }

    /// Returns indexed [`Note`] records, sorted by path.
    #[inline]
    #[must_use]
    pub fn notes(&self) -> &[Note] {
        &self.notes
    }

    /// Returns the inbound link map keyed by target [`Note`] path.
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

    /// Returns borrowed entries pairing each [`FileBase`] with its optional
    /// [`Note`] and inbound links.
    #[inline]
    pub(crate) fn entries(
        &self,
    ) -> impl Iterator<Item = FileIndexEntry<'_>> + '_ {
        FileIndexEntryIter::new(self)
    }

    /// Returns the persistence plan [`super::store::IndexStore::persist_index`]
    /// uses to choose a full rewrite vs. a row-level incremental write.
    pub(super) fn delta(&self) -> &super::delta::IndexDelta {
        &self.delta
    }
}

/// Borrowed file row paired with optional parsed Note data and inbound links.
#[derive(Copy, Clone)]
pub(crate) struct FileIndexEntry<'a> {
    base: &'a FileBase,
    note: Option<&'a Note>,
    inlinks: &'a [PathBuf],
}

impl<'a> FileIndexEntry<'a> {
    /// Returns the [`FileBase`] for this entry's file.
    #[inline]
    pub(crate) const fn base(&self) -> &'a FileBase {
        self.base
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

/// Iterator that pairs each [`FileBase`] with its optional [`Note`] and
/// inlinks via a merge-join over path-sorted slices.
struct FileIndexEntryIter<'a> {
    bases: std::slice::Iter<'a, FileBase>,
    notes: std::iter::Peekable<std::slice::Iter<'a, Note>>,
    inlinks: &'a InlinkMap,
}

impl<'a> FileIndexEntryIter<'a> {
    fn new(index: &'a FileIndex) -> Self {
        Self {
            bases: index.bases.iter(),
            notes: index.notes.iter().peekable(),
            inlinks: &index.inlinks,
        }
    }
}

impl<'a> Iterator for FileIndexEntryIter<'a> {
    type Item = FileIndexEntry<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        let file = self.bases.next()?;
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
            base: file,
            note,
            inlinks,
        })
    }
}

/// Binary-searches path-sorted `notes` for an exact path match.
///
/// Shared by the [`super::inlinks`] submodule, which needs the same search over
/// a bare `&[Note]` slice while resolving link targets during
/// [`super::IndexerService::build`]/[`super::IndexerService::refresh`].
pub(super) fn find_by_path<'a>(
    notes: &'a [Note],
    path: &Path,
) -> Option<&'a Note> {
    let idx = notes.binary_search_by(|note| note.path().cmp(path)).ok()?;
    notes.get(idx)
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
