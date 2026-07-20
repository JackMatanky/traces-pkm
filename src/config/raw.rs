//! Raw (unresolved) config types deserialized from TOML.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Raw (unresolved) configuration data deserialized from TOML.
///
/// Shared by both local and global config layers.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawConfig {
    #[serde(default)]
    pub(crate) templates: RawTemplateConfig,
}

/// Raw `[templates]` table.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawTemplateConfig {
    pub(crate) directory: Option<PathBuf>,
    #[serde(default)]
    pub(crate) output_dir: Option<PathBuf>,
}
