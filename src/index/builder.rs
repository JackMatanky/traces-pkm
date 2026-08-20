//! Internal build pipeline for [`super::FileIndex`].
//!
//! [`IndexBuilder`] is a **plan** — it holds the scan result and a reuse
//! directive, deferring all note parsing, sorting, and inlink derivation
//! to [`Self::build`]. Callers use [`super::FileIndex::build`] and
//! [`super::FileIndex::refresh`] instead of this type directly.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use super::{
    FileFormat,
    error::IndexBuilderError,
    inlinks::{InlinkMap, derive_inlinks},
    scan,
};
use crate::{file::FileRecord, note::parse_markdown};

/// Build plan for a [`FileIndex`].
///
/// Stores the fresh scan result and (optionally) moved notes from a previous
/// index. All heavy work — note parsing, sorting, inlink derivation — happens
/// once in [`Self::build`], not across intermediate steps.
///
/// # Invariants
///
/// - [`Self::from_scan`] always produces records sorted by path (guaranteed by
///   [`scan::scan_root`]).
/// - [`Self::reuse_unchanged`] consumes the previous index, moving its notes
///   and inlinks into the plan.
/// - [`Self::build`] produces a [`FileIndex`] with sorted records and notes,
///   and correctly derived inlinks (reused when nothing changed, recomputed
///   otherwise).
pub(crate) struct IndexBuilder {
    records: Vec<FileRecord>,
    /// `None` = fresh build (parse all notes at build time).
    /// `Some(records, notes, inlinks)` = refresh (reuse moved notes for
    /// unchanged records, parse only changed ones at build time).
    reuse: Option<ReusePlan>,
}

struct ReusePlan {
    previous_records: Vec<FileRecord>,
    notes: HashMap<PathBuf, crate::note::Note>,
    inlinks: InlinkMap,
}

impl IndexBuilder {
    /// Scans `root` for regular files. Does NOT parse markdown yet — parsing
    /// is deferred to [`Self::build`].
    ///
    /// # Errors
    ///
    /// - [`IndexBuilderError::Scan`] if the directory cannot be read or a
    ///   file's metadata cannot be inspected.
    pub(super) fn from_scan(
        root: &Path,
    ) -> Result<Self, super::FileIndexError> {
        let records = scan::scan_root(root)?;
        Ok(Self {
            records,
            reuse: None,
        })
    }

    /// Consumes `previous` and plans reuse of its notes for unchanged records.
    ///
    /// Moved (not cloned) from `previous`:
    /// - All [`Note`]s, indexed by path for O(1) lookup during build.
    /// - The [`InlinkMap`], reused unchanged if build detects no mutations.
    /// - The previous `records` slice, used during build to detect unchanged
    ///   file metadata.
    ///
    /// Parsing of changed or newly added notes is deferred to [`Self::build`].
    pub(super) fn reuse_unchanged(self, previous: super::FileIndex) -> Self {
        let (previous_records, notes_vec, inlinks) = previous.into_parts();
        let notes: HashMap<_, _> = notes_vec
            .into_iter()
            .map(|n| (n.path().to_path_buf(), n))
            .collect();
        Self {
            records: self.records,
            reuse: Some(ReusePlan {
                previous_records,
                notes,
                inlinks,
            }),
        }
    }

    /// Consumes the plan and produces a [`FileIndex`].
    ///
    /// - **Fresh build** (`reuse: None`): parses every markdown record from
    ///   disk, sorts notes, derives inlinks.
    /// - **Refresh** (`reuse: Some`): for each markdown record, reuses the
    ///   moved note if the file metadata is unchanged, otherwise parses from
    ///   disk. Sorts notes, then derives inlinks only if any note was reparsed
    ///   or the record set changed.
    ///
    /// # Errors
    ///
    /// - [`IndexBuilderError::NoteParse`] if a markdown file cannot be read.
    /// - [`IndexBuilderError::MissingNote`] if a matched record's note is
    ///   absent from the moved notes map (indicates a logic bug).
    pub(super) fn build(
        self,
        root: &Path,
    ) -> Result<super::FileIndex, IndexBuilderError> {
        let Self {
            records,
            reuse,
        } = self;
        match reuse {
            None => Self::build_fresh(records, root),
            Some(reuse) => Self::build_with_reuse(records, root, reuse),
        }
    }

    fn build_fresh(
        records: Vec<FileRecord>,
        root: &Path,
    ) -> Result<super::FileIndex, IndexBuilderError> {
        let mut notes = Vec::new();
        for record in &records {
            if record.format() == FileFormat::Note {
                notes.push(parse_note(root, record)?);
            }
        }
        notes.sort_by(|a, b| a.path().cmp(b.path()));
        let inlinks = derive_inlinks(&notes);
        Ok(super::FileIndex {
            records,
            notes,
            inlinks,
        })
    }

    fn build_with_reuse(
        records: Vec<FileRecord>,
        root: &Path,
        mut reuse: ReusePlan,
    ) -> Result<super::FileIndex, IndexBuilderError> {
        let mut notes = Vec::with_capacity(records.len());
        let mut dirty = false;
        // Precondition: records and reuse.previous_records are path-sorted
        // (guaranteed by scan_root).
        let mut prev_iter = reuse.previous_records.iter().peekable();

        for record in &records {
            while prev_iter.peek().is_some_and(|p| p.path() < record.path()) {
                prev_iter.next();
            }

            if record.format() != FileFormat::Note {
                continue;
            }

            let unchanged = prev_iter
                .peek()
                .is_some_and(|p| p.path() == record.path() && **p == *record);

            if unchanged {
                let note =
                    reuse.notes.remove(record.path()).ok_or_else(|| {
                        IndexBuilderError::MissingNote {
                            path: record.path().to_path_buf(),
                        }
                    })?;
                notes.push(note);
            } else {
                dirty = true;
                notes.push(parse_note(root, record)?);
            }
        }

        notes.sort_by(|a, b| a.path().cmp(b.path()));

        // Inlinks depend on every Note's outlinks (ambiguous link resolution
        // considers the full set). Recompute when the record set changed or any
        // note was reparsed.
        let records_changed = records != reuse.previous_records;
        let inlinks = if dirty || records_changed {
            derive_inlinks(&notes)
        } else {
            reuse.inlinks
        };

        Ok(super::FileIndex {
            records,
            notes,
            inlinks,
        })
    }
}

/// Reads and parses the markdown file for `record`.
fn parse_note(
    root: &Path,
    record: &FileRecord,
) -> Result<crate::note::Note, IndexBuilderError> {
    let full_path = root.join(record.path());
    let content = std::fs::read_to_string(&full_path).map_err(|source| {
        IndexBuilderError::NoteParse {
            path: full_path,
            source,
        }
    })?;
    Ok(parse_markdown(record.path(), &content))
}
