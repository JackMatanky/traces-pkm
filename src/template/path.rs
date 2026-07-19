//! [`TemplateInputPath`] and [`TemplateName`]: newtypes for a template
//! identifier *before* it's tied to a specific directory.
//!
//! [`super::resolve`] needs to join a template directory with a user- or
//! filesystem-supplied relative path without ever escaping that
//! directory. Before [`TemplateInputPath`] existed, that safety was a
//! runtime bool check (`is_safe_template_relative_path`) callers had to
//! remember to call; [`TemplateInputPath::try_from`] makes the unsafe
//! state unconstructible instead — every function that takes a
//! `&TemplateInputPath` gets the guarantee for free.
//!
//! `TemplateInputPath` is deliberately *not* tied to a template
//! directory — it validates the shape of a candidate identifier (the raw
//! `-i <name>` argument, or a filename found while scanning a directory),
//! nothing more. Once resolution actually finds a match in a specific
//! directory, [`super::resolve::TemplatePath`] pairs that same validated
//! identifier with the [`super::resolve::TemplateSource`] it was found
//! in — a different, later-stage type, because "safe relative path" and
//! "path relative to *this* template directory" are different facts with
//! different lifecycles.

use std::path::{Component, Path, PathBuf};

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
/// (`"folder/daily.md"` is a valid `TemplateInputPath`) — see
/// [`TemplateName`] for the stricter "bare name" case. Implements
/// [`AsRef<Path>`] so it can be passed anywhere a path is expected
/// (`Path::join`, `fs::read_to_string`, …) without an extra accessor
/// call.
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
    /// or `.`.
    fn try_from(path: &Path) -> Result<Self, Self::Error> {
        if path.is_absolute() {
            return Err(TemplateInputPathError::Absolute(path.to_path_buf()));
        }
        let is_safe = path.components().all(|component| {
            matches!(component, Component::Normal(_) | Component::CurDir)
        });
        if !is_safe {
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
}

impl AsRef<Path> for TemplateInputPath {
    fn as_ref(&self) -> &Path {
        &self.0
    }
}

/// A template's bare name: a single path segment with no file
/// extension, e.g. `"daily"` from a resolved `"daily.md"`.
///
/// Used for stem-matching within a single directory and for deriving the
/// default output filename — never carries directory segments, since
/// both of those are single-directory, leaf-name concepts. Only
/// constructible from a [`TemplateInputPath`] (never a bare,
/// unvalidated [`Path`]): a template's name is only ever meaningful
/// relative to the template directory it was resolved from. Implements
/// [`AsRef<Path>`] for the same reason as [`TemplateInputPath`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct TemplateName(PathBuf);

impl From<&TemplateInputPath> for TemplateName {
    /// Drops `input_path`'s directory segments and extension, e.g.
    /// `"folder/daily.md"` -> `"daily"`.
    ///
    /// Infallible: [`Path::file_stem`] returns `None` only for paths with
    /// no final component (`.`, `..`, root, or empty); a
    /// [`TemplateInputPath`] is always relative with at least one
    /// `Normal` component, so it always has one.
    fn from(input_path: &TemplateInputPath) -> Self {
        Self(input_path.0.file_stem().map_or_else(PathBuf::new, PathBuf::from))
    }
}

impl AsRef<Path> for TemplateName {
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
    fn template_name_drops_directory_segments_and_extension() {
        let input_path =
            TemplateInputPath::try_from(Path::new("folder/report.md"))
                .expect("valid template path");

        assert_eq!(
            TemplateName::from(&input_path).as_ref(),
            Path::new("report")
        );
    }

    #[test]
    fn template_name_of_an_extensionless_path_is_unchanged() {
        let input_path = TemplateInputPath::try_from(Path::new("daily"))
            .expect("valid path");

        assert_eq!(
            TemplateName::from(&input_path).as_ref(),
            Path::new("daily")
        );
    }
}
