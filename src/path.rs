//! Root-relative path validation and confinement.
//!
//! Shared by subsystems that resolve caller-supplied paths against a trusted
//! root directory.
//!
//! - [`SafeRelativePath`] performs a lexical check: no `..`, no absolute path,
//!   and at least one [`Component::Normal`] segment. It does not touch the
//!   filesystem.
//! - [`RootConfinedPath`] adds the filesystem check: canonicalize the longest
//!   existing ancestor of `root.join(candidate)` and confirm it remains inside
//!   `root`.
//!
//! The returned confined path is the plain join, not the canonicalized form.
//! Later I/O resolves the same symlinks, and the plain join preserves `root`'s
//! original spelling, such as `/tmp` instead of `/private/tmp`.
//!
//! A symlink planted between validation and a later write is the inherent race
//! in any path-confinement check.

use std::{
    io,
    path::{Component, Path, PathBuf},
};

use thiserror::Error;

/// Error returned by safe relative path and root confinement parsing.
#[derive(Debug, Error)]
pub(crate) enum PathError {
    /// Absolute, contains `..`, contains a component other than a plain name or
    /// `.`, or has no plain-name component at all.
    #[error("path is absolute or contains an unsafe component")]
    NotRelative,
    /// The candidate's existing ancestor resolves outside `root`.
    #[error("path escapes the root directory")]
    EscapesRoot,
    /// Canonicalizing `root` or the candidate's existing ancestor failed for a
    /// reason other than not existing. Fails closed.
    #[error("failed to verify path is inside the root directory")]
    Verify(#[source] io::Error),
}

/// Relative path proven safe by its components alone.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SafeRelativePath(PathBuf);

impl SafeRelativePath {
    /// Validates `candidate`: not absolute, only
    /// [`Component::Normal`]/[`Component::CurDir`] components, and at least one
    /// `Normal` component.
    ///
    /// # Errors
    ///
    /// Returns [`PathError::NotRelative`] if any check fails.
    pub(crate) fn parse(candidate: &Path) -> Result<Self, PathError> {
        if candidate.is_absolute() {
            return Err(PathError::NotRelative);
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
            return Err(PathError::NotRelative);
        }
        Ok(Self(candidate.to_path_buf()))
    }
}

impl AsRef<Path> for SafeRelativePath {
    #[inline]
    fn as_ref(&self) -> &Path {
        &self.0
    }
}

/// Path proven by filesystem inspection to resolve inside `root`.
///
/// The path itself may not exist yet; [`Self::parse`] validates the longest
/// existing ancestor.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RootConfinedPath(PathBuf);

impl RootConfinedPath {
    /// Validates `candidate` and returns its plain join with `root`.
    ///
    /// First validates lexical safety with [`SafeRelativePath::parse`], then
    /// canonicalizes the longest existing ancestor of the joined path to
    /// confirm it remains inside `root`.
    ///
    /// # Errors
    ///
    /// - [`PathError::NotRelative`] if `candidate` fails
    ///   [`SafeRelativePath::parse`]
    /// - [`PathError::EscapesRoot`] if the ancestor resolves outside `root`
    /// - [`PathError::Verify`] if canonicalizing `root` or the ancestor fails
    pub(crate) fn parse(
        root: &Path,
        candidate: &Path,
    ) -> Result<Self, PathError> {
        let safe = SafeRelativePath::parse(candidate)?;
        let joined = root.join(safe.as_ref());
        let existing_ancestor = Self::longest_existing_ancestor(&joined);
        let canonical_ancestor =
            existing_ancestor.canonicalize().map_err(PathError::Verify)?;
        let canonical_root = root.canonicalize().map_err(PathError::Verify)?;
        if !canonical_ancestor.starts_with(&canonical_root) {
            return Err(PathError::EscapesRoot);
        }
        Ok(Self(joined))
    }

    /// Consumes `self` and returns the confined path.
    #[inline]
    #[must_use]
    pub(crate) fn into_path_buf(self) -> PathBuf {
        self.0
    }

    /// Returns the longest ancestor of `path` that already exists on disk.
    fn longest_existing_ancestor(path: &Path) -> PathBuf {
        path.ancestors()
            .find(|ancestor| ancestor.exists())
            .map_or_else(PathBuf::new, Path::to_path_buf)
    }
}

impl AsRef<Path> for RootConfinedPath {
    #[inline]
    fn as_ref(&self) -> &Path {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    mod safe_relative_path {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn parse_accepts_a_plain_relative_path() {
            let parsed = SafeRelativePath::parse(Path::new("notes/daily.md"))
                .expect("plain relative path is safe");

            assert_eq!(parsed.as_ref(), Path::new("notes/daily.md"));
        }

        #[test]
        fn parse_rejects_an_absolute_path() {
            let error = SafeRelativePath::parse(Path::new("/etc/passwd"))
                .expect_err("absolute path is rejected");

            assert!(matches!(error, PathError::NotRelative));
        }

        #[test]
        fn parse_rejects_a_parent_dir_component() {
            let error = SafeRelativePath::parse(Path::new("../escape.md"))
                .expect_err("`..` is rejected");

            assert!(matches!(error, PathError::NotRelative));
        }

        #[test]
        fn parse_rejects_an_empty_path() {
            let error = SafeRelativePath::parse(Path::new(""))
                .expect_err("empty path has no Normal component");

            assert!(matches!(error, PathError::NotRelative));
        }

        #[test]
        fn parse_rejects_a_bare_current_dir() {
            let error = SafeRelativePath::parse(Path::new("."))
                .expect_err("bare `.` has no Normal component");

            assert!(matches!(error, PathError::NotRelative));
        }

        #[test]
        fn parse_accepts_a_leading_current_dir() {
            let parsed = SafeRelativePath::parse(Path::new("./daily.md"))
                .expect("leading `.` alongside a Normal component is safe");

            assert_eq!(parsed.as_ref(), Path::new("./daily.md"));
        }
    }

    mod root_confined_path {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn parse_confines_an_existing_candidate_inside_root() {
            let temp = tempfile::tempdir().expect("create temp dir");
            std::fs::write(temp.path().join("daily.md"), "content")
                .expect("seed file");

            let confined =
                RootConfinedPath::parse(temp.path(), Path::new("daily.md"))
                    .expect("candidate resolves inside root");

            assert_eq!(confined.as_ref(), temp.path().join("daily.md"));
        }

        #[test]
        fn parse_confines_a_candidate_that_does_not_exist_yet() {
            let temp = tempfile::tempdir().expect("create temp dir");

            let confined = RootConfinedPath::parse(
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
                RootConfinedPath::parse(temp.path(), Path::new("../escape.md"))
                    .expect_err("`..` is rejected");

            assert!(matches!(error, PathError::NotRelative));
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
                RootConfinedPath::parse(&root, Path::new("link/secret.md"))
                    .expect_err("symlink escaping root is rejected");

            assert!(matches!(error, PathError::EscapesRoot));
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

            // `link/new/note.md` doesn't exist yet, but its existing
            // ancestor (`link`) is a symlink escaping `root` — the write
            // path this closes the gap for.
            let error =
                RootConfinedPath::parse(&root, Path::new("link/new/note.md"))
                    .expect_err("escaping symlink ancestor is rejected");

            assert!(matches!(error, PathError::EscapesRoot));
        }
    }
}
