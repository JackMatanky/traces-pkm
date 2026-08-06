//! Errors from config discovery, file validation, loading, and trust-state
//! operations.
//!
//! Every variant here crosses a `config` submodule boundary — either raised
//! in one file and consumed in another, or composed by [`ConfigLoadError`]
//! into the result [`crate::cli::CliError`] reports. File-local
//! parsing/construction failures that never propagate past the file that
//! raises them stay defined there instead.

use std::{io, path::PathBuf};

use thiserror::Error;

use super::{
    discovery::DiscoveryScope,
    file::{LocalConfigFile, Tracked},
    trust::ConfigTrustStatus,
};
use crate::{FileStateStoreError, hash::HashError};

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

/// Errors raised while finding candidate config files.
#[derive(Debug, Error)]
pub(crate) enum DiscoveryError {
    /// No local `.traces/config.toml` was found in any ancestor directory.
    #[error("no local config found from {cwd}")]
    LocalConfigAbsent {
        /// The working directory from which discovery started.
        cwd: PathBuf,
    },
    /// Discovery could not access a path.
    #[error("failed to access path {path} during discovery")]
    PathInaccessible {
        /// Path that could not be accessed.
        path: PathBuf,
        /// Source I/O error.
        #[source]
        source: io::Error,
    },
    /// Rejects a file anchor for [`DiscoveryScope::Full`], which always
    /// requires a directory anchor.
    #[error("{kind:?} discovery cannot be anchored at file {path}")]
    UnsupportedFileAnchor {
        /// Discovery kind.
        kind: DiscoveryScope,
        /// Unsupported file anchor path.
        path: PathBuf,
    },
    /// Rejects [`Full`] for trust-request resolution, which only supports
    /// [`NearestLocal`] and [`LocalSubtree`].
    ///
    /// [`Full`]: DiscoveryScope::Full
    /// [`NearestLocal`]: DiscoveryScope::NearestLocal
    /// [`LocalSubtree`]: DiscoveryScope::LocalSubtree
    #[error("{scope:?} discovery cannot be used for trust request resolution")]
    UnsupportedTrustScope {
        /// Unsupported discovery scope.
        scope: DiscoveryScope,
    },
    /// A discovered config file path/source combination was invalid.
    #[error(transparent)]
    ConfigFile(#[from] ConfigFileError),
}

/// Errors raised while building a [`super::Config`] from discovered files.
#[derive(Debug, Error)]
pub(crate) enum ConfigBuilderError {
    /// Only full discovery output can feed config loading.
    #[error(
        "config builder input requires full discovery output, got {actual:?}"
    )]
    WrongDiscoveryKindForBuild {
        /// Actual discovery kind.
        actual: DiscoveryScope,
    },
    /// Full discovery found no local config candidates.
    #[error("full discovery output did not contain a local config")]
    FullDiscoveryWithoutLocal,
    /// Full discovery found locals, but none contains the discovery anchor.
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
    /// The merged local/global config could not be re-extracted to resolve
    /// the effective output directory.
    #[error("failed to merge local and global config")]
    Merge {
        /// Source figment error.
        #[source]
        source: Box<figment::Error>,
    },
    /// Config file lifecycle validation failed.
    #[error(transparent)]
    ConfigFile(#[from] ConfigFileError),
}

/// Errors raised while validating config paths, trust state, or TOML content.
#[derive(Debug, Error)]
pub(crate) enum ConfigFileError {
    /// The path is not a local `.traces/config.toml` file.
    #[error("unsupported local config file {path}")]
    UnsupportedLocalConfigFile {
        /// Rejected local config path.
        path: PathBuf,
    },
    /// The path is not a supported global `config.toml` file.
    #[error("unsupported global config file {path}")]
    UnsupportedGlobalConfigFile {
        /// Rejected global config path.
        path: PathBuf,
    },
    /// The config file could not be read or parsed.
    #[error("failed to load config file {path}")]
    Read {
        /// File that failed to load.
        path: PathBuf,
        /// TOML provider error from `figment`.
        #[source]
        source: Box<figment::Error>,
    },
    /// Trust verification failed before the file could be parsed.
    #[error("failed to check trust for {root}")]
    TrustCheckFailed {
        /// Workspace root being checked.
        root: PathBuf,
        /// Underlying trust-store or hash error.
        source: Box<ConfigStateError>,
    },
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

/// Errors raised while scaffolding a local config file's on-disk content.
#[derive(Debug, Error)]
pub(crate) enum ConfigScaffoldError {
    /// The collected template and output directories could not be
    /// serialised to TOML.
    #[error("failed to serialise local config")]
    Serialize {
        /// Source TOML serialization error.
        #[source]
        source: toml::ser::Error,
    },
    /// The serialised local config could not be written to disk.
    #[error("failed to write local config file")]
    Write {
        /// Source filesystem error.
        #[source]
        source: io::Error,
    },
}
