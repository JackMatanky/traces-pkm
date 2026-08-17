//! `number` field type definition and parsing.

use std::collections::BTreeMap;

use super::{SchemaFieldType, parser::SchemaFieldParser};
use crate::field::FieldValue;

/// Resolved `number` field options.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct SchemaNumberField {
    min: Option<f64>,
    max: Option<f64>,
    step: Option<f64>,
}

impl SchemaNumberField {
    /// Return the inclusive minimum bound, if set.
    #[inline]
    #[must_use]
    pub(crate) fn min(&self) -> Option<f64> {
        self.min
    }

    /// Return the inclusive maximum bound, if set.
    #[inline]
    #[must_use]
    pub(crate) fn max(&self) -> Option<f64> {
        self.max
    }

    /// Return the increment step, if set.
    #[inline]
    #[must_use]
    pub(crate) fn step(&self) -> Option<f64> {
        self.step
    }

    /// Build an instance for tests.
    #[cfg(test)]
    pub(crate) fn for_test(
        min: Option<f64>,
        max: Option<f64>,
        step: Option<f64>,
    ) -> Self {
        Self {
            min,
            max,
            step,
        }
    }

    /// Parse `options` against `number`'s `min`/`max`/`step` attributes,
    /// merging with `base` when present.
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
        let (base_min, base_max, base_step) = match base {
            Some(SchemaFieldType::Number(base_def)) => {
                (base_def.min, base_def.max, base_def.step)
            }
            _ => (None, None, None),
        };

        let min = parser.f64(options, "min", base_min);
        let max = parser.f64(options, "max", base_max);
        let step = parser.f64(options, "step", base_step);

        SchemaFieldType::Number(SchemaNumberField {
            min,
            max,
            step,
        })
    }
}
