//! YAML frontmatter parsing and key-value metadata storage.
//!
//! [`RawFrontmatter`] preserves unparsed source YAML. [`Frontmatter`] stores
//! parsed YAML key-value pairs mapping [`FieldKey`] to [`NoteFieldValue`]
//! values.
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use tracing::warn;
use yaml_serde as serde_yaml;

use super::field::{NoteFieldValue, yaml_payload_key_to_string};
use crate::{FieldKey, FieldKeyRef};

/// Raw YAML frontmatter text from a Markdown note.
///
/// Preserves the unparsed YAML between frontmatter delimiters (`---`).
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct RawFrontmatter(String);

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

/// Structured frontmatter fields parsed from `RawFrontmatter`.
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
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "called by Note::fields in test builds; retained for \
                      accessor symmetry"
        )
    )]
    pub(crate) fn fields(&self) -> &IndexMap<FieldKey, NoteFieldValue> {
        &self.fields
    }

    /// Returns the value of the field matching `key` by string lookup, if
    /// present.
    #[inline]
    #[must_use]
    pub(crate) fn get(&self, key: &str) -> Option<&NoteFieldValue> {
        self.fields.get(&FieldKeyRef::new(key))
    }

    /// Returns the value of the field matching `key`, if present.
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
    pub(crate) fn get_by_key(&self, key: &FieldKey) -> Option<&NoteFieldValue> {
        self.fields.get(key)
    }

    /// Returns a flat iterator over the scalar value or list elements of the
    /// field matching `key` by string lookup, if present.
    pub(crate) fn get_values(
        &self,
        key: &str,
    ) -> impl Iterator<Item = &NoteFieldValue> {
        let value = self.get(key);
        let list = match value {
            Some(NoteFieldValue::List(items)) => items.as_slice(),
            _ => &[],
        };
        let scalar = match value {
            Some(NoteFieldValue::List(_) | NoteFieldValue::Null) | None => None,
            Some(other) => Some(other),
        };
        scalar.into_iter().chain(list.iter())
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

#[cfg(test)]
mod tests {
    use super::*;

    mod raw_frontmatter {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn treats_whitespace_only_as_empty() {
            let raw = RawFrontmatter::new("   \n  \t  ");
            assert!(raw.is_empty());
        }

        #[test]
        fn last_duplicate_frontmatter_key_wins() {
            let key = FieldKey::try_new("title").unwrap();
            let mut fields = IndexMap::new();
            fields.insert(key.clone(), NoteFieldValue::String("First".into()));
            fields.insert(key.clone(), NoteFieldValue::String("Second".into()));
            let fm = Frontmatter::new(fields);
            assert_eq!(
                fm.get_by_key(&key),
                Some(&NoteFieldValue::String("Second".into()))
            );
        }

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

        #[test]
        fn frontmatter_get_matches_canonical_key() {
            let mut fields = IndexMap::new();
            fields.insert(
                FieldKey::try_new("MyTitle").unwrap(),
                NoteFieldValue::String("hello".into()),
            );
            let fm = Frontmatter::new(fields);
            assert_eq!(
                fm.get("mytitle"),
                Some(&NoteFieldValue::String("hello".into()))
            );
        }
    }
}
