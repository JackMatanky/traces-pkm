//! Unified config tracking and trust state.

use std::path::{Path, PathBuf};

use thiserror::Error;

use super::file::{ConfigFile, Discovered, Tracked};
use crate::{
    Blake3FileHash, FileStateStore, FileStateStoreError, FileStoreCleanMode,
    dirs, hash::HashError,
};

const COMPANION_SUFFIX: &str = ".hash";

/// Stores seen config files and trusted workspace roots.
#[derive(Clone, Debug)]
pub(crate) struct ConfigStateStore {
    tracked: FileStateStore,
    trusted: FileStateStore,
}

/// Subject of a trust operation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct TrustSubject {
    kind: TrustSubjectKind,
    path: PathBuf,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum TrustSubjectKind {
    Root,
    Config,
}

/// Trust subjects resolved from a discovery operation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct TrustSubjects(Box<[TrustSubject]>);

impl TrustSubjects {
    /// Creates trust subjects from discovered configs.
    #[inline]
    #[must_use]
    pub(crate) fn new(subjects: Vec<TrustSubject>) -> Self {
        Self(subjects.into_boxed_slice())
    }

    /// Creates a single trust subject.
    #[inline]
    #[must_use]
    pub(crate) fn single(subject: TrustSubject) -> Self {
        Self(Box::new([subject]))
    }
}

impl IntoIterator for TrustSubjects {
    type IntoIter = std::vec::IntoIter<TrustSubject>;
    type Item = TrustSubject;

    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        self.0.into_vec().into_iter()
    }
}

impl TrustSubject {
    /// Trust a workspace root without binding trust to a config-file hash.
    #[inline]
    #[must_use]
    pub(crate) fn root(root: &Path) -> Self {
        Self {
            kind: TrustSubjectKind::Root,
            path: root.to_path_buf(),
        }
    }

    /// Trust a discovered local config and bind it to its current content hash.
    #[inline]
    #[must_use]
    pub(crate) fn discovered(file: &ConfigFile<Discovered>) -> Self {
        Self {
            kind: TrustSubjectKind::Config,
            path: file.path().to_path_buf(),
        }
    }

    /// Trust a tracked local config and bind it to its current content hash.
    #[inline]
    #[must_use]
    pub(crate) fn tracked(file: &ConfigFile<Tracked>) -> Self {
        Self {
            kind: TrustSubjectKind::Config,
            path: file.path().to_path_buf(),
        }
    }

    /// The workspace root this subject refers to.
    #[inline]
    #[must_use]
    pub(crate) fn root_path(&self) -> &Path {
        match self.kind {
            TrustSubjectKind::Root => &self.path,
            TrustSubjectKind::Config => {
                self.path.parent().and_then(Path::parent).unwrap_or(&self.path)
            }
        }
    }

    /// The config file if present, otherwise the workspace root.
    #[inline]
    #[must_use]
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    /// The config file path, when this subject carries one.
    #[inline]
    #[must_use]
    pub(crate) fn config_file(&self) -> Option<&Path> {
        match self.kind {
            TrustSubjectKind::Root => None,
            TrustSubjectKind::Config => Some(&self.path),
        }
    }
}

/// Trust state for a workspace root.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum WorkspaceTrustStatus {
    /// The workspace root is trusted.
    Trusted,
    /// The workspace root is not trusted.
    Untrusted,
}

/// Trust state for a config file inside a workspace.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum ConfigTrustStatus {
    /// The workspace is trusted and the config hash still matches.
    Trusted,
    /// The workspace root is not trusted.
    Untrusted,
    /// The workspace is trusted, but no config hash is recorded.
    MissingBaseline,
    /// The workspace is trusted, but the config hash no longer matches.
    Stale,
}

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

impl ConfigStateStore {
    /// Creates the production state store at the platform state-dir roots.
    #[inline]
    #[must_use]
    pub(crate) fn new() -> Self {
        Self {
            tracked: FileStateStore::new((*dirs::TRACKED_CONFIGS).clone()),
            trusted: FileStateStore::new((*dirs::TRUSTED_CONFIGS).clone()),
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
    pub(crate) fn track_seen_config(&self, config: &ConfigFile<Discovered>) {
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
    /// Returns [`ConfigStateError`] when trust cannot be recorded or the config
    /// file cannot be hashed.
    #[inline]
    pub(crate) fn grant_trust(
        &self,
        subject: &TrustSubject,
    ) -> Result<(), ConfigStateError> {
        self.trusted.record(subject.root_path())?;
        let Some(config_file) = subject.config_file() else {
            return Ok(());
        };
        let digest = Blake3FileHash::new(config_file)?;
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
    /// Returns [`ConfigStateError`] when the trust store cannot be queried.
    #[inline]
    pub(crate) fn workspace_trust_status(
        &self,
        subject: &TrustSubject,
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
    /// Returns [`ConfigStateError`] when the trust store cannot be queried or
    /// the config file cannot be hashed.
    #[inline]
    pub(crate) fn config_trust_status(
        &self,
        subject: &TrustSubject,
    ) -> Result<ConfigTrustStatus, ConfigStateError> {
        if !self.trusted.contains(subject.root_path())? {
            return Ok(ConfigTrustStatus::Untrusted);
        }
        let Some(config_file) = subject.config_file() else {
            return Ok(ConfigTrustStatus::Trusted);
        };
        let Some(recorded) = self
            .trusted
            .read_companion(subject.root_path(), COMPANION_SUFFIX)?
        else {
            return Ok(ConfigTrustStatus::MissingBaseline);
        };
        let current = Blake3FileHash::new(config_file)?;
        if recorded.trim() == current.to_string() {
            Ok(ConfigTrustStatus::Trusted)
        } else {
            Ok(ConfigTrustStatus::Stale)
        }
    }

    /// Revokes trust for a workspace and its config-hash companion.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigStateError`] when the trust entry cannot be removed.
    #[inline]
    pub(crate) fn revoke_trust(
        &self,
        subject: &TrustSubject,
    ) -> Result<usize, ConfigStateError> {
        self.trusted
            .remove_with_companions(subject.root_path(), &[COMPANION_SUFFIX])
            .map_err(Into::into)
    }

    /// Lists live tracked config files.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigStateError`] when the tracked-config store cannot be
    /// read.
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
    /// Returns [`ConfigStateError`] when stale entries cannot be cleaned.
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
    /// Returns [`ConfigStateError`] when the trust store cannot be read.
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
    /// Returns [`ConfigStateError`] when stale entries cannot be cleaned.
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

    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn config_trust_detects_stale_content() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let root = temp.path().join("project");
        let path = root.join(".traces/config.toml");
        fs::create_dir_all(path.parent().expect("config parent"))
            .expect("create config parent");
        fs::write(&path, "[templates]\noutput_dir = \"notes\"")
            .expect("write config");
        let file = ConfigFile::<Discovered>::local(path.clone())
            .expect("local config");
        let state = ConfigStateStore::at(
            temp.path().join("tracked"),
            temp.path().join("trusted"),
        );
        let subject = TrustSubject::discovered(&file);

        state.grant_trust(&subject).expect("grant trust");
        assert_eq!(
            state.config_trust_status(&subject).expect("check trust"),
            ConfigTrustStatus::Trusted
        );

        fs::write(&path, "[templates]\noutput_dir = \"changed\"")
            .expect("rewrite config");

        assert_eq!(
            state.config_trust_status(&subject).expect("check stale trust"),
            ConfigTrustStatus::Stale
        );
    }

    #[test]
    fn root_trust_has_workspace_status() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let root = temp.path().join("project");
        fs::create_dir_all(&root).expect("create root");
        let state = ConfigStateStore::at(
            temp.path().join("tracked"),
            temp.path().join("trusted"),
        );
        let subject = TrustSubject::root(&root);

        assert_eq!(
            state.workspace_trust_status(&subject).expect("check untrusted"),
            WorkspaceTrustStatus::Untrusted
        );

        state.grant_trust(&subject).expect("grant trust");

        assert_eq!(
            state.workspace_trust_status(&subject).expect("check trusted"),
            WorkspaceTrustStatus::Trusted
        );
    }
}
