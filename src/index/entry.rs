//! [`FileIndex`] and its constituent [`FileEntry`] rows.

use std::path::PathBuf;

use super::{delta::IndexDelta, inlinks::InlinkMap};
use crate::{FileBase, Note};

/// Persisted cache of file records, parsed Note metadata, and derived inbound
/// links.
///
/// Every regular file under the project root contributes one [`FileEntry`]: its
/// [`FileBase`] metadata, and for Markdown files, its parsed [`Note`] plus
/// derived inbound links. [`IndexerService`] produces, persists, and loads it;
/// `FileIndex` itself carries no `&Path`.
///
/// Construction always flows through [`IndexerService`]'s [`build`], [`load`],
/// or [`refresh`] methods, never directly.
///
/// [`IndexerService`]: super::service::IndexerService
/// [`build`]: super::service::IndexerService::build
/// [`load`]: super::service::IndexerService::load
/// [`refresh`]: super::service::IndexerService::refresh
#[derive(Clone, Debug)]
pub struct FileIndex {
    entries: Box<[FileEntry]>,
    delta: IndexDelta,
}

impl FileIndex {
    /// Creates an index from its constituent parts.
    ///
    /// Used exclusively by [`IndexBuilder`] and [`IndexerService::load`] after
    /// scanning, parsing, and inlink derivation are complete.
    ///
    /// [`IndexerService::load`]: super::service::IndexerService::load
    /// [`IndexBuilder`]: super::builder::IndexBuilder
    pub(super) fn new(entries: Box<[FileEntry]>, delta: IndexDelta) -> Self {
        Self {
            entries,
            delta,
        }
    }

    /// Returns [`FileEntry`]s, sorted by path.
    #[inline]
    #[must_use]
    pub fn entries(&self) -> &[FileEntry] {
        &self.entries
    }

    /// Returns the [`FileEntry`] at `position`.
    #[expect(
        clippy::expect_used,
        reason = "RowIndex is always in bounds: values are only constructed \
                  from a valid range over entries"
    )]
    #[inline]
    pub(crate) fn entry_at(&self, position: RowIndex) -> &FileEntry {
        self.entries.get(position.get()).expect("RowIndex is always in bounds")
    }

    /// Returns the [`super::delta::IndexDelta`] that
    /// [`super::store::IndexStore::persist_index`] uses to choose between a
    /// full rewrite and a row-level incremental write.
    pub(super) fn delta(&self) -> &super::delta::IndexDelta {
        &self.delta
    }
}

/// A file's metadata, and (if it is a Note) its parsed content and inbound
/// links. A non-Note file structurally cannot carry inlinks (link resolution
/// only ever targets a Note's own path), so inlinks live inside the boxed
/// `NoteEntry`, not as a sibling field every entry carries regardless.
#[derive(Clone, Debug, PartialEq)]
pub struct FileEntry {
    file: FileBase,
    note: Option<Box<NoteEntry>>,
}

impl FileEntry {
    /// Creates a new [`FileEntry`].
    pub(super) fn new(file: FileBase, note: Option<Note>) -> Self {
        Self {
            file,
            note: note.map(|note| Box::new(NoteEntry::new(note))),
        }
    }

    /// Returns this entry's [`FileBase`] metadata.
    #[inline]
    #[must_use]
    pub fn file(&self) -> &FileBase {
        &self.file
    }

    /// Returns the parsed [`Note`], or `None` for a non-Markdown file.
    #[inline]
    #[must_use]
    pub fn note(&self) -> Option<&Note> {
        self.note.as_deref().map(|entry| &entry.note)
    }

    /// Returns inbound link paths for this entry, or an empty slice if
    /// absent.
    #[inline]
    #[must_use]
    pub(crate) fn inlinks(&self) -> &[PathBuf] {
        self.note.as_deref().map_or(&[], |entry| &entry.inlinks)
    }
}

/// A [`Note`] paired with its inbound links, boxed to keep non-Note `FileEntry`
/// small. Inlinks are index-level and cross-file, so they sit beside `Note`,
/// not inside it.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct NoteEntry {
    note: Note,
    inlinks: Box<[PathBuf]>,
}

impl NoteEntry {
    pub(super) fn new(note: Note) -> Self {
        Self {
            note,
            inlinks: Box::default(),
        }
    }

    pub(super) fn set_inlinks(&mut self, inlinks: Box<[PathBuf]>) {
        self.inlinks = inlinks;
    }
}

/// Position of a [`FileEntry`] within [`FileIndex::entries`].
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct RowIndex(usize);

impl RowIndex {
    /// Creates a [`RowIndex`] for the given position into
    /// [`FileIndex::entries`].
    #[inline]
    #[must_use]
    pub(crate) const fn new(position: usize) -> Self {
        Self(position)
    }

    /// Returns the row index as a `usize`.
    #[inline]
    #[must_use]
    const fn get(self) -> usize {
        self.0
    }
}

/// Merges sorted `files` with sorted `notes`, redistributes `inlinks` into
/// each entry, and returns boxed [`FileEntry`]s. Used by
/// [`super::IndexerService::load`].
pub(super) fn assemble_entries(
    files: Vec<FileBase>,
    notes: Vec<Note>,
    inlinks: InlinkMap,
) -> Box<[FileEntry]> {
    let mut notes_iter = notes.into_iter().peekable();
    let mut entries = Vec::with_capacity(files.len());
    for file in files {
        while notes_iter.peek().is_some_and(|note| note.path() < file.path()) {
            notes_iter.next();
        }
        let note = notes_iter.next_if(|note| note.path() == file.path());
        entries.push(FileEntry::new(file, note));
    }
    redistribute_inlinks(&mut entries, inlinks);
    entries.into_boxed_slice()
}

/// Distributes inlink sources from `inlinks` map into each matching
/// [`FileEntry`].
pub(super) fn redistribute_inlinks(
    entries: &mut [FileEntry],
    inlinks: InlinkMap,
) {
    for (target, sources) in inlinks {
        if let Ok(index) =
            entries.binary_search_by(|entry| entry.file().path().cmp(&target))
            && let Some(note_entry) =
                entries.get_mut(index).and_then(|entry| entry.note.as_mut())
        {
            note_entry.set_inlinks(sources.into_boxed_slice());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::service::IndexerService;

    mod position_lookup {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn entry_size_stays_under_target() {
            assert!(
                std::mem::size_of::<FileEntry>() <= 128,
                "FileEntry grew past its ~120-byte target — Note must stay \
                 boxed (its own shell is 240 bytes); check for an \
                 accidentally un-boxed field before raising this bound"
            );
        }

        #[test]
        fn entry_at_agrees_with_entries_index() {
            let temp = tempfile::tempdir().expect("create temp dir");
            std::fs::write(temp.path().join("a.md"), "# A").expect("write a");
            std::fs::write(temp.path().join("b.txt"), "plain text")
                .expect("write b.txt");
            std::fs::write(temp.path().join("c.md"), "# C").expect("write c");
            let index =
                IndexerService::new(temp.path()).build().expect("build index");

            for (i, entry) in index.entries().iter().enumerate() {
                let position = RowIndex::new(i);
                assert_eq!(index.entry_at(position), entry);
            }
        }

        #[test]
        fn note_returns_none_for_a_non_markdown_file() {
            let temp = tempfile::tempdir().expect("create temp dir");
            std::fs::write(temp.path().join("plain.txt"), "no frontmatter")
                .expect("write plain.txt");
            let index =
                IndexerService::new(temp.path()).build().expect("build index");

            assert_eq!(index.entries().len(), 1);
            assert_eq!(index.entry_at(RowIndex::new(0)).note(), None);
        }
    }
}
