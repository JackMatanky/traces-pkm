//! File rows persisted by the index.
//!
//! [`super::service::IndexerService`] is the only producer; construction flows
//! through its `build`, `load`, or `refresh` methods.

use std::path::PathBuf;

use super::inlinks::InlinkMap;
use crate::{FileBase, Note};

/// Persisted file records with parsed note metadata and derived inbound links.
///
/// Every regular file under the project root contributes one [`FileEntry`].
/// Markdown files include a parsed [`Note`]; all entries may carry backlinks.
/// [`IndexerService`] produces, persists, and loads it; `FileIndex` itself
/// carries no `&Path`.
///
/// [`IndexerService`]: super::service::IndexerService
#[derive(Clone, Debug)]
pub struct FileIndex {
    entries: Box<[FileEntry]>,
}

impl FileIndex {
    pub(super) fn new(entries: Box<[FileEntry]>) -> Self {
        Self {
            entries,
        }
    }

    /// Assembles an index from sorted `files`, sorted `notes`, and `inlinks`.
    #[cfg(any(test, feature = "test-utils"))]
    #[inline]
    #[must_use]
    pub fn assemble(
        files: Vec<FileBase>,
        notes: Vec<Note>,
        inlinks: InlinkMap,
    ) -> Self {
        Self::assemble_internal(files, notes, inlinks)
    }

    /// Builds an in-memory index from `(path, markdown_source)` pairs for test
    /// fixtures.
    ///
    /// Available in unit tests and under the `test-utils` feature for
    /// integration tests/benches.
    #[cfg(any(test, feature = "test-utils"))]
    #[inline]
    #[must_use]
    pub fn new_test(notes: &[(&str, &str)]) -> Self {
        let mut items = Vec::with_capacity(notes.len());
        items.extend(notes.iter().map(|(path_str, src)| {
            let p = std::path::Path::new(path_str);
            let note = crate::parse_note(p, src);
            let size = u64::try_from(src.len()).unwrap_or(u64::MAX);
            (note, size)
        }));
        items.sort_by(|(a, _), (b, _)| a.path().cmp(b.path()));

        let mut parsed = Vec::with_capacity(items.len());
        let mut files = Vec::with_capacity(items.len());
        for (note, size) in items {
            files.push(FileBase::note_with_size_for_test(note.path(), size));
            parsed.push(note);
        }

        let inlinks = InlinkMap::new(&parsed, &files);
        Self::assemble(files, parsed, inlinks)
    }

    /// Assembles an index from sorted `files`, sorted `notes`, and `inlinks`.
    #[cfg(not(any(test, feature = "test-utils")))]
    #[inline]
    #[must_use]
    pub(crate) fn assemble(
        files: Vec<FileBase>,
        notes: Vec<Note>,
        inlinks: InlinkMap,
    ) -> Self {
        Self::assemble_internal(files, notes, inlinks)
    }

    fn assemble_internal(
        files: Vec<FileBase>,
        notes: Vec<Note>,
        inlinks: InlinkMap,
    ) -> Self {
        let mut notes_iter = notes.into_iter().peekable();
        let mut entries = Vec::with_capacity(files.len());
        for file in files {
            while notes_iter
                .peek()
                .is_some_and(|note| note.path() < file.path())
            {
                notes_iter.next();
            }
            let note = notes_iter.next_if(|note| note.path() == file.path());
            entries.push(FileEntry::new(file, note));
        }
        redistribute_inlinks(&mut entries, inlinks);
        Self::new(entries.into_boxed_slice())
    }

    /// Returns [`FileEntry`]s, sorted by path.
    #[inline]
    #[must_use]
    pub fn entries(&self) -> &[FileEntry] {
        &self.entries
    }

    /// Returns the [`FileEntry`] at `position`.
    ///
    /// # Panics
    ///
    /// Panics if `position` is not a valid row index for this index. Row
    /// indices are only constructed from a valid range over entries.
    #[expect(
        clippy::expect_used,
        reason = "RowIndex is always in bounds: values are only constructed \
                  from a valid range over entries"
    )]
    #[inline]
    pub(crate) fn entry_at(&self, position: RowIndex) -> &FileEntry {
        self.entries.get(position.get()).expect("RowIndex is always in bounds")
    }
}

/// Parsed note metadata and inbound links for one indexed file.
///
/// Notes stay boxed to keep [`FileEntry`]'s stack footprint small (~96 bytes).
/// Inlinks also apply to non-Markdown attachments such as images and PDFs.
#[derive(Clone, Debug, PartialEq)]
pub struct FileEntry {
    file: FileBase,
    note: Option<Box<Note>>,
    inlinks: Box<[PathBuf]>,
}

impl FileEntry {
    pub(super) fn new(file: FileBase, note: Option<Note>) -> Self {
        Self {
            file,
            note: note.map(Box::new),
            inlinks: Box::default(),
        }
    }

    /// Returns the record's [`FileBase`] metadata.
    #[inline]
    #[must_use]
    pub fn file(&self) -> &FileBase {
        &self.file
    }

    /// Returns the parsed [`Note`], or `None` for a non-Markdown file.
    #[inline]
    #[must_use]
    pub fn note(&self) -> Option<&Note> {
        self.note.as_deref()
    }

    /// Returns canonically sorted inbound link paths.
    #[inline]
    #[must_use]
    pub fn inlinks(&self) -> &[PathBuf] {
        &self.inlinks
    }

    pub(super) fn set_inlinks(&mut self, inlinks: Box<[PathBuf]>) {
        self.inlinks = inlinks;
    }
}

/// Position of a [`FileEntry`] within [`FileIndex::entries`].
#[derive(Copy, Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct RowIndex(usize);

impl RowIndex {
    #[inline]
    #[must_use]
    pub(crate) const fn new(position: usize) -> Self {
        Self(position)
    }

    #[inline]
    #[must_use]
    const fn get(self) -> usize {
        self.0
    }
}

/// Moves inlink sources into their matching [`FileEntry`]s.
pub(super) fn redistribute_inlinks(
    entries: &mut [FileEntry],
    inlinks: InlinkMap,
) {
    for (target, sources) in inlinks.into_entries() {
        if let Ok(index) =
            entries.binary_search_by(|entry| entry.file().path().cmp(&target))
            && let Some(entry) = entries.get_mut(index)
        {
            entry.set_inlinks(sources);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::IndexerService;

    mod position_lookup {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn entry_size_stays_under_target() {
            assert!(
                std::mem::size_of::<FileEntry>() <= 160,
                "FileEntry grew past its target: Note must stay boxed (Note \
                 itself is 240 bytes); check for an accidental field before \
                 raising this bound"
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

        #[test]
        fn non_markdown_file_can_carry_inlinks() {
            let temp = tempfile::tempdir().expect("create temp dir");
            std::fs::write(temp.path().join("attachment.png"), [
                0x89, 0x50, 0x4E, 0x47,
            ])
            .expect("write attachment.png");
            std::fs::write(
                temp.path().join("note.md"),
                "# Note\n\n[[attachment.png]]\n",
            )
            .expect("write note.md");
            let index =
                IndexerService::new(temp.path()).build().expect("build index");

            let png_entry = index
                .entries()
                .iter()
                .find(|e| {
                    e.file().path() == std::path::Path::new("attachment.png")
                })
                .expect("attachment entry exists");
            assert!(png_entry.note().is_none());
            assert_eq!(png_entry.inlinks(), [PathBuf::from("note.md")]);
        }
    }
    mod new_test {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn assembles_index_from_note_tuples() {
            let index = FileIndex::new_test(&[
                ("a.md", "# A\nLink to [[b]]"),
                ("b.md", "# B"),
            ]);

            assert_eq!(index.entries().len(), 2);
            let a = index.entry_at(RowIndex::new(0));
            assert_eq!(a.file().name().as_str(), "a");
            assert!(a.note().is_some());

            let b = index.entry_at(RowIndex::new(1));
            assert_eq!(b.file().name().as_str(), "b");
            assert_eq!(b.inlinks().len(), 1);
        }
        #[test]
        fn creates_no_files_on_disk() {
            // The cwd is process-global state; hold CWD_TEST_LOCK so another
            // test's CwdGuard-mediated change cannot be observed mid-walk.
            let _cwd = crate::cli::CwdGuard::same_dir();
            let count_entries =
                || std::fs::read_dir(".").map_or(0, std::iter::Iterator::count);
            let before = count_entries();
            let _index = FileIndex::new_test(&[(
                "ephemeral_test_note.md",
                "# Ephemeral\ncontent with [[link]]",
            )]);
            assert!(!std::path::Path::new("ephemeral_test_note.md").exists());
            assert!(!std::path::Path::new(".traces/index.redb").exists());
            let after = count_entries();
            assert_eq!(before, after);
        }
    }
}
