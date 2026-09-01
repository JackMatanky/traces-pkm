//! Errors from config discovery, file validation, loading, and trust-state
//! operations.
//!
//! Every variant here crosses a `config` submodule boundary: raised in one file
//! and consumed in another, or composed by [`ConfigLoadError`] into the result
//! [`crate::cli::CliError`] reports. File-local parsing or construction
//! failures that never propagate past their origin file stay defined there.

use std::{io, path::PathBuf};

use thiserror::Error;

use super::{
    discovery::DiscoveryScope,
    file::{LocalConfigFile, Tracked},
    trust::ConfigTrustStatus,
};
use crate::{
    FieldNameError, FileStateStoreError, hash::HashError, tag::TagError,
};

/// Errors from the full config-loading pipeline.
#[derive(Debug, Error)]
pub(crate) enum ConfigLoadError {
    /// Discovery failed before any config could be loaded.
    #[error(transparent)]
    Discovery(#[from] DiscoveryError),
    /// Build failed after discovery selected candidate config files.
    #[error(transparent)]
    Build(#[from] ConfigBuilderError),
}

/// Convenience alias for config discovery operations.
pub(crate) type DiscoveryResult<T> = std::result::Result<T, DiscoveryError>;

/// Errors raised while finding candidate config files.
#[derive(Debug, Error)]
pub(crate) enum DiscoveryError {
    /// No local `.traces/config.toml` was found in any ancestor directory.
    #[error("no local config found from {cwd}")]
    LocalConfigAbsent {
        /// The working directory from which discovery started.
        cwd: PathBuf,
    },
    /// A filesystem path could not be inspected during discovery.
    #[error("failed to access path {path} during discovery")]
    PathInaccessible {
        /// Path that could not be accessed.
        path: PathBuf,
        /// Source I/O error.
        #[source]
        source: io::Error,
    },
    /// [`DiscoveryScope::Full`] does not accept a file anchor.
    ///
    /// Full discovery always requires a directory root so it can walk ancestors
    /// to find the nearest local config.
    #[error("{kind:?} discovery cannot be anchored at file {path}")]
    UnsupportedFileAnchor {
        /// Discovery kind that rejected the anchor.
        kind: DiscoveryScope,
        /// Unsupported file anchor path.
        path: PathBuf,
    },
    /// [`DiscoveryScope::Full`] cannot be used for trust-request resolution.
    ///
    /// Trust resolution supports only [`DiscoveryScope::NearestLocal`] and
    /// [`DiscoveryScope::LocalSubtree`].
    #[error("{scope:?} discovery cannot be used for trust request resolution")]
    UnsupportedTrustScope {
        /// Unsupported discovery scope.
        scope: DiscoveryScope,
    },
    /// A discovered config file path or source combination was invalid.
    #[error(transparent)]
    ConfigFile(#[from] ConfigFileError),
}

/// Errors raised while building a [`super::Config`] from discovered files.
#[derive(Debug, Error)]
pub(crate) enum ConfigBuilderError {
    /// The builder requires [`DiscoveryScope::Full`] output but received a
    /// different scope.
    #[error(
        "config builder input requires full discovery output, got {actual:?}"
    )]
    WrongDiscoveryKindForBuild {
        /// Actual discovery kind received.
        actual: DiscoveryScope,
    },
    /// Full discovery produced no local config candidates.
    #[error("full discovery output did not contain a local config")]
    FullDiscoveryWithoutLocal,
    /// Full discovery produced locals, but none contains the discovery anchor.
    ///
    /// This means the anchor directory is not under any discovered local
    /// config's root.
    #[error(
        "full discovery output did not contain a local config for anchor \
         {anchor}"
    )]
    FullDiscoveryWithoutAnchorLocal {
        /// Discovery anchor path that no local config contained.
        anchor: PathBuf,
    },
    /// Config file trust validation halted, requiring user action.
    #[error("config file is untrusted: {status:?}")]
    Untrusted {
        /// The halted config file.
        file: LocalConfigFile<Tracked>,
        /// The trust status that caused the halt.
        status: ConfigTrustStatus,
    },
    /// A config file failed path validation, tracking, or parsing.
    #[error(transparent)]
    ConfigFile(#[from] ConfigFileError),
}

/// Errors raised while validating config paths, trust state, or TOML content.
#[derive(Debug, Error)]
pub(crate) enum ConfigFileError {
    /// The path is not a local `.traces/config.toml` file.
    ///
    /// Local config paths must end with `.traces/config.toml` and have a parent
    /// directory named `.traces`.
    #[error("unsupported local config file {path}")]
    UnsupportedLocalConfigFile {
        /// Rejected local config path.
        path: PathBuf,
    },
    /// The path is not a supported global `config.toml` file.
    ///
    /// Global config paths must end with `config.toml`.
    #[error("unsupported global config file {path}")]
    UnsupportedGlobalConfigFile {
        /// Rejected global config path.
        path: PathBuf,
    },
    /// The config file could not be read or parsed as TOML.
    #[error("failed to load config file {path}")]
    Read {
        /// File that failed to load.
        path: PathBuf,
        /// TOML deserialization error.
        #[source]
        source: Box<toml::de::Error>,
    },
    /// Trust verification failed before the file could be parsed.
    #[error("failed to check trust for {root}")]
    TrustCheckFailed {
        /// Workspace root being checked.
        root: PathBuf,
        /// Underlying trust-store or hash error.
        source: Box<ConfigStateError>,
    },
    /// A configured subdirectory path was invalid (absolute, contained `..`, or
    /// escaped root).
    #[error("invalid config subdirectory path {path}")]
    InvalidSubDir {
        /// Subdirectory path that failed validation.
        path: PathBuf,
        /// Underlying path validation failure.
        #[source]
        source: crate::path::PathError,
    },
    /// A `[frontmatter]` or `[schemas]` config table named an invalid field
    /// key.
    #[error("invalid `{table}` config")]
    InvalidFieldKey {
        /// The TOML table that failed validation ("frontmatter" or "schemas").
        table: &'static str,
        /// The underlying validation failure.
        #[source]
        source: FieldNameError,
    },
    /// A `[tasks] tag_filters` entry failed [`crate::tag::Tag`] validation.
    #[error("invalid `tasks.tag_filters` entry {entry:?}")]
    InvalidTagFilter {
        /// The offending entry, before `#`-normalization.
        entry: String,
        /// The underlying tag validation failure.
        #[source]
        source: TagError,
    },
}

impl ConfigFileError {
    /// Builds an invalid field-key error for a named config table.
    pub(crate) fn invalid_field_key(
        table: &'static str,
        source: FieldNameError,
    ) -> Self {
        Self::InvalidFieldKey {
            table,
            source,
        }
    }
}

/// Errors from config tracking or trust-state operations.
#[derive(Debug, Error)]
pub enum ConfigStateError {
    /// The underlying hash-keyed store operation failed.
    #[error(transparent)]
    Store(#[from] FileStateStoreError),
    /// Hashing a config file failed.
    #[error(transparent)]
    Hash(#[from] HashError),
}

/// Errors raised while scaffolding a local config file on disk.
#[derive(Debug, Error)]
pub(crate) enum ConfigScaffoldError {
    /// The collected template and output directories could not be serialised to
    /// TOML.
    #[error("failed to serialise local config")]
    Serialize {
        /// Source TOML serialization error.
        #[source]
        source: toml::ser::Error,
    },
    /// The serialised local config could not be written to disk.
    ///
    /// This includes the case where the file already exists, since
    /// [`std::fs::File::create_new`] is used to prevent silent clobbering.
    #[error("failed to write local config file")]
    Write {
        /// Source filesystem error.
        #[source]
        source: io::Error,
    },
}
