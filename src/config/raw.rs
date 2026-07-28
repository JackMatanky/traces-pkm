//! Config types deserialized directly from TOML, before path resolution.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Raw (unresolved) configuration data deserialized from TOML.
///
/// Shared by both local and global config layers.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawConfig {
    /// The `[templates]` table.
    #[serde(default)]
    pub(crate) templates: RawTemplateConfig,
}

/// Raw `[templates]` table.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawTemplateConfig {
    /// Template directory as configured; joined against the config file's
    /// root to resolve an absolute path.
    pub(crate) directory: Option<PathBuf>,
    /// Output directory for rendered templates, used verbatim (relative or
    /// absolute) when set. Falls back to the config root when absent.
    #[serde(default)]
    pub(crate) output_dir: Option<PathBuf>,
}
