//! Raw (unresolved) config types deserialized from TOML.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Raw (unresolved) configuration data deserialized from TOML.
///
/// Shared by both local and global config layers.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawConfig {
    #[serde(default)]
    templates: RawTemplateConfig,
}

impl RawConfig {
    /// Creates a raw config with both supported template settings present.
    #[must_use]
    #[inline]
    pub(crate) fn new(directory: PathBuf, output_dir: PathBuf) -> Self {
        Self {
            templates: RawTemplateConfig {
                directory: Some(directory),
                output_dir: Some(output_dir),
            },
        }
    }

    /// The template directory from this raw config, if set.
    #[must_use]
    pub(super) fn template_directory(&self) -> Option<&Path> {
        self.templates.directory.as_deref()
    }

    /// The output directory from this raw config, if set.
    #[must_use]
    pub(super) fn output_dir(&self) -> Option<&Path> {
        self.templates.output_dir.as_deref()
    }
}

/// Raw `[templates]` table.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct RawTemplateConfig {
    directory: Option<PathBuf>,
    #[serde(default)]
    output_dir: Option<PathBuf>,
}
