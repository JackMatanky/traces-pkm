//! Validate and label path values in the template pipeline.
//!
//! Public API:
//!
//! - [`TemplatePathInput`] - Relative template identifier accepted at CLI and
//!   include boundaries.
//! - [`TemplatePath`] - Resolved template path produced after loader lookup.
//! - [`DeclaredOutputPath`] - Raw `file.write_to()` value captured during
//!   rendering.
//! - [`TemplatePathError`] - Validation and lookup failures for template
//!   identifiers.
//!
//! [`TemplatePath`] is created by [`TemplateLoader`] immediately after finding
//! a real file. [`DeclaredOutputPath`] is intentionally not validated here;
//! [`writer`] resolves and confines it alongside `-o` output paths.
//!
//! [`TemplateLoader`]: super::loader::TemplateLoader
//! [`writer`]: super::writer

use std::{
    fs, io,
    path::{Path, PathBuf},
};

use thiserror::Error;

use crate::path::{PathError, SafeRelativePath};

/// The extension every rendered note gets by default, absent an explicit
/// `-o`/`file.write_to()` override.
const DEFAULT_EXTENSION: &str = "md";

/// Holds a validated template identifier before loader resolution.
///
/// The identifier is relative, contains at least one normal path component, and
/// cannot escape through parent traversal. It does not prove the template file
/// exists.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TemplatePathInput(SafeRelativePath);

impl TemplatePathInput {
    /// Parses a path as a template identifier.
    ///
    /// Accepts relative names such as `daily`, `daily.md`, and
    /// `folder/daily.md`. Rejects absolute paths, parent traversal, empty
    /// paths, and paths without a normal component.
    ///
    /// # Errors
    ///
    /// - [`TemplatePathError::Absolute`] if `path` is absolute.
    /// - [`TemplatePathError::UnsafeComponent`] for `..`, any component that is
    ///   not a plain name or `.`, or a path with no [`Component::Normal`].
    ///
    /// # Examples
    ///
    /// ```rust
    /// # #[cfg(feature = "test-utils")]
    /// fn main() -> Result<(), traces_pkm::TemplatePathError> {
    /// use std::path::Path;
    ///
    /// use traces_pkm::TemplatePathInput;
    ///
    /// let input = TemplatePathInput::parse(Path::new("daily.md"))?;
    /// assert_eq!(input.as_ref(), Path::new("daily.md"));
    /// # Ok(())
    /// # }
    /// # #[cfg(not(feature = "test-utils"))]
    /// # fn main() {}
    /// ```
    ///
    /// [`Component::Normal`]: std::path::Component::Normal
    #[inline]
    pub fn parse(path: &Path) -> Result<Self, TemplatePathError> {
        SafeRelativePath::parse(path).map(Self).map_err(|error| match error {
            PathError::Absolute => {
                TemplatePathError::Absolute(path.to_path_buf())
            }
            PathError::UnsafeComponent
            | PathError::EscapesRoot
            | PathError::Verify(_) => {
                TemplatePathError::UnsafeComponent(path.to_path_buf())
            }
        })
    }
}

impl AsRef<Path> for TemplatePathInput {
    #[inline]
    fn as_ref(&self) -> &Path {
        self.0.as_ref()
    }
}

/// A validated template path resolved against a template directory, proven to
/// exist on disk.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct TemplatePath {
    input: TemplatePathInput,
    source_dir: PathBuf,
}

impl TemplatePath {
    /// Creates a [`TemplatePath`] proven to exist by [`TemplateLoader`]'s
    /// search: called only from [`TemplateLoader::find_path_in`] and
    /// [`TemplateLoader::find_name_in`], immediately after each confirms the
    /// file exists. Nothing later in the pipeline re-verifies it.
    ///
    /// [`TemplateLoader`]: super::loader::TemplateLoader
    /// [`TemplateLoader::find_path_in`]: super::loader::TemplateLoader::find_path_in
    /// [`TemplateLoader::find_name_in`]: super::loader::TemplateLoader::find_name_in
    #[inline]
    #[must_use]
    pub(super) const fn verified(
        input: TemplatePathInput,
        source_dir: PathBuf,
    ) -> Self {
        Self {
            input,
            source_dir,
        }
    }

    /// Test-only fixture constructor, bypassing the existence guarantee
    /// [`Self::verified`] documents. Production code must go through
    /// [`TemplateLoader::find`].
    ///
    /// [`TemplateLoader::find`]: super::loader::TemplateLoader::find
    #[cfg(test)]
    #[must_use]
    pub(super) const fn for_test(
        input: TemplatePathInput,
        source_dir: PathBuf,
    ) -> Self {
        Self {
            input,
            source_dir,
        }
    }

    /// Returns the absolute path by joining `source_dir` with `input`.
    #[inline]
    #[must_use]
    pub(super) fn absolute(&self) -> PathBuf {
        self.source_dir.join(self.input.as_ref())
    }

    /// Returns the default output filename with its extension forced to `md`,
    /// keeping directory segments.
    #[inline]
    #[must_use]
    pub(super) fn default_output_filename(&self) -> PathBuf {
        self.input.as_ref().with_extension(DEFAULT_EXTENSION)
    }

    /// Reads this resolved template's source from disk.
    ///
    /// # Errors
    ///
    /// - [`io::Error`] if the resolved template file cannot be read.
    pub(super) fn read(&self) -> io::Result<String> {
        fs::read_to_string(self.absolute())
    }
}

impl AsRef<Path> for TemplatePath {
    fn as_ref(&self) -> &Path {
        self.input.as_ref()
    }
}

/// A raw `file.write_to()` path declared by the template during rendering.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct DeclaredOutputPath(PathBuf);

impl DeclaredOutputPath {
    #[inline]
    #[must_use]
    pub(super) const fn new(path: PathBuf) -> Self {
        Self(path)
    }

    #[inline]
    #[must_use]
    pub(super) fn as_path(&self) -> &Path {
        &self.0
    }
}

/// Reports template identifier validation and lookup failures.
///
/// Validation errors are raised before any template directory is searched.
/// Lookup errors are raised after validation succeeds.
#[derive(Debug, Error)]
pub enum TemplatePathError {
    /// The identifier was absolute.
    ///
    /// Template lookup only accepts paths relative to configured template
    /// directories.
    #[error("template path {0} must be relative, not absolute")]
    Absolute(PathBuf),
    /// The identifier was not a safe relative path.
    ///
    /// Covers parent traversal, unsupported components, an empty path, and a
    /// bare `.` with no normal component.
    #[error("template path {0} is not a valid template identifier")]
    UnsafeComponent(PathBuf),
    /// Stem lookup matched more than one file in one template directory.
    #[error("template name \"{name}\" matched multiple files: {candidates:?}")]
    AmbiguousTemplate {
        /// Requested identifier that produced multiple matches.
        name: PathBuf,
        /// Matching paths relative to the searched template directory.
        candidates: Vec<PathBuf>,
    },
    /// Reading a template directory failed during stem lookup.
    #[error("failed to read template directory {directory}")]
    DirectoryRead {
        /// Directory that could not be read.
        directory: PathBuf,
        /// Filesystem error returned while reading the directory.
        #[source]
        source: io::Error,
    },
    /// No configured template directory contained the requested template.
    #[error("template \"{0}\" not found")]
    TemplateNotFound(PathBuf),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn validated(name: &str) -> TemplatePath {
        let name =
            TemplatePathInput::parse(Path::new(name)).expect("valid candidate");
        TemplatePath::for_test(name, PathBuf::from("/dir"))
    }

    fn write_file(dir: &Path, name: &str) -> PathBuf {
        let path = dir.join(name);
        let parent = path.parent().expect("template path parent");
        fs::create_dir_all(parent).expect("create template parent");
        fs::write(&path, "content").expect("write template");
        path
    }

    mod validation {
        use pretty_assertions::assert_eq;
        use rstest::rstest;

        use super::*;

        #[test]
        fn accepts_a_plain_relative_name() {
            let path = validated("daily.md");

            assert_eq!(path.as_ref(), Path::new("daily.md"));
        }

        #[test]
        fn accepts_a_nested_relative_path() {
            let path = validated("folder/daily.md");

            assert_eq!(path.as_ref(), Path::new("folder/daily.md"));
        }

        #[test]
        fn accepts_a_path_with_a_leading_current_dir_segment() {
            // "./daily.md" splits into [CurDir, Normal("daily.md")]: a
            // leading CurDir component doesn't itself count toward
            // `has_normal_component`, but doesn't disqualify the path
            // either — the trailing Normal component still does. This
            // is the exact case `has_normal_component` exists to allow
            // (vs. a bare "." with no Normal component at all).
            let path = validated("./daily.md");

            assert_eq!(path.as_ref(), Path::new("./daily.md"));
        }

        #[test]
        fn rejects_an_absolute_path() {
            // A syntactically absolute path is rejected before any I/O
            // happens — parse() never reads the filesystem, so this
            // never touches whatever real file may or may not exist at
            // this well-known path.
            let error = TemplatePathInput::parse(Path::new("/etc/passwd"))
                .expect_err("absolute path is rejected");

            assert!(matches!(error, TemplatePathError::Absolute(_)));
        }

        #[rstest]
        #[case::parent_traversal("../outside.md")]
        #[case::nested_parent_traversal("folder/../../outside.md")]
        #[case::empty_path("")]
        #[case::bare_current_dir(".")]
        fn rejects_unsafe_components(#[case] input: &str) {
            let error = TemplatePathInput::parse(Path::new(input))
                .expect_err("unsafe component is rejected");

            assert!(matches!(error, TemplatePathError::UnsafeComponent(_)));
        }
    }

    mod default_output_filename {
        use pretty_assertions::assert_eq;

        use super::*;

        fn found(dir: &Path, name: &str) -> TemplatePath {
            let path = write_file(dir, name);
            let rel = path.strip_prefix(dir).expect("relative path");
            TemplatePath::for_test(
                TemplatePathInput::parse(rel).expect("relative path is safe"),
                dir.to_path_buf(),
            )
        }

        #[test]
        fn keeps_the_directory_and_the_default_extension() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let resolved = found(temp.path(), "folder/report.md");

            assert_eq!(
                resolved.default_output_filename(),
                Path::new("folder/report.md")
            );
        }

        #[test]
        fn forces_the_default_extension_over_the_source_files_own() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let resolved = found(temp.path(), "daily.txt");

            assert_eq!(
                resolved.default_output_filename(),
                Path::new("daily.md")
            );
        }
    }

    mod read {
        use pretty_assertions::assert_eq;

        use super::*;

        fn found(dir: &Path, name: &str) -> TemplatePath {
            let path = write_file(dir, name);
            let rel = path.strip_prefix(dir).expect("relative path");
            TemplatePath::for_test(
                TemplatePathInput::parse(rel).expect("relative path is safe"),
                dir.to_path_buf(),
            )
        }

        #[test]
        fn reads_the_resolved_templates_content() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let resolved = found(temp.path(), "daily.md");
            fs::write(resolved.absolute(), "hello world")
                .expect("overwrite fixture content");

            assert_eq!(resolved.read().expect("read succeeds"), "hello world");
        }

        #[test]
        fn propagates_the_io_error_when_the_file_is_removed_after_resolution() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let resolved = found(temp.path(), "daily.md");
            fs::remove_file(resolved.absolute()).expect("remove fixture");

            let error =
                resolved.read().expect_err("removed file fails to read");

            assert_eq!(error.kind(), io::ErrorKind::NotFound);
        }
    }
}
