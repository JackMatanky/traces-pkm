//! Query field path parsing and resolution.
//!
//! A query field path string (for example, `file.name`, `task.completed`,
//! `tags`, or a bare frontmatter key) resolves to a [`FieldPath`] variant that
//! can be applied to each [`crate::query::QueryRecord`] to extract a
//! [`NoteFieldValue`].
//!
//! # Supported Accessors
//!
//! - `file.<field>`: [`FileField`] accessors backed by [`FileBase`] metadata
//!   (path, name, folder, size, timestamps).
//! - `task.<field>`: [`TaskField`] accessors valid on task-level records only
//!   (completed, text).
//! - `tags`: Note tags as a list of tag strings.
//! - `inlinks`: Project-relative paths of Notes linking to this Note.
//! - Bare keys: frontmatter or inline metadata field keys.
//!
//! [`NoteFieldValue`]: crate::NoteFieldValue
//! [`FileBase`]: crate::FileBase

use crate::{FieldKey, field, query::error::FieldPathError};

/// A `file.<field>` accessor backed by [`FileBase`] metadata.
///
/// Each variant maps to a specific accessor name (for example, `file.name`,
/// `file.mtime`) and resolves to a [`NoteFieldValue`] by reading the
/// corresponding [`FileBase`] method.
///
/// The full set of accepted accessor names (including aliases like `ctime`
/// for `created_at`) is listed in [`ACCESSOR_NAMES`](Self::ACCESSOR_NAMES).
///
/// [`FileBase`]: crate::FileBase
/// [`NoteFieldValue`]: crate::NoteFieldValue
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum FileField {
    /// Accesses [`crate::FileBase::path`].
    Path,
    /// Accesses [`crate::FileBase::name`].
    Name,
    /// Accesses [`crate::FileBase::folder`].
    Folder,
    /// Accesses [`crate::FileBase::size`].
    Size,
    /// Accesses [`crate::FileBase::created_at_or_modified`] as a
    /// datetime without a UTC offset.
    CreatedDateTime,
    /// Accesses [`crate::FileBase::created_at_or_modified`] as a bare
    /// date.
    CreatedDate,
    /// Accesses [`crate::FileBase::modified_at`] as a datetime without
    /// a UTC offset.
    ModifiedDateTime,
    /// Accesses [`crate::FileBase::modified_at`] as a bare date.
    ModifiedDate,
}

impl FileField {
    /// Lists all `file.<field>` accessor names accepted by [`Self::parse`],
    /// including aliases.
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

    /// Parses the field portion of a `file.<field>` accessor string.
    ///
    /// Returns `None` when `name` is unknown, allowing the caller to retain the
    /// full `file.<field>` path for a [`FieldPathError`].
    ///
    /// [`FieldPathError`]: crate::query::error::FieldPathError
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
}

/// A `task.<field>` accessor valid on task-level records.
///
/// Resolves to [`crate::NoteFieldValue::Null`] on page-level records.
/// Accepted names: `completed`, `text`.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum TaskField {
    /// Accesses task completion state (`- [ ]` versus `- [x]`).
    Completed,
    /// Accesses task item text.
    Text,
}

impl TaskField {
    /// Lists all `task.<field>` accessor names accepted by [`Self::parse`].
    pub(super) const ACCESSOR_NAMES: &'static [&'static str] =
        &["completed", "text"];

    /// Parses the field portion of a `task.<field>` accessor string, returning
    /// `None` for unknown names.
    ///
    /// [`FieldPathError`]: crate::query::error::FieldPathError
    pub(super) fn parse(name: &str) -> Option<Self> {
        match name {
            "completed" => Some(Self::Completed),
            "text" => Some(Self::Text),
            _ => None,
        }
    }
}

/// A resolved query field path.
///
/// A `FieldPath` is parsed once per [`crate::query::QueryRecordSet`]
/// transformation and subsequently applied to each
/// [`crate::query::QueryRecord`] to extract a [`NoteFieldValue`].
///
/// Recognized path forms: `file.<field>`, `task.<field>`, `tags`, `inlinks`,
/// or a bare frontmatter key. Unknown `file.*` or `task.*` accessors produce
/// a [`FieldPathError`] with an optional "did you mean" suggestion.
///
/// [`NoteFieldValue`]: crate::NoteFieldValue
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum FieldPath {
    /// Wraps a `file.<field>` accessor ([`FileField`]).
    File(FileField),
    /// Wraps a `task.<field>` accessor ([`TaskField`]), which resolves to
    /// [`crate::NoteFieldValue::Null`] on page-level records.
    Task(TaskField),
    /// Accesses a frontmatter or inline field key.
    Metadata(String),
    /// Accesses Note tags as a [`crate::NoteFieldValue::List`] of tag
    /// strings.
    Tags,
    /// Accesses project-relative paths of Notes linking to this Note as a
    /// [`crate::NoteFieldValue::List`].
    ///
    /// Derived dynamically by `derive_inlinks` rather than stored directly on
    /// the Note.
    Inlinks,
}

impl FieldPath {
    /// Parses a query field path string into a [`FieldPath`].
    ///
    /// Leading and trailing whitespace is trimmed.
    ///
    /// # Errors
    ///
    /// Returns [`FieldPathError`] if `path` is empty, has invalid `.`
    /// structure (for example, `file.` or `a.b`), or names an unknown
    /// `file.<field>` or `task.<field>` accessor.
    ///
    /// [`FieldPathError`]: crate::query::error::FieldPathError
    pub(crate) fn parse(path: &str) -> Result<Self, FieldPathError> {
        let path = path.trim();
        let invalid = || FieldPathError::new(path, None);
        if let Some(field) = path.strip_prefix("file.") {
            return if field.is_empty() || field.contains('.') {
                Err(invalid())
            } else {
                FileField::parse(field).map(Self::File).ok_or_else(|| {
                    accessor_typo_error(
                        path,
                        "file",
                        FileField::ACCESSOR_NAMES,
                        field,
                    )
                })
            };
        }
        if let Some(field) = path.strip_prefix("task.") {
            return if field.is_empty() || field.contains('.') {
                Err(invalid())
            } else {
                TaskField::parse(field).map(Self::Task).ok_or_else(|| {
                    accessor_typo_error(
                        path,
                        "task",
                        TaskField::ACCESSOR_NAMES,
                        field,
                    )
                })
            };
        }
        if path.is_empty()
            || path == "file"
            || path == "task"
            || path.contains('.')
        {
            return Err(invalid());
        }
        if path == "tags" {
            return Ok(Self::Tags);
        }
        if path == "inlinks" {
            return Ok(Self::Inlinks);
        }
        Ok(Self::Metadata(
            FieldKey::try_from(path)
                .map_err(|_| invalid())?
                .canonical()
                .to_owned(),
        ))
    }
}

/// Constructs a [`FieldPathError`] containing an optional "did you mean"
/// suggestion for a typo in a `file.<field>` or `task.<field>` accessor.
fn accessor_typo_error(
    path: &str,
    prefix: &str,
    candidates: &[&'static str],
    field: &str,
) -> FieldPathError {
    FieldPathError::new(
        path,
        closest_accessor(candidates, field)
            .map(|name| format!("{prefix}.{name}"))
            .as_deref(),
    )
}

/// Finds the accessor name closest to `input` within the edit-distance
/// threshold.
fn closest_accessor(
    candidates: &[&'static str],
    input: &str,
) -> Option<&'static str> {
    field::closest_match(candidates.iter().map(|&name| (name, name)), input)
}

#[cfg(test)]
mod tests {
    use super::*;

    mod file_field {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn names_round_trip_through_parse() {
            for name in FileField::ACCESSOR_NAMES {
                assert!(
                    FileField::parse(name).is_some(),
                    "{name} should parse"
                );
            }
        }

        #[test]
        fn rejects_an_unknown_accessor_name() {
            assert_eq!(FileField::parse("bogus"), None);
        }
    }

    mod task_field {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn parses_completed_and_text() {
            assert_eq!(
                TaskField::parse("completed"),
                Some(TaskField::Completed)
            );
            assert_eq!(TaskField::parse("text"), Some(TaskField::Text));
        }

        #[test]
        fn rejects_an_unknown_accessor_name() {
            assert_eq!(TaskField::parse("bogus"), None);
        }

        #[test]
        fn accessor_names_round_trip_through_parse() {
            for name in TaskField::ACCESSOR_NAMES {
                assert!(
                    TaskField::parse(name).is_some(),
                    "{name} should parse"
                );
            }
        }
    }

    mod accessor_matching {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn closest_accessor_matches_within_the_half_length_threshold() {
            let candidates: &[&str] = &["path", "name", "folder"];
            assert_eq!(closest_accessor(candidates, "nam"), Some("name"));
        }

        #[test]
        fn closest_accessor_rejects_a_match_past_the_threshold() {
            // "na" has threshold ceil(2/2).max(1) = 1, but its distance to
            // "name" is 2 (insert "m", "e"): too far to suggest.
            let candidates: &[&str] = &["name"];
            assert_eq!(closest_accessor(candidates, "na"), None);
        }

        #[test]
        fn closest_accessor_returns_none_for_an_empty_candidate_list() {
            assert_eq!(closest_accessor(&[], "name"), None);
        }

        #[test]
        fn closest_accessor_breaks_ties_by_iteration_order() {
            // Both "cat" and "bat" are distance 1 from "mat"; the first
            // candidate in iteration order wins.
            let candidates: &[&str] = &["cat", "bat"];
            assert_eq!(closest_accessor(candidates, "mat"), Some("cat"));
        }
    }

    mod field_path {
        use pretty_assertions::assert_eq;
        use rstest::rstest;

        use super::*;

        #[test]
        fn parses_a_file_accessor() {
            assert_eq!(
                FieldPath::parse("file.name"),
                Ok(FieldPath::File(FileField::Name))
            );
        }

        #[test]
        fn parses_a_task_accessor() {
            assert_eq!(
                FieldPath::parse("task.completed"),
                Ok(FieldPath::Task(TaskField::Completed))
            );
        }

        #[test]
        fn parses_tags_as_the_tags_variant() {
            assert_eq!(FieldPath::parse("tags"), Ok(FieldPath::Tags));
        }

        #[test]
        fn parses_inlinks_as_the_inlinks_variant() {
            assert_eq!(FieldPath::parse("inlinks"), Ok(FieldPath::Inlinks));
        }

        #[test]
        fn parses_a_bare_key_as_metadata() {
            assert_eq!(
                FieldPath::parse("rating"),
                Ok(FieldPath::Metadata("rating".to_owned()))
            );
        }

        #[rstest]
        #[case::empty("")]
        #[case::bare_file("file")]
        #[case::trailing_dot("file.")]
        #[case::unknown_file_accessor("file.bogus")]
        #[case::extra_file_segment("file.name.extra")]
        #[case::bare_task("task")]
        #[case::trailing_dot_task("task.")]
        #[case::unknown_task_accessor("task.bogus")]
        #[case::extra_task_segment("task.completed.extra")]
        #[case::dotted_metadata_path("a.b")]
        #[case::canonical_empty_bare_key("!!!")]
        fn rejects_malformed_paths(#[case] path: &str) {
            assert_eq!(
                FieldPath::parse(path),
                Err(FieldPathError::new(path, None))
            );
        }

        #[test]
        fn suggests_the_closest_file_accessor_for_a_typo() {
            assert_eq!(
                FieldPath::parse("file.nam"),
                Err(FieldPathError::new("file.nam", Some("file.name")))
            );
        }

        #[test]
        fn suggests_the_closest_task_accessor_for_a_typo() {
            assert_eq!(
                FieldPath::parse("task.complete"),
                Err(FieldPathError::new(
                    "task.complete",
                    Some("task.completed")
                ))
            );
        }

        #[test]
        fn no_suggestion_for_an_unrelated_unknown_accessor() {
            assert_eq!(
                FieldPath::parse("file.bogus"),
                Err(FieldPathError::new("file.bogus", None))
            );
        }
    }
}
