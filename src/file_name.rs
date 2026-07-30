//! [`FileName`]/[`BaseName`]: a file's full name and its extension-stripped
//! stem, as two distinct types instead of interchangeable strings.
//!
//! Not specific to any one subsystem - the index's `FileRecord` is the
//! first consumer, but anything reasoning about a file's name or extension
//! (e.g. `template::engine::path_ops`'s `basename`/`extension` filters)
//! shares these instead of reinventing stem/extension parsing.

use std::path::Path;

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// A file's full name, including any extension (e.g. `"todo.md"`) - the
/// last component of a path. Distinct from [`BaseName`], which strips the
/// extension.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct FileName(String);

impl FileName {
    /// This name's extension, if any (e.g. `"md"` for `"todo.md"`).
    #[must_use]
    pub(crate) fn extension(&self) -> Option<&str> {
        Path::new(&self.0).extension().and_then(|ext| ext.to_str())
    }
}

/// `path` has no final component to name a file with (e.g. `/`, `..`, or an
/// empty path).
#[derive(Debug, Error)]
#[error("path has no file name")]
pub(crate) struct MissingFileName;

impl TryFrom<&Path> for FileName {
    type Error = MissingFileName;

    /// # Errors
    ///
    /// Returns [`MissingFileName`] if `path` has no final component (e.g.
    /// `/`, `..`, or an empty path).
    fn try_from(path: &Path) -> Result<Self, Self::Error> {
        path.file_name()
            .map(|name| Self(name.to_string_lossy().into_owned()))
            .ok_or(MissingFileName)
    }
}

/// A file's name with any extension stripped (e.g. `"todo"` for
/// `"todo.md"`). Distinct from [`FileName`], which keeps the extension.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct BaseName(String);

impl BaseName {
    /// This name as a plain string slice.
    #[inline]
    #[must_use]
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&FileName> for BaseName {
    /// Strips `name`'s extension, if any. Always succeeds: a name with no
    /// extension (or a dotfile like `.gitignore`, where the leading dot
    /// isn't treated as an extension separator) keeps its full text as the
    /// stem.
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
