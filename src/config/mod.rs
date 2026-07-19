//! Configuration discovery, tracking, loading, and template resolution.
//!
//! Discovers config files by walking up the directory tree from a working
//! directory. Build records discovered candidates as best-effort tracking,
//! then merges the user's global config before local `.traces/config.toml` so
//! local values win. Provides the crate-internal [`ConfigService`] entry
//! point plus read-only config domain types. [`DiscoveryOutcome`] is the opaque
//! token parsed into the selected config-builder input.
//!
//! `pub(crate)`, not `pub`: this module is private in `lib.rs` (only `cli`,
//! a sibling module in the same crate, consumes it — nothing outside this
//! crate does, and unlike `dialog`, this module has no doctests that would
//! need real crate-external reachability). Re-exports here match that:
//! `pub(crate)`, not `pub`, since a `pub` re-export from a private module
//! is unreachable from outside the crate anyway — matching the visibility
//! to what's actually reachable, rather than leaving it wider than it can
//! ever be used, is the point.
//!
//! Error types are `thiserror`-only, no `miette::Diagnostic` — this module
//! stays agnostic to how its errors are displayed. `crate::cli` is where
//! that presentation belongs (see `cli::error::ConfigTrustCliError` for the
//! pattern): a future CLI command wraps [`ConfigBuilderError`],
//! [`DiscoveryError`], and [`ResolutionError`] the same way once `init`/
//! `render` land. Infrastructure errors
//! ([`crate::hash::HashError`], and `StoreError`/`TrustError` from this
//! module's private `store`/`trust` submodules) are only observable
//! through the `#[source]` chain of the three re-exported types above and
//! [`ConfigService`]'s admin methods — they cannot be named directly from
//! outside `config`. [`ConfigFile`] is re-exported only so the CLI can consume
//! resolved local config targets without a duplicate trust-target type.

#![cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "config loading is implemented before the render command \
                  consumes it"
    )
)]

pub(crate) use discovery::LOCAL_CONFIG_FILE;
pub(crate) use file::{ConfigFile, Discovered as DiscoveredConfigFile};
pub(crate) use raw::RawConfig;
pub(crate) use service::ConfigService;
#[cfg(test)]
pub(crate) use trust::TrustState;
pub(crate) use trust::TrustTarget;

mod builder;
mod dirs;
mod discovery;
mod domain;
mod file;
mod raw;
mod service;
mod store;
mod tracker;
mod trust;
