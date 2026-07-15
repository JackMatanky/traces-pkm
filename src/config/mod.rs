//! Configuration discovery, tracking, loading, and template resolution.
//!
//! Discovers config files by walking up the directory tree from a working
//! directory. Build records discovered candidates as best-effort tracking,
//! then merges the user's global config before local `.traces/config.toml` so
//! local values win. Provides the public [`ConfigService`] entry point plus
//! read-only config domain types. [`DiscoveryOutcome`] is the opaque token
//! connecting `ConfigService::discover` to `ConfigService::build`.
//!
//! Error types are `thiserror`-only, no `miette::Diagnostic` — this crate
//! stays agnostic to how its errors are displayed. A future CLI layer (not
//! part of this module) wraps [`ConfigBuilderError`], [`DiscoveryError`],
//! and [`ResolutionError`] to add help text and error codes. Infrastructure
//! errors ([`crate::hash::HashError`], and `StoreError`/`TrustError` from
//! this module's private `store`/`trust` submodules) are only observable
//! through the `#[source]` chain of the three re-exported types above and
//! [`ConfigService`]'s admin methods — they cannot be named directly from
//! outside `config`.

pub use builder::ConfigBuilderError;
pub use discovery::{DiscoveryError, DiscoveryOutcome};
pub use domain::{Config, ResolutionError, ResolvedTemplate};
pub use service::ConfigService;
pub use trust::TrustState;

mod builder;
mod candidate;
mod dirs;
mod discovery;
mod domain;
mod raw;
mod service;
mod store;
mod tracker;
mod trust;
