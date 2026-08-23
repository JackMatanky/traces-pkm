//! `number` field type definition and parsing.

use indexmap::IndexMap;

use super::{SchemaFieldType, parser::SchemaFieldParser};
use crate::field::FieldValue;

/// Resolved `number` field options.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct SchemaNumberField {
    min: Option<f64>,
    max: Option<f64>,
    step: Option<f64>,
}

impl SchemaNumberField {
    /// Return the inclusive minimum bound, if set.
    #[inline]
    #[must_use]
    #[expect(dead_code, reason = "reserved for future schema consumers")]
    pub(crate) const fn min(&self) -> Option<f64> {
        self.min
    }

    /// Return the inclusive maximum bound, if set.
    #[inline]
    #[must_use]
    #[expect(dead_code, reason = "reserved for future schema consumers")]
    pub(crate) const fn max(&self) -> Option<f64> {
        self.max
    }

    /// Return the increment step, if set.
    #[inline]
    #[must_use]
    #[expect(dead_code, reason = "reserved for future schema consumers")]
    pub(crate) const fn step(&self) -> Option<f64> {
        self.step
    }

    /// Build an instance for tests.
    #[cfg(test)]
    pub(crate) const fn for_test(
        min: Option<f64>,
        max: Option<f64>,
        step: Option<f64>,
    ) -> Self {
        Self {
            min,
            max,
            step,
        }
    }

    /// Parse `options` against `number`'s `min`/`max`/`step` attributes,
    /// merging with `base` when present.
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
        let (base_min, base_max, base_step) = base
            .map_or((None, None, None), |base| (base.min, base.max, base.step));

        let min = parser.f64(options, "min", base_min);
        let max = parser.f64(options, "max", base_max);
        let step = parser.f64(options, "step", base_step);

        SchemaFieldType::Number(Self {
            min,
            max,
            step,
        })
    }
}

#[cfg(test)]
mod tests {
    use indexmap::IndexMap;
    use pretty_assertions::assert_eq;
    use rstest::rstest;

    use super::*;
    use crate::schema::fields::{
        SchemaFieldTypeTag, SchemaNumberField, address::FieldAddress,
        error::SchemaFieldParserError, parser::SchemaFieldParser,
    };

    fn address() -> FieldAddress {
        FieldAddress::try_from("#book/field").expect("valid ref")
    }

    fn options(pairs: &[(&str, FieldValue)]) -> IndexMap<String, FieldValue> {
        pairs.iter().map(|(k, v)| ((*k).to_owned(), v.clone())).collect()
    }

    #[rstest]
    #[case::min("min")]
    #[case::max("max")]
    fn rejects_string_value_as_type_mismatch(#[case] key: &str) {
        let opts = options(&[(key, FieldValue::String("abc".to_owned()))]);

        let addr = address();
        let mut parser =
            SchemaFieldParser::new(addr.as_ref(), SchemaFieldTypeTag::Number);
        let _ = SchemaNumberField::parse(&mut parser, &opts, None);
        let errors = parser.finish(&opts);

        assert_eq!(errors.len(), 1);
        assert!(matches!(
            errors.first().expect("expected error"),
            SchemaFieldParserError::TypeMismatch { .. }
        ));
    }

    #[test]
    fn accepts_an_integer_min_as_a_float() {
        let opts = options(&[("min", FieldValue::Int(0))]);

        let addr = address();
        let mut parser =
            SchemaFieldParser::new(addr.as_ref(), SchemaFieldTypeTag::Number);
        let field_type = SchemaNumberField::parse(&mut parser, &opts, None);
        let errors = parser.finish(&opts);

        assert!(errors.is_empty());
        assert_eq!(
            field_type,
            SchemaFieldType::Number(SchemaNumberField::for_test(
                Some(0.0),
                None,
                None
            ))
        );
    }

    #[test]
    fn rejects_unknown_key() {
        let opts = options(&[("values", FieldValue::Int(1))]);

        let addr = address();
        let mut parser =
            SchemaFieldParser::new(addr.as_ref(), SchemaFieldTypeTag::Number);
        let _ = SchemaNumberField::parse(&mut parser, &opts, None);
        let errors = parser.finish(&opts);

        assert_eq!(errors.len(), 1);
        assert!(matches!(
            errors.first().expect("expected error"),
            SchemaFieldParserError::UnknownKey { .. }
        ));
    }

    #[test]
    fn parses_valid_min_max_step() {
        let opts = options(&[
            ("min", FieldValue::Int(0)),
            ("max", FieldValue::Int(100)),
            ("step", FieldValue::Int(5)),
        ]);

        let addr = address();
        let mut parser =
            SchemaFieldParser::new(addr.as_ref(), SchemaFieldTypeTag::Number);
        let field_type = SchemaNumberField::parse(&mut parser, &opts, None);
        let errors = parser.finish(&opts);

        assert!(errors.is_empty());
        assert_eq!(
            field_type,
            SchemaFieldType::Number(SchemaNumberField::for_test(
                Some(0.0),
                Some(100.0),
                Some(5.0),
            ))
        );
    }

    mod accessors {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn returns_configured_min_value() {
            let field = SchemaNumberField::for_test(Some(0.0), None, None);
            assert_eq!(field.min(), Some(0.0));
        }

        #[test]
        fn returns_configured_max_value() {
            let field = SchemaNumberField::for_test(None, Some(100.0), None);
            assert_eq!(field.max(), Some(100.0));
        }

        #[test]
        fn returns_configured_step_value() {
            let field = SchemaNumberField::for_test(None, None, Some(5.0));
            assert_eq!(field.step(), Some(5.0));
        }

        #[test]
        fn returns_none_for_unset_fields() {
            let field = SchemaNumberField::for_test(None, None, None);
            assert_eq!(field.min(), None);
            assert_eq!(field.max(), None);
            assert_eq!(field.step(), None);
        }
    }

    #[test]
    fn inherits_min_max_step_from_number_base() {
        let base =
            SchemaNumberField::for_test(Some(0.0), Some(100.0), Some(5.0));
        let opts = IndexMap::new();

        let addr = address();
        let mut parser =
            SchemaFieldParser::new(addr.as_ref(), SchemaFieldTypeTag::Number);
        let field_type =
            SchemaNumberField::parse(&mut parser, &opts, Some(&base));
        let errors = parser.finish(&opts);

        assert!(errors.is_empty());
        assert_eq!(
            field_type,
            SchemaFieldType::Number(SchemaNumberField::for_test(
                Some(0.0),
                Some(100.0),
                Some(5.0),
            ))
        );
    }

    #[test]
    fn getters_return_stored_min_max_step_values() {
        let field =
            SchemaNumberField::for_test(Some(1.5), Some(9.5), Some(0.5));

        assert_eq!(field.min(), Some(1.5));
        assert_eq!(field.max(), Some(9.5));
        assert_eq!(field.step(), Some(0.5));
    }

    #[test]
    fn getters_return_none_when_fields_are_unset() {
        let field = SchemaNumberField::for_test(None, None, None);

        assert_eq!(field.min(), None);
        assert_eq!(field.max(), None);
        assert_eq!(field.step(), None);
    }
}
