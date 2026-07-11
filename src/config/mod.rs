//! Configuration discovery, tracking, loading, and template resolution.
//!
//! Discovers config files by walking up the directory tree from a working
//! directory. Build records discovered candidates as best-effort tracking,
//! then merges the user's global config before local `.traces/config.toml` so
//! local values win. Provides the public [`ConfigService`] entry point plus
//! read-only config domain types. [`DiscoveryOutcome`] is the opaque token
//! connecting `ConfigService::discover` to `ConfigService::build`.

pub use builder::ConfigBuilderError;
pub use discovery::{DiscoveryError, DiscoveryOutcome};
pub use domain::{Config, ConfigError, ResolutionError, ResolvedTemplate};
pub use service::ConfigService;

mod builder;
mod candidate;
mod discovery;
mod domain;
mod paths;
mod raw;
mod service;
mod store;
mod tracker;
