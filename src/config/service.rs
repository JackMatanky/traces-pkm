//! Service-owned config loading: full discovery selects files, then the builder
//! records, trust-checks, parses, and merges them. Tracking and trust
//! administration live in [`super::store::ConfigStateStore`].

use std::path::{Path, PathBuf};

use thiserror::Error;

use super::{
    builder::{ConfigBuilder, ConfigBuilderError, ConfigBuilderInput},
    discovery::{
        DiscoveryAnchor, DiscoveryContext, DiscoveryEngine, DiscoveryError,
        DiscoveryOutcome, DiscoveryScope,
    },
    domain::Config,
    store::{
        ConfigStateError, ConfigStateStore, ConfigTrustStatus, TrustSubject,
        TrustSubjects, WorkspaceTrustStatus,
    },
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
/// to orchestrate those phases themselves. Holds the state store so the build
/// pipeline and trust-admin methods share the same tracked-config and trusted
/// workspace records.
#[derive(Clone, Debug)]
pub(crate) struct ConfigService {
    state: ConfigStateStore,
}

impl ConfigService {
    /// Creates a `ConfigService` backed by the OS-correct tracked-config and
    /// trust stores.
    #[must_use]
    #[inline]
    pub(crate) fn new() -> Self {
        Self {
            state: ConfigStateStore::new(),
        }
    }

    /// Creates a `ConfigService` backed by explicit tracked-config and
    /// trust-store roots. Test-only: production callers always want the
    /// OS-correct roots from [`Self::new`]. `pub(crate)` (not restricted
    /// to this module) so the CLI layer's tests (`crate::cli::trust`) can
    /// construct an isolated service without touching the real OS state
    /// directories, mirroring [`ConfigStateStore::at`].
    #[cfg(test)]
    #[must_use]
    pub(crate) fn at(tracked_root: PathBuf, trusted_root: PathBuf) -> Self {
        Self {
            state: ConfigStateStore::at(tracked_root, trusted_root),
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
            DiscoveryScope::Full,
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
    /// global candidates are never checked.
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
            .store_locals(&self.state)?
            .merge()?
            .build())
    }

    /// Resolves trust subjects from one user-supplied filesystem path.
    ///
    /// # Errors
    ///
    /// Returns [`DiscoveryError`] when discovery cannot inspect the path or the
    /// requested traversal scope cannot be resolved.
    #[inline]
    #[expect(
        clippy::unused_self,
        reason = "service owns the discovery seam even though trust-subject \
                  discovery has no state dependency today"
    )]
    pub(crate) fn trust_subjects(
        &self,
        path: &Path,
        scope: DiscoveryScope,
    ) -> Result<TrustSubjects, DiscoveryError> {
        DiscoveryEngine.trust_subjects(path, scope)
    }

    /// Grants trust for a workspace root, and for config subjects also records
    /// the config file's current content hash.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigStateError`] when trust cannot be recorded or a config
    /// file cannot be hashed.
    #[inline]
    pub(crate) fn trust(
        &self,
        subject: &TrustSubject,
    ) -> Result<(), ConfigStateError> {
        self.state.grant_trust(subject)
    }

    /// Returns the trust status for `subject`.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigStateError`] when the status check cannot complete.
    #[inline]
    pub(crate) fn trust_status(
        &self,
        subject: &TrustSubject,
    ) -> Result<&'static str, ConfigStateError> {
        if subject.config_file().is_some() {
            match self.state.config_trust_status(subject)? {
                ConfigTrustStatus::Trusted => Ok("trusted"),
                ConfigTrustStatus::Untrusted => Ok("untrusted"),
                ConfigTrustStatus::MissingBaseline
                | ConfigTrustStatus::Stale => Ok("stale"),
            }
        } else {
            match self.state.workspace_trust_status(subject)? {
                WorkspaceTrustStatus::Trusted => Ok("trusted"),
                WorkspaceTrustStatus::Untrusted => Ok("untrusted"),
            }
        }
    }

    /// Removes trust for `subject`'s workspace root, including any content-hash
    /// companion. Returns the number of root entries removed.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigStateError`] when the trust entry cannot be removed.
    #[inline]
    pub(crate) fn untrust(
        &self,
        subject: &TrustSubject,
    ) -> Result<usize, ConfigStateError> {
        self.state.revoke_trust(subject)
    }

    /// Lists the canonical paths of all live tracked configs.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigStateError`] when the tracking store exists but cannot
    /// be read.
    #[inline]
    pub(crate) fn list_tracked(
        &self,
    ) -> Result<Vec<PathBuf>, ConfigStateError> {
        self.state.list_tracked_configs()
    }

    /// Removes dangling tracked-config entries (target deleted or moved).
    /// Returns the number of entries removed.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigStateError`] when the tracking store exists but cannot
    /// be read, or a stale entry cannot be removed.
    #[inline]
    pub(crate) fn clean_tracked_store(
        &self,
    ) -> Result<usize, ConfigStateError> {
        self.state.clean_tracked_configs()
    }

    /// Lists the canonical paths of all currently trusted roots.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigStateError`] when the trust store exists but cannot be
    /// read.
    #[inline]
    pub(crate) fn list_trusted(
        &self,
    ) -> Result<Vec<PathBuf>, ConfigStateError> {
        self.state.list_trusted_workspaces()
    }

    /// Removes dangling trust entries (target root deleted or moved),
    /// including each removed entry's content-hash companion. Returns the
    /// number of root entries removed.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigStateError`] when the trust store exists but cannot be
    /// read, a stale root entry cannot be removed, or an existing content-hash
    /// companion cannot be removed.
    #[inline]
    pub(crate) fn clean_trusted_store(
        &self,
    ) -> Result<usize, ConfigStateError> {
        self.state.clean_trusted_workspaces()
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

    use super::*;
    use crate::config::file::{
        ConfigFileError, ConfigFileTrustError, Discovered, LocalConfigFile,
    };

    #[test]
    fn new_is_stateless() {
        assert_eq!(
            format!("{:?}", ConfigService::new()),
            format!("{:?}", ConfigService::new())
        );
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
        let service = ConfigService::at(
            temp.path().join("tracked-store"),
            temp.path().join("trust-store"),
        );

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
        let service = ConfigService::at(
            temp.path().join("tracked-store"),
            temp.path().join("trust-store"),
        );
        let config = discovered_config(&config_file);

        assert_eq!(
            service
                .trust_status(&TrustSubject::discovered(&config))
                .expect("check trust"),
            "untrusted"
        );

        service.trust(&TrustSubject::discovered(&config)).expect("trust root");

        assert_eq!(
            service
                .trust_status(&TrustSubject::discovered(&config))
                .expect("check trust"),
            "trusted"
        );
    }

    #[test]
    fn trust_file_target_derives_local_root_and_hashes_config() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let root = temp.path().join("project");
        let config_file = root.join(".traces/config.toml");
        fs::create_dir_all(config_file.parent().expect("config parent"))
            .expect("create config parent");
        fs::write(&config_file, "a = 1").expect("write config");
        let service = ConfigService::at(
            temp.path().join("tracked-store"),
            temp.path().join("trust-store"),
        );

        let config = discovered_config(&config_file);
        service.trust(&TrustSubject::discovered(&config)).expect("trust file");

        assert_eq!(
            service
                .trust_status(&TrustSubject::discovered(&config))
                .expect("check trust"),
            "trusted"
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

        let config = discovered_config(&config_file);
        service.trust(&TrustSubject::discovered(&config)).expect("trust root");

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
        let config = discovered_config(&config_file);
        service.trust(&TrustSubject::discovered(&config)).expect("trust root");
        fs::remove_dir_all(&root).expect("delete project dir");

        let removed =
            service.clean_trusted_store().expect("clean trusted store");

        assert_eq!(removed, 1);
        assert!(service.list_trusted().expect("list trusted").is_empty());
    }

    fn local_candidates(temp: &Path) -> (PathBuf, PathBuf, DiscoveryOutcome) {
        let cwd = temp.join("project");
        let config_path = cwd.join(".traces/config.toml");
        fs::create_dir_all(config_path.parent().expect("config path parent"))
            .expect("create config parent");
        fs::write(&config_path, "").expect("write config");
        let local = LocalConfigFile::<Discovered>::try_new(config_path.clone())
            .expect("valid local config");
        let candidates = DiscoveryOutcome::new(
            DiscoveryAnchor::Directory(cwd.clone()),
            vec![local],
            Vec::new(),
        );
        (cwd, config_path, candidates)
    }

    fn discovered_config(config_path: &Path) -> LocalConfigFile<Discovered> {
        LocalConfigFile::<Discovered>::try_new(config_path.to_path_buf())
            .expect("valid local config")
    }

    /// Builds a service rooted at temp stores, with `cwd` (the candidate's
    /// project root) pre-trusted so `build` clears the trust gate.
    fn trusted_service(
        temp: &Path,
        _cwd: &Path,
        config_path: &Path,
    ) -> ConfigService {
        let service = ConfigService::at(
            temp.join("tracked-store"),
            temp.join("trust-store"),
        );
        let config = discovered_config(config_path);
        service
            .trust(&TrustSubject::discovered(&config))
            .expect("trust candidate root");
        service
    }
}
