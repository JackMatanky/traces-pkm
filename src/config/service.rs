//! Service-owned config loading: full discovery selects files, then the builder
//! records, trust-checks, parses, and merges them. Tracking is best-effort
//! bookkeeping (see [`super::tracker`]).

use std::path::{Path, PathBuf};

use thiserror::Error;

use super::{
    builder::{ConfigBuilder, ConfigBuilderError, ConfigBuilderInput},
    discovery::{
        DiscoveryAnchor, DiscoveryContext, DiscoveryEngine, DiscoveryError,
        DiscoveryOutcome, DiscoveryType,
    },
    domain::Config,
    file::{ConfigFile, Discovered as FileDiscovered},
    store::StoreError,
    tracker::ConfigTracker,
    trust::{ConfigTrust, TrustError, TrustState, TrustTarget},
};

/// Errors from the full config loading pipeline.
#[derive(Debug, Error)]
pub(crate) enum ConfigLoadError {
    /// Discovery failed before any config could be loaded.
    #[error(transparent)]
    Discovery(#[from] DiscoveryError),
    /// Build failed after discovery selected candidate config files.
    #[error(transparent)]
    Build(#[from] ConfigBuilderError),
}

/// Entry point for discovering and building configuration.
///
/// Coordinator that hides discovery-before-build sequencing behind the normal
/// [`ConfigService::load`] entry point. Filesystem discovery stays separate
/// from tracking, trust, parse, and merge internals, but callers no longer need
/// to orchestrate those phases themselves. Holds the tracked-config store so
/// the build pipeline can record local configs and admin methods can list/clean
/// the same store. Also holds the trust store so the build pipeline can gate
/// untrusted local configs and admin methods can manage trust directly.
#[derive(Clone, Debug)]
pub(crate) struct ConfigService {
    tracker: ConfigTracker,
    trust: ConfigTrust,
}

impl ConfigService {
    /// Creates a `ConfigService` backed by the OS-correct tracked-config and
    /// trust stores.
    #[must_use]
    #[inline]
    pub(crate) fn new() -> Self {
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

    /// Discovers and builds config from `cwd` in one service-owned pipeline.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigLoadError::Discovery`] when no local config is found or
    /// discovery cannot inspect a path. Returns [`ConfigLoadError::Build`] when
    /// trust, parsing, or merging fails after discovery succeeds.
    #[inline]
    pub(crate) fn load(&self, cwd: &Path) -> Result<Config, ConfigLoadError> {
        let discovered = Self::discover(cwd)?;
        self.build(discovered).map_err(Into::into)
    }

    /// Discovers config files from `cwd`.
    ///
    /// Returns discovered config files plus the invocation cwd. The local
    /// project config is required; the global config is optional. An
    /// associated function, not a method: discovery is a pure filesystem
    /// walk from `cwd` with no dependency on `ConfigService`'s own state
    /// (the tracked-config/trust stores) — [`Self::build`] is where that
    /// state actually gets used, on the resulting [`DiscoveryOutcome`].
    ///
    /// # Errors
    ///
    /// Returns an error when local config is absent or when a path cannot be
    /// accessed during discovery.
    #[inline]
    fn discover(cwd: &Path) -> Result<DiscoveryOutcome, DiscoveryError> {
        let context = DiscoveryContext::new(
            DiscoveryType::Full,
            DiscoveryAnchor::Directory(cwd.to_path_buf()),
        )?;
        DiscoveryEngine.process(context)
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
    /// Returns [`ConfigBuilderError::ConfigFile`] when a candidate config file
    /// fails path validation, tracking/trust transition, or parsing.
    #[inline]
    fn build(
        &self,
        discovered: DiscoveryOutcome,
    ) -> Result<Config, ConfigBuilderError> {
        let input = ConfigBuilderInput::try_from(discovered)?;
        Ok(ConfigBuilder::new(input)
            .store_locals(&self.tracker, &self.trust)?
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
    pub(crate) fn trust(
        &self,
        target: TrustTarget<'_>,
    ) -> Result<(), TrustError> {
        self.trust.trust(target)
    }

    /// Marks a discovered local config file's project root as trusted and
    /// records its current content hash.
    ///
    /// # Errors
    ///
    /// Returns [`TrustError`] when the root cannot be canonicalized or
    /// recorded, or the config file cannot be hashed.
    #[inline]
    pub(crate) fn trust_config_file(
        &self,
        file: &ConfigFile<FileDiscovered>,
    ) -> Result<(), TrustError> {
        self.trust.trust(TrustTarget::File(file.path()))
    }

    /// Checks whether `target` is trusted and, for config-file targets, whether
    /// the current content still matches what [`Self::trust`] last recorded.
    ///
    /// # Errors
    ///
    /// Returns [`TrustError`] when the trust check itself fails (store
    /// I/O, target validation, or hashing config-file content).
    #[inline]
    pub(crate) fn is_trusted(
        &self,
        target: TrustTarget<'_>,
    ) -> Result<TrustState, TrustError> {
        self.trust.is_trusted(target)
    }

    /// Resolves one or many user trust targets using config discovery
    /// semantics.
    ///
    /// # Errors
    ///
    /// Returns [`DiscoveryError`] when no local config can be found or a path
    /// cannot be inspected.
    #[inline]
    pub(crate) fn resolve_trust_targets(
        cwd: &Path,
        path: Option<&Path>,
        all: bool,
    ) -> Result<Vec<ConfigFile<FileDiscovered>>, DiscoveryError> {
        let start = resolve_start(cwd, path);
        let kind = if all {
            DiscoveryType::LocalSubtree
        } else {
            DiscoveryType::NearestLocal
        };
        let anchor = if start.is_file() {
            DiscoveryAnchor::File(start)
        } else {
            DiscoveryAnchor::Directory(start)
        };
        let context = DiscoveryContext::new(kind, anchor)?;
        let outcome = DiscoveryEngine.process(context)?;
        Ok(outcome.local().to_vec())
    }

    /// Removes `root` from the trust store, including its content-hash
    /// companion. Returns the number of root entries removed.
    ///
    /// # Errors
    ///
    /// Returns [`TrustError`] when `root` cannot be canonicalized or removal
    /// fails.
    #[inline]
    pub(crate) fn untrust(&self, root: &Path) -> Result<usize, TrustError> {
        self.trust.untrust(root)
    }

    /// Lists the canonical paths of all live tracked configs.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when the tracking store exists but cannot be
    /// read.
    #[inline]
    pub(crate) fn list_tracked(&self) -> Result<Vec<PathBuf>, StoreError> {
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
    pub(crate) fn clean_tracked_store(&self) -> Result<usize, StoreError> {
        self.tracker.clean()
    }

    /// Lists the canonical paths of all currently trusted roots.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when the trust store exists but cannot be
    /// read.
    #[inline]
    pub(crate) fn list_trusted(&self) -> Result<Vec<PathBuf>, StoreError> {
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
    pub(crate) fn clean_trusted_store(&self) -> Result<usize, StoreError> {
        self.trust.clean()
    }
}

fn resolve_start(cwd: &Path, path: Option<&Path>) -> PathBuf {
    match path {
        Some(path) if path.is_absolute() => path.to_path_buf(),
        Some(path) => cwd.join(path),
        None => cwd.to_path_buf(),
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
    use crate::config::file::{ConfigFileError, ConfigFileTrustError};

    #[test]
    fn new_is_stateless() {
        assert_eq!(
            format!("{:?}", ConfigService::new()),
            format!("{:?}", ConfigService::new())
        );
    }

    fn local_candidates(temp: &Path) -> (PathBuf, PathBuf, DiscoveryOutcome) {
        let cwd = temp.join("project");
        let config_path = cwd.join(".traces/config.toml");
        fs::create_dir_all(config_path.parent().expect("config path parent"))
            .expect("create config parent");
        fs::write(&config_path, "").expect("write config");
        let local = crate::config::file::ConfigFile::<
            crate::config::file::Discovered,
        >::local(config_path.clone())
        .expect("valid local config");
        let candidates = DiscoveryOutcome::new(
            DiscoveryAnchor::Directory(cwd.clone()),
            vec![local],
            Vec::new(),
        );
        (cwd, config_path, candidates)
    }

    /// Builds a service rooted at temp stores, with `cwd` (the candidate's
    /// project root) pre-trusted so `build` clears the trust gate.
    fn trusted_service(
        temp: &Path,
        _cwd: &Path,
        config_path: &Path,
    ) -> ConfigService {
        let service = ConfigService {
            tracker: ConfigTracker::at(temp.join("tracked-store")),
            trust: ConfigTrust::at(temp.join("trust-store")),
        };
        service
            .trust(TrustTarget::File(config_path))
            .expect("trust candidate root");
        service
    }

    #[test]
    fn load_discovers_trusted_local_config_and_records_it() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let root = temp.path().join("project");
        let cwd = root.join("notes/daily");
        let config_path = root.join(".traces/config.toml");
        fs::create_dir_all(config_path.parent().expect("config path parent"))
            .expect("create config parent");
        fs::create_dir_all(&cwd).expect("create cwd");
        fs::write(
            &config_path,
            "[templates]\ndirectory = \".traces/templates\"\noutput_dir = \
             \"notes\"",
        )
        .expect("write config");
        let service = trusted_service(temp.path(), &root, &config_path);

        let config = service.load(&cwd).expect("load config");

        assert_eq!(config.root(), root.as_path());
        assert_eq!(config.output_dir(), Path::new("notes"));
        assert_eq!(
            service.list_tracked().expect("list tracked configs"),
            vec![config_path.canonicalize().expect("canonicalize config")]
        );
    }
    #[test]
    fn build_records_the_candidate_and_list_tracked_reflects_it_idempotently() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let (cwd, config_path, candidates) = local_candidates(temp.path());
        let service = trusted_service(temp.path(), &cwd, &config_path);

        service.build(candidates.clone()).expect("build config");

        assert_eq!(
            service.list_tracked().expect("list tracked configs"),
            vec![config_path.canonicalize().expect("canonicalize config")]
        );

        // Idempotent through the full pipeline, not just at the store layer.
        service.build(candidates).expect("build config again");
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
        service.build(candidates).expect("build config");
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
            .build(candidates)
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

        let result = service.build(candidates);

        assert!(matches!(
            result,
            Err(ConfigBuilderError::ConfigFile(ConfigFileError::Trust(
                ConfigFileTrustError::RootNotTrusted { root }
            ))) if root == cwd
        ));
    }

    #[test]
    fn build_rejects_a_candidate_whose_root_is_trusted_but_stale() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let (cwd, config_path, candidates) = local_candidates(temp.path());
        let service = trusted_service(temp.path(), &cwd, &config_path);
        fs::write(&config_path, "directory = \"changed\"")
            .expect("edit config after trusting");

        let result = service.build(candidates);

        assert!(matches!(
            result,
            Err(ConfigBuilderError::ConfigFile(ConfigFileError::Trust(
                ConfigFileTrustError::StaleConfigContent { root }
            ))) if root == cwd
        ));
    }

    #[test]
    fn trust_then_is_trusted_returns_trusted() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let root = temp.path().join("project");
        fs::create_dir_all(&root).expect("create project dir");
        let config_file = root.join(".traces/config.toml");
        fs::create_dir_all(config_file.parent().expect("config parent"))
            .expect("create config parent");
        fs::write(&config_file, "a = 1").expect("write config");
        let service = ConfigService {
            tracker: ConfigTracker::at(temp.path().join("tracked-store")),
            trust: ConfigTrust::at(temp.path().join("trust-store")),
        };

        assert_eq!(
            service
                .is_trusted(TrustTarget::File(&config_file))
                .expect("check trust"),
            TrustState::Untrusted
        );

        service.trust(TrustTarget::File(&config_file)).expect("trust root");

        assert_eq!(
            service
                .is_trusted(TrustTarget::File(&config_file))
                .expect("check trust"),
            TrustState::Trusted
        );
    }

    #[test]
    fn trust_file_target_derives_local_root_and_hashes_config() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let root = temp.path().join("project");
        let config_file = root.join(".traces/config.toml");
        fs::create_dir_all(config_file.parent().expect("config parent"))
            .expect("create config parent");
        fs::create_dir_all(config_file.parent().expect("config parent"))
            .expect("create config parent");
        fs::write(&config_file, "a = 1").expect("write config");
        let service = ConfigService::at(
            temp.path().join("tracked-store"),
            temp.path().join("trust-store"),
        );

        service.trust(TrustTarget::File(&config_file)).expect("trust file");

        assert_eq!(
            service
                .is_trusted(TrustTarget::File(&config_file))
                .expect("check trust"),
            TrustState::Trusted
        );
    }
    #[test]
    fn list_trusted_reflects_trusted_roots() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let root = temp.path().join("project");
        fs::create_dir_all(&root).expect("create project dir");
        let config_file = root.join(".traces/config.toml");
        fs::create_dir_all(config_file.parent().expect("config parent"))
            .expect("create config parent");
        fs::write(&config_file, "a = 1").expect("write config");
        let service = ConfigService::at(
            temp.path().join("tracked-store"),
            temp.path().join("trust-store"),
        );

        assert!(service.list_trusted().expect("list trusted").is_empty());

        service.trust(TrustTarget::File(&config_file)).expect("trust root");

        assert_eq!(service.list_trusted().expect("list trusted"), vec![
            root.canonicalize().expect("canonicalize root")
        ]);
    }

    #[test]
    fn clean_trusted_store_prunes_a_root_whose_directory_was_deleted() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let root = temp.path().join("project");
        fs::create_dir_all(&root).expect("create project dir");
        let config_file = root.join(".traces/config.toml");
        fs::create_dir_all(config_file.parent().expect("config parent"))
            .expect("create config parent");
        fs::write(&config_file, "a = 1").expect("write config");
        let service = ConfigService::at(
            temp.path().join("tracked-store"),
            temp.path().join("trust-store"),
        );
        service.trust(TrustTarget::File(&config_file)).expect("trust root");
        fs::remove_dir_all(&root).expect("delete project dir");

        let removed =
            service.clean_trusted_store().expect("clean trusted store");

        assert_eq!(removed, 1);
        assert!(service.list_trusted().expect("list trusted").is_empty());
    }
}
