//! Deserialization shapes for config TOML.
//!
//! These serde types match the on-disk schema and deny unknown fields.
//!
//! # Boundary
//!
//! This module preserves TOML values exactly as configured. Path resolution and
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
}

/// Raw `[templates]` table exactly as written in TOML.
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
