//! Configuration discovery, tracking, loading, and trust.
//!
//! Discovers config files by walking up the directory tree from a working
//! directory, tracks candidates as best-effort bookkeeping, then merges the
//! user's global config before local `.traces/config.toml` so local values
//! win. Entry point: [`ConfigService`]; also exposes read-only config
//! domain types. Template resolution against these directories is
//! `crate::template`'s concern.
//!
//! `pub(crate)`: only `cli` and `template` consume it.

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
