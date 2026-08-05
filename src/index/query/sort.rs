//! Provides Dataview-compatible equality and ordering for resolved
//! [`FieldValue`] instances.

use std::cmp::Ordering;

use crate::note::FieldValue;

/// Defines the sort direction for [`super::QueryOutcome::sort`] and CLI
/// `--order` flags.
///
/// Rust and Template callers matching Dataview's boolean convention
/// (`.sort(path, descending: bool)`) use [`Self::is_descending`] to bridge to
/// the `bool`-based comparator. CLI commands use this type directly as a
/// [`clap::ValueEnum`], enabling `--order` to accept `asc` or `desc` without
/// per-command enum duplication.
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

/// Compares two resolved [`FieldValue`] instances, `a` and `b`, of the same
/// comparable kind.
///
/// Value ordering rules:
/// - Numbers are ordered by magnitude.
/// - Strings, dates, and durations are ordered lexicographically.
/// - Booleans are ordered with `false < true`.
///
/// Returns `Some` with the [`Ordering`] of `a` relative to `b` when they can be
/// compared, or `None` if they have differing kinds or unorderable values.
pub(super) fn compare_field_values(
    a: &FieldValue,
    b: &FieldValue,
) -> Option<Ordering> {
    match (a, b) {
        (FieldValue::Number(x), FieldValue::Number(y)) => x.partial_cmp(y),
        (FieldValue::Bool(x), FieldValue::Bool(y)) => Some(x.cmp(y)),
        _ => match (a.as_str(), b.as_str()) {
            (Some(x), Some(y)) => Some(x.cmp(y)),
            _ => None,
        },
    }
}

/// Returns `true` if two resolved [`FieldValue`] instances, `a` and `b`,
/// represent equal values under filter comparison (`==` and `!=`).
///
/// Returns `true` when structural equality (`a == b`) holds, or when
/// [`compare_field_values`] returns `Some(Ordering::Equal)`. This cross-kind
/// text normalization allows string literals to match date or duration fields.
pub(super) fn fields_equal(a: &FieldValue, b: &FieldValue) -> bool {
    a == b || compare_field_values(a, b) == Some(Ordering::Equal)
}

/// Compares two resolved [`FieldValue`] instances to establish a total order
/// for [`super::QueryOutcome::sort`] and [`super::QueryOutcome::group_by`].
///
/// # Arguments
///
/// * `a` - First field value to compare.
/// * `b` - Second field value to compare.
/// * `descending` - Whether to reverse the comparison result.
///
/// Returns the [`Ordering`] of `a` relative to `b` according to Dataview's
/// `compareValue` semantics:
/// - Null values: [`FieldValue::Null`] acts as the minimum value.
/// - Direction: `descending` reverses the comparator uniformly, so
///   [`FieldValue::Null`] leads ascending and trails descending.
/// - Non-null ordering: Values are ordered by [`compare_field_values`], falling
///   back to [`Ordering::Equal`] to preserve stable relative order for
///   incomparable kinds.
pub(super) fn sort_key_cmp(
    a: &FieldValue,
    b: &FieldValue,
    descending: bool,
) -> Ordering {
    let ord = match (a, b) {
        (FieldValue::Null, FieldValue::Null) => Ordering::Equal,
        (FieldValue::Null, _) => Ordering::Less,
        (_, FieldValue::Null) => Ordering::Greater,
        _ => compare_field_values(a, b).unwrap_or(Ordering::Equal),
    };
    if descending {
        ord.reverse()
    } else {
        ord
    }
}

/// Wraps a resolved [`FieldValue`] so [`slice::sort_by_cached_key`] can order
/// by it using [`sort_key_cmp`].
///
/// [`FieldValue`] does not implement [`Ord`] directly because comparison
/// requires a `descending` flag and null-as-minimum fallback rules that depend
/// on sort options. [`SortKey`] provides an [`Ord`] implementation scoped to a
/// single sorting operation for [`super::QueryOutcome::sort_by_field`].
pub(super) struct SortKey {
    pub(super) value: FieldValue,
    pub(super) descending: bool,
}

impl PartialEq for SortKey {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for SortKey {}

impl PartialOrd for SortKey {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for SortKey {
    fn cmp(&self, other: &Self) -> Ordering {
        sort_key_cmp(&self.value, &other.value, self.descending)
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, path::Path};

    use pretty_assertions::assert_eq;

    use super::super::*;
    use crate::index::FileIndex;

    fn outcome_for_files(temp: &Path, files: &[(&str, &str)]) -> QueryOutcome {
        for (name, content) in files {
            fs::write(temp.join(name), content).expect("write note");
        }
        FileIndex::build(temp).expect("build index").query(&QuerySource::All)
    }

    fn outcome_for(temp: &Path, content: &str) -> QueryOutcome {
        outcome_for_files(temp, &[("note.md", content)])
    }

    fn names(outcome: &QueryOutcome) -> Vec<String> {
        outcome
            .iter()
            .map(|record| record.file().name().as_str().to_owned())
            .collect()
    }

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
            Err(QueryError::unknown_field_path("file.bogus", None))
        );
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
}
