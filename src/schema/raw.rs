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
//! ([`FieldAddress`], [`RawFieldSource`]) at deserialization time: a
//! [`RawSchemaFieldDef`] with neither `type` nor `$ref` cannot exist past
//! parsing. Every other type-specific key (`values`, `folders`/`ext`/`class`,
//! `min`/`max`/`step`, `format`) lands in [`RawSchemaFieldDef::options`] as a
//! generic [`FieldValue`], not a fixed Rust type: whether a key belongs to the
//! field's resolved type, and whether its value is shaped correctly, is
//! [`super::fields::SchemaFieldBuilder`]'s job, not this module's. Inheritance,
//! `$ref` resolution against other Schemas, and the reserved Global Schema's
//! `required` degrade are applied later in [`super::service`].

use std::collections::BTreeMap;

use serde::{Deserialize, Deserializer};
use thiserror::Error;

use super::{address::FieldAddress, name::SchemaName};
use crate::field::{FieldName, FieldValue};

/// Hold raw Schema data from one `.traces/schemas/<name>.toml` file.
///
/// The filename stem (not any field on this type) is the Schema name; see
/// [`super::SchemaService::resolve`](super::service::SchemaService::resolve).
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
    pub(crate) fields: BTreeMap<FieldName, RawSchemaFieldDef>,
}

/// Hold raw field definition data parsed from TOML.
#[derive(Clone, Debug)]
pub(crate) struct RawSchemaFieldDef {
    /// Store the field's declared source: a direct `type`, or a `$ref`
    /// (optionally overriding its `type` locally).
    pub(crate) source: RawFieldSource,
    /// Whether the field must be set. Ignored (with a warning) on the reserved
    /// Global Schema.
    pub(crate) required: Option<bool>,
    /// Whether the field accepts multiple values.
    pub(crate) multi: Option<bool>,
    /// Every type-specific key this field declared (`values`,
    /// `folders`/`ext`/`class`, `min`/`max`/`step`, `format`), keyed by its
    /// TOML name and preserved as a generic [`FieldValue`].
    ///
    /// A flat bag rather than one Rust field per key: whether a key belongs to
    /// this field's resolved type, and whether its value is shaped correctly
    /// for that key, is validated by
    /// [`super::fields::SchemaFieldBuilder`] once the field's effective type
    /// is known — a `date` field declaring `values`, or a `number` field
    /// declaring `min = "abc"`, parses fine here and fails there.
    pub(crate) options: BTreeMap<String, FieldValue>,
}

impl<'de> Deserialize<'de> for RawSchemaFieldDef {
    /// Deserialize the `[fields.<name>]` TOML table, converting its
    /// `type`/`$ref` keys into a validated [`RawFieldSource`] and every other
    /// present key into [`RawSchemaFieldDef::options`].
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
        let source = match (wire.kind, wire.reference) {
            (Some(kind), None) => RawFieldSource::Direct(kind),
            (Some(kind), Some(address)) => RawFieldSource::Ref {
                address,
                override_type: Some(kind),
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
        let mut options = BTreeMap::new();
        for (key, value) in [
            ("values", wire.values),
            ("folders", wire.folders),
            ("ext", wire.ext),
            ("class", wire.class),
            ("min", wire.min),
            ("max", wire.max),
            ("step", wire.step),
            ("format", wire.format),
        ] {
            if let Some(value) = value {
                options.insert(key.to_owned(), value);
            }
        }
        Ok(Self {
            source,
            required: wire.required,
            multi: wire.multi,
            options,
        })
    }
}

impl RawSchemaFieldDef {
    /// Build a direct field definition of `kind`, with no type-specific
    /// options.
    ///
    /// Test-only convenience constructor: tests needing `required`, `multi`,
    /// or type-specific options use struct-update syntax from the result.
    #[cfg(test)]
    #[inline]
    #[must_use]
    pub(crate) fn direct(kind: RawSchemaFieldType) -> Self {
        Self {
            source: RawFieldSource::Direct(kind),
            required: None,
            multi: None,
            options: BTreeMap::new(),
        }
    }

    /// Build a `$ref`-only field definition targeting `address`, with no
    /// type-specific options.
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
            options: BTreeMap::new(),
        }
    }
}

/// Represent the `type` key of a raw field definition.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum RawSchemaFieldType {
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

impl std::fmt::Display for RawSchemaFieldType {
    /// Writes the lowercase `type` key value a Schema author would write in
    /// TOML (`"select"`, not `"Select"`), so field-attribute error messages
    /// read like the source they describe.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Input => "input",
            Self::Select => "select",
            Self::Boolean => "boolean",
            Self::Number => "number",
            Self::Date => "date",
            Self::File => "file",
        })
    }
}

/// Identify a raw field's declared source.
///
/// Parsed once at TOML deserialization time so a [`RawSchemaFieldDef`] with
/// neither `type` nor `$ref` cannot exist past parsing (see
/// [`RawFieldDefError`]).
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum RawFieldSource {
    /// Use a `type` key with no `$ref`.
    Direct(RawSchemaFieldType),
    /// Use a `$ref` address to a base definition, with an optional local `type`
    /// override.
    Ref {
        address: FieldAddress,
        override_type: Option<RawSchemaFieldType>,
    },
}

/// Describe why a [`RawSchemaFieldDef`] failed to deserialize.
#[derive(Debug, Error)]
pub(crate) enum RawFieldDefError {
    /// Neither `type` nor `$ref` was present.
    #[error("field definition has neither `type` nor `$ref`")]
    MissingSource,
}

/// Mirror the TOML wire shape for one `[fields.<name>]` table.
///
/// Mirrors the TOML exactly (`type`/`$ref` still optional and separate) so
/// `#[serde(deny_unknown_fields)]` rejects a typo'd key. Every type-specific
/// key deserializes as a generic [`FieldValue`], not a fixed Rust type: a
/// `min = "abc"` on a `number` field parses fine here and fails validation
/// later, in [`super::fields::SchemaFieldBuilder`], as
/// `AttributeValueTypeMismatch` rather than a TOML parse error.
/// [`RawSchemaFieldDef`] converts this into a validated [`RawFieldSource`] plus
/// an `options` map during deserialization; nothing outside this module ever
/// sees a `RawFieldDefToml`.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawFieldDefToml {
    /// Store the field kind. Optional only when `reference` supplies it.
    #[serde(rename = "type")]
    kind: Option<RawSchemaFieldType>,
    /// Store a parsed `$ref` address shape.
    ///
    /// Raw deserialization only parses the address into a [`FieldAddress`].
    /// `RefResolver` later checks that it resolves to Global or an ancestor
    /// Schema field.
    #[serde(rename = "$ref")]
    reference: Option<FieldAddress>,
    required: Option<bool>,
    multi: Option<bool>,
    values: Option<FieldValue>,
    folders: Option<FieldValue>,
    ext: Option<FieldValue>,
    class: Option<FieldValue>,
    min: Option<FieldValue>,
    max: Option<FieldValue>,
    step: Option<FieldValue>,
    format: Option<FieldValue>,
}

#[cfg(test)]
mod tests {
    mod raw_schema_field_def {
        use pretty_assertions::assert_eq;

        use super::super::*;

        #[test]
        fn deserializes_a_direct_type() {
            let raw: RawSchemaFieldDef =
                toml::from_str(r#"type = "input""#).expect("valid toml");

            assert_eq!(
                raw.source,
                RawFieldSource::Direct(RawSchemaFieldType::Input)
            );
        }

        #[test]
        fn deserializes_a_ref_only_definition() {
            let raw: RawSchemaFieldDef =
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
            let raw: RawSchemaFieldDef = toml::from_str(
                r##"
                type = "file"
                "$ref" = "#global/cover"
                "##,
            )
            .expect("valid toml");

            assert_eq!(raw.source, RawFieldSource::Ref {
                address: FieldAddress::try_from("#global/cover")
                    .expect("valid ref"),
                override_type: Some(RawSchemaFieldType::File),
            });
        }

        #[test]
        fn rejects_a_definition_with_neither_type_nor_ref() {
            let err = toml::from_str::<RawSchemaFieldDef>("required = true")
                .expect_err("missing source rejected");

            assert!(
                err.to_string()
                    .contains("field definition has neither `type` nor `$ref`"),
                "unexpected error: {err}"
            );
        }

        #[test]
        fn rejects_an_unknown_key() {
            let err = toml::from_str::<RawSchemaFieldDef>(
                "type = \"input\"\ntypo_key = true",
            )
            .expect_err("unknown key rejected");

            assert!(
                err.to_string().contains("typo_key"),
                "unexpected error: {err}"
            );
        }

        #[test]
        fn collects_declared_type_specific_keys_into_options() {
            let raw: RawSchemaFieldDef = toml::from_str(
                "type = \"select\"\nvalues = [\"draft\", \"done\"]",
            )
            .expect("valid toml");

            assert_eq!(
                raw.options.get("values"),
                Some(&FieldValue::List(vec![
                    FieldValue::String("draft".to_owned()),
                    FieldValue::String("done".to_owned()),
                ]))
            );
        }

        #[test]
        fn omits_absent_type_specific_keys_from_options() {
            let raw: RawSchemaFieldDef =
                toml::from_str("type = \"input\"").expect("valid toml");

            assert!(raw.options.is_empty());
        }

        #[test]
        fn accepts_a_wrongly_shaped_value_at_parse_leaving_validation_to_the_builder()
         {
            // `min` is a `number`-type key, but nothing at this layer knows
            // this field is (or isn't) a `number` field, or that "abc" is the
            // wrong shape for `min`: both checks are
            // `SchemaFieldBuilder`'s job once the field's resolved type is
            // known (see `schema::fields`'s `AttributeValueTypeMismatch`).
            let raw: RawSchemaFieldDef =
                toml::from_str("type = \"number\"\nmin = \"abc\"")
                    .expect("valid toml: value shape isn't validated here");

            assert_eq!(
                raw.options.get("min"),
                Some(&FieldValue::String("abc".to_owned()))
            );
        }

        #[test]
        fn deserializes_a_number_with_a_step() {
            let raw: RawSchemaFieldDef =
                toml::from_str("type = \"number\"\nstep = 0.5")
                    .expect("valid toml");

            assert_eq!(
                raw.source,
                RawFieldSource::Direct(RawSchemaFieldType::Number)
            );
            assert_eq!(raw.options.get("step"), Some(&FieldValue::Float(0.5)));
        }
    }

    mod raw_field_type {
        use pretty_assertions::assert_eq;
        use rstest::rstest;

        use super::super::*;

        #[rstest]
        #[case::input(RawSchemaFieldType::Input, "input")]
        #[case::select(RawSchemaFieldType::Select, "select")]
        #[case::boolean(RawSchemaFieldType::Boolean, "boolean")]
        #[case::number(RawSchemaFieldType::Number, "number")]
        #[case::date(RawSchemaFieldType::Date, "date")]
        #[case::file(RawSchemaFieldType::File, "file")]
        fn display_matches_the_toml_type_key_value(
            #[case] kind: RawSchemaFieldType,
            #[case] expected: &str,
        ) {
            assert_eq!(kind.to_string(), expected);
        }
    }
}
