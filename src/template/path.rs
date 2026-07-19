//! Newtypes for template identifiers: [`TemplatePath`] and [`TemplateName`].
//!
//! Resolution (`super::resolve`) and the include loader (`super::engine`)
//! both need to join a template directory with a user- or
//! filesystem-supplied relative path without ever escaping that
//! directory. Before these types existed, that safety was a runtime bool
//! check (`is_safe_template_relative_path`) callers had to remember to
//! call; [`TemplatePath`]'s `TryFrom` impls make the unsafe state
//! unconstructible instead — every function that takes a `&TemplatePath`
//! gets the guarantee for free.

use std::path::{Component, Path, PathBuf};

use thiserror::Error;

/// Errors constructing a [`TemplatePath`].
///
/// `thiserror`-only, no `miette::Diagnostic` — matches this module's
/// convention (see `crate::config::mod`'s docs for why).
#[derive(Debug, Error)]
pub(super) enum TemplatePathError {
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
    /// The path is safe but names no file in the given directory.
    #[error("{0} does not exist in {1}")]
    NotFound(PathBuf, PathBuf),
}

/// A template identifier that is safe to join onto any template
/// directory: relative, and free of `..`/root/prefix components.
///
/// May still include a file extension and nested directory segments
/// (`"folder/daily.md"` is a valid `TemplatePath`) — see [`TemplateName`]
/// for the stricter "bare name" case. Implements [`AsRef<Path>`] so it
/// can be passed anywhere a path is expected (`Path::join`,
/// `fs::read_to_string`, …) without an extra accessor call.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct TemplatePath(PathBuf);

impl TryFrom<&Path> for TemplatePath {
    type Error = TemplatePathError;

    /// Validates `path` as a safe, directory-relative template path.
    ///
    /// # Errors
    ///
    /// Returns [`TemplatePathError::Absolute`] when `path` is absolute.
    /// Returns [`TemplatePathError::UnsafeComponent`] when `path` contains
    /// a `..` or other component that isn't a plain name or `.`.
    fn try_from(path: &Path) -> Result<Self, Self::Error> {
        if path.is_absolute() {
            return Err(TemplatePathError::Absolute(path.to_path_buf()));
        }
        let is_safe = path.components().all(|component| {
            matches!(component, Component::Normal(_) | Component::CurDir)
        });
        if !is_safe {
            return Err(TemplatePathError::UnsafeComponent(path.to_path_buf()));
        }
        Ok(Self(path.to_path_buf()))
    }
}

impl TryFrom<(&Path, &Path)> for TemplatePath {
    type Error = TemplatePathError;

    /// Validates `name` (the second element) exactly as
    /// [`TryFrom<&Path>`](TemplatePath), then additionally checks it
    /// names an existing file within `dir` (the first element).
    ///
    /// # Errors
    ///
    /// Returns the same errors as [`TryFrom<&Path>`](TemplatePath) when
    /// `name` itself is unsafe. Returns
    /// [`TemplatePathError::NotFound`] when `name` is safe but
    /// `dir.join(name)` isn't an existing file.
    fn try_from((dir, name): (&Path, &Path)) -> Result<Self, Self::Error> {
        let template_path = Self::try_from(name)?;
        if !dir.join(&template_path.0).is_file() {
            return Err(TemplatePathError::NotFound(
                template_path.0,
                dir.to_path_buf(),
            ));
        }
        Ok(template_path)
    }
}

impl AsRef<Path> for TemplatePath {
    fn as_ref(&self) -> &Path {
        &self.0
    }
}

/// A template's bare name: a single path segment with no file
/// extension, e.g. `"daily"` from a resolved `"daily.md"`.
///
/// Used for stem-matching within a single directory and for deriving the
/// default output filename — never carries directory segments, since
/// both of those are single-directory, leaf-name concepts. Implements
/// [`AsRef<Path>`] for the same reason as [`TemplatePath`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct TemplateName(PathBuf);

impl From<&Path> for TemplateName {
    /// Derives a name from any path's final component, dropping
    /// directory segments and the extension.
    ///
    /// Infallible: [`Path::file_stem`] returns `None` only for paths with
    /// no final component (`.`, `..`, root, or empty); template paths
    /// this is called on always come from a resolved file, which always
    /// has one.
    fn from(path: &Path) -> Self {
        Self(path.file_stem().map_or_else(PathBuf::new, PathBuf::from))
    }
}

impl From<&TemplatePath> for TemplateName {
    /// Drops `template_path`'s directory segments and extension, e.g.
    /// `"folder/daily.md"` -> `"daily"`.
    fn from(template_path: &TemplatePath) -> Self {
        Self::from(template_path.0.as_path())
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
        let path = TemplatePath::try_from(Path::new("daily.md"))
            .expect("valid template path");

        assert_eq!(path.as_ref(), Path::new("daily.md"));
    }

    #[test]
    fn try_from_accepts_a_nested_relative_path() {
        let path = TemplatePath::try_from(Path::new("folder/daily.md"))
            .expect("valid template path");

        assert_eq!(path.as_ref(), Path::new("folder/daily.md"));
    }

    #[test]
    fn try_from_rejects_an_absolute_path() {
        let error = TemplatePath::try_from(Path::new("/etc/passwd"))
            .expect_err("absolute path is rejected");

        assert!(matches!(error, TemplatePathError::Absolute(_)));
    }

    #[test]
    fn try_from_rejects_parent_traversal() {
        let error = TemplatePath::try_from(Path::new("../outside.md"))
            .expect_err("parent traversal is rejected");

        assert!(matches!(error, TemplatePathError::UnsafeComponent(_)));
    }

    #[test]
    fn try_from_rejects_nested_parent_traversal() {
        let error =
            TemplatePath::try_from(Path::new("folder/../../outside.md"))
                .expect_err("nested parent traversal is rejected");

        assert!(matches!(error, TemplatePathError::UnsafeComponent(_)));
    }

    #[test]
    fn dir_and_name_try_from_succeeds_for_an_existing_file() {
        let temp = tempfile::tempdir().expect("create temp dir");
        std::fs::write(temp.path().join("daily.md"), "content")
            .expect("write template");

        let path = TemplatePath::try_from((temp.path(), Path::new("daily.md")))
            .expect("existing file resolves");

        assert_eq!(path.as_ref(), Path::new("daily.md"));
    }

    #[test]
    fn dir_and_name_try_from_fails_for_a_missing_file() {
        let temp = tempfile::tempdir().expect("create temp dir");

        let error =
            TemplatePath::try_from((temp.path(), Path::new("missing.md")))
                .expect_err("missing file is rejected");

        assert!(matches!(error, TemplatePathError::NotFound(..)));
    }

    #[test]
    fn dir_and_name_try_from_still_rejects_unsafe_names() {
        let temp = tempfile::tempdir().expect("create temp dir");

        let error =
            TemplatePath::try_from((temp.path(), Path::new("../outside.md")))
                .expect_err("unsafe name is rejected before the fs check");

        assert!(matches!(error, TemplatePathError::UnsafeComponent(_)));
    }

    #[test]
    fn template_name_from_drops_directory_segments_and_extension() {
        let name = TemplateName::from(Path::new(
            "/abs/local-templates/folder/report.md",
        ));

        assert_eq!(name.as_ref(), Path::new("report"));
    }

    #[test]
    fn template_name_from_template_path_matches_from_path() {
        let template_path =
            TemplatePath::try_from(Path::new("folder/daily.md"))
                .expect("valid template path");

        assert_eq!(
            TemplateName::from(&template_path).as_ref(),
            Path::new("daily")
        );
    }
}
