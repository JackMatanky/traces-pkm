//! Frontmatter and inline-field metadata values.
//!
//! [`RawFrontmatter`] preserves source YAML. [`Frontmatter`] stores parsed YAML
//! key-value pairs.

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use tracing::warn;
use yaml_serde as serde_yaml;

use super::Link;
use crate::field::FieldKey;

/// Raw YAML frontmatter text from a Markdown note.
///
/// Preserves the unparsed YAML between frontmatter delimiters (`---`).
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RawFrontmatter(String);

impl RawFrontmatter {
    /// Stores unparsed frontmatter text.
    #[inline]
    #[must_use]
    pub(crate) fn new(raw: impl Into<String>) -> Self {
        Self(raw.into())
    }

    /// Returns the YAML text between frontmatter delimiters.
    #[inline]
    #[must_use]
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }

    /// Returns `true` if the raw frontmatter text is empty or whitespace-only.
    #[inline]
    #[must_use]
    pub(crate) fn is_empty(&self) -> bool {
        self.0.trim().is_empty()
    }
}

/// Structured frontmatter fields parsed from [`RawFrontmatter`].
///
/// Converts raw YAML into an [`IndexMap`] of field key-value pairs. Malformed
/// or non-mapping YAML produces an empty frontmatter after logging the parse
/// failure.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
pub struct Frontmatter {
    fields: IndexMap<FieldKey, NoteFieldValue>,
}

impl Frontmatter {
    /// Creates frontmatter from parsed metadata fields.
    #[inline]
    #[must_use]
    pub(crate) fn new(fields: IndexMap<FieldKey, NoteFieldValue>) -> Self {
        Self {
            fields,
        }
    }

    /// Returns the parsed frontmatter fields.
    #[inline]
    #[must_use]
    pub(crate) fn fields(&self) -> &IndexMap<FieldKey, NoteFieldValue> {
        &self.fields
    }

    /// Returns the value of the field matching `key`, if present.
    #[inline]
    #[must_use]
    pub(crate) fn get(&self, key: &FieldKey) -> Option<&NoteFieldValue> {
        self.fields.get(key)
    }

    /// Returns `true` if no structured fields were parsed.
    #[inline]
    #[must_use]
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "no current caller outside tests; kept for Frontmatter \
                      accessor symmetry with its fields"
        )
    )]
    pub(crate) fn is_empty(&self) -> bool {
        self.fields.is_empty()
    }
}

/// Converts raw YAML frontmatter into structured fields.
///
/// Empty, malformed, or non-mapping frontmatter becomes
/// [`Frontmatter::default`] after logging parse failures.
impl From<&RawFrontmatter> for Frontmatter {
    #[inline]
    fn from(raw: &RawFrontmatter) -> Self {
        if raw.is_empty() {
            return Self::default();
        }
        let val = match serde_yaml::from_str::<serde_yaml::Value>(raw.as_str())
        {
            Ok(v) => v,
            Err(err) => {
                warn!(
                    %err,
                    "failed to parse YAML frontmatter block; \
                    ignoring malformed fields"
                );
                return Self::default();
            }
        };
        let serde_yaml::Value::Mapping(map) = val else {
            warn!(
                "YAML frontmatter is not a key-value mapping; ignoring \
                 top-level value"
            );
            return Self::default();
        };
        let mut fields = IndexMap::new();
        for (raw_key, raw_value) in map {
            let Some(key_str) = yaml_payload_key_to_string(raw_key) else {
                continue;
            };
            let Ok(key) = FieldKey::try_new(key_str) else {
                continue;
            };
            fields.insert(key, NoteFieldValue::from(raw_value));
        }
        Self::new(fields)
    }
}

/// A metadata value parsed from YAML frontmatter or inline field text.
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
    pub(crate) fn as_str(&self) -> Option<&str> {
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
///
/// Top-level frontmatter keys use [`FieldKey`] instead; this helper is only for
/// keys nested inside a [`NoteFieldValue::Object`] payload, which are
/// structure, not queryable field identity.
fn yaml_payload_key_to_string(key: serde_yaml::Value) -> Option<String> {
    match key {
        serde_yaml::Value::String(s) => Some(s),
        serde_yaml::Value::Number(n) => Some(n.to_string()),
        serde_yaml::Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

/// Checks whether `s` starts with an ISO date format `YYYY-MM-DD`.
pub(super) fn is_iso_date(s: &str) -> bool {
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

    mod raw_frontmatter {
        use pretty_assertions::assert_eq;

        use super::*;
        #[test]
        fn parses_valid_yaml_into_structured_frontmatter() {
            let raw = RawFrontmatter::new("title: Test\ndraft: true\n");
            let fm = Frontmatter::from(&raw);

            assert_eq!(fm.fields().len(), 2);
            let title = fm
                .fields()
                .iter()
                .find(|(k, _)| k.is_canonical_match("title"))
                .expect("title");
            assert_eq!(title.1, &NoteFieldValue::String("Test".to_owned()));
        }

        #[test]
        fn converts_empty_raw_frontmatter_to_empty_frontmatter() {
            let raw = RawFrontmatter::new("  \n");
            let fm = Frontmatter::from(&raw);

            assert_eq!(fm.is_empty(), true);
        }

        #[test]
        fn handles_malformed_yaml_resiliently_with_empty_fields() {
            let raw = RawFrontmatter::new("invalid: [yaml: :");
            let fm = Frontmatter::from(&raw);

            assert_eq!(fm.is_empty(), true);
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
}
