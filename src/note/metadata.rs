//! Frontmatter and Dataview-compatible inline metadata values.
//!
//! [`RawFrontmatter`] stores source YAML. [`Frontmatter`] and
//! [`MetadataField`] store parsed key-value pairs. [`InlineField`] records the
//! [`InlineFieldForm`] used in Markdown body text.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use tracing::warn;
use yaml_serde as serde_yaml;

use super::Outlink;

/// Raw YAML frontmatter block from a Markdown note.
#[derive(Clone, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
pub(crate) struct RawFrontmatter(String);

impl RawFrontmatter {
    /// Stores unparsed frontmatter text.
    #[inline]
    #[must_use]
    pub(crate) fn new(raw: impl Into<String>) -> Self {
        Self(raw.into())
    }

    /// YAML text between frontmatter delimiters.
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

/// Structured key-value metadata parsed from [`RawFrontmatter`].
#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
pub(crate) struct Frontmatter {
    fields: Vec<MetadataField>,
}

impl Frontmatter {
    /// Creates frontmatter from parsed metadata fields.
    #[inline]
    #[must_use]
    pub(crate) fn new(fields: Vec<MetadataField>) -> Self {
        Self {
            fields,
        }
    }

    /// Parsed frontmatter fields.
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
            let Some(key) = yaml_key_to_string(k) else {
                continue;
            };
            fields.push(MetadataField::new(key, FieldValue::from(v)));
        }
        Self::new(fields)
    }
}

/// Dataview-compatible inline field syntax.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub(crate) enum InlineFieldForm {
    /// `Key:: Value`, filling an entire line.
    Body,
    /// `[Key:: Value]`, with the key visible in rendered Markdown.
    VisibleKey,
    /// `(Key:: Value)`, with the key hidden in rendered Markdown.
    HiddenKey,
}

/// Field key shared by [`MetadataField`] and [`InlineField`] — the key half
/// of a Dataview-compatible `Key:: Value` pair, distinct from the many other
/// bare `String`s this module handles (list item text, link targets, tag
/// text).
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub(crate) struct FieldKey(String);

impl FieldKey {
    /// Creates a field key from `text`.
    #[inline]
    #[must_use]
    fn new(text: impl Into<String>) -> Self {
        Self(text.into())
    }

    /// Field key text.
    #[inline]
    #[must_use]
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

impl PartialEq<str> for FieldKey {
    #[inline]
    fn eq(&self, other: &str) -> bool {
        self.0 == other
    }
}

/// Key-value metadata from frontmatter or Markdown body text.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub(crate) struct MetadataField {
    key: FieldKey,
    value: FieldValue,
}

impl MetadataField {
    /// Creates a metadata field from `key` and `value`.
    #[inline]
    #[must_use]
    pub(crate) fn new(key: impl Into<String>, value: FieldValue) -> Self {
        Self {
            key: FieldKey::new(key),
            value,
        }
    }

    /// Field key.
    #[inline]
    #[must_use]
    pub(crate) fn key(&self) -> &FieldKey {
        &self.key
    }

    /// Field value.
    #[inline]
    #[must_use]
    pub(crate) fn value(&self) -> &FieldValue {
        &self.value
    }
}

/// Dataview-compatible inline field with its source syntax.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub(crate) struct InlineField {
    #[serde(flatten)]
    metadata: MetadataField,
    form: InlineFieldForm,
}

impl InlineField {
    /// Creates an inline field.
    ///
    /// # Arguments
    ///
    /// * `key` - Field key.
    /// * `value` - Field value.
    /// * `form` - Inline field syntax.
    #[inline]
    #[must_use]
    pub(crate) fn new(
        key: impl Into<String>,
        value: FieldValue,
        form: InlineFieldForm,
    ) -> Self {
        Self {
            metadata: MetadataField::new(key, value),
            form,
        }
    }

    /// Field key.
    #[inline]
    #[must_use]
    pub(crate) fn key(&self) -> &FieldKey {
        self.metadata.key()
    }

    /// Field value.
    #[inline]
    #[must_use]
    pub(crate) fn value(&self) -> &FieldValue {
        self.metadata.value()
    }

    /// Inline field syntax.
    #[inline]
    #[must_use]
    pub(crate) fn form(&self) -> InlineFieldForm {
        self.form
    }

    /// Underlying key-value metadata without syntax information.
    #[inline]
    #[must_use]
    pub(crate) fn metadata(&self) -> &MetadataField {
        &self.metadata
    }
}

/// Dataview-compatible metadata value.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub(crate) enum FieldValue {
    /// Empty or missing value.
    Null,
    /// Boolean value.
    Bool(bool),
    /// Numeric value.
    Number(f64),
    /// Text value.
    String(String),
    /// ISO date string.
    Date(String),
    /// Dataview duration literal in source spelling.
    Duration(String),
    /// Link value.
    Link(Outlink),
    /// Ordered list value stored in a [`Vec`].
    List(Vec<FieldValue>),
    /// Keyed object value stored in a deterministically ordered [`BTreeMap`].
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

    /// Returns textual content for string-like values.
    #[inline]
    #[must_use]
    pub(crate) fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(s) | Self::Date(s) | Self::Duration(s) => Some(s),
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
                } else if let Some(link) = Outlink::parse_wikilink(trimmed) {
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
                let mut btree = BTreeMap::new();
                for (k, v) in map {
                    let Some(key) = yaml_key_to_string(k) else {
                        continue;
                    };
                    btree.insert(key, Self::from(v));
                }
                Self::Object(btree)
            }
            serde_yaml::Value::Tagged(tagged) => Self::from(tagged.value),
        }
    }
}

/// Coerces a YAML scalar `key` to its string representation for use as a
/// [`MetadataField`]/[`FieldValue::Object`] key.
///
/// Returns `None` for a YAML value kind that can't stand as a key
/// (`Null`, `Sequence`, `Mapping`, `Tagged`); callers skip that entry
/// rather than failing the whole document.
fn yaml_key_to_string(key: serde_yaml::Value) -> Option<String> {
    match key {
        serde_yaml::Value::String(s) => Some(s),
        serde_yaml::Value::Number(n) => Some(n.to_string()),
        serde_yaml::Value::Bool(b) => Some(b.to_string()),
        _ => None,
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

        #[test]
        fn converts_wikilink_strings_into_link_values() {
            let yaml = serde_yaml::from_str::<serde_yaml::Value>(
                r#"
                link: "[[Project Alpha|Alpha]]"
                "#,
            )
            .expect("valid yaml");

            assert_eq!(
                FieldValue::from(yaml),
                FieldValue::Object(BTreeMap::from([(
                    "link".to_owned(),
                    FieldValue::Link(Outlink::new(
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
                FieldValue::from(yaml),
                FieldValue::Object(BTreeMap::from([(
                    "outer".to_owned(),
                    FieldValue::Object(BTreeMap::from([(
                        "inner".to_owned(),
                        FieldValue::String("value".to_owned())
                    )]))
                )]))
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
            let field = InlineField::new(
                "Author",
                FieldValue::String("Jane Doe".to_owned()),
                form,
            );

            assert_eq!(field.key(), "Author");
            assert_eq!(
                field.value(),
                &FieldValue::String("Jane Doe".to_owned())
            );
            assert_eq!(field.form(), form);
        }
    }
}
