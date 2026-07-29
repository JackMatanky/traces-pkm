//! File metadata captured by the index.
//!
//! [`FileRecord`] stores root-relative identity, type classification, file
//! timestamps, and size for every regular file under a project root.

use std::{
    fs,
    path::{Path, PathBuf},
    time::SystemTime,
};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::error::FileIndexError;
use crate::file_name::{BaseName, FileName};

/// Metadata captured for one regular file under a project root.
///
/// Stored paths are project-root-relative so the index can move with the
/// project directory.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub(crate) struct FileRecord {
    path: PathBuf,
    name: BaseName,
    folder: PathBuf,
    format: FileFormat,
    created: Option<Timestamp>,
    modified_at: Timestamp,
    size: u64,
}

impl FileRecord {
    /// Builds a [`FileRecord`] from filesystem metadata.
    ///
    /// `path` is the absolute file path under `root`; both are used to store a
    /// project-relative path in the record.
    ///
    /// # Errors
    ///
    /// Returns [`FileIndexError::Io`] if the file's modification time cannot
    /// be read.
    pub(super) fn from_metadata(
        path: &Path,
        root: &Path,
        metadata: &fs::Metadata,
    ) -> Result<Self, FileIndexError> {
        let relative = path.strip_prefix(root).unwrap_or(path).to_path_buf();
        let modified_at =
            metadata.modified().map(Timestamp::from).map_err(|source| {
                FileIndexError::Io {
                    path: path.to_path_buf(),
                    source,
                }
            })?;
        let created = metadata.created().map(Timestamp::from).ok();
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
            created,
            modified_at,
            size: metadata.len(),
        })
    }

    /// The file's path, relative to the project root.
    #[inline]
    #[must_use]
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    /// The file's name, without its extension.
    #[inline]
    #[must_use]
    pub(crate) fn name(&self) -> &BaseName {
        &self.name
    }

    /// The file's parent directory, relative to the project root. Empty for
    /// files directly under the project root.
    #[inline]
    #[must_use]
    pub(crate) fn folder(&self) -> &Path {
        &self.folder
    }

    /// Whether this file is a markdown note or another regular file.
    #[inline]
    #[must_use]
    pub(crate) fn format(&self) -> FileFormat {
        self.format
    }

    /// This file's creation time, as reported by the filesystem — `None` if
    /// unsupported on the host OS or filesystem.
    ///
    /// See [`Self::created_at_or_modified`] for a convenience accessor that
    /// falls back to [`Self::modified_at`].
    #[inline]
    #[must_use]
    pub(crate) fn created_at(&self) -> Option<Timestamp> {
        self.created
    }

    /// Returns [`Self::created_at`] if available, falling back to
    /// [`Self::modified_at`] when creation time is unsupported on the host
    /// OS/filesystem.
    #[inline]
    #[must_use]
    pub(crate) fn created_at_or_modified(&self) -> Timestamp {
        self.created.unwrap_or(self.modified_at)
    }

    /// This file's last modification time.
    #[inline]
    #[must_use]
    pub(crate) fn modified_at(&self) -> Timestamp {
        self.modified_at
    }

    /// This file's size in bytes.
    #[inline]
    #[must_use]
    pub(crate) fn size(&self) -> u64 {
        self.size
    }
}

/// Coarse file classification used by the two-tier index.
///
/// Markdown notes get parsed [`Note`](super::note::Note) metadata in addition
/// to their [`FileRecord`]. Other files only keep general file metadata.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub(crate) enum FileFormat {
    /// A markdown Note (`.md` or `.markdown` extension).
    Note,
    /// Any other file.
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

/// Timestamp associated with indexed file metadata.
///
/// Wraps [`DateTime<Utc>`] so file timestamps have one formatting and ordering
/// type instead of leaking storage-library or filesystem clock types.
#[derive(
    Copy, Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Deserialize, Serialize,
)]
pub(crate) struct Timestamp(DateTime<Utc>);

impl Timestamp {
    /// Returns the current UTC timestamp.
    #[inline]
    #[must_use]
    pub(crate) fn now() -> Self {
        Self(Utc::now())
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

    /// Builds a `FileRecord` with `created`/`modified_at` set directly, for
    /// exercising timestamp accessor behavior without touching the
    /// filesystem.
    fn record_with(
        created: Option<Timestamp>,
        modified_at: Timestamp,
    ) -> FileRecord {
        FileRecord {
            path: PathBuf::from("note.md"),
            name: BaseName::from(
                &FileName::try_from(Path::new("note.md"))
                    .expect("valid file name"),
            ),
            folder: PathBuf::new(),
            format: FileFormat::Note,
            created,
            modified_at,
            size: 0,
        }
    }

    mod from_name {
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
            fs::create_dir_all(file.parent().expect("parent")).expect("mkdir");
            fs::write(&file, "content").expect("write file");

            let record = FileRecord::from_metadata(
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
            assert_eq!(record.modified_at().0 <= Utc::now(), true);
        }

        #[test]
        fn returns_an_empty_folder_when_the_file_is_directly_under_root() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let file = temp.path().join("readme.md");
            fs::write(&file, "hi").expect("write file");

            let record = FileRecord::from_metadata(
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
            let reported = Timestamp(modified_at.0 - chrono::Duration::days(1));
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
            let reported = Timestamp(modified_at.0 - chrono::Duration::days(1));
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
