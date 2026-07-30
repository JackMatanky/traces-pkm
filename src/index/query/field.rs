//! Field path parsing and file metadata accessor resolution.

use super::{super::file::FileRecord, QueryError};
use crate::note::FieldValue;

/// A query field path, resolved once per [`super::QueryOutcome`]
/// transformation and then applied to every [`super::IndexRecord`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum FieldPath {
    /// A `file.<field>` accessor.
    File(FileField),
    /// A frontmatter or inline field, looked up by key.
    Metadata(String),
    /// The Note's markdown tags, as a [`FieldValue::List`] of tag strings.
    Tags,
}

impl FieldPath {
    /// Parses a query field path string into a [`FieldPath`].
    ///
    /// Resolves `file.<field>` accessors, `tags`, or frontmatter/inline field
    /// keys.
    ///
    /// # Errors
    ///
    /// Returns [`QueryError::UnknownFieldPath`] if `path` is empty, uses an
    /// unknown `file.*` accessor, or has unexpected `.` structure.
    pub(super) fn parse(path: &str) -> Result<Self, QueryError> {
        let path = path.trim();
        let invalid = || QueryError::UnknownFieldPath {
            path: path.to_owned(),
        };
        if let Some(field) = path.strip_prefix("file.") {
            return if field.is_empty() || field.contains('.') {
                Err(invalid())
            } else {
                FileField::parse(field).map(Self::File)
            };
        }
        if path.is_empty() || path == "file" || path.contains('.') {
            return Err(invalid());
        }
        if path == "tags" {
            return Ok(Self::Tags);
        }
        Ok(Self::Metadata(path.to_owned()))
    }
}

/// General `file.*` metadata accessors available to query field paths.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(super) enum FileField {
    /// [`FileRecord::path`].
    Path,
    /// [`FileRecord::name`].
    Name,
    /// [`FileRecord::folder`].
    Folder,
    /// [`FileRecord::size`].
    Size,
    /// [`FileRecord::created_at_or_modified`], as a datetime with no UTC
    /// offset.
    CreatedDateTime,
    /// [`FileRecord::created_at_or_modified`], as a bare date.
    CreatedDate,
    /// [`FileRecord::modified_at`], as a datetime with no UTC offset.
    ModifiedDateTime,
    /// [`FileRecord::modified_at`], as a bare date.
    ModifiedDate,
}

impl FileField {
    /// Parses a `file.<field>` accessor name (the part after `"file."`).
    ///
    /// # Errors
    ///
    /// Returns [`QueryError::UnknownFieldPath`] if `name` is not a known
    /// accessor.
    pub(super) fn parse(name: &str) -> Result<Self, QueryError> {
        match name {
            "path" => Ok(Self::Path),
            "name" => Ok(Self::Name),
            "folder" => Ok(Self::Folder),
            "size" => Ok(Self::Size),
            "created_at" | "ctime" => Ok(Self::CreatedDateTime),
            "cdate" => Ok(Self::CreatedDate),
            "modified_at" | "mtime" => Ok(Self::ModifiedDateTime),
            "mdate" => Ok(Self::ModifiedDate),
            _ => Err(QueryError::UnknownFieldPath {
                path: format!("file.{name}"),
            }),
        }
    }

    /// Resolves this accessor against `file`.
    pub(super) fn resolve(self, file: &FileRecord) -> FieldValue {
        match self {
            Self::Path => {
                FieldValue::String(file.path().to_string_lossy().into_owned())
            }
            Self::Name => FieldValue::String(file.name().as_str().to_owned()),
            Self::Folder => {
                FieldValue::String(file.folder().to_string_lossy().into_owned())
            }
            #[expect(
                clippy::as_conversions,
                clippy::cast_precision_loss,
                reason = "file sizes stay well under 2^53 bytes for PKM-scale \
                          projects, so f64 keeps exact byte counts"
            )]
            Self::Size => FieldValue::Number(file.size() as f64),
            Self::CreatedDateTime => FieldValue::Date(
                file.created_at_or_modified().to_datetime_string(),
            ),
            Self::CreatedDate => {
                FieldValue::Date(file.created_at_or_modified().to_date_string())
            }
            Self::ModifiedDateTime => {
                FieldValue::Date(file.modified_at().to_datetime_string())
            }
            Self::ModifiedDate => {
                FieldValue::Date(file.modified_at().to_date_string())
            }
        }
    }
}
