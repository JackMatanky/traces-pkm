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

/// Represents a raw YAML frontmatter block from a Markdown note.
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

/// Represents structured frontmatter fields parsed from [`RawFrontmatter`].
#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
pub struct Frontmatter {
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

    /// Returns the parsed frontmatter fields.
    #[inline]
    #[must_use]
    pub(crate) fn fields(&self) -> &[MetadataField] {
        &self.fields
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

/// Distinguishes an inline field's source syntax: bare, bracket-wrapped, or
/// paren-wrapped.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub enum InlineFieldForm {
    /// `Key:: Value`, filling an entire line.
    Body,
    /// `[Key:: Value]`, with the key visible in rendered Markdown.
    VisibleKey,
    /// `(Key:: Value)`, with the key hidden in rendered Markdown.
    HiddenKey,
}

/// A metadata key shared by frontmatter and inline fields.
///
/// Stores the original key text for display and a canonical form for
/// case-insensitive, whitespace-normalized matching.
#[derive(Clone, Debug, Eq, Deserialize, Serialize)]
pub(crate) struct FieldKey {
    /// Original key text as written by the user.
    name: String,
    /// Canonical form for case-insensitive matching.
    canonical: String,
}

impl FieldKey {
    /// Creates a field key from `raw`, computing its canonical form.
    #[inline]
    #[must_use]
    pub(crate) fn new(raw: impl Into<String>) -> Self {
        let name = raw.into();
        let canonical = Self::canonicalize(&name);
        Self {
            name,
            canonical,
        }
    }

    /// Returns the original key text.
    #[inline]
    #[must_use]
    #[allow(dead_code, reason = "used in tests")]
    pub(crate) fn name(&self) -> &str {
        &self.name
    }

    /// Returns the canonical key form for matching.
    #[inline]
    #[must_use]
    #[allow(dead_code, reason = "used in tests")]
    pub(crate) fn canonical(&self) -> &str {
        &self.canonical
    }

    /// Normalizes a raw key string for case-insensitive matching.
    ///
    /// Transformations:
    /// - ASCII whitespace → `-`
    /// - `_`, `-`, ASCII alphanumeric → kept, lowercased
    /// - Non-ASCII (emoji, Unicode letters) → kept, lowercased
    /// - Everything else (`!`, `@`, `(`, etc.) → stripped
    fn canonicalize(raw: &str) -> String {
        let mut result = String::with_capacity(raw.len());
        for ch in raw.chars() {
            if ch.is_ascii_whitespace() {
                result.push('-');
                continue;
            }
            if ch == '_'
                || ch == '-'
                || ch.is_ascii_alphanumeric()
                || !ch.is_ascii()
            {
                for c in ch.to_lowercase() {
                    result.push(c);
                }
            }
            // strip everything else
        }
        result
    }
}

impl PartialEq for FieldKey {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        self.canonical == other.canonical
    }
}

impl PartialEq<str> for FieldKey {
    #[inline]
    fn eq(&self, other: &str) -> bool {
        self.canonical == Self::canonicalize(other)
    }
}

impl PartialEq<&str> for FieldKey {
    #[inline]
    fn eq(&self, other: &&str) -> bool {
        self.canonical == Self::canonicalize(other)
    }
}

/// Represents key-value metadata from frontmatter or Markdown body text.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct MetadataField {
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

    /// Returns the field key.
    #[inline]
    #[must_use]
    pub(crate) fn key(&self) -> &FieldKey {
        &self.key
    }

    /// Returns the field value.
    #[inline]
    #[must_use]
    pub(crate) fn value(&self) -> &FieldValue {
        &self.value
    }
}

/// Represents a `Key:: Value` inline field with its source syntax.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct InlineField {
    metadata: MetadataField,
    form: InlineFieldForm,
}

impl InlineField {
    /// Creates an inline field from its key, value, and source syntax.
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
    pub(crate) fn key(&self) -> &FieldKey {
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
    pub(crate) fn value(&self) -> &FieldValue {
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
    pub(crate) fn form(&self) -> InlineFieldForm {
        self.form
    }

    /// Returns the underlying key-value metadata without syntax information.
    #[inline]
    #[must_use]
    pub(crate) fn metadata(&self) -> &MetadataField {
        &self.metadata
    }
}

/// Represents a metadata value parsed from YAML frontmatter or inline field
/// text.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub enum FieldValue {
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
    /// Duration literal in source spelling, such as `4h15m` or `4 yrs, 6 wks`.
    Duration(String),
    /// Link value.
    Link(Link),
    /// Ordered list value stored in a [`Vec`].
    List(Vec<FieldValue>),
    /// Keyed object value stored in a deterministically ordered [`BTreeMap`].
    Object(BTreeMap<String, FieldValue>),
}

impl FieldValue {
    /// Returns the inner text for [`FieldValue::String`],
    /// [`FieldValue::Date`], and [`FieldValue::Duration`] variants, or
    /// `None` for any other kind.
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

/// Coerces a YAML scalar key into a metadata object key.
///
/// Returns `None` for YAML values that cannot stand as keys: `Null`,
/// `Sequence`, `Mapping`, and `Tagged`. Callers skip those entries rather than
/// failing the whole document.
fn yaml_key_to_string(key: serde_yaml::Value) -> Option<String> {
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
            let field = InlineField::new(
                "Author",
                FieldValue::String("Jane Doe".to_owned()),
                form,
            );

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
            let field = InlineField::new(
                "Author",
                FieldValue::String("Jane Doe".to_owned()),
                form,
            );

            let bytes =
                postcard::to_allocvec(&field).expect("encode inline field");
            let decoded: InlineField =
                postcard::from_bytes(&bytes).expect("decode inline field");

            assert_eq!(decoded, field);
        }
    }

    mod field_key {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn stores_original_name() {
            let key = FieldKey::new("Status");
            assert_eq!(key.name(), "Status");
        }

        #[test]
        fn computes_canonical_form() {
            let key = FieldKey::new("Time Played");
            assert_eq!(key.canonical(), "time-played");
        }

        #[test]
        fn lowercases_ascii() {
            let key = FieldKey::new("Status");
            assert_eq!(key.canonical(), "status");
        }

        #[test]
        fn replaces_whitespace_with_hyphens() {
            let key = FieldKey::new("due date");
            assert_eq!(key.canonical(), "due-date");
        }

        #[test]
        fn strips_special_characters() {
            let key = FieldKey::new("field-name!");
            assert_eq!(key.canonical(), "field-name");
        }

        #[test]
        fn preserves_underscores_and_hyphens() {
            let key = FieldKey::new("my_field-name");
            assert_eq!(key.canonical(), "my_field-name");
        }

        #[test]
        fn preserves_emoji() {
            let key = FieldKey::new("🗓️due");
            assert_eq!(key.canonical(), "🗓️due");
        }

        #[test]
        fn partial_eq_str_uses_canonical() {
            let key = FieldKey::new("Status");
            assert_eq!(key, "status");
            assert_eq!(key, "Status");
        }

        #[test]
        fn partial_eq_field_key_uses_canonical() {
            let a = FieldKey::new("Status");
            let b = FieldKey::new("status");
            assert_eq!(a, b);
        }
    }
}
