//! Metadata field values parsed from YAML frontmatter and inline field text.
//!
//! [`NoteFieldValue`] represents strongly typed values extracted from Markdown
//! notes, including scalars (booleans, numbers, strings, dates, durations),
//! links, lists, and objects.

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use yaml_serde as serde_yaml;

use super::Link;

/// A metadata value parsed from YAML frontmatter or inline field text.
///
/// Supports nulls, booleans, numbers, strings, ISO dates, duration literals,
/// wikilinks, ordered lists, and nested objects.
///
/// # Examples
///
/// ```rust
/// # #[cfg(feature = "test-utils")]
/// # {
/// use traces_pkm::NoteFieldValue;
///
/// let val = NoteFieldValue::String("Draft".to_owned());
/// assert_eq!(val.as_str(), Some("Draft"));
/// # }
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
    /// ISO `YYYY-MM-DD` date string.
    Date(String),
    /// Duration literal in source spelling, such as `4h15m` or `4 yrs, 6 wks`.
    Duration(String),
    /// A link parsed from wikilink or Markdown link syntax.
    Link(Link),
    /// Ordered list value.
    List(Vec<Self>),
    /// Keyed object value stored in a deterministically ordered map.
    Object(IndexMap<String, Self>),
}

impl NoteFieldValue {
    /// Returns the inner text for [`NoteFieldValue::String`],
    /// [`NoteFieldValue::Date`], and [`NoteFieldValue::Duration`] variants,
    /// or `None` for any other kind.
    #[inline]
    #[must_use]
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(s) | Self::Date(s) | Self::Duration(s) => Some(s),
            _ => None,
        }
    }
}

/// Converts YAML scalars, sequences, mappings, and tags into metadata values.
///
/// Plain strings are classified further: wikilink syntax becomes
/// [`NoteFieldValue::Link`], an ISO date prefix becomes
/// [`NoteFieldValue::Date`], and anything else stays
/// [`NoteFieldValue::String`].
impl From<serde_yaml::Value> for NoteFieldValue {
    #[inline]
    fn from(val: serde_yaml::Value) -> Self {
        match val {
            serde_yaml::Value::Null => Self::Null,
            serde_yaml::Value::Bool(b) => Self::Bool(b),
            serde_yaml::Value::Number(n) => n.as_f64().map_or_else(
                || {
                    n.as_i64().map_or(Self::Null, |i| {
                        #[expect(
                            clippy::as_conversions,
                            clippy::cast_precision_loss,
                            reason = "YAML integer numbers converted to f64"
                        )]
                        Self::Number(i as f64)
                    })
                },
                Self::Number,
            ),
            serde_yaml::Value::String(s) => {
                let trimmed = s.trim();
                if trimmed.is_empty() {
                    Self::Null
                } else if let Some(link) = Link::parse_wikilink(trimmed) {
                    Self::Link(link)
                } else if is_iso_date(trimmed) {
                    Self::Date(s)
                } else {
                    Self::String(s)
                }
            }
            serde_yaml::Value::Sequence(seq) => {
                Self::List(seq.into_iter().map(Self::from).collect())
            }
            serde_yaml::Value::Mapping(map) => {
                let mut index_map = IndexMap::new();
                for (k, v) in map {
                    let Some(key) = yaml_payload_key_to_string(k) else {
                        continue;
                    };
                    index_map.insert(key, Self::from(v));
                }
                Self::Object(index_map)
            }
            serde_yaml::Value::Tagged(tagged) => Self::from(tagged.value),
        }
    }
}

/// Coerces a YAML scalar key into a nested [`NoteFieldValue::Object`] payload
/// key.
///
/// Returns `None` for YAML values that cannot stand as keys: `Null`,
/// `Sequence`, `Mapping`, and `Tagged`. Callers skip those entries rather than
/// failing the whole document.
pub(super) fn yaml_payload_key_to_string(
    key: serde_yaml::Value,
) -> Option<String> {
    match key {
        serde_yaml::Value::String(s) => Some(s),
        serde_yaml::Value::Number(n) => Some(n.to_string()),
        serde_yaml::Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

/// Checks whether `s` starts with an ISO date format `YYYY-MM-DD`.
pub(crate) fn is_iso_date(s: &str) -> bool {
    let bytes = s.as_bytes();
    bytes.len() >= 10
        && bytes.get(0..4).is_some_and(|b| b.iter().all(u8::is_ascii_digit))
        && bytes.get(4) == Some(&b'-')
        && bytes.get(5..7).is_some_and(|b| b.iter().all(u8::is_ascii_digit))
        && bytes.get(7) == Some(&b'-')
        && bytes.get(8..10).is_some_and(|b| b.iter().all(u8::is_ascii_digit))
}

#[cfg(test)]
mod tests {
    use super::*;

    mod yaml_key_conversion {
        use super::*;

        #[test]
        fn converts_number_key_to_string() {
            let key = serde_yaml::Value::Number(42.into());
            assert_eq!(yaml_payload_key_to_string(key), Some("42".to_owned()));
        }

        #[test]
        fn converts_bool_key_to_string() {
            let key = serde_yaml::Value::Bool(true);
            assert_eq!(
                yaml_payload_key_to_string(key),
                Some("true".to_owned())
            );
        }
    }

    mod is_iso_date {
        use super::*;

        #[test]
        fn accepts_valid_date() {
            assert!(is_iso_date("2026-08-22"));
        }
        #[test]
        fn rejects_date_without_dashes() {
            assert!(!is_iso_date("20260822"));
        }
        #[test]
        fn rejects_short_string() {
            assert!(!is_iso_date("2026-08"));
        }
        #[test]
        fn rejects_non_digit_in_year() {
            assert!(!is_iso_date("abcd-08-22"));
        }
        #[test]
        fn rejects_non_digit_in_month() {
            assert!(!is_iso_date("2026-ab-22"));
        }
        #[test]
        fn rejects_non_digit_in_day() {
            assert!(!is_iso_date("2026-08-cd"));
        }

        #[test]
        fn rejects_correct_length_but_missing_first_dash() {
            assert!(!is_iso_date("202608-22"));
        }

        #[test]
        fn rejects_correct_length_but_missing_second_dash() {
            assert!(!is_iso_date("2026-0822"));
        }
    }

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
                NoteFieldValue::from(yaml),
                NoteFieldValue::Object(IndexMap::from_iter([
                    ("bool".to_owned(), NoteFieldValue::Bool(true)),
                    (
                        "date".to_owned(),
                        NoteFieldValue::Date("2026-07-29".to_owned())
                    ),
                    (
                        "list".to_owned(),
                        NoteFieldValue::List(vec![
                            NoteFieldValue::Number(1.0),
                            NoteFieldValue::Number(2.0)
                        ]),
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
                NoteFieldValue::from(yaml),
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
                NoteFieldValue::from(yaml),
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
        use super::*;

        #[test]
        fn as_str_returns_inner_str_for_string_date_and_duration() {
            let str_val = NoteFieldValue::String("text".to_owned());
            let date_val = NoteFieldValue::Date("2026-09-02".to_owned());
            let dur_val = NoteFieldValue::Duration("4h".to_owned());

            assert_eq!(str_val.as_str(), Some("text"));
            assert_eq!(date_val.as_str(), Some("2026-09-02"));
            assert_eq!(dur_val.as_str(), Some("4h"));
        }

        #[test]
        fn as_str_returns_none_for_non_string_variants() {
            assert_eq!(NoteFieldValue::Null.as_str(), None);
            assert_eq!(NoteFieldValue::Bool(true).as_str(), None);
            assert_eq!(NoteFieldValue::Number(42.0).as_str(), None);
            assert_eq!(NoteFieldValue::List(Vec::new()).as_str(), None);
            assert_eq!(NoteFieldValue::Object(IndexMap::new()).as_str(), None);
        }
    }
}
