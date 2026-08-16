//! `date` field type definition and parsing.

use std::collections::BTreeMap;

use super::{
    super::raw::RawSchemaFieldType,
    SchemaFieldType,
    address::FieldAddressRef,
    error::{AttributeError, expect_string, type_mismatch, unknown_key},
};
use crate::field::FieldValue;

/// Own declaration of a `date` field's type-specific options. See
/// [`super::select::SchemaSelectFieldDef`]'s docs for why this is one level
/// more `Option` than [`SchemaFieldType::Date`].
#[derive(Default)]
pub(super) struct SchemaDateFieldDef {
    format: Option<String>,
}

impl SchemaDateFieldDef {
    /// Parses every key in `options` against `date`'s one valid attribute
    /// (`format`), merges with `base` when present, and returns the effective
    /// [`SchemaFieldType::Date`] alongside every per-key failure encountered.
    pub(super) fn parse(
        address: FieldAddressRef<'_>,
        options: &BTreeMap<String, FieldValue>,
        base: Option<&SchemaFieldType>,
    ) -> (SchemaFieldType, Vec<AttributeError>) {
        let mut def = Self::default();
        let mut errors = Vec::new();
        for (key, value) in options {
            match key.as_str() {
                "format" => match expect_string(value) {
                    Some(format) => def.format = Some(format),
                    None => errors.push(type_mismatch(
                        address,
                        RawSchemaFieldType::Date,
                        key,
                        value,
                        "a string",
                    )),
                },
                _ => {
                    errors.push(unknown_key(
                        address,
                        RawSchemaFieldType::Date,
                        key,
                    ));
                }
            }
        }
        let format = def.format.or_else(|| match base {
            Some(SchemaFieldType::Date {
                format,
            }) => format.clone(),
            _ => None,
        });
        (
            SchemaFieldType::Date {
                format,
            },
            errors,
        )
    }
}
