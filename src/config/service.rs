//! Two-phase config loading: [`discover`](ConfigService::discover) finds
//! candidate files, [`build`](ConfigService::build) records, reads, and merges
//! them. Tracking is best-effort bookkeeping (see [`super::tracker`]).

use std::path::{Path, PathBuf};

use super::{
    builder::{ConfigBuilder, Discovered},
    discovery::{DiscoveryError, DiscoveryOutcome, DiscoveryProcessor},
    domain::{Config, ConfigError, TrustState},
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
    /// failure does not fail the build. Each local candidate's project root
    /// is then checked against the trust store before any file is parsed;
    /// global candidates are never checked (global config trust is skipped
    /// entirely — see `super::trust::ConfigTrust`'s module docs for why).
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError::Untrusted`] when a local candidate's project
    /// root is not trusted. Returns [`ConfigError::Stale`] when the root is
    /// trusted but the config file's content has changed since. Returns
    /// [`ConfigError::TrustIo`] when the trust check itself fails. Returns
    /// an error when a candidate config file cannot be read or parsed.
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

    /// Marks `root`'s canonical path as trusted, and records
    /// `config_file`'s current content hash as the baseline future checks
    /// compare against.
    ///
    /// Idempotent on the root entry: trusting an already-trusted root is a
    /// no-op there; re-running this after `config_file` changes refreshes
    /// the recorded content hash, clearing any staleness.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError::TrustIo`] when `root` cannot be canonicalized
    /// or recorded, `config_file` cannot be hashed, or the content-hash
    /// companion record cannot be written.
    #[inline]
    pub fn trust(
        &self,
        root: &Path,
        config_file: &Path,
    ) -> Result<(), ConfigError> {
        self.trust.trust(root, config_file).map_err(|source| {
            ConfigError::TrustIo {
                path: root.to_path_buf(),
                source,
            }
        })
    }

    /// Checks whether `root` is trusted and, if so, whether `config_file`'s
    /// current content still matches what [`Self::trust`] last recorded.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError::TrustIo`] when the trust check itself fails
    /// (store I/O, or hashing `config_file`'s current content).
    #[inline]
    pub fn is_trusted(
        &self,
        root: &Path,
        config_file: &Path,
    ) -> Result<TrustState, ConfigError> {
        self.trust.is_trusted(root, config_file).map_err(|source| {
            ConfigError::TrustIo {
                path: root.to_path_buf(),
                source,
            }
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

    /// Builds a service rooted at temp stores, with `cwd` (the candidate's
    /// project root) pre-trusted so `build` clears the trust gate.
    fn trusted_service(
        temp: &Path,
        cwd: &Path,
        config_path: &Path,
    ) -> ConfigService {
        let service = ConfigService {
            tracker: ConfigTracker::at(temp.join("tracked-store")),
            trust: ConfigTrust::at(temp.join("trust-store")),
        };
        service.trust(cwd, config_path).expect("trust candidate root");
        service
    }

    #[test]
    fn build_records_the_candidate_and_list_tracked_reflects_it_idempotently() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let (cwd, config_path, candidates) = local_candidates(temp.path());
        let service = trusted_service(temp.path(), &cwd, &config_path);

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
        let (cwd, config_path, candidates) = local_candidates(temp.path());
        let service = trusted_service(temp.path(), &cwd, &config_path);
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
        let service = trusted_service(temp.path(), &cwd, &config_path);

        let config = service
            .build(&candidates)
            .expect("build must succeed despite a tracking write failure");

        assert_eq!(config.root(), cwd.as_path());
    }

    #[test]
    fn build_rejects_a_candidate_with_an_untrusted_root() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let (cwd, _config_path, candidates) = local_candidates(temp.path());
        let service = ConfigService {
            tracker: ConfigTracker::at(temp.path().join("tracked-store")),
            trust: ConfigTrust::at(temp.path().join("trust-store")),
        };

        let result = service.build(&candidates);

        assert!(matches!(
            result,
            Err(ConfigError::Untrusted { path }) if path == cwd
        ));
    }

    #[test]
    fn build_rejects_a_candidate_whose_root_is_trusted_but_stale() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let (cwd, config_path, candidates) = local_candidates(temp.path());
        let service = trusted_service(temp.path(), &cwd, &config_path);
        fs::write(&config_path, "directory = \"changed\"")
            .expect("edit config after trusting");

        let result = service.build(&candidates);

        assert!(matches!(
            result,
            Err(ConfigError::Stale { path }) if path == cwd
        ));
    }

    #[test]
    fn trust_then_is_trusted_returns_trusted() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let root = temp.path().join("project");
        fs::create_dir_all(&root).expect("create project dir");
        let config_file = root.join("config.toml");
        fs::write(&config_file, "a = 1").expect("write config");
        let service = ConfigService {
            tracker: ConfigTracker::at(temp.path().join("tracked-store")),
            trust: ConfigTrust::at(temp.path().join("trust-store")),
        };

        assert_eq!(
            service.is_trusted(&root, &config_file).expect("check trust"),
            TrustState::Untrusted
        );

        service.trust(&root, &config_file).expect("trust root");

        assert_eq!(
            service.is_trusted(&root, &config_file).expect("check trust"),
            TrustState::Trusted
        );
    }
}
