//! Deserialization shapes for Schema TOML.
//!
//! These serde types match the on-disk `.traces/schemas/<name>.toml` shape and
//! deny unknown fields, so a typo'd key fails at parse rather than silently
//! vanishing.
//!
//! # Boundary
//!
//! This module preserves the TOML values exactly as configured, but parses
//! `$ref` strings and a Field Definition's `type`/`$ref` source into validated
//! shapes ([`FieldRef`], [`FieldSource`]) at deserialization time: a
//! `RawFieldDef` with neither `type` nor `$ref` cannot exist past parsing.
//! Inheritance, `$ref` resolution against other Schemas, and the reserved
//! Global Schema's `required` degrade are applied later in [`super::resolve`].

use std::{collections::BTreeMap, fmt, str::FromStr};

use serde::{Deserialize, Deserializer};
use thiserror::Error;

use super::name::SchemaName;
use crate::field::{FieldName, FieldNameError};

/// Raw Schema data deserialized from one `.traces/schemas/<name>.toml` file.
///
/// The filename stem (not any field on this type) is the Schema name; see
/// [`super::SchemaRegistry::load`].
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawSchema {
    /// Parent Schema names, first-listed wins when parents define the same
    /// field.
    #[serde(default)]
    pub(crate) extends: Vec<SchemaName>,
    /// Field names dropped from inherited (parent) Field Definitions.
    #[serde(default)]
    pub(crate) excludes: Vec<FieldName>,
    /// Field Definitions keyed by field name.
    #[serde(default)]
    pub(crate) fields: BTreeMap<FieldName, RawFieldDef>,
}

/// A Field Definition's declared source: either a direct `type`, or a `$ref` to
/// a base definition, optionally overriding its `type` locally.
///
/// Parsed once at TOML deserialization time, so a `RawFieldDef` with neither
/// `type` nor `$ref` cannot exist past parsing (see [`RawFieldDefError`]).
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum FieldSource {
    /// A `type` key with no `$ref`.
    Direct(RawFieldType),
    /// A `$ref` to a base definition, with an optional local `type` override.
    Ref {
        reference: FieldRef,
        override_type: Option<RawFieldType>,
    },
}

/// A bounded `$ref` value: `#<schema>/<field>`, naming the Global Schema or a
/// transitive `extends` ancestor and one of its fields.
///
/// Schema-specific (unlike [`FieldName`] and [`FieldKey`]): it pairs a
/// [`SchemaName`] with a [`FieldName`] and has no meaning outside a Schema's
/// own `$ref` resolution.
///
/// [`FieldKey`]: crate::field::FieldKey
#[derive(Clone, Eq, PartialEq)]
pub(crate) struct FieldRef {
    schema: SchemaName,
    field: FieldName,
}

/// A [`FieldRef`] failed to parse.
#[derive(Debug, Error)]
pub(crate) enum FieldRefError {
    /// `reference` was not shaped `#<schema>/<field>` with both segments
    /// non-empty.
    #[error("malformed $ref {reference:?}: expected `#<schema>/<field>`")]
    Malformed {
        reference: String,
    },
    /// The field segment failed [`FieldName`] validation.
    #[error(transparent)]
    FieldName(#[from] FieldNameError),
}

/// A `RawFieldDef` declared neither `type` nor `$ref`.
#[derive(Debug, Error)]
pub(crate) enum RawFieldDefError {
    /// Neither `type` nor `$ref` was present.
    #[error("field definition has neither `type` nor `$ref`")]
    MissingSource,
}

impl FieldRef {
    /// Returns the referenced Schema's name.
    #[inline]
    #[must_use]
    pub(crate) fn schema(&self) -> &SchemaName {
        &self.schema
    }

    /// Returns the referenced field's name.
    #[inline]
    #[must_use]
    pub(crate) fn field(&self) -> &FieldName {
        &self.field
    }
}

impl TryFrom<&str> for FieldRef {
    type Error = FieldRefError;

    /// # Errors
    ///
    /// Returns [`FieldRefError::Malformed`] when `raw` is not shaped
    /// `#<schema>/<field>` with both segments non-empty, and
    /// [`FieldRefError::FieldName`] when the field segment fails
    /// [`FieldName`] validation.
    fn try_from(raw: &str) -> Result<Self, Self::Error> {
        let malformed = || FieldRefError::Malformed {
            reference: raw.to_owned(),
        };
        let stripped = raw.strip_prefix('#').ok_or_else(malformed)?;
        let (schema, field) = stripped.split_once('/').ok_or_else(malformed)?;
        if schema.is_empty() || field.is_empty() {
            return Err(malformed());
        }
        Ok(Self {
            schema: SchemaName::from(schema),
            field: FieldName::try_from(field)?,
        })
    }
}

impl TryFrom<String> for FieldRef {
    type Error = FieldRefError;

    /// # Errors
    ///
    /// Returns [`FieldRefError::Malformed`] when `raw` is not shaped
    /// `#<schema>/<field>` with both segments non-empty, and
    /// [`FieldRefError::FieldName`] when the field segment fails
    /// [`FieldName`] validation.
    fn try_from(raw: String) -> Result<Self, Self::Error> {
        Self::try_from(raw.as_str())
    }
}

impl FromStr for FieldRef {
    type Err = FieldRefError;

    /// # Errors
    ///
    /// Returns [`FieldRefError::Malformed`] when `raw` is not shaped
    /// `#<schema>/<field>` with both segments non-empty, and
    /// [`FieldRefError::FieldName`] when the field segment fails
    /// [`FieldName`] validation.
    fn from_str(raw: &str) -> Result<Self, Self::Err> {
        Self::try_from(raw)
    }
}

impl fmt::Display for FieldRef {
    /// Writes `#<schema>/<field>` directly into the formatter, without
    /// allocating an intermediate `String`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{}/{}", self.schema, self.field)
    }
}

impl fmt::Debug for FieldRef {
    /// Matches `str`'s own `Debug` (quoted, escaped) applied to this `$ref`'s
    /// `#<schema>/<field>` display form, so wrapping a `$ref` in this type
    /// never changes an error message's text.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.to_string(), f)
    }
}

impl<'de> Deserialize<'de> for FieldRef {
    /// Deserializes from a string shaped `#<schema>/<field>` and validates
    /// it as a [`FieldRef`].
    ///
    /// # Errors
    ///
    /// See [`FieldRef::try_from`].
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        Self::try_from(raw).map_err(serde::de::Error::custom)
    }
}

/// Raw Field Definition data exactly as written in TOML, with `type`/`$ref`
/// already parsed into a validated [`FieldSource`].
#[derive(Clone, Debug)]
pub(crate) struct RawFieldDef {
    /// The field's declared source: a direct `type`, or a `$ref` (optionally
    /// overriding its `type` locally).
    pub(crate) source: FieldSource,
    /// Whether the field must be set. Ignored (with a warning) on the
    /// reserved Global Schema.
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
}

/// Wire shape for one `.traces/schemas/<name>.toml` `[fields.<name>]` table:
/// mirrors the TOML exactly (`type`/`$ref` still optional and separate), so
/// `#[serde(deny_unknown_fields)]` still rejects a typo'd key. [`RawFieldDef`]
/// itself converts this into a validated [`FieldSource`] during
/// deserialization; nothing outside this module ever sees a `RawFieldDefToml`.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawFieldDefToml {
    /// The field's kind. Optional only when `reference` supplies it.
    #[serde(rename = "type")]
    field_type: Option<RawFieldType>,
    /// A bounded `$ref` to a base definition: `#global/<field>` or
    /// `#<ancestor-schema>/<field>`.
    #[serde(rename = "$ref")]
    reference: Option<FieldRef>,
    required: Option<bool>,
    multi: Option<bool>,
    values: Option<Vec<String>>,
    folders: Option<Vec<String>>,
    ext: Option<String>,
    class: Option<Vec<String>>,
}

impl<'de> Deserialize<'de> for RawFieldDef {
    /// Deserializes the `[fields.<name>]` TOML table, converting its
    /// `type`/`$ref` keys into a validated [`FieldSource`].
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
            (Some(field_type), None) => FieldSource::Direct(field_type),
            (Some(field_type), Some(reference)) => FieldSource::Ref {
                reference,
                override_type: Some(field_type),
            },
            (None, Some(reference)) => FieldSource::Ref {
                reference,
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
        })
    }
}

impl RawFieldDef {
    /// Builds a direct (non-`$ref`) Field Definition of `field_type`, with
    /// every optional key unset.
    ///
    /// Test/pub(crate) convenience constructor: tests needing `required`,
    /// `multi`, or type-specific options use struct-update syntax from the
    /// result (`RawFieldDef { values: Some(...), ..RawFieldDef::direct(...)
    /// }`).
    #[inline]
    #[must_use]
    #[cfg_attr(not(test), expect(dead_code, reason = "used in tests"))]
    pub(crate) fn direct(field_type: RawFieldType) -> Self {
        Self {
            source: FieldSource::Direct(field_type),
            required: None,
            multi: None,
            values: None,
            folders: None,
            ext: None,
            class: None,
        }
    }

    /// Builds a `$ref`-only Field Definition targeting `reference`, with
    /// every optional key unset.
    #[inline]
    #[must_use]
    #[cfg_attr(not(test), expect(dead_code, reason = "used in tests"))]
    pub(crate) fn reference(reference: FieldRef) -> Self {
        Self {
            source: FieldSource::Ref {
                reference,
                override_type: None,
            },
            required: None,
            multi: None,
            values: None,
            folders: None,
            ext: None,
            class: None,
        }
    }
}

/// The `type` key of a raw Field Definition.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum RawFieldType {
    Input,
    Select,
    Boolean,
    Number,
    Date,
    File,
}

#[cfg(test)]
mod tests {
    mod field_ref {
        use pretty_assertions::assert_eq;

        use super::super::*;

        #[test]
        fn parses_a_well_formed_reference() {
            let target = FieldRef::try_from("#book/status").expect("parses");

            assert_eq!(target.schema(), &SchemaName::from("book"));
            assert_eq!(target.field().as_str(), "status");
        }

        #[test]
        fn rejects_a_reference_missing_the_hash_prefix() {
            assert!(matches!(
                FieldRef::try_from("book/status"),
                Err(FieldRefError::Malformed { .. })
            ));
        }

        #[test]
        fn rejects_a_reference_missing_the_slash_separator() {
            assert!(matches!(
                FieldRef::try_from("#bookstatus"),
                Err(FieldRefError::Malformed { .. })
            ));
        }

        #[test]
        fn rejects_a_reference_with_an_empty_schema_segment() {
            assert!(matches!(
                FieldRef::try_from("#/status"),
                Err(FieldRefError::Malformed { .. })
            ));
        }

        #[test]
        fn rejects_a_reference_with_an_empty_field_segment() {
            assert!(matches!(
                FieldRef::try_from("#book/"),
                Err(FieldRefError::Malformed { .. })
            ));
        }

        #[test]
        fn rejects_a_reference_whose_field_segment_fails_field_name_validation()
        {
            assert!(matches!(
                FieldRef::try_from("#book/!!!"),
                Err(FieldRefError::FieldName(_))
            ));
        }

        #[test]
        fn display_round_trips_the_original_shape() {
            let target = FieldRef::try_from("#book/status").expect("parses");

            assert_eq!(target.to_string(), "#book/status");
        }

        #[test]
        fn debug_matches_the_quoted_display_form() {
            let target = FieldRef::try_from("#book/status").expect("parses");

            assert_eq!(format!("{target:?}"), "\"#book/status\"");
        }
    }

    mod raw_field_def {
        use pretty_assertions::assert_eq;

        use super::super::*;

        #[test]
        fn deserializes_a_direct_type() {
            let raw: RawFieldDef =
                toml::from_str(r#"type = "input""#).expect("valid toml");

            assert_eq!(raw.source, FieldSource::Direct(RawFieldType::Input));
        }

        #[test]
        fn deserializes_a_ref_only_definition() {
            let raw: RawFieldDef =
                toml::from_str(r##""$ref" = "#global/status""##)
                    .expect("valid toml");

            assert_eq!(raw.source, FieldSource::Ref {
                reference: FieldRef::try_from("#global/status")
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

            assert_eq!(raw.source, FieldSource::Ref {
                reference: FieldRef::try_from("#global/cover")
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
    }
}
