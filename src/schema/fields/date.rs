//! `date` field type definition and parsing.

use std::collections::BTreeMap;

use super::{
    super::raw::RawSchemaFieldType,
    SchemaFieldType,
    address::FieldAddressRef,
    error::{AttributeError, expect_string, type_mismatch, unknown_key},
};
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
            Some(SchemaFieldType::Date(base_def)) => base_def.format.clone(),
            _ => None,
        });
        (
            SchemaFieldType::Date(SchemaDateField {
                format,
            }),
            errors,
        )
    }
}
