//! Loads TOML configuration from the filesystem into a resolved [`Config`].
//!
//! # Pipeline
//!
//! 1. **Discover** local `.traces/config.toml` and optional global
//!    `traces/config.toml` from a directory anchor ([`discovery`]).
//! 2. **Track** discovered local configs in a best-effort store
//!    ([`store::ConfigStateStore`]).
//! 3. **Verify trust** before parsing local content; reject untrusted or stale
//!    files ([`trust`]).
//! 4. **Parse** TOML into [`raw::RawConfig`] ([`mod@file`]).
//! 5. **Merge** global config before local so local values win
//!    ([`service::ConfigService`]).
//!
//! Path resolution and default filling happen in [`model`], which produces
//! the final [`Config`].
//!
//! # Main Types
//!
//! - [`ConfigService`] owns the load pipeline and trust administration.
//! - [`Config`] holds merged settings ready for consumers.
//! - [`TrustRequest`] names a workspace root or config file for trust
//!   operations.
//!
//! Filesystem paths are resolved by the [`crate::dirs`] module.

mod discovery;
mod error;
mod file;
mod model;
mod raw;
mod service;
mod specs;
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
#[cfg(any(test, feature = "test-utils"))]
pub use model::{DateFieldConfig, FrontmatterConfig, SchemasConfig};
pub use service::ConfigService;
pub(crate) use specs::SchemaConfigSpec;
#[cfg(test)]
pub(crate) use trust::ConfigTrustStatus;
pub use trust::TrustRequest;
pub(crate) use trust::TrustRequests;
