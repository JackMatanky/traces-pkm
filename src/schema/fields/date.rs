//! `date` field type definition and parsing.

use std::collections::BTreeMap;

use super::{SchemaFieldType, parser::SchemaFieldParser};
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
    /// * `parser`: pre-constructed parser for this field.
    /// * `options`: raw key-value pairs from the TOML definition.
    /// * `base`: inherited field type to fall back to for unset keys.
    pub(super) fn parse(
        parser: &mut SchemaFieldParser<'_>,
        options: &BTreeMap<String, FieldValue>,
        base: Option<&SchemaFieldType>,
    ) -> SchemaFieldType {
        let base_format = match base {
            Some(SchemaFieldType::Date(base_def)) => base_def.format.clone(),
            _ => None,
        };

        let format = parser.string(options, "format", base_format);

        SchemaFieldType::Date(SchemaDateField {
            format,
        })
    }
}
