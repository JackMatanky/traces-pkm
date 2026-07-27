//! Root-relative path validation and confinement, shared by every
//! subsystem that resolves a caller-supplied candidate path against a
//! trusted root directory.
//!
//! [`SafeRelativePath`] is the lexical stage: no `..`, no absolute path,
//! at least one real ([`Component::Normal`]) segment. No I/O — it never
//! implies the path, or any root, exists.
//!
//! [`RootConfinedPath`] builds on it with an I/O stage:
//! [`RootConfinedPath::parse`] canonicalizes the longest ancestor of
//! `root.join(candidate)` that already exists and verifies it is still
//! inside `root`'s own canonicalization — catching a symlink planted
//! inside `root` that resolves outside it, which the lexical stage alone
//! cannot see (`root.join(candidate)` can pass [`SafeRelativePath::parse`]
//! yet still land outside `root` once symlinks resolve). Canonicalization
//! is used only to verify: the returned path is the plain
//! `root.join(candidate)`, in `root`'s own textual form, not the
//! canonicalized one — the kernel resolves the same symlinks at actual
//! I/O time as this check already did, so the plain join is equally safe
//! to use. A candidate that doesn't exist yet — the common case for a
//! write target — is handled the same way: only the ancestor that already
//! exists needs checking, since a path component that doesn't exist on
//! disk can't be a symlink. `root` itself is always a valid fallback
//! ancestor, since every caller here already requires it to exist.
//!
//! Neither type performs any filesystem mutation — no directory is
//! created, no file is written. A caller that goes on to create what a
//! [`RootConfinedPath`] names should not need to re-verify confinement
//! afterward; a symlink planted between this check and that write is a
//! race inherent to any confinement check, not one particular to this
//! module.

use std::{
    io,
    path::{Component, Path, PathBuf},
};

use thiserror::Error;

/// Every way [`SafeRelativePath::parse`]/[`RootConfinedPath::parse`] can
/// fail. Deliberately generic over every caller's own root/candidate
/// vocabulary — each caller converts this into its own error type with
/// its own message (see `crate::template::writer`,
/// `crate::template::engine::file_ops`, `crate::template::path`).
#[derive(Debug, Error)]
pub(crate) enum PathError {
    /// The candidate is absolute, contains `..`, contains any component
    /// other than a plain name or `.`, or has no plain-name component at
    /// all (an empty path, or a bare `.`).
    #[error("path is absolute or contains an unsafe component")]
    NotRelative,
    /// The candidate's existing ancestor canonicalizes to somewhere
    /// outside `root` — a symlink escape.
    #[error("path escapes the root directory")]
    EscapesRoot,
    /// `root`, or the candidate's existing ancestor, could not be
    /// canonicalized for a reason other than not existing — permission
    /// denied, a broken symlink loop, etc. Fails closed: treated as
    /// unsafe rather than silently assumed safe, mirroring how
    /// `crate::template::engine::path_ops` distinguishes "doesn't exist"
    /// from a genuine I/O failure instead of folding both into `false`.
    #[error("failed to verify path is inside the root directory")]
    Verify(#[source] io::Error),
}

/// A relative path proven safe by its components alone. No I/O — never
/// touches disk, never implies the path (or any root) exists.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SafeRelativePath(PathBuf);

impl SafeRelativePath {
    /// Validates `candidate`'s components: not absolute, no component
    /// other than [`Component::Normal`]/[`Component::CurDir`], and at
    /// least one [`Component::Normal`] component.
    ///
    /// # Errors
    ///
    /// Returns [`PathError::NotRelative`] when `candidate` fails any of
    /// the checks above.
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

/// A path proven, by filesystem inspection, to resolve inside `root` —
/// accounting for a symlink among its already-existing ancestors. May
/// itself not exist yet: only the existing prefix of `root.join(candidate)`
/// is canonicalized and checked; see the module docs.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RootConfinedPath(PathBuf);

impl RootConfinedPath {
    /// Validates `candidate` via [`SafeRelativePath::parse`], joins it
    /// onto `root`, then verifies the join stays inside `root` even
    /// after resolving symlinks: canonicalizes the longest ancestor of
    /// `root.join(candidate)` that already exists, and checks the
    /// canonical ancestor is still inside `root.canonicalize()`.
    /// Canonicalization is used only to verify — the returned path is
    /// the plain `root.join(candidate)`, not the canonicalized form:
    /// the kernel resolves the same symlinks at actual I/O time as this
    /// check already did, so the plain join is equally safe to use, and
    /// keeps the output in the same textual form as `root` was given in
    /// (avoiding e.g. a `/tmp` root silently becoming `/private/tmp` in
    /// a path shown to the user).
    ///
    /// # Errors
    ///
    /// Returns [`PathError::NotRelative`] when `candidate` fails
    /// [`SafeRelativePath::parse`]. Returns [`PathError::EscapesRoot`]
    /// when the canonicalized ancestor lands outside `root`. Returns
    /// [`PathError::Verify`] when canonicalizing `root` or the
    /// candidate's existing ancestor fails for a reason other than not
    /// existing.
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

    /// Consumes `self`, returning the confined path.
    #[inline]
    #[must_use]
    pub(crate) fn into_path_buf(self) -> PathBuf {
        self.0
    }

    /// The longest ancestor of `path` that already exists on disk —
    /// `path` itself if it exists, all the way up to `path`'s
    /// filesystem root otherwise (which always exists, so this always
    /// finds some ancestor).
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
