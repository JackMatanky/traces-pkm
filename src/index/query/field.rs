//! Parses and resolves query field paths.
//!
//! A query field path string resolves to a [`FieldPath`], which represents one
//! of:
//! - A `file.<field>` accessor ([`FileField`])
//! - A `task.<field>` accessor ([`TaskField`])
//! - The `tags` accessor
//! - The `inlinks` accessor
//! - A frontmatter or inline metadata field key

use super::{super::file::FileRecord, error::QueryError};
use crate::note::{FieldKey, FieldValue};

/// Represents a `file.<field>` accessor backed by [`FileRecord`] metadata.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum FileField {
    /// Accesses [`FileRecord::path`].
    Path,
    /// Accesses [`FileRecord::name`].
    Name,
    /// Accesses [`FileRecord::folder`].
    Folder,
    /// Accesses [`FileRecord::size`].
    Size,
    /// Accesses [`FileRecord::created_at_or_modified`] as a datetime without a
    /// UTC offset.
    CreatedDateTime,
    /// Accesses [`FileRecord::created_at_or_modified`] as a bare date.
    CreatedDate,
    /// Accesses [`FileRecord::modified_at`] as a datetime without a UTC offset.
    ModifiedDateTime,
    /// Accesses [`FileRecord::modified_at`] as a bare date.
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

    /// Parses a `file.<field>` accessor name string.
    ///
    /// Returns [`None`] when `name` is unknown. Callers construct
    /// [`UnknownFieldPath`] directly because they retain the full
    /// `file.<field>` path.
    ///
    /// [`UnknownFieldPath`]: QueryError::UnknownFieldPath
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

    /// Resolves this accessor's value for a [`FileRecord`].
    ///
    /// Returns the evaluated [`FieldValue`].
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

/// Represents a `task.<field>` accessor valid on task-level records.
///
/// Applied to task records produced by [`FileIndex::query_tasks`].
///
/// [`FileIndex::query_tasks`]: super::super::FileIndex::query_tasks
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(super) enum TaskField {
    /// Accesses task completion state (`- [ ]` versus `- [x]`).
    Completed,
    /// Accesses task item text.
    Text,
}

impl TaskField {
    /// Lists all `task.<field>` accessor names accepted by [`Self::parse`].
    pub(super) const ACCESSOR_NAMES: &'static [&'static str] =
        &["completed", "text"];

    /// Parses the field portion of a `task.<field>` accessor string.
    ///
    /// Returns [`None`] if `name` is not a recognized accessor name. Mirrors
    /// [`FileField::parse`], allowing the caller constructing
    /// [`UnknownFieldPath`] to supply the full `task.<field>` path.
    ///
    /// [`UnknownFieldPath`]: QueryError::UnknownFieldPath
    pub(super) fn parse(name: &str) -> Option<Self> {
        match name {
            "completed" => Some(Self::Completed),
            "text" => Some(Self::Text),
            _ => None,
        }
    }
}

/// Represents a resolved query field path.
///
/// A [`FieldPath`] is resolved once per [`QueryOutcome`] transformation and
/// subsequently applied to each [`IndexRecord`].
///
/// [`QueryOutcome`]: super::QueryOutcome
/// [`IndexRecord`]: super::IndexRecord
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum FieldPath {
    /// Wraps a `file.<field>` accessor ([`FileField`]).
    File(FileField),
    /// Wraps a `task.<field>` accessor ([`TaskField`]), which resolves to
    /// [`FieldValue::Null`] on page-level records.
    Task(TaskField),
    /// Accesses a frontmatter or inline field key.
    Metadata(String),
    /// Accesses Note tags as a [`FieldValue::List`] of tag strings.
    Tags,
    /// Accesses project-relative paths of Notes linking to this Note as a
    /// [`FieldValue::List`].
    ///
    /// Derived dynamically by [`derive_inlinks`] rather than stored directly on
    /// the Note.
    ///
    /// [`derive_inlinks`]: super::super::inlinks::derive_inlinks
    Inlinks,
}

impl FieldPath {
    /// Parses a query field path string into a [`FieldPath`].
    ///
    /// Resolves `path` to one of:
    /// - A `file.<field>` accessor ([`FileField`])
    /// - A `task.<field>` accessor ([`TaskField`])
    /// - The `tags` accessor ([`FieldPath::Tags`])
    /// - The `inlinks` accessor ([`FieldPath::Inlinks`])
    /// - A frontmatter or inline metadata field key ([`FieldPath::Metadata`])
    ///
    /// # Errors
    ///
    /// Returns [`UnknownFieldPath`] if `path` meets any of the following
    /// conditions:
    /// - Is empty
    /// - Contains invalid `.` structure
    /// - Specifies an unrecognized `file.<field>` or `task.<field>` accessor
    ///   name
    ///
    /// [`UnknownFieldPath`]: QueryError::UnknownFieldPath
    pub(super) fn parse(path: &str) -> Result<Self, QueryError> {
        let path = path.trim();
        let invalid = || QueryError::unknown_field_path(path, None);
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
        Ok(Self::Metadata(FieldKey::new(path).canonical().to_owned()))
    }
}

/// Constructs an [`UnknownFieldPath`] error containing a suggestion hint.
///
/// Evaluates `field` against `candidates` using [`closest_accessor`] and
/// appends a "did you mean" suggestion hint to the returned [`QueryError`] if a
/// plausible match exists.
///
/// # Arguments
///
/// * `path` - The full unparsable field path string (for example `"file.nam"`).
/// * `prefix` - The accessor prefix string (`"file"` or `"task"`).
/// * `candidates` - The slice of valid accessor names for this prefix.
/// * `field` - The field name following `prefix`.
///
/// Returns a [`QueryError::UnknownFieldPath`] populated with the full path and
/// an optional suggested accessor.
///
/// [`UnknownFieldPath`]: QueryError::UnknownFieldPath
fn accessor_typo_error(
    path: &str,
    prefix: &str,
    candidates: &[&'static str],
    field: &str,
) -> QueryError {
    QueryError::unknown_field_path(
        path,
        closest_accessor(candidates, field)
            .map(|name| format!("{prefix}.{name}"))
            .as_deref(),
    )
}

/// Finds the accessor name with the smallest edit distance to an input string.
///
/// Evaluates `input` against each candidate in `candidates` using
/// [`edit_distance`].
///
/// The matching threshold is half of `input`'s character count rounded up, with
/// a minimum threshold of 1 (`input.chars().count().div_ceil(2).max(1)`). This
/// threshold ensures single-character typos (such as `"nam"` for `"name"`)
/// match while preventing unrelated words (such as `"bogus"`) from matching any
/// candidate accessor.
///
/// Returns `Some(&'static str)` containing the candidate with the smallest edit
/// distance if that distance does not exceed the calculated threshold. Returns
/// [`None`] if `candidates` is empty or no candidate falls within the matching
/// threshold.
fn closest_accessor(
    candidates: &[&'static str],
    input: &str,
) -> Option<&'static str> {
    let threshold = input.chars().count().div_ceil(2).max(1);
    candidates
        .iter()
        .map(|&name| (name, edit_distance(input, name)))
        .min_by_key(|&(_, distance)| distance)
        .filter(|&(_, distance)| distance <= threshold)
        .map(|(name, _)| name)
}

/// Calculates the Levenshtein edit distance between two strings.
///
/// Computes the minimum number of single-character insertions, deletions, or
/// substitutions required to transform `a` into `b` using an iterative two-row
/// Wagner-Fischer algorithm.
///
/// Returns the edit distance as a [`usize`].
fn edit_distance(a: &str, b: &str) -> usize {
    let b_chars: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b_chars.len()).collect();
    for (i, ch_a) in a.chars().enumerate() {
        let mut next_row = Vec::with_capacity(row.len());
        next_row.push(i.saturating_add(1));
        for (j, &ch_b) in b_chars.iter().enumerate() {
            let substitution_cost = usize::from(ch_a != ch_b);
            let deletion = row
                .get(j.saturating_add(1))
                .copied()
                .unwrap_or(usize::MAX)
                .saturating_add(1);
            let insertion = next_row
                .get(j)
                .copied()
                .unwrap_or(usize::MAX)
                .saturating_add(1);
            let substitution = row
                .get(j)
                .copied()
                .unwrap_or(usize::MAX)
                .saturating_add(substitution_cost);
            next_row.push(deletion.min(insertion).min(substitution));
        }
        row = next_row;
    }
    row.last().copied().unwrap_or(0)
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
        use rstest::rstest;

        use super::*;

        #[rstest]
        #[case::identical("name", "name", 0)]
        #[case::classic_kitten_sitting("kitten", "sitting", 3)]
        #[case::empty_a("", "abc", 3)]
        #[case::empty_b("abc", "", 3)]
        #[case::single_insertion("nam", "name", 1)]
        #[case::single_deletion("name", "nam", 1)]
        #[case::single_substitution("cat", "hat", 1)]
        fn edit_distance_computes_the_minimum_operation_count(
            #[case] a: &str,
            #[case] b: &str,
            #[case] expected: usize,
        ) {
            assert_eq!(edit_distance(a, b), expected);
        }

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
        fn rejects_malformed_paths(#[case] path: &str) {
            assert_eq!(
                FieldPath::parse(path),
                Err(QueryError::unknown_field_path(path, None))
            );
        }

        #[test]
        fn suggests_the_closest_file_accessor_for_a_typo() {
            assert_eq!(
                FieldPath::parse("file.nam"),
                Err(QueryError::unknown_field_path(
                    "file.nam",
                    Some("file.name")
                ))
            );
        }

        #[test]
        fn suggests_the_closest_task_accessor_for_a_typo() {
            assert_eq!(
                FieldPath::parse("task.complete"),
                Err(QueryError::unknown_field_path(
                    "task.complete",
                    Some("task.completed")
                ))
            );
        }

        #[test]
        fn no_suggestion_for_an_unrelated_unknown_accessor() {
            assert_eq!(
                FieldPath::parse("file.bogus"),
                Err(QueryError::unknown_field_path("file.bogus", None))
            );
        }
    }
}
