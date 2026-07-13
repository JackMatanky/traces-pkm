//! Two-phase config loading: [`discover`](ConfigService::discover) finds
//! candidate files, [`build`](ConfigService::build) records, reads, and merges
//! them. Tracking is best-effort bookkeeping (see [`super::tracker`]).

use std::path::{Path, PathBuf};

use super::{
    builder::{ConfigBuilder, Discovered},
    discovery::{DiscoveryError, DiscoveryOutcome, DiscoveryProcessor},
    domain::{Config, ConfigError},
    tracker::ConfigTracker,
    trust::ConfigTrust,
};

/// Entry point for discovering and building configuration.
///
/// Coordinator that separates file discovery from tracking, parsing, merging,
/// and resolution. Loading is a two-step pipeline: call
/// [`ConfigService::discover`] with the working directory to collect
/// candidate config files, then pass those candidates to
/// [`ConfigService::build`] to read, parse, and merge them. This keeps
/// filesystem discovery separate from the tracking/build stages. Holds the
/// tracked-config store so [`ConfigService::build`] can record candidates and
/// [`ConfigService::list_tracked`]/[`ConfigService::clean_tracked_store`] can
/// administer that same store. Also holds the trust store so
/// [`ConfigService::build`] can gate untrusted config directories and
/// [`ConfigService::trust`]/[`ConfigService::is_trusted`] can administer it
/// directly.
#[derive(Clone, Debug)]
pub struct ConfigService {
    tracker: ConfigTracker,
    trust: ConfigTrust,
}

impl ConfigService {
    /// Creates a `ConfigService` backed by the OS-correct tracked-config and
    /// trust stores.
    #[must_use]
    #[inline]
    pub fn new() -> Self {
        Self {
            tracker: ConfigTracker::new(),
            trust: ConfigTrust::new(),
        }
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
    /// failure does not fail the build. Each candidate's parent directory is
    /// then checked against the trust store before any file is parsed.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError::Untrusted`] when a candidate's parent
    /// directory is not trusted. Returns [`ConfigError::TrustIo`] when the
    /// trust check itself fails. Returns an error when a candidate config
    /// file cannot be read or parsed.
    #[inline]
    pub fn build(
        &self,
        discovered: &DiscoveryOutcome,
    ) -> Result<Config, ConfigError> {
        Ok(ConfigBuilder::<Discovered>::new(discovered)
            .track(&self.tracker)
            .trust(&self.trust)?
            .merge()?
            .build())
    }

    /// Marks `dir`'s canonical path as trusted.
    ///
    /// Idempotent: trusting an already-trusted directory is a no-op.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError::TrustIo`] when `dir` cannot be canonicalized
    /// or the trust entry cannot be written.
    #[inline]
    pub fn trust(&self, dir: &Path) -> Result<(), ConfigError> {
        self.trust.trust(dir).map_err(|source| ConfigError::TrustIo {
            path: dir.to_path_buf(),
            source,
        })
    }

    /// Returns whether `dir`'s canonical path has a trust entry.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError::TrustIo`] when `dir` cannot be canonicalized
    /// or the trust entry's existence cannot be determined.
    #[inline]
    pub fn is_trusted(&self, dir: &Path) -> Result<bool, ConfigError> {
        self.trust.is_trusted(dir).map_err(|source| ConfigError::TrustIo {
            path: dir.to_path_buf(),
            source,
        })
    }

    /// Lists the canonical paths of all live tracked configs.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError::Tracking`] when the tracking store exists but
    /// cannot be read.
    #[inline]
    pub fn list_tracked(&self) -> Result<Vec<PathBuf>, ConfigError> {
        Ok(self.tracker.list_all()?)
    }

    /// Removes dangling tracked-config entries (target deleted or moved).
    /// Returns the number of entries removed.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError::Tracking`] when the tracking store exists but
    /// cannot be read, or a stale entry cannot be removed.
    #[inline]
    pub fn clean_tracked_store(&self) -> Result<usize, ConfigError> {
        Ok(self.tracker.clean()?)
    }
}

impl Default for ConfigService {
    #[inline]
    fn default() -> Self {
        Self::new()
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

    fn local_candidates(temp: &Path) -> (PathBuf, PathBuf, DiscoveryOutcome) {
        let cwd = temp.join("project");
        let config_path = cwd.join(".traces/config.toml");
        fs::create_dir_all(config_path.parent().expect("config path parent"))
            .expect("create config parent");
        fs::write(&config_path, "").expect("write config");
        let candidates = DiscoveryOutcome::new(
            cwd.clone(),
            vec![crate::config::candidate::CandidateConfigFile::new(
                cwd.clone(),
                crate::config::candidate::ConfigSource::Local(
                    config_path.clone(),
                ),
            )],
            Vec::new(),
        );
        (cwd, config_path, candidates)
    }

    /// Builds a service rooted at temp stores, with `config_path`'s parent
    /// directory pre-trusted so `build` clears the trust gate.
    fn trusted_service(temp: &Path, config_path: &Path) -> ConfigService {
        let service = ConfigService {
            tracker: ConfigTracker::at(temp.join("tracked-store")),
            trust: ConfigTrust::at(temp.join("trust-store")),
        };
        service
            .trust(config_path.parent().expect("config path parent"))
            .expect("trust config directory");
        service
    }

    #[test]
    fn build_records_the_candidate_and_list_tracked_reflects_it_idempotently() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let (_cwd, config_path, candidates) = local_candidates(temp.path());
        let service = trusted_service(temp.path(), &config_path);

        service.build(&candidates).expect("build config");

        assert_eq!(
            service.list_tracked().expect("list tracked configs"),
            vec![config_path.canonicalize().expect("canonicalize config")]
        );

        // Idempotent through the full pipeline, not just at the store layer.
        service.build(&candidates).expect("build config again");
        assert_eq!(
            service.list_tracked().expect("list tracked configs").len(),
            1
        );
    }

    #[test]
    fn clean_tracked_store_prunes_entries_whose_config_was_deleted() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let (_cwd, config_path, candidates) = local_candidates(temp.path());
        let service = trusted_service(temp.path(), &config_path);
        service.build(&candidates).expect("build config");
        fs::remove_file(&config_path).expect("remove config");

        let removed =
            service.clean_tracked_store().expect("clean tracked store");

        assert_eq!(removed, 1);
        assert!(
            service.list_tracked().expect("list tracked configs").is_empty()
        );
    }

    #[test]
    fn build_succeeds_even_when_the_tracking_store_write_fails() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let (cwd, config_path, candidates) = local_candidates(temp.path());
        fs::write(temp.path().join("tracked-store"), "")
            .expect("occupy tracked store with a file");
        let service = trusted_service(temp.path(), &config_path);

        let config = service
            .build(&candidates)
            .expect("build must succeed despite a tracking write failure");

        assert_eq!(config.root(), cwd.as_path());
    }

    #[test]
    fn build_rejects_a_candidate_in_an_untrusted_directory() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let (_cwd, config_path, candidates) = local_candidates(temp.path());
        let service = ConfigService {
            tracker: ConfigTracker::at(temp.path().join("tracked-store")),
            trust: ConfigTrust::at(temp.path().join("trust-store")),
        };
        let expected_dir =
            config_path.parent().expect("config path parent").to_path_buf();

        let result = service.build(&candidates);

        assert!(matches!(
            result,
            Err(ConfigError::Untrusted { path }) if path == expected_dir
        ));
    }

    #[test]
    fn trust_then_is_trusted_returns_true() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let dir = temp.path().join("project");
        fs::create_dir_all(&dir).expect("create project dir");
        let service = ConfigService {
            tracker: ConfigTracker::at(temp.path().join("tracked-store")),
            trust: ConfigTrust::at(temp.path().join("trust-store")),
        };

        assert!(!service.is_trusted(&dir).expect("check trust"));

        service.trust(&dir).expect("trust directory");

        assert!(service.is_trusted(&dir).expect("check trust"));
    }
}
