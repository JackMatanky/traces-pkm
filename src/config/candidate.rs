//! Config file discovery candidates and their origin metadata.

use std::path::{Path, PathBuf};

/// Opaque config candidate returned by the discovery pipeline.
///
/// This is a pipeline token consumed by `ConfigBuilder`. Its fields are
/// intentionally private; callers pass it through unchanged rather than
/// inspect or construct it.
#[derive(Clone, Debug)]
pub struct CandidateConfigFile {
    root: PathBuf,
    source: ConfigSource,
}

impl CandidateConfigFile {
    /// Creates a new candidate.
    #[inline]
    #[must_use]
    pub(super) fn new(root: PathBuf, source: ConfigSource) -> Self {
        Self {
            root,
            source,
        }
    }

    /// The project root or global config parent directory.
    #[inline]
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Origin of this config file.
    #[inline]
    #[must_use]
    pub fn source(&self) -> &ConfigSource {
        &self.source
    }

    /// Absolute path to the config file on disk.
    #[inline]
    #[must_use]
    pub fn path(&self) -> &Path {
        match &self.source {
            ConfigSource::Local(path) | ConfigSource::Global(path) => path,
        }
    }
}

/// Origin of a discovered configuration.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ConfigSource {
    /// Loaded from a local `.traces/config.toml`.
    Local(PathBuf),
    /// Loaded from the user's global config file.
    Global(PathBuf),
}
