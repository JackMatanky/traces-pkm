//! YAML frontmatter parsing and key-value metadata storage.
//!
//! [`RawFrontmatter`] preserves unparsed source YAML. [`Frontmatter`] stores
//! parsed YAML key-value pairs mapping [`FieldKey`] to [`NoteFieldValue`]
//! values.
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::warn;

use super::field::NoteFieldValue;
use crate::{FieldKey, FieldKeyRef, Tag, field::FieldValueRef, yaml};

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
    fn as_str(&self) -> &str {
        &self.0
    }

    /// Returns `true` if the raw frontmatter text is empty or whitespace-only.
    #[inline]
    #[must_use]
    pub(crate) fn is_empty(&self) -> bool {
        self.0.trim().is_empty()
    }

    /// Parses the raw YAML text into structured frontmatter fields.
    ///
    /// Mapping keys deserialize directly to `String` under the shared YAML
    /// config (see [`crate::yaml::parse`]); a key that fails [`FieldKey`]
    /// validation (e.g. an empty canonical form) is skipped rather than
    /// failing the whole parse.
    ///
    /// # Errors
    ///
    /// - [`Parse`] if the YAML text fails to parse.
    /// - [`NotMapping`] if the top-level YAML value is not a mapping.
    ///
    /// [`Parse`]: FrontmatterParseError::Parse
    /// [`NotMapping`]: FrontmatterParseError::NotMapping
    fn parse(&self) -> Result<Frontmatter, FrontmatterParseError> {
        let parsed =
            yaml::parse(self.as_str()).map_err(FrontmatterParseError::Parse)?;
        let noyalib::Value::Mapping(map) = parsed else {
            return Err(FrontmatterParseError::NotMapping);
        };
        // Every top-level key normally parses into a field (a skip is rare:
        // a key failing `FieldKey` validation), so `map.len()` is a tight
        // upper bound known upfront - avoids `IndexMap`'s amortized growth
        // needing to guess capacity across repeated `insert` calls.
        let mut fields = IndexMap::with_capacity(map.len());
        for (key_str, raw_value) in map {
            let Ok(key) = FieldKey::try_new(key_str) else {
                continue;
            };
            let field_value = FieldValueRef::from(raw_value);
            fields.insert(key, NoteFieldValue::from(field_value));
        }
        Ok(Frontmatter::new(fields))
    }
}

/// Reports why [`RawFrontmatter::parse`] could not produce structured
/// [`Frontmatter`] fields.
#[derive(Debug, Error)]
enum FrontmatterParseError {
    /// The raw YAML text failed to parse.
    #[error("failed to parse YAML frontmatter: {0}")]
    Parse(#[source] noyalib::Error),
    /// The parsed YAML document is not a top-level key-value mapping.
    #[error("YAML frontmatter is not a key-value mapping")]
    NotMapping,
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
            reason = "sole caller is Note::fields (index-query#03 reserved \
                      accessor), itself dead in lib builds"
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

    /// Returns a flat iterator over the scalar value or list elements of the
    /// field matching `key` by string lookup, if present.
    pub(crate) fn get_values(
        &self,
        key: &str,
    ) -> impl Iterator<Item = &NoteFieldValue> {
        let value = self.get(key);
        let list = match value {
            Some(NoteFieldValue::List(items)) => items.as_ref(),
            _ => &[],
        };
        let scalar = match value {
            Some(NoteFieldValue::List(_) | NoteFieldValue::Null) | None => None,
            Some(other) => Some(other),
        };
        scalar.into_iter().chain(list.iter())
    }

    /// Returns an iterator over tags extracted from the frontmatter field
    /// matching `key`.
    ///
    /// Splits comma-separated strings and sequence entries into individual tag
    /// candidates, trimming whitespace and ignoring any candidate that fails
    /// tag syntax validation.
    pub(crate) fn tags<'a>(
        &'a self,
        key: &'a str,
    ) -> impl Iterator<Item = Tag> + 'a {
        let mut buf = String::new();
        self.get_values(key)
            .filter_map(NoteFieldValue::as_str)
            .flat_map(|value| value.split(','))
            .filter_map(move |candidate| {
                Tag::parse_lenient_into(candidate, &mut buf).ok()
            })
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
        raw.parse().unwrap_or_else(|err| {
            warn!(
                %err,
                "failed to parse YAML frontmatter block; ignoring malformed \
                fields"
            );
            Self::default()
        })
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

            assert_eq!(fm.fields().is_empty(), true);
        }

        #[test]
        fn handles_malformed_yaml_resiliently_with_empty_fields() {
            let raw = RawFrontmatter::new("invalid: [yaml: :");
            let fm = Frontmatter::from(&raw);

            assert_eq!(fm.fields().is_empty(), true);
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

        mod parse {
            use pretty_assertions::assert_eq;

            use super::super::*;

            #[test]
            fn returns_structured_fields_for_a_valid_mapping() {
                let raw = RawFrontmatter::new("title: Test\n");

                let fm = raw.parse().expect("valid mapping");

                assert_eq!(
                    fm.get("title"),
                    Some(&NoteFieldValue::String("Test".to_owned()))
                );
            }

            #[test]
            fn returns_parse_error_for_malformed_yaml() {
                let raw = RawFrontmatter::new("invalid: [yaml: :");

                assert!(matches!(
                    raw.parse(),
                    Err(FrontmatterParseError::Parse(_))
                ));
            }

            #[test]
            fn returns_not_mapping_error_for_a_top_level_sequence() {
                let raw = RawFrontmatter::new("- a\n- b\n");

                assert!(matches!(
                    raw.parse(),
                    Err(FrontmatterParseError::NotMapping)
                ));
            }

            #[test]
            fn returns_parse_error_for_a_non_scalar_mapping_key() {
                let raw = RawFrontmatter::new("[a, b]: v\n");

                assert!(matches!(
                    raw.parse(),
                    Err(FrontmatterParseError::Parse(_))
                ));
                assert!(Frontmatter::from(&raw).fields().is_empty());
            }

            #[test]
            fn skips_keys_that_fail_field_key_validation() {
                let raw = RawFrontmatter::new("a/b: 1\n");

                let fm = raw.parse().expect("valid mapping");

                assert!(fm.fields().is_empty());
            }
        }

        mod yaml_1_2_parity {
            use pretty_assertions::assert_eq;
            use rstest::rstest;

            use super::super::*;
            use crate::DateValue;

            #[rstest]
            #[case::yes("draft: yes\n", "draft", "yes")]
            #[case::no("draft: no\n", "draft", "no")]
            #[case::leading_zero("zip: 01234\n", "zip", "01234")]
            #[case::sexagesimal("meeting: 10:30\n", "meeting", "10:30")]
            fn ambiguous_yaml_1_1_scalar_stays_a_string(
                #[case] source: &str,
                #[case] key: &str,
                #[case] expected: &str,
            ) {
                let raw = RawFrontmatter::new(source);

                let fm = Frontmatter::from(&raw);

                assert_eq!(
                    fm.get(key),
                    Some(&NoteFieldValue::String(expected.to_owned()))
                );
            }

            #[test]
            fn iso_date_value_parses_as_a_date() {
                let raw = RawFrontmatter::new("date: 2026-07-29\n");

                let fm = Frontmatter::from(&raw);

                assert_eq!(
                    fm.get("date"),
                    Some(&NoteFieldValue::Date(
                        DateValue::parse_iso("2026-07-29").expect("valid date")
                    ))
                );
            }

            #[test]
            fn merge_key_stays_a_literal_entry_instead_of_merging() {
                let raw =
                    RawFrontmatter::new("base: &b\n  x: 1\n<<: *b\ny: 2\n");

                let fm = Frontmatter::from(&raw);

                assert_eq!(fm.get("y"), Some(&NoteFieldValue::Number(2.0)));
                assert_eq!(fm.get("x"), None);
            }

            #[test]
            fn last_duplicate_yaml_key_wins() {
                let raw = RawFrontmatter::new("title: First\ntitle: Second\n");

                let fm = Frontmatter::from(&raw);

                assert_eq!(
                    fm.get("title"),
                    Some(&NoteFieldValue::String("Second".to_owned()))
                );
            }
        }
    }
}
