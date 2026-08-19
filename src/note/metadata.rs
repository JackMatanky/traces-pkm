//! Frontmatter and inline-field metadata values.
//!
//! [`RawFrontmatter`] preserves source YAML. [`Frontmatter`] stores parsed YAML
//! key-value pairs. [`InlineField`] records body metadata together with the
//! [`InlineFieldForm`] that produced it.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use tracing::warn;
use yaml_serde as serde_yaml;

use super::Link;
use crate::field::{FieldKey, FieldKeyError};

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
/// Converts raw YAML into a list of [`MetadataField`] entries. Malformed or
/// non-mapping YAML produces an empty frontmatter after logging the parse
/// failure.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
pub struct Frontmatter {
    fields: Vec<MetadataField>,
}

impl Frontmatter {
    /// Creates frontmatter from parsed metadata fields.
    #[inline]
    #[must_use]
    pub(crate) const fn new(fields: Vec<MetadataField>) -> Self {
        Self {
            fields,
        }
    }

    /// Returns the parsed frontmatter fields.
    #[inline]
    #[must_use]
    pub(crate) fn fields(&self) -> &[MetadataField] {
        &self.fields
    }

    /// Returns the value of the field matching `key`, if present.
    #[inline]
    #[must_use]
    #[expect(
        dead_code,
        reason = "no current caller in production or tests; kept as a \
                  general-purpose accessor alongside `fields()` — the \
                  file-field label-resolution consumer that used it was \
                  removed by the schema-query decoupling refactor, but the \
                  accessor itself is generically useful for any future \
                  frontmatter-key lookup"
    )]
    pub(crate) fn get(&self, key: &FieldKey) -> Option<&FieldValue> {
        self.fields
            .iter()
            .find(|field| field.key() == key)
            .map(MetadataField::value)
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
    pub(crate) const fn is_empty(&self) -> bool {
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
        let mut fields = Vec::with_capacity(map.len());
        for (k, v) in map {
            let Ok(key) = FieldKey::try_from(k) else {
                continue;
            };
            fields.push(MetadataField::from_key(key, FieldValue::from(v)));
        }
        Self::new(fields)
    }
}

/// The syntax form of an inline field.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub enum InlineFieldForm {
    /// `Key:: Value`, filling an entire line.
    Body,
    /// `[Key:: Value]`, with the key visible in rendered Markdown.
    VisibleKey,
    /// `(Key:: Value)`, with the key hidden in rendered Markdown.
    HiddenKey,
}

/// Key-value metadata from frontmatter or Markdown body text.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct MetadataField {
    key: FieldKey,
    value: FieldValue,
}

impl MetadataField {
    /// Creates a metadata field from an already-validated `key` and `value`.
    #[inline]
    #[must_use]
    pub(crate) const fn from_key(key: FieldKey, value: FieldValue) -> Self {
        Self {
            key,
            value,
        }
    }

    /// Parses `key` into a [`FieldKey`] and creates a metadata field.
    ///
    /// # Errors
    ///
    /// Returns a [`FieldKeyError`] if `key` fails to parse; see
    /// [`FieldKey::try_new`].
    #[cfg_attr(not(test), expect(dead_code, reason = "used in tests"))]
    pub(crate) fn try_new(
        key: impl Into<String>,
        value: FieldValue,
    ) -> Result<Self, FieldKeyError> {
        Ok(Self::from_key(FieldKey::try_new(key)?, value))
    }

    /// Returns the field key.
    #[inline]
    #[must_use]
    pub(crate) const fn key(&self) -> &FieldKey {
        &self.key
    }

    /// Returns the field value.
    #[inline]
    #[must_use]
    pub(crate) const fn value(&self) -> &FieldValue {
        &self.value
    }
}

/// A `Key:: Value` inline field with its source syntax.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct InlineField {
    metadata: MetadataField,
    form: InlineFieldForm,
}

impl InlineField {
    /// Creates an inline field from an already-validated `key`, `value`, and
    /// source syntax.
    #[inline]
    #[must_use]
    pub(crate) const fn from_key(
        key: FieldKey,
        value: FieldValue,
        form: InlineFieldForm,
    ) -> Self {
        Self {
            metadata: MetadataField::from_key(key, value),
            form,
        }
    }

    /// Parses `key` into a [`FieldKey`] and creates an inline field.
    ///
    /// # Errors
    ///
    /// Returns a [`FieldKeyError`] if `key` fails to parse; see
    /// [`FieldKey::try_new`].
    #[cfg_attr(not(test), expect(dead_code, reason = "used in tests"))]
    pub(crate) fn try_new(
        key: impl Into<String>,
        value: FieldValue,
        form: InlineFieldForm,
    ) -> Result<Self, FieldKeyError> {
        Ok(Self::from_key(FieldKey::try_new(key)?, value, form))
    }

    /// Returns the field key.
    #[inline]
    #[must_use]
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "no current caller outside tests; kept for InlineField \
                      accessor symmetry with its embedded MetadataField"
        )
    )]
    pub(crate) const fn key(&self) -> &FieldKey {
        self.metadata.key()
    }

    /// Returns the field value.
    #[inline]
    #[must_use]
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "no current caller outside tests; kept for InlineField \
                      accessor symmetry with its embedded MetadataField"
        )
    )]
    pub(crate) const fn value(&self) -> &FieldValue {
        self.metadata.value()
    }

    /// Returns the inline field's source form (body, visible key, or hidden
    /// key).
    #[inline]
    #[must_use]
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "no current caller outside tests; kept for InlineField \
                      accessor symmetry with its embedded MetadataField"
        )
    )]
    pub(crate) const fn form(&self) -> InlineFieldForm {
        self.form
    }

    /// Returns the underlying key-value metadata without syntax information.
    #[inline]
    #[must_use]
    pub(crate) const fn metadata(&self) -> &MetadataField {
        &self.metadata
    }
}

/// A metadata value parsed from YAML frontmatter or inline field text.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub enum FieldValue {
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
    Object(BTreeMap<String, Self>),
}

impl FieldValue {
    /// Returns the inner text for [`FieldValue::String`], [`FieldValue::Date`],
    /// and [`FieldValue::Duration`] variants, or `None` for any other kind.
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
/// [`FieldValue::Link`], an ISO date prefix becomes [`FieldValue::Date`], and
/// anything else stays [`FieldValue::String`].
impl From<serde_yaml::Value> for FieldValue {
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
                let mut btree = BTreeMap::new();
                for (k, v) in map {
                    let Some(key) = yaml_payload_key_to_string(k) else {
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

/// Coerces a YAML scalar key into a nested [`FieldValue::Object`] payload key.
///
/// Returns `None` for YAML values that cannot stand as keys: `Null`,
/// `Sequence`, `Mapping`, and `Tagged`. Callers skip those entries rather than
/// failing the whole document.
///
/// Top-level frontmatter keys use [`FieldKey`] instead; this helper is only for
/// keys nested inside a [`FieldValue::Object`] payload, which are structure,
/// not queryable field identity.
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
                .find(|f| f.key().is_canonical_match("title"))
                .expect("title");
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
                    FieldValue::Link(Link::new(
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
            let field = InlineField::try_new(
                "Author",
                FieldValue::String("Jane Doe".to_owned()),
                form,
            )
            .expect("valid test field key");

            assert_eq!(field.key().name(), "Author");
            assert_eq!(field.key().canonical(), "author");
            assert_eq!(
                field.value(),
                &FieldValue::String("Jane Doe".to_owned())
            );
            assert_eq!(field.form(), form);
        }

        #[rstest]
        #[case::body(InlineFieldForm::Body)]
        #[case::visible_key(InlineFieldForm::VisibleKey)]
        #[case::hidden_key(InlineFieldForm::HiddenKey)]
        fn round_trips_through_postcard_encoding(
            #[case] form: InlineFieldForm,
        ) {
            let field = InlineField::try_new(
                "Author",
                FieldValue::String("Jane Doe".to_owned()),
                form,
            )
            .expect("valid test field key");

            let bytes =
                postcard::to_allocvec(&field).expect("encode inline field");
            let decoded: InlineField =
                postcard::from_bytes(&bytes).expect("decode inline field");

            assert_eq!(decoded, field);
        }
    }
}
