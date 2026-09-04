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
            Self::String(s) | Self::Date(s) | Self::Duration(s) => Some(s),
            _ => None,
        }
    }

    /// Returns the parsed calendar date if this value is
    /// [`NoteFieldValue::Date`] or a [`NoteFieldValue::String`] beginning
    /// with a valid `YYYY-MM-DD` ISO date, or `None` otherwise.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use chrono::NaiveDate;
    /// use traces_pkm::NoteFieldValue;
    ///
    /// let date_val = NoteFieldValue::Date("2025-01-15".to_owned());
    /// assert_eq!(date_val.as_date(), NaiveDate::from_ymd_opt(2025, 1, 15));
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
            Self::Date(s) | Self::String(s) if s.len() >= 10 => {
                chrono::NaiveDate::parse_from_str(&s[..10], "%Y-%m-%d").ok()
            }
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

/// Converts a duration unit string into seconds.
fn unit_to_seconds(unit: &str) -> Option<f64> {
    if unit.eq_ignore_ascii_case("ms")
        || unit.eq_ignore_ascii_case("millisecond")
        || unit.eq_ignore_ascii_case("milliseconds")
    {
        Some(0.001)
    } else if unit.eq_ignore_ascii_case("s")
        || unit.eq_ignore_ascii_case("sec")
        || unit.eq_ignore_ascii_case("secs")
        || unit.eq_ignore_ascii_case("second")
        || unit.eq_ignore_ascii_case("seconds")
    {
        Some(1.0)
    } else if unit.eq_ignore_ascii_case("m")
        || unit.eq_ignore_ascii_case("min")
        || unit.eq_ignore_ascii_case("mins")
        || unit.eq_ignore_ascii_case("minute")
        || unit.eq_ignore_ascii_case("minutes")
    {
        Some(60.0)
    } else if unit.eq_ignore_ascii_case("h")
        || unit.eq_ignore_ascii_case("hr")
        || unit.eq_ignore_ascii_case("hrs")
        || unit.eq_ignore_ascii_case("hour")
        || unit.eq_ignore_ascii_case("hours")
    {
        Some(3600.0)
    } else if unit.eq_ignore_ascii_case("d")
        || unit.eq_ignore_ascii_case("day")
        || unit.eq_ignore_ascii_case("days")
    {
        Some(86_400.0)
    } else if unit.eq_ignore_ascii_case("w")
        || unit.eq_ignore_ascii_case("wk")
        || unit.eq_ignore_ascii_case("wks")
        || unit.eq_ignore_ascii_case("week")
        || unit.eq_ignore_ascii_case("weeks")
    {
        Some(604_800.0)
    } else if unit.eq_ignore_ascii_case("mo")
        || unit.eq_ignore_ascii_case("mos")
        || unit.eq_ignore_ascii_case("month")
        || unit.eq_ignore_ascii_case("months")
    {
        Some(2_592_000.0)
    } else if unit.eq_ignore_ascii_case("yr")
        || unit.eq_ignore_ascii_case("yrs")
        || unit.eq_ignore_ascii_case("year")
        || unit.eq_ignore_ascii_case("years")
    {
        Some(31_536_000.0)
    } else {
        None
    }
}

/// Parses a duration spelling into its total duration in seconds.
///
/// Supports `<number><unit>` components separated by spaces and/or commas
/// (for example: `1h`, `30m`, `1h 30m`, `4 yrs, 6 wks`). Returns `None` if
/// unparseable or empty.
#[inline]
#[must_use]
pub fn duration_seconds(spelling: &str) -> Option<f64> {
    let bytes = spelling.as_bytes();
    let len = bytes.len();
    let mut pos = 0;
    let mut total = 0.0;
    let mut parsed_any = false;

    while pos < len {
        while pos < len
            && bytes
                .get(pos)
                .is_some_and(|&b| b.is_ascii_whitespace() || b == b',')
        {
            pos = pos.saturating_add(1);
        }
        if pos >= len {
            break;
        }

        let num_start = pos;
        let mut has_decimal = false;
        while pos < len {
            if bytes.get(pos).is_some_and(u8::is_ascii_digit) {
                pos = pos.saturating_add(1);
            } else if bytes.get(pos) == Some(&b'.') && !has_decimal {
                has_decimal = true;
                pos = pos.saturating_add(1);
            } else {
                break;
            }
        }
        if num_start == pos {
            return None;
        }
        let num_slice = bytes.get(num_start..pos)?;
        let num_str = std::str::from_utf8(num_slice).ok()?;
        let number: f64 = num_str.parse().ok()?;
        if !number.is_finite() {
            return None;
        }

        while pos < len && bytes.get(pos).is_some_and(u8::is_ascii_whitespace) {
            pos = pos.saturating_add(1);
        }
        let unit_start = pos;
        while pos < len && bytes.get(pos).is_some_and(u8::is_ascii_alphabetic) {
            pos = pos.saturating_add(1);
        }
        if unit_start == pos {
            return None;
        }
        let unit_slice = bytes.get(unit_start..pos)?;
        let unit_str = std::str::from_utf8(unit_slice).ok()?;
        let multiplier = unit_to_seconds(unit_str)?;
        total += number * multiplier;
        parsed_any = true;
    }

    if parsed_any {
        Some(total)
    } else {
        None
    }
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

    mod duration_parsing {
        use super::*;

        #[test]
        fn parses_single_and_multi_part_durations() {
            assert_eq!(duration_seconds("1h"), Some(3600.0));
            assert_eq!(duration_seconds("30m"), Some(1800.0));
            assert_eq!(duration_seconds("1h 30m"), Some(5400.0));
            assert_eq!(
                duration_seconds("4 yrs, 6 wks"),
                Some(4.0 * 31_536_000.0 + 6.0 * 604_800.0)
            );
            assert_eq!(duration_seconds("1.5h"), Some(5400.0));
            assert_eq!(duration_seconds("10s"), Some(10.0));
            assert_eq!(duration_seconds("500ms"), Some(0.5));
        }

        #[test]
        fn rejects_unparseable_durations() {
            assert_eq!(duration_seconds(""), None);
            assert_eq!(duration_seconds("   "), None);
            assert_eq!(duration_seconds("invalid"), None);
            assert_eq!(duration_seconds("1h invalid"), None);
            assert_eq!(duration_seconds("foo 30m"), None);
        }
    }
}
