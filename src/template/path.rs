//! [`TemplatePath`]: a template identifier resolved against configured
//! directories, proven to exist on disk and safe to read.
//!
//! [`TemplatePathError`] covers every way validation or search can fail.

use std::{
    fs, io,
    path::{Path, PathBuf},
};

use thiserror::Error;

use crate::path::SafeRelativePath;

/// The extension every rendered note gets by default, absent an explicit
/// `-o`/`file.write_to()` override.
const DEFAULT_EXTENSION: &str = "md";

/// A validated template path resolved against a template directory, proven to
/// exist on disk.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct TemplatePath {
    path: SafeRelativePath,
    source_dir: PathBuf,
}

impl TemplatePath {
    /// Creates a resolved [`TemplatePath`] from an already-validated relative
    /// path and its source directory. `path` must come from [`Self::parse`]
    /// (or another [`SafeRelativePath`] proven safe the same way) — this is
    /// the only constructor, so a [`TemplatePath`] can never wrap an
    /// unvalidated path.
    #[inline]
    #[must_use]
    pub(super) fn new(path: SafeRelativePath, source_dir: PathBuf) -> Self {
        Self {
            path,
            source_dir,
        }
    }

    /// Validates `path`'s components via [`SafeRelativePath::parse`] — no
    /// filesystem access, purely a check on the path's shape. Re-derives which
    /// of the two rejection reasons applies, since
    /// [`SafeRelativePath::parse`]'s single
    /// [`PathError`](crate::path::PathError) doesn't distinguish them.
    ///
    /// # Errors
    ///
    /// Returns [`TemplatePathError::Absolute`] when `path` is absolute.
    /// Returns [`TemplatePathError::UnsafeComponent`] for `..`, any component
    /// that isn't a plain name or `.`, or a path with no [`Component::Normal`].
    ///
    /// [`Component::Normal`]: std::path::Component::Normal
    pub(super) fn parse(
        path: &Path,
    ) -> Result<SafeRelativePath, TemplatePathError> {
        SafeRelativePath::parse(path).map_err(|_| {
            if path.is_absolute() {
                TemplatePathError::Absolute(path.to_path_buf())
            } else {
                TemplatePathError::UnsafeComponent(path.to_path_buf())
            }
        })
    }

    /// This candidate with its extension stripped and directory segments kept:
    /// `"folder/daily.md"` -> `"folder/daily"`.
    #[inline]
    #[must_use]
    pub(super) fn name(&self) -> PathBuf {
        self.path.as_ref().with_extension("")
    }

    /// Whether this candidate carries an extension: `"daily.md"` -> `true`.
    #[inline]
    #[must_use]
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "tested in has_extension unit tests")
    )]
    pub(super) fn has_extension(&self) -> bool {
        self.path.as_ref().extension().is_some()
    }

    /// Builds the absolute path on demand: `source_dir` joined with `path`.
    #[inline]
    #[must_use]
    pub(super) fn absolute(&self) -> PathBuf {
        self.source_dir.join(self.path.as_ref())
    }

    /// The default output filename: [`Self::name`] with extension forced to
    /// `md`.
    #[inline]
    #[must_use]
    pub(super) fn default_output_filename(&self) -> PathBuf {
        self.name().with_extension(DEFAULT_EXTENSION)
    }

    /// Reads this resolved template's source from disk.
    pub(super) fn read(&self) -> io::Result<String> {
        fs::read_to_string(self.absolute())
    }
}

impl AsRef<Path> for TemplatePath {
    fn as_ref(&self) -> &Path {
        self.path.as_ref()
    }
}

/// Every way producing a [`TemplatePath`] can fail: validation
/// ([`Self::Absolute`], [`Self::UnsafeComponent`]) and search
/// ([`Self::AmbiguousTemplate`], [`Self::TemplateNotFound`], or
/// [`Self::DirectoryRead`]).
#[derive(Debug, Error)]
pub(crate) enum TemplatePathError {
    /// `name` is absolute. A template identifier must be relative to whichever
    /// directory it is searched in.
    #[error("template path {0} must be relative, not absolute")]
    Absolute(PathBuf),
    /// `name` can't stay inside a directory: some component could escape it
    /// (most notably `..`), or there's no
    /// [`std::path::Component::Normal`] component at
    /// all (an empty path, or a bare `.`).
    #[error("template path {0} is not a valid template identifier")]
    UnsafeComponent(PathBuf),
    /// More than one file in a Template Directory matched the name.
    #[error("template name \"{name}\" matched multiple files: {candidates:?}")]
    AmbiguousTemplate {
        /// The identifier the User requested.
        name: PathBuf,
        /// Matching paths relative to the Template Directory.
        candidates: Vec<PathBuf>,
    },
    /// The configured Template Directory could not be read.
    #[error("failed to read template directory {directory}")]
    DirectoryRead {
        /// The Template Directory that could not be read.
        directory: PathBuf,
        /// The underlying filesystem failure.
        #[source]
        source: io::Error,
    },
    /// No searched Template Directory had a match.
    #[error("template \"{0}\" not found")]
    TemplateNotFound(PathBuf),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn validated(name: &str) -> TemplatePath {
        let rel =
            TemplatePath::parse(Path::new(name)).expect("valid candidate");
        TemplatePath::new(rel, PathBuf::from("/dir"))
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
            let error = TemplatePath::parse(Path::new("/etc/passwd"))
                .expect_err("absolute path is rejected");

            assert!(matches!(error, TemplatePathError::Absolute(_)));
        }

        #[rstest]
        #[case::parent_traversal("../outside.md")]
        #[case::nested_parent_traversal("folder/../../outside.md")]
        #[case::empty_path("")]
        #[case::bare_current_dir(".")]
        fn rejects_unsafe_components(#[case] input: &str) {
            let error = TemplatePath::parse(Path::new(input))
                .expect_err("unsafe component is rejected");

            assert!(matches!(error, TemplatePathError::UnsafeComponent(_)));
        }
    }

    mod name {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn strips_only_the_extension_keeping_directory_segments() {
            assert_eq!(
                validated("folder/report.md").name(),
                Path::new("folder/report")
            );
        }

        #[test]
        fn strips_the_extension_from_a_flat_path_with_no_directory() {
            assert_eq!(validated("daily.md").name(), Path::new("daily"));
        }

        #[test]
        fn is_unchanged_for_an_extensionless_path() {
            assert_eq!(validated("daily").name(), Path::new("daily"));
        }

        #[test]
        fn keeps_the_leading_dot_of_a_dot_prefixed_file() {
            assert_eq!(validated(".draft.md").name(), Path::new(".draft"));
        }
    }

    mod has_extension {
        use super::*;

        #[test]
        fn is_true_when_a_dot_extension_is_present() {
            assert!(validated("daily.md").has_extension());
        }

        #[test]
        fn is_false_for_a_bare_name() {
            assert!(!validated("daily").has_extension());
        }

        #[test]
        fn is_false_for_a_dot_prefixed_file_without_a_real_extension() {
            // ".draft" is a dotfile, not an extension: Path::extension()
            // treats a lone leading dot as part of the file stem, the
            // same convention `name()` relies on to keep it intact.
            assert!(!validated(".draft").has_extension());
        }
    }

    mod default_output_filename {
        use pretty_assertions::assert_eq;

        use super::*;

        fn found(dir: &Path, name: &str) -> TemplatePath {
            let path = write_file(dir, name);
            let rel = path.strip_prefix(dir).expect("relative path");
            TemplatePath::new(
                TemplatePath::parse(rel).expect("relative path is safe"),
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
            TemplatePath::new(
                TemplatePath::parse(rel).expect("relative path is safe"),
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
