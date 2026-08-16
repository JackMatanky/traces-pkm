//! `number` field type definition and parsing.

use std::collections::BTreeMap;

use super::{
    super::raw::RawSchemaFieldType,
    SchemaFieldType,
    address::FieldAddressRef,
    error::{AttributeError, type_mismatch, unknown_key},
};
use crate::field::FieldValue;

/// Raw `number` field options before `$ref` merge.
#[derive(Default)]
pub(super) struct SchemaNumberFieldDef {
    min: Option<f64>,
    max: Option<f64>,
    step: Option<f64>,
}

impl SchemaNumberFieldDef {
    /// Parse `options` against `number`'s `min`/`max`/`step` attributes,
    /// merging with `base` when present.
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
        let mut def = Self::default();
        let mut errors = Vec::new();
        for (key, value) in options {
            let slot = match key.as_str() {
                "min" => &mut def.min,
                "max" => &mut def.max,
                "step" => &mut def.step,
                _ => {
                    errors.push(unknown_key(
                        address,
                        RawSchemaFieldType::Number,
                        key,
                    ));
                    continue;
                }
            };
            match value.as_f64() {
                Some(number) => *slot = Some(number),
                None => errors.push(type_mismatch(
                    address,
                    RawSchemaFieldType::Number,
                    key,
                    value,
                    "a number",
                )),
            }
        }
        let (base_min, base_max, base_step) = match base {
            Some(SchemaFieldType::Number {
                min,
                max,
                step,
            }) => (*min, *max, *step),
            _ => (None, None, None),
        };
        (
            SchemaFieldType::Number {
                min: def.min.or(base_min),
                max: def.max.or(base_max),
                step: def.step.or(base_step),
            },
            errors,
        )
    }
}
