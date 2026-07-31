//! Field path parsing and field metadata resolution.

use super::{super::file::FileField, QueryError};

/// A query field path, resolved once per [`super::QueryOutcome`]
/// transformation and then applied to every [`super::IndexRecord`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum FieldPath {
    /// A `file.<field>` accessor.
    File(FileField),
    /// A frontmatter or inline field, looked up by key.
    Metadata(String),
    /// The Note's markdown tags, as a
    /// [`FieldValue::List`](crate::note::FieldValue::List) of tag strings.
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

#[cfg(test)]
mod tests {
    use std::{fs, path::Path};

    use pretty_assertions::assert_eq;
    use rstest::rstest;

    use super::super::*;
    use crate::index::FileIndex;

    fn outcome_for_files(temp: &Path, files: &[(&str, &str)]) -> QueryOutcome {
        for (name, content) in files {
            fs::write(temp.join(name), content).expect("write note");
        }
        FileIndex::build(temp).expect("build index").query(&Source::All)
    }

    fn outcome_for(temp: &Path, content: &str) -> QueryOutcome {
        outcome_for_files(temp, &[("note.md", content)])
    }

    #[test]
    fn resolves_file_path_name_folder_and_size() {
        let temp = tempfile::tempdir().expect("create temp dir");
        fs::create_dir_all(temp.path().join("notes")).expect("mkdir");
        let outcome =
            outcome_for_files(temp.path(), &[("notes/todo.md", "body")]);
        let record = outcome.get(0).expect("record");

        assert_eq!(
            record.field("file.path"),
            Ok(FieldValue::String("notes/todo.md".to_owned()))
        );
        assert_eq!(
            record.field("file.name"),
            Ok(FieldValue::String("todo".to_owned()))
        );
        assert_eq!(
            record.field("file.folder"),
            Ok(FieldValue::String("notes".to_owned()))
        );
        assert_eq!(record.field("file.size"), Ok(FieldValue::Number(4.0)));
    }

    #[test]
    fn resolves_dataview_style_time_accessors_from_file_record() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let outcome = outcome_for(temp.path(), "body");
        let record = outcome.get(0).expect("record");
        let file = record.file();

        assert_eq!(
            record.field("file.mtime"),
            Ok(FieldValue::Date(file.modified_at().to_datetime_string()))
        );
        assert_eq!(
            record.field("file.mdate"),
            Ok(FieldValue::Date(file.modified_at().to_date_string()))
        );
        assert_eq!(
            record.field("file.ctime"),
            Ok(FieldValue::Date(
                file.created_at_or_modified().to_datetime_string()
            ))
        );
        assert_eq!(
            record.field("file.cdate"),
            Ok(FieldValue::Date(
                file.created_at_or_modified().to_date_string()
            ))
        );
        assert_eq!(record.field("file.created_at"), record.field("file.ctime"));
        assert_eq!(
            record.field("file.modified_at"),
            record.field("file.mtime")
        );
    }

    #[test]
    fn resolves_frontmatter_and_inline_fields_by_key() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let outcome =
            outcome_for(temp.path(), "---\nrating: 5\n---\nStatus:: Draft");
        let record = outcome.get(0).expect("record");

        assert_eq!(record.field("rating"), Ok(FieldValue::Number(5.0)));
        assert_eq!(
            record.field("Status"),
            Ok(FieldValue::String("Draft".to_owned()))
        );
    }

    #[test]
    fn frontmatter_field_takes_precedence_over_same_key_inline_field() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let outcome = outcome_for(
            temp.path(),
            "---\nstatus: Approved\n---\nstatus:: Draft",
        );
        let record = outcome.get(0).expect("record");

        assert_eq!(
            record.field("status"),
            Ok(FieldValue::String("Approved".to_owned()))
        );
    }

    #[test]
    fn resolves_tags_as_a_list_of_tag_strings() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let outcome = outcome_for(temp.path(), "Filed under #book #read");
        let record = outcome.get(0).expect("record");

        assert_eq!(
            record.field("tags"),
            Ok(FieldValue::List(vec![
                FieldValue::String("#book".to_owned()),
                FieldValue::String("#read".to_owned()),
            ]))
        );
    }

    #[test]
    fn missing_field_resolves_to_null() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let outcome = outcome_for(temp.path(), "body, no frontmatter");
        let record = outcome.get(0).expect("record");

        assert_eq!(record.field("no_such_field"), Ok(FieldValue::Null));
    }

    #[rstest]
    #[case::empty("")]
    #[case::bare_file("file")]
    #[case::trailing_dot("file.")]
    #[case::unknown_file_accessor("file.bogus")]
    #[case::extra_file_segment("file.name.extra")]
    #[case::dotted_metadata_path("a.b")]
    fn rejects_malformed_field_paths(#[case] path: &str) {
        let temp = tempfile::tempdir().expect("create temp dir");
        let outcome = outcome_for(temp.path(), "body");
        let record = outcome.get(0).expect("record");

        assert_eq!(
            record.field(path),
            Err(QueryError::UnknownFieldPath {
                path: path.to_owned()
            })
        );
    }
}
