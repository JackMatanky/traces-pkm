//! Configuration loading and trust boundary.
//!
//! The config seam used by the rest of the crate: exports [`ConfigService`] for
//! operations and [`Config`] for read-only resolved settings, while submodules
//! keep discovery, lifecycle, raw TOML, and trust-state details separate.
//!
//! # Load Pipeline
//!
//! - Discover local and global TOML files from a filesystem anchor.
//! - Track discovered local configs as best-effort state.
//! - Reject untrusted or stale local config content before parsing.
//! - Merge global config before local config so local values win.

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
pub use model::Config;
pub use service::ConfigService;
#[cfg(test)]
pub(crate) use trust::ConfigTrustStatus;
pub use trust::TrustRequest;
pub(crate) use trust::TrustRequests;
