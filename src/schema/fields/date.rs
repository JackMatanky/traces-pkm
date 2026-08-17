//! `date` field type definition and parsing.

use std::collections::BTreeMap;

use super::{SchemaFieldType, parser::SchemaFieldParser};
use crate::field::FieldValue;

/// Resolved `date` field options.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct SchemaDateField {
    format: Option<String>,
}

impl SchemaDateField {
    /// Return the display/parse format, if set.
    #[inline]
    #[must_use]
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "reserved for future schema consumers")
    )]
    pub(crate) fn format(&self) -> Option<&str> {
        self.format.as_deref()
    }

    /// Build an instance for tests.
    #[cfg(test)]
    pub(crate) fn for_test(format: Option<String>) -> Self {
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
        options: &BTreeMap<String, FieldValue>,
        base: Option<&SchemaFieldType>,
    ) -> SchemaFieldType {
        let base_format = match base {
            Some(SchemaFieldType::Date(base_def)) => base_def.format.clone(),
            _ => None,
        };

        let format = parser.string(options, "format", base_format);

        SchemaFieldType::Date(SchemaDateField {
            format,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::schema::{
        error::SchemaFieldParserError,
        fields::{
            SchemaDateField, address::FieldAddress, parser::SchemaFieldParser,
        },
    };

    fn address() -> FieldAddress {
        FieldAddress::try_from("#book/field").expect("valid ref")
    }

    fn options(pairs: &[(&str, FieldValue)]) -> BTreeMap<String, FieldValue> {
        pairs.iter().map(|(k, v)| ((*k).to_owned(), v.clone())).collect()
    }

    #[test]
    fn parses_a_valid_format_string() {
        let opts =
            options(&[("format", FieldValue::String("YYYY-MM-DD".to_owned()))]);

        let addr = address();
        let mut parser = SchemaFieldParser::new(
            addr.as_ref(),
            SchemaFieldType::Date(SchemaDateField::default()),
        );
        let field_type = SchemaDateField::parse(&mut parser, &opts, None);
        let errors = parser.finish(&opts);

        assert!(errors.is_empty());
        let SchemaFieldType::Date(def) = field_type else {
            panic!("expected Date");
        };
        assert_eq!(def.format(), Some("YYYY-MM-DD"));
    }

    #[test]
    fn inherits_format_from_date_base() {
        let base = SchemaFieldType::Date(SchemaDateField::for_test(Some(
            "YYYY-MM-DD".to_owned(),
        )));

        let addr = address();
        let mut parser = SchemaFieldParser::new(
            addr.as_ref(),
            SchemaFieldType::Date(SchemaDateField::default()),
        );
        let field_type =
            SchemaDateField::parse(&mut parser, &BTreeMap::new(), Some(&base));
        let errors = parser.finish(&BTreeMap::new());

        assert!(errors.is_empty());
        let SchemaFieldType::Date(def) = field_type else {
            panic!("expected Date");
        };
        assert_eq!(def.format(), Some("YYYY-MM-DD"));
    }

    #[test]
    fn ignores_non_date_base() {
        let base = SchemaFieldType::Input;

        let addr = address();
        let mut parser = SchemaFieldParser::new(
            addr.as_ref(),
            SchemaFieldType::Date(SchemaDateField::default()),
        );
        let field_type =
            SchemaDateField::parse(&mut parser, &BTreeMap::new(), Some(&base));
        let errors = parser.finish(&BTreeMap::new());

        assert!(errors.is_empty());
        assert_eq!(
            field_type,
            SchemaFieldType::Date(SchemaDateField::default())
        );
    }

    #[test]
    fn returns_type_mismatch_when_format_is_not_a_string() {
        let opts = options(&[("format", FieldValue::Int(123))]);

        let addr = address();
        let mut parser = SchemaFieldParser::new(
            addr.as_ref(),
            SchemaFieldType::Date(SchemaDateField::default()),
        );
        let _ = SchemaDateField::parse(&mut parser, &opts, None);
        let errors = parser.finish(&opts);

        assert_eq!(errors.len(), 1);
        assert!(matches!(
            errors[0],
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
        let mut parser = SchemaFieldParser::new(
            addr.as_ref(),
            SchemaFieldType::Date(SchemaDateField::default()),
        );
        let _ = SchemaDateField::parse(&mut parser, &opts, None);
        let errors = parser.finish(&opts);

        assert_eq!(errors.len(), 1);
        assert!(matches!(errors[0], SchemaFieldParserError::UnknownKey { .. }));
    }
}
