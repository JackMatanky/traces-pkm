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
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawFrontmatterConfig {
    /// Frontmatter key holding a Note's display title.
    pub(crate) title: Option<String>,
    /// Frontmatter key holding a Note's aliases.
    pub(crate) aliases: Option<String>,
    /// Frontmatter key and date format used for the creation timestamp.
    pub(crate) date_created: Option<RawDateFieldConfig>,
    /// Frontmatter key and date format used for the modification timestamp.
    pub(crate) date_modified: Option<RawDateFieldConfig>,
}

/// A `{name, format}` pair naming a date-valued frontmatter key.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawDateFieldConfig {
    /// Frontmatter key name.
    pub(crate) name: String,
    /// Date format string applied to the key's value.
    pub(crate) format: String,
}
