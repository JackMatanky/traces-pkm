//! Metadata extracted from frontmatter and Dataview-compatible inline fields.
//!
//! [`RawFrontmatter`] stores the source YAML block. [`Frontmatter`] and
//! [`MetadataField`] store parsed key-value metadata with a [`FieldSource`] and
//! typed [`FieldValue`].

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use tracing::warn;
use yaml_serde as serde_yaml;

use super::structure::Outlink;

/// Raw YAML frontmatter block extracted from a markdown note.
#[derive(Clone, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
pub(crate) struct RawFrontmatter(String);

impl RawFrontmatter {
    /// Stores `raw` as the unparsed frontmatter text.
    #[inline]
    #[must_use]
    pub(crate) fn new(raw: impl Into<String>) -> Self {
        Self(raw.into())
    }

    /// Raw YAML text between the frontmatter delimiters.
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

/// Structured key-value metadata parsed from a [`RawFrontmatter`] block.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
pub(crate) struct Frontmatter {
    fields: Vec<MetadataField>,
}

impl Frontmatter {
    /// Stores the structured frontmatter `fields`.
    #[inline]
    #[must_use]
    pub(crate) fn new(fields: Vec<MetadataField>) -> Self {
        Self {
            fields,
        }
    }

    /// Parsed key-value fields from frontmatter.
    #[inline]
    #[must_use]
    pub(crate) fn fields(&self) -> &[MetadataField] {
        &self.fields
    }

    /// Returns `true` if no structured fields were parsed.
    #[inline]
    #[must_use]
    pub(crate) fn is_empty(&self) -> bool {
        self.fields.is_empty()
    }
}

impl From<&RawFrontmatter> for Frontmatter {
    fn from(raw: &RawFrontmatter) -> Self {
        if raw.is_empty() {
            return Self::default();
        }
        let val = match serde_yaml::from_str::<serde_yaml::Value>(raw.as_str())
        {
            Ok(v) => v,
            Err(err) => {
                warn!(%err, "failed to parse YAML frontmatter block; ignoring malformed fields");
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
        let mut fields = Vec::with_capacity(map.len());
        for (k, v) in map {
            let key = match k {
                serde_yaml::Value::String(s) => s,
                serde_yaml::Value::Number(n) => n.to_string(),
                serde_yaml::Value::Bool(b) => b.to_string(),
                _ => continue,
            };
            fields.push(MetadataField::new(
                key,
                FieldValue::from(v),
                FieldSource::Frontmatter,
            ));
        }
        Self::new(fields)
    }
}

/// Dataview-compatible inline field syntax form.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub(crate) enum InlineFieldForm {
    /// `Key:: Value`, filling an entire line.
    Body,
    /// `[Key:: Value]`, with the key visible in rendered markdown.
    VisibleKey,
    /// `(Key:: Value)`, with the key hidden in rendered markdown.
    HiddenKey,
}

/// Source location of a [`MetadataField`].
#[derive(Copy, Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub(crate) enum FieldSource {
    /// YAML frontmatter.
    Frontmatter,
    /// Dataview inline field syntax in markdown body text.
    Body(InlineFieldForm),
}

/// Key-value metadata parsed from frontmatter or markdown body text.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub(crate) struct MetadataField {
    key: String,
    value: FieldValue,
    source: FieldSource,
}

impl MetadataField {
    /// Creates a metadata field from a key, value, and source.
    #[inline]
    #[must_use]
    pub(crate) fn new(
        key: impl Into<String>,
        value: FieldValue,
        source: FieldSource,
    ) -> Self {
        Self {
            key: key.into(),
            value,
            source,
        }
    }

    /// Metadata key.
    #[inline]
    #[must_use]
    pub(crate) fn key(&self) -> &str {
        &self.key
    }

    /// Typed metadata value.
    #[inline]
    #[must_use]
    pub(crate) fn value(&self) -> &FieldValue {
        &self.value
    }

    /// Source location of this field.
    #[inline]
    #[must_use]
    pub(crate) fn source(&self) -> FieldSource {
        self.source
    }

    /// Inline field syntax form, if this field came from body text.
    #[inline]
    #[must_use]
    pub(crate) fn form(&self) -> Option<InlineFieldForm> {
        match self.source {
            FieldSource::Body(form) => Some(form),
            FieldSource::Frontmatter => None,
        }
    }
}

/// Typed Dataview-compatible metadata value.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub(crate) enum FieldValue {
    /// Empty or missing value.
    Null,
    /// Boolean value.
    Bool(bool),
    /// Floating-point numeric value.
    Number(f64),
    /// Plain string value.
    String(String),
    /// ISO-date-like string value.
    Date(String),
    /// Link value.
    Link(Outlink),
    /// Ordered list value.
    List(Vec<FieldValue>),
    /// Keyed object value with deterministic key order.
    Object(BTreeMap<String, FieldValue>),
}

impl FieldValue {
    /// Returns `true` if this value is [`FieldValue::Null`].
    #[expect(dead_code, reason = "domain accessor for QueryOps filtering")]
    #[inline]
    #[must_use]
    pub(crate) fn is_null(&self) -> bool {
        matches!(self, Self::Null)
    }

    /// Returns the string slice if this value is [`FieldValue::String`] or
    /// [`FieldValue::Date`].
    #[inline]
    #[must_use]
    pub(crate) fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(s) | Self::Date(s) => Some(s),
            _ => None,
        }
    }
}

impl From<serde_yaml::Value> for FieldValue {
    fn from(val: serde_yaml::Value) -> Self {
        match val {
            serde_yaml::Value::Null => Self::Null,
            serde_yaml::Value::Bool(b) => Self::Bool(b),
            serde_yaml::Value::Number(n) => {
                if let Some(f) = n.as_f64() {
                    Self::Number(f)
                } else if let Some(i) = n.as_i64() {
                    #[expect(
                        clippy::as_conversions,
                        clippy::cast_precision_loss,
                        reason = "YAML integer numbers converted to f64"
                    )]
                    Self::Number(i as f64)
                } else {
                    Self::Null
                }
            }
            serde_yaml::Value::String(s) => {
                let trimmed = s.trim();
                if trimmed.is_empty() {
                    Self::Null
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
                let mut btree = BTreeMap::new();
                for (k, v) in map {
                    let key = match k {
                        serde_yaml::Value::String(s) => s,
                        serde_yaml::Value::Number(n) => n.to_string(),
                        serde_yaml::Value::Bool(b) => b.to_string(),
                        _ => continue,
                    };
                    btree.insert(key, Self::from(v));
                }
                Self::Object(btree)
            }
            serde_yaml::Value::Tagged(tagged) => Self::from(tagged.value),
        }
    }
}

/// Returns `true` if `s` starts with an ISO date format `YYYY-MM-DD`.
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
            let title =
                fm.fields().iter().find(|f| f.key() == "title").expect("title");
            assert_eq!(title.value(), &FieldValue::String("Test".to_owned()));
            assert_eq!(title.source(), FieldSource::Frontmatter);
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
                FieldValue::from(yaml),
                FieldValue::Object(BTreeMap::from([
                    ("bool".to_owned(), FieldValue::Bool(true)),
                    (
                        "date".to_owned(),
                        FieldValue::Date("2026-07-29".to_owned())
                    ),
                    (
                        "list".to_owned(),
                        FieldValue::List(vec![
                            FieldValue::Number(1.0),
                            FieldValue::Number(2.0)
                        ]),
                    ),
                    ("null_val".to_owned(), FieldValue::Null),
                    ("num".to_owned(), FieldValue::Number(42.5)),
                    ("str".to_owned(), FieldValue::String("hello".to_owned())),
                ]))
            );
        }
    }

    mod inline_field {
        use pretty_assertions::assert_eq;
        use rstest::rstest;

        use super::*;

        #[rstest]
        #[case::body(InlineFieldForm::Body)]
        #[case::visible_key(InlineFieldForm::VisibleKey)]
        #[case::hidden_key(InlineFieldForm::HiddenKey)]
        fn stores_key_value_and_form(#[case] form: InlineFieldForm) {
            let field = MetadataField::new(
                "Author",
                FieldValue::String("Jane Doe".to_owned()),
                FieldSource::Body(form),
            );

            assert_eq!(field.key(), "Author");
            assert_eq!(
                field.value(),
                &FieldValue::String("Jane Doe".to_owned())
            );
            assert_eq!(field.form(), Some(form));
        }
    }
}
