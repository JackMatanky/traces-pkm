//! Discovers, tracks, trusts, parses, and merges TOML config files into a
//! resolved [`Config`].
//!
//! # Types
//!
//! - [`ConfigService`] orchestrates the full load pipeline and trust
//!   administration.
//! - [`Config`] holds merged local/global settings for consumers.
//! - [`TrustRequest`] targets a workspace root or config file for trust
//!   operations.
//!
//! # Pipeline
//!
//! 1. Discover local and global TOML files from a filesystem anchor.
//! 2. Track discovered local configs as best-effort state.
//! 3. Reject untrusted or stale local config content before parsing.
//! 4. Merge global config before local config so local values win.

mod discovery;
mod error;
mod file;
mod model;
mod raw;
mod service;
mod store;
mod trust;

pub(crate) use discovery::{
    DiscoveryScope, LOCAL_CONFIG_DIR, LOCAL_CONFIG_FILE,
};
#[cfg(test)]
pub(crate) use error::ConfigFileError;
pub(crate) use error::{
    ConfigBuilderError, ConfigLoadError, ConfigScaffoldError, ConfigStateError,
    DiscoveryError,
};
#[cfg(any(test, feature = "test-utils"))]
pub(crate) use file::{Discovered, LocalConfigFile};
/// Merged local/global settings for note schemas.
pub use model::{Config, SchemasConfig};
#[cfg(any(test, feature = "test-utils"))]
pub use model::{DateFieldConfig, FrontmatterConfig};
/// Orchestrates config loading and trust administration.
pub use service::ConfigService;
#[cfg(test)]
pub(crate) use trust::ConfigTrustStatus;
/// Target of a trust or untrust operation.
pub use trust::TrustRequest;
pub(crate) use trust::TrustRequests;
