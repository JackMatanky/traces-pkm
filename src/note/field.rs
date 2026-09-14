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
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

use super::Link;
use crate::{DateTimeValue, DateValue, field::FieldValueRef};

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
    Duration(String),
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
            Self::String(s) | Self::Duration(s) => Some(s),
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
}

/// Converts a borrowed field value into a [`NoteFieldValue`].
///
/// Handles note-specific post-classification: empty strings become null,
/// wikilink syntax becomes [`NoteFieldValue::Link`].
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
            let dur_val = NoteFieldValue::Duration("4h".to_owned());
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
}
