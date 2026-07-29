//! Metadata domain types: `RawFrontmatter`, `Frontmatter`, `MetadataField`,
//! `FieldValue`, `FieldSource`, and `InlineFieldForm`.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use tracing::warn;
use yaml_serde as serde_yaml;

use super::structure::Outlink;

/// Unparsed raw YAML frontmatter text block extracted from a markdown Note.
#[derive(Clone, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
pub(crate) struct RawFrontmatter(String);

impl RawFrontmatter {
    /// Creates a new [`RawFrontmatter`] instance.
    #[inline]
    #[must_use]
    pub(crate) fn new(raw: impl Into<String>) -> Self {
        Self(raw.into())
    }

    /// Raw YAML content of the frontmatter block.
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

/// Structured frontmatter metadata parsed from a [`RawFrontmatter`] block.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
pub(crate) struct Frontmatter {
    fields: Vec<MetadataField>,
}

impl Frontmatter {
    /// Creates a new [`Frontmatter`] with the given structured fields.
    #[inline]
    #[must_use]
    pub(crate) fn new(fields: Vec<MetadataField>) -> Self {
        Self {
            fields,
        }
    }

    /// Structured key-value fields parsed from frontmatter.
    #[inline]
    #[must_use]
    pub(crate) fn fields(&self) -> &[MetadataField] {
        &self.fields
    }

    /// Returns `true` if this frontmatter block contains no structured fields.
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
/// Dataview-compatible Inline Field syntax form in body text.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub(crate) enum InlineFieldForm {
    /// `Key:: Value` filling an entire line.
    Body,
    /// `[Key:: Value]` — the key stays visible in rendered text.
    VisibleKey,
    /// `(Key:: Value)` — the key is hidden in rendered text.
    HiddenKey,
}

/// Origin source of a [`MetadataField`].
#[derive(Copy, Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub(crate) enum FieldSource {
    Frontmatter,
    Body(InlineFieldForm),
}

/// A key-value metadata field extracted from frontmatter or note body.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub(crate) struct MetadataField {
    key: String,
    value: FieldValue,
    source: FieldSource,
}

impl MetadataField {
    /// Creates a new [`MetadataField`].
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

    /// The field's key.
    #[inline]
    #[must_use]
    pub(crate) fn key(&self) -> &str {
        &self.key
    }

    /// The field's value.
    #[inline]
    #[must_use]
    pub(crate) fn value(&self) -> &FieldValue {
        &self.value
    }

    /// Origin source of this field.
    #[inline]
    #[must_use]
    pub(crate) fn source(&self) -> FieldSource {
        self.source
    }

    /// Syntax form if this field is from body inline field syntax.
    #[inline]
    #[must_use]
    pub(crate) fn form(&self) -> Option<InlineFieldForm> {
        match self.source {
            FieldSource::Body(form) => Some(form),
            FieldSource::Frontmatter => None,
        }
    }
}

/// Dataview-compatible metadata field value.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub(crate) enum FieldValue {
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    Date(String),
    Link(Outlink),
    List(Vec<FieldValue>),
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

#[expect(
    clippy::as_conversions,
    clippy::cast_precision_loss,
    reason = "YAML integer numbers converted to f64"
)]
impl From<serde_yaml::Value> for FieldValue {
    fn from(val: serde_yaml::Value) -> Self {
        match val {
            serde_yaml::Value::Null => Self::Null,
            serde_yaml::Value::Bool(b) => Self::Bool(b),
            serde_yaml::Value::Number(n) => {
                if let Some(f) = n.as_f64() {
                    Self::Number(f)
                } else if let Some(i) = n.as_i64() {
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
fn is_iso_date(s: &str) -> bool {
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
            assert_eq!(title.value(), &FieldValue::String("Test".to_string()));
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

            let field_val = FieldValue::from(yaml);
            if let FieldValue::Object(map) = field_val {
                assert_eq!(
                    map.get("str"),
                    Some(&FieldValue::String("hello".to_string()))
                );
                assert_eq!(map.get("num"), Some(&FieldValue::Number(42.5)));
                assert_eq!(map.get("bool"), Some(&FieldValue::Bool(true)));
                assert_eq!(map.get("null_val"), Some(&FieldValue::Null));
                assert_eq!(
                    map.get("date"),
                    Some(&FieldValue::Date("2026-07-29".to_string()))
                );
                assert_eq!(
                    map.get("list"),
                    Some(&FieldValue::List(vec![
                        FieldValue::Number(1.0),
                        FieldValue::Number(2.0)
                    ]))
                );
            } else {
                panic!("expected FieldValue::Object");
            }
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
                FieldValue::String("Jane Doe".to_string()),
                FieldSource::Body(form),
            );

            assert_eq!(field.key(), "Author");
            assert_eq!(
                field.value(),
                &FieldValue::String("Jane Doe".to_string())
            );
            assert_eq!(field.form(), Some(form));
        }
    }
}
