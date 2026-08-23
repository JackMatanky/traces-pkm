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
        debug_assert!(
            notes.windows(2).all(|pair| {
                let [a, b] = pair else {
                    return true;
                };
                a.path() <= b.path()
            }),
            "notes must already be sorted by path: scan_root sorts records, \
             and this loop preserves that order while filtering to \
             Note-format entries"
        );
        let inlinks = derive_inlinks(&notes);
        Ok(super::FileIndex::new(records, notes, inlinks))
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
            dirty |= Self::has_deleted_note(&mut prev_iter, record.path());

            if record.format() != FileFormat::Note {
                continue;
            }

            let (note, reparsed) = Self::reconcile_note(
                record,
                &mut prev_iter,
                &mut reuse.notes,
                root,
            )?;
            dirty |= reparsed;
            notes.push(note);
        }

        // Any previous entries left unconsumed sort after every current
        // record — trailing deletions. Same Note-only rule as
        // has_deleted_note.
        dirty |= prev_iter.any(|p| p.format() == FileFormat::Note);

        debug_assert!(
            notes.windows(2).all(|pair| {
                let [a, b] = pair else {
                    return true;
                };
                a.path() <= b.path()
            }),
            "notes must already be sorted by path: scan_root sorts records, \
             and this loop preserves that order while filtering to \
             Note-format entries"
        );

        // Inlinks depend on every Note's outlinks (ambiguous link resolution
        // considers the full set). Recompute only when a Note was added,
        // removed, or reparsed; non-Markdown file changes never affect the
        // link graph, so they must not force a full recompute.
        let inlinks = if dirty {
            derive_inlinks(&notes)
        } else {
            reuse.inlinks
        };

        Ok(super::FileIndex::new(records, notes, inlinks))
    }

    /// Advances `prev_iter` past every previously-indexed record with a path
    /// strictly less than `current_path` (records deleted since the last
    /// index). Returns `true` if any skipped record was a Note — only a
    /// deleted Note changes the inbound-link graph; a deleted non-Markdown
    /// file (image, PDF, ...) never contributed outlinks.
    fn has_deleted_note(
        prev_iter: &mut std::iter::Peekable<std::slice::Iter<'_, FileRecord>>,
        current_path: &Path,
    ) -> bool {
        let mut deleted_note = false;
        while prev_iter.peek().is_some_and(|p| p.path() < current_path) {
            if prev_iter.next().is_some_and(|p| p.format() == FileFormat::Note)
            {
                deleted_note = true;
            }
        }
        deleted_note
    }

    /// Reuses `record`'s previously-parsed Note if a previously-indexed
    /// record at the same path has unchanged metadata, otherwise parses it
    /// fresh from disk. Returns the resolved Note and whether it was
    /// reparsed (`true`) or reused unchanged (`false`).
    ///
    /// Consumes `prev_iter`'s peeked entry whenever its path matches
    /// `record`'s path (whether reused or superseded) so the entry is never
    /// also counted as a deletion by a later `has_deleted_note` call or the
    /// trailing-deletion check — the previous version of this logic only
    /// peeked and never consumed a matched entry, so every matched Note was
    /// spuriously counted as deleted on the next call, forcing an
    /// unnecessary `derive_inlinks` recompute on almost every refresh.
    fn reconcile_note(
        record: &FileRecord,
        prev_iter: &mut std::iter::Peekable<std::slice::Iter<'_, FileRecord>>,
        prev_notes: &mut HashMap<PathBuf, crate::note::Note>,
        root: &Path,
    ) -> Result<(crate::note::Note, bool), IndexBuilderError> {
        let previous_matches_path =
            prev_iter.peek().is_some_and(|p| p.path() == record.path());
        let unchanged = previous_matches_path
            && prev_iter.peek().is_some_and(|p| **p == *record);

        if previous_matches_path {
            prev_iter.next();
        }

        if unchanged {
            let note = prev_notes.remove(record.path()).ok_or_else(|| {
                IndexBuilderError::MissingNote {
                    path: record.path().to_path_buf(),
                }
            })?;
            Ok((note, false))
        } else {
            Ok((parse_note(root, record)?, true))
        }
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

    mod constructor {
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
    }

    mod inlink_reuse {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn reused_index_preserves_inlinks_when_nothing_changes() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(
                temp.path().join("a.md"),
                "---\ntitle: A\n---\nLink to [[b]].",
            )
            .expect("write a");
            fs::write(temp.path().join("b.md"), "---\ntitle: B\n---\nBody.")
                .expect("write b");

            let first = IndexBuilder::from_scan(temp.path())
                .expect("scan")
                .build(temp.path())
                .expect("build");
            let first_inlinks = first.inlinks().len();

            let second = IndexBuilder::from_scan(temp.path())
                .expect("scan")
                .reuse_unchanged(first)
                .build(temp.path())
                .expect("build");

            assert_eq!(
                second.inlinks().len(),
                first_inlinks,
                "inlinks must be reused when nothing changed"
            );
        }

        #[test]
        fn deleted_non_note_file_does_not_mark_dirty() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(
                temp.path().join("note.md"),
                "---\ntitle: Note\n---\nBody.",
            )
            .expect("write note");
            fs::write(temp.path().join("image.png"), "fake")
                .expect("write image");

            let first = IndexBuilder::from_scan(temp.path())
                .expect("scan")
                .build(temp.path())
                .expect("build");
            let first_inlinks = first.inlinks().len();

            fs::remove_file(temp.path().join("image.png"))
                .expect("delete image");

            let second = IndexBuilder::from_scan(temp.path())
                .expect("scan")
                .reuse_unchanged(first)
                .build(temp.path())
                .expect("build");

            assert_eq!(
                second.inlinks().len(),
                first_inlinks,
                "deleting non-note file must not recompute inlinks"
            );
        }
    }

    mod reuse {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn skips_parse_for_unchanged_records() {
            // Arrange
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(
                temp.path().join("note.md"),
                "---\ntitle: Test\n---\nBody.",
            )
            .expect("write note");

            let first = IndexBuilder::from_scan(temp.path())
                .expect("scan")
                .build(temp.path())
                .expect("build");
            let first_len = first.notes().len();

            // Act
            let second = IndexBuilder::from_scan(temp.path())
                .expect("scan")
                .reuse_unchanged(first)
                .build(temp.path())
                .expect("build");

            // Assert
            assert_eq!(first_len, second.notes().len());
        }

        #[test]
        fn reparse_when_record_content_changes() {
            // Arrange
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(
                temp.path().join("note.md"),
                "---\ntitle: V1\n---\nBody.",
            )
            .expect("write note");

            let first = IndexBuilder::from_scan(temp.path())
                .expect("scan")
                .build(temp.path())
                .expect("build");

            fs::write(
                temp.path().join("note.md"),
                "---\ntitle: V2\n---\nBody.",
            )
            .expect("rewrite note");

            // Act
            let second = IndexBuilder::from_scan(temp.path())
                .expect("scan")
                .reuse_unchanged(first)
                .build(temp.path())
                .expect("build");

            // Assert
            let title = second
                .notes()
                .first()
                .expect("note must exist")
                .frontmatter()
                .and_then(|fm| {
                    fm.get(&crate::field::FieldKey::try_new("title").unwrap())
                        .cloned()
                });
            assert_eq!(
                title,
                Some(crate::note::NoteFieldValue::String("V2".to_owned()))
            );
        }

        #[test]
        fn removes_deleted_notes_from_index() {
            // Arrange
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(
                temp.path().join("note.md"),
                "---\ntitle: Test\n---\nBody.",
            )
            .expect("write note");

            let first = IndexBuilder::from_scan(temp.path())
                .expect("scan")
                .build(temp.path())
                .expect("build");
            assert_eq!(first.notes().len(), 1);

            fs::remove_file(temp.path().join("note.md")).expect("delete note");

            // Act
            let second = IndexBuilder::from_scan(temp.path())
                .expect("scan")
                .reuse_unchanged(first)
                .build(temp.path())
                .expect("build");

            // Assert
            assert_eq!(second.notes().len(), 0, "deleted note must be removed");
        }

        #[test]
        fn includes_newly_added_notes() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("a.md"), "---\ntitle: A\n---\nBody.")
                .expect("write a");

            let first = IndexBuilder::from_scan(temp.path())
                .expect("scan")
                .build(temp.path())
                .expect("build");
            assert_eq!(first.notes().len(), 1);

            fs::write(temp.path().join("b.md"), "---\ntitle: B\n---\nBody.")
                .expect("write b");

            let second = IndexBuilder::from_scan(temp.path())
                .expect("scan")
                .reuse_unchanged(first)
                .build(temp.path())
                .expect("build");

            assert_eq!(second.notes().len(), 2, "new note must be included");
        }

        #[test]
        fn preserves_task_content_for_unchanged_records() {
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
        fn reparses_task_content_when_record_changes() {
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

    mod reconcile_note {
        use super::*;

        #[test]
        fn consumes_the_matched_previous_entry_so_it_is_not_double_counted() {
            // Arrange: one previously-indexed Note, unchanged in the fresh
            // scan.
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("a.md"), "content").expect("write note");
            let previous = scan::scan_root(temp.path()).expect("scan root");
            let record = previous.first().expect("one record");
            let mut prev_iter = previous.iter().peekable();
            let mut prev_notes = HashMap::from([(
                record.path().to_path_buf(),
                crate::note::parse_markdown("a.md", "content"),
            )]);

            // Act
            let (_, reparsed) = IndexBuilder::reconcile_note(
                record,
                &mut prev_iter,
                &mut prev_notes,
                temp.path(),
            )
            .expect("reconcile succeeds");

            // Assert: unchanged, and the matched entry is consumed, not left
            // for the next has_deleted_note/trailing check to miscount as
            // deleted.
            assert!(!reparsed);
            assert!(prev_iter.peek().is_none());
        }

        #[test]
        fn consumes_the_matched_previous_entry_even_when_the_record_changed() {
            // Arrange: previously-indexed Note whose content (and thus size)
            // differs in the fresh scan, at the same path.
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("a.md"), "content").expect("write note");
            let previous = scan::scan_root(temp.path()).expect("scan root");

            fs::write(temp.path().join("a.md"), "different content")
                .expect("rewrite note");
            let current = scan::scan_root(temp.path()).expect("rescan root");
            let record = current.first().expect("one record");
            let mut prev_iter = previous.iter().peekable();
            let mut prev_notes = HashMap::new();

            // Act
            let (_, reparsed) = IndexBuilder::reconcile_note(
                record,
                &mut prev_iter,
                &mut prev_notes,
                temp.path(),
            )
            .expect("reconcile succeeds");

            // Assert: reparsed, and the matched (now-stale) previous entry
            // is still consumed — the doc comment's "whether reused or
            // superseded" claim, exercised on the superseded branch.
            assert!(reparsed);
            assert!(prev_iter.peek().is_none());
        }
    }
}
