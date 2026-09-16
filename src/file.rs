//! File metadata and file-name newtypes.
//!
//! This module provides two families of types for working with files in a
//! project:
//!
//! - [`FileBase`] captures filesystem metadata (path, timestamps, size) for
//!   every regular file under a project root.
//! - The file-name newtypes ([`FileName`], [`BaseName`], [`BaseNameRef`])
//!   represent different views of a path's final component: full name, owned
//!   stem, and borrowed stem respectively.
//!
//! # File-name decomposition
//!
//! Given a path like `notes/todo.md`:
//!
//! - [`FileName`] stores `todo.md` (the final component, including extension).
//! - [`BaseName`] stores `todo` (the stem, extension stripped).
//! - [`BaseNameRef`] borrows the same `todo` without allocation.
//!
//! Dotfiles follow [`Path::file_stem`]: `.gitignore` has no extension, so both
//! [`FileName`] and [`BaseName`] store `.gitignore`.
//!
//! # Timestamps
//!
//! [`FileBase`] stores metadata timestamps as [`crate::DateTimeValue`], the
//! crate's single date/date-time type (see `src/date.rs`).

use std::{
    fs,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    DateTimeValue,
    path::{FolderRef, RelativePath},
};

/// Metadata captured for one regular file under a project root.
///
/// Stored paths are project-root-relative so the index can move with the
/// project directory.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct FileBase {
    #[serde(with = "crate::index::path")]
    path: PathBuf,
    name: BaseName,
    #[serde(with = "crate::index::path")]
    folder: PathBuf,
    format: FileFormat,
    created_at: Option<DateTimeValue>,
    modified_at: DateTimeValue,
    size: u64,
}

impl FileBase {
    /// Builds a [`FileBase`] from filesystem metadata.
    ///
    /// `relative` is a project-root-relative path already validated by the
    /// scanner. The modification time is read from `metadata`; creation time is
    /// captured if the host OS reports it, and [`None`] otherwise.
    ///
    /// # Errors
    ///
    /// - [`std::io::Error`] if the file's modification time cannot be read.
    pub(crate) fn from_metadata(
        relative: RelativePath,
        metadata: &fs::Metadata,
    ) -> Result<Self, std::io::Error> {
        let relative = relative.into_path_buf();
        let modified_at = metadata.modified().map(DateTimeValue::from)?;
        let created_at = metadata.created().map(DateTimeValue::from).ok();
        let file_name =
            FileName::try_from(relative.as_path()).unwrap_or_default();
        let name = BaseName::from(&file_name);
        let format = FileFormat::from_name(&file_name);
        let folder =
            relative.parent().unwrap_or_else(|| Path::new("")).to_path_buf();

        Ok(Self {
            path: relative,
            name,
            folder,
            format,
            created_at,
            modified_at,
            size: metadata.len(),
        })
    }

    /// Builds a [`FileBase`] with custom fields for test fixtures.
    #[cfg(any(test, feature = "test-utils"))]
    #[inline]
    #[must_use]
    pub fn for_test<P: Into<PathBuf>>(path: P, format: FileFormat) -> Self {
        let path = path.into();
        let folder =
            path.parent().unwrap_or_else(|| Path::new("")).to_path_buf();
        let file_name = FileName::try_from(path.as_path()).unwrap_or_default();
        let name = BaseName::from(&file_name);
        Self {
            path,
            name,
            folder,
            format,
            created_at: None,
            modified_at: DateTimeValue::now(),
            size: 10,
        }
    }

    /// Builds a [`FileBase`] for a Markdown note with custom paths for test
    /// fixtures.
    #[cfg(any(test, feature = "test-utils"))]
    #[inline]
    #[must_use]
    pub fn note_for_test<P: Into<PathBuf>>(path: P) -> Self {
        Self::for_test(path, FileFormat::Note)
    }

    /// Builds a [`FileBase`] for a Markdown note with custom paths and byte
    /// size for test fixtures.
    #[cfg(any(test, feature = "test-utils"))]
    #[inline]
    #[must_use]
    pub fn note_with_size_for_test<P: Into<PathBuf>>(
        path: P,
        size: u64,
    ) -> Self {
        let mut file = Self::for_test(path, FileFormat::Note);
        file.size = size;
        file
    }

    /// Returns the file's path, relative to the project root.
    #[inline]
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Returns the file's name, without its extension.
    #[inline]
    #[must_use]
    pub(crate) const fn name(&self) -> &BaseName {
        &self.name
    }

    /// Returns the file's parent directory, relative to the project root.
    ///
    /// Returns a [`FolderRef`] pointing to `""` for files directly under the
    /// project root.
    #[inline]
    #[must_use]
    pub(crate) fn folder(&self) -> FolderRef<'_> {
        FolderRef::of(&self.path)
    }

    /// Returns whether this file is a Markdown note or another regular file.
    #[must_use]
    pub(crate) const fn format(&self) -> FileFormat {
        self.format
    }

    /// Returns the filesystem creation timestamp, if the host reports one.
    ///
    /// Use [`Self::created_at_or_modified`] when unsupported creation times
    /// should fall back to [`Self::modified_at`].
    #[inline]
    #[must_use]
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "no current caller outside tests; documented deliberate \
                      API in index-query#01's FileIndex baseline design, \
                      distinct from created_at_or_modified which field \
                      resolution uses"
        )
    )]
    pub(crate) const fn created_at(&self) -> Option<DateTimeValue> {
        self.created_at
    }

    /// Returns [`Self::created_at`] when available, falling back to
    /// [`Self::modified_at`] when creation time is unsupported on the host OS
    /// or filesystem.
    #[inline]
    #[must_use]
    pub(crate) fn created_at_or_modified(&self) -> DateTimeValue {
        self.created_at.unwrap_or(self.modified_at)
    }

    /// Returns this file's last modification time.
    #[inline]
    #[must_use]
    pub(crate) const fn modified_at(&self) -> DateTimeValue {
        self.modified_at
    }

    /// Returns this file's size in bytes.
    #[inline]
    #[must_use]
    pub(crate) const fn size(&self) -> u64 {
        self.size
    }
}

/// Final path component of a file, including any extension.
///
/// Wraps the text returned by [`Path::file_name`]. For `todo.md`, stores
/// `todo.md`. For `.gitignore`, stores `.gitignore`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct FileName(String);

impl FileName {
    /// Returns this name's extension, if any.
    ///
    /// Dotfiles without another extension return [`None`]. For example,
    /// `.gitignore` has no extension, while `.env.local` returns
    /// `Some("local")`.
    #[must_use]
    pub(crate) fn extension(&self) -> Option<&str> {
        Path::new(&self.0).extension().and_then(|ext| ext.to_str())
    }
}

impl TryFrom<&Path> for FileName {
    type Error = FileNameError;

    /// Builds a [`FileName`] from `path`'s final component.
    ///
    /// # Errors
    ///
    /// - [`FileNameError::Missing`] if `path` has no final component, such as
    ///   `/`, `..`, or an empty path.
    fn try_from(path: &Path) -> Result<Self, Self::Error> {
        path.file_name()
            .map(|name| Self(name.to_string_lossy().into_owned()))
            .ok_or(FileNameError::Missing)
    }
}

/// Owned file name with any extension stripped.
///
/// Uses [`Path::file_stem`] on [`FileName`]'s stored text. For `todo.md`,
/// stores `todo`. Dotfiles such as `.gitignore` keep their full text as the
/// stem.
#[derive(Clone, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
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

/// Borrowed file name with any extension stripped.
///
/// Use this instead of [`BaseName`] when a comparison or hash lookup can borrow
/// directly from a [`Path`]. Dotfile behavior matches [`Path::file_stem`].
#[derive(Copy, Clone, Debug, Eq, Hash, PartialEq)]
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
    pub(crate) const fn as_str(&self) -> &str {
        self.0
    }
}

impl std::borrow::Borrow<str> for BaseNameRef<'_> {
    fn borrow(&self) -> &str {
        self.0
    }
}

/// Coarse file classification used by the two-tier index.
///
/// Markdown notes get parsed [`crate::Note`] metadata in addition to their
/// [`FileBase`]. Other files only keep general file metadata.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub enum FileFormat {
    /// Markdown file parsed into a [`crate::Note`].
    Note,
    /// Regular non-Markdown file.
    Other,
}

impl FileFormat {
    /// Classifies `.md` and `.markdown` file names as [`Self::Note`].
    ///
    /// Extension matching is ASCII case-insensitive. Every other extension, or
    /// a missing extension, is [`Self::Other`].
    fn from_name(name: &FileName) -> Self {
        match name.extension() {
            Some(ext)
                if ext.eq_ignore_ascii_case("md")
                    || ext.eq_ignore_ascii_case("markdown") =>
            {
                Self::Note
            }
            _ => Self::Other,
        }
    }
}

/// Reports why a [`FileName`] could not be constructed.
#[derive(Debug, Error)]
pub(crate) enum FileNameError {
    /// The path has no final component.
    #[error("path has no file name")]
    Missing,
}

#[cfg(test)]
mod tests {
    use chrono::Utc;

    use super::*;

    /// Builds a `FileBase` with `created_at`/`modified_at` set directly, for
    /// exercising timestamp accessor behavior without touching the filesystem.
    fn record_with(
        created_at: Option<DateTimeValue>,
        modified_at: DateTimeValue,
    ) -> FileBase {
        FileBase {
            path: PathBuf::from("note.md"),
            name: BaseName::from(
                &FileName::try_from(Path::new("note.md"))
                    .expect("valid file name"),
            ),
            folder: PathBuf::new(),
            format: FileFormat::Note,
            created_at,
            modified_at,
            size: 0,
        }
    }

    mod file_record {
        use super::*;

        mod from_metadata {
            use pretty_assertions::assert_eq;

            use super::*;

            fn metadata_for(path: &Path) -> fs::Metadata {
                fs::metadata(path).expect("read metadata")
            }

            #[test]
            fn splits_the_name_from_the_extension() {
                let temp = tempfile::tempdir().expect("create temp dir");
                let file = temp.path().join("notes").join("todo.md");
                fs::create_dir_all(file.parent().expect("parent"))
                    .expect("mkdir");
                fs::write(&file, "content").expect("write file");

                let relative = RelativePath::derive(temp.path(), &file)
                    .expect("derive relative path");
                let record =
                    FileBase::from_metadata(relative, &metadata_for(&file))
                        .expect("build record");

                assert_eq!(record.name().as_str(), "todo");
                assert_eq!(record.path(), Path::new("notes/todo.md"));
                assert_eq!(record.folder().as_path(), Path::new("notes"));
                assert_eq!(record.format(), FileFormat::Note);
                assert_eq!(record.size(), 7);
                assert!(record.modified_at().into_inner() <= Utc::now());
            }

            #[test]
            fn returns_an_empty_folder_when_the_file_is_directly_under_root() {
                let temp = tempfile::tempdir().expect("create temp dir");
                let file = temp.path().join("readme.md");
                fs::write(&file, "hi").expect("write file");

                let relative = RelativePath::derive(temp.path(), &file)
                    .expect("derive relative path");
                let record =
                    FileBase::from_metadata(relative, &metadata_for(&file))
                        .expect("build record");

                assert_eq!(record.name().as_str(), "readme");
                assert_eq!(record.path(), Path::new("readme.md"));
                assert_eq!(record.folder().as_path(), Path::new(""));
                assert_eq!(record.format(), FileFormat::Note);
                assert_eq!(record.size(), 2);
            }
        }

        mod created_at {
            use pretty_assertions::assert_eq;

            use super::*;

            #[test]
            fn returns_none_when_creation_time_is_unsupported() {
                let record = record_with(None, DateTimeValue::now());

                assert_eq!(record.created_at(), None);
            }

            #[test]
            fn returns_some_when_creation_time_is_reported() {
                let modified_at = DateTimeValue::now();
                let reported = DateTimeValue::from(
                    modified_at.into_inner() - chrono::Duration::days(1),
                );
                let record = record_with(Some(reported), modified_at);

                assert_eq!(record.created_at(), Some(reported));
            }
        }

        mod created_at_or_modified {
            use pretty_assertions::assert_eq;

            use super::*;

            #[test]
            fn returns_created_when_present() {
                let modified_at = DateTimeValue::now();
                let reported = DateTimeValue::from(
                    modified_at.into_inner() - chrono::Duration::days(1),
                );
                let record = record_with(Some(reported), modified_at);

                assert_eq!(record.created_at_or_modified(), reported);
            }

            #[test]
            fn falls_back_to_modified_when_created_is_none() {
                let modified_at = DateTimeValue::now();
                let record = record_with(None, modified_at);

                assert_eq!(record.created_at_or_modified(), modified_at);
            }
        }
    }

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

            assert!(matches!(error, FileNameError::Missing));
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

    mod format {
        use pretty_assertions::assert_eq;
        use rstest::rstest;

        use super::*;

        #[rstest]
        #[case::lowercase_md_extension("note.md", FileFormat::Note)]
        #[case::uppercase_markdown_extension("note.MARKDOWN", FileFormat::Note)]
        #[case::non_markdown_extension("config.toml", FileFormat::Other)]
        #[case::no_extension("LICENSE", FileFormat::Other)]
        fn classifies_by_extension(
            #[case] file_name: &str,
            #[case] expected: FileFormat,
        ) {
            let name = FileName::try_from(Path::new(file_name))
                .expect("valid file name");

            assert_eq!(FileFormat::from_name(&name), expected);
        }
    }
}
