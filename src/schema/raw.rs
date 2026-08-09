//! Deserialization shapes for Schema TOML.
//!
//! These serde types match the on-disk `.traces/schemas/<name>.toml` shape and
//! deny unknown fields, so a typo'd key fails at parse rather than silently
//! vanishing.
//!
//! # Boundary
//!
//! This module preserves the TOML values exactly as configured. Inheritance,
//! `$ref` resolution, and the reserved Global Schema's `required` degrade are
//! applied later in [`super::resolve`].

use std::collections::BTreeMap;

use serde::Deserialize;

use super::name::SchemaName;

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
    pub(crate) excludes: Vec<String>,
    /// Field Definitions keyed by field name.
    #[serde(default)]
    pub(crate) fields: BTreeMap<String, RawFieldDef>,
}

/// Raw Field Definition data exactly as written in TOML.
///
/// Either `field_type` or `reference` (`$ref`) must be present: a
/// [`super::error::SchemaError::MissingFieldType`] is raised during resolution
/// when both are absent. When `reference` is present, any other field set here
/// overrides the same key on the referenced base definition; an absent key
/// inherits the base's value.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawFieldDef {
    /// The field's kind. Optional only when `reference` supplies it.
    #[serde(rename = "type")]
    pub(crate) field_type: Option<RawFieldType>,
    /// A bounded `$ref` to a base definition: `#global/<field>` or
    /// `#<ancestor-schema>/<field>`.
    #[serde(rename = "$ref")]
    pub(crate) reference: Option<String>,
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
