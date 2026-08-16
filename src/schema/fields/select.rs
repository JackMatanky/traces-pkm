//! `select` field type definition, entry type, and parsing.

use std::collections::BTreeMap;

use super::{
    super::raw::RawSchemaFieldType,
    SchemaFieldType,
    address::FieldAddressRef,
    error::{AttributeError, expect_string_list, type_mismatch, unknown_key},
};
use crate::field::FieldValue;

/// Raw `select` field options before `$ref` merge.
#[derive(Default)]
pub(super) struct SchemaSelectFieldDef {
    values: Vec<SchemaSelectFieldEntry>,
}

impl SchemaSelectFieldDef {
    /// Parse `options` against `select`'s `values` attribute, merging with
    /// `base` when present. Returns the effective [`SchemaFieldType::Select`]
    /// and every per-key validation failure.
    pub(super) fn parse(
        address: FieldAddressRef<'_>,
        options: &BTreeMap<String, FieldValue>,
        base: Option<&SchemaFieldType>,
    ) -> (SchemaFieldType, Vec<AttributeError>) {
        let mut def = Self::default();
        let mut errors = Vec::new();
        for (key, value) in options {
            match key.as_str() {
                "values" => match expect_string_list(value) {
                    Some(values) => {
                        def.values = values
                            .into_iter()
                            .map(SchemaSelectFieldEntry::literal)
                            .collect();
                    }
                    None => errors.push(type_mismatch(
                        address,
                        RawSchemaFieldType::Select,
                        key,
                        value,
                        "an array of strings",
                    )),
                },
                _ => {
                    errors.push(unknown_key(
                        address,
                        RawSchemaFieldType::Select,
                        key,
                    ));
                }
            }
        }
        let values = if def.values.is_empty() {
            match base {
                Some(SchemaFieldType::Select {
                    values,
                }) => values.clone(),
                _ => Vec::new(),
            }
        } else {
            def.values
        };
        (
            SchemaFieldType::Select {
                values,
            },
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
    /// Build a literal entry where `label` equals `value` and `extra` is
    /// empty.
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
