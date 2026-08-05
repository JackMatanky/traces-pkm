//! Configuration loading and trust boundary.
//!
//! The config seam used by the rest of the crate: exports [`ConfigService`]
//! for operations and [`Config`] for read-only resolved settings, while
//! submodules keep discovery, lifecycle, raw TOML, and trust-state details
//! separate.
//!
//! # Load Pipeline
//!
//! - Discover local and global TOML files from a filesystem anchor.
//! - Track discovered local configs as best-effort state.
//! - Reject untrusted or stale local config content before parsing.
//! - Merge global config before local config so local values win.

#![cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "config loading is implemented before the render command \
                  consumes it"
    )
)]

mod discovery;
mod file;
mod model;
mod raw;
mod service;
mod store;
mod trust;

pub(crate) use discovery::{DiscoveryError, DiscoveryScope, LOCAL_CONFIG_FILE};
#[cfg(test)]
pub(crate) use file::{Discovered, LocalConfigFile};
pub(crate) use model::Config;
pub(crate) use raw::{RawConfig, RawTemplateConfig};
#[cfg(test)]
pub(crate) use service::{ConfigBuilderError, ConfigBuilderInputError};
pub(crate) use service::{ConfigLoadError, ConfigService};
pub(crate) use store::ConfigStateError;
#[cfg(test)]
pub(crate) use trust::ConfigTrustStatus;
pub(crate) use trust::TrustRequest;
