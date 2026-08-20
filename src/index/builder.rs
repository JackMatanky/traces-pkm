//! Internal build pipeline for [`super::FileIndex`].
//!
//! [`IndexBuilder`] composes scan → parse → sort → derive-inlinks into
//! testable stages. Callers use [`super::FileIndex::build`] and
//! [`super::FileIndex::refresh`] instead of this type directly.

use std::path::Path;

use super::{FileFormat, FileIndex, inlinks::derive_inlinks, scan};
use crate::{file::FileRecord, note::Note};

/// Composable build pipeline for a [`FileIndex`].
///
/// Construct via [`Self::from_scan`] (fresh build) or chain
/// [`Self::reuse_unchanged`] (refresh) before calling
/// [`Self::sort_and_derive_inlinks`] and [`Self::build`].
pub(crate) struct IndexBuilder {
    records: Vec<FileRecord>,
    notes: Vec<Note>,
}

impl IndexBuilder {
    /// Scans `root` for regular files and parses markdown into [`Note`]s.
    pub(super) fn from_scan(
        root: &Path,
    ) -> Result<Self, super::FileIndexError> {
        let records = scan::scan_root(root)?;
        let mut notes = Vec::new();
        for record in &records {
            if record.format() == FileFormat::Note {
                notes.push(FileIndex::parse_note_file(root, record)?);
            }
        }
        Ok(Self {
            records,
            notes,
        })
    }

    /// Reuses unchanged [`Note`]s from `previous`, re-parsing only those
    /// whose [`FileRecord`] changed. Uses a merge-join over the
    /// path-sorted record slices for O(n + m) reconciliation.
    pub(super) fn reuse_unchanged(
        mut self,
        previous: &FileIndex,
        root: &Path,
    ) -> Self {
        let mut new_notes = Vec::with_capacity(self.notes.len());
        let mut prev_iter = previous.records().iter().peekable();

        for record in &self.records {
            while prev_iter.peek().is_some_and(|p| p.path() < record.path()) {
                prev_iter.next();
            }
            let unchanged = prev_iter
                .peek()
                .is_some_and(|p| p.path() == record.path() && **p == *record);

            if record.format() == FileFormat::Note {
                let note = match unchanged {
                    true => previous
                        .note(record.path())
                        .cloned()
                        .expect("note must exist for matching record"),
                    false => FileIndex::parse_note_file(root, record)
                        .expect("parse failed"),
                };
                new_notes.push(note);
            }
        }

        self.notes = new_notes;
        self
    }

    /// Sorts notes by path and derives inbound link edges.
    pub(super) fn sort_and_derive_inlinks(mut self) -> Self {
        self.notes.sort_by(|a, b| a.path().cmp(b.path()));
        self
    }

    /// Consumes the builder and produces a [`FileIndex`].
    pub(super) fn build(self) -> FileIndex {
        let inlinks = derive_inlinks(&self.notes);
        FileIndex {
            records: self.records,
            notes: self.notes,
            inlinks,
        }
    }
}
