//! [`FileRecord`]: the general metadata indexed for every file.

use std::{
    fs,
    path::{Path, PathBuf},
    time::SystemTime,
};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::error::FileIndexError;
use crate::file_name::{BaseName, FileName};

/// Coarse classification of a [`FileRecord`] — whether Traces treats it as a
/// markdown Note (eligible for future Note Metadata extraction; see the spec's
/// two-tier File Record / Note Metadata model) or a plain file.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum FileKind {
    /// A markdown Note (`.md` or `.markdown` extension).
    Note,
    /// Any other file.
    Other,
}

impl FileKind {
    /// Classifies a file by its name's extension: `.md`/`.markdown`
    /// (case-insensitive) is a Note, everything else is `Other`.
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

/// A single point in time associated with a file: its creation or
/// modification time.
///
/// Wraps [`DateTime<Utc>`] to avoid colliding with `std::time::Instant` or
/// `std::fs::FileTimes`, and gives file timestamps one shared place for
/// formatting/comparison behavior as later tickets need it.
#[derive(
    Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize,
)]
pub(crate) struct Timestamp(DateTime<Utc>);

impl Timestamp {
    /// The current time.
    #[must_use]
    pub(crate) fn now() -> Self {
        Self(Utc::now())
    }
}

impl From<SystemTime> for Timestamp {
    /// Converts a filesystem timestamp to UTC, discarding sub-timezone
    /// precision concerns — [`SystemTime`] carries no timezone, so this is a
    /// lossless reinterpretation, not a conversion.
    fn from(time: SystemTime) -> Self {
        Self(DateTime::<Utc>::from(time))
    }
}

/// General metadata indexed for every file under a project root, regardless
/// of type.
///
/// `path` and `folder` are project-root-relative.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct FileRecord {
    path: PathBuf,
    name: BaseName,
    folder: PathBuf,
    kind: FileKind,
    created: Option<Timestamp>,
    modified_at: Timestamp,
    size: u64,
}

impl FileRecord {
    /// Builds a File Record for `path` (absolute, under `root`) from its
    /// filesystem `metadata`.
    ///
    /// # Errors
    ///
    /// Returns [`FileIndexError::Io`] if `metadata`'s modification time
    /// cannot be read.
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
        let kind = FileKind::from_name(&file_name);
        let folder =
            relative.parent().unwrap_or_else(|| Path::new("")).to_path_buf();

        Ok(Self {
            path: relative,
            name,
            folder,
            kind,
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

    /// Whether this file is a markdown Note or a plain file.
    #[inline]
    #[must_use]
    pub(crate) fn kind(&self) -> FileKind {
        self.kind
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

    /// The file's creation time, falling back to its modification time where
    /// the filesystem doesn't report a creation time.
    #[inline]
    #[must_use]
    pub(crate) fn created_at_or_modified(&self) -> Timestamp {
        self.created.unwrap_or(self.modified_at)
    }

    /// The file's last modification time.
    #[inline]
    #[must_use]
    pub(crate) fn modified_at(&self) -> Timestamp {
        self.modified_at
    }

    /// The file's size in bytes.
    #[inline]
    #[must_use]
    pub(crate) fn size(&self) -> u64 {
        self.size
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    mod from_name {
        use pretty_assertions::assert_eq;
        use rstest::rstest;

        use super::*;

        #[rstest]
        #[case::lowercase_md_extension("note.md", FileKind::Note)]
        #[case::mixed_case_markdown_extension("note.MARKDOWN", FileKind::Note)]
        #[case::non_markdown_extension("config.toml", FileKind::Other)]
        #[case::no_extension("LICENSE", FileKind::Other)]
        fn classifies_by_extension(
            #[case] file_name: &str,
            #[case] expected: FileKind,
        ) {
            let name = FileName::try_from(Path::new(file_name))
                .expect("valid file name");

            assert_eq!(FileKind::from_name(&name), expected);
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
            assert_eq!(record.kind(), FileKind::Note);
            assert_eq!(record.size(), 7);
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

            assert_eq!(record.folder(), Path::new(""));
        }

        #[test]
        fn keeps_the_absolute_path_when_the_file_is_outside_project_root() {
            let root = tempfile::tempdir().expect("create root dir");
            let outside = tempfile::tempdir().expect("create outside dir");
            let file = outside.path().join("stray.md");
            fs::write(&file, "content").expect("write file");

            // `path` isn't under `root`, so `strip_prefix` fails and the
            // record falls back to keeping the full path as given — this
            // never happens through `scan_root`, which only ever passes
            // descendants of `root`, but the fallback is a real branch.
            let record = FileRecord::from_metadata(
                &file,
                root.path(),
                &metadata_for(&file),
            )
            .expect("build record");

            assert_eq!(record.path(), file);
        }

        #[test]
        fn matches_the_filesystem_modified_timestamp() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let file = temp.path().join("note.md");
            fs::write(&file, "content").expect("write file");
            let metadata = metadata_for(&file);
            let expected = Timestamp::from(
                metadata.modified().expect("filesystem reports modified time"),
            );

            let record =
                FileRecord::from_metadata(&file, temp.path(), &metadata)
                    .expect("build record");

            assert_eq!(record.modified_at(), expected);
        }

        #[test]
        fn created_at_or_modified_is_non_future() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let file = temp.path().join("note.md");
            fs::write(&file, "content").expect("write file");

            let record = FileRecord::from_metadata(
                &file,
                temp.path(),
                &metadata_for(&file),
            )
            .expect("build record");

            assert!(
                record.created_at_or_modified() <= Timestamp::now(),
                "expected non-future created_at_or_modified, got {:?}",
                record.created_at_or_modified()
            );
        }
    }

    mod created_at {
        use pretty_assertions::assert_eq;

        use super::*;

        /// A minimal `FileRecord`, built via struct literal (private-field
        /// access is available here, same module tree) so `created_at`'s
        /// fallback logic can be tested deterministically - whether the
        /// real dev/CI filesystem reports a creation time isn't something
        /// a test controls.
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
                kind: FileKind::Note,
                created,
                modified_at,
                size: 0,
            }
        }

        #[test]
        fn created_at_returns_raw_option() {
            let modified_at = Timestamp::now();
            let reported = Timestamp(modified_at.0 - chrono::Duration::days(1));
            let record = record_with(Some(reported), modified_at);

            assert_eq!(record.created_at(), Some(reported));

            let unsupported = record_with(None, modified_at);
            assert_eq!(unsupported.created_at(), None);
        }

        #[test]
        fn created_at_or_modified_returns_reported_value_when_available() {
            let modified_at = Timestamp::now();
            let reported = Timestamp(modified_at.0 - chrono::Duration::days(1));
            let record = record_with(Some(reported), modified_at);

            assert_eq!(record.created_at_or_modified(), reported);
        }

        #[test]
        fn created_at_or_modified_falls_back_to_modified_at_when_unsupported() {
            let modified_at = Timestamp::now();
            let record = record_with(None, modified_at);

            assert_eq!(record.created_at_or_modified(), modified_at);
        }
    }
}
