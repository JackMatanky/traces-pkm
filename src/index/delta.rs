//! Diffing algorithms for incremental refresh: two-pointer merges over
//! path-sorted [`FileBase`] and [`InlinkMap`] state.

use std::path::PathBuf;

use super::inlinks::InlinkMap;
use crate::FileBase;

/// Computed difference between disk files and persisted index metadata.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(super) struct IndexDelta {
    upserted: Box<[FileBase]>,
    deleted: Box<[FileBase]>,
}

impl IndexDelta {
    /// Computes file additions, modifications, and deletions between `current`
    /// disk files and `persisted` index metadata.
    pub(super) fn compute(
        current: &[FileBase],
        persisted: &[FileBase],
    ) -> Self {
        let mut upserted = Vec::new();
        let mut deleted = Vec::new();
        let mut cur = current.iter().peekable();
        let mut prev = persisted.iter().peekable();
        loop {
            match (cur.peek(), prev.peek()) {
                (Some(c), Some(p)) => match c.path().cmp(p.path()) {
                    std::cmp::Ordering::Less => {
                        upserted.push((*c).clone());
                        cur.next();
                    }
                    std::cmp::Ordering::Greater => {
                        deleted.push((*p).clone());
                        prev.next();
                    }
                    std::cmp::Ordering::Equal => {
                        if *c != *p {
                            upserted.push((*c).clone());
                        }
                        cur.next();
                        prev.next();
                    }
                },
                (Some(c), None) => {
                    upserted.push((*c).clone());
                    cur.next();
                }
                (None, Some(p)) => {
                    deleted.push((*p).clone());
                    prev.next();
                }
                (None, None) => break,
            }
        }
        Self {
            upserted: upserted.into_boxed_slice(),
            deleted: deleted.into_boxed_slice(),
        }
    }

    /// Returns `true` if no files were added, modified, or deleted.
    #[inline]
    #[must_use]
    pub(super) fn is_empty(&self) -> bool {
        self.upserted.is_empty() && self.deleted.is_empty()
    }

    /// Returns the added or modified files.
    #[inline]
    #[must_use]
    pub(super) fn upserted(&self) -> &[FileBase] {
        &self.upserted
    }

    /// Returns the deleted files.
    #[inline]
    #[must_use]
    pub(super) fn deleted(&self) -> &[FileBase] {
        &self.deleted
    }
}

/// Computed difference in inbound link edges.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(super) struct InlinkDelta {
    upserted: Box<[(PathBuf, PathBuf)]>,
    deleted: Box<[(PathBuf, PathBuf)]>,
}

impl InlinkDelta {
    /// Computes added and removed `(target, source)` inlink edges between
    /// `current_links` and `persisted_links`.
    #[inline]
    #[must_use]
    pub(super) fn compute(
        current_links: &InlinkMap,
        persisted_links: &InlinkMap,
    ) -> Self {
        let mut upserted = Vec::new();
        let mut deleted = Vec::new();

        for (target, cur_sources) in current_links.iter() {
            let prev_sources = persisted_links.inlinks_of(target);
            diff_sorted_sources(
                target,
                cur_sources,
                prev_sources,
                &mut upserted,
            );
        }

        for (target, prev_sources) in persisted_links.iter() {
            let cur_sources = current_links.inlinks_of(target);
            diff_sorted_sources(
                target,
                prev_sources,
                cur_sources,
                &mut deleted,
            );
        }

        Self {
            upserted: upserted.into_boxed_slice(),
            deleted: deleted.into_boxed_slice(),
        }
    }

    /// Returns `true` if no inbound link edges were added or removed.
    #[inline]
    #[must_use]
    pub(super) fn is_empty(&self) -> bool {
        self.upserted.is_empty() && self.deleted.is_empty()
    }

    /// Returns the added inbound link edges.
    #[inline]
    #[must_use]
    pub(super) fn upserted(&self) -> &[(PathBuf, PathBuf)] {
        &self.upserted
    }

    /// Returns the removed inbound link edges.
    #[inline]
    #[must_use]
    pub(super) fn deleted(&self) -> &[(PathBuf, PathBuf)] {
        &self.deleted
    }
}

/// Pushes every `(target, source)` pair present in `left` but absent from
/// `right` into `diff`. Both slices are sorted, so a single merge pass
/// suffices; called twice with swapped arguments by [`InlinkDelta::compute`] to
/// get both additions (`current` minus `persisted`) and removals (`persisted`
/// minus `current`) from the same routine.
fn diff_sorted_sources(
    target: &std::path::Path,
    left: &[PathBuf],
    right: &[PathBuf],
    diff: &mut Vec<(PathBuf, PathBuf)>,
) {
    let mut left_iter = left.iter().peekable();
    let mut right_iter = right.iter().peekable();
    loop {
        match (left_iter.peek(), right_iter.peek()) {
            (Some(&l), Some(&r)) => match l.cmp(r) {
                std::cmp::Ordering::Less => {
                    diff.push((target.to_path_buf(), l.clone()));
                    left_iter.next();
                }
                std::cmp::Ordering::Greater => {
                    right_iter.next();
                }
                std::cmp::Ordering::Equal => {
                    left_iter.next();
                    right_iter.next();
                }
            },
            (Some(&l), None) => {
                diff.push((target.to_path_buf(), l.clone()));
                left_iter.next();
            }
            (None, _) => break,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    mod file_diff {
        use std::fs;

        use pretty_assertions::assert_eq;

        use super::*;
        use crate::IndexerService;

        #[test]
        fn returns_true_when_diff_is_empty() {
            let diff = IndexDelta::default();
            assert!(diff.is_empty());
        }

        #[test]
        fn deleted_note_sets_has_deleted_note() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("a.md"), "content").expect("write a");
            let previous =
                IndexerService::new(temp.path()).scan().expect("scan");
            fs::remove_file(temp.path().join("a.md")).expect("delete a");
            let current =
                IndexerService::new(temp.path()).scan().expect("scan");

            let diff = IndexDelta::compute(&current, &previous);

            let deleted_paths: Vec<_> =
                diff.deleted().iter().map(FileBase::path).collect();
            assert_eq!(deleted_paths, [std::path::Path::new("a.md")]);
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

            let diff = IndexDelta::compute(&current, &previous);

            let deleted_paths: Vec<_> =
                diff.deleted().iter().map(FileBase::path).collect();
            assert_eq!(deleted_paths, [std::path::Path::new("image.png")]);
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

            let diff = IndexDelta::compute(&current, &previous);

            let upserted_paths: Vec<_> =
                diff.upserted().iter().map(FileBase::path).collect();
            assert_eq!(upserted_paths, [std::path::Path::new("a.md")]);
            assert!(diff.deleted().is_empty());
        }
    }

    mod inlink_delta {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn returns_true_when_delta_is_empty() {
            let delta = InlinkDelta::default();
            assert!(delta.is_empty());
        }

        fn make_inlinks(entries: &[(PathBuf, &[PathBuf])]) -> InlinkMap {
            let mut map = std::collections::HashMap::new();
            for (target, sources) in entries {
                let mut sorted = sources.to_vec();
                sorted.sort();
                map.insert(target.clone(), sorted.into_boxed_slice());
            }
            InlinkMap::from_raw(map)
        }

        #[test]
        fn detects_added_and_removed_inlinks() {
            let previous =
                make_inlinks(&[(PathBuf::from("a.md"), &[PathBuf::from(
                    "x.md",
                )])]);
            let current =
                make_inlinks(&[(PathBuf::from("b.md"), &[PathBuf::from(
                    "x.md",
                )])]);

            let delta = InlinkDelta::compute(&current, &previous);

            assert_eq!(delta.upserted(), &[(
                PathBuf::from("b.md"),
                PathBuf::from("x.md")
            )]);
            assert_eq!(delta.deleted(), &[(
                PathBuf::from("a.md"),
                PathBuf::from("x.md")
            )]);
        }

        #[test]
        fn ignores_source_order_differences() {
            let previous = make_inlinks(&[(PathBuf::from("a.md"), &[
                PathBuf::from("x.md"),
                PathBuf::from("y.md"),
            ])]);
            let current = make_inlinks(&[(PathBuf::from("a.md"), &[
                PathBuf::from("y.md"),
                PathBuf::from("x.md"),
            ])]);

            let delta = InlinkDelta::compute(&current, &previous);

            assert!(delta.is_empty());
        }
    }
}
