//! File-name newtypes shared by the index and template layers.
//!
//! [`FileName`] keeps the final path component exactly as written, including
//! any extension. [`BaseName`] stores the same name with the extension
//! stripped. Keeping them distinct avoids passing interchangeable strings
//! through code that needs different stem and extension semantics.

use std::path::Path;

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// File's final path component, including any extension.
///
/// For `todo.md`, this stores `todo.md`. Use [`BaseName`] when the extension
/// should be stripped.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct FileName(String);

impl FileName {
    /// Returns this name's extension, if any.
    #[must_use]
    pub(crate) fn extension(&self) -> Option<&str> {
        Path::new(&self.0).extension().and_then(|ext| ext.to_str())
    }
}

/// Error returned when a path has no final component.
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
    ///   `..`, or an empty path.
    fn try_from(path: &Path) -> Result<Self, Self::Error> {
        path.file_name()
            .map(|name| Self(name.to_string_lossy().into_owned()))
            .ok_or(MissingFileName)
    }
}

/// File name with any extension stripped.
///
/// For `todo.md`, this stores `todo`. Dotfiles such as `.gitignore` keep their
/// full text as the stem.
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
}
