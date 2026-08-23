//! `date` field type definition and parsing.

use indexmap::IndexMap;

use super::{SchemaFieldType, parser::SchemaFieldParser};
use crate::field::FieldValue;

/// Resolved `date` field options.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct SchemaDateField {
    format: Option<String>,
}

impl SchemaDateField {
    /// Return the display/parse format, if set.
    #[inline]
    #[must_use]
    #[cfg(test)]
    pub(crate) fn format(&self) -> Option<&str> {
        self.format.as_deref()
    }

    /// Build an instance for tests.
    #[cfg(test)]
    pub(crate) const fn for_test(format: Option<String>) -> Self {
        Self {
            format,
        }
    }

    /// Parse `options` against `date`'s `format` attribute, merging with `base`
    /// when present.
    ///
    /// # Arguments
    ///
    /// * `parser`: pre-constructed parser for this field.
    /// * `options`: raw key-value pairs from the TOML definition.
    /// * `base`: inherited field type to fall back to for unset keys.
    pub(super) fn parse(
        parser: &mut SchemaFieldParser<'_>,
        options: &IndexMap<String, FieldValue>,
        base: Option<&Self>,
    ) -> SchemaFieldType {
        let base_format = base.and_then(|base| base.format.clone());

        let format = parser.string(options, "format", base_format);

        SchemaFieldType::Date(Self {
            format,
        })
    }
}

#[cfg(test)]
mod tests {
    use indexmap::IndexMap;

    use super::*;
    use crate::schema::fields::{
        SchemaDateField, SchemaFieldTypeTag, address::FieldAddress,
        error::SchemaFieldParserError, parser::SchemaFieldParser,
    };

    fn address() -> FieldAddress {
        FieldAddress::try_from("#book/field").expect("valid ref")
    }

    fn options(pairs: &[(&str, FieldValue)]) -> IndexMap<String, FieldValue> {
        pairs.iter().map(|(k, v)| ((*k).to_owned(), v.clone())).collect()
    }

    #[test]
    #[expect(clippy::panic, reason = "test assertion on enum variant")]
    fn parses_a_valid_format_string() {
        let opts =
            options(&[("format", FieldValue::String("YYYY-MM-DD".to_owned()))]);

        let addr = address();
        let mut parser =
            SchemaFieldParser::new(addr.as_ref(), SchemaFieldTypeTag::Date);
        let field_type = SchemaDateField::parse(&mut parser, &opts, None);
        let errors = parser.finish(&opts);

        assert!(errors.is_empty());
        match &field_type {
            SchemaFieldType::Date(def) => {
                assert_eq!(def.format(), Some("YYYY-MM-DD"));
            }
            other => panic!("expected Date, got {other:?}"),
        }
    }

    #[test]
    #[expect(clippy::panic, reason = "test assertion on enum variant")]
    fn inherits_format_from_date_base() {
        let base = SchemaDateField::for_test(Some("YYYY-MM-DD".to_owned()));

        let addr = address();
        let mut parser =
            SchemaFieldParser::new(addr.as_ref(), SchemaFieldTypeTag::Date);
        let field_type =
            SchemaDateField::parse(&mut parser, &IndexMap::new(), Some(&base));
        let errors = parser.finish(&IndexMap::new());

        assert!(errors.is_empty());
        match &field_type {
            SchemaFieldType::Date(def) => {
                assert_eq!(def.format(), Some("YYYY-MM-DD"));
            }
            other => panic!("expected Date, got {other:?}"),
        }
    }

    #[test]
    fn returns_type_mismatch_when_format_is_not_a_string() {
        let opts = options(&[("format", FieldValue::Int(123))]);

        let addr = address();
        let mut parser =
            SchemaFieldParser::new(addr.as_ref(), SchemaFieldTypeTag::Date);
        let _ = SchemaDateField::parse(&mut parser, &opts, None);
        let errors = parser.finish(&opts);

        assert_eq!(errors.len(), 1);
        assert!(matches!(
            errors.first().expect("expected error"),
            SchemaFieldParserError::TypeMismatch { .. }
        ));
    }

    #[test]
    fn rejects_unknown_key() {
        let opts = options(&[(
            "values",
            FieldValue::List(vec![FieldValue::String("x".to_owned())]),
        )]);

        let addr = address();
        let mut parser =
            SchemaFieldParser::new(addr.as_ref(), SchemaFieldTypeTag::Date);
        let _ = SchemaDateField::parse(&mut parser, &opts, None);
        let errors = parser.finish(&opts);

        assert_eq!(errors.len(), 1);
        assert!(matches!(
            errors.first().expect("expected error"),
            SchemaFieldParserError::UnknownKey { .. }
        ));
    }
}
