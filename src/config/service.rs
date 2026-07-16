//! Two-phase config loading: [`discover`](ConfigService::discover) finds
//! candidate files, [`build`](ConfigService::build) records, reads, and merges
//! them. Tracking is best-effort bookkeeping (see [`super::tracker`]).

use std::path::{Path, PathBuf};

use super::{
    builder::{ConfigBuilder, ConfigBuilderError, Discovered},
    discovery::{DiscoveryError, DiscoveryOutcome, DiscoveryProcessor},
    domain::Config,
    store::StoreError,
    tracker::ConfigTracker,
    trust::{ConfigTrust, TrustError, TrustState, TrustTarget},
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

    /// Creates a `ConfigService` backed by explicit tracked-config and
    /// trust-store roots. Test-only: production callers always want the
    /// OS-correct roots from [`Self::new`]. `pub(crate)` (not restricted
    /// to this module) so the CLI layer's tests (`crate::cli::trust`) can
    /// construct an isolated service without touching the real OS state
    /// directories, mirroring [`ConfigTracker::at`]/[`ConfigTrust::at`].
    #[cfg(test)]
    #[must_use]
    pub(crate) fn at(tracked_root: PathBuf, trusted_root: PathBuf) -> Self {
        Self {
            tracker: ConfigTracker::at(tracked_root),
            trust: ConfigTrust::at(trusted_root),
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
    /// Returns [`ConfigBuilderError::RootNotTrusted`] when a local
    /// candidate's project root is not trusted. Returns
    /// [`ConfigBuilderError::StaleConfigContent`] when the root is trusted
    /// but the config file's content has changed since. Returns
    /// [`ConfigBuilderError::TrustCheckFailed`] when the trust check itself
    /// fails. Returns [`ConfigBuilderError::ConfigParseFailed`] when a
    /// candidate config file cannot be read or parsed.
    #[inline]
    pub fn build(
        &self,
        discovered: &DiscoveryOutcome,
    ) -> Result<Config, ConfigBuilderError> {
        Ok(ConfigBuilder::<Discovered>::new(discovered)
            .track(&self.tracker)
            .trust(&self.trust)?
            .merge()?
            .build())
    }

    /// Marks `target`'s workspace root as trusted and, for
    /// [`TrustTarget::ConfigFile`], records its config file's current
    /// content hash as the baseline future checks compare against.
    ///
    /// Idempotent on the root entry: trusting an already-trusted root is a
    /// no-op there; re-running this after the config file changes
    /// refreshes the recorded content hash, clearing any staleness.
    ///
    /// # Errors
    ///
    /// Returns [`TrustError`] when the root cannot be canonicalized or
    /// recorded, [`TrustTarget::ConfigFile`]'s config file cannot be
    /// hashed, or the content-hash companion record cannot be written.
    #[inline]
    pub fn trust(&self, target: TrustTarget<'_>) -> Result<(), TrustError> {
        self.trust.trust(target)
    }

    /// Checks whether `root` is trusted and, if so, whether `config_file`'s
    /// current content still matches what [`Self::trust`] last recorded.
    ///
    /// # Errors
    ///
    /// Returns [`TrustError`] when the trust check itself fails (store
    /// I/O, or hashing `config_file`'s current content).
    #[inline]
    pub fn is_trusted(
        &self,
        root: &Path,
        config_file: &Path,
    ) -> Result<TrustState, TrustError> {
        self.trust.is_trusted(root, config_file)
    }

    /// Lists the canonical paths of all live tracked configs.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when the tracking store exists but cannot be
    /// read.
    #[inline]
    pub fn list_tracked(&self) -> Result<Vec<PathBuf>, StoreError> {
        self.tracker.list_all()
    }

    /// Removes dangling tracked-config entries (target deleted or moved).
    /// Returns the number of entries removed.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when the tracking store exists but cannot be
    /// read, or a stale entry cannot be removed.
    #[inline]
    pub fn clean_tracked_store(&self) -> Result<usize, StoreError> {
        self.tracker.clean()
    }

    /// Lists the canonical paths of all currently trusted roots.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when the trust store exists but cannot be
    /// read.
    #[inline]
    pub fn list_trusted(&self) -> Result<Vec<PathBuf>, StoreError> {
        self.trust.list_all()
    }

    /// Removes dangling trust entries (target root deleted or moved),
    /// including each removed entry's content-hash companion. Returns the
    /// number of root entries removed.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when the trust store exists but cannot be
    /// read, a stale root entry cannot be removed, or an existing
    /// content-hash companion cannot be removed.
    #[inline]
    pub fn clean_trusted_store(&self) -> Result<usize, StoreError> {
        self.trust.clean()
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
        service
            .trust(TrustTarget::ConfigFile {
                root: cwd,
                config_file: config_path,
            })
            .expect("trust candidate root");
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
            Err(ConfigBuilderError::RootNotTrusted { root }) if root == cwd
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
            Err(ConfigBuilderError::StaleConfigContent { root }) if root == cwd
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

        service
            .trust(TrustTarget::ConfigFile {
                root: &root,
                config_file: &config_file,
            })
            .expect("trust root");

        assert_eq!(
            service.is_trusted(&root, &config_file).expect("check trust"),
            TrustState::Trusted
        );
    }

    #[test]
    fn list_trusted_reflects_trusted_roots() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let root = temp.path().join("project");
        fs::create_dir_all(&root).expect("create project dir");
        let config_file = root.join("config.toml");
        fs::write(&config_file, "a = 1").expect("write config");
        let service = ConfigService::at(
            temp.path().join("tracked-store"),
            temp.path().join("trust-store"),
        );

        assert!(service.list_trusted().expect("list trusted").is_empty());

        service
            .trust(TrustTarget::ConfigFile {
                root: &root,
                config_file: &config_file,
            })
            .expect("trust root");

        assert_eq!(service.list_trusted().expect("list trusted"), vec![
            root.canonicalize().expect("canonicalize root")
        ]);
    }

    #[test]
    fn clean_trusted_store_prunes_a_root_whose_directory_was_deleted() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let root = temp.path().join("project");
        fs::create_dir_all(&root).expect("create project dir");
        let config_file = root.join("config.toml");
        fs::write(&config_file, "a = 1").expect("write config");
        let service = ConfigService::at(
            temp.path().join("tracked-store"),
            temp.path().join("trust-store"),
        );
        service
            .trust(TrustTarget::ConfigFile {
                root: &root,
                config_file: &config_file,
            })
            .expect("trust root");
        fs::remove_dir_all(&root).expect("delete project dir");

        let removed =
            service.clean_trusted_store().expect("clean trusted store");

        assert_eq!(removed, 1);
        assert!(service.list_trusted().expect("list trusted").is_empty());
    }
}
