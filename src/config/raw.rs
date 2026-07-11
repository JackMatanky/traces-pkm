//! Raw (unresolved) config types deserialized from TOML.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Raw (unresolved) configuration data deserialized from TOML.
///
/// Shared by both local and global config layers.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RawConfig {
    directory: Option<PathBuf>,
    #[serde(default)]
    output_dir: Option<PathBuf>,
}

impl RawConfig {
    /// The template directory from this raw config, if set.
    #[must_use]
    pub(super) fn template_directory(&self) -> Option<&Path> {
        self.directory.as_deref()
    }

    /// The output directory from this raw config, if set.
    #[must_use]
    pub(super) fn output_dir(&self) -> Option<&Path> {
        self.output_dir.as_deref()
    }
}
