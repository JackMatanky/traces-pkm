//! `number` field type definition and parsing.

use std::collections::BTreeMap;

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
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "reserved for future schema consumers")
    )]
    pub(crate) fn min(&self) -> Option<f64> {
        self.min
    }

    /// Return the inclusive maximum bound, if set.
    #[inline]
    #[must_use]
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "reserved for future schema consumers")
    )]
    pub(crate) fn max(&self) -> Option<f64> {
        self.max
    }

    /// Return the increment step, if set.
    #[inline]
    #[must_use]
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "reserved for future schema consumers")
    )]
    pub(crate) fn step(&self) -> Option<f64> {
        self.step
    }

    /// Build an instance for tests.
    #[cfg(test)]
    pub(crate) fn for_test(
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
        options: &BTreeMap<String, FieldValue>,
        base: Option<&SchemaFieldType>,
    ) -> SchemaFieldType {
        let (base_min, base_max, base_step) = match base {
            Some(SchemaFieldType::Number(base_def)) => {
                (base_def.min, base_def.max, base_def.step)
            }
            _ => (None, None, None),
        };

        let min = parser.f64(options, "min", base_min);
        let max = parser.f64(options, "max", base_max);
        let step = parser.f64(options, "step", base_step);

        SchemaFieldType::Number(SchemaNumberField {
            min,
            max,
            step,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use pretty_assertions::assert_eq;

    use super::{
        super::{
            super::error::SchemaFieldParserError, address::FieldAddress,
            parser::SchemaFieldParser,
        },
        *,
    };

    fn address() -> FieldAddress {
        FieldAddress::try_from("#book/field").expect("valid ref")
    }

    fn options(pairs: &[(&str, FieldValue)]) -> BTreeMap<String, FieldValue> {
        pairs.iter().map(|(k, v)| ((*k).to_owned(), v.clone())).collect()
    }

    #[test]
    fn rejects_string_min_as_type_mismatch() {
        let opts = options(&[("min", FieldValue::String("abc".to_owned()))]);

        let addr = address();
        let mut parser = SchemaFieldParser::new(
            addr.as_ref(),
            SchemaFieldType::Number(SchemaNumberField::default()),
        );
        let _ = SchemaNumberField::parse(&mut parser, &opts, None);
        let errors = parser.finish(&opts);

        assert_eq!(errors.len(), 1);
        assert!(matches!(
            errors[0],
            SchemaFieldParserError::TypeMismatch { .. }
        ));
    }

    #[test]
    fn accepts_an_integer_min_as_a_float() {
        let opts = options(&[("min", FieldValue::Int(0))]);

        let addr = address();
        let mut parser = SchemaFieldParser::new(
            addr.as_ref(),
            SchemaFieldType::Number(SchemaNumberField::default()),
        );
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
        let mut parser = SchemaFieldParser::new(
            addr.as_ref(),
            SchemaFieldType::Number(SchemaNumberField::default()),
        );
        let _ = SchemaNumberField::parse(&mut parser, &opts, None);
        let errors = parser.finish(&opts);

        assert_eq!(errors.len(), 1);
        assert!(matches!(errors[0], SchemaFieldParserError::UnknownKey { .. }));
    }

    #[test]
    fn parses_valid_min_max_step() {
        let opts = options(&[
            ("min", FieldValue::Int(0)),
            ("max", FieldValue::Int(100)),
            ("step", FieldValue::Int(5)),
        ]);

        let addr = address();
        let mut parser = SchemaFieldParser::new(
            addr.as_ref(),
            SchemaFieldType::Number(SchemaNumberField::default()),
        );
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

    #[test]
    fn inherits_min_max_step_from_number_base() {
        let base = SchemaFieldType::Number(SchemaNumberField::for_test(
            Some(0.0),
            Some(100.0),
            Some(5.0),
        ));
        let opts = BTreeMap::new();

        let addr = address();
        let mut parser = SchemaFieldParser::new(
            addr.as_ref(),
            SchemaFieldType::Number(SchemaNumberField::default()),
        );
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
    fn ignores_non_number_base() {
        let base = SchemaFieldType::Input;
        let opts = BTreeMap::new();

        let addr = address();
        let mut parser = SchemaFieldParser::new(
            addr.as_ref(),
            SchemaFieldType::Number(SchemaNumberField::default()),
        );
        let field_type =
            SchemaNumberField::parse(&mut parser, &opts, Some(&base));
        let errors = parser.finish(&opts);

        assert!(errors.is_empty());
        assert_eq!(
            field_type,
            SchemaFieldType::Number(SchemaNumberField::for_test(
                None, None, None,
            ))
        );
    }

    #[test]
    fn returns_type_mismatch_when_max_is_not_a_number() {
        let opts = options(&[("max", FieldValue::String("abc".to_owned()))]);

        let addr = address();
        let mut parser = SchemaFieldParser::new(
            addr.as_ref(),
            SchemaFieldType::Number(SchemaNumberField::default()),
        );
        let _ = SchemaNumberField::parse(&mut parser, &opts, None);
        let errors = parser.finish(&opts);

        assert_eq!(errors.len(), 1);
        assert!(matches!(
            errors[0],
            SchemaFieldParserError::TypeMismatch { .. }
        ));
    }
}
