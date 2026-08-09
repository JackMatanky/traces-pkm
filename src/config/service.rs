//! Orchestrates config loading and trust administration.
//!
//! [`ConfigService`] is the single entry point for discovering, building,
//! and trusting config files.
//!
//! # Loading Pipeline
//!
//! 1. Discover local and global TOML candidates from `cwd`.
//! 2. Record local candidates in the tracking store.
//! 3. Verify trust before parsing local content.
//! 4. Parse TOML into [`RawConfig`].
//! 5. Merge global before local so local values win.
//!
//! Trust administration resolves subjects and delegates durable state to
//! [`super::store::ConfigStateStore`].

use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

use figment::{Figment, providers::Serialized};

#[cfg(test)]
use super::error::ConfigFileError;
use super::{
    LOCAL_CONFIG_FILE,
    discovery::{
        DiscoveryAnchor, DiscoveryContext, DiscoveryEngine, DiscoveryOutcome,
        DiscoveryScope,
    },
    error::{
        ConfigBuilderError, ConfigLoadError, ConfigScaffoldError,
        ConfigStateError, DiscoveryError,
    },
    file::{
        Discovered as FileDiscovered, GlobalConfigFile, LocalConfigFile,
        Parsed, Tracked, TrustOutcome,
    },
    model::{Config, FrontmatterConfig, SchemasConfig, TemplateConfig},
    raw::{RawConfig, RawTemplateConfig},
    store::ConfigStateStore,
    trust::{ConfigTrustStatus, TrustRequest, TrustRequests},
};

/// Selects the deepest local config containing the discovery anchor, plus the
/// optional global config merged before it.
#[derive(Debug)]
struct ConfigBuilderInput {
    /// Selected local config; this is merged after `global`.
    local: LocalConfigFile<FileDiscovered>,
    /// Optional global config; this is merged before `local`.
    global: Option<GlobalConfigFile<FileDiscovered>>,
}

impl TryFrom<DiscoveryOutcome> for ConfigBuilderInput {
    type Error = ConfigBuilderError;

    #[inline]
    fn try_from(outcome: DiscoveryOutcome) -> Result<Self, Self::Error> {
        let (kind, anchor, discovered_locals, discovered_globals) =
            outcome.into_parts();
        if kind != DiscoveryScope::Full {
            return Err(ConfigBuilderError::WrongDiscoveryKindForBuild {
                actual: kind,
            });
        }

        let discovered_locals = discovered_locals.into_vec();
        if discovered_locals.is_empty() {
            return Err(ConfigBuilderError::FullDiscoveryWithoutLocal);
        }

        let anchor_path = anchor.path().to_path_buf();
        let local = discovered_locals
            .into_iter()
            .filter(|file| anchor_path.starts_with(file.root()))
            .max_by_key(|file| file.root().components().count())
            .ok_or(ConfigBuilderError::FullDiscoveryWithoutAnchorLocal {
                anchor: anchor_path,
            })?;
        let global = discovered_globals.into_vec().into_iter().next();
        Ok(Self {
            local,
            global,
        })
    }
}

/// Entry point for config loading and trust administration.
///
/// Filesystem discovery (`load`) and `TrustRequest` operations (`trust`,
/// `untrust`) are separate surfaces on this type.
#[derive(Clone, Debug)]
pub struct ConfigService {
    state: ConfigStateStore,
}

impl ConfigService {
    /// Creates a service backed by the OS-correct tracked-config and trust
    /// stores.
    #[must_use]
    #[inline]
    pub(crate) fn new() -> Self {
        Self {
            state: ConfigStateStore::new(),
        }
    }

    /// Creates a service backed by explicit tracked-config and trust-store
    /// roots.
    ///
    /// Test-only constructor for `crate::cli::trust` tests that need
    /// isolated stores instead of real OS state directories.
    #[cfg(any(test, feature = "test-utils"))]
    #[inline]
    #[must_use]
    pub fn at(tracked_root: PathBuf, trusted_root: PathBuf) -> Self {
        Self {
            state: ConfigStateStore::at(tracked_root, trusted_root),
        }
    }

    /// Discovers and builds config from `cwd` in one service-owned pipeline.
    ///
    /// # Errors
    ///
    /// - [`ConfigLoadError::Discovery`] when no local config is found or
    ///   discovery cannot inspect a path.
    /// - [`ConfigLoadError::Build`] when trust, parsing, or merging fails after
    ///   discovery succeeds.
    #[inline]
    pub(crate) fn load(&self, cwd: &Path) -> Result<Config, ConfigLoadError> {
        let discovered = Self::discover(cwd)?;
        self.build(discovered).map_err(Into::into)
    }

    /// Discovers config files from `cwd`.
    ///
    /// The local project config is required; the global config is optional.
    ///
    /// # Errors
    ///
    /// - [`DiscoveryError::LocalConfigAbsent`] when no local config exists in
    ///   any ancestor directory.
    /// - [`DiscoveryError::PathInaccessible`] when a path cannot be accessed.
    #[inline]
    fn discover(cwd: &Path) -> Result<DiscoveryOutcome, DiscoveryError> {
        let ctx = DiscoveryContext::new(
            DiscoveryScope::Full,
            DiscoveryAnchor::Directory(cwd.to_path_buf()),
        )?;
        DiscoveryEngine::process(ctx)
    }

    /// Builds a [`Config`] from discovered candidates.
    ///
    /// Selects the local and optional global config files according to
    /// [`ConfigBuilderInput`]'s precedence rules. Parses and merges global
    /// settings before local settings, resolving the final output directory.
    ///
    /// Trust and tracking notes:
    ///
    /// - The local candidate's root is checked against the trust store before
    ///   parsing; a global candidate is never checked.
    /// - Recording the candidate in the tracking store is best-effort; a write
    ///   failure does not fail the build.
    ///
    /// # Errors
    ///
    /// - [`ConfigBuilderError::WrongDiscoveryKindForBuild`],
    ///   [`ConfigBuilderError::FullDiscoveryWithoutLocal`], or
    ///   [`ConfigBuilderError::FullDiscoveryWithoutAnchorLocal`] when discovery
    ///   output is not valid builder input.
    /// - [`ConfigBuilderError::Untrusted`] when the local config's workspace is
    ///   not trusted, is missing its baseline hash, or is stale.
    /// - [`ConfigBuilderError::ConfigFile`] when a selected config file fails
    ///   path validation, tracking, trust transition, or parsing.
    /// - [`ConfigBuilderError::Merge`] when the merged local/global config
    ///   cannot be re-extracted for its output directory.
    /// - [`ConfigBuilderError::InvalidFieldKey`] when a `[frontmatter]` or
    ///   `[schemas]` key name is empty, whitespace-only, or canonicalizes to
    ///   nothing.
    fn build(
        &self,
        discovered: DiscoveryOutcome,
    ) -> Result<Config, ConfigBuilderError> {
        let input = ConfigBuilderInput::try_from(discovered)?;
        let tracked_local =
            LocalConfigFile::<Tracked>::from((input.local, &self.state));
        let trusted_local = match tracked_local.verify_trust(&self.state)? {
            TrustOutcome::Trusted(trusted) => trusted,
            TrustOutcome::Halted(file, status) => {
                return Err(ConfigBuilderError::Untrusted {
                    file,
                    status,
                });
            }
        };

        let root = trusted_local.root().to_path_buf();
        let mut figment = Figment::new();
        let mut global_dir = None;

        if let Some(global) = input.global {
            let parsed = GlobalConfigFile::<Parsed>::try_from(global)?;
            global_dir = parsed.resolved_template_dir();
            figment = figment.merge(Serialized::defaults(parsed.raw()));
        }

        let parsed_local = LocalConfigFile::<Parsed>::try_from(trusted_local)?;
        let local_dir = parsed_local.resolved_template_dir();
        figment = figment.merge(Serialized::defaults(parsed_local.raw()));

        let merged = figment.extract::<RawConfig>().map_err(|source| {
            ConfigBuilderError::Merge {
                source: Box::new(source),
            }
        })?;
        let output =
            merged.templates.output_dir.unwrap_or_else(|| root.clone());

        let schemas =
            SchemasConfig::try_from(merged.schemas).map_err(|source| {
                ConfigBuilderError::InvalidFieldKey {
                    table: "schemas",
                    source,
                }
            })?;
        let frontmatter = FrontmatterConfig::try_from(merged.frontmatter)
            .map_err(|source| ConfigBuilderError::InvalidFieldKey {
                table: "frontmatter",
                source,
            })?;

        Ok(Config::new(
            root,
            TemplateConfig::new(local_dir, global_dir, output),
            schemas,
            frontmatter,
        ))
    }

    /// Resolves trust subjects from one user-supplied filesystem path.
    ///
    /// # Errors
    ///
    /// - [`DiscoveryError::PathInaccessible`] when the path cannot be
    ///   inspected.
    /// - [`DiscoveryError::UnsupportedTrustScope`] when `scope` is
    ///   [`DiscoveryScope::Full`], which trust resolution does not support.
    /// - [`DiscoveryError::ConfigFile`] when a config-file anchor is invalid.
    /// - [`DiscoveryError::LocalConfigAbsent`] when
    ///   [`DiscoveryScope::LocalSubtree`] discovery has no local root to walk
    ///   from.
    #[inline]
    #[expect(
        clippy::unused_self,
        reason = "service owns the discovery seam even though trust-subject \
                  discovery has no state dependency today"
    )]
    pub(crate) fn trust_requests(
        &self,
        path: &Path,
        scope: DiscoveryScope,
    ) -> Result<TrustRequests, DiscoveryError> {
        DiscoveryEngine::trust_requests(path, scope)
    }

    /// Grants trust for a workspace root.
    ///
    /// When `subject` carries a config file, also records the file's current
    /// content hash as the trust baseline.
    ///
    /// # Errors
    ///
    /// - [`ConfigStateError::Store`] when trust cannot be recorded.
    /// - [`ConfigStateError::Hash`] when the config file cannot be hashed.
    #[inline]
    pub(crate) fn trust(
        &self,
        subject: &TrustRequest,
    ) -> Result<(), ConfigStateError> {
        self.state.grant_trust(subject)
    }

    /// Returns the trust status for `subject`.
    ///
    /// For config-file subjects, checks the baseline hash. For root-only
    /// subjects, checks only workspace presence.
    ///
    /// # Errors
    ///
    /// - [`ConfigStateError::Store`] when the trust store cannot be read.
    /// - [`ConfigStateError::Hash`] when the config file cannot be hashed.
    #[inline]
    pub(crate) fn trust_status(
        &self,
        subject: &TrustRequest,
    ) -> Result<ConfigTrustStatus, ConfigStateError> {
        if subject.config_file().is_some() {
            self.state.config_trust_status(subject)
        } else {
            self.state
                .workspace_trust_status(subject)
                .map(ConfigTrustStatus::from)
        }
    }

    /// Removes trust for `subject`'s workspace root.
    ///
    /// Returns the number of root entries removed.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigStateError::Store`] when the trust entry cannot be
    /// removed.
    #[inline]
    pub fn untrust(
        &self,
        subject: &TrustRequest,
    ) -> Result<usize, ConfigStateError> {
        self.state.revoke_trust(subject)
    }

    /// Lists the canonical paths of all live tracked configs.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigStateError::Store`] when the tracking store exists but
    /// cannot be read.
    #[inline]
    pub(crate) fn list_tracked(
        &self,
    ) -> Result<Vec<PathBuf>, ConfigStateError> {
        self.state.list_tracked_configs()
    }

    /// Removes dangling tracked-config entries.
    ///
    /// Returns the number of entries removed.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigStateError::Store`] when the tracking store exists but
    /// cannot be read, or a stale entry cannot be removed.
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
    /// Returns [`ConfigStateError::Store`] when the trust store exists but
    /// cannot be read.
    #[inline]
    pub(crate) fn list_trusted(
        &self,
    ) -> Result<Vec<PathBuf>, ConfigStateError> {
        self.state.list_trusted_workspaces()
    }

    /// Removes dangling trust entries and their content-hash companions.
    ///
    /// Returns the number of root entries removed.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigStateError::Store`] when the trust store exists but
    /// cannot be read, a stale root entry cannot be removed, or an existing
    /// content-hash companion cannot be removed.
    #[inline]
    pub(crate) fn clean_trusted_store(
        &self,
    ) -> Result<usize, ConfigStateError> {
        self.state.clean_trusted_workspaces()
    }

    /// Serialises `directory`/`output_dir` as the local template config and
    /// writes it to `root.join(LOCAL_CONFIG_FILE)`.
    ///
    /// `[schemas]` and `[frontmatter]` are written as empty tables (their
    /// serde defaults), so a freshly scaffolded config behaves identically to
    /// one that omits those tables entirely.
    ///
    /// Uses [`std::fs::File::create_new`] rather than [`std::fs::write`] so
    /// this fails atomically if the file already exists, preventing a
    /// concurrent `traces init` or a file planted between the existence check
    /// and write from being silently clobbered.
    ///
    /// # Errors
    ///
    /// - [`ConfigScaffoldError::Serialize`] when TOML serialization fails.
    /// - [`ConfigScaffoldError::Write`] when the file already exists, or
    ///   creating or writing it fails.
    #[inline]
    pub(crate) fn scaffold_local(
        root: &Path,
        directory: &Path,
        output_dir: &Path,
    ) -> Result<(), ConfigScaffoldError> {
        let config = RawConfig {
            templates: RawTemplateConfig {
                directory: Some(directory.to_path_buf()),
                output_dir: Some(output_dir.to_path_buf()),
            },
            ..RawConfig::default()
        };
        let contents = toml::to_string(&config).map_err(|source| {
            ConfigScaffoldError::Serialize {
                source,
            }
        })?;
        let mut file = fs::File::create_new(root.join(LOCAL_CONFIG_FILE))
            .map_err(|source| ConfigScaffoldError::Write {
                source,
            })?;
        file.write_all(contents.as_bytes()).map_err(|source| {
            ConfigScaffoldError::Write {
                source,
            }
        })
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
        file::{Discovered, LocalConfigFile},
    };

    struct Fixture {
        temp: tempfile::TempDir,
        tracked_root: PathBuf,
        _trusted_root: PathBuf,
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
                _trusted_root: trusted_root,
                service,
            }
        }

        fn target_dir(&self, name: &str) -> PathBuf {
            let path = self.temp.path().join(name);
            fs::create_dir_all(&path).expect("create target dir");
            path
        }

        fn create_config(root: &Path, contents: &str) -> PathBuf {
            let config_path = root.join(".traces/config.toml");
            fs::create_dir_all(config_path.parent().unwrap())
                .expect("create config parent");
            fs::write(&config_path, contents).expect("write config");
            config_path
        }

        fn discovered_config(
            config_path: &Path,
        ) -> LocalConfigFile<Discovered> {
            LocalConfigFile::<Discovered>::try_new(config_path.to_path_buf())
                .expect("valid local config")
        }

        fn trust_config(&self, config_path: &Path) {
            let config = Fixture::discovered_config(config_path);
            self.service
                .trust(&TrustRequest::from(&config))
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
            assert!(format!("{service:?}").contains(tracked.to_str().unwrap()));
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

            let config_path = Fixture::create_config(
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
            let config_path = Fixture::create_config(&cwd, "");
            let local = Fixture::discovered_config(&config_path);
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
            let (_cwd, _config_path, candidates) = local_candidates(&fixture);
            // Do NOT trust the root

            // Act
            let result = fixture.service.build(candidates);

            // Assert
            assert!(matches!(
                result,
                Err(ConfigBuilderError::Untrusted {
                    status: ConfigTrustStatus::Untrusted,
                    ..
                })
            ));
        }

        #[test]
        fn rejects_trusted_but_stale_root() {
            // Arrange
            let fixture = Fixture::new();
            let (_cwd, config_path, candidates) = local_candidates(&fixture);
            fixture.trust_config(&config_path);

            // Edit config after trusting to make it stale
            fs::write(&config_path, "directory = \"changed\"").unwrap();

            // Act
            let result = fixture.service.build(candidates);

            // Assert
            assert!(matches!(
                result,
                Err(ConfigBuilderError::Untrusted {
                    status: ConfigTrustStatus::Stale,
                    ..
                })
            ));
        }
    }

    mod trust_requests {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn delegates_to_discovery_engine() {
            // Arrange
            let fixture = Fixture::new();
            let cwd = fixture.target_dir("project");
            Fixture::create_config(&cwd, "");

            // Act
            let result = fixture
                .service
                .trust_requests(&cwd, DiscoveryScope::NearestLocal);

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
            let subject = TrustRequest::from(root.as_path());

            // Act
            let result = fixture.service.trust(&subject);

            // Assert
            assert!(result.is_ok());
            assert_eq!(
                fixture.service.trust_status(&subject).unwrap(),
                ConfigTrustStatus::Trusted
            );
        }

        #[test]
        fn records_config_trust_and_hashes_content() {
            // Arrange
            let fixture = Fixture::new();
            let root = fixture.target_dir("project");
            let config_path = Fixture::create_config(&root, "a = 1");
            let config = Fixture::discovered_config(&config_path);
            let subject = TrustRequest::from(&config);

            // Act
            let result = fixture.service.trust(&subject);

            // Assert
            assert!(result.is_ok());
            assert_eq!(
                fixture.service.trust_status(&subject).unwrap(),
                ConfigTrustStatus::Trusted
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
            let subject = TrustRequest::from(root.as_path());

            // Act
            let result = fixture.service.trust_status(&subject);

            // Assert
            assert!(result.is_ok());
            assert_eq!(result.unwrap(), ConfigTrustStatus::Untrusted);
        }

        #[test]
        fn returns_untrusted_for_unknown_config() {
            // Arrange
            let fixture = Fixture::new();
            let root = fixture.target_dir("project");
            let config_path = Fixture::create_config(&root, "a = 1");
            let config = Fixture::discovered_config(&config_path);
            let subject = TrustRequest::from(&config);

            // Act
            let result = fixture.service.trust_status(&subject);

            // Assert
            assert!(result.is_ok());
            assert_eq!(result.unwrap(), ConfigTrustStatus::Untrusted);
        }

        #[test]
        fn returns_stale_when_config_content_changes() {
            // Arrange
            let fixture = Fixture::new();
            let root = fixture.target_dir("project");
            let config_path = Fixture::create_config(&root, "a = 1");
            let config = Fixture::discovered_config(&config_path);
            let subject = TrustRequest::from(&config);

            fixture.service.trust(&subject).unwrap();
            fs::write(&config_path, "a = 2").unwrap();

            // Act
            let result = fixture.service.trust_status(&subject);

            // Assert
            assert!(result.is_ok());
            assert_eq!(result.unwrap(), ConfigTrustStatus::Stale);
        }
    }

    mod untrust {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn returns_zero_when_already_untrusted() {
            // Arrange
            let fixture = Fixture::new();
            let root = fixture.target_dir("project");
            let subject = TrustRequest::from(root.as_path());

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
            let config_path = Fixture::create_config(&cwd, "");
            let local = Fixture::discovered_config(&config_path);
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
            let config_path = Fixture::create_config(&cwd, "");
            let local = Fixture::discovered_config(&config_path);
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
            let config_path = Fixture::create_config(&cwd, "");
            let local = Fixture::discovered_config(&config_path);
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
            let config_path = Fixture::create_config(&root, "a = 1");
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
            let config_path = Fixture::create_config(&root, "a = 1");
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
            let config_path = Fixture::create_config(&root, "a = 1");
            fixture.trust_config(&config_path);

            // Act
            let result = fixture.service.clean_trusted_store();

            // Assert
            assert!(result.is_ok());
            assert_eq!(result.unwrap(), 0);
            assert_eq!(fixture.service.list_trusted().unwrap().len(), 1);
        }
    }

    mod scaffold_local {
        use super::*;

        #[test]
        fn writes_the_local_config_file() {
            let root = tempfile::tempdir().expect("create temp dir");
            std::fs::create_dir(root.path().join(".traces"))
                .expect("create .traces dir");

            ConfigService::scaffold_local(
                root.path(),
                Path::new("templates"),
                Path::new("notes"),
            )
            .expect("scaffold local config");

            let contents =
                std::fs::read_to_string(root.path().join(LOCAL_CONFIG_FILE))
                    .expect("read written config");
            assert!(contents.contains("templates"));
        }

        #[test]
        fn refuses_to_overwrite_an_existing_config_file() {
            let root = tempfile::tempdir().expect("create temp dir");
            std::fs::create_dir(root.path().join(".traces"))
                .expect("create .traces dir");
            ConfigService::scaffold_local(
                root.path(),
                Path::new("templates"),
                Path::new("notes"),
            )
            .expect("first scaffold succeeds");

            let error = ConfigService::scaffold_local(
                root.path(),
                Path::new("other"),
                Path::new("elsewhere"),
            )
            .expect_err(
                "second scaffold at the same root must fail, not clobber",
            );

            assert!(matches!(error, ConfigScaffoldError::Write { .. }));
        }
    }

    /// Tests discovery-output selection and the build pipeline.
    ///
    /// Migrated from the standalone `builder` module and kept as a separate
    /// fixture because it needs local/global candidate helpers the outer test
    /// [`Fixture`] does not.
    ///
    /// [`Fixture`]: super::Fixture
    mod builder {
        use super::*;

        struct Fixture {
            temp: tempfile::TempDir,
            trust_store: tempfile::TempDir,
            tracked_store: tempfile::TempDir,
        }

        impl Fixture {
            fn new() -> Self {
                Self {
                    temp: tempfile::tempdir().expect("create temp dir"),
                    trust_store: tempfile::tempdir()
                        .expect("create trust store"),
                    tracked_store: tempfile::tempdir()
                        .expect("create tracked store"),
                }
            }

            fn service(&self) -> ConfigService {
                ConfigService::at(
                    self.tracked_store.path().to_path_buf(),
                    self.trust_store.path().to_path_buf(),
                )
            }

            fn write_config(&self, subpath: &str, contents: &str) -> PathBuf {
                let path = self.temp.path().join(subpath);
                let parent = path.parent().expect("config path parent");
                fs::create_dir_all(parent).expect("create config parent");
                fs::write(&path, contents).expect("write config");
                path
            }

            fn local(
                &self,
                root_subpath: &str,
            ) -> LocalConfigFile<FileDiscovered> {
                let root = self.temp.path().join(root_subpath);
                let path = root.join(".traces/config.toml");
                if !path.exists() {
                    self.write_config(
                        &format!("{root_subpath}/.traces/config.toml"),
                        "[templates]",
                    );
                }
                LocalConfigFile::<FileDiscovered>::try_new(path)
                    .expect("valid local config")
            }

            fn global(
                &self,
                root_subpath: &str,
            ) -> GlobalConfigFile<FileDiscovered> {
                let root = self.temp.path().join(root_subpath);
                let path = root.join("config.toml");
                if !path.exists() {
                    self.write_config(
                        &format!("{root_subpath}/config.toml"),
                        "[templates]",
                    );
                }
                GlobalConfigFile::<FileDiscovered>::try_new(path)
                    .expect("valid global config")
            }

            fn trust(&self, local: &LocalConfigFile<FileDiscovered>) {
                self.service()
                    .trust(&TrustRequest::from(local))
                    .expect("trust local");
            }
        }

        fn build(
            fixture: &Fixture,
            local: LocalConfigFile<FileDiscovered>,
            global: Option<GlobalConfigFile<FileDiscovered>>,
        ) -> Result<Config, ConfigBuilderError> {
            fixture.trust(&local);
            let anchor = local.root().to_path_buf();
            let outcome = DiscoveryOutcome::with_kind(
                DiscoveryScope::Full,
                DiscoveryAnchor::Directory(anchor),
                vec![local],
                global.into_iter().collect(),
            );
            fixture.service().build(outcome)
        }

        mod input {
            use pretty_assertions::assert_eq;

            use super::*;

            #[test]
            fn rejects_non_full_discovery_output() {
                let fixture = Fixture::new();
                let local = fixture.local("project");
                let outcome = DiscoveryOutcome::with_kind(
                    DiscoveryScope::NearestLocal,
                    DiscoveryAnchor::Directory(local.root().to_path_buf()),
                    vec![local],
                    Vec::new(),
                );

                let error = ConfigBuilderInput::try_from(outcome)
                    .expect_err("wrong kind");

                assert!(matches!(
                    error,
                    ConfigBuilderError::WrongDiscoveryKindForBuild {
                        actual: DiscoveryScope::NearestLocal
                    }
                ));
            }

            #[test]
            fn rejects_full_discovery_without_local() {
                let fixture = Fixture::new();
                let anchor = fixture.temp.path().join("project");
                let outcome = DiscoveryOutcome::with_kind(
                    DiscoveryScope::Full,
                    DiscoveryAnchor::Directory(anchor),
                    Vec::new(), // Empty locals
                    Vec::new(),
                );

                let error = ConfigBuilderInput::try_from(outcome)
                    .expect_err("missing locals");

                assert!(matches!(
                    error,
                    ConfigBuilderError::FullDiscoveryWithoutLocal
                ));
            }

            #[test]
            fn rejects_full_discovery_without_anchor_local() {
                let fixture = Fixture::new();
                let local = fixture.local("project");
                let anchor = fixture.temp.path().join("other");
                let outcome = DiscoveryOutcome::with_kind(
                    DiscoveryScope::Full,
                    DiscoveryAnchor::Directory(anchor.clone()),
                    vec![local],
                    Vec::new(),
                );

                let error = ConfigBuilderInput::try_from(outcome)
                    .expect_err("missing anchor local");

                assert!(matches!(
                    error,
                    ConfigBuilderError::FullDiscoveryWithoutAnchorLocal { anchor: error_anchor }
                        if error_anchor == anchor
                ));
            }

            #[test]
            fn selects_nearest_local_for_full_discovery() {
                let fixture = Fixture::new();
                let parent = fixture.local("parent");
                let child = fixture.local("parent/child");
                let anchor = fixture.temp.path().join("parent/child/notes");

                let outcome = DiscoveryOutcome::with_kind(
                    DiscoveryScope::Full,
                    DiscoveryAnchor::Directory(anchor),
                    vec![parent, child.clone()],
                    Vec::new(),
                );

                let input = ConfigBuilderInput::try_from(outcome)
                    .expect("select builder input");

                assert_eq!(input.local.root(), child.root());
            }

            #[test]
            fn selects_first_discovered_global() {
                let fixture = Fixture::new();
                let local = fixture.local("project");
                let global1 = fixture.global("global1");
                let global2 = fixture.global("global2");

                let outcome = DiscoveryOutcome::with_kind(
                    DiscoveryScope::Full,
                    DiscoveryAnchor::Directory(local.root().to_path_buf()),
                    vec![local],
                    vec![global1.clone(), global2],
                );

                // Act
                let result = ConfigBuilderInput::try_from(outcome);

                // Assert
                let input = result.expect("select builder input");
                let global = input.global.expect("expected global");
                assert_eq!(global.path(), global1.path());
            }
        }

        mod merge {
            use pretty_assertions::assert_eq;

            use super::*;

            #[test]
            fn extracts_local_output_dir() {
                let fixture = Fixture::new();
                let local_path = fixture.write_config(
                    "project/.traces/config.toml",
                    "[templates]\noutput_dir = \"local_out\"",
                );
                let local =
                    LocalConfigFile::<FileDiscovered>::try_new(local_path)
                        .unwrap();

                // Act
                let config = build(&fixture, local, None).expect("build");

                // Assert
                assert_eq!(config.output_dir(), Path::new("local_out"));
            }

            #[test]
            fn leaves_global_template_dir_empty_when_missing() {
                let fixture = Fixture::new();
                let local_path = fixture.write_config(
                    "project/.traces/config.toml",
                    "[templates]\noutput_dir = \"local_out\"",
                );
                let local =
                    LocalConfigFile::<FileDiscovered>::try_new(local_path)
                        .unwrap();

                // Act
                let config = build(&fixture, local, None).expect("build");

                // Assert
                assert_eq!(config.global_template_dir(), None);
            }

            #[test]
            fn extracts_local_template_dir() {
                let fixture = Fixture::new();
                let local_path = fixture.write_config(
                    "project/.traces/config.toml",
                    "[templates]\ndirectory = \".traces/templates\"",
                );
                let global_path = fixture.write_config(
                    "global/config.toml",
                    "[templates]\ndirectory = \"global_tmpl\"",
                );

                let local =
                    LocalConfigFile::<FileDiscovered>::try_new(local_path)
                        .unwrap();
                let global =
                    GlobalConfigFile::<FileDiscovered>::try_new(global_path)
                        .unwrap();

                // Act
                let config = build(&fixture, local.clone(), Some(global))
                    .expect("build");

                // Assert
                assert_eq!(
                    config.local_template_dir(),
                    Some(local.root().join(".traces/templates").as_path())
                );
            }

            #[test]
            fn extracts_global_template_dir() {
                let fixture = Fixture::new();
                let local_path = fixture.write_config(
                    "project/.traces/config.toml",
                    "[templates]\ndirectory = \".traces/templates\"",
                );
                let global_path = fixture.write_config(
                    "global/config.toml",
                    "[templates]\ndirectory = \"global_tmpl\"",
                );

                let local =
                    LocalConfigFile::<FileDiscovered>::try_new(local_path)
                        .unwrap();
                let global =
                    GlobalConfigFile::<FileDiscovered>::try_new(global_path)
                        .unwrap();

                // Act
                let config = build(&fixture, local, Some(global.clone()))
                    .expect("build");

                // Assert
                assert_eq!(
                    config.global_template_dir(),
                    Some(global.root().join("global_tmpl").as_path())
                );
            }

            #[test]
            fn prioritizes_local_output_dir() {
                let fixture = Fixture::new();
                let local_path = fixture.write_config(
                    "project/.traces/config.toml",
                    "[templates]\noutput_dir = \"local_out\"",
                );
                let global_path = fixture.write_config(
                    "global/config.toml",
                    "[templates]\noutput_dir = \"global_out\"",
                );

                let local =
                    LocalConfigFile::<FileDiscovered>::try_new(local_path)
                        .unwrap();
                let global =
                    GlobalConfigFile::<FileDiscovered>::try_new(global_path)
                        .unwrap();

                // Act
                let config =
                    build(&fixture, local, Some(global)).expect("build");

                // Assert
                assert_eq!(config.output_dir(), Path::new("local_out"));
            }

            #[test]
            fn uses_local_root_when_output_dir_missing() {
                let fixture = Fixture::new();
                let local_path = fixture
                    .write_config("project/.traces/config.toml", "[templates]");
                let local =
                    LocalConfigFile::<FileDiscovered>::try_new(local_path)
                        .unwrap();

                // Act
                let config =
                    build(&fixture, local.clone(), None).expect("build");

                // Assert
                assert_eq!(config.output_dir(), local.root());
            }

            #[test]
            fn returns_error_when_global_parsing_fails() {
                let fixture = Fixture::new();
                let local_path = fixture
                    .write_config("project/.traces/config.toml", "[templates]");
                let global_path =
                    fixture.write_config("global/config.toml", "[[[BAD TOML");

                let local =
                    LocalConfigFile::<FileDiscovered>::try_new(local_path)
                        .unwrap();
                let global =
                    GlobalConfigFile::<FileDiscovered>::try_new(global_path)
                        .unwrap();

                // Act
                let result = build(&fixture, local, Some(global));

                assert!(matches!(
                    result,
                    Err(ConfigBuilderError::ConfigFile(
                        ConfigFileError::Read { .. }
                    ))
                ));
            }

            #[test]
            fn returns_error_when_local_parsing_fails() {
                let fixture = Fixture::new();
                let local_path = fixture
                    .write_config("project/.traces/config.toml", "[[[BAD TOML");
                let local =
                    LocalConfigFile::<FileDiscovered>::try_new(local_path)
                        .unwrap();

                // Act
                let result = build(&fixture, local, None);

                assert!(matches!(
                    result,
                    Err(ConfigBuilderError::ConfigFile(
                        ConfigFileError::Read { .. }
                    ))
                ));
            }

            #[test]
            fn falls_back_to_global_output_dir_when_local_omits_output_dir() {
                let fixture = Fixture::new();
                let local_path = fixture.write_config(
                    "project/.traces/config.toml",
                    "[templates]\ndirectory = \"local_tmpl\"",
                );
                let global_path = fixture.write_config(
                    "global/config.toml",
                    "[templates]\noutput_dir = \"global_out\"",
                );
                let local =
                    LocalConfigFile::<FileDiscovered>::try_new(local_path)
                        .unwrap();
                let global =
                    GlobalConfigFile::<FileDiscovered>::try_new(global_path)
                        .unwrap();

                // Act
                let config =
                    build(&fixture, local, Some(global)).expect("build");

                // Assert
                assert_eq!(config.output_dir(), Path::new("global_out"));
            }
        }

        mod schemas {
            use pretty_assertions::assert_eq;

            use super::*;

            #[test]
            fn extracts_local_class_field() {
                let fixture = Fixture::new();
                let local_path = fixture.write_config(
                    "project/.traces/config.toml",
                    "[schemas]\nclass_field = \"kind\"",
                );
                let local =
                    LocalConfigFile::<FileDiscovered>::try_new(local_path)
                        .unwrap();

                // Act
                let config = build(&fixture, local, None).expect("build");

                // Assert
                assert_eq!(config.schemas().class_field_name(), "kind");
            }

            #[test]
            fn defaults_class_field_when_unconfigured() {
                let fixture = Fixture::new();
                let local_path = fixture
                    .write_config("project/.traces/config.toml", "[templates]");
                let local =
                    LocalConfigFile::<FileDiscovered>::try_new(local_path)
                        .unwrap();

                // Act
                let config = build(&fixture, local, None).expect("build");

                // Assert
                assert_eq!(config.schemas().class_field_name(), "class");
            }

            #[test]
            fn extracts_local_directory() {
                let fixture = Fixture::new();
                let local_path = fixture.write_config(
                    "project/.traces/config.toml",
                    "[schemas]\ndirectory = \"custom/schemas\"",
                );
                let local =
                    LocalConfigFile::<FileDiscovered>::try_new(local_path)
                        .unwrap();

                // Act
                let config = build(&fixture, local, None).expect("build");

                // Assert
                assert_eq!(
                    config.schemas().directory(),
                    Path::new("custom/schemas")
                );
            }

            #[test]
            fn defaults_directory_when_unconfigured() {
                let fixture = Fixture::new();
                let local_path = fixture
                    .write_config("project/.traces/config.toml", "[templates]");
                let local =
                    LocalConfigFile::<FileDiscovered>::try_new(local_path)
                        .unwrap();

                // Act
                let config = build(&fixture, local, None).expect("build");

                // Assert
                assert_eq!(
                    config.schemas().directory(),
                    Path::new(".traces/schemas/")
                );
            }

            #[test]
            fn prioritizes_local_class_field_over_global() {
                let fixture = Fixture::new();
                let local_path = fixture.write_config(
                    "project/.traces/config.toml",
                    "[schemas]\nclass_field = \"local_kind\"",
                );
                let global_path = fixture.write_config(
                    "global/config.toml",
                    "[schemas]\nclass_field = \"global_kind\"",
                );
                let local =
                    LocalConfigFile::<FileDiscovered>::try_new(local_path)
                        .unwrap();
                let global =
                    GlobalConfigFile::<FileDiscovered>::try_new(global_path)
                        .unwrap();

                // Act
                let config =
                    build(&fixture, local, Some(global)).expect("build");

                // Assert
                assert_eq!(config.schemas().class_field_name(), "local_kind");
            }

            #[test]
            fn falls_back_to_global_directory_when_local_omits_directory() {
                let fixture = Fixture::new();
                let local_path = fixture.write_config(
                    "project/.traces/config.toml",
                    "[schemas]\nclass_field = \"local_kind\"",
                );
                let global_path = fixture.write_config(
                    "global/config.toml",
                    "[schemas]\ndirectory = \"global/schemas\"",
                );
                let local =
                    LocalConfigFile::<FileDiscovered>::try_new(local_path)
                        .unwrap();
                let global =
                    GlobalConfigFile::<FileDiscovered>::try_new(global_path)
                        .unwrap();

                // Act
                let config =
                    build(&fixture, local, Some(global)).expect("build");

                // Assert
                assert_eq!(
                    config.schemas().directory(),
                    Path::new("global/schemas")
                );
            }

            #[test]
            fn rejects_unknown_key() {
                let fixture = Fixture::new();
                let local_path = fixture.write_config(
                    "project/.traces/config.toml",
                    "[schemas]\nbogus = 1",
                );
                let local =
                    LocalConfigFile::<FileDiscovered>::try_new(local_path)
                        .unwrap();

                // Act
                let result = build(&fixture, local, None);

                // Assert
                assert!(matches!(
                    result,
                    Err(ConfigBuilderError::ConfigFile(
                        ConfigFileError::Read { .. }
                    ))
                ));
            }

            #[test]
            fn rejects_empty_class_field() {
                let fixture = Fixture::new();
                let local_path = fixture.write_config(
                    "project/.traces/config.toml",
                    "[schemas]\nclass_field = \"   \"",
                );
                let local =
                    LocalConfigFile::<FileDiscovered>::try_new(local_path)
                        .unwrap();

                // Act
                let result = build(&fixture, local, None);

                // Assert
                assert!(matches!(
                    result,
                    Err(ConfigBuilderError::InvalidFieldKey {
                        table: "schemas",
                        ..
                    })
                ));
            }
        }

        mod frontmatter {
            use pretty_assertions::assert_eq;

            use super::*;

            #[test]
            fn parses_title() {
                let fixture = Fixture::new();
                let local_path = fixture.write_config(
                    "project/.traces/config.toml",
                    "[frontmatter]\ntitle = \"Title\"",
                );
                let local =
                    LocalConfigFile::<FileDiscovered>::try_new(local_path)
                        .unwrap();

                // Act
                let config = build(&fixture, local, None).expect("build");

                // Assert
                assert_eq!(config.frontmatter().title_name(), "Title");
            }

            #[test]
            fn parses_aliases() {
                let fixture = Fixture::new();
                let local_path = fixture.write_config(
                    "project/.traces/config.toml",
                    "[frontmatter]\naliases = \"Aliases\"",
                );
                let local =
                    LocalConfigFile::<FileDiscovered>::try_new(local_path)
                        .unwrap();

                // Act
                let config = build(&fixture, local, None).expect("build");

                // Assert
                assert_eq!(config.frontmatter().aliases_name(), "Aliases");
            }

            #[test]
            fn defaults_title_when_unconfigured() {
                let fixture = Fixture::new();
                let local_path = fixture
                    .write_config("project/.traces/config.toml", "[templates]");
                let local =
                    LocalConfigFile::<FileDiscovered>::try_new(local_path)
                        .unwrap();

                // Act
                let config = build(&fixture, local, None).expect("build");

                // Assert
                assert_eq!(config.frontmatter().title_name(), "title");
            }

            #[test]
            fn defaults_aliases_when_unconfigured() {
                let fixture = Fixture::new();
                let local_path = fixture
                    .write_config("project/.traces/config.toml", "[templates]");
                let local =
                    LocalConfigFile::<FileDiscovered>::try_new(local_path)
                        .unwrap();

                // Act
                let config = build(&fixture, local, None).expect("build");

                // Assert
                assert_eq!(config.frontmatter().aliases_name(), "aliases");
            }

            #[test]
            fn parses_date_created() {
                let fixture = Fixture::new();
                let local_path = fixture.write_config(
                    "project/.traces/config.toml",
                    "[frontmatter.date_created]\nname = \"created\"\nformat = \
                     \"%Y-%m-%d\"",
                );
                let local =
                    LocalConfigFile::<FileDiscovered>::try_new(local_path)
                        .unwrap();

                // Act
                let config = build(&fixture, local, None).expect("build");

                // Assert
                let created = config.frontmatter().date_created();
                assert_eq!(created.name(), "created");
                assert_eq!(created.format(), "%Y-%m-%d");
            }

            #[test]
            fn parses_date_modified() {
                let fixture = Fixture::new();
                let local_path = fixture.write_config(
                    "project/.traces/config.toml",
                    "[frontmatter.date_modified]\nname = \"modified\"\nformat \
                     = \"%Y-%m-%dT%H:%M:%S\"",
                );
                let local =
                    LocalConfigFile::<FileDiscovered>::try_new(local_path)
                        .unwrap();

                // Act
                let config = build(&fixture, local, None).expect("build");

                // Assert
                let modified = config.frontmatter().date_modified();
                assert_eq!(modified.name(), "modified");
                assert_eq!(modified.format(), "%Y-%m-%dT%H:%M:%S");
            }

            #[test]
            fn defaults_date_created_when_unconfigured() {
                let fixture = Fixture::new();
                let local_path = fixture
                    .write_config("project/.traces/config.toml", "[templates]");
                let local =
                    LocalConfigFile::<FileDiscovered>::try_new(local_path)
                        .unwrap();

                // Act
                let config = build(&fixture, local, None).expect("build");

                // Assert
                let created = config.frontmatter().date_created();
                assert_eq!(created.name(), "date_created");
                assert_eq!(created.format(), "%Y-%m-%dT%H:%M:%S");
            }

            #[test]
            fn defaults_date_modified_when_unconfigured() {
                let fixture = Fixture::new();
                let local_path = fixture
                    .write_config("project/.traces/config.toml", "[templates]");
                let local =
                    LocalConfigFile::<FileDiscovered>::try_new(local_path)
                        .unwrap();

                // Act
                let config = build(&fixture, local, None).expect("build");

                // Assert
                let modified = config.frontmatter().date_modified();
                assert_eq!(modified.name(), "date_modified");
                assert_eq!(modified.format(), "%Y-%m-%dT%H:%M:%S");
            }

            #[test]
            fn rejects_unknown_key() {
                let fixture = Fixture::new();
                let local_path = fixture.write_config(
                    "project/.traces/config.toml",
                    "[frontmatter]\nbogus = 1",
                );
                let local =
                    LocalConfigFile::<FileDiscovered>::try_new(local_path)
                        .unwrap();

                // Act
                let result = build(&fixture, local, None);

                // Assert
                assert!(matches!(
                    result,
                    Err(ConfigBuilderError::ConfigFile(
                        ConfigFileError::Read { .. }
                    ))
                ));
            }

            #[test]
            fn rejects_unknown_date_field_key() {
                let fixture = Fixture::new();
                let local_path = fixture.write_config(
                    "project/.traces/config.toml",
                    "[frontmatter.date_created]\nname = \"created\"\nbogus = 1",
                );
                let local =
                    LocalConfigFile::<FileDiscovered>::try_new(local_path)
                        .unwrap();

                // Act
                let result = build(&fixture, local, None);

                // Assert
                assert!(matches!(
                    result,
                    Err(ConfigBuilderError::ConfigFile(
                        ConfigFileError::Read { .. }
                    ))
                ));
            }

            #[test]
            fn defaults_date_created_format_when_only_name_configured() {
                let fixture = Fixture::new();
                let local_path = fixture.write_config(
                    "project/.traces/config.toml",
                    "[frontmatter.date_created]\nname = \"created_at\"",
                );
                let local =
                    LocalConfigFile::<FileDiscovered>::try_new(local_path)
                        .unwrap();

                // Act
                let config = build(&fixture, local, None).expect("build");

                // Assert
                let created = config.frontmatter().date_created();
                assert_eq!(created.name(), "created_at");
                assert_eq!(created.format(), "%Y-%m-%dT%H:%M:%S");
            }

            #[test]
            fn defaults_date_modified_name_when_only_format_configured() {
                let fixture = Fixture::new();
                let local_path = fixture.write_config(
                    "project/.traces/config.toml",
                    "[frontmatter.date_modified]\nformat = \"%Y-%m-%d\"",
                );
                let local =
                    LocalConfigFile::<FileDiscovered>::try_new(local_path)
                        .unwrap();

                // Act
                let config = build(&fixture, local, None).expect("build");

                // Assert
                let modified = config.frontmatter().date_modified();
                assert_eq!(modified.name(), "date_modified");
                assert_eq!(modified.format(), "%Y-%m-%d");
            }

            #[test]
            fn rejects_empty_date_created_name() {
                let fixture = Fixture::new();
                let local_path = fixture.write_config(
                    "project/.traces/config.toml",
                    "[frontmatter.date_created]\nname = \"   \"",
                );
                let local =
                    LocalConfigFile::<FileDiscovered>::try_new(local_path)
                        .unwrap();

                // Act
                let result = build(&fixture, local, None);

                // Assert
                assert!(matches!(
                    result,
                    Err(ConfigBuilderError::InvalidFieldKey {
                        table: "frontmatter",
                        ..
                    })
                ));
            }

            #[test]
            fn prioritizes_local_title_over_global() {
                let fixture = Fixture::new();
                let local_path = fixture.write_config(
                    "project/.traces/config.toml",
                    "[frontmatter]\ntitle = \"local_title\"",
                );
                let global_path = fixture.write_config(
                    "global/config.toml",
                    "[frontmatter]\ntitle = \"global_title\"",
                );
                let local =
                    LocalConfigFile::<FileDiscovered>::try_new(local_path)
                        .unwrap();
                let global =
                    GlobalConfigFile::<FileDiscovered>::try_new(global_path)
                        .unwrap();

                // Act
                let config =
                    build(&fixture, local, Some(global)).expect("build");

                // Assert
                assert_eq!(config.frontmatter().title_name(), "local_title");
            }

            #[test]
            fn prioritizes_local_aliases_over_global() {
                let fixture = Fixture::new();
                let local_path = fixture.write_config(
                    "project/.traces/config.toml",
                    "[frontmatter]\naliases = \"aka\"",
                );
                let global_path = fixture.write_config(
                    "global/config.toml",
                    "[frontmatter]\naliases = \"aliases\"",
                );
                let local =
                    LocalConfigFile::<FileDiscovered>::try_new(local_path)
                        .unwrap();
                let global =
                    GlobalConfigFile::<FileDiscovered>::try_new(global_path)
                        .unwrap();

                // Act
                let config =
                    build(&fixture, local, Some(global)).expect("build");

                // Assert
                assert_eq!(config.frontmatter().aliases_name(), "aka");
            }

            #[test]
            fn falls_back_to_global_aliases_when_local_omits_aliases() {
                let fixture = Fixture::new();
                let local_path = fixture.write_config(
                    "project/.traces/config.toml",
                    "[frontmatter]\ntitle = \"local_title\"",
                );
                let global_path = fixture.write_config(
                    "global/config.toml",
                    "[frontmatter]\naliases = \"aka\"",
                );
                let local =
                    LocalConfigFile::<FileDiscovered>::try_new(local_path)
                        .unwrap();
                let global =
                    GlobalConfigFile::<FileDiscovered>::try_new(global_path)
                        .unwrap();

                // Act
                let config =
                    build(&fixture, local, Some(global)).expect("build");

                // Assert
                assert_eq!(config.frontmatter().title_name(), "local_title");
                assert_eq!(config.frontmatter().aliases_name(), "aka");
            }

            #[test]
            fn rejects_empty_title() {
                let fixture = Fixture::new();
                let local_path = fixture.write_config(
                    "project/.traces/config.toml",
                    "[frontmatter]\ntitle = \"\"",
                );
                let local =
                    LocalConfigFile::<FileDiscovered>::try_new(local_path)
                        .unwrap();

                // Act
                let result = build(&fixture, local, None);

                // Assert
                assert!(matches!(
                    result,
                    Err(ConfigBuilderError::InvalidFieldKey {
                        table: "frontmatter",
                        ..
                    })
                ));
            }

            #[test]
            fn rejects_whitespace_only_aliases() {
                let fixture = Fixture::new();
                let local_path = fixture.write_config(
                    "project/.traces/config.toml",
                    "[frontmatter]\naliases = \"   \"",
                );
                let local =
                    LocalConfigFile::<FileDiscovered>::try_new(local_path)
                        .unwrap();

                // Act
                let result = build(&fixture, local, None);

                // Assert
                assert!(matches!(
                    result,
                    Err(ConfigBuilderError::InvalidFieldKey {
                        table: "frontmatter",
                        ..
                    })
                ));
            }
        }
    }
}
