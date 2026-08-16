//! `select` field type definition, entry type, and parsing.

use std::collections::BTreeMap;

use super::{
    super::raw::RawSchemaFieldType,
    SchemaFieldType,
    address::FieldAddressRef,
    error::{AttributeError, expect_string_list, type_mismatch, unknown_key},
};
use crate::field::FieldValue;

/// Own declaration of a `select` field's type-specific options, not yet merged
/// with an inherited `$ref` base.
#[derive(Default)]
pub(super) struct SchemaSelectFieldDef {
    values: Vec<SchemaSelectFieldEntry>,
}

impl SchemaSelectFieldDef {
    /// Parses every key in `options` against `select`'s one valid attribute
    /// (`values`), merges with `base` when present, and returns the effective
    /// [`SchemaFieldType::Select`] alongside every per-key failure encountered
    /// — an unrecognized key or a wrongly-shaped value does not stop parsing
    /// the rest.
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

/// One selectable entry a `select`/`multi` field's `values` resolves to.
///
/// No memory of source: literal today (every entry built by
/// [`SchemaSelectFieldEntry::literal`]); an inline object or values-file entry
/// once ticket 08 lands. `template/engine/schema.rs` renders an entry as a
/// plain string when `label == value` and `extra` is empty (always true under
/// this ticket), else as `{value, label, ...extra}`.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SchemaSelectFieldEntry {
    value: FieldValue,
    label: FieldValue,
    extra: BTreeMap<String, FieldValue>,
}

impl SchemaSelectFieldEntry {
    /// Builds a flat entry from a plain declared string: `label` defaults to
    /// `value`, `extra` is empty. The only shape a literal `values = [...]`
    /// array produces.
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
