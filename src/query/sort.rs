//! Equality, ordering, and sort-key utilities for resolved [`NoteFieldValue`]
//! instances.
//!
//! This module provides comparison and ordering primitives used by
//! [`super::QueryRecordSet::filter`], [`super::QueryRecordSet::sort`], and
//! [`super::QueryRecordSet::group_by`].
//!
//! # Sorting Ordering and Null Precedence
//!
//! Values are ordered according to their comparable kind (numbers by magnitude,
//! strings/dates/durations lexicographically, booleans with `false < true`).
//!
//! [`NoteFieldValue::Null`] acts as the minimum value in sort operations. Under
//! a total order, null values lead ascending sorts and trail descending sorts.
//!
//! # Examples
//!
//! ```ignore
//! # use traces_pkm::query::SortOrder;
//! let order = SortOrder::Ascending;
//! assert!(!order.is_descending());
//! ```

use std::cmp::Ordering;

use super::record::QueryFieldValueRef;
use crate::note::NoteFieldValue;

/// Sort direction for sorting operations and CLI configuration.
///
/// The CLI commands use this type directly as a [`clap::ValueEnum`], enabling
/// `--order` to accept `asc` or `desc` directly.
///
/// # Examples
///
/// ```ignore
/// # use traces_pkm::query::SortOrder;
/// let order = SortOrder::Ascending;
/// assert!(!order.is_descending());
/// ```
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, clap::ValueEnum)]
pub(crate) enum SortOrder {
    /// Ascending order (the default).
    #[default]
    #[value(name = "asc")]
    Ascending,
    /// Descending order.
    #[value(name = "desc")]
    Descending,
}

impl SortOrder {
    /// Returns `true` if this order is [`Self::Descending`].
    #[inline]
    #[must_use]
    pub(crate) const fn is_descending(self) -> bool {
        matches!(self, Self::Descending)
    }
}

/// Compares two resolved [`NoteFieldValue`] instances of the same comparable
/// kind.
///
/// Value ordering rules:
/// - Numbers are ordered by magnitude.
/// - Strings, dates, and durations are ordered lexicographically.
/// - Booleans are ordered with `false < true`.
///
/// Returns `Some` with the [`Ordering`] of `a` relative to `b` when they can be
/// compared, or `None` if they have different kinds or unorderable values.
pub(super) fn compare_field_values(
    a: &NoteFieldValue,
    b: &NoteFieldValue,
) -> Option<Ordering> {
    match (a, b) {
        (NoteFieldValue::Number(x), NoteFieldValue::Number(y)) => {
            x.partial_cmp(y)
        }
        (NoteFieldValue::Bool(x), NoteFieldValue::Bool(y)) => Some(x.cmp(y)),
        _ => match (a.as_str(), b.as_str()) {
            (Some(x), Some(y)) => Some(x.cmp(y)),
            _ => None,
        },
    }
}

/// Returns whether two resolved [`NoteFieldValue`] instances represent equal
/// values under filter comparison (`==` and `!=`).
///
/// Returns `true` when structural equality (`a == b`) holds, or when
/// [`compare_field_values`] returns `Some(Ordering::Equal)`. This cross-kind
/// text normalization allows string literals to match date or duration fields.
pub(super) fn fields_equal(a: &NoteFieldValue, b: &NoteFieldValue) -> bool {
    a == b || compare_field_values(a, b) == Some(Ordering::Equal)
}

impl QueryFieldValueRef<'_> {
    /// Compares this resolved field against an owned literal to establish
    /// ordering for `<`, `<=`, `>`, `>=` filter comparisons.
    pub(super) fn compare_to_literal(
        &self,
        literal: &NoteFieldValue,
    ) -> Option<Ordering> {
        match (self, literal) {
            (Self::Number(x), NoteFieldValue::Number(y)) => x.partial_cmp(y),
            (Self::Bool(x), NoteFieldValue::Bool(y)) => Some(x.cmp(y)),
            (Self::Date(x), NoteFieldValue::Date(y))
            | (Self::Duration(x), NoteFieldValue::Duration(y)) => {
                Some(x.cmp(&y.as_str()))
            }
            (Self::Object(_), NoteFieldValue::Object(_)) => None,
            (Self::Owned(value), literal) => {
                compare_field_values(value, literal)
            }
            _ => match (self.as_str(), literal.as_str()) {
                (Some(x), Some(y)) => Some(x.cmp(y)),
                _ => None,
            },
        }
    }

    /// Returns whether this resolved field equals an owned literal under
    /// filter comparison rules (`==`, `!=`).
    #[expect(
        clippy::float_cmp,
        reason = "query numeric equality intentionally uses exact parsed \
                  metadata equality; ordering still uses total_cmp"
    )]
    pub(super) fn is_equal_to_literal(&self, literal: &NoteFieldValue) -> bool {
        match self {
            Self::Null => matches!(literal, NoteFieldValue::Null),
            Self::Bool(value) => {
                matches!(literal, NoteFieldValue::Bool(other) if value == other)
            }
            Self::Number(value) => {
                matches!(literal, NoteFieldValue::Number(other) if value == other)
            }
            Self::Text(value) => literal.as_str() == Some(value),
            Self::Link(value) => {
                matches!(literal, NoteFieldValue::Link(other) if *value == other)
            }
            Self::Date(value) | Self::Duration(value) => {
                literal.as_str() == Some(value)
            }
            Self::Object(value) => {
                matches!(literal, NoteFieldValue::Object(other) if *value == other)
            }
            Self::List(_) | Self::Owned(_) => {
                fields_equal(&self.to_owned_value(), literal)
            }
        }
    }
}

/// Compares two resolved [`NoteFieldValue`] instances to establish a total
/// order for [`super::QueryRecordSet::sort`] and
/// [`super::QueryRecordSet::group_by`].
///
/// # Arguments
///
/// * `a` - First field value to compare.
/// * `b` - Second field value to compare.
/// * `descending` - Whether to reverse the comparison result.
///
/// Returns the [`Ordering`] of `a` relative to `b`:
/// - [`NoteFieldValue::Null`] acts as the minimum value.
/// - `descending` reverses the comparator uniformly, so null leads ascending
///   and trails descending.
/// - Numeric values use [`f64::total_cmp`], including non-finite values and
///   signed zero.
/// - Other non-null values use [`compare_field_values`], falling back to
///   [`Ordering::Equal`] to preserve stable relative order for incomparable
///   kinds.
pub(super) fn sort_key_cmp(
    a: &NoteFieldValue,
    b: &NoteFieldValue,
    descending: bool,
) -> Ordering {
    let ord = match (a, b) {
        (NoteFieldValue::Null, NoteFieldValue::Null) => Ordering::Equal,
        (NoteFieldValue::Null, _) => Ordering::Less,
        (_, NoteFieldValue::Null) => Ordering::Greater,
        (NoteFieldValue::Number(x), NoteFieldValue::Number(y)) => {
            x.total_cmp(y)
        }
        _ => compare_field_values(a, b).unwrap_or(Ordering::Equal),
    };
    if descending {
        ord.reverse()
    } else {
        ord
    }
}

/// Wraps a resolved [`NoteFieldValue`] so [`slice::sort_by_cached_key`] can
/// order by it using [`sort_key_cmp`].
///
/// [`NoteFieldValue`] does not implement [`Ord`] directly because comparison
/// requires a `descending` flag and null-as-minimum fallback rules that depend
/// on sort options. `SortKey` provides an [`Ord`] implementation scoped to a
/// single sorting operation for [`super::QueryRecordSet::sort_by_field`].
pub(super) struct SortKey {
    pub(super) value: NoteFieldValue,
    pub(super) descending: bool,
}

/// Compares two `SortKey` instances for equality based on their total ordering.
impl PartialEq for SortKey {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

/// Establishes total equivalence for `SortKey`.
impl Eq for SortKey {}

/// Compares two `SortKey` instances for ordering based on their total ordering.
impl PartialOrd for SortKey {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Compares two `SortKey` instances to establish their total ordering.
impl Ord for SortKey {
    fn cmp(&self, other: &Self) -> Ordering {
        sort_key_cmp(&self.value, &other.value, self.descending)
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, path::Path};

    use super::super::*;
    use crate::index::IndexerService;

    fn outcome_for_files(
        temp: &Path,
        files: &[(&str, &str)],
    ) -> QueryRecordSet {
        for (name, content) in files {
            fs::write(temp.join(name), content).expect("write note");
        }
        let index = IndexerService::new(temp).build().expect("build index");
        QueryService::new("class")
            .execute(&index, QueryRequest::pages(QuerySource::All))
    }

    fn outcome_for(temp: &Path, content: &str) -> QueryRecordSet {
        outcome_for_files(temp, &[("note.md", content)])
    }

    fn names(outcome: &QueryRecordSet) -> Vec<String> {
        outcome
            .iter()
            .map(|record| record.file().name().as_str().to_owned())
            .collect()
    }

    mod sort {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn orders_ascending_by_default() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let outcome = outcome_for_files(temp.path(), &[
                ("b.md", "---\nrating: 7\n---"),
                ("a.md", "---\nrating: 3\n---"),
            ]);

            let sorted = outcome.sort("rating", false).expect("valid sort");

            assert_eq!(names(&sorted), ["a", "b"]);
        }

        #[test]
        fn orders_descending_when_requested() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let outcome = outcome_for_files(temp.path(), &[
                ("b.md", "---\nrating: 7\n---"),
                ("a.md", "---\nrating: 3\n---"),
            ]);

            let sorted = outcome.sort("rating", true).expect("valid sort");

            assert_eq!(names(&sorted), ["b", "a"]);
        }

        #[test]
        fn missing_field_sorts_as_the_minimum_value() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let outcome = outcome_for_files(temp.path(), &[
                ("rated.md", "---\nrating: 3\n---"),
                ("unrated.md", "no frontmatter"),
            ]);

            let ascending =
                outcome.clone().sort("rating", false).expect("valid sort");
            let descending = outcome.sort("rating", true).expect("valid sort");

            // Matches Dataview: Null is the minimum value, so it leads
            // ascending and trails descending, like any other value would.
            assert_eq!(names(&ascending), ["unrated", "rated"]);
            assert_eq!(names(&descending), ["rated", "unrated"]);
        }

        #[test]
        fn ties_keep_original_relative_order() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let outcome = outcome_for_files(temp.path(), &[
                ("a.md", "---\nrating: 5\n---"),
                ("b.md", "---\nrating: 5\n---"),
            ]);

            let sorted = outcome.sort("rating", false).expect("valid sort");

            assert_eq!(names(&sorted), ["a", "b"]);
        }

        #[test]
        fn rejects_malformed_field_path() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let outcome = outcome_for(temp.path(), "body");

            assert_eq!(
                outcome.sort("file.bogus", false),
                Err(QueryError::Request(QueryRequestError::FieldPath(
                    FieldPathError::new("file.bogus", None)
                )))
            );
        }

        #[test]
        fn sorts_boolean_field_false_before_true() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let outcome = outcome_for_files(temp.path(), &[
                ("true.md", "---\nactive: true\n---"),
                ("false.md", "---\nactive: false\n---"),
            ]);

            let sorted = outcome.sort("active", false).expect("valid sort");

            assert_eq!(names(&sorted), ["false", "true"]);
        }

        #[test]
        fn sorts_null_field_alongside_boolean_field() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let outcome = outcome_for_files(temp.path(), &[
                ("true.md", "---\nactive: true\n---"),
                ("none.md", "no frontmatter"),
            ]);

            let sorted = outcome.sort("active", false).expect("valid sort");

            assert_eq!(names(&sorted), ["none", "true"]);
        }
    }

    mod sort_order {
        use pretty_assertions::assert_eq;

        use super::super::SortOrder;

        #[test]
        fn default_is_ascending() {
            assert_eq!(SortOrder::default(), SortOrder::Ascending);
        }

        #[test]
        fn only_descending_is_descending() {
            assert!(!SortOrder::Ascending.is_descending());
            assert!(SortOrder::Descending.is_descending());
        }
    }

    mod compare_field_values {
        use pretty_assertions::assert_eq;

        use super::super::compare_field_values;
        use crate::note::NoteFieldValue;

        #[test]
        fn orders_false_before_true() {
            let a = NoteFieldValue::Bool(false);
            let b = NoteFieldValue::Bool(true);

            assert_eq!(
                compare_field_values(&a, &b),
                Some(std::cmp::Ordering::Less)
            );
        }
    }

    mod sort_key_cmp {
        use std::cmp::Ordering;

        use pretty_assertions::assert_eq;

        use super::super::sort_key_cmp;
        use crate::note::NoteFieldValue;
        #[test]
        fn null_is_less_than_any_value() {
            let null = NoteFieldValue::Null;
            let number = NoteFieldValue::Number(1.0);
            let string = NoteFieldValue::String("hello".to_owned());
            let boolean = NoteFieldValue::Bool(true);

            assert_eq!(sort_key_cmp(&null, &number, false), Ordering::Less);
            assert_eq!(sort_key_cmp(&null, &string, false), Ordering::Less);
            assert_eq!(sort_key_cmp(&null, &boolean, false), Ordering::Less);
        }

        #[test]
        fn null_trails_in_descending_order() {
            let null = NoteFieldValue::Null;
            let number = NoteFieldValue::Number(1.0);

            assert_eq!(sort_key_cmp(&null, &number, true), Ordering::Greater);
        }

        #[test]
        fn sorts_non_finite_and_signed_zero_numbers_totally() {
            let nan = NoteFieldValue::Number(f64::NAN);
            let infinity = NoteFieldValue::Number(f64::INFINITY);
            let negative_zero = NoteFieldValue::Number(-0.0);
            let zero = NoteFieldValue::Number(0.0);

            assert_eq!(
                sort_key_cmp(&nan, &infinity, false),
                f64::NAN.total_cmp(&f64::INFINITY)
            );
            assert_eq!(
                sort_key_cmp(&negative_zero, &zero, false),
                Ordering::Less
            );
        }
    }
}
