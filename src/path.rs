//! Validate relative paths and confine them to a root.
//!
//! Main types:
//! - [`RelativePath`] - Relative path accepted by lexical checks only
//! - [`SafeRelativePath`] - Path accepted by lexical and filesystem checks
//! - [`PathError`] - Path validation failure
//!
//! Confinement happens in two phases:
//! - Lexical validation rejects absolute paths, parent-directory components,
//!   non-normal components other than `.`, and paths without a named component
//! - Filesystem validation uses `strict-path` boundary resolution to resolve
//!   symlinks, Windows 8.3 short names, and alternate data streams before the
//!   containment check
//!
//! The returned confined path is the plain join, not the canonicalized path. A
//! symlink changed after validation and before later I/O can still redirect the
//! operation; callers that need race-free writes must use directory handles or
//! platform-specific open-at APIs.

use std::{
    convert::TryFrom,
    path::{Component, Path, PathBuf},
};

use strict_path::{PathBoundary, StrictPathError};
use thiserror::Error;

/// Reports why path validation failed.
#[derive(Debug, Error)]
pub(crate) enum PathError {
    /// Rejects an absolute candidate path.
    #[error("path is absolute, expected a relative path")]
    Absolute,
    /// Rejects a candidate outside the expected root.
    ///
    /// Returned when a rooted walk path does not begin with `root`, or when
    /// `strict-path` resolves a confined candidate outside the root boundary.
    #[error("path is outside the root directory")]
    OutsideRoot,
    /// Rejects a lexically unsafe relative path.
    ///
    /// Returned when the candidate contains `..`, contains a component other
    /// than a plain name or `.`, or has no plain-name component.
    #[error(
        "path contains an unsafe component (such as `..`) or has no named \
         component"
    )]
    UnsafeComponent,
    /// Reports that `strict-path` filesystem validation could not be completed.
    #[error(transparent)]
    StrictPath(#[from] StrictPathError),
}

impl PathError {
    /// Routes a confinement failure to an escape or verification outcome.
    ///
    /// Uses `escape` for [`Self::Absolute`], [`Self::UnsafeComponent`], and
    /// [`Self::OutsideRoot`]. Uses `unverifiable` for [`Self::StrictPath`] and
    /// passes through the source [`StrictPathError`].
    #[must_use]
    pub(crate) fn fold_confinement<T>(
        self,
        escape: impl FnOnce() -> T,
        unverifiable: impl FnOnce(StrictPathError) -> T,
    ) -> T {
        match self {
            Self::Absolute | Self::UnsafeComponent | Self::OutsideRoot => {
                escape()
            }
            Self::StrictPath(source) => unverifiable(source),
        }
    }
}

/// Stores a relative path proven safe by lexical checks.
///
/// Does not touch the filesystem and does not resolve symlinks.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RelativePath(PathBuf);

impl RelativePath {
    /// Validates `candidate` by inspecting path components only.
    ///
    /// Accepts paths that are relative, contain only [`Component::Normal`] and
    /// [`Component::CurDir`] components, and contain at least one normal
    /// component.
    ///
    /// # Errors
    ///
    /// - [`PathError::Absolute`] if `candidate` is absolute
    /// - [`PathError::UnsafeComponent`] if `candidate` contains `..`, a
    ///   component that is not a plain name or `.`, or has no plain-name
    ///   component at all
    pub(crate) fn parse(candidate: &Path) -> Result<Self, PathError> {
        Self::try_from(candidate)
    }

    /// Derives `path` relative to `root`, failing if the prefix does not match
    /// or the remainder is lexically unsafe.
    ///
    /// # Errors
    ///
    /// - [`PathError::OutsideRoot`] if `path` is not under `root`
    /// - [`PathError::UnsafeComponent`] if the derived remainder contains an
    ///   unsafe component or has no named component
    pub(crate) fn derive(root: &Path, path: &Path) -> Result<Self, PathError> {
        let relative =
            path.strip_prefix(root).map_err(|_| PathError::OutsideRoot)?;
        Self::parse(relative)
    }

    /// Consumes `self` and returns the relative path.
    #[inline]
    #[must_use]
    pub(crate) fn into_path_buf(self) -> PathBuf {
        self.0
    }
}

impl<'a> TryFrom<&'a Path> for RelativePath {
    type Error = PathError;

    fn try_from(candidate: &'a Path) -> Result<Self, Self::Error> {
        if candidate.is_absolute() {
            return Err(PathError::Absolute);
        }
        let mut has_normal_component = false;
        let is_safe = candidate.components().all(|component| match component {
            Component::Normal(_) => {
                has_normal_component = true;
                true
            }
            Component::CurDir => true,
            _ => false,
        });
        if !is_safe || !has_normal_component {
            return Err(PathError::UnsafeComponent);
        }
        Ok(Self(candidate.to_path_buf()))
    }
}

impl AsRef<Path> for RelativePath {
    #[inline]
    fn as_ref(&self) -> &Path {
        &self.0
    }
}

/// Stores a path proven to resolve inside a root directory.
///
/// The path itself may not exist yet. [`Self::parse`] validates the candidate
/// with `strict-path` and returns `root.join(candidate)`, preserving the root's
/// original spelling.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SafeRelativePath(PathBuf);

impl SafeRelativePath {
    /// Validates `candidate` and returns its plain join with `root`.
    ///
    /// Uses two-phase validation:
    /// - Lexical check: [`RelativePath::parse`] rejects absolute paths,
    ///   parent-directory components, non-normal components other than `.`, and
    ///   paths without a named component
    /// - Filesystem check: uses `strict-path` to validate `root` as a boundary
    ///   and resolve the candidate's existing components before rejecting paths
    ///   outside canonical `root`
    ///
    /// The returned path is not canonicalized. A symlink changed after this
    /// function returns and before later I/O can still race the confinement
    /// check.
    ///
    /// # Errors
    ///
    /// - [`PathError::Absolute`] if `candidate` is absolute
    /// - [`PathError::UnsafeComponent`] if `candidate` contains an unsafe
    ///   component or has no named component
    /// - [`PathError::OutsideRoot`] if `candidate` resolves outside `root`
    /// - [`PathError::StrictPath`] if `strict-path` cannot validate `root` as a
    ///   boundary or resolve the candidate's existing path components
    pub(crate) fn parse(
        root: &Path,
        candidate: &Path,
    ) -> Result<Self, PathError> {
        let safe = RelativePath::parse(candidate)?;
        let boundary: PathBoundary<()> = PathBoundary::try_new(root)?;
        let _ = boundary.strict_join(safe.as_ref()).map_err(
            |error| match error {
                StrictPathError::PathEscapesBoundary {
                    ..
                } => PathError::OutsideRoot,
                other => PathError::StrictPath(other),
            },
        )?;
        Ok(Self(root.join(safe.as_ref())))
    }

    /// Consumes `self` and returns the confined path.
    #[inline]
    #[must_use]
    pub(crate) fn into_path_buf(self) -> PathBuf {
        self.into()
    }
}

impl From<SafeRelativePath> for PathBuf {
    #[inline]
    fn from(path: SafeRelativePath) -> Self {
        path.0
    }
}

impl AsRef<Path> for SafeRelativePath {
    #[inline]
    fn as_ref(&self) -> &Path {
        &self.0
    }
}

/// A borrowed reference to a directory path.
///
/// Wraps `Path` to distinguish directory paths from file paths at the type
/// level. The containing folder of `notes/todo.md` is `notes`; the containing
/// folder of `root.md` is `""`.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub(crate) struct FolderRef<'a>(&'a Path);

impl<'a> FolderRef<'a> {
    /// Extracts the containing folder from `path`.
    ///
    /// Returns the root `""` for top-level paths.
    #[inline]
    #[must_use]
    pub(crate) fn of(path: &'a Path) -> Self {
        Self(path.parent().unwrap_or_else(|| Path::new("")))
    }

    /// Returns the inner path.
    #[inline]
    #[must_use]
    pub(crate) fn as_path(self) -> &'a Path {
        self.0
    }

    /// Tree distance: hops up to the nearest shared ancestor plus down to the
    /// other folder. The same folder has distance `0`.
    pub(crate) fn distance_to(self, other: FolderRef<'_>) -> usize {
        let mut a_iter = self.0.components();
        let mut b_iter = other.0.components();
        let mut shared: usize = 0;
        let mut a_count: usize = 0;
        let mut b_count: usize = 0;
        let mut matching = true;
        loop {
            match (a_iter.next(), b_iter.next()) {
                (Some(ac), Some(bc)) => {
                    a_count = a_count.saturating_add(1);
                    b_count = b_count.saturating_add(1);
                    if matching && ac == bc {
                        shared = shared.saturating_add(1);
                    } else {
                        matching = false;
                    }
                }

                (Some(_), None) => {
                    a_count = a_count
                        .saturating_add(1)
                        .saturating_add(a_iter.count());
                    break;
                }
                (None, Some(_)) => {
                    b_count = b_count
                        .saturating_add(1)
                        .saturating_add(b_iter.count());
                    break;
                }
                (None, None) => break,
            }
        }
        a_count
            .saturating_sub(shared)
            .saturating_add(b_count.saturating_sub(shared))
    }
}

impl<'a> From<FolderRef<'a>> for &'a Path {
    #[inline]
    fn from(folder: FolderRef<'a>) -> Self {
        folder.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    mod relative_path {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn derive_accepts_a_root_child() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let path = temp.path().join("a.md");

            let relative =
                RelativePath::derive(temp.path(), &path).expect("derive path");

            assert_eq!(relative.as_ref(), Path::new("a.md"));
        }

        #[test]
        fn parse_accepts_a_plain_relative_path() {
            let parsed = RelativePath::parse(Path::new("notes/daily.md"))
                .expect("plain relative path is safe");

            assert_eq!(parsed.as_ref(), Path::new("notes/daily.md"));
        }

        #[test]
        fn parse_rejects_an_absolute_path() {
            let error = RelativePath::parse(Path::new("/etc/passwd"))
                .expect_err("absolute path is rejected");

            assert!(matches!(error, PathError::Absolute));
        }

        #[test]
        fn parse_rejects_a_parent_dir_component() {
            let error = RelativePath::parse(Path::new("../escape.md"))
                .expect_err("`..` is rejected");

            assert!(matches!(error, PathError::UnsafeComponent));
        }

        #[test]
        fn parse_rejects_an_empty_path() {
            let error = RelativePath::parse(Path::new(""))
                .expect_err("empty path has no Normal component");

            assert!(matches!(error, PathError::UnsafeComponent));
        }

        #[test]
        fn parse_rejects_a_bare_current_dir() {
            let error = RelativePath::parse(Path::new("."))
                .expect_err("bare `.` has no Normal component");

            assert!(matches!(error, PathError::UnsafeComponent));
        }

        #[test]
        fn parse_accepts_a_leading_current_dir() {
            let parsed = RelativePath::parse(Path::new("./daily.md"))
                .expect("leading `.` alongside a Normal component is safe");

            assert_eq!(parsed.as_ref(), Path::new("./daily.md"));
        }

        #[test]
        fn derive_rejects_an_out_of_root_path() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let root_a = temp.path().join("a");
            let root_b = temp.path().join("b");
            std::fs::create_dir(&root_a).expect("create root a");
            std::fs::create_dir(&root_b).expect("create root b");

            let error = RelativePath::derive(&root_a, &root_b.join("x.md"))
                .expect_err("sibling path is outside root");

            assert!(matches!(error, PathError::OutsideRoot));
        }

        #[test]
        fn derive_rejects_parent_components_in_the_remainder() {
            let temp = tempfile::tempdir().expect("create temp dir");

            let error = RelativePath::derive(
                temp.path(),
                &temp.path().join("a/../b.md"),
            )
            .expect_err("parent component is rejected");

            assert!(matches!(error, PathError::UnsafeComponent));
        }
    }

    mod safe_relative_path {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn parse_confines_an_existing_candidate_inside_root() {
            let temp = tempfile::tempdir().expect("create temp dir");
            std::fs::write(temp.path().join("daily.md"), "content")
                .expect("seed file");

            let confined =
                SafeRelativePath::parse(temp.path(), Path::new("daily.md"))
                    .expect("candidate resolves inside root");

            assert_eq!(confined.as_ref(), temp.path().join("daily.md"));
        }

        #[test]
        fn parse_confines_a_candidate_that_does_not_exist_yet() {
            let temp = tempfile::tempdir().expect("create temp dir");

            let confined = SafeRelativePath::parse(
                temp.path(),
                Path::new("notes/2026/daily.md"),
            )
            .expect("not-yet-existing candidate still resolves inside root");

            assert_eq!(
                confined.as_ref(),
                temp.path().join("notes/2026/daily.md")
            );
        }

        #[test]
        fn parse_rejects_an_unsafe_candidate_before_touching_disk() {
            let temp = tempfile::tempdir().expect("create temp dir");

            let error =
                SafeRelativePath::parse(temp.path(), Path::new("../escape.md"))
                    .expect_err("`..` is rejected");

            assert!(matches!(error, PathError::UnsafeComponent));
        }

        #[cfg(unix)]
        #[test]
        fn parse_rejects_a_candidate_escaping_through_an_existing_symlink() {
            use std::os::unix::fs::symlink;

            let temp = tempfile::tempdir().expect("create temp dir");
            let root = temp.path().join("root");
            let outside = temp.path().join("outside");
            std::fs::create_dir(&root).expect("create root");
            std::fs::create_dir(&outside).expect("create outside dir");
            symlink(&outside, root.join("link")).expect("create symlink");

            let error =
                SafeRelativePath::parse(&root, Path::new("link/secret.md"))
                    .expect_err("symlink escaping root is rejected");

            assert!(matches!(error, PathError::OutsideRoot));
        }

        #[cfg(unix)]
        #[test]
        fn parse_rejects_a_not_yet_existing_candidate_through_an_existing_escaping_symlink()
         {
            use std::os::unix::fs::symlink;

            let temp = tempfile::tempdir().expect("create temp dir");
            let root = temp.path().join("root");
            let outside = temp.path().join("outside");
            std::fs::create_dir(&root).expect("create root");
            std::fs::create_dir(&outside).expect("create outside dir");
            symlink(&outside, root.join("link")).expect("create symlink");

            // `link/new/note.md` doesn't exist yet, but its existing symlink
            // component escapes `root`, which is the write path this check
            // guards against.
            let error =
                SafeRelativePath::parse(&root, Path::new("link/new/note.md"))
                    .expect_err("escaping symlink ancestor is rejected");

            assert!(matches!(error, PathError::OutsideRoot));
        }

        #[cfg(windows)]
        #[test]
        fn parse_rejects_a_candidate_escaping_through_an_existing_junction() {
            use std::process::Command;

            let temp = tempfile::tempdir().expect("create temp dir");
            let root = temp.path().join("root");
            let outside = temp.path().join("outside");
            std::fs::create_dir(&root).expect("create root");
            std::fs::create_dir(&outside).expect("create outside dir");
            let junction = root.join("link");
            let output = Command::new("cmd")
                .args(["/C", "mklink", "/J"])
                .arg(&junction)
                .arg(&outside)
                .output()
                .expect("run mklink");
            assert!(output.status.success(), "mklink /J failed: {output:?}");

            let error =
                SafeRelativePath::parse(&root, Path::new("link/secret.md"))
                    .expect_err("junction escaping root is rejected");

            std::fs::remove_dir(junction).expect("remove junction");
            assert!(matches!(error, PathError::OutsideRoot));
        }
    }

    mod folder_ref {
        use super::*;

        #[test]
        fn same_folder_has_zero_distance() {
            let a = FolderRef::of(Path::new("folder/a.md"));
            let b = FolderRef::of(Path::new("folder/b.md"));
            assert_eq!(a.distance_to(b), 0);
        }

        #[test]
        fn parent_and_child_have_distance_one() {
            let a = FolderRef::of(Path::new("folder/sub/a.md"));
            let b = FolderRef::of(Path::new("folder/b.md"));
            assert_eq!(a.distance_to(b), 1);
            assert_eq!(b.distance_to(a), 1);
        }

        #[test]
        fn siblings_have_distance_two() {
            let a = FolderRef::of(Path::new("folder/sub1/a.md"));
            let b = FolderRef::of(Path::new("folder/sub2/b.md"));
            assert_eq!(a.distance_to(b), 2);
        }

        #[test]
        fn different_subtrees_step_through_common_ancestor() {
            let a = FolderRef::of(Path::new("a/b/c/file.md"));
            let b = FolderRef::of(Path::new("a/d/e/file.md"));
            assert_eq!(a.distance_to(b), 4);
        }

        #[test]
        fn root_and_nested_folder_compute_correct_distance() {
            let a = FolderRef::of(Path::new("root.md"));
            let b = FolderRef::of(Path::new("a/b/c/nested.md"));
            assert_eq!(a.distance_to(b), 3);
            assert_eq!(b.distance_to(a), 3);
        }

        #[test]
        fn of_extracts_parent_directory() {
            assert_eq!(
                FolderRef::of(Path::new("a/b/file.md")).as_path(),
                Path::new("a/b")
            );
            assert_eq!(
                FolderRef::of(Path::new("root.md")).as_path(),
                Path::new("")
            );
        }
    }
}
