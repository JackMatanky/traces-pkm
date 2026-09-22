//! Incremental-refresh deltas for path-sorted file and inlink state.

use std::path::PathBuf;

use super::{inlinks::InlinkMap, sort::SortedByPath};
use crate::FileBase;

/// Computed difference between disk files and persisted index metadata.
///
/// Both result slices remain ascending by path: [`Self::compute` performs a
/// single merge over its path-sorted inputs, so upserted and deleted rows
/// inherit that order.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(super) struct FileDelta {
    upserted: Box<[FileBase]>,
    deleted: Box<[FileBase]>,
}

impl FileDelta {
    /// Computes added, modified, and deleted files.
    ///
    /// # Panics
    ///
    /// Panics in debug builds when either input is not ascending by path.
    pub(super) fn compute(
        current: &SortedByPath<FileBase>,
        persisted: &SortedByPath<FileBase>,
    ) -> Self {
        let mut upserted = Vec::new();
        let mut deleted = Vec::new();
        let mut current_iter = current.as_slice().iter().peekable();
        let mut persisted_iter = persisted.as_slice().iter().peekable();
        loop {
            match (current_iter.peek(), persisted_iter.peek()) {
                (Some(c), Some(p)) => match c.path().cmp(p.path()) {
                    std::cmp::Ordering::Less => {
                        upserted.push((*c).clone());
                        current_iter.next();
                    }
                    std::cmp::Ordering::Greater => {
                        deleted.push((*p).clone());
                        persisted_iter.next();
                    }
                    std::cmp::Ordering::Equal => {
                        if *c != *p {
                            upserted.push((*c).clone());
                        }
                        current_iter.next();
                        persisted_iter.next();
                    }
                },
                (Some(c), None) => {
                    upserted.push((*c).clone());
                    current_iter.next();
                }
                (None, Some(p)) => {
                    deleted.push((*p).clone());
                    persisted_iter.next();
                }
                (None, None) => break,
            }
        }
        Self {
            upserted: upserted.into_boxed_slice(),
            deleted: deleted.into_boxed_slice(),
        }
    }

    #[inline]
    #[must_use]
    pub(super) fn is_empty(&self) -> bool {
        self.upserted.is_empty() && self.deleted.is_empty()
    }

    /// Returns added or changed files, ascending by path.
    #[inline]
    #[must_use]
    pub(super) fn upserted(&self) -> &[FileBase] {
        &self.upserted
    }

    /// Returns removed files, ascending by path.
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
    /// Computes added and removed inlink edges.
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

    #[inline]
    #[must_use]
    pub(super) fn is_empty(&self) -> bool {
        self.upserted.is_empty() && self.deleted.is_empty()
    }

    #[inline]
    #[must_use]
    pub(super) fn upserted(&self) -> &[(PathBuf, PathBuf)] {
        &self.upserted
    }

    #[inline]
    #[must_use]
    pub(super) fn deleted(&self) -> &[(PathBuf, PathBuf)] {
        &self.deleted
    }
}

/// Pushes `left - right` source paths for `target` into `diff`.
///
/// Both slices must be sorted; the function performs one merge pass.
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
    use std::{fs, path::Path};

    use super::*;

    mod file_delta {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn reports_a_default_delta_as_empty() {
            let delta = FileDelta::default();
            assert!(delta.is_empty());
        }

        #[test]
        fn returns_deleted_note_when_note_is_removed() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("a.md"), "content").expect("write a");
            let previous = SortedByPath::sorted(
                crate::IndexerService::scan(temp.path()).expect("scan"),
            );
            fs::remove_file(temp.path().join("a.md")).expect("delete a");
            let current = SortedByPath::sorted(
                crate::IndexerService::scan(temp.path()).expect("scan"),
            );

            let delta = FileDelta::compute(&current, &previous);

            let deleted_paths: Vec<_> =
                delta.deleted().iter().map(FileBase::path).collect();
            assert_eq!(deleted_paths, [Path::new("a.md")]);
        }

        #[test]
        fn returns_deleted_file_when_non_note_is_removed() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("image.png"), "fake")
                .expect("write image");
            let previous = SortedByPath::sorted(
                crate::IndexerService::scan(temp.path()).expect("scan"),
            );
            fs::remove_file(temp.path().join("image.png"))
                .expect("delete image");
            let current = SortedByPath::sorted(
                crate::IndexerService::scan(temp.path()).expect("scan"),
            );

            let delta = FileDelta::compute(&current, &previous);

            let deleted_paths: Vec<_> =
                delta.deleted().iter().map(FileBase::path).collect();
            assert_eq!(deleted_paths, [Path::new("image.png")]);
        }

        #[test]
        fn returns_upserted_note_without_deletion_when_contents_change() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("a.md"), "v1").expect("write a");
            let previous = SortedByPath::sorted(
                crate::IndexerService::scan(temp.path()).expect("scan"),
            );
            fs::write(temp.path().join("a.md"), "v2, longer content")
                .expect("rewrite a");
            let current = SortedByPath::sorted(
                crate::IndexerService::scan(temp.path()).expect("scan"),
            );

            let delta = FileDelta::compute(&current, &previous);

            let upserted_paths: Vec<_> =
                delta.upserted().iter().map(FileBase::path).collect();
            assert_eq!(upserted_paths, [Path::new("a.md")]);
            assert_eq!(delta.deleted(), []);
        }

        #[test]
        fn treats_identical_unsorted_inputs_as_unchanged() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("a.md"), "same").expect("write a");
            fs::write(temp.path().join("b.md"), "same").expect("write b");
            let persisted = SortedByPath::sorted(
                crate::IndexerService::scan(temp.path()).expect("scan"),
            );
            let current = persisted.clone();

            assert!(FileDelta::compute(&current, &persisted).is_empty());
        }
    }

    mod inlink {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn reports_a_default_delta_as_empty() {
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
