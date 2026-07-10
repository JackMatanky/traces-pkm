//! Two-phase config loading: [`discover`](ConfigService::discover) collects
//! candidate files, [`build`](ConfigService::build) reads and merges them.

use std::path::Path;

use super::{
    builder::{ConfigBuilder, Discovered},
    discovery::DiscoveryProcessor,
    discovery::{DiscoveryError, DiscoveryOutcome},
    domain::{Config, ConfigError},
};

/// Entry point for discovering and building configuration.
///
/// Stateless coordinator that separates file discovery from parsing, merging,
/// and resolution.
/// Loading is a two-step pipeline: call [`ConfigService::discover`] with the
/// working directory to collect candidate config files, then pass those
/// candidates to [`ConfigService::build`] to read, parse, and merge them. This
/// keeps filesystem discovery separate from the trust/build stages.
#[derive(Clone, Debug, Default)]
pub struct ConfigService;

impl ConfigService {
    /// Creates a `ConfigService`.
    #[must_use]
    #[inline]
    pub fn new() -> Self {
        Self
    }

    /// Discovers config files relative to `cwd`.
    ///
    /// Returns real config files plus the invocation cwd.
    ///
    /// # Errors
    ///
    /// Returns an error when a path cannot be accessed during discovery.
    #[inline]
    pub fn discover(
        &self,
        cwd: &Path,
    ) -> Result<DiscoveryOutcome, DiscoveryError> {
        DiscoveryProcessor::new(cwd)
            .collect_local()?
            .collect_global()
            .map(DiscoveryProcessor::finish)
    }

    /// Builds a [`Config`] from discovered candidates.
    ///
    /// # Errors
    ///
    /// Returns an error when a candidate config file cannot be read or parsed.
    #[inline]
    pub fn build(
        &self,
        discovered: &DiscoveryOutcome,
    ) -> Result<Config, ConfigError> {
        Ok(ConfigBuilder::<Discovered>::new(discovered)
            .track()
            .trust()
            .merge()?
            .build())
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::indexing_slicing,
        clippy::panic_in_result_fn,
        clippy::unwrap_used,
        reason = "test code uses direct assertions and temp-file setup"
    )]

    use std::fs;

    use super::*;

    #[test]
    fn new_is_stateless() {
        assert_eq!(
            format!("{:?}", ConfigService::new()),
            format!("{:?}", ConfigService::new())
        );
    }

    #[test]
    fn build_default_candidate_returns_default_config()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let cwd = temp.path().join("project");
        fs::create_dir_all(&cwd)?;
        let candidates =
            DiscoveryOutcome::new(cwd.clone(), Vec::new(), Vec::new());

        let config = ConfigService::new().build(&candidates)?;

        assert_eq!(config.root(), cwd.as_path());
        assert_eq!(config.output_dir(), cwd.as_path());
        assert!(config.sources().is_empty());
        Ok(())
    }
}
