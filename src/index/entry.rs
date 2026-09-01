//! [`FileIndex`] and its constituent [`FileEntry`] rows.

use std::path::PathBuf;

use crate::{file::FileBase, note::Note};

/// Persisted cache of file records, parsed Note metadata, and derived inbound
/// links.
///
/// Every regular file under the project root contributes one [`FileEntry`]:
/// its [`FileBase`] metadata, and for Markdown files, its parsed [`Note`]
/// plus derived inbound links. A pure value type:
/// [`super::IndexerService`] produces, persists, and loads it; `FileIndex`
/// itself carries no `&Path`.
///
/// Construction always flows through [`super::IndexerService`]'s
/// [`build`](super::IndexerService::build),
/// [`load`](super::IndexerService::load), or
/// [`refresh`](super::IndexerService::refresh) methods, never directly.
#[derive(Clone, Debug)]
pub struct FileIndex {
    entries: Box<[FileEntry]>,
    delta: super::delta::IndexDelta,
}

/// A file's metadata, and — if it is a Note — its parsed content and inbound
/// links. A non-Note file structurally cannot carry inlinks (link resolution
/// only ever targets a Note's own path), so inlinks live inside the boxed
/// `NoteEntry`, not as a sibling field every entry carries regardless.
#[derive(Clone, Debug, PartialEq)]
pub struct FileEntry {
    pub(super) base: FileBase,
    pub(super) note: Option<Box<NoteEntry>>,
}

/// Boxed together with its inlinks so a non-Note `FileEntry` costs one
/// pointer instead of a full `Note`-shaped shell. `Note` itself stays a pure
/// single-file parse result — inlinks are index-level and cross-file, so
/// they sit beside `Note`, not inside it.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct NoteEntry {
    pub(super) note: Note,
    pub(super) inlinks: Box<[PathBuf]>,
}

impl FileEntry {
    /// Returns this entry's general file metadata.
    #[inline]
    #[must_use]
    pub fn base(&self) -> &FileBase {
        &self.base
    }

    /// Returns parsed Note metadata, or `None` for a non-Markdown file.
    #[inline]
    #[must_use]
    pub(crate) fn note(&self) -> Option<&Note> {
        self.note.as_deref().map(|entry| &entry.note)
    }

    /// Returns project-relative paths of Notes whose wikilinks resolve to
    /// this entry, or an empty slice for a non-Note entry or one with no
    /// inbound links.
    #[inline]
    #[must_use]
    pub(crate) fn inlinks(&self) -> &[PathBuf] {
        self.note.as_deref().map_or(&[], |entry| &entry.inlinks)
    }

    /// Builds a [`FileEntry`] with custom fields for test fixtures.
    #[cfg(any(test, feature = "test-utils"))]
    #[allow(
        dead_code,
        reason = "fixture helper used by tests outside entry.rs"
    )]
    pub(crate) fn new_test(base: FileBase, note: Option<Note>) -> Self {
        Self {
            base,
            note: note.map(|note| {
                Box::new(NoteEntry {
                    note,
                    inlinks: Box::default(),
                })
            }),
        }
    }
}

const _: () = assert!(
    std::mem::size_of::<FileEntry>() <= 128,
    "FileEntry grew past its ~120-byte target — Note must stay boxed (its own \
     shell is 240 bytes); check for an accidentally un-boxed field before \
     raising this bound"
);

impl FileIndex {
    /// Creates an index from its constituent parts.
    ///
    /// Used exclusively by [`super::builder::IndexBuilder`] and
    /// [`super::service::IndexerService::load`] after scanning, parsing, and
    /// inlink derivation are complete.
    pub(super) fn new(
        entries: Box<[FileEntry]>,
        delta: super::delta::IndexDelta,
    ) -> Self {
        Self {
            entries,
            delta,
        }
    }

    /// Returns indexed entries, sorted by path.
    #[inline]
    #[must_use]
    pub fn entries(&self) -> &[FileEntry] {
        &self.entries
    }

    /// Returns the entry at `position`. O(1).
    #[expect(
        clippy::expect_used,
        reason = "RowIndex is always in bounds: values are only constructed \
                  from a valid range over entries"
    )]
    #[inline]
    pub(crate) fn entry_at(&self, position: RowIndex) -> &FileEntry {
        self.entries.get(position.get()).expect("RowIndex is always in bounds")
    }

    /// Returns the persistence plan [`super::store::IndexStore::persist_index`]
    /// uses to choose a full rewrite vs. a row-level incremental write.
    pub(super) fn delta(&self) -> &super::delta::IndexDelta {
        &self.delta
    }
}

/// Position of a [`FileEntry`] within [`FileIndex::entries`].
///
/// Newtype instead of a bare `usize` so a row position can't be confused with
/// an unrelated count (a `--limit` value, a list length) at a call site.
/// `RowIndex` values are only ever constructed from a valid range over
/// [`FileIndex::entries`], so they always address a valid position.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct RowIndex(usize);

impl RowIndex {
    /// Wraps `position`, a row's index into [`FileIndex::entries`].
    #[inline]
    #[must_use]
    pub(crate) const fn new(position: usize) -> Self {
        Self(position)
    }

    /// Returns the wrapped position.
    #[inline]
    #[must_use]
    const fn get(self) -> usize {
        self.0
    }
}

/// Pairs each `entries[i]` with the sources listed for its own path in
/// `inlink_map`, mutating in place. Every target `derive_inlinks` (or a
/// previously-persisted `InlinkMap`, when refresh reuses one unchanged) can
/// produce is guaranteed to be a Note's own path already present in
/// `entries`, so the binary search is expected to always succeed.
pub(super) fn redistribute_inlinks(
    entries: &mut [FileEntry],
    inlink_map: super::inlinks::InlinkMap,
) {
    for (target, sources) in inlink_map {
        if let Ok(index) =
            entries.binary_search_by(|entry| entry.base().path().cmp(&target))
            && let Some(note_entry) =
                entries.get_mut(index).and_then(|entry| entry.note.as_mut())
        {
            note_entry.inlinks = sources.into_boxed_slice();
        }
    }
}

/// Pairs sorted `files` with sorted `notes` (a merge-join over both
/// path-sorted slices — the same two-pointer algorithm `compute_note_positions`
/// used, producing owned `FileEntry` values instead of a position table),
/// then redistributes `inlinks` into each note-bearing entry. Used by
/// [`super::IndexerService::load`], which already has a persisted, already-
/// derived inlink map and does not need [`super::inlinks::derive_inlinks`]'s
/// recomputation.
pub(super) fn assemble_entries(
    files: Vec<FileBase>,
    notes: Vec<Note>,
    inlinks: super::inlinks::InlinkMap,
) -> Box<[FileEntry]> {
    let mut notes_iter = notes.into_iter().peekable();
    let mut entries = Vec::with_capacity(files.len());
    for base in files {
        while notes_iter.peek().is_some_and(|note| note.path() < base.path()) {
            notes_iter.next();
        }
        let note =
            notes_iter.next_if(|note| note.path() == base.path()).map(|note| {
                Box::new(NoteEntry {
                    note,
                    inlinks: Box::default(),
                })
            });
        entries.push(FileEntry {
            base,
            note,
        });
    }
    redistribute_inlinks(&mut entries, inlinks);
    entries.into_boxed_slice()
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::service::IndexerService;

    mod position_lookup {
        use pretty_assertions::assert_eq;

        use super::*;

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
