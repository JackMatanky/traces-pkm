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
