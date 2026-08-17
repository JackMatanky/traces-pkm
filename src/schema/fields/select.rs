//! `select` field type definition, entry type, and parsing.

use std::collections::BTreeMap;

use super::{
    super::raw::RawSchemaFieldType, SchemaFieldType, address::FieldAddressRef,
    error::AttributeError, parser::SchemaFieldParser,
};
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
    /// `base` when present. Returns the effective [`SchemaFieldType::Select`]
    /// and every per-key validation failure.
    ///
    /// # Arguments
    ///
    /// * `address`: field address for error context.
    /// * `options`: raw key-value pairs from the TOML definition.
    /// * `base`: inherited field type to fall back to for unset keys.
    pub(super) fn parse(
        address: FieldAddressRef<'_>,
        options: &BTreeMap<String, FieldValue>,
        base: Option<&SchemaFieldType>,
    ) -> (SchemaFieldType, Vec<AttributeError>) {
        let mut errors = Vec::new();
        let mut parser =
            SchemaFieldParser::new(address, RawSchemaFieldType::Select);

        let values =
            parser.string_list(options, "values", Vec::new(), &mut errors);
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

        errors.extend(parser.finish(options));
        (
            SchemaFieldType::Select(SchemaSelectField {
                values,
            }),
            errors,
        )
    }
}

/// One selectable entry a `select`/`multi` field resolves to.
///
/// Rendered as a plain string when `label == value` and `extra` is empty,
/// otherwise as `{value, label, ...extra}`.
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
