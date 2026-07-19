//! Newtypes for template identifiers: [`TemplatePath`] and [`TemplateName`].
//!
//! Resolution (`super::resolve`) and the include loader (`super::loader`)
//! both need to join a template directory with a user- or
//! filesystem-supplied relative path without ever escaping that
//! directory. Before these types existed, that safety was a runtime bool
//! check (`is_safe_template_relative_path`) callers had to remember to
//! call; [`TemplatePath::new`] makes the unsafe state unconstructible
//! instead — every function that takes a `&TemplatePath` gets the
//! guarantee for free.

use std::path::{Component, Path, PathBuf};

use thiserror::Error;

/// Errors constructing a [`TemplatePath`].
///
/// `thiserror`-only, no `miette::Diagnostic` — matches this module's
/// convention (see `crate::config::mod`'s docs for why).
#[derive(Debug, Error)]
pub(crate) enum TemplatePathError {
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

/// A template identifier that is safe to join onto any template
/// directory: relative, and free of `..`/root/prefix components.
///
/// May still include a file extension and nested directory segments
/// (`"folder/daily.md"` is a valid `TemplatePath`) — see [`TemplateName`]
/// for the stricter "bare name" case.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct TemplatePath(PathBuf);

impl TemplatePath {
    /// Validates `path` as a safe, directory-relative template path.
    ///
    /// # Errors
    ///
    /// Returns [`TemplatePathError::Absolute`] when `path` is absolute.
    /// Returns [`TemplatePathError::UnsafeComponent`] when `path` contains
    /// a `..` or other component that isn't a plain name or `.`.
    pub(super) fn new(
        path: impl AsRef<Path>,
    ) -> Result<Self, TemplatePathError> {
        let path = path.as_ref();
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

    /// The wrapped relative path.
    #[must_use]
    pub(super) fn as_path(&self) -> &Path {
        &self.0
    }

    /// The final component's stem as a [`TemplateName`] — directory
    /// segments and the extension are both dropped, e.g.
    /// `"folder/daily.md"` -> `"daily"`.
    #[must_use]
    pub(super) fn name(&self) -> TemplateName {
        TemplateName::from_stem(&self.0)
    }
}

/// A template's bare name: a single path segment with no file
/// extension, e.g. `"daily"` from a resolved `"daily.md"`.
///
/// Used for stem-matching within a single directory and for deriving the
/// default output filename — never carries directory segments, since
/// both of those are single-directory, leaf-name concepts.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct TemplateName(PathBuf);

impl TemplateName {
    /// Derives a name from any path's final component, dropping
    /// directory segments and the extension.
    ///
    /// Infallible: [`Path::file_stem`] returns `None` only for paths with
    /// no final component (`.`, `..`, root, or empty); template paths
    /// this is called on always come from a resolved file, which always
    /// has one.
    #[must_use]
    pub(super) fn from_stem(path: &Path) -> Self {
        Self(path.file_stem().map_or_else(PathBuf::new, PathBuf::from))
    }

    /// The wrapped bare name.
    #[must_use]
    pub(super) fn as_path(&self) -> &Path {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn new_accepts_a_plain_relative_name() {
        let path = TemplatePath::new("daily.md").expect("valid template path");

        assert_eq!(path.as_path(), Path::new("daily.md"));
    }

    #[test]
    fn new_accepts_a_nested_relative_path() {
        let path =
            TemplatePath::new("folder/daily.md").expect("valid template path");

        assert_eq!(path.as_path(), Path::new("folder/daily.md"));
    }

    #[test]
    fn new_rejects_an_absolute_path() {
        let error = TemplatePath::new("/etc/passwd")
            .expect_err("absolute path is rejected");

        assert!(matches!(error, TemplatePathError::Absolute(_)));
    }

    #[test]
    fn new_rejects_parent_traversal() {
        let error = TemplatePath::new("../outside.md")
            .expect_err("parent traversal is rejected");

        assert!(matches!(error, TemplatePathError::UnsafeComponent(_)));
    }

    #[test]
    fn new_rejects_nested_parent_traversal() {
        let error = TemplatePath::new("folder/../../outside.md")
            .expect_err("nested parent traversal is rejected");

        assert!(matches!(error, TemplatePathError::UnsafeComponent(_)));
    }

    #[test]
    fn name_drops_directory_segments_and_extension() {
        let path =
            TemplatePath::new("folder/daily.md").expect("valid template path");

        assert_eq!(path.name().as_path(), Path::new("daily"));
    }

    #[test]
    fn name_of_an_extensionless_path_is_unchanged() {
        let path = TemplatePath::new("daily").expect("valid template path");

        assert_eq!(path.name().as_path(), Path::new("daily"));
    }

    #[test]
    fn from_stem_derives_the_bare_name_from_any_path() {
        let name = TemplateName::from_stem(Path::new(
            "/abs/local-templates/folder/report.md",
        ));

        assert_eq!(name.as_path(), Path::new("report"));
    }
}
