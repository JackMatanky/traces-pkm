//! Metadata field values parsed from YAML frontmatter and inline field text.
//!
//! This module provides [`NoteFieldValue`], which represents strongly typed
//! metadata values extracted from Markdown notes, including scalars (booleans,
//! numbers, strings, dates, durations), links, lists, and objects.
//!
//! # Examples
//!
//! ```rust
//! use traces_pkm::NoteFieldValue;
//!
//! let val = NoteFieldValue::String("value".to_owned());
//! assert_eq!(val.as_str(), Some("value"));
//! ```
use std::fmt::Write as _;

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

use super::Link;
use crate::{
    DateTimeValue, DateValue, DurationValue, Tag, field::FieldValueRef,
};

/// A metadata value parsed from YAML frontmatter or inline field text.
///
/// Supports nulls, booleans, numbers, strings, ISO dates, duration literals,
/// wikilinks, ordered lists, and nested objects.
///
/// # Examples
///
/// ```rust
/// use traces_pkm::NoteFieldValue;
///
/// let val = NoteFieldValue::String("Draft".to_owned());
/// assert_eq!(val.as_str(), Some("Draft"));
/// ```
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub enum NoteFieldValue {
    /// Empty or missing value.
    Null,
    /// Boolean value (`true` or `false`).
    Bool(bool),
    /// Numeric value stored as `f64`.
    Number(f64),
    /// Plain text value.
    String(String),
    /// ISO `YYYY-MM-DD` date.
    Date(DateValue),
    /// ISO `YYYY-MM-DDThh:mm:ss` date-time.
    DateTime(DateTimeValue),
    /// Duration literal in source spelling, such as `4h15m` or `4 yrs, 6 wks`.
    /// Equality and ordering compare by total parsed seconds, not source
    /// spelling.
    Duration(DurationValue),
    /// A link parsed from wikilink or Markdown link syntax.
    Link(Link),
    /// Ordered list value.
    List(Box<[Self]>),
    /// Keyed object value stored in a deterministically ordered map.
    Object(IndexMap<String, Self>),
}

impl NoteFieldValue {
    /// Returns the inner text for [`NoteFieldValue::String`] and
    /// [`NoteFieldValue::Duration`] variants, or `None` for any other kind.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use traces_pkm::NoteFieldValue;
    ///
    /// let val = NoteFieldValue::String("Draft".to_owned());
    /// assert_eq!(val.as_str(), Some("Draft"));
    ///
    /// let num = NoteFieldValue::Number(42.0);
    /// assert_eq!(num.as_str(), None);
    /// ```
    #[inline]
    #[must_use]
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(s) => Some(s),
            Self::Duration(dv) => Some(dv.as_str()),
            _ => None,
        }
    }

    /// Returns the parsed calendar date if this value is
    /// [`NoteFieldValue::Date`], [`NoteFieldValue::DateTime`], or a
    /// [`NoteFieldValue::String`] beginning with a valid `YYYY-MM-DD` ISO
    /// date, or `None` otherwise.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use chrono::NaiveDate;
    /// use traces_pkm::NoteFieldValue;
    ///
    /// let str_val = NoteFieldValue::String("2025-01-15T12:00:00".to_owned());
    /// assert_eq!(str_val.as_date(), NaiveDate::from_ymd_opt(2025, 1, 15));
    ///
    /// let invalid = NoteFieldValue::String("not-a-date".to_owned());
    /// assert_eq!(invalid.as_date(), None);
    /// ```
    #[inline]
    #[must_use]
    pub fn as_date(&self) -> Option<chrono::NaiveDate> {
        match self {
            Self::Date(value) => Some(value.into_inner()),
            Self::DateTime(value) => Some(value.date().into_inner()),
            Self::String(s) => s
                .get(..10)
                .and_then(|prefix| DateValue::parse_iso(prefix).ok())
                .map(DateValue::into_inner),
            _ => None,
        }
    }

    /// Borrows this value without cloning.
    #[inline]
    #[must_use]
    pub fn as_ref(&self) -> NoteFieldValueRef<'_> {
        match self {
            Self::Null => NoteFieldValueRef::Null,
            Self::Bool(v) => NoteFieldValueRef::Bool(*v),
            Self::Number(v) => NoteFieldValueRef::Number(*v),
            Self::String(v) => NoteFieldValueRef::String(v),
            Self::Date(v) => NoteFieldValueRef::Date(*v),
            Self::DateTime(v) => NoteFieldValueRef::DateTime(*v),
            Self::Duration(v) => NoteFieldValueRef::Duration(v),
            Self::Link(v) => NoteFieldValueRef::Link(v),
            Self::List(v) => NoteFieldValueRef::List(v),
            Self::Object(v) => NoteFieldValueRef::Object(v),
        }
    }
}

/// Borrowed mirror of [`NoteFieldValue`], resolved without allocation from a
/// note's parsed metadata.
#[derive(Copy, Clone, Debug)]
pub enum NoteFieldValueRef<'a> {
    /// Empty or missing value.
    Null,
    /// Boolean value.
    Bool(bool),
    /// Numeric value.
    Number(f64),
    /// Borrowed text.
    String(&'a str),
    /// ISO `YYYY-MM-DD` date.
    Date(DateValue),
    /// ISO `YYYY-MM-DDThh:mm:ss` date-time.
    DateTime(DateTimeValue),
    /// Borrowed duration.
    Duration(&'a DurationValue),
    /// Borrowed link.
    Link(&'a Link),
    /// Borrowed ordered list.
    List(&'a [NoteFieldValue]),
    /// Borrowed keyed object.
    Object(&'a IndexMap<String, NoteFieldValue>),
}

/// Discriminant-only companion to the canonical rank order. The single place
/// the rank numbers are written down; [`NoteFieldValueRef::rank`] and
/// `SortKey::rank` (`src/query/sort.rs`) both delegate to
/// [`NoteFieldType::rank`], so they cannot drift from each other.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum NoteFieldType {
    Null,
    Bool,
    Number,
    Duration,
    /// Covers both [`NoteFieldValueRef::Date`] and
    /// [`NoteFieldValueRef::DateTime`]: the same comparable temporal domain at
    /// different precision. [`NoteFieldValueRef::is_equal_to_literal`] already
    /// treats a bare `Date` and a midnight-UTC `DateTime` as equal; giving them
    /// separate ranks would silently coarsen that into "different kind, never
    /// equal".
    Temporal,
    Text,
    Link,
    List,
    Object,
}

impl NoteFieldType {
    /// `Null < Bool < Number < Duration < Temporal < Text < Link < List <
    /// Object`.
    pub(crate) const fn rank(self) -> u8 {
        match self {
            Self::Null => 0,
            Self::Bool => 1,
            Self::Number => 2,
            Self::Duration => 3,
            Self::Temporal => 4,
            Self::Text => 5,
            Self::Link => 6,
            Self::List => 7,
            Self::Object => 8,
        }
    }
}

impl NoteFieldValueRef<'_> {
    const fn kind(&self) -> NoteFieldType {
        match self {
            Self::Null => NoteFieldType::Null,
            Self::Bool(_) => NoteFieldType::Bool,
            Self::Number(_) => NoteFieldType::Number,
            Self::Duration(_) => NoteFieldType::Duration,
            Self::Date(_) | Self::DateTime(_) => NoteFieldType::Temporal,
            Self::String(_) => NoteFieldType::Text,
            Self::Link(_) => NoteFieldType::Link,
            Self::List(_) => NoteFieldType::List,
            Self::Object(_) => NoteFieldType::Object,
        }
    }

    /// Ordinal rank of this value's kind in the canonical total order.
    const fn rank(&self) -> u8 {
        self.kind().rank()
    }

    /// Canonical total order across all field kinds: `Null < Bool < Number <
    /// Duration < Temporal (Date/DateTime) < Text < Link < List < Object`.
    /// Kinds compare by rank except where a pair has an explicit arm below
    /// (same-kind comparisons, and the Date/DateTime cross-kind case, which
    /// share one rank).
    #[inline]
    #[must_use]
    pub fn compare(&self, other: &Self) -> std::cmp::Ordering {
        use std::cmp::Ordering;
        match (self, other) {
            (Self::Null, Self::Null) => Ordering::Equal,
            (Self::Bool(a), Self::Bool(b)) => a.cmp(b),
            (Self::Number(a), Self::Number(b)) => {
                normalize_zero(*a).total_cmp(&normalize_zero(*b))
            }
            (Self::Duration(a), Self::Duration(b)) => a.cmp(b),
            (Self::Date(a), Self::Date(b)) => a.cmp(b),
            (Self::DateTime(a), Self::DateTime(b)) => a.cmp(b),
            (Self::Date(a), Self::DateTime(b)) => {
                DateTimeValue::from(*a).cmp(b)
            }
            (Self::DateTime(a), Self::Date(b)) => {
                a.cmp(&DateTimeValue::from(*b))
            }
            (Self::String(a), Self::String(b)) => a.cmp(b),
            (Self::Link(a), Self::Link(b)) => {
                a.target().cmp(b.target()).then_with(|| a.text().cmp(b.text()))
            }
            (Self::List(a), Self::List(b)) => compare_lists(a, b),
            (Self::Object(a), Self::Object(b)) => compare_objects(a, b),
            _ => self.rank().cmp(&other.rank()),
        }
    }
}

impl NoteFieldValueRef<'_> {
    /// Appends the shared query-display text used by list joins, table cells,
    /// and text output.
    pub(crate) fn append_text(&self, out: &mut String) {
        match self {
            Self::Null => {}
            Self::Bool(value) => {
                out.push_str(if *value {
                    "true"
                } else {
                    "false"
                });
            }
            Self::Number(value) => {
                let _ = write!(out, "{value}");
            }
            Self::String(value) => out.push_str(value),
            Self::Duration(value) => out.push_str(value.as_str()),
            Self::Date(value) => {
                let _ = write!(out, "{value}");
            }
            Self::DateTime(value) => {
                let _ = write!(out, "{value}");
            }
            Self::Link(link) => out.push_str(link.target()),
            Self::Object(fields) => {
                for (idx, (key, field)) in fields.iter().enumerate() {
                    if idx > 0 {
                        out.push_str(", ");
                    }
                    out.push_str(key);
                    out.push_str(": ");
                    field.as_ref().append_text(out);
                }
            }
            Self::List(values) => {
                for (idx, value) in values.iter().enumerate() {
                    if idx > 0 {
                        out.push_str(", ");
                    }
                    value.as_ref().append_text(out);
                }
            }
        }
    }

    pub(crate) fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(value) => Some(value),
            Self::Duration(value) => Some(value.as_str()),
            _ => None,
        }
    }

    pub(crate) fn to_owned_value(self) -> NoteFieldValue {
        match self {
            Self::Null => NoteFieldValue::Null,
            Self::Bool(value) => NoteFieldValue::Bool(value),
            Self::Number(value) => NoteFieldValue::Number(value),
            Self::String(value) => NoteFieldValue::String(value.to_owned()),
            Self::Date(value) => NoteFieldValue::Date(value),
            Self::DateTime(value) => NoteFieldValue::DateTime(value),
            Self::Duration(value) => NoteFieldValue::Duration((*value).clone()),
            Self::Link(value) => NoteFieldValue::Link((*value).clone()),
            Self::List(values) => NoteFieldValue::List(values.into()),
            Self::Object(value) => NoteFieldValue::Object((*value).clone()),
        }
    }

    /// Applies filter equality semantics (`==`, `!=`) to `literal`.
    #[expect(
        clippy::float_cmp,
        reason = "query numeric equality intentionally uses exact parsed \
                  metadata equality; ordering uses compare()"
    )]
    pub(crate) fn is_equal_to_literal(&self, literal: &NoteFieldValue) -> bool {
        match self {
            Self::Null => matches!(literal, NoteFieldValue::Null),
            Self::Bool(value) => {
                matches!(literal, NoteFieldValue::Bool(other) if value == other)
            }
            Self::Number(value) => {
                matches!(literal, NoteFieldValue::Number(other) if value == other)
            }
            Self::String(value) => literal.as_str() == Some(value),
            Self::Link(value) => {
                matches!(literal, NoteFieldValue::Link(other) if *value == other)
            }
            Self::Date(value) => match literal {
                NoteFieldValue::Date(other) => value == other,
                NoteFieldValue::DateTime(other) => {
                    other.is_equal_to_date(*value)
                }
                _ => false,
            },
            Self::DateTime(value) => match literal {
                NoteFieldValue::DateTime(other) => value == other,
                NoteFieldValue::Date(other) => value.is_equal_to_date(*other),
                _ => false,
            },
            Self::Duration(dv) => match literal {
                NoteFieldValue::Duration(other) => *dv == other,
                _ => false,
            },
            Self::Object(value) => {
                matches!(literal, NoteFieldValue::Object(other) if *value == other)
            }
            Self::List(_) => self.to_owned_value() == *literal,
        }
    }

    /// Evaluates `contains(field_val, target)`.
    ///
    /// Lists match exact values or descendant tags; string-like fields use
    /// substring containment.
    pub(crate) fn is_containing(&self, target: &NoteFieldValue) -> bool {
        match self {
            Self::List(items) => {
                let target_str = target.as_str();
                items.iter().any(|item| {
                    is_tag_or_value_matching(item, target, target_str)
                })
            }
            _ => match (self.as_str(), target.as_str()) {
                (Some(haystack), Some(needle)) => haystack.contains(needle),
                _ => false,
            },
        }
    }
}

/// Matches exact values, or tag descendants when both values stringify to tags.
fn is_tag_or_value_matching(
    item: &NoteFieldValue,
    target: &NoteFieldValue,
    target_str: Option<&str>,
) -> bool {
    if item == target {
        return true;
    }
    let (Some(item_str), Some(target_str)) = (item.as_str(), target_str) else {
        return false;
    };
    item_str.starts_with('#')
        && target_str.starts_with('#')
        && Tag::parse(item_str).is_ok_and(|tag| tag.is_contained_in(target_str))
}

impl PartialEq for NoteFieldValueRef<'_> {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        self.compare(other) == std::cmp::Ordering::Equal
    }
}
impl Eq for NoteFieldValueRef<'_> {}

/// `-0.0` and `0.0` both normalize to `0.0` before `total_cmp`, so signed zero
/// does not affect ordering.
fn normalize_zero(n: f64) -> f64 {
    if n == 0.0 {
        0.0
    } else {
        n
    }
}

/// Element-wise comparison; the shorter list is `Less` when every shared
/// element compares equal.
fn compare_lists(
    a: &[NoteFieldValue],
    b: &[NoteFieldValue],
) -> std::cmp::Ordering {
    for (x, y) in a.iter().zip(b.iter()) {
        let ord = x.as_ref().compare(&y.as_ref());
        if ord != std::cmp::Ordering::Equal {
            return ord;
        }
    }
    a.len().cmp(&b.len())
}

/// Sorted key lists first (`Vec<&str>: Ord` already gives exactly the list rule
/// -- element-wise, shorter-is-less-when-prefix), then values in that
/// sorted-key order.
fn compare_objects(
    a: &IndexMap<String, NoteFieldValue>,
    b: &IndexMap<String, NoteFieldValue>,
) -> std::cmp::Ordering {
    let mut a_keys: Vec<&str> = a.keys().map(String::as_str).collect();
    let mut b_keys: Vec<&str> = b.keys().map(String::as_str).collect();
    a_keys.sort_unstable();
    b_keys.sort_unstable();
    let key_order = a_keys.cmp(&b_keys);
    if key_order != std::cmp::Ordering::Equal {
        return key_order;
    }
    for key in a_keys {
        let (Some(x), Some(y)) = (a.get(key), b.get(key)) else {
            continue;
        };
        let ord = x.as_ref().compare(&y.as_ref());
        if ord != std::cmp::Ordering::Equal {
            return ord;
        }
    }
    std::cmp::Ordering::Equal
}

/// Converts a borrowed field value into a [`NoteFieldValue`].
///
/// Handles note-specific post-classification of scalar strings, applied
/// recursively through lists and objects: empty strings become null, wikilink
/// syntax becomes [`NoteFieldValue::Link`], and any remaining duration-shaped
/// spelling (e.g. `4h15m`) becomes [`NoteFieldValue::Duration`].
impl From<FieldValueRef<'_>> for NoteFieldValue {
    #[inline]
    fn from(value: FieldValueRef<'_>) -> Self {
        match value {
            FieldValueRef::Null => Self::Null,
            FieldValueRef::Bool(b) => Self::Bool(b),
            FieldValueRef::Int(i) =>
            {
                #[expect(
                    clippy::as_conversions,
                    clippy::cast_precision_loss,
                    reason = "integer field value converted to f64"
                )]
                Self::Number(i as f64)
            }
            FieldValueRef::Float(f) => Self::Number(f),
            FieldValueRef::String(s) => {
                let trimmed = s.trim();
                if trimmed.is_empty() {
                    Self::Null
                } else if let Some(link) = Link::parse_wikilink(trimmed) {
                    Self::Link(link)
                } else if let Ok(dv) = DurationValue::parse(trimmed) {
                    Self::Duration(dv)
                } else {
                    Self::String(s.into_owned())
                }
            }
            FieldValueRef::Date(value) => Self::Date(value),
            FieldValueRef::DateTime(value) => Self::DateTime(value),
            FieldValueRef::List(arr) => {
                Self::List(arr.into_iter().map(Into::into).collect())
            }
            FieldValueRef::Object(map) => Self::Object(
                map.into_iter()
                    .map(|(k, v)| (k.into_owned(), v.into()))
                    .collect(),
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::field::FieldValueRef;

    mod field_value {
        use pretty_assertions::assert_eq;

        use super::*;
        use crate::note::LinkType;

        #[test]
        fn converts_serde_yaml_value_into_field_value_variants() {
            let yaml = serde_yaml::from_str::<serde_yaml::Value>(
                "
                str: hello
                num: 42.5
                bool: true
                null_val: null
                date: 2026-07-29
                list: [1, 2]
            ",
            )
            .expect("valid yaml");

            assert_eq!(
                NoteFieldValue::from(FieldValueRef::from(yaml)),
                NoteFieldValue::Object(IndexMap::from_iter([
                    ("bool".to_owned(), NoteFieldValue::Bool(true)),
                    (
                        "date".to_owned(),
                        NoteFieldValue::Date(
                            DateValue::parse_iso("2026-07-29")
                                .expect("valid date")
                        )
                    ),
                    (
                        "list".to_owned(),
                        NoteFieldValue::List(
                            vec![
                                NoteFieldValue::Number(1.0),
                                NoteFieldValue::Number(2.0),
                            ]
                            .into(),
                        ),
                    ),
                    ("null_val".to_owned(), NoteFieldValue::Null),
                    ("num".to_owned(), NoteFieldValue::Number(42.5)),
                    (
                        "str".to_owned(),
                        NoteFieldValue::String("hello".to_owned())
                    ),
                ]))
            );
        }

        #[test]
        fn converts_wikilink_strings_into_link_values() {
            let yaml = serde_yaml::from_str::<serde_yaml::Value>(
                r#"
                link: "[[Project Alpha|Alpha]]"
                "#,
            )
            .expect("valid yaml");

            assert_eq!(
                NoteFieldValue::from(FieldValueRef::from(yaml)),
                NoteFieldValue::Object(IndexMap::from_iter([(
                    "link".to_owned(),
                    NoteFieldValue::Link(Link::new(
                        "Project Alpha",
                        "Alpha",
                        LinkType::Wikilink
                    ))
                )]))
            );
        }

        #[test]
        fn preserves_nested_yaml_objects() {
            let yaml = serde_yaml::from_str::<serde_yaml::Value>(
                "
                outer:
                  inner: value
                ",
            )
            .expect("valid yaml");

            assert_eq!(
                NoteFieldValue::from(FieldValueRef::from(yaml)),
                NoteFieldValue::Object(IndexMap::from_iter([(
                    "outer".to_owned(),
                    NoteFieldValue::Object(IndexMap::from_iter([(
                        "inner".to_owned(),
                        NoteFieldValue::String("value".to_owned())
                    )]))
                )]))
            );
        }

        #[test]
        fn converts_duration_strings_into_duration_values() {
            let yaml = serde_yaml::from_str::<serde_yaml::Value>(
                "
                scalar: 1h 30m
                list: [1h, 30m]
                map:
                  dev: 4h
                ",
            )
            .expect("valid yaml");

            assert_eq!(
                NoteFieldValue::from(FieldValueRef::from(yaml)),
                NoteFieldValue::Object(IndexMap::from_iter([
                    (
                        "list".to_owned(),
                        NoteFieldValue::List(
                            vec![
                                NoteFieldValue::Duration(
                                    DurationValue::parse("1h")
                                        .expect("valid duration"),
                                ),
                                NoteFieldValue::Duration(
                                    DurationValue::parse("30m")
                                        .expect("valid duration"),
                                ),
                            ]
                            .into(),
                        ),
                    ),
                    (
                        "map".to_owned(),
                        NoteFieldValue::Object(IndexMap::from_iter([(
                            "dev".to_owned(),
                            NoteFieldValue::Duration(
                                DurationValue::parse("4h")
                                    .expect("valid duration"),
                            ),
                        )])),
                    ),
                    (
                        "scalar".to_owned(),
                        NoteFieldValue::Duration(
                            DurationValue::parse("1h 30m")
                                .expect("valid duration"),
                        ),
                    ),
                ]))
            );
        }

        #[test]
        fn keeps_a_bare_number_string_as_string_not_duration() {
            let yaml =
                serde_yaml::from_str::<serde_yaml::Value>(r#"code: "42""#)
                    .expect("valid yaml");

            assert_eq!(
                NoteFieldValue::from(FieldValueRef::from(yaml)),
                NoteFieldValue::Object(IndexMap::from_iter([(
                    "code".to_owned(),
                    NoteFieldValue::String("42".to_owned())
                )]))
            );
        }
    }

    mod equality {
        use pretty_assertions::{assert_eq, assert_ne};

        use super::*;

        #[test]
        fn returns_true_for_identical_note_field_values() {
            assert_eq!(
                NoteFieldValue::Number(1.0),
                NoteFieldValue::Number(1.0)
            );
        }

        #[test]
        fn returns_false_for_different_note_field_values() {
            assert_ne!(
                NoteFieldValue::Number(1.0),
                NoteFieldValue::Number(2.0)
            );
        }

        #[test]
        fn returns_false_for_a_string_literal_against_a_typed_date_field() {
            assert_ne!(
                NoteFieldValue::String("2024-01-01".into()),
                NoteFieldValue::Date(
                    DateValue::parse_iso("2024-01-01").expect("valid date")
                )
            );
        }

        #[test]
        fn returns_true_for_semantically_equivalent_durations() {
            let a = NoteFieldValue::Duration(
                DurationValue::parse("1h 30m").expect("valid duration"),
            );
            let b = NoteFieldValue::Duration(
                DurationValue::parse("90m").expect("valid duration"),
            );
            assert_eq!(a, b);
        }
    }

    mod accessors {
        use rstest::rstest;

        use super::*;

        #[test]
        fn as_str_returns_inner_str_for_string_variant() {
            let str_val = NoteFieldValue::String("text".to_owned());
            assert_eq!(str_val.as_str(), Some("text"));
        }

        #[test]
        fn as_str_returns_inner_str_for_duration_variant() {
            let dur_val = NoteFieldValue::Duration(
                DurationValue::parse("4h").expect("valid duration"),
            );
            assert_eq!(dur_val.as_str(), Some("4h"));
        }

        #[test]
        fn as_str_returns_none_for_a_typed_date_field() {
            let date_val = NoteFieldValue::Date(
                DateValue::parse_iso("2026-09-02").expect("valid date"),
            );
            assert_eq!(date_val.as_str(), None);
        }

        #[test]
        fn as_str_returns_none_for_a_typed_datetime_field() {
            let datetime_val = NoteFieldValue::DateTime(
                DateTimeValue::parse_iso("2026-09-02T14:30:00")
                    .expect("valid datetime"),
            );
            assert_eq!(datetime_val.as_str(), None);
        }

        #[rstest]
        #[case::null(NoteFieldValue::Null)]
        #[case::bool(NoteFieldValue::Bool(true))]
        #[case::number(NoteFieldValue::Number(42.0))]
        #[case::list(NoteFieldValue::List(Box::default()))]
        #[case::object(NoteFieldValue::Object(IndexMap::new()))]
        fn as_str_returns_none_for_non_string_variant(
            #[case] value: NoteFieldValue,
        ) {
            assert_eq!(value.as_str(), None);
        }

        #[test]
        fn as_date_extracts_the_calendar_date_from_a_typed_date_field() {
            let date_val = NoteFieldValue::Date(
                DateValue::parse_iso("2026-09-02").expect("valid date"),
            );
            assert_eq!(
                date_val.as_date(),
                chrono::NaiveDate::from_ymd_opt(2026, 9, 2)
            );
        }

        #[test]
        fn as_date_extracts_the_calendar_date_from_a_typed_datetime_field() {
            let datetime_val = NoteFieldValue::DateTime(
                DateTimeValue::parse_iso("2026-09-02T14:30:00")
                    .expect("valid datetime"),
            );
            assert_eq!(
                datetime_val.as_date(),
                chrono::NaiveDate::from_ymd_opt(2026, 9, 2)
            );
        }

        #[test]
        fn as_date_parses_the_leading_ten_bytes_of_a_string_variant() {
            let str_val =
                NoteFieldValue::String("2026-09-02T14:30:00".to_owned());
            assert_eq!(
                str_val.as_date(),
                chrono::NaiveDate::from_ymd_opt(2026, 9, 2)
            );
        }

        #[test]
        fn as_date_returns_none_for_a_string_shorter_than_an_iso_date() {
            let str_val = NoteFieldValue::String("2026-09".to_owned());
            assert_eq!(str_val.as_date(), None);
        }

        #[test]
        fn as_date_returns_none_for_a_string_ending_mid_char_boundary() {
            // The first 10 bytes of "2026-09-0é" split a multi-byte UTF-8
            // character; `as_date` must not panic on this input.
            let str_val = NoteFieldValue::String("2026-09-0é-x".to_owned());
            assert_eq!(str_val.as_date(), None);
        }

        #[test]
        fn as_date_returns_none_for_the_null_variant() {
            assert_eq!(NoteFieldValue::Null.as_date(), None);
        }

        #[test]
        fn as_date_returns_none_for_a_non_date_shaped_string() {
            assert_eq!(
                NoteFieldValue::String("not-a-date".to_owned()).as_date(),
                None
            );
        }
    }

    mod compare {
        use std::cmp::Ordering;

        use pretty_assertions::assert_eq;

        use super::*;

        fn date(s: &str) -> DateValue {
            DateValue::parse_iso(s).expect("valid date")
        }

        fn datetime(s: &str) -> DateTimeValue {
            DateTimeValue::parse_iso(s).expect("valid datetime")
        }

        fn duration(s: &str) -> DurationValue {
            DurationValue::parse(s).expect("valid duration")
        }

        #[test]
        fn orders_every_kind_by_ascending_rank() {
            let link =
                Link::new("target", "text", crate::note::LinkType::Markdown);
            let list_items = [NoteFieldValue::Number(1.0)];
            let object_map = IndexMap::from_iter([(
                "a".to_owned(),
                NoteFieldValue::Number(1.0),
            )]);
            let duration_a = duration("1h");
            let ordered = [
                NoteFieldValueRef::Null,
                NoteFieldValueRef::Bool(false),
                NoteFieldValueRef::Number(1.0),
                NoteFieldValueRef::Duration(&duration_a),
                NoteFieldValueRef::Date(date("2026-01-01")),
                NoteFieldValueRef::String("a"),
                NoteFieldValueRef::Link(&link),
                NoteFieldValueRef::List(&list_items),
                NoteFieldValueRef::Object(&object_map),
            ];
            for pair in ordered.windows(2) {
                let (Some(a), Some(b)) =
                    (pair.first().copied(), pair.get(1).copied())
                else {
                    continue;
                };
                assert_eq!(a.compare(&b), Ordering::Less, "{a:?} vs {b:?}");
                assert_eq!(b.compare(&a), Ordering::Greater, "{b:?} vs {a:?}");
            }
        }

        #[test]
        fn two_nan_numbers_compare_equal() {
            let a = NoteFieldValueRef::Number(f64::NAN);
            let b = NoteFieldValueRef::Number(f64::NAN);
            assert_eq!(a.compare(&b), Ordering::Equal);
        }

        #[test]
        fn negative_zero_and_positive_zero_compare_equal() {
            let a = NoteFieldValueRef::Number(-0.0);
            let b = NoteFieldValueRef::Number(0.0);
            assert_eq!(a.compare(&b), Ordering::Equal);
        }

        #[test]
        fn durations_with_different_spelling_compare_equal() {
            let a = duration("1h");
            let b = duration("60m");
            assert_eq!(
                NoteFieldValueRef::Duration(&a)
                    .compare(&NoteFieldValueRef::Duration(&b)),
                Ordering::Equal
            );
        }

        #[test]
        fn a_bare_date_compares_equal_to_a_midnight_datetime_on_the_same_day() {
            let d = date("2026-07-29");
            let dt = datetime("2026-07-29T00:00:00");
            assert_eq!(
                NoteFieldValueRef::Date(d)
                    .compare(&NoteFieldValueRef::DateTime(dt)),
                Ordering::Equal
            );
        }

        #[test]
        fn a_bare_date_orders_before_an_afternoon_datetime_on_the_same_day() {
            let d = date("2026-07-29");
            let dt = datetime("2026-07-29T14:30:00");
            assert_eq!(
                NoteFieldValueRef::Date(d)
                    .compare(&NoteFieldValueRef::DateTime(dt)),
                Ordering::Less
            );
        }

        #[test]
        fn list_prefix_orders_below_its_longer_extension() {
            let short = [NoteFieldValue::Number(1.0)];
            let long =
                [NoteFieldValue::Number(1.0), NoteFieldValue::Number(2.0)];
            assert_eq!(
                NoteFieldValueRef::List(&short)
                    .compare(&NoteFieldValueRef::List(&long)),
                Ordering::Less
            );
        }

        #[test]
        fn link_compares_by_target_then_text() {
            let a =
                Link::new("target-a", "text", crate::note::LinkType::Markdown);
            let b =
                Link::new("target-b", "text", crate::note::LinkType::Markdown);
            assert_eq!(
                NoteFieldValueRef::Link(&a)
                    .compare(&NoteFieldValueRef::Link(&b)),
                Ordering::Less
            );
        }

        #[test]
        fn objects_with_differing_key_sets_order_by_sorted_keys() {
            let a = IndexMap::from_iter([
                ("a".to_owned(), NoteFieldValue::Number(1.0)),
                ("b".to_owned(), NoteFieldValue::Number(2.0)),
            ]);
            let b = IndexMap::from_iter([
                ("a".to_owned(), NoteFieldValue::Number(1.0)),
                ("c".to_owned(), NoteFieldValue::Number(3.0)),
            ]);
            assert_eq!(
                NoteFieldValueRef::Object(&a)
                    .compare(&NoteFieldValueRef::Object(&b)),
                Ordering::Less
            );
        }
    }
}
