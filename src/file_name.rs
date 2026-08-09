//! Define file-name and base-name newtypes.
//!
//! Main types:
//! - [`FileName`] - Final path component including any extension
//! - [`BaseName`] - Owned file stem with any extension stripped
//! - [`BaseNameRef`] - Borrowed file stem
//! - [`MissingFileName`] - Error for paths without a final component
//!
//! Dotfiles follow [`Path::file_stem`]: `.gitignore` has no extension and keeps
//! `.gitignore` as its base name.

use std::path::Path;

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Stores a file's final path component.
///
/// Keeps the name exactly as returned by [`Path::file_name`], including any
/// extension. For `todo.md`, stores `todo.md`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct FileName(String);

impl FileName {
    /// Returns this name's extension.
    ///
    /// Dotfiles without another extension return [`None`]. For example,
    /// `.gitignore` has no extension, while `.env.local` returns `local`.
    #[must_use]
    pub(crate) fn extension(&self) -> Option<&str> {
        Path::new(&self.0).extension().and_then(|ext| ext.to_str())
    }
}

/// Reports that a path has no final component.
#[derive(Debug, Error)]
#[error("path has no file name")]
pub(crate) struct MissingFileName;

impl TryFrom<&Path> for FileName {
    type Error = MissingFileName;

    /// Builds a [`FileName`] from `path`'s final component.
    ///
    /// # Errors
    ///
    /// - [`MissingFileName`] if `path` has no final component, such as `/`,
    ///   `..`, or an empty path
    fn try_from(path: &Path) -> Result<Self, Self::Error> {
        path.file_name()
            .map(|name| Self(name.to_string_lossy().into_owned()))
            .ok_or(MissingFileName)
    }
}

/// Stores a file name with any extension stripped.
///
/// Uses [`Path::file_stem`] on [`FileName`]'s stored text. For `todo.md`,
/// stores `todo`. Dotfiles such as `.gitignore` keep their full text as the
/// stem.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct BaseName(String);

impl BaseName {
    /// Returns this name as a string slice.
    #[inline]
    #[must_use]
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&FileName> for BaseName {
    fn from(name: &FileName) -> Self {
        Self(
            Path::new(&name.0)
                .file_stem()
                .map(|stem| stem.to_string_lossy().into_owned())
                .unwrap_or_default(),
        )
    }
}

/// Borrows a file name with any extension stripped.
///
/// Use this instead of [`BaseName`] when one comparison or hash lookup can
/// borrow directly from a [`Path`]. Dotfile behavior matches
/// [`Path::file_stem`].
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub(crate) struct BaseNameRef<'a>(&'a str);

impl<'a> BaseNameRef<'a> {
    /// Borrows `path`'s file stem.
    ///
    /// Returns [`None`] when `path` has no final component or the stem is not
    /// valid UTF-8.
    #[must_use]
    pub(crate) fn from_path(path: &'a Path) -> Option<Self> {
        path.file_stem().and_then(|stem| stem.to_str()).map(Self)
    }

    /// Returns this stem as a string slice.
    #[inline]
    #[must_use]
    pub(crate) fn as_str(&self) -> &str {
        self.0
    }
}

impl std::borrow::Borrow<str> for BaseNameRef<'_> {
    fn borrow(&self) -> &str {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    mod file_name {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn keeps_the_extension() {
            let name = FileName::try_from(Path::new("todo.md"))
                .expect("valid file name");

            assert_eq!(name.extension(), Some("md"));
        }

        #[test]
        fn fails_when_the_path_has_no_final_component() {
            let error = FileName::try_from(Path::new(".."))
                .expect_err("path with no file name is rejected");

            assert!(matches!(error, MissingFileName));
        }
    }

    mod base_name {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn strips_the_extension() {
            let name = FileName::try_from(Path::new("todo.md"))
                .expect("valid file name");

            assert_eq!(BaseName::from(&name).as_str(), "todo");
        }

        #[test]
        fn keeps_the_whole_name_when_there_is_no_extension() {
            let name = FileName::try_from(Path::new("LICENSE"))
                .expect("valid file name");

            assert_eq!(BaseName::from(&name).as_str(), "LICENSE");
        }

        #[test]
        fn treats_a_leading_dot_as_part_of_the_stem() {
            let name = FileName::try_from(Path::new(".gitignore"))
                .expect("valid file name");

            assert_eq!(BaseName::from(&name).as_str(), ".gitignore");
        }
    }

    mod base_name_ref {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn borrows_the_stem_without_the_extension() {
            let stem = BaseNameRef::from_path(Path::new("todo.md"))
                .expect("valid path");

            assert_eq!(stem.as_str(), "todo");
        }

        #[test]
        fn returns_none_when_the_path_has_no_final_component() {
            assert_eq!(BaseNameRef::from_path(Path::new("..")), None);
        }

        #[test]
        fn compares_equal_for_the_same_stem_across_different_paths() {
            let a = BaseNameRef::from_path(Path::new("a/todo.md"))
                .expect("valid path");
            let b = BaseNameRef::from_path(Path::new("b/todo.markdown"))
                .expect("valid path");

            assert_eq!(a, b);
        }

        #[test]
        #[cfg(unix)]
        fn returns_none_when_the_stem_is_not_valid_utf8() {
            use std::{ffi::OsStr, os::unix::ffi::OsStrExt};

            let invalid = OsStr::from_bytes(&[0x66, 0x6f, 0x80, 0x6f]); // "fo\x80o"
            let path = Path::new(invalid);

            assert_eq!(BaseNameRef::from_path(path), None);
        }
    }
}
