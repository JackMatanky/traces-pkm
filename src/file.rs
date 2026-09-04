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
//! [`Timestamp`] wraps [`DateTime<Utc>`] to unify formatting and ordering
//! across the index layer. It provides several format helpers for query field
//! values (e.g., `ctime`, `mdate`).
//!
//! [`DateTime<Utc>`]: chrono::DateTime
//! [`DateTime<Utc>`]: chrono::Utc

use std::{
    fs,
    path::{Path, PathBuf},
    time::SystemTime,
};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;

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
    created_at: Option<Timestamp>,
    modified_at: Timestamp,
    size: u64,
}

impl FileBase {
    /// Builds a [`FileBase`] from filesystem metadata.
    ///
    /// `path` is the absolute file path under `root`; both are used to store a
    /// project-relative path in the record. The modification time is read from
    /// `metadata`; creation time is captured if the host OS reports it, and
    /// [`None`] otherwise.
    ///
    /// # Errors
    ///
    /// - [`std::io::Error`] if the file's modification time cannot be read.
    pub(crate) fn from_metadata(
        path: &Path,
        root: &Path,
        metadata: &fs::Metadata,
    ) -> Result<Self, std::io::Error> {
        // TODO: `unwrap_or` silently stores absolute paths for any input
        // outside `root`. Replace with a strict lexical confinement check (see
        // `SafeRelativePath`); deferred from the dirtree deepening because
        // per-file `RootConfinedPath` canonicalization would add filesystem
        // syscalls to every index build.
        let relative = path.strip_prefix(root).unwrap_or(path).to_path_buf();
        let modified_at = metadata.modified().map(Timestamp::from)?;
        let created_at = metadata.created().map(Timestamp::from).ok();
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
    #[cfg(test)]
    pub(crate) fn new_test(
        path: PathBuf,
        folder: PathBuf,
        format: FileFormat,
    ) -> Self {
        let file_name = FileName::try_from(path.as_path()).unwrap_or_default();
        let name = BaseName::from(&file_name);
        Self {
            path,
            name,
            folder,
            format,
            created_at: None,
            modified_at: Timestamp::now(),
            size: 10,
        }
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
    /// Returns an empty [`Path`] for files directly under the project root.
    #[inline]
    #[must_use]
    pub(crate) fn folder(&self) -> &Path {
        &self.folder
    }

    /// Returns whether this file is a markdown note or another regular file.
    #[inline]
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
    pub(crate) const fn created_at(&self) -> Option<Timestamp> {
        self.created_at
    }

    /// Returns [`Self::created_at`] when available, falling back to
    /// [`Self::modified_at`] when creation time is unsupported on the host
    /// OS or filesystem.
    #[inline]
    #[must_use]
    pub(crate) fn created_at_or_modified(&self) -> Timestamp {
        self.created_at.unwrap_or(self.modified_at)
    }

    /// Returns this file's last modification time.
    #[inline]
    #[must_use]
    pub(crate) const fn modified_at(&self) -> Timestamp {
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

/// Reports that a path has no final component.
#[derive(Debug, Error)]
#[error("path has no file name")]
pub(crate) struct MissingFileName;

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
/// Use this instead of [`BaseName`] when a comparison or hash lookup can
/// borrow directly from a [`Path`]. Dotfile behavior matches
/// [`Path::file_stem`].
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
/// Markdown notes get parsed [`Note`] metadata in addition to their
/// [`FileBase`]. Other files only keep general file metadata.
///
/// [`Note`]: crate::Note
#[derive(Copy, Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub(crate) enum FileFormat {
    /// Markdown file parsed into a [`Note`].
    ///
    /// [`Note`]: crate::Note
    Note,
    /// Regular non-markdown file.
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

/// UTC timestamp stored with indexed file metadata.
///
/// Wraps [`DateTime<Utc>`] so index code uses one formatting and ordering type
/// instead of leaking filesystem clock details.
///
/// [`DateTime<Utc>`]: chrono::DateTime
/// [`Utc`]: chrono::Utc
#[derive(
    Copy, Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize,
)]
pub(crate) struct Timestamp(DateTime<Utc>);

impl Timestamp {
    /// Returns the current UTC timestamp.
    #[inline]
    #[must_use]
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "no current caller outside tests; kept for Timestamp \
                      constructor symmetry with Utc::now"
        )
    )]
    pub(crate) fn now() -> Self {
        Self(Utc::now())
    }

    /// Formats this timestamp as an RFC 3339 date and time with a UTC offset.
    ///
    /// Produces values like `"2026-07-29T14:30:00+00:00"`. The offset is always
    /// `+00:00` because [`Timestamp`] is always UTC, so prefer
    /// [`Self::to_datetime_string`] unless the offset itself matters.
    #[inline]
    #[must_use]
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "no current caller outside tests; documented deliberate \
                      API in index-query#05, split out alongside \
                      to_datetime_string/to_date_string which field \
                      resolution uses"
        )
    )]
    pub(crate) fn to_offset_string(self) -> String {
        self.0.to_rfc3339()
    }

    /// Formats this timestamp as a date and time without a UTC offset.
    ///
    /// Produces values like `"2026-07-29T14:30:00"` for the `ctime`/`mtime`
    /// query field values, where offset text would break literal filter
    /// matching.
    #[inline]
    #[must_use]
    pub(crate) fn to_datetime_string(self) -> String {
        self.0.format("%Y-%m-%dT%H:%M:%S").to_string()
    }

    /// Formats this timestamp as a bare date without time or offset.
    ///
    /// Produces values like `"2026-07-29"` for the `cdate`/`mdate` query
    /// field values.
    #[inline]
    #[must_use]
    pub(crate) fn to_date_string(self) -> String {
        self.0.format("%Y-%m-%d").to_string()
    }

    /// Formats this timestamp as a bare time-of-day component without a date.
    ///
    /// Produces values like `"14:30:00"`.
    #[inline]
    #[must_use]
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "no current caller outside tests; documented deliberate \
                      API in index-query#05, added alongside \
                      to_datetime_string/to_date_string which field \
                      resolution uses"
        )
    )]
    pub(crate) fn to_time_string(self) -> String {
        self.0.format("%H:%M:%S").to_string()
    }

    /// Returns a new timestamp truncated to the start of the UTC day (midnight,
    /// 00:00:00).
    #[inline]
    #[must_use]
    pub(crate) fn start_of_day(self) -> Self {
        let naive = match self.0.date_naive().and_hms_opt(0, 0, 0) {
            Some(midnight) => midnight,
            None => self.0.naive_utc(),
        };
        Self(naive.and_utc())
    }

    /// Returns `true` if this timestamp has a non-zero time-of-day component.
    #[inline]
    #[must_use]
    pub(crate) fn has_time_component(self) -> bool {
        use chrono::Timelike as _;
        self.0.hour() != 0
            || self.0.minute() != 0
            || self.0.second() != 0
            || self.0.nanosecond() != 0
    }

    /// Formats this timestamp into `out`, using [`Self::to_datetime_string`]'s
    /// form when it has a non-zero time-of-day component (see
    /// [`Self::has_time_component`]), or [`Self::to_date_string`]'s bare-date
    /// form otherwise. Writes directly into `out` with no intermediate
    /// `String` allocation, unlike calling either formatter and pushing its
    /// result.
    #[inline]
    pub(crate) fn append_conditional(self, out: &mut String) {
        use std::fmt::Write as _;
        if self.has_time_component() {
            let _ = write!(out, "{}", self.0.format("%Y-%m-%dT%H:%M:%S"));
        } else {
            let _ = write!(out, "{}", self.0.format("%Y-%m-%d"));
        }
    }

    /// Formats this timestamp as an owned [`String`], using
    /// [`Self::to_datetime_string`]'s form when it has a non-zero
    /// time-of-day component, or [`Self::to_date_string`]'s bare-date form
    /// otherwise. Prefer [`Self::append_conditional`] when writing into an
    /// existing buffer.
    #[inline]
    #[must_use]
    pub(crate) fn to_conditional_string(self) -> String {
        if self.has_time_component() {
            self.to_datetime_string()
        } else {
            self.to_date_string()
        }
    }

    /// Parses an ISO-8601, RFC 3339, or `YYYY-MM-DD` date string into a
    /// `Timestamp`.
    ///
    /// When given a bare date `YYYY-MM-DD`, parses strictly as midnight UTC
    /// (`00:00:00.000Z`).
    pub(crate) fn parse_iso(input: &str) -> Option<Self> {
        let trimmed = input.trim();
        if let Ok(dt) = DateTime::parse_from_rfc3339(trimmed) {
            return Some(Self(dt.with_timezone(&Utc)));
        }
        if let Ok(naive) =
            chrono::NaiveDateTime::parse_from_str(trimmed, "%Y-%m-%dT%H:%M:%S")
        {
            return Some(Self(naive.and_utc()));
        }
        if let Ok(naive) =
            chrono::NaiveDateTime::parse_from_str(trimmed, "%Y-%m-%d %H:%M:%S")
        {
            return Some(Self(naive.and_utc()));
        }
        if let Ok(date) = chrono::NaiveDate::parse_from_str(trimmed, "%Y-%m-%d")
        {
            let midnight = date.and_hms_opt(0, 0, 0)?;
            return Some(Self(midnight.and_utc()));
        }
        None
    }
}

impl From<SystemTime> for Timestamp {
    fn from(time: SystemTime) -> Self {
        Self(DateTime::<Utc>::from(time))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a `FileBase` with `created_at`/`modified_at` set directly, for
    /// exercising timestamp accessor behavior without touching the filesystem.
    fn record_with(
        created_at: Option<Timestamp>,
        modified_at: Timestamp,
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

                let record = FileBase::from_metadata(
                    &file,
                    temp.path(),
                    &metadata_for(&file),
                )
                .expect("build record");

                assert_eq!(record.name().as_str(), "todo");
                assert_eq!(record.path(), Path::new("notes/todo.md"));
                assert_eq!(record.folder(), Path::new("notes"));
                assert_eq!(record.format(), FileFormat::Note);
                assert_eq!(record.size(), 7);
                assert!(record.modified_at().0 <= Utc::now());
            }

            #[test]
            fn returns_an_empty_folder_when_the_file_is_directly_under_root() {
                let temp = tempfile::tempdir().expect("create temp dir");
                let file = temp.path().join("readme.md");
                fs::write(&file, "hi").expect("write file");

                let record = FileBase::from_metadata(
                    &file,
                    temp.path(),
                    &metadata_for(&file),
                )
                .expect("build record");

                assert_eq!(record.name().as_str(), "readme");
                assert_eq!(record.path(), Path::new("readme.md"));
                assert_eq!(record.folder(), Path::new(""));
                assert_eq!(record.format(), FileFormat::Note);
                assert_eq!(record.size(), 2);
            }
        }

        mod created_at {
            use pretty_assertions::assert_eq;

            use super::*;

            #[test]
            fn returns_none_when_creation_time_is_unsupported() {
                let record = record_with(None, Timestamp::now());

                assert_eq!(record.created_at(), None);
            }

            #[test]
            fn returns_some_when_creation_time_is_reported() {
                let modified_at = Timestamp::now();
                let reported =
                    Timestamp(modified_at.0 - chrono::Duration::days(1));
                let record = record_with(Some(reported), modified_at);

                assert_eq!(record.created_at(), Some(reported));
            }
        }

        mod created_at_or_modified {
            use pretty_assertions::assert_eq;

            use super::*;

            #[test]
            fn returns_created_when_present() {
                let modified_at = Timestamp::now();
                let reported =
                    Timestamp(modified_at.0 - chrono::Duration::days(1));
                let record = record_with(Some(reported), modified_at);

                assert_eq!(record.created_at_or_modified(), reported);
            }

            #[test]
            fn falls_back_to_modified_when_created_is_none() {
                let modified_at = Timestamp::now();
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

    mod timestamps {
        use chrono::TimeZone;
        use pretty_assertions::assert_eq;

        use super::*;

        fn fixed_timestamp() -> Timestamp {
            Timestamp(
                Utc.with_ymd_and_hms(2026, 7, 29, 14, 30, 5).single().expect(
                    "2026-07-29 14:30:05 UTC is a valid, unambiguous instant",
                ),
            )
        }

        #[test]
        fn to_offset_string_includes_the_utc_offset() {
            assert_eq!(
                fixed_timestamp().to_offset_string(),
                "2026-07-29T14:30:05+00:00"
            );
        }

        #[test]
        fn to_datetime_string_omits_the_offset() {
            assert_eq!(
                fixed_timestamp().to_datetime_string(),
                "2026-07-29T14:30:05"
            );
        }

        #[test]
        fn to_date_string_omits_the_time() {
            assert_eq!(fixed_timestamp().to_date_string(), "2026-07-29");
        }

        #[test]
        fn to_time_string_omits_the_date() {
            assert_eq!(fixed_timestamp().to_time_string(), "14:30:05");
        }
    }
}
