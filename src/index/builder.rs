//! Internal build pipeline for [`super::FileIndex`].
//!
//! [`IndexBuilder`] is a **plan**: it holds the scan result and a reuse
//! directive, deferring all note parsing, sorting, and inlink derivation
//! to [`IndexBuilder::build`]. Callers use [`super::IndexerService::build`]
//! and [`super::IndexerService::refresh`] instead of this type directly.

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

/// Build plan for a [`super::FileIndex`].
///
/// Stores the fresh scan result and (optionally) moved notes from a previous
/// index. All heavy work (note parsing, sorting, inlink derivation) happens
/// once in [`Self::build`], not across intermediate steps.
///
/// # Invariants
///
/// - [`Self::from_scan`] always produces records sorted by path (guaranteed by
///   [`scan::scan_root`]).
/// - [`Self::reuse_unchanged`] consumes the previous index, moving its notes
///   and inlinks into the plan.
/// - [`Self::build`] produces a [`super::FileIndex`] with sorted records and
///   notes, and correctly derived inlinks (reused when nothing changed,
///   recomputed otherwise).
pub(crate) struct IndexBuilder {
    records: Vec<FileRecord>,
    /// `None` = fresh build (parse all notes at build time).
    /// `Some(records, notes, inlinks)` = refresh (reuse moved notes for
    /// unchanged records, parse only changed ones at build time).
    reuse: Option<RefreshCache>,
}

impl IndexBuilder {
    /// Scans `root` for regular files. Parsing is deferred to [`Self::build`].
    ///
    /// # Errors
    ///
    /// - [`super::FileIndexError::Io`] if a directory cannot be read or a
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
    /// - All [`crate::note::Note`]s, indexed by path for O(1) lookup during
    ///   build.
    /// - The [`InlinkMap`], reused unchanged if build detects no mutations.
    /// - The previous `records` slice, used during build to detect unchanged
    ///   file metadata.
    ///
    /// Parsing of changed or newly added notes is deferred to [`Self::build`].
    pub(super) fn reuse_unchanged(self, cache: super::FileIndex) -> Self {
        let super::FileIndex {
            records: previous,
            notes: notes_vec,
            inlinks,
        } = cache;
        let notes: HashMap<_, _> = notes_vec
            .into_iter()
            .map(|n| (n.path().to_path_buf(), n))
            .collect();
        Self {
            records: self.records,
            reuse: Some(RefreshCache {
                previous,
                notes,
                inlinks,
            }),
        }
    }

    /// Consumes the plan and produces a [`super::FileIndex`].
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
        mut reuse: RefreshCache,
    ) -> Result<super::FileIndex, IndexBuilderError> {
        let mut notes = Vec::with_capacity(records.len());
        let mut dirty = false;
        // Precondition: records and reuse.previous are path-sorted
        // (guaranteed by scan_root).
        let mut prev_iter = reuse.previous.iter().peekable();

        for record in &records {
            while prev_iter.peek().is_some_and(|p| p.path() < record.path()) {
                // A previously-indexed record no longer exists at this path.
                // Only a deleted Note changes the inbound-link graph; a
                // deleted non-Markdown file (image, PDF, ...) never
                // contributed outlinks.
                if prev_iter
                    .next()
                    .is_some_and(|p| p.format() == FileFormat::Note)
                {
                    dirty = true;
                }
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

        // Any previous entries left unconsumed sort after every current
        // record — trailing deletions. Same Note-only rule as above.
        dirty |= prev_iter.any(|p| p.format() == FileFormat::Note);

        notes.sort_by(|a, b| a.path().cmp(b.path()));

        // Inlinks depend on every Note's outlinks (ambiguous link resolution
        // considers the full set). Recompute only when a Note was added,
        // removed, or reparsed; non-Markdown file changes never affect the
        // link graph, so they must not force a full recompute.
        let inlinks = if dirty {
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

struct RefreshCache {
    previous: Vec<FileRecord>,
    notes: HashMap<PathBuf, crate::note::Note>,
    inlinks: InlinkMap,
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

#[cfg(test)]
mod tests {
    use std::{fs, path::Path};

    use super::*;
    use crate::{file::FileRecord, index::IndexerService, note::Note};

    mod builder {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn from_scan_produces_sorted_records() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("b.md"), "# B").expect("write b");
            fs::write(temp.path().join("a.md"), "# A").expect("write a");

            let index = IndexBuilder::from_scan(temp.path())
                .expect("scan")
                .build(temp.path())
                .expect("build");

            assert_eq!(
                index
                    .records()
                    .iter()
                    .map(FileRecord::path)
                    .collect::<Vec<_>>(),
                [Path::new("a.md"), Path::new("b.md")]
            );
        }

        #[test]
        fn from_scan_parses_markdown_notes() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("note.md"), "- [ ] task")
                .expect("write note");
            fs::write(temp.path().join("readme.txt"), "text")
                .expect("write txt");

            let index = IndexBuilder::from_scan(temp.path())
                .expect("scan")
                .build(temp.path())
                .expect("build");

            assert_eq!(index.records().len(), 2);
            assert_eq!(index.notes().len(), 1);
            assert_eq!(
                index.note(Path::new("note.md")).map(Note::path),
                Some(Path::new("note.md"))
            );
        }

        #[test]
        fn reuse_unchanged_skips_reparsing() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("note.md"), "- [ ] task")
                .expect("write note");
            let built =
                IndexerService::new(temp.path()).build().expect("build index");

            let index = IndexBuilder::from_scan(temp.path())
                .expect("scan")
                .reuse_unchanged(built)
                .build(temp.path())
                .expect("build");

            assert_eq!(
                index
                    .note(Path::new("note.md"))
                    .map(Note::tasks)
                    .map(Iterator::count),
                Some(1)
            );
        }

        #[test]
        fn reuse_unchanged_reparses_changed_notes() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("note.md"), "- [ ] task")
                .expect("write note");
            let built =
                IndexerService::new(temp.path()).build().expect("build index");

            fs::write(temp.path().join("note.md"), "- [ ] task\n- [x] done")
                .expect("rewrite note");

            let index = IndexBuilder::from_scan(temp.path())
                .expect("scan")
                .reuse_unchanged(built)
                .build(temp.path())
                .expect("build");

            assert_eq!(
                index
                    .note(Path::new("note.md"))
                    .map(Note::tasks)
                    .map(Iterator::count),
                Some(2)
            );
        }
    }
}
