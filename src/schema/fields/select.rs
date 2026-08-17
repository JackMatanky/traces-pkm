//! `select` field type definition, entry type, and parsing.

use std::collections::BTreeMap;

use super::{SchemaFieldType, parser::SchemaFieldParser};
use crate::field::FieldValue;

/// Resolved `select` field options.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct SchemaSelectField {
    values: Vec<SchemaSelectFieldEntry>,
}

impl SchemaSelectField {
    /// Return the selectable entries.
    #[inline]
    #[must_use]
    pub(crate) fn values(&self) -> &[SchemaSelectFieldEntry] {
        &self.values
    }

    /// Build an instance for tests.
    #[cfg(test)]
    pub(crate) fn for_test(values: Vec<SchemaSelectFieldEntry>) -> Self {
        Self {
            values,
        }
    }

    /// Parse `options` against `select`'s `values` attribute, merging with
    /// `base` when present. Returns the effective [`SchemaFieldType::Select`].
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
        let values = parser.string_list(options, "values", Vec::new());
        let values = if values.is_empty() {
            match base {
                Some(SchemaFieldType::Select(base_def)) => {
                    base_def.values.clone()
                }
                _ => Vec::new(),
            }
        } else {
            values.into_iter().map(SchemaSelectFieldEntry::literal).collect()
        };

        SchemaFieldType::Select(SchemaSelectField {
            values,
        })
    }
}

/// One selectable entry a `select`/`multi` field resolves to.
///
/// Rendered as a plain string when `label` equals `value` and `extra` is
/// empty, otherwise as `{value, label, ...extra}`.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SchemaSelectFieldEntry {
    value: FieldValue,
    label: FieldValue,
    extra: BTreeMap<String, FieldValue>,
}

impl SchemaSelectFieldEntry {
    /// Build a literal entry where `label` equals `value` and `extra` is empty.
    pub(crate) fn literal(value: String) -> Self {
        Self {
            value: FieldValue::String(value.clone()),
            label: FieldValue::String(value),
            extra: BTreeMap::new(),
        }
    }

    /// Return this entry's value.
    #[inline]
    #[must_use]
    pub(crate) fn value(&self) -> &FieldValue {
        &self.value
    }

    /// Return this entry's display label.
    #[inline]
    #[must_use]
    pub(crate) fn label(&self) -> &FieldValue {
        &self.label
    }

    /// Return this entry's passthrough keys beyond `value`/`label`.
    #[inline]
    #[must_use]
    pub(crate) fn extra(&self) -> &BTreeMap<String, FieldValue> {
        &self.extra
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use pretty_assertions::assert_eq;

    use super::*;
    use crate::schema::{
        error::SchemaFieldParserError,
        fields::{address::FieldAddress, parser::SchemaFieldParser},
    };

    fn address() -> FieldAddress {
        FieldAddress::try_from("#book/field").expect("valid ref")
    }

    fn options(pairs: &[(&str, FieldValue)]) -> BTreeMap<String, FieldValue> {
        pairs.iter().map(|(k, v)| ((*k).to_owned(), v.clone())).collect()
    }

    #[test]
    fn collects_declared_values_as_literal_entries() {
        let opts = options(&[(
            "values",
            FieldValue::List(vec![
                FieldValue::String("draft".to_owned()),
                FieldValue::String("done".to_owned()),
            ]),
        )]);

        let addr = address();
        let mut parser = SchemaFieldParser::new(
            addr.as_ref(),
            SchemaFieldType::Select(SchemaSelectField::default()),
        );
        let field_type = SchemaSelectField::parse(&mut parser, &opts, None);
        let errors = parser.finish(&opts);

        assert!(errors.is_empty());
        let SchemaFieldType::Select(def) = field_type else {
            panic!("expected Select");
        };
        assert_eq!(def.values().len(), 2);
        assert_eq!(
            def.values()[0].value(),
            &FieldValue::String("draft".to_owned())
        );
        assert_eq!(
            def.values()[0].label(),
            &FieldValue::String("draft".to_owned())
        );
        assert!(def.values()[0].extra().is_empty());
    }

    #[test]
    fn defaults_to_empty_values_when_options_omit_them() {
        let opts = options(&[]);

        let addr = address();
        let mut parser = SchemaFieldParser::new(
            addr.as_ref(),
            SchemaFieldType::Select(SchemaSelectField::default()),
        );
        let field_type = SchemaSelectField::parse(&mut parser, &opts, None);
        let errors = parser.finish(&opts);

        assert!(errors.is_empty());
        assert_eq!(
            field_type,
            SchemaFieldType::Select(SchemaSelectField::default())
        );
    }

    #[test]
    fn rejects_non_list_values_as_type_mismatch() {
        let opts =
            options(&[("values", FieldValue::String("draft".to_owned()))]);

        let addr = address();
        let mut parser = SchemaFieldParser::new(
            addr.as_ref(),
            SchemaFieldType::Select(SchemaSelectField::default()),
        );
        let _ = SchemaSelectField::parse(&mut parser, &opts, None);
        let errors = parser.finish(&opts);

        assert_eq!(errors.len(), 1);
        assert!(matches!(
            errors[0],
            SchemaFieldParserError::TypeMismatch { .. }
        ));
    }

    #[test]
    fn falls_back_to_bases_values_when_options_omit_them() {
        let base = SchemaFieldType::Select(SchemaSelectField::for_test(vec![
            SchemaSelectFieldEntry::literal("old".to_owned()),
        ]));

        let addr = address();
        let mut parser = SchemaFieldParser::new(
            addr.as_ref(),
            SchemaFieldType::Select(SchemaSelectField::default()),
        );
        let field_type = SchemaSelectField::parse(
            &mut parser,
            &BTreeMap::new(),
            Some(&base),
        );
        let errors = parser.finish(&BTreeMap::new());

        assert!(errors.is_empty());
        assert_eq!(field_type, base);
    }

    #[test]
    fn returns_type_mismatch_when_values_contains_non_strings() {
        let opts = options(&[(
            "values",
            FieldValue::List(vec![FieldValue::Int(1), FieldValue::Int(2)]),
        )]);

        let addr = address();
        let mut parser = SchemaFieldParser::new(
            addr.as_ref(),
            SchemaFieldType::Select(SchemaSelectField::default()),
        );
        let _ = SchemaSelectField::parse(&mut parser, &opts, None);
        let errors = parser.finish(&opts);

        assert_eq!(errors.len(), 1);
        assert!(matches!(
            errors[0],
            SchemaFieldParserError::TypeMismatch { .. }
        ));
    }

    #[test]
    fn ignores_a_mismatched_base_type() {
        let base = SchemaFieldType::Input;

        let addr = address();
        let mut parser = SchemaFieldParser::new(
            addr.as_ref(),
            SchemaFieldType::Select(SchemaSelectField::default()),
        );
        let field_type = SchemaSelectField::parse(
            &mut parser,
            &BTreeMap::new(),
            Some(&base),
        );
        let errors = parser.finish(&BTreeMap::new());

        assert!(errors.is_empty());
        assert_eq!(
            field_type,
            SchemaFieldType::Select(SchemaSelectField::default())
        );
    }
}
