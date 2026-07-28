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

/// General metadata indexed for every file under a project root, regardless
/// of type.
///
/// `path` and `folder` are project-root-relative. `created_at` falls back to
/// `modified_at` on filesystems that don't report a creation time (e.g. some
/// Linux filesystems without `statx` support).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct FileRecord {
    path: PathBuf,
    name: String,
    folder: PathBuf,
    kind: FileKind,
    created_at: DateTime<Utc>,
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
        let created_at =
            metadata.created().map(system_time_to_utc).unwrap_or(modified_at);
        let name = relative
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_default();
        let folder =
            relative.parent().unwrap_or_else(|| Path::new("")).to_path_buf();
        let kind = match relative.extension().and_then(|ext| ext.to_str()) {
            Some(ext) if ext.eq_ignore_ascii_case("md") => FileKind::Note,
            Some(ext) if ext.eq_ignore_ascii_case("markdown") => FileKind::Note,
            _ => FileKind::Other,
        };

        Ok(Self {
            path: relative,
            name,
            folder,
            kind,
            created_at,
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

    /// The file's creation time, or its modification time where the
    /// filesystem doesn't report creation time.
    #[inline]
    #[must_use]
    pub(crate) fn created_at(&self) -> DateTime<Utc> {
        self.created_at
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
    use pretty_assertions::assert_eq;

    use super::*;

    fn metadata_for(path: &Path) -> fs::Metadata {
        fs::metadata(path).expect("read metadata")
    }

    #[test]
    fn splits_name_from_extension() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let file = temp.path().join("notes").join("todo.md");
        fs::create_dir_all(file.parent().expect("parent")).expect("mkdir");
        fs::write(&file, "content").expect("write file");

        let record =
            FileRecord::from_metadata(&file, temp.path(), &metadata_for(&file))
                .expect("build record");

        assert_eq!(record.name(), "todo");
        assert_eq!(record.path(), Path::new("notes/todo.md"));
        assert_eq!(record.folder(), Path::new("notes"));
        assert_eq!(record.size(), 7);
    }

    #[test]
    fn file_directly_under_root_has_an_empty_folder() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let file = temp.path().join("readme.md");
        fs::write(&file, "hi").expect("write file");

        let record =
            FileRecord::from_metadata(&file, temp.path(), &metadata_for(&file))
                .expect("build record");

        assert_eq!(record.folder(), Path::new(""));
    }

    #[test]
    fn file_with_no_extension_keeps_its_full_name() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let file = temp.path().join("LICENSE");
        fs::write(&file, "mit").expect("write file");

        let record =
            FileRecord::from_metadata(&file, temp.path(), &metadata_for(&file))
                .expect("build record");

        assert_eq!(record.name(), "LICENSE");
    }

    #[test]
    fn modified_at_matches_the_filesystem_timestamp() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let file = temp.path().join("note.md");
        fs::write(&file, "content").expect("write file");
        let metadata = metadata_for(&file);
        let expected: DateTime<Utc> = metadata
            .modified()
            .expect("filesystem reports modified time")
            .into();

        let record = FileRecord::from_metadata(&file, temp.path(), &metadata)
            .expect("build record");

        assert_eq!(record.modified_at(), expected);
    }

    #[test]
    fn created_at_falls_back_to_modified_at_when_unsupported() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let file = temp.path().join("note.md");
        fs::write(&file, "content").expect("write file");
        let metadata = metadata_for(&file);

        let record = FileRecord::from_metadata(&file, temp.path(), &metadata)
            .expect("build record");

        // Whether or not this filesystem reports a creation time,
        // `created_at` is always populated — either with the real value or
        // the `modified_at` fallback — never left unset.
        if metadata.created().is_err() {
            assert_eq!(record.created_at(), record.modified_at());
        }
    }

    #[test]
    fn markdown_extensions_are_classified_as_notes() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let md = temp.path().join("note.md");
        let markdown = temp.path().join("note.MARKDOWN");
        fs::write(&md, "content").expect("write .md file");
        fs::write(&markdown, "content").expect("write .MARKDOWN file");

        let md_record =
            FileRecord::from_metadata(&md, temp.path(), &metadata_for(&md))
                .expect("build .md record");
        let markdown_record = FileRecord::from_metadata(
            &markdown,
            temp.path(),
            &metadata_for(&markdown),
        )
        .expect("build .MARKDOWN record");

        assert_eq!(md_record.kind(), FileKind::Note);
        assert_eq!(markdown_record.kind(), FileKind::Note);
    }

    #[test]
    fn non_markdown_files_are_classified_as_other() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let file = temp.path().join("config.toml");
        fs::write(&file, "content").expect("write file");

        let record =
            FileRecord::from_metadata(&file, temp.path(), &metadata_for(&file))
                .expect("build record");

        assert_eq!(record.kind(), FileKind::Other);
    }
}
