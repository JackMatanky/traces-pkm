//! Persistent state for config tracking and trust.
//!
//! [`ConfigStateStore`] wraps two [`FileStateStore`]s: one for config files
//! discovered during loads, and one for trusted workspace roots plus config
//! content baselines.

use std::{
    fs,
    path::{Path, PathBuf},
};

use thiserror::Error;

use super::{
    file::{Discovered, LocalConfigFile},
    trust::{ConfigTrustStatus, TrustRequest, WorkspaceTrustStatus},
};
use crate::{
    Blake3FileHash, FileStateStore, FileStateStoreError, FileStoreCleanMode,
    dirs, hash::HashError,
};

/// Errors from config tracking or trust-state operations.
#[derive(Debug, Error)]
pub(crate) enum ConfigStateError {
    /// The underlying hash-keyed store operation failed.
    #[error(transparent)]
    Store(#[from] FileStateStoreError),
    /// Hashing a config file failed.
    #[error(transparent)]
    Hash(#[from] HashError),
}

/// Result of checking whether a config file may be parsed.
///
/// Only [`Self::Trusted`] carries already-read content. The enum keeps the
/// content and trust decision together so callers cannot accidentally pair a
/// trusted status with missing content.
#[derive(Debug, Eq, PartialEq)]
pub(crate) enum ConfigTrustCheck {
    /// The workspace root is not trusted.
    Untrusted,
    /// The workspace root is trusted and a baseline hash exists, but the config
    /// file's current content no longer matches it.
    Stale,
    /// The workspace root is trusted, but no content-hash baseline was ever
    /// recorded for this config file.
    MissingBaseline,
    /// The workspace root is trusted and the config file's content matches its
    /// baseline hash. Carries the content read while verifying it.
    Trusted(String),
}

impl ConfigTrustCheck {
    /// The status-only view, discarding any trusted content.
    #[inline]
    #[must_use]
    pub(crate) fn status(&self) -> ConfigTrustStatus {
        match self {
            Self::Untrusted => ConfigTrustStatus::Untrusted,
            Self::Stale => ConfigTrustStatus::Stale,
            Self::MissingBaseline => ConfigTrustStatus::MissingBaseline,
            Self::Trusted(_) => ConfigTrustStatus::Trusted,
        }
    }
}

const COMPANION_SUFFIX: &str = ".hash";

/// Backing store for config tracking and trust records.
///
/// Wraps two independent hash-keyed [`FileStateStore`]s: `tracked` records
/// config files discovery has seen (best-effort bookkeeping); `trusted` records
/// workspace roots the user has explicitly trusted, plus each trusted config
/// file's content-hash baseline used to detect drift.
#[derive(Clone, Debug)]
pub(crate) struct ConfigStateStore {
    tracked: FileStateStore,
    trusted: FileStateStore,
}

impl ConfigStateStore {
    /// Creates the production state store at the platform state-dir roots.
    #[inline]
    #[must_use]
    pub(crate) fn new() -> Self {
        Self {
            tracked: FileStateStore::from((*dirs::TRACKED_CONFIGS).clone()),
            trusted: FileStateStore::from((*dirs::TRUSTED_CONFIGS).clone()),
        }
    }

    /// Creates a state store at explicit roots for tests.
    #[cfg(test)]
    #[inline]
    #[must_use]
    pub(crate) fn at(tracked_root: PathBuf, trusted_root: PathBuf) -> Self {
        Self {
            tracked: FileStateStore::at(tracked_root),
            trusted: FileStateStore::at(trusted_root),
        }
    }

    /// Records that discovery saw a config file.
    ///
    /// Best-effort: tracking is bookkeeping, so write failures warn and do not
    /// fail config loading.
    #[inline]
    pub(crate) fn track_seen_config(
        &self,
        config: &LocalConfigFile<Discovered>,
    ) {
        if let Err(error) = self.tracked.record(config.path()) {
            tracing::warn!(
                path = %config.path().display(),
                error = %error,
                "failed to record seen config file"
            );
        }
    }

    /// Grants trust for a workspace, optionally recording a config hash.
    ///
    /// # Errors
    ///
    /// - [`ConfigStateError::Store`] when trust cannot be recorded
    /// - [`ConfigStateError::Hash`] when the config file cannot be hashed
    #[inline]
    pub(crate) fn grant_trust(
        &self,
        subject: &TrustRequest,
    ) -> Result<(), ConfigStateError> {
        self.trusted.record(subject.root_path())?;
        let Some(config_file) = subject.config_file() else {
            return Ok(());
        };
        let digest = Blake3FileHash::try_from(config_file)?;
        self.trusted.write_companion(
            subject.root_path(),
            COMPANION_SUFFIX,
            digest.to_string(),
        )?;
        Ok(())
    }

    /// Returns the workspace-root trust status.
    ///
    /// # Errors
    ///
    /// - [`ConfigStateError::Store`] when the trust store cannot be read
    #[inline]
    pub(crate) fn workspace_trust_status(
        &self,
        subject: &TrustRequest,
    ) -> Result<WorkspaceTrustStatus, ConfigStateError> {
        if self.trusted.contains(subject.root_path())? {
            Ok(WorkspaceTrustStatus::Trusted)
        } else {
            Ok(WorkspaceTrustStatus::Untrusted)
        }
    }

    /// Returns the config-file trust status.
    ///
    /// # Errors
    ///
    /// - [`ConfigStateError::Store`] when the trust store cannot be read
    /// - [`ConfigStateError::Hash`] when the config file cannot be hashed
    #[inline]
    pub(crate) fn config_trust_status(
        &self,
        subject: &TrustRequest,
    ) -> Result<ConfigTrustStatus, ConfigStateError> {
        let Some(config_file) = subject.config_file() else {
            return Ok(if self.trusted.contains(subject.root_path())? {
                ConfigTrustStatus::Trusted
            } else {
                ConfigTrustStatus::Untrusted
            });
        };
        self.config_file_trust_check(subject.root_path(), config_file)
            .map(|check| check.status())
    }

    /// Checks config-file trust and returns content only when trusted.
    ///
    /// Requires both `root` and `config_path` directly. A root-only
    /// [`TrustRequest`] has no config path, while a trusted config-file result
    /// must always carry content.
    ///
    /// Reads content once into memory and hashes that buffer before returning
    /// it for parsing. A second independent read would open a TOCTOU window
    /// between the trust check and the file's actual use.
    ///
    /// # Errors
    ///
    /// - [`ConfigStateError::Store`] when the trust store cannot be read
    /// - [`ConfigStateError::Hash`] when the config file cannot be read
    pub(crate) fn config_file_trust_check(
        &self,
        root: &Path,
        config_path: &Path,
    ) -> Result<ConfigTrustCheck, ConfigStateError> {
        if !self.trusted.contains(root)? {
            return Ok(ConfigTrustCheck::Untrusted);
        }
        let Some(recorded) =
            self.trusted.read_companion(root, COMPANION_SUFFIX)?
        else {
            return Ok(ConfigTrustCheck::MissingBaseline);
        };
        let content = fs::read_to_string(config_path).map_err(|source| {
            ConfigStateError::Hash(HashError::Read {
                path: config_path.to_path_buf(),
                source,
            })
        })?;
        let current = Blake3FileHash::from(content.as_str());
        if recorded.trim() == current.to_string() {
            Ok(ConfigTrustCheck::Trusted(content))
        } else {
            Ok(ConfigTrustCheck::Stale)
        }
    }

    /// Revokes trust for a workspace and its config-hash companion.
    ///
    /// # Errors
    ///
    /// - [`ConfigStateError::Store`] when the trust entry cannot be removed
    #[inline]
    pub(crate) fn revoke_trust(
        &self,
        subject: &TrustRequest,
    ) -> Result<usize, ConfigStateError> {
        self.trusted
            .remove_with_companions(subject.root_path(), &[COMPANION_SUFFIX])
            .map_err(Into::into)
    }

    /// Lists live tracked config files.
    ///
    /// # Errors
    ///
    /// - [`ConfigStateError::Store`] when the tracked-config store cannot be
    ///   read
    #[inline]
    pub(crate) fn list_tracked_configs(
        &self,
    ) -> Result<Vec<PathBuf>, ConfigStateError> {
        self.tracked.list_all().map_err(Into::into)
    }

    /// Removes stale tracked config entries.
    ///
    /// # Errors
    ///
    /// - [`ConfigStateError::Store`] when stale entries cannot be cleaned
    #[inline]
    pub(crate) fn clean_tracked_configs(
        &self,
    ) -> Result<usize, ConfigStateError> {
        self.tracked.clean(FileStoreCleanMode::EntriesOnly).map_err(Into::into)
    }

    /// Lists trusted workspace roots.
    ///
    /// # Errors
    ///
    /// - [`ConfigStateError::Store`] when the trust store cannot be read
    #[inline]
    pub(crate) fn list_trusted_workspaces(
        &self,
    ) -> Result<Vec<PathBuf>, ConfigStateError> {
        self.trusted.list_all().map_err(Into::into)
    }

    /// Removes stale trusted-workspace entries and orphaned config-hash
    /// records.
    ///
    /// # Errors
    ///
    /// - [`ConfigStateError::Store`] when stale entries cannot be cleaned
    #[inline]
    pub(crate) fn clean_trusted_workspaces(
        &self,
    ) -> Result<usize, ConfigStateError> {
        self.trusted
            .clean(FileStoreCleanMode::WithCompanions(&[COMPANION_SUFFIX]))
            .map_err(Into::into)
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    struct Fixture {
        _temp: tempfile::TempDir,
        root: PathBuf,
        state: ConfigStateStore,
    }

    impl Fixture {
        fn new() -> Self {
            let temp = tempfile::tempdir().expect("create temp dir");
            // Canonicalize temp dir to resolve macOS /var -> /private/var
            // symlink
            #[expect(
                clippy::disallowed_methods,
                reason = "canonicalize is required here, not a shortcut: \
                          macOS temp dirs are under /var, a symlink to \
                          /private/var, and later path comparisons need the \
                          resolved form to match"
            )]
            let root = std::fs::canonicalize(temp.path()).unwrap();
            let state = ConfigStateStore::at(
                root.join("tracked"),
                root.join("trusted"),
            );
            Self {
                _temp: temp,
                root,
                state,
            }
        }

        fn project_root(&self) -> PathBuf {
            self.root.join("project")
        }

        fn write_config(&self, content: &str) -> LocalConfigFile<Discovered> {
            let path = self.project_root().join(".traces/config.toml");
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).expect("create config parent");
            }
            fs::write(&path, content).expect("write config");
            LocalConfigFile::try_new(path).expect("local config")
        }
    }

    mod track_seen_config {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn records_config_path_in_store() {
            // Arrange
            let fixture = Fixture::new();
            let config = fixture.write_config("");

            // Act
            fixture.state.track_seen_config(&config);

            // Assert
            let tracked = fixture.state.list_tracked_configs().expect("list");
            assert_eq!(tracked, vec![config.path().to_path_buf()]);
        }
    }

    mod grant_trust {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn records_workspace_without_companion_when_no_config_present() {
            // Arrange
            let fixture = Fixture::new();
            fs::create_dir_all(fixture.project_root()).expect("create root");
            let subject = TrustRequest::from(fixture.project_root().as_path());

            // Act
            fixture.state.grant_trust(&subject).expect("grant trust");

            // Assert
            let trusted =
                fixture.state.list_trusted_workspaces().expect("list");
            assert_eq!(trusted, vec![fixture.project_root()]);

            // Check no baseline was created by querying config status
            let config = fixture.write_config("content");
            let config_subject = TrustRequest::from(&config);
            let status = fixture
                .state
                .config_trust_status(&config_subject)
                .expect("status");
            assert_eq!(status, ConfigTrustStatus::MissingBaseline);
        }

        #[test]
        fn records_workspace_and_companion_when_config_present() {
            // Arrange
            let fixture = Fixture::new();
            let config = fixture.write_config("content");
            let subject = TrustRequest::from(&config);

            // Act
            fixture.state.grant_trust(&subject).expect("grant trust");

            // Assert
            let workspace_status = fixture
                .state
                .workspace_trust_status(&subject)
                .expect("workspace status");
            assert_eq!(workspace_status, WorkspaceTrustStatus::Trusted);

            let config_status = fixture
                .state
                .config_trust_status(&subject)
                .expect("config status");
            assert_eq!(config_status, ConfigTrustStatus::Trusted);
        }
    }

    mod workspace_trust_status {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn returns_untrusted_for_unknown_root() {
            // Arrange
            let fixture = Fixture::new();
            fs::create_dir_all(fixture.project_root()).expect("create root");
            let subject = TrustRequest::from(fixture.project_root().as_path());

            // Act
            let status =
                fixture.state.workspace_trust_status(&subject).expect("status");

            // Assert
            assert_eq!(status, WorkspaceTrustStatus::Untrusted);
        }

        #[test]
        fn returns_trusted_for_known_root() {
            // Arrange
            let fixture = Fixture::new();
            fs::create_dir_all(fixture.project_root()).expect("create root");
            let subject = TrustRequest::from(fixture.project_root().as_path());
            fixture.state.grant_trust(&subject).expect("grant trust");

            // Act
            let status =
                fixture.state.workspace_trust_status(&subject).expect("status");

            // Assert
            assert_eq!(status, WorkspaceTrustStatus::Trusted);
        }
    }

    mod config_trust_status {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn returns_untrusted_when_workspace_unknown() {
            // Arrange
            let fixture = Fixture::new();
            let config = fixture.write_config("content");
            let subject = TrustRequest::from(&config);

            // Act
            let status =
                fixture.state.config_trust_status(&subject).expect("status");

            // Assert
            assert_eq!(status, ConfigTrustStatus::Untrusted);
        }

        #[test]
        fn returns_trusted_when_workspace_trusted_but_no_config_requested() {
            // Arrange
            let fixture = Fixture::new();
            fs::create_dir_all(fixture.project_root()).expect("create root");
            let subject = TrustRequest::from(fixture.project_root().as_path());
            fixture.state.grant_trust(&subject).expect("grant trust");

            // Act
            let status =
                fixture.state.config_trust_status(&subject).expect("status");

            // Assert
            assert_eq!(status, ConfigTrustStatus::Trusted);
        }

        #[test]
        fn returns_missing_baseline_when_workspace_trusted_but_companion_missing()
         {
            // Arrange
            let fixture = Fixture::new();
            fs::create_dir_all(fixture.project_root()).expect("create root");
            let root_subject =
                TrustRequest::from(fixture.project_root().as_path());
            fixture.state.grant_trust(&root_subject).expect("grant trust");

            let config = fixture.write_config("content");
            let config_subject = TrustRequest::from(&config);

            // Act
            let status = fixture
                .state
                .config_trust_status(&config_subject)
                .expect("status");

            // Assert
            assert_eq!(status, ConfigTrustStatus::MissingBaseline);
        }

        #[test]
        fn returns_trusted_when_hash_matches_companion() {
            // Arrange
            let fixture = Fixture::new();
            let config = fixture.write_config("content");
            let subject = TrustRequest::from(&config);
            fixture.state.grant_trust(&subject).expect("grant trust");

            // Act
            let status =
                fixture.state.config_trust_status(&subject).expect("status");

            // Assert
            assert_eq!(status, ConfigTrustStatus::Trusted);
        }

        #[test]
        fn returns_stale_when_hash_differs_from_companion() {
            // Arrange
            let fixture = Fixture::new();
            let config = fixture.write_config("original");
            let subject = TrustRequest::from(&config);
            fixture.state.grant_trust(&subject).expect("grant trust");
            fs::write(config.path(), "changed").expect("rewrite");

            // Act
            let status =
                fixture.state.config_trust_status(&subject).expect("status");

            // Assert
            assert_eq!(status, ConfigTrustStatus::Stale);
        }
    }

    mod revoke_trust {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn removes_workspace_from_trusted_store() {
            // Arrange
            let fixture = Fixture::new();
            let config = fixture.write_config("content");
            let subject = TrustRequest::from(&config);
            fixture.state.grant_trust(&subject).expect("grant trust");

            // Act
            fixture.state.revoke_trust(&subject).expect("revoke trust");

            // Assert
            let status =
                fixture.state.workspace_trust_status(&subject).expect("status");
            assert_eq!(status, WorkspaceTrustStatus::Untrusted);
        }

        #[test]
        fn removes_companion_hash_file() {
            // Arrange
            let fixture = Fixture::new();
            let config = fixture.write_config("content");
            let subject = TrustRequest::from(&config);
            fixture.state.grant_trust(&subject).expect("grant trust");

            // Act
            fixture.state.revoke_trust(&subject).expect("revoke trust");

            // Assert: Verify companion is gone by re-granting root trust and
            // checking config trust
            let root_subject =
                TrustRequest::from(fixture.project_root().as_path());
            fixture.state.grant_trust(&root_subject).expect("grant trust");

            let status =
                fixture.state.config_trust_status(&subject).expect("status");
            assert_eq!(status, ConfigTrustStatus::MissingBaseline);
        }
    }

    mod tracked_configs {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn lists_tracked_config_paths() {
            // Arrange
            let fixture = Fixture::new();
            let config = fixture.write_config("content");
            fixture.state.track_seen_config(&config);

            // Act
            let tracked = fixture.state.list_tracked_configs().expect("list");

            // Assert
            assert_eq!(tracked, vec![config.path().to_path_buf()]);
        }

        #[test]
        fn cleans_stale_tracked_config_paths() {
            // Arrange
            let fixture = Fixture::new();
            let config = fixture.write_config("content");
            fixture.state.track_seen_config(&config);
            fs::remove_file(config.path()).expect("remove file");

            // Act
            let removed =
                fixture.state.clean_tracked_configs().expect("clean configs");

            // Assert
            assert_eq!(removed, 1);
            let tracked = fixture.state.list_tracked_configs().expect("list");
            assert!(tracked.is_empty());
        }
    }

    mod trusted_workspaces {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn lists_trusted_workspace_roots() {
            // Arrange
            let fixture = Fixture::new();
            fs::create_dir_all(fixture.project_root()).expect("create root");
            let subject = TrustRequest::from(fixture.project_root().as_path());
            fixture.state.grant_trust(&subject).expect("grant trust");

            // Act
            let trusted =
                fixture.state.list_trusted_workspaces().expect("list");

            // Assert
            assert_eq!(trusted, vec![fixture.project_root()]);
        }

        #[test]
        fn cleans_stale_trusted_workspace_roots_and_companions() {
            // Arrange
            let fixture = Fixture::new();
            let config = fixture.write_config("content");
            let subject = TrustRequest::from(&config);
            fixture.state.grant_trust(&subject).expect("grant trust");
            fs::remove_dir_all(fixture.project_root()).expect("remove root");

            // Act
            let removed = fixture
                .state
                .clean_trusted_workspaces()
                .expect("clean workspaces");

            // Assert
            assert_eq!(removed, 1);
            let trusted =
                fixture.state.list_trusted_workspaces().expect("list");
            assert!(trusted.is_empty());
        }
    }
}
