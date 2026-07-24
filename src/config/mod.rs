//! Configuration discovery, tracking, loading, and trust.
//!
//! Discovers config files by walking up the directory tree from a working
//! directory. Discovered candidates are recorded as best-effort tracking,
//! then the user's global config is merged before local
//! `.traces/config.toml` so local values win. Provides the crate-internal
//! [`ConfigService`] entry point plus read-only config domain types.
//! [`DiscoveryOutcome`](discovery::DiscoveryOutcome) is the opaque token
//! parsed into the selected config-builder input.
//!
//! Template resolution against the directories this module parses is
//! `crate::template`'s concern, not this module's: this module only holds
//! parsed/merged config data.
//!
//! `pub(crate)`: only `cli` and `template`, sibling modules in this crate,
//! consume it.
//!
//! Error types are `thiserror`-only, no `miette::Diagnostic`; CLI-facing
//! presentation is added by `crate::cli` (see
//! `cli::error::ConfigTrustCliError`).

#![cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "config loading is implemented before the render command \
                  consumes it"
    )
)]

pub(crate) use discovery::{DiscoveryScope, LOCAL_CONFIG_FILE};
pub(crate) use domain::Config;
#[cfg(test)]
pub(crate) use file::{Discovered, LocalConfigFile};
pub(crate) use raw::{RawConfig, RawTemplateConfig};
pub(crate) use service::{ConfigLoadError, ConfigService};
pub(crate) use trust::TrustRequest;

mod builder;
mod discovery;
mod domain;
mod file;
mod raw;
mod service;
mod store;
mod trust;
