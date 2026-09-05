//! Diffing algorithms for incremental refresh: two-pointer merges over
//! path-sorted [`FileBase`] and [`InlinkMap`] state.

use std::path::PathBuf;

use super::{FileFormat, inlinks::InlinkMap};
use crate::FileBase;

/// Computed difference between disk files and persisted index metadata.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(super) struct FileDiff {
    pub(super) upserted: Box<[FileBase]>,
    pub(super) deleted: Box<[FileBase]>,
    pub(super) has_deleted_note: bool,
}

impl FileDiff {
    /// Computes file additions, modifications, and deletions between `current`
    /// disk files and `persisted` index metadata.
    pub(super) fn compute(
        current: &[FileBase],
        persisted: &[FileBase],
    ) -> Self {
        let mut upserted = Vec::new();
        let mut deleted = Vec::new();
        let mut has_deleted_note = false;
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
                        if p.format() == FileFormat::Note {
                            has_deleted_note = true;
                        }
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
                    if p.format() == FileFormat::Note {
                        has_deleted_note = true;
                    }
                    deleted.push((*p).clone());
                    prev.next();
                }
                (None, None) => break,
            }
        }
        Self {
            upserted: upserted.into_boxed_slice(),
            deleted: deleted.into_boxed_slice(),
            has_deleted_note,
        }
    }

    /// Returns `true` if no files were added, modified, or deleted.
    #[inline]
    #[must_use]
    pub(super) fn is_empty(&self) -> bool {
        self.upserted.is_empty() && self.deleted.is_empty()
    }
}

/// Computed difference in inbound link edges.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(super) struct InlinkDelta {
    pub(super) upserted: Box<[(PathBuf, PathBuf)]>,
    pub(super) deleted: Box<[(PathBuf, PathBuf)]>,
}

impl InlinkDelta {
    /// Computes added and removed `(target, source)` inlink edges between
    /// `current_links` and `persisted_links`.
    pub(super) fn compute(
        current_links: &InlinkMap,
        persisted_links: &InlinkMap,
    ) -> Self {
        let mut upserted = Vec::new();
        for (target, sources) in current_links {
            let prev_sources = persisted_links.get(target);
            for source in sources {
                if prev_sources.is_none_or(|ps| !ps.contains(source)) {
                    upserted.push((target.clone(), source.clone()));
                }
            }
        }
        let mut deleted = Vec::new();
        for (target, sources) in persisted_links {
            let cur_sources = current_links.get(target);
            for source in sources {
                if cur_sources.is_none_or(|cs| !cs.contains(source)) {
                    deleted.push((target.clone(), source.clone()));
                }
            }
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
            let diff = FileDiff::default();
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

            let diff = FileDiff::compute(&current, &previous);

            let deleted_paths: Vec<_> =
                diff.deleted.iter().map(FileBase::path).collect();
            assert_eq!(deleted_paths, [std::path::Path::new("a.md")]);
            assert!(diff.has_deleted_note);
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

            let diff = FileDiff::compute(&current, &previous);

            let deleted_paths: Vec<_> =
                diff.deleted.iter().map(FileBase::path).collect();
            assert_eq!(deleted_paths, [std::path::Path::new("image.png")]);
            assert!(!diff.has_deleted_note);
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

            let diff = FileDiff::compute(&current, &previous);

            let upserted_paths: Vec<_> =
                diff.upserted.iter().map(FileBase::path).collect();
            assert_eq!(upserted_paths, [std::path::Path::new("a.md")]);
            assert!(diff.deleted.is_empty());
            assert!(!diff.has_deleted_note);
        }
    }

    mod inlink_delta {
        use std::collections::HashMap;

        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn returns_true_when_delta_is_empty() {
            let delta = InlinkDelta::default();
            assert!(delta.is_empty());
        }

        #[test]
        fn detects_added_and_removed_inlinks() {
            let previous: InlinkMap = HashMap::from([(
                PathBuf::from("a.md"),
                vec![PathBuf::from("x.md")],
            )]);
            let current: InlinkMap = HashMap::from([(
                PathBuf::from("b.md"),
                vec![PathBuf::from("x.md")],
            )]);

            let delta = InlinkDelta::compute(&current, &previous);

            assert_eq!(delta.upserted.as_ref(), &[(
                PathBuf::from("b.md"),
                PathBuf::from("x.md")
            )]);
            assert_eq!(delta.deleted.as_ref(), &[(
                PathBuf::from("a.md"),
                PathBuf::from("x.md")
            )]);
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

            let delta = InlinkDelta::compute(&current, &previous);

            assert!(delta.is_empty());
        }
    }
}
