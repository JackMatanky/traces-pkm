//! [`FileRecord`]: the general metadata indexed for every file.

use std::{
    fs,
    path::{Path, PathBuf},
    time::SystemTime,
};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::error::FileIndexError;

/// Coarse classification of a [`FileRecord`] — whether Traces treats it as
/// a markdown Note (eligible for future Note Metadata extraction; see the
/// spec's two-tier File Record / Note Metadata model) or a plain file.
///
/// Named `kind` to match the `file_records` schema in ADR-0005.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum FileKind {
    /// A markdown Note (`.md` or `.markdown` extension).
    Note,
    /// Any other file.
    Other,
}

impl FileKind {
    /// Classifies `path` by its extension: `.md`/`.markdown`
    /// (case-insensitive) is a Note, everything else is `Other`.
    fn from_path(path: &Path) -> Self {
        match path.extension().and_then(|ext| ext.to_str()) {
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

/// A File Record's creation time, as reported by the filesystem.
///
/// `None` on filesystems that don't report a creation time at all (e.g.
/// some Linux filesystems without `statx` support) — see
/// [`FileRecord::created_at`] for the convenience accessor most callers
/// want, which falls back to `modified_at` in that case. Keeping the two
/// apart means a future caller that cares about provenance (e.g. a "sort
/// by creation time" query) can tell a real value from the fallback
/// instead of silently treating a modification time as a creation time.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CreatedAt(Option<DateTime<Utc>>);

impl CreatedAt {
    /// The filesystem-reported creation time, or `None` if this filesystem
    /// doesn't support reporting one.
    #[inline]
    #[must_use]
    pub(crate) fn reported(&self) -> Option<DateTime<Utc>> {
        self.0
    }
}

/// General metadata indexed for every file under a project root, regardless
/// of type.
///
/// `path` and `folder` are project-root-relative.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct FileRecord {
    path: PathBuf,
    name: String,
    folder: PathBuf,
    kind: FileKind,
    created: CreatedAt,
    modified_at: DateTime<Utc>,
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
            metadata.modified().map(system_time_to_utc).map_err(|source| {
                FileIndexError::Io {
                    path: path.to_path_buf(),
                    source,
                }
            })?;
        let created =
            CreatedAt(metadata.created().map(system_time_to_utc).ok());
        let name = relative
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_default();
        let folder =
            relative.parent().unwrap_or_else(|| Path::new("")).to_path_buf();
        let kind = FileKind::from_path(&relative);

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
    pub(crate) fn name(&self) -> &str {
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

    /// This file's creation time, as reported by the filesystem - `None` if
    /// unsupported. See [`Self::created_at`] for the always-populated
    /// convenience accessor.
    #[inline]
    #[must_use]
    pub(crate) fn created(&self) -> CreatedAt {
        self.created
    }

    /// The file's creation time, or its modification time where the
    /// filesystem doesn't report a creation time. Use [`Self::created`] to
    /// tell the two cases apart.
    #[inline]
    #[must_use]
    pub(crate) fn created_at(&self) -> DateTime<Utc> {
        self.created.reported().unwrap_or(self.modified_at)
    }

    /// The file's last modification time.
    #[inline]
    #[must_use]
    pub(crate) fn modified_at(&self) -> DateTime<Utc> {
        self.modified_at
    }

    /// The file's size in bytes.
    #[inline]
    #[must_use]
    pub(crate) fn size(&self) -> u64 {
        self.size
    }
}

/// Converts a filesystem timestamp to UTC, discarding sub-timezone precision
/// concerns — [`SystemTime`] carries no timezone, so this is a lossless
/// reinterpretation, not a conversion.
fn system_time_to_utc(time: SystemTime) -> DateTime<Utc> {
    DateTime::<Utc>::from(time)
}

#[cfg(test)]
mod tests {
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
            fs::create_dir_all(file.parent().expect("parent")).expect("mkdir");
            fs::write(&file, "content").expect("write file");

            let record = FileRecord::from_metadata(
                &file,
                temp.path(),
                &metadata_for(&file),
            )
            .expect("build record");

            assert_eq!(record.name(), "todo");
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
            let expected: DateTime<Utc> = metadata
                .modified()
                .expect("filesystem reports modified time")
                .into();

            let record =
                FileRecord::from_metadata(&file, temp.path(), &metadata)
                    .expect("build record");

            assert_eq!(record.modified_at(), expected);
        }

        #[test]
        fn always_populates_created_at() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let file = temp.path().join("note.md");
            fs::write(&file, "content").expect("write file");

            let record = FileRecord::from_metadata(
                &file,
                temp.path(),
                &metadata_for(&file),
            )
            .expect("build record");

            // Whichever branch is taken - the real filesystem value or the
            // `modified_at` fallback - the field is populated, never a
            // sentinel/default. The fallback branch itself is covered
            // deterministically by `created_at`'s own tests below, since
            // whether this filesystem reports a creation time isn't
            // something a test controls.
            assert!(
                record.created_at() <= Utc::now(),
                "expected a populated, non-future created_at, got {:?}",
                record.created_at()
            );
        }
    }

    mod from_path {
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
            assert_eq!(FileKind::from_path(Path::new(file_name)), expected);
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
            created: CreatedAt,
            modified_at: DateTime<Utc>,
        ) -> FileRecord {
            FileRecord {
                path: PathBuf::from("note.md"),
                name: "note".to_owned(),
                folder: PathBuf::new(),
                kind: FileKind::Note,
                created,
                modified_at,
                size: 0,
            }
        }

        #[test]
        fn returns_the_reported_value_when_the_filesystem_supports_it() {
            let modified_at = Utc::now();
            let reported = modified_at - chrono::Duration::days(1);
            let record = record_with(CreatedAt(Some(reported)), modified_at);

            assert_eq!(record.created_at(), reported);
            assert_eq!(record.created().reported(), Some(reported));
        }

        #[test]
        fn falls_back_to_modified_at_when_unsupported() {
            let modified_at = Utc::now();
            let record = record_with(CreatedAt(None), modified_at);

            assert_eq!(record.created_at(), modified_at);
            assert_eq!(record.created().reported(), None);
        }
    }
}
