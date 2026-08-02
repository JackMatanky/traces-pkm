//! `file.*` accessor parsing and resolution for query field paths.

use super::super::file::FileRecord;
use crate::note::FieldValue;

/// Query `file.*` accessors backed by [`FileRecord`] metadata.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum FileField {
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
    /// `file.<field>` accessor names [`Self::parse`] accepts, including
    /// every alias.
    pub(crate) const ACCESSOR_NAMES: &'static [&'static str] = &[
        "path",
        "name",
        "folder",
        "size",
        "created_at",
        "ctime",
        "cdate",
        "modified_at",
        "mtime",
        "mdate",
    ];

    /// Parses a `file.<field>` accessor name.
    ///
    /// Returns `None` when `name` is unknown. Callers build
    /// [`super::QueryError::UnknownFieldPath`] themselves because they still
    /// have the full `file.<field>` path.
    pub(crate) fn parse(name: &str) -> Option<Self> {
        match name {
            "path" => Some(Self::Path),
            "name" => Some(Self::Name),
            "folder" => Some(Self::Folder),
            "size" => Some(Self::Size),
            "created_at" | "ctime" => Some(Self::CreatedDateTime),
            "cdate" => Some(Self::CreatedDate),
            "modified_at" | "mtime" => Some(Self::ModifiedDateTime),
            "mdate" => Some(Self::ModifiedDate),
            _ => None,
        }
    }

    /// Returns this accessor's value for `file`.
    pub(crate) fn resolve(self, file: &FileRecord) -> FieldValue {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_round_trip_through_parse() {
        for name in FileField::ACCESSOR_NAMES {
            assert!(FileField::parse(name).is_some(), "{name} should parse");
        }
    }
}
