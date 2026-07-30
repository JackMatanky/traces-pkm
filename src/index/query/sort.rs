//! Dataview-compatible value comparison and sort ordering.

use std::cmp::Ordering;

use crate::note::FieldValue;

/// Compares two resolved field values of the same comparable kind.
///
/// Orders numbers by magnitude, strings/dates/durations lexicographically, and
/// booleans with `false < true`. Returns `None` for differing kinds or
/// unorderable values.
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

/// Whether `a` and `b` represent the same value for `.filter()`'s `==`/`!=`.
///
/// Falls back to [`compare_field_values`] returning [`Ordering::Equal`] when
/// [`FieldValue`]'s own structural equality says no — the same cross-kind text
/// normalization that lets a `String` literal match a `Date`/`Duration` field
/// for the ordering operators.
pub(super) fn fields_equal(a: &FieldValue, b: &FieldValue) -> bool {
    a == b || compare_field_values(a, b) == Some(Ordering::Equal)
}

/// Total order for [`super::QueryOutcome::sort`] and
/// [`super::QueryOutcome::group_by`].
///
/// Matches Dataview's `compareValue` semantics:
/// - **Null Values**: [`FieldValue::Null`] acts as the minimum value.
/// - **Direction**: `descending` reverses the entire comparator uniformly (so
///   `Null` leads ascending and trails descending).
/// - **Non-Null Ordering**: Ordered by [`compare_field_values`], falling back
///   to [`Ordering::Equal`] to maintain stable relative order for incomparable
///   kinds.
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
