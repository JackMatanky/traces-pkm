//! `date` field type definition and parsing.

use std::collections::BTreeMap;

use super::{
    SchemaFieldType, address::FieldAddressRef, error::SchemaFieldParserError,
    parser::SchemaFieldParser,
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
    ) -> (SchemaFieldType, Vec<SchemaFieldParserError>) {
        let base_format = match base {
            Some(SchemaFieldType::Date(base_def)) => base_def.format.clone(),
            _ => None,
        };

        let mut errors = Vec::new();
        let mut parser = SchemaFieldParser::new(
            address,
            SchemaFieldType::Date(SchemaDateField::default()),
        );

        let format = parser.string(options, "format", base_format, &mut errors);

        errors.extend(parser.finish(options));
        (
            SchemaFieldType::Date(SchemaDateField {
                format,
            }),
            errors,
        )
    }
}
