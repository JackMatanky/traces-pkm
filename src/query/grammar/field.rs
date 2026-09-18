//! Query field path parsing and resolution.
//!
//! Field paths such as `file.name`, `list.completed`, `tags`, `inlinks`, or
//! bare metadata keys resolve to a [`FieldPath`] that extracts a
//! [`NoteFieldValue`] from each [`crate::query::QueryRow`].
//!
//! # Supported accessors
//!
//! - `file.<field>`: [`FileField`] accessors backed by [`FileBase`] metadata.
//! - `list.<field>`: [`ListField`] accessors valid on list rows.
//! - `tags`: Note tags.
//! - `inlinks`: Project-relative paths of Notes linking to this Note.
//! - Bare keys: frontmatter or inline metadata field keys.
//!
//! [`NoteFieldValue`]: crate::NoteFieldValue
//! [`FileBase`]: crate::FileBase

use crate::{FieldKey, query::error::FieldPathError, strsim::closest_match};

/// A `file.<field>` accessor backed by [`FileBase`] metadata.
///
/// Accepted accessor names, including aliases such as `ctime` for
/// `created_at`, are listed in [`ACCESSOR_NAMES`](Self::ACCESSOR_NAMES).
///
/// [`FileBase`]: crate::FileBase
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum FileField {
    Path,
    Name,
    Folder,
    Size,
    /// Accesses [`crate::FileBase::created_at`] (falling back to
    /// [`crate::FileBase::modified_at`]) as a datetime without a UTC offset.
    CreatedDateTime,
    /// Accesses [`crate::FileBase::created_at`] (falling back to
    /// [`crate::FileBase::modified_at`]) as a bare date.
    CreatedDate,
    /// Accesses [`crate::FileBase::modified_at`] as a datetime without a UTC
    /// offset.
    ModifiedDateTime,
    /// Accesses [`crate::FileBase::modified_at`] as a bare date.
    ModifiedDate,
    /// Accesses note-level tags from the row's file.
    Tags,
}

impl FileField {
    /// Accepted `file.<field>` accessor names, including aliases.
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
        "tags",
    ];

    /// Parses the field portion of a `file.<field>` accessor string.
    ///
    /// Returns `None` for unknown names so callers can report the full
    /// `file.<field>` path.
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
            "tags" => Some(Self::Tags),
            _ => None,
        }
    }
}

/// A task-specific list field.
///
/// These fields resolve to [`crate::NoteFieldValue::Null`] on plain bullets and
/// non-task checkboxes.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum TaskField {
    Status,
    StatusType,
    StatusSymbol,
    Completed,
    Priority,
    Due,
    Done,
    Created,
    Start,
    Scheduled,
    Cancelled,
    FullyComplete,
}

impl TaskField {
    /// Accepted task-specific `list.<field>` accessor names.
    #[cfg(test)]
    pub(super) const ACCESSOR_NAMES: &'static [&'static str] = &[
        "status",
        "status_type",
        "status_symbol",
        "completed",
        "priority",
        "due",
        "done",
        "created",
        "start",
        "scheduled",
        "cancelled",
        "fully_complete",
    ];

    /// Parses a task-specific list field name.
    pub(super) fn parse(name: &str) -> Option<Self> {
        match name {
            "status" => Some(Self::Status),
            "status_type" => Some(Self::StatusType),
            "status_symbol" => Some(Self::StatusSymbol),
            "completed" => Some(Self::Completed),
            "priority" => Some(Self::Priority),
            "due" => Some(Self::Due),
            "done" => Some(Self::Done),
            "created" => Some(Self::Created),
            "start" => Some(Self::Start),
            "scheduled" => Some(Self::Scheduled),
            "cancelled" => Some(Self::Cancelled),
            "fully_complete" => Some(Self::FullyComplete),
            _ => None,
        }
    }
}

/// A universal or task-specific `list.<field>` accessor.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum ListField {
    Text,
    RawText,
    Line,
    Parent,
    Depth,
    Tags,
    IsTask,
    Kind,
    IsOrdered,
    Task(TaskField),
}

impl ListField {
    /// Accepted `list.<field>` accessor names.
    pub(crate) const ACCESSOR_NAMES: &'static [&'static str] = &[
        "text",
        "raw_text",
        "line",
        "parent",
        "depth",
        "tags",
        "is_task",
        "kind",
        "is_ordered",
        "status",
        "status_type",
        "status_symbol",
        "completed",
        "priority",
        "due",
        "done",
        "created",
        "start",
        "scheduled",
        "cancelled",
        "fully_complete",
    ];

    /// Parses the field portion of a canonical `list.<field>` accessor.
    pub(crate) fn parse(name: &str) -> Option<Self> {
        match name {
            "text" => Some(Self::Text),
            "raw_text" => Some(Self::RawText),
            "line" => Some(Self::Line),
            "parent" => Some(Self::Parent),
            "depth" => Some(Self::Depth),
            "tags" => Some(Self::Tags),
            "is_task" => Some(Self::IsTask),
            "kind" => Some(Self::Kind),
            "is_ordered" => Some(Self::IsOrdered),
            _ => TaskField::parse(name).map(Self::Task),
        }
    }
}

/// A resolved query field path.
///
/// Parsed once per [`crate::query::QuerySet`] transformation, then applied to
/// each [`crate::query::QueryRow`] to extract a [`NoteFieldValue`].
///
/// Unknown `file.*` or `list.*` accessors produce a [`FieldPathError`] with an
/// optional suggestion.
///
/// [`NoteFieldValue`]: crate::NoteFieldValue
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum FieldPath {
    File(FileField),
    /// Resolves to [`crate::NoteFieldValue::Null`] on page-level records.
    List(ListField),
    /// Frontmatter or inline field key.
    Metadata(String),
    Tags,
    /// Project-relative paths of Notes linking to this Note.
    ///
    /// Derived dynamically by `InlinkMap` rather than stored on the Note.
    Inlinks,
}

impl FieldPath {
    /// Parses a query field path string into a [`FieldPath`].
    ///
    /// Leading and trailing whitespace is trimmed.
    ///
    /// # Errors
    ///
    /// - [`FieldPathError`] if `path` is empty or contains an invalid
    ///   identifier.
    /// - [`FieldPathError`] if `path` has invalid `.` structure (for example,
    ///   `file.`, `a.b`, or more than two segments).
    /// - [`FieldPathError`] if `path` uses obsolete `task.<field>` syntax.
    /// - [`FieldPathError`] if `path` names an unknown `file.<field>` or
    ///   `list.<field>` accessor.
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
                    Self::accessor_typo_error(
                        path,
                        "file",
                        FileField::ACCESSOR_NAMES,
                        field,
                    )
                })
            };
        }
        if let Some(field) = path.strip_prefix("list.") {
            return if field.is_empty() || field.contains('.') {
                Err(invalid())
            } else {
                ListField::parse(field).map(Self::List).ok_or_else(|| {
                    Self::accessor_typo_error(
                        path,
                        "list",
                        ListField::ACCESSOR_NAMES,
                        field,
                    )
                })
            };
        }
        if let Some(field) = path.strip_prefix("task.") {
            return if field.is_empty() || field.contains('.') {
                Err(invalid())
            } else {
                Err(FieldPathError::new(path, Some(&format!("list.{field}"))))
            };
        }
        if path.is_empty()
            || path == "file"
            || path == "task"
            || path == "list"
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

    fn accessor_typo_error(
        path: &str,
        prefix: &str,
        candidates: &[&'static str],
        field: &str,
    ) -> FieldPathError {
        FieldPathError::new(
            path,
            Self::closest_accessor(candidates, field)
                .map(|name| format!("{prefix}.{name}"))
                .as_deref(),
        )
    }

    /// Finds the closest accessor name within the edit-distance threshold.
    fn closest_accessor(
        candidates: &[&'static str],
        input: &str,
    ) -> Option<&'static str> {
        closest_match(candidates.iter().map(|&name| (name, name)), input)
    }
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
        use rstest::rstest;

        use super::*;

        #[rstest]
        #[case::status("status", TaskField::Status)]
        #[case::status_type("status_type", TaskField::StatusType)]
        #[case::status_symbol("status_symbol", TaskField::StatusSymbol)]
        #[case::completed("completed", TaskField::Completed)]
        #[case::priority("priority", TaskField::Priority)]
        #[case::due("due", TaskField::Due)]
        #[case::done("done", TaskField::Done)]
        #[case::created("created", TaskField::Created)]
        #[case::start("start", TaskField::Start)]
        #[case::scheduled("scheduled", TaskField::Scheduled)]
        #[case::cancelled("cancelled", TaskField::Cancelled)]
        #[case::fully_complete("fully_complete", TaskField::FullyComplete)]
        fn parses_all_task_field_variants(
            #[case] name: &str,
            #[case] expected: TaskField,
        ) {
            assert_eq!(TaskField::parse(name), Some(expected));
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

    mod list_field {
        use pretty_assertions::assert_eq;
        use rstest::rstest;

        use super::*;

        #[rstest]
        #[case::text("text", ListField::Text)]
        #[case::raw_text("raw_text", ListField::RawText)]
        #[case::line("line", ListField::Line)]
        #[case::parent("parent", ListField::Parent)]
        #[case::depth("depth", ListField::Depth)]
        #[case::tags("tags", ListField::Tags)]
        #[case::is_task("is_task", ListField::IsTask)]
        #[case::kind("kind", ListField::Kind)]
        #[case::is_ordered("is_ordered", ListField::IsOrdered)]
        #[case::embedded_due("due", ListField::Task(TaskField::Due))]
        #[case::embedded_completed(
            "completed",
            ListField::Task(TaskField::Completed)
        )]
        fn parses_all_list_field_variants(
            #[case] name: &str,
            #[case] expected: ListField,
        ) {
            assert_eq!(ListField::parse(name), Some(expected));
        }

        #[test]
        fn rejects_an_unknown_accessor_name() {
            assert_eq!(ListField::parse("bogus"), None);
        }

        #[test]
        fn accessor_names_round_trip_through_parse() {
            for name in ListField::ACCESSOR_NAMES {
                assert!(
                    ListField::parse(name).is_some(),
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
            assert_eq!(
                FieldPath::closest_accessor(candidates, "nam"),
                Some("name")
            );
        }

        #[test]
        fn closest_accessor_rejects_a_match_past_the_threshold() {
            // "na" has threshold 1, but distance 2 from "name": too far to
            // suggest.
            let candidates: &[&str] = &["name"];
            assert_eq!(FieldPath::closest_accessor(candidates, "na"), None);
        }

        #[test]
        fn closest_accessor_returns_none_for_an_empty_candidate_list() {
            assert_eq!(FieldPath::closest_accessor(&[], "name"), None);
        }

        #[test]
        fn closest_accessor_breaks_ties_by_iteration_order() {
            // Both candidates are distance 1 from "mat"; iteration order wins.
            let candidates: &[&str] = &["cat", "bat"];
            assert_eq!(
                FieldPath::closest_accessor(candidates, "mat"),
                Some("cat")
            );
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
        fn parses_a_file_tags_accessor() {
            assert_eq!(
                FieldPath::parse("file.tags"),
                Ok(FieldPath::File(FileField::Tags))
            );
        }

        #[test]
        fn parses_a_list_accessor() {
            assert_eq!(
                FieldPath::parse("list.completed"),
                Ok(FieldPath::List(ListField::Task(TaskField::Completed)))
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
        #[case::unknown_file_accessor("file.zzzz")]
        #[case::extra_file_segment("file.name.extra")]
        #[case::bare_task("task")]
        #[case::trailing_dot_task("task.")]
        #[case::extra_task_segment("list.completed.extra")]
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
        fn rejects_task_accessor_with_list_suggestion() {
            assert_eq!(
                FieldPath::parse("task.completed"),
                Err(FieldPathError::new(
                    "task.completed",
                    Some("list.completed")
                ))
            );
        }

        #[test]
        fn no_suggestion_for_an_unrelated_unknown_accessor() {
            assert_eq!(
                FieldPath::parse("file.zzzz"),
                Err(FieldPathError::new("file.zzzz", None))
            );
        }
    }
}
