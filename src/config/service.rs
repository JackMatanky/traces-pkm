//! Two-phase config loading: [`discover`](ConfigService::discover) finds
//! candidate files, [`build`](ConfigService::build) records, reads, and merges
//! them. Tracking is best-effort bookkeeping (see [`super::tracker`]).

use std::path::Path;

use super::{
    builder::{ConfigBuilder, Discovered},
    discovery::{DiscoveryError, DiscoveryOutcome, DiscoveryProcessor},
    domain::{Config, ConfigError},
};

/// Entry point for discovering and building configuration.
///
/// Stateless coordinator that separates file discovery from tracking, parsing,
/// merging, and resolution.
/// Loading is a two-step pipeline: call [`ConfigService::discover`] with the
/// working directory to collect candidate config files, then pass those
/// candidates to [`ConfigService::build`] to read, parse, and merge them. This
/// keeps filesystem discovery separate from the tracking/build stages.
#[derive(Clone, Debug, Default)]
pub struct ConfigService;

impl ConfigService {
    /// Creates a `ConfigService`.
    #[must_use]
    #[inline]
    pub fn new() -> Self {
        Self
    }

    /// Discovers config files from `cwd`.
    ///
    /// Returns discovered config files plus the invocation cwd. The local
    /// project config is required; the global config is optional.
    ///
    /// # Errors
    ///
    /// Returns an error when local config is absent or when a path cannot be
    /// accessed during discovery.
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
    /// Candidate paths are recorded in the config tracking store before they
    /// are read. This is a best-effort side effect; a tracking store write
    /// failure does not fail the build.
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
    use std::fs;

    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn new_is_stateless() {
        assert_eq!(
            format!("{:?}", ConfigService::new()),
            format!("{:?}", ConfigService::new())
        );
    }

    #[test]
    fn build_default_candidate_returns_default_config() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let cwd = temp.path().join("project");
        fs::create_dir_all(&cwd).expect("create cwd");
        let candidates =
            DiscoveryOutcome::new(cwd.clone(), Vec::new(), Vec::new());

        let config = ConfigService::new()
            .build(&candidates)
            .expect("build default config");

        assert_eq!(config.root(), cwd.as_path());
        assert_eq!(config.output_dir(), cwd.as_path());
    }
}
