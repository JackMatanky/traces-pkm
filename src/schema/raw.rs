//! On-disk Schema TOML shapes and serde deserialization.
//!
//! These types match `.traces/schemas/<name>.toml` and deny unknown fields.
//! `$ref` strings and `type`/`$ref` source are parsed into validated shapes
//! ([`FieldAddress`], [`RawSchemaFieldSource`]) at deserialization time.
//! Type-specific keys land in [`RawSchemaFieldDef::options`] as generic
//! [`FieldValue`]s; their validation is
//! [`SchemaFieldBuilder::build`](super::fields::SchemaFieldBuilder::build)'s
//! job.

use std::collections::BTreeMap;

use serde::{Deserialize, Deserializer};
use thiserror::Error;

use super::{SchemaName, fields::FieldAddress};
use crate::field::{FieldName, FieldValue};

/// One `.traces/schemas/<name>.toml` file, parsed but not yet resolved.
///
/// The filename stem (not any field on this type) is the Schema name.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawSchema {
    /// Parent Schema names, first-listed wins on shared fields.
    #[serde(default)]
    pub(crate) extends: Vec<SchemaName>,
    /// Field names to drop from inherited definitions.
    #[serde(default)]
    pub(crate) excludes: Vec<FieldName>,
    /// Field definitions keyed by name.
    #[serde(default)]
    pub(crate) fields: BTreeMap<FieldName, RawSchemaFieldDef>,
}

/// A field definition parsed from TOML, before `$ref` resolution.
#[derive(Clone, Debug)]
pub(crate) struct RawSchemaFieldDef {
    /// The field's source: a direct `type` or a `$ref` with optional override.
    pub(crate) source: RawSchemaFieldSource,
    /// Whether the field must be set. Ignored (with a warning) on the Global
    /// Schema.
    pub(crate) required: Option<bool>,
    /// Whether the field accepts multiple values.
    pub(crate) multi: Option<bool>,
    /// Type-specific options as raw [`FieldValue`]s, keyed by TOML name.
    pub(crate) options: BTreeMap<String, FieldValue>,
}

impl RawSchemaFieldDef {
    /// Build a `Direct` field definition with no options.
    ///
    /// Test-only: production code deserializes from TOML.
    #[cfg(test)]
    #[inline]
    #[must_use]
    pub(crate) fn direct(kind: RawSchemaFieldType) -> Self {
        Self {
            source: RawSchemaFieldSource::Direct(kind),
            required: None,
            multi: None,
            options: BTreeMap::new(),
        }
    }

    /// Build a `$ref`-only field definition with no options.
    #[cfg(test)]
    #[inline]
    #[must_use]
    pub(crate) fn reference(address: FieldAddress) -> Self {
        Self {
            source: RawSchemaFieldSource::Ref {
                address,
                override_type: None,
            },
            required: None,
            multi: None,
            options: BTreeMap::new(),
        }
    }
}

impl<'de> Deserialize<'de> for RawSchemaFieldDef {
    /// Deserialize the `[fields.<name>]` TOML table, converting its
    /// `type`/`$ref` keys into a validated [`RawSchemaFieldSource`] and every
    /// other present key into [`RawSchemaFieldDef::options`].
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
            (Some(kind), None) => RawSchemaFieldSource::Direct(kind),
            (Some(kind), Some(address)) => RawSchemaFieldSource::Ref {
                address,
                override_type: Some(kind),
            },
            (None, Some(address)) => RawSchemaFieldSource::Ref {
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

/// The `type` key of a raw field definition.
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

/// How a raw field was declared: `type` alone, or `$ref` with optional
/// `type` override.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum RawSchemaFieldSource {
    /// Use a `type` key with no `$ref`.
    Direct(RawSchemaFieldType),
    /// Use a `$ref` address to a base definition, with an optional local `type`
    /// override.
    Ref {
        address: FieldAddress,
        override_type: Option<RawSchemaFieldType>,
    },
}

/// Why a [`RawSchemaFieldDef`] failed to deserialize.
#[derive(Debug, Error)]
pub(crate) enum RawFieldDefError {
    /// Neither `type` nor `$ref` was present.
    #[error("field definition has neither `type` nor `$ref`")]
    MissingSource,
}

/// Wire shape for one `[fields.<name>]` TOML table.
///
/// `type`/`$ref` are optional and separate; every type-specific key
/// deserializes as a generic [`FieldValue`]. Converts into a validated
/// [`RawSchemaFieldSource`] plus [`RawSchemaFieldDef::options`] during
/// deserialization.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawFieldDefToml {
    /// The field's `type` key. Optional when `reference` supplies it.
    #[serde(rename = "type")]
    kind: Option<RawSchemaFieldType>,
    /// A parsed `$ref` address. Resolution happens in
    /// [`super::fields::RefAddressResolver`].
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
                RawSchemaFieldSource::Direct(RawSchemaFieldType::Input)
            );
        }

        #[test]
        fn deserializes_a_ref_only_definition() {
            let raw: RawSchemaFieldDef =
                toml::from_str(r##""$ref" = "#global/status""##)
                    .expect("valid toml");

            assert_eq!(raw.source, RawSchemaFieldSource::Ref {
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

            assert_eq!(raw.source, RawSchemaFieldSource::Ref {
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
            // known (see `SchemaFieldParserError::TypeMismatch`).
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
                RawSchemaFieldSource::Direct(RawSchemaFieldType::Number)
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
