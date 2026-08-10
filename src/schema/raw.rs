//! Deserialize Schema TOML files into validated raw shapes.
//!
//! These serde types match the on-disk `.traces/schemas/<name>.toml` shape and
//! deny unknown fields, so a typo'd key fails at parse rather than silently
//! vanishing.
//!
//! # Boundary
//!
//! Preserves TOML values exactly as configured, but parses `$ref` strings and
//! a Field Definition's `type`/`$ref` source into validated shapes
//! ([`FieldAddress`], [`RawFieldSource`]) at
//! deserialization time: a `RawFieldDef` with neither `type` nor `$ref`
//! cannot exist past parsing. Inheritance, `$ref` resolution against other
//! Schemas, and the reserved Global Schema's `required` degrade are applied
//! later in [`super::resolve`].

use std::collections::BTreeMap;

use serde::{Deserialize, Deserializer};
use thiserror::Error;

use super::{address::FieldAddress, name::SchemaName};
use crate::field::FieldName;

/// Hold raw Schema data from one `.traces/schemas/<name>.toml` file.
///
/// The filename stem (not any field on this type) is the Schema name; see
/// [`super::SchemaRegistry::load`](super::registry::SchemaRegistry::load).
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawSchema {
    /// Store parent Schema names, first-listed wins when parents define the
    /// same field.
    #[serde(default)]
    pub(crate) extends: Vec<SchemaName>,
    /// Store field names dropped from inherited Field Definitions.
    #[serde(default)]
    pub(crate) excludes: Vec<FieldName>,
    /// Store Field Definitions keyed by field name.
    #[serde(default)]
    pub(crate) fields: BTreeMap<FieldName, RawFieldDef>,
}

/// Identify a raw field's declared source.
///
/// Parsed once at TOML deserialization time so a [`RawFieldDef`] with neither
/// `type` nor `$ref` cannot exist past parsing (see [`RawFieldDefError`]).
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum RawFieldSource {
    /// Use a `type` key with no `$ref`.
    Direct(RawFieldType),
    /// Use a `$ref` address to a base definition, with an optional local `type`
    /// override.
    Ref {
        address: FieldAddress,
        override_type: Option<RawFieldType>,
    },
}

/// Describe why a [`RawFieldDef`] failed to deserialize.
#[derive(Debug, Error)]
pub(crate) enum RawFieldDefError {
    /// Neither `type` nor `$ref` was present.
    #[error("field definition has neither `type` nor `$ref`")]
    MissingSource,
}

/// Hold raw field definition data parsed from TOML.
#[derive(Clone, Debug)]
pub(crate) struct RawFieldDef {
    /// Store the field's declared source: a direct `type`, or a `$ref`
    /// (optionally overriding its `type` locally).
    pub(crate) source: RawFieldSource,
    /// Whether the field must be set. Ignored (with a warning) on the reserved
    /// Global Schema.
    pub(crate) required: Option<bool>,
    /// Whether the field accepts multiple values.
    pub(crate) multi: Option<bool>,
    /// `select`-type selectable values.
    pub(crate) values: Option<Vec<String>>,
    /// `file`-type filter: folders to search under.
    pub(crate) folders: Option<Vec<String>>,
    /// `file`-type filter: file extension to match.
    pub(crate) ext: Option<String>,
    /// `file`-type filter: File Classes to match, is-a transitive.
    pub(crate) class: Option<Vec<String>>,
    /// `number`-type inclusive minimum.
    /// Inert, stored for schema authoring.
    pub(crate) min: Option<f64>,
    /// `number`-type inclusive maximum.
    /// Inert, stored for schema authoring.
    pub(crate) max: Option<f64>,
    /// `number`-type increment step.
    /// Inert today, stored like `required`/`multi` for schema authoring and
    /// future guardrails.
    pub(crate) step: Option<f64>,
    /// `date`-type display/parse format (strftime).
    /// Inert, stored for schema authoring.
    pub(crate) format: Option<String>,
}

impl<'de> Deserialize<'de> for RawFieldDef {
    /// Deserialize the `[fields.<name>]` TOML table, converting its
    /// `type`/`$ref` keys into a validated [`RawFieldSource`].
    ///
    /// # Errors
    ///
    /// Fails when neither `type` nor `$ref` is present, when `$ref` is not
    /// shaped `#<schema>/<field>`, or when any other key fails to parse (an
    /// unknown key, per `#[serde(deny_unknown_fields)]` on the wire shape).
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = RawFieldDefToml::deserialize(deserializer)?;
        let source = match (wire.field_type, wire.reference) {
            (Some(field_type), None) => RawFieldSource::Direct(field_type),
            (Some(field_type), Some(address)) => RawFieldSource::Ref {
                address,
                override_type: Some(field_type),
            },
            (None, Some(address)) => RawFieldSource::Ref {
                address,
                override_type: None,
            },
            (None, None) => {
                return Err(serde::de::Error::custom(
                    RawFieldDefError::MissingSource,
                ));
            }
        };
        Ok(Self {
            source,
            required: wire.required,
            multi: wire.multi,
            values: wire.values,
            folders: wire.folders,
            ext: wire.ext,
            class: wire.class,
            min: wire.min,
            max: wire.max,
            step: wire.step,
            format: wire.format,
        })
    }
}

/// Mirror the TOML wire shape for one `[fields.<name>]` table.
///
/// Mirrors the TOML exactly
/// (`type`/`$ref` still optional and separate) so
/// `#[serde(deny_unknown_fields)]` rejects a typo'd key. [`RawFieldDef`]
/// converts this into a validated [`RawFieldSource`] during deserialization;
/// nothing outside this module ever sees a `RawFieldDefToml`.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawFieldDefToml {
    /// Store the field kind. Optional only when `reference` supplies it.
    #[serde(rename = "type")]
    field_type: Option<RawFieldType>,
    /// Store a parsed `$ref` address shape.
    ///
    /// Raw deserialization only parses the address into a [`FieldAddress`].
    /// `RefResolver` later checks that it resolves to Global or an ancestor
    /// Schema field.
    #[serde(rename = "$ref")]
    reference: Option<FieldAddress>,
    required: Option<bool>,
    multi: Option<bool>,
    values: Option<Vec<String>>,
    folders: Option<Vec<String>>,
    ext: Option<String>,
    class: Option<Vec<String>>,
    min: Option<f64>,
    max: Option<f64>,
    step: Option<f64>,
    format: Option<String>,
}

impl RawFieldDef {
    /// Build a direct field definition of `field_type`, with
    /// every optional key unset.
    ///
    /// Test-only convenience constructor: tests needing `required`, `multi`,
    /// or type-specific options use struct-update syntax from the result.
    #[cfg(test)]
    #[inline]
    #[must_use]
    pub(crate) fn direct(field_type: RawFieldType) -> Self {
        Self {
            source: RawFieldSource::Direct(field_type),
            required: None,
            multi: None,
            values: None,
            folders: None,
            ext: None,
            class: None,
            min: None,
            max: None,
            step: None,
            format: None,
        }
    }

    /// Build a `$ref`-only field definition targeting `address`, with every
    /// optional key unset.
    #[cfg(test)]
    #[inline]
    #[must_use]
    pub(crate) fn reference(address: FieldAddress) -> Self {
        Self {
            source: RawFieldSource::Ref {
                address,
                override_type: None,
            },
            required: None,
            multi: None,
            values: None,
            folders: None,
            ext: None,
            class: None,
            min: None,
            max: None,
            step: None,
            format: None,
        }
    }
}

/// Represent the `type` key of a raw field definition.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum RawFieldType {
    /// Free-form text input.
    Input,
    /// Configured selectable values.
    Select,
    /// Boolean value.
    Boolean,
    /// Numeric value.
    Number,
    /// Date value.
    Date,
    /// File link with optional filters.
    File,
}

#[cfg(test)]
mod tests {
    mod raw_field_def {
        use pretty_assertions::assert_eq;

        use super::super::*;

        #[test]
        fn deserializes_a_direct_type() {
            let raw: RawFieldDef =
                toml::from_str(r#"type = "input""#).expect("valid toml");

            assert_eq!(raw.source, RawFieldSource::Direct(RawFieldType::Input));
        }

        #[test]
        fn deserializes_a_ref_only_definition() {
            let raw: RawFieldDef =
                toml::from_str(r##""$ref" = "#global/status""##)
                    .expect("valid toml");

            assert_eq!(raw.source, RawFieldSource::Ref {
                address: FieldAddress::try_from("#global/status")
                    .expect("valid ref"),
                override_type: None,
            });
        }

        #[test]
        fn deserializes_a_ref_with_a_local_type_override() {
            let raw: RawFieldDef = toml::from_str(
                r##"
                type = "file"
                "$ref" = "#global/cover"
                "##,
            )
            .expect("valid toml");

            assert_eq!(raw.source, RawFieldSource::Ref {
                address: FieldAddress::try_from("#global/cover")
                    .expect("valid ref"),
                override_type: Some(RawFieldType::File),
            });
        }

        #[test]
        fn rejects_a_definition_with_neither_type_nor_ref() {
            let err = toml::from_str::<RawFieldDef>("required = true")
                .expect_err("missing source rejected");

            assert!(
                err.to_string()
                    .contains("field definition has neither `type` nor `$ref`"),
                "unexpected error: {err}"
            );
        }

        #[test]
        fn rejects_an_unknown_key() {
            let err = toml::from_str::<RawFieldDef>(
                "type = \"input\"\ntypo_key = true",
            )
            .expect_err("unknown key rejected");

            assert!(
                err.to_string().contains("typo_key"),
                "unexpected error: {err}"
            );
        }

        #[test]
        fn deserializes_a_number_with_a_step() {
            let raw: RawFieldDef =
                toml::from_str("type = \"number\"\nstep = 0.5")
                    .expect("valid toml");

            assert_eq!(
                raw.source,
                RawFieldSource::Direct(RawFieldType::Number)
            );
            assert_eq!(raw.step, Some(0.5));
        }
    }
}
