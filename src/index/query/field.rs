//! Query field path parsing and resolution.
//!
//! [`FieldPath`] is the unified accessor a query field path string resolves
//! to: a `file.<field>` accessor ([`FileField`]), a `task.<field>` accessor
//! ([`TaskField`]), `tags`, `inlinks`, or a frontmatter/inline field key.

use super::{super::file::FileRecord, error::QueryError};
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

/// A `task.<field>` accessor, valid on task-level rows built by
/// [`super::super::FileIndex::query_tasks`].
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(super) enum TaskField {
    /// Task completion state (`- [ ]` vs `- [x]`).
    Completed,
    /// Task item text.
    Text,
}

impl TaskField {
    /// `task.<field>` accessor names [`Self::parse`] accepts.
    pub(super) const ACCESSOR_NAMES: &'static [&'static str] =
        &["completed", "text"];

    /// Parses a `task.<field>` accessor name (the part after `"task."`).
    ///
    /// Returns `None` if `name` is not a known accessor. Mirrors
    /// [`FileField::parse`]'s single failure mode; the caller building
    /// [`QueryError::UnknownFieldPath`] already has the full `task.<field>`
    /// path.
    pub(super) fn parse(name: &str) -> Option<Self> {
        match name {
            "completed" => Some(Self::Completed),
            "text" => Some(Self::Text),
            _ => None,
        }
    }
}

/// A query field path, resolved once per
/// [`super::QueryOutcome`] transformation and then applied to every
/// [`super::IndexRecord`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum FieldPath {
    /// A `file.<field>` accessor.
    File(FileField),
    /// A `task.<field>` accessor, resolving to [`FieldValue::Null`] on
    /// page-level records.
    Task(TaskField),
    /// A frontmatter or inline field, looked up by key.
    Metadata(String),
    /// The Note's markdown tags, as a [`FieldValue::List`] of tag strings.
    Tags,
    /// Notes whose outlinks resolve to this Note, as a [`FieldValue::List`]
    /// of project-relative path strings. Derived by
    /// [`super::super::inlinks::derive_inlinks`], not stored on the Note
    /// itself.
    Inlinks,
}

impl FieldPath {
    /// Parses a query field path string into a [`FieldPath`].
    ///
    /// Resolves `file.<field>` accessors, `task.<field>` accessors, `tags`,
    /// `inlinks`, or frontmatter/inline field keys.
    ///
    /// # Errors
    ///
    /// - [`QueryError::UnknownFieldPath`] if `path` is empty, uses an unknown
    ///   `file.*`/`task.*` accessor, or has unexpected `.` structure.
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
        Ok(Self::Metadata(path.to_owned()))
    }
}

/// Builds [`QueryError::UnknownFieldPath`] for a `<prefix>.<field>` path
/// whose accessor `field` matched neither [`FileField::parse`] nor
/// [`TaskField::parse`], adding a "did you mean" suggestion when `field` is
/// a plausible typo of one of `candidates`.
///
/// Shared by [`FieldPath::parse`]'s `file.`/`task.` branches, which differ
/// only in `prefix` and which accessor list to check against.
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

/// Finds the accessor name in `candidates` with the smallest edit distance
/// from `input`, when that distance is small enough to be a plausible typo
/// rather than an unrelated word.
///
/// Shared "did you mean" suggestion builder for [`FieldPath::parse`]'s
/// `file.<field>`/`task.<field>` failure branches. `candidates` is always
/// [`FileField::ACCESSOR_NAMES`] or [`TaskField::ACCESSOR_NAMES`], each a
/// handful of short, fixed names, so a brute-force scan is cheap. There is no
/// equivalent for frontmatter/inline-field keys: those are arbitrary
/// per-project data [`FieldPath::parse`] never sees, not a fixed list to
/// compare against.
///
/// The threshold is half of `input`'s length (rounded up, minimum 1): tight
/// enough that unrelated words like `"bogus"` never match one of the ten
/// `file.*` accessors, loose enough to catch a single-character typo like
/// `"nam"` for `"name"`.
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

/// Levenshtein edit distance between `a` and `b`: the minimum number of
/// single-character insertions, deletions, or substitutions turning one into
/// the other.
///
/// Iterative two-row Wagner-Fischer, built without indexing (every project
/// lint here denies `clippy::indexing_slicing`) by growing `next_row`
/// through `.push()` and reading prior entries with `.get()`. `candidates` in
/// [`closest_accessor`] are a handful of characters each, so the O(n*m) shape
/// stays trivially cheap.
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
