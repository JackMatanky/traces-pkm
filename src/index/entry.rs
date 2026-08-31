//! [`FileIndex`] data structure and its borrowed entry view.

use std::path::Path;

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
    pub(super) files: Vec<FileBase>,
    pub(super) notes: Vec<Note>,
    /// Pairs `files[i]` with its Note's position in `notes`, or `None`.
    /// Computed once (see [`compute_note_positions`]) so [`Self::note_at`]
    /// is O(1) instead of re-deriving the pairing via `Path` comparison.
    note_positions: Vec<Option<NoteIndex>>,
    pub(super) inlinks: InlinkMap,
    pub(super) delta: super::delta::IndexDelta,
}

impl FileIndex {
    /// Creates an index from its constituent parts.
    ///
    /// Used exclusively by [`super::builder::IndexBuilder`] after scanning,
    /// parsing, and inlink derivation are complete.
    pub(crate) fn new(
        files: Vec<FileBase>,
        notes: Vec<Note>,
        inlinks: InlinkMap,
        delta: super::delta::IndexDelta,
    ) -> Self {
        let note_positions = compute_note_positions(&files, &notes);
        Self {
            files,
            notes,
            note_positions,
            inlinks,
            delta,
        }
    }

    /// Returns indexed [`FileBase`]s, sorted by path.
    #[inline]
    #[must_use]
    pub fn files(&self) -> &[FileBase] {
        &self.files
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
    ///
    /// Test-only: production code resolves a row's Note through
    /// [`Self::note_at`], which is O(1); this path-based lookup remains for
    /// tests and fixtures that only have a `&Path` to hand, not a
    /// `RowIndex`.
    #[cfg(test)]
    #[inline]
    #[must_use]
    pub(crate) fn note(&self, path: &Path) -> Option<&Note> {
        find_by_path(&self.notes, path)
    }

    /// Returns the [`FileBase`] at `position`. O(1).
    #[expect(
        clippy::expect_used,
        reason = "RowIndex is always in bounds: values are only constructed \
                  from a valid range over files"
    )]
    #[inline]
    pub(crate) fn file_at(&self, position: RowIndex) -> &FileBase {
        self.files.get(position.get()).expect("RowIndex is always in bounds")
    }

    /// Returns the Note at `position`'s row, or `None` if that file has no
    /// parsed Note. O(1) — indexes the precomputed `files` -> `notes` map
    /// instead of re-deriving the pairing via `Path` comparison.
    #[expect(
        clippy::expect_used,
        reason = "RowIndex and NoteIndex are always in bounds: constructed \
                  only from valid ranges over files and notes"
    )]
    #[inline]
    pub(crate) fn note_at(&self, position: RowIndex) -> Option<&Note> {
        self.note_positions.get(position.get()).copied().flatten().map(
            |NoteIndex(i)| {
                self.notes.get(i).expect("NoteIndex is always in bounds")
            },
        )
    }

    /// Returns the persistence plan [`super::store::IndexStore::persist_index`]
    /// uses to choose a full rewrite vs. a row-level incremental write.
    pub(super) fn delta(&self) -> &super::delta::IndexDelta {
        &self.delta
    }
}

/// Position of a [`FileBase`] within [`FileIndex::files`].
///
/// Newtype instead of a bare `usize` so a row position can't be confused
/// with an unrelated count (a `--limit` value, a list length) at a call
/// site. `RowIndex` values are only ever constructed from a valid range over
/// [`FileIndex::files`], so they always address a valid position.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct RowIndex(usize);

impl RowIndex {
    /// Wraps `position`, a row's index into [`FileIndex::files`].
    #[inline]
    #[must_use]
    pub(crate) const fn new(position: usize) -> Self {
        Self(position)
    }

    /// Returns the wrapped position.
    #[inline]
    #[must_use]
    pub(crate) const fn get(self) -> usize {
        self.0
    }
}

/// Position of a [`Note`] within [`FileIndex::notes`]. Private: never leaves
/// this file — [`FileIndex::note_at`] returns `&Note`, not an index, so
/// nothing outside this module needs to know a note's position, only a
/// row's.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
struct NoteIndex(usize);

/// Pairs each `files[i]` with its Note's position in `notes`, or `None` if
/// that file has no Note. A single merge-join pass over both path-sorted
/// slices, computed once at construction — every later
/// [`FileIndex::note_at`] call is then a plain array index.
fn compute_note_positions(
    files: &[FileBase],
    notes: &[Note],
) -> Vec<Option<NoteIndex>> {
    let mut note_positions = Vec::with_capacity(files.len());
    let mut notes_iter = notes.iter().enumerate().peekable();
    for file in files {
        while notes_iter
            .peek()
            .is_some_and(|(_, note)| note.path() < file.path())
        {
            notes_iter.next();
        }
        let position = notes_iter
            .next_if(|(_, note)| note.path() == file.path())
            .map(|(i, _)| NoteIndex(i));
        note_positions.push(position);
    }
    note_positions
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

    mod position_lookup {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn file_at_and_note_at_agree_with_files_and_note_by_path() {
            let temp = tempfile::tempdir().expect("create temp dir");
            std::fs::write(temp.path().join("a.md"), "# A").expect("write a");
            std::fs::write(temp.path().join("b.txt"), "plain text")
                .expect("write b.txt");
            std::fs::write(temp.path().join("c.md"), "# C").expect("write c");
            let index =
                IndexerService::new(temp.path()).build().expect("build index");

            for (i, file) in index.files().iter().enumerate() {
                let position = RowIndex::new(i);
                assert_eq!(index.file_at(position), file);
                assert_eq!(index.note_at(position), index.note(file.path()));
            }
        }

        #[test]
        fn note_at_returns_none_for_a_non_markdown_file() {
            let temp = tempfile::tempdir().expect("create temp dir");
            std::fs::write(temp.path().join("plain.txt"), "no frontmatter")
                .expect("write plain.txt");
            let index =
                IndexerService::new(temp.path()).build().expect("build index");

            assert_eq!(index.files().len(), 1);
            assert_eq!(index.note_at(RowIndex::new(0)), None);
        }
    }
}
