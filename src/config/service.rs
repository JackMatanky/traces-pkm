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
    use std::{
        fs,
        path::{Path, PathBuf},
    };

    use super::*;
    use crate::config::{
        discovery::DiscoveryAnchor,
        file::{
            ConfigFileError, ConfigFileTrustError, Discovered, LocalConfigFile,
        },
    };

    struct Fixture {
        temp: tempfile::TempDir,
        tracked_root: PathBuf,
        trusted_root: PathBuf,
        service: ConfigService,
    }

    impl Fixture {
        fn new() -> Self {
            let temp = tempfile::tempdir().expect("create temp dir");
            let tracked_root = temp.path().join("tracked-store");
            let trusted_root = temp.path().join("trust-store");
            let service =
                ConfigService::at(tracked_root.clone(), trusted_root.clone());
            Self {
                temp,
                tracked_root,
                trusted_root,
                service,
            }
        }

        fn target_dir(&self, name: &str) -> PathBuf {
            let path = self.temp.path().join(name);
            fs::create_dir_all(&path).expect("create target dir");
            path
        }

        fn create_config(&self, root: &Path, contents: &str) -> PathBuf {
            let config_path = root.join(".traces/config.toml");
            fs::create_dir_all(config_path.parent().unwrap())
                .expect("create config parent");
            fs::write(&config_path, contents).expect("write config");
            config_path
        }

        fn discovered_config(
            &self,
            config_path: &Path,
        ) -> LocalConfigFile<Discovered> {
            LocalConfigFile::<Discovered>::try_new(config_path.to_path_buf())
                .expect("valid local config")
        }

        fn trust_config(&self, config_path: &Path) {
            let config = self.discovered_config(config_path);
            self.service
                .trust(&TrustSubject::discovered(&config))
                .expect("trust candidate root");
        }
    }

    mod constructor {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn new_creates_os_backed_stores() {
            // Arrange & Act
            let service1 = ConfigService::new();
            let service2 = ConfigService::new();

            // Assert
            // Just verifying it doesn't panic and constructs identically
            assert_eq!(format!("{:?}", service1), format!("{:?}", service2));
        }

        #[test]
        fn at_creates_custom_rooted_stores() {
            // Arrange
            let temp = tempfile::tempdir().unwrap();
            let tracked = temp.path().join("tracked");
            let trusted = temp.path().join("trusted");

            // Act
            let service = ConfigService::at(tracked.clone(), trusted.clone());

            // Assert
            assert!(
                format!("{:?}", service).contains(tracked.to_str().unwrap())
            );
        }
    }

    mod load {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn returns_discovery_error_when_no_config_found() {
            // Arrange
            let fixture = Fixture::new();
            let cwd = fixture.target_dir("project/notes/daily");

            // Act
            let result = fixture.service.load(&cwd);

            // Assert
            assert!(matches!(
                result,
                Err(ConfigLoadError::Discovery(
                    DiscoveryError::LocalConfigAbsent { .. }
                ))
            ));
        }

        #[test]
        fn discovers_and_builds_trusted_local_config() {
            // Arrange
            let fixture = Fixture::new();
            let root = fixture.target_dir("project");
            let cwd = root.join("notes/daily");
            fs::create_dir_all(&cwd).unwrap();

            let config_path = fixture.create_config(
                &root,
                "[templates]\ndirectory = \".traces/templates\"\noutput_dir = \
                 \"notes\"",
            );
            fixture.trust_config(&config_path);

            // Act
            let result = fixture.service.load(&cwd);

            // Assert
            assert!(result.is_ok());
            let config = result.unwrap();
            assert_eq!(config.root(), root.as_path());
            assert_eq!(config.output_dir(), Path::new("notes"));
        }
    }

    mod build {
        use pretty_assertions::assert_eq;

        use super::*;

        fn local_candidates(
            fixture: &Fixture,
        ) -> (PathBuf, PathBuf, DiscoveryOutcome) {
            let cwd = fixture.target_dir("project");
            let config_path = fixture.create_config(&cwd, "");
            let local = fixture.discovered_config(&config_path);
            let candidates = DiscoveryOutcome::new(
                DiscoveryAnchor::Directory(cwd.clone()),
                vec![local],
                Vec::new(),
            );
            (cwd, config_path, candidates)
        }

        #[test]
        fn records_candidate_in_tracking_store() {
            // Arrange
            let fixture = Fixture::new();
            let (_cwd, config_path, candidates) = local_candidates(&fixture);
            fixture.trust_config(&config_path);

            // Act
            let result = fixture.service.build(candidates);

            // Assert
            assert!(result.is_ok());
            let tracked = fixture.service.list_tracked().unwrap();
            assert_eq!(tracked, vec![config_path.canonicalize().unwrap()]);
        }

        #[test]
        fn tracking_record_is_idempotent() {
            // Arrange
            let fixture = Fixture::new();
            let (_cwd, config_path, candidates) = local_candidates(&fixture);
            fixture.trust_config(&config_path);
            fixture.service.build(candidates.clone()).unwrap();

            // Act
            let result = fixture.service.build(candidates);

            // Assert
            assert!(result.is_ok());
            let tracked = fixture.service.list_tracked().unwrap();
            assert_eq!(tracked.len(), 1);
        }

        #[test]
        fn succeeds_even_when_tracking_store_write_fails() {
            // Arrange
            let fixture = Fixture::new();
            let (cwd, config_path, candidates) = local_candidates(&fixture);
            fixture.trust_config(&config_path);

            // Occupy the tracked-store path with a file so directory creation
            // fails
            fs::write(&fixture.tracked_root, "").unwrap();

            // Act
            let result = fixture.service.build(candidates);

            // Assert
            assert!(result.is_ok());
            assert_eq!(result.unwrap().root(), cwd.as_path());
        }

        #[test]
        fn rejects_untrusted_root() {
            // Arrange
            let fixture = Fixture::new();
            let (cwd, _config_path, candidates) = local_candidates(&fixture);
            // Do NOT trust the root

            // Act
            let result = fixture.service.build(candidates);

            // Assert
            assert!(matches!(
                result,
                Err(ConfigBuilderError::ConfigFile(ConfigFileError::Trust(
                    ConfigFileTrustError::RootNotTrusted { root }
                ))) if root == cwd
            ));
        }

        #[test]
        fn rejects_trusted_but_stale_root() {
            // Arrange
            let fixture = Fixture::new();
            let (cwd, config_path, candidates) = local_candidates(&fixture);
            fixture.trust_config(&config_path);

            // Edit config after trusting to make it stale
            fs::write(&config_path, "directory = \"changed\"").unwrap();

            // Act
            let result = fixture.service.build(candidates);

            // Assert
            assert!(matches!(
                result,
                Err(ConfigBuilderError::ConfigFile(ConfigFileError::Trust(
                    ConfigFileTrustError::StaleConfigContent { root }
                ))) if root == cwd
            ));
        }
    }

    mod trust_subjects {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn delegates_to_discovery_engine() {
            // Arrange
            let fixture = Fixture::new();
            let cwd = fixture.target_dir("project");
            fixture.create_config(&cwd, "");

            // Act
            let result = fixture
                .service
                .trust_subjects(&cwd, DiscoveryScope::NearestLocal);

            // Assert
            assert!(result.is_ok());
            let subjects = result.unwrap();
            assert_eq!(subjects.into_iter().count(), 1);
        }
    }

    mod trust {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn records_workspace_trust() {
            // Arrange
            let fixture = Fixture::new();
            let root = fixture.target_dir("project");
            let subject = TrustSubject::root(&root);

            // Act
            let result = fixture.service.trust(&subject);

            // Assert
            assert!(result.is_ok());
            assert_eq!(
                fixture.service.trust_status(&subject).unwrap(),
                "trusted"
            );
        }

        #[test]
        fn records_config_trust_and_hashes_content() {
            // Arrange
            let fixture = Fixture::new();
            let root = fixture.target_dir("project");
            let config_path = fixture.create_config(&root, "a = 1");
            let config = fixture.discovered_config(&config_path);
            let subject = TrustSubject::discovered(&config);

            // Act
            let result = fixture.service.trust(&subject);

            // Assert
            assert!(result.is_ok());
            assert_eq!(
                fixture.service.trust_status(&subject).unwrap(),
                "trusted"
            );
        }
    }

    mod trust_status {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn returns_untrusted_for_unknown_workspace() {
            // Arrange
            let fixture = Fixture::new();
            let root = fixture.target_dir("project");
            let subject = TrustSubject::root(&root);

            // Act
            let result = fixture.service.trust_status(&subject);

            // Assert
            assert!(result.is_ok());
            assert_eq!(result.unwrap(), "untrusted");
        }

        #[test]
        fn returns_untrusted_for_unknown_config() {
            // Arrange
            let fixture = Fixture::new();
            let root = fixture.target_dir("project");
            let config_path = fixture.create_config(&root, "a = 1");
            let config = fixture.discovered_config(&config_path);
            let subject = TrustSubject::discovered(&config);

            // Act
            let result = fixture.service.trust_status(&subject);

            // Assert
            assert!(result.is_ok());
            assert_eq!(result.unwrap(), "untrusted");
        }

        #[test]
        fn returns_stale_when_config_content_changes() {
            // Arrange
            let fixture = Fixture::new();
            let root = fixture.target_dir("project");
            let config_path = fixture.create_config(&root, "a = 1");
            let config = fixture.discovered_config(&config_path);
            let subject = TrustSubject::discovered(&config);

            fixture.service.trust(&subject).unwrap();
            fs::write(&config_path, "a = 2").unwrap();

            // Act
            let result = fixture.service.trust_status(&subject);

            // Assert
            assert!(result.is_ok());
            assert_eq!(result.unwrap(), "stale");
        }
    }

    mod untrust {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn removes_trust_from_subject() {
            // Arrange
            let fixture = Fixture::new();
            let root = fixture.target_dir("project");
            let subject = TrustSubject::root(&root);
            fixture.service.trust(&subject).unwrap();

            // Act
            let result = fixture.service.untrust(&subject);

            // Assert
            assert!(result.is_ok());
            assert_eq!(result.unwrap(), 1); // 1 entry removed
            assert_eq!(
                fixture.service.trust_status(&subject).unwrap(),
                "untrusted"
            );
        }

        #[test]
        fn returns_zero_when_already_untrusted() {
            // Arrange
            let fixture = Fixture::new();
            let root = fixture.target_dir("project");
            let subject = TrustSubject::root(&root);

            // Act
            let result = fixture.service.untrust(&subject);

            // Assert
            assert!(result.is_ok());
            assert_eq!(result.unwrap(), 0);
        }
    }

    mod list_tracked {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn returns_empty_when_no_tracked_configs() {
            // Arrange
            let fixture = Fixture::new();

            // Act
            let result = fixture.service.list_tracked();

            // Assert
            assert!(result.is_ok());
            assert!(result.unwrap().is_empty());
        }

        #[test]
        fn returns_recorded_configs() {
            // Arrange
            let fixture = Fixture::new();
            let cwd = fixture.target_dir("project");
            let config_path = fixture.create_config(&cwd, "");
            let local = fixture.discovered_config(&config_path);
            let candidates = DiscoveryOutcome::new(
                DiscoveryAnchor::Directory(cwd.clone()),
                vec![local],
                Vec::new(),
            );
            fixture.trust_config(&config_path);
            fixture.service.build(candidates).unwrap();

            // Act
            let result = fixture.service.list_tracked();

            // Assert
            assert!(result.is_ok());
            assert_eq!(result.unwrap(), vec![
                config_path.canonicalize().unwrap()
            ]);
        }
    }

    mod clean_tracked_store {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn prunes_entries_whose_config_was_deleted() {
            // Arrange
            let fixture = Fixture::new();
            let cwd = fixture.target_dir("project");
            let config_path = fixture.create_config(&cwd, "");
            let local = fixture.discovered_config(&config_path);
            let candidates = DiscoveryOutcome::new(
                DiscoveryAnchor::Directory(cwd.clone()),
                vec![local],
                Vec::new(),
            );
            fixture.trust_config(&config_path);
            fixture.service.build(candidates).unwrap();

            fs::remove_file(&config_path).unwrap();

            // Act
            let result = fixture.service.clean_tracked_store();

            // Assert
            assert!(result.is_ok());
            assert_eq!(result.unwrap(), 1);
            assert!(fixture.service.list_tracked().unwrap().is_empty());
        }

        #[test]
        fn leaves_live_entries_untouched() {
            // Arrange
            let fixture = Fixture::new();
            let cwd = fixture.target_dir("project");
            let config_path = fixture.create_config(&cwd, "");
            let local = fixture.discovered_config(&config_path);
            let candidates = DiscoveryOutcome::new(
                DiscoveryAnchor::Directory(cwd.clone()),
                vec![local],
                Vec::new(),
            );
            fixture.trust_config(&config_path);
            fixture.service.build(candidates).unwrap();

            // Act
            let result = fixture.service.clean_tracked_store();

            // Assert
            assert!(result.is_ok());
            assert_eq!(result.unwrap(), 0);
            assert_eq!(fixture.service.list_tracked().unwrap().len(), 1);
        }
    }

    mod list_trusted {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn returns_empty_when_no_trusted_roots() {
            // Arrange
            let fixture = Fixture::new();

            // Act
            let result = fixture.service.list_trusted();

            // Assert
            assert!(result.is_ok());
            assert!(result.unwrap().is_empty());
        }

        #[test]
        fn returns_trusted_roots() {
            // Arrange
            let fixture = Fixture::new();
            let root = fixture.target_dir("project");
            let config_path = fixture.create_config(&root, "a = 1");
            fixture.trust_config(&config_path);

            // Act
            let result = fixture.service.list_trusted();

            // Assert
            assert!(result.is_ok());
            assert_eq!(result.unwrap(), vec![root.canonicalize().unwrap()]);
        }
    }

    mod clean_trusted_store {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn prunes_root_whose_directory_was_deleted() {
            // Arrange
            let fixture = Fixture::new();
            let root = fixture.target_dir("project");
            let config_path = fixture.create_config(&root, "a = 1");
            fixture.trust_config(&config_path);
            fs::remove_dir_all(&root).unwrap();

            // Act
            let result = fixture.service.clean_trusted_store();

            // Assert
            assert!(result.is_ok());
            assert_eq!(result.unwrap(), 1);
            assert!(fixture.service.list_trusted().unwrap().is_empty());
        }

        #[test]
        fn leaves_live_roots_untouched() {
            // Arrange
            let fixture = Fixture::new();
            let root = fixture.target_dir("project");
            let config_path = fixture.create_config(&root, "a = 1");
            fixture.trust_config(&config_path);

            // Act
            let result = fixture.service.clean_trusted_store();

            // Assert
            assert!(result.is_ok());
            assert_eq!(result.unwrap(), 0);
            assert_eq!(fixture.service.list_trusted().unwrap().len(), 1);
        }
    }
}
