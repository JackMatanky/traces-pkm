//! Diffing algorithms for incremental refresh: two-pointer merges over
//! path-sorted [`FileBase`] and [`InlinkMap`] state.
//!
//! [`super::cache::RefreshCache`] wraps these free functions against its held
//! previous state. Keeping them separate enables unit testing against plain
//! `(current, previous)` values without an
//! [`IndexStore`][super::store::IndexStore] fixture.
//!
//! [`IncrementalDelta`]: struct@IncrementalDelta

use std::path::PathBuf;

use super::{FileFormat, inlinks::InlinkMap};
use crate::file::FileBase;

/// Per-path persistence plan produced by
/// [`super::builder::IndexBuilder::build`].
///
/// [`super::store::IndexStore::persist_index`] reads this to choose between
/// a full rewrite ([`Full`]) or row-level incremental write ([`Incremental`]).
///
/// Boxing the `Incremental` payload shrinks `IndexDelta` from 96 to 8 bytes:
/// every [`Full`] build (the common first-time case) avoids paying for four
/// inline `Vec`s.
///
/// `Full` and `Incremental` are not interchangeable even when an incremental
/// diff is empty. `Full` unconditionally wipes all tables before rewriting;
/// `Incremental` only deletes paths its diff names. A `Full`-built index
/// retagged `Incremental` against fabricated empty state would silently orphan
/// deleted rows.
///
/// [`Full`]: IndexDelta::Full
/// [`Incremental`]: IndexDelta::Incremental
#[derive(Clone, Debug)]
pub(crate) enum IndexDelta {
    /// Produced by a fresh build: no previous state exists to diff against.
    Full,
    /// Produced by a refresh that reused a previous index.
    Incremental(Box<IncrementalDelta>),
}

/// The changed-path plan behind [`IndexDelta::Incremental`].
///
/// Produced by [`RefreshCache`]'s diffing pass. Each field names the paths that
/// changed since the last persist:
///
/// - `upserted` and `deleted` cover [`crate::file::FileBase`] and
///   [`crate::note::Note`] rows.
/// - `links_upserted` and `links_deleted` cover the
///   [`super::inlinks::InlinkMap`] multimap table.
///
/// [`IndexStore::persist_incremental`] reads these fields to patch only the
/// changed rows instead of rewriting the entire database.
///
/// [`RefreshCache`]: super::cache::RefreshCache
/// [`IndexStore::persist_incremental`]: super::store::IndexStore::persist_incremental
#[derive(Clone, Debug)]
pub(crate) struct IncrementalDelta {
    /// Paths whose `FileBase` (and `Note`, if applicable) must be upserted
    /// into `FILES`/`NOTES`, added or metadata-changed since the last
    /// persist.
    pub(crate) upserted: Vec<PathBuf>,
    /// Paths removed since the last persist, must be deleted from
    /// `FILES`/`NOTES`.
    pub(crate) deleted: Vec<PathBuf>,
    /// Target paths in `LINKS` whose source set is new or changed; `None` when
    /// inlinks were reused unchanged (nothing to write).
    pub(crate) links_upserted: Option<Vec<PathBuf>>,
    /// Target paths removed from `LINKS`, always present alongside
    /// `links_upserted` (both `None`/both populated; empty `Vec` is a valid
    /// "no removals" case, distinct from `None`'s "inlinks unchanged,
    /// nothing computed").
    pub(crate) links_deleted: Vec<PathBuf>,
}

impl IncrementalDelta {
    /// True if this delta names no changes at all.
    ///
    /// Persisting an empty delta would open a write transaction only to commit
    /// nothing, so callers short-circuit.
    pub(crate) fn is_empty(&self) -> bool {
        self.upserted.is_empty()
            && self.deleted.is_empty()
            && self.links_deleted.is_empty()
            && self.links_upserted.as_ref().is_none_or(Vec::is_empty)
    }
}

/// Diffs two path-sorted `FileBase` slices via a two-pointer merge, returning
/// current-side paths that are new or changed (`upserted`), previous-side paths
/// absent from `current` (`deleted`), and whether any deleted entry was a Note
/// (the trailing `bool`). A deleted Note always forces an inlink recompute; an
/// upserted Note's contribution to staleness depends on its outlinks, which
/// needs Note content this function structurally doesn't have, so it is
/// deliberately not folded in here (see
/// [`super::cache::RefreshCache::reconcile_note`]'s backdating).
pub(super) fn diff_files(
    current: &[FileBase],
    previous: &[FileBase],
) -> (Vec<PathBuf>, Vec<PathBuf>, bool) {
    let mut upserted = Vec::new();
    let mut deleted = Vec::new();
    let mut has_deleted_note = false;
    let mut cur = current.iter().peekable();
    let mut prev = previous.iter().peekable();
    loop {
        match (cur.peek(), prev.peek()) {
            (Some(c), Some(p)) => match c.path().cmp(p.path()) {
                std::cmp::Ordering::Less => {
                    upserted.push(c.path().to_path_buf());
                    cur.next();
                }
                std::cmp::Ordering::Greater => {
                    if p.format() == FileFormat::Note {
                        has_deleted_note = true;
                    }
                    deleted.push(p.path().to_path_buf());
                    prev.next();
                }
                std::cmp::Ordering::Equal => {
                    if *c != *p {
                        upserted.push(c.path().to_path_buf());
                    }
                    cur.next();
                    prev.next();
                }
            },
            (Some(c), None) => {
                upserted.push(c.path().to_path_buf());
                cur.next();
            }
            (None, Some(p)) => {
                if p.format() == FileFormat::Note {
                    has_deleted_note = true;
                }
                deleted.push(p.path().to_path_buf());
                prev.next();
            }
            (None, None) => break,
        }
    }
    (upserted, deleted, has_deleted_note)
}

/// Diffs two target-keyed inlink maps by source-set membership (order
/// independent; [`derive_inlinks`](super::inlinks::derive_inlinks)'s output and
/// a redb-loaded map are not guaranteed to list one target's sources in the
/// same order even when the set is identical). Returns target paths whose
/// source set is new or changed (`upserted`) and target paths removed entirely
/// (`deleted`).
pub(super) fn diff_inlinks(
    previous: &InlinkMap,
    current: &InlinkMap,
) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let mut upserted = Vec::new();
    for (target, sources) in current {
        let changed = match previous.get(target) {
            Some(prev_sources) => {
                let mut prev_sorted: Vec<_> = prev_sources.iter().collect();
                let mut cur_sorted: Vec<_> = sources.iter().collect();
                prev_sorted.sort_unstable();
                cur_sorted.sort_unstable();
                prev_sorted != cur_sorted
            }
            None => true,
        };
        if changed {
            upserted.push(target.clone());
        }
    }
    let deleted = previous
        .keys()
        .filter(|target| !current.contains_key(*target))
        .cloned()
        .collect();
    (upserted, deleted)
}

#[cfg(test)]
mod tests {
    use super::*;

    mod is_empty {
        use rstest::rstest;

        use super::*;

        /// Builds an [`IncrementalDelta`] with all-empty fields except the one
        /// under test, so each case isolates exactly one field's contribution
        /// to [`IncrementalDelta::is_empty`].
        fn delta(
            upserted: Vec<PathBuf>,
            deleted: Vec<PathBuf>,
            links_upserted: Option<Vec<PathBuf>>,
            links_deleted: Vec<PathBuf>,
        ) -> IncrementalDelta {
            IncrementalDelta {
                upserted,
                deleted,
                links_upserted,
                links_deleted,
            }
        }

        #[test]
        fn is_true_when_every_field_is_empty_or_none() {
            let delta = delta(vec![], vec![], None, vec![]);

            assert!(delta.is_empty());
        }

        #[test]
        fn is_true_when_links_upserted_is_an_empty_vec_not_none() {
            // `Some(vec![])` ("recomputed, nothing changed") must count as
            // empty alongside `None` ("never recomputed") — the two are
            // different reasons for the same "nothing to write" outcome.
            let delta = delta(vec![], vec![], Some(vec![]), vec![]);

            assert!(delta.is_empty());
        }

        #[rstest]
        #[case::upserted(delta(vec![PathBuf::from("a.md")], vec![], None, vec![]))]
        #[case::deleted(delta(vec![], vec![PathBuf::from("a.md")], None, vec![]))]
        #[case::links_deleted(delta(vec![], vec![], None, vec![PathBuf::from("a.md")]))]
        #[case::links_upserted_nonempty(delta(
            vec![],
            vec![],
            Some(vec![PathBuf::from("a.md")]),
            vec![]
        ))]
        fn is_false_when_any_field_names_a_change(
            #[case] delta: IncrementalDelta,
        ) {
            assert!(!delta.is_empty());
        }
    }

    mod diff_files {
        use std::fs;

        use pretty_assertions::assert_eq;

        use super::*;
        use crate::index::IndexerService;

        #[test]
        fn deleted_note_sets_has_deleted_note() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("a.md"), "content").expect("write a");
            let previous =
                IndexerService::new(temp.path()).scan().expect("scan");
            fs::remove_file(temp.path().join("a.md")).expect("delete a");
            let current =
                IndexerService::new(temp.path()).scan().expect("scan");

            let (_, deleted, has_deleted_note) =
                diff_files(&current, &previous);

            assert_eq!(deleted, [PathBuf::from("a.md")]);
            assert!(has_deleted_note);
        }

        #[test]
        fn deleted_non_note_file_does_not_set_has_deleted_note() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("image.png"), "fake")
                .expect("write image");
            let previous =
                IndexerService::new(temp.path()).scan().expect("scan");
            fs::remove_file(temp.path().join("image.png"))
                .expect("delete image");
            let current =
                IndexerService::new(temp.path()).scan().expect("scan");

            let (_, deleted, has_deleted_note) =
                diff_files(&current, &previous);

            assert_eq!(deleted, [PathBuf::from("image.png")]);
            assert!(!has_deleted_note);
        }

        #[test]
        fn upserted_note_does_not_set_has_deleted_note() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("a.md"), "v1").expect("write a");
            let previous =
                IndexerService::new(temp.path()).scan().expect("scan");
            fs::write(temp.path().join("a.md"), "v2, longer content")
                .expect("rewrite a");
            let current =
                IndexerService::new(temp.path()).scan().expect("scan");

            let (upserted, deleted, has_deleted_note) =
                diff_files(&current, &previous);

            assert_eq!(upserted, [PathBuf::from("a.md")]);
            assert!(deleted.is_empty());
            assert!(
                !has_deleted_note,
                "upserted-Note staleness is decided elsewhere (backdating), \
                 not by diff_files"
            );
        }
    }

    mod diff_inlinks {
        use std::collections::HashMap;

        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn detects_new_and_removed_targets() {
            let previous: InlinkMap = HashMap::from([(
                PathBuf::from("a.md"),
                vec![PathBuf::from("x.md")],
            )]);
            let current: InlinkMap = HashMap::from([(
                PathBuf::from("b.md"),
                vec![PathBuf::from("x.md")],
            )]);

            let (upserted, deleted) = diff_inlinks(&previous, &current);

            assert_eq!(upserted, [PathBuf::from("b.md")]);
            assert_eq!(deleted, [PathBuf::from("a.md")]);
        }

        #[test]
        fn ignores_source_order_differences() {
            let previous: InlinkMap =
                HashMap::from([(PathBuf::from("a.md"), vec![
                    PathBuf::from("x.md"),
                    PathBuf::from("y.md"),
                ])]);
            let current: InlinkMap =
                HashMap::from([(PathBuf::from("a.md"), vec![
                    PathBuf::from("y.md"),
                    PathBuf::from("x.md"),
                ])]);

            let (upserted, deleted) = diff_inlinks(&previous, &current);

            assert!(upserted.is_empty());
            assert!(deleted.is_empty());
        }
    }
}
