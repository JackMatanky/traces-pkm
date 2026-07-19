//! [`TemplateInputPath`]: a template identifier's shape, validated
//! *before* it's tied to a specific directory.
//!
//! [`super::loader`] needs to join a template directory with a user- or
//! filesystem-supplied relative path without ever escaping that
//! directory. Before [`TemplateInputPath`] existed, that safety was a
//! runtime bool check callers had to remember to invoke;
//! [`TemplateInputPath::try_from`] makes the unsafe state unconstructible
//! instead — every function that takes a `&TemplateInputPath` gets the
//! guarantee for free.
//!
//! Deliberately not tied to a template directory — it validates the
//! shape of a candidate identifier (the raw `-i <name>` argument, or a
//! filename found while scanning a directory), nothing more. Once
//! [`super::loader::TemplateLoader`] actually finds a match in a
//! directory, [`super::loader::TemplatePath`] is the distinct,
//! later-stage type that names the file's real, absolute location —
//! "safe relative shape" and "found on disk under a configured
//! directory" are different facts with different lifecycles, established
//! by different code (pure validation here vs. filesystem search there).

use std::{
    ffi::OsStr,
    path::{Component, Path, PathBuf},
};

use thiserror::Error;

/// Errors constructing a [`TemplateInputPath`].
///
/// `thiserror`-only, no `miette::Diagnostic` — matches this module's
/// convention (see `crate::config::mod`'s docs for why).
#[derive(Debug, Error)]
pub(super) enum TemplateInputPathError {
    /// The path is absolute; template paths must be relative to a
    /// template directory.
    #[error("template path {0} must be relative, not absolute")]
    Absolute(PathBuf),
    /// The path contains a component other than a plain name or `.`
    /// (most notably `..`), which could escape the template directory
    /// it's joined onto.
    #[error(
        "template path {0} must not contain '..' or other unsafe components"
    )]
    UnsafeComponent(PathBuf),
}

/// A candidate template identifier, validated safe to join onto any
/// template directory: relative, and free of `..`/root/prefix
/// components. Not yet tied to any specific directory — see this
/// module's docs for why that's a distinct, later concern.
///
/// May still include a file extension and nested directory segments
/// (`"folder/daily.md"` is a valid `TemplateInputPath`) — [`Self::stem`]
/// strips both for the "bare name" case. Implements [`AsRef<Path>`] so it
/// can be passed anywhere a path is expected (`Path::join`,
/// `fs::read_to_string`, …) without an extra accessor call.
///
/// Deliberately has no directory-aware constructor: whether a
/// `TemplateInputPath` names a real file is a question about a specific
/// directory, answered by [`Self::exists_in`], not baked into
/// construction — validation (pure, no I/O) and existence (I/O,
/// directory-dependent) are different questions with different costs.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct TemplateInputPath(PathBuf);

impl TryFrom<&Path> for TemplateInputPath {
    type Error = TemplateInputPathError;

    /// Validates `path` as a safe, directory-relative template
    /// identifier.
    ///
    /// # Errors
    ///
    /// Returns [`TemplateInputPathError::Absolute`] when `path` is
    /// absolute. Returns [`TemplateInputPathError::UnsafeComponent`] when
    /// `path` contains a `..` or other component that isn't a plain name
    /// or `.`, or when `path` has no `Normal` component at all (e.g. an
    /// empty path, or a bare `.`) — in either case there's no safe file
    /// name to join onto a directory.
    fn try_from(path: &Path) -> Result<Self, Self::Error> {
        if path.is_absolute() {
            return Err(TemplateInputPathError::Absolute(path.to_path_buf()));
        }
        let mut has_normal_component = false;
        let is_safe = path.components().all(|component| match component {
            Component::Normal(_) => {
                has_normal_component = true;
                true
            }
            Component::CurDir => true,
            _ => false,
        });
        if !is_safe || !has_normal_component {
            return Err(TemplateInputPathError::UnsafeComponent(
                path.to_path_buf(),
            ));
        }
        Ok(Self(path.to_path_buf()))
    }
}

impl TemplateInputPath {
    /// Whether this path names an existing file within `dir`.
    #[inline]
    #[must_use]
    pub(super) fn exists_in(&self, dir: &Path) -> bool {
        dir.join(&self.0).is_file()
    }

    /// This path's bare stem: no directory segments, no extension, e.g.
    /// `"folder/daily.md"` -> `"daily"`.
    ///
    /// Used for stem-matching within a single directory
    /// ([`super::loader::TemplateLoader`]) and for deriving the default
    /// output filename ([`super::service::TemplateService`]) — a plain
    /// [`OsStr`] borrow rather than a further newtype, since neither use
    /// needs anything beyond what [`Path::file_stem`] already guarantees.
    ///
    /// [`Path::file_stem`] returns `None` only for a path with no final
    /// `Normal` component; [`Self::try_from`] rejects any
    /// `TemplateInputPath` without one, so this always has a stem in
    /// practice — the fallback below exists only so the type stays
    /// panic-free rather than leaning on that invariant at runtime.
    #[inline]
    #[must_use]
    pub(super) fn stem(&self) -> &OsStr {
        self.0.file_stem().unwrap_or_else(|| self.0.as_os_str())
    }
}

impl AsRef<Path> for TemplateInputPath {
    fn as_ref(&self) -> &Path {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn try_from_accepts_a_plain_relative_name() {
        let path = TemplateInputPath::try_from(Path::new("daily.md"))
            .expect("valid template path");

        assert_eq!(path.as_ref(), Path::new("daily.md"));
    }

    #[test]
    fn try_from_accepts_a_nested_relative_path() {
        let path = TemplateInputPath::try_from(Path::new("folder/daily.md"))
            .expect("valid template path");

        assert_eq!(path.as_ref(), Path::new("folder/daily.md"));
    }

    #[test]
    fn try_from_rejects_an_absolute_path() {
        let error = TemplateInputPath::try_from(Path::new("/etc/passwd"))
            .expect_err("absolute path is rejected");

        assert!(matches!(error, TemplateInputPathError::Absolute(_)));
    }

    #[test]
    fn try_from_rejects_parent_traversal() {
        let error = TemplateInputPath::try_from(Path::new("../outside.md"))
            .expect_err("parent traversal is rejected");

        assert!(matches!(error, TemplateInputPathError::UnsafeComponent(_)));
    }

    #[test]
    fn try_from_rejects_nested_parent_traversal() {
        let error =
            TemplateInputPath::try_from(Path::new("folder/../../outside.md"))
                .expect_err("nested parent traversal is rejected");

        assert!(matches!(error, TemplateInputPathError::UnsafeComponent(_)));
    }

    #[test]
    fn try_from_rejects_an_empty_path() {
        let error = TemplateInputPath::try_from(Path::new(""))
            .expect_err("empty path has no safe file name");

        assert!(matches!(error, TemplateInputPathError::UnsafeComponent(_)));
    }

    #[test]
    fn try_from_rejects_a_bare_current_dir() {
        let error = TemplateInputPath::try_from(Path::new("."))
            .expect_err("bare current-dir has no safe file name");

        assert!(matches!(error, TemplateInputPathError::UnsafeComponent(_)));
    }

    #[test]
    fn exists_in_is_true_for_an_existing_file() {
        let temp = tempfile::tempdir().expect("create temp dir");
        std::fs::write(temp.path().join("daily.md"), "content")
            .expect("write template");
        let path = TemplateInputPath::try_from(Path::new("daily.md"))
            .expect("valid template path");

        assert!(path.exists_in(temp.path()));
    }

    #[test]
    fn exists_in_is_false_for_a_missing_file() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let path = TemplateInputPath::try_from(Path::new("missing.md"))
            .expect("valid template path");

        assert!(!path.exists_in(temp.path()));
    }

    #[test]
    fn exists_in_is_false_for_a_directory() {
        let temp = tempfile::tempdir().expect("create temp dir");
        std::fs::create_dir(temp.path().join("daily")).expect("create dir");
        let path = TemplateInputPath::try_from(Path::new("daily"))
            .expect("valid template path");

        assert!(!path.exists_in(temp.path()));
    }

    #[test]
    fn stem_drops_directory_segments_and_extension() {
        let input_path =
            TemplateInputPath::try_from(Path::new("folder/report.md"))
                .expect("valid template path");

        assert_eq!(input_path.stem(), OsStr::new("report"));
    }

    #[test]
    fn stem_of_an_extensionless_path_is_unchanged() {
        let input_path = TemplateInputPath::try_from(Path::new("daily"))
            .expect("valid path");

        assert_eq!(input_path.stem(), OsStr::new("daily"));
    }
}
