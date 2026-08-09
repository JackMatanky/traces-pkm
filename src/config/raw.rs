//! Deserializes TOML config into serde types that deny unknown fields.
//!
//! # Boundary
//!
//! Preserves TOML values exactly as configured. Path resolution and
//! local-over-global precedence are applied later by the builder pipeline.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Raw configuration data deserialized from TOML.
///
/// Shared by local and global config layers before merge precedence or path
/// resolution is applied.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawConfig {
    /// The `[templates]` table.
    #[serde(default)]
    pub(crate) templates: RawTemplateConfig,
    /// The `[schemas]` table.
    #[serde(default)]
    pub(crate) schemas: RawSchemasConfig,
    /// The `[frontmatter]` table.
    #[serde(default)]
    pub(crate) frontmatter: RawFrontmatterConfig,
}

/// Represents the raw `[templates]` table exactly as written in TOML.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawTemplateConfig {
    /// Template directory as configured, before joining against the config
    /// file's root.
    pub(crate) directory: Option<PathBuf>,
    /// Output directory for rendered templates. Relative values stay relative;
    /// absent values fall back to the config root.
    #[serde(default)]
    pub(crate) output_dir: Option<PathBuf>,
}

/// Represents the raw `[schemas]` table exactly as written in TOML.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawSchemasConfig {
    /// Frontmatter key naming a Note's File Class(es), before the `class`
    /// default is applied.
    pub(crate) class_field: Option<String>,
    /// Schema registry directory, before the `.traces/schemas/` default is
    /// applied.
    pub(crate) directory: Option<PathBuf>,
}

/// Represents the raw `[frontmatter]` table exactly as written in TOML.
///
/// Every field skips serialization when `None`: [`super::service`]'s Figment
/// merge re-serializes each parsed layer to overlay local onto global, and an
/// explicit `null` for an unconfigured local key would otherwise overwrite a
/// configured global value for that same key instead of leaving it absent.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawFrontmatterConfig {
    /// Frontmatter key holding a Note's display title.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) title: Option<String>,
    /// Frontmatter key holding a Note's aliases.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) aliases: Option<String>,
    /// Frontmatter key and date format used for the creation timestamp.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) date_created: Option<RawDateFieldConfig>,
    /// Frontmatter key and date format used for the modification timestamp.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) date_modified: Option<RawDateFieldConfig>,
}

/// A `{name, format}` pair naming a date-valued frontmatter key. Both fields
/// are optional; missing values are resolved to role-aware defaults by
/// [`super::model::DateFieldConfig`]. Each field skips serialization when
/// `None`, for the same Figment merge reason as [`RawFrontmatterConfig`].
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawDateFieldConfig {
    /// Frontmatter key name, if configured.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) name: Option<String>,
    /// Date format string applied to the key's value, if configured.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) format: Option<String>,
}
