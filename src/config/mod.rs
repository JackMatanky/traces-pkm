//! Configuration discovery, tracking, loading, and trust.
//!
//! [`ConfigService`] discovers config files from a working directory, tracks
//! candidates as best-effort bookkeeping, checks local trust state, and merges
//! global config before local `.traces/config.toml` so local values win.
//!
//! Read-only domain types expose the resolved project root, template
//! directories, and output directory to `cli` and `template`.

#![cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "config loading is implemented before the render command \
                  consumes it"
    )
)]

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

mod discovery;
mod file;
mod model;
mod raw;
mod service;
mod store;
mod trust;
