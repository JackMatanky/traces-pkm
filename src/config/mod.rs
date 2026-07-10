//! Configuration discovery, loading, and resolution.
//!
//! Discovers config files by walking up the directory tree from a working
//! directory, merging local `.traces/config.toml` with the user's global
//! config. Provides the public [`ConfigService`] entry point. The public facade
//! exposes [`DiscoveryOutcome`] only as the opaque token connecting
//! `ConfigService::discover` to `ConfigService::build`.

pub use builder::ConfigBuilderError;
pub use candidate::{CandidateConfigFile, ConfigSource};
pub use discovery::{DiscoveryError, DiscoveryOutcome};
pub use domain::{
    Config, ConfigError, ResolutionError, ResolvedTemplate, TemplateConfig,
};
pub use service::ConfigService;

pub(crate) mod builder;
mod candidate;
mod discovery;
mod domain;
mod paths;
mod raw;
mod service;
pub(crate) mod tracker;
