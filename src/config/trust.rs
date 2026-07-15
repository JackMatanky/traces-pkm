//! Trust adapter: records and checks trusted project roots, plus a
//! content-hash companion for the config file within a trusted root, so an
//! edit to that file after trust was granted is detected rather than
//! silently accepted forever.
//!
//! Global config trust is skipped entirely by the caller (see
//! `super::builder::ConfigBuilder::trust`) — this type is only ever
//! exercised for local candidates, anchored at the project root
//! (`candidate.root()`), not the config file's own path or its parent
//! directory. Unlike tracking, trust propagates errors instead of
//! swallowing them (see [`ConfigTrust::trust`]'s doc for why).

use std::{
    fs, io,
    path::{Path, PathBuf},
};

use thiserror::Error;

use super::{
    dirs,
    store::{ConfigFileStore, StoreError},
};
use crate::hash::{self, HashError};

/// Errors from a [`ConfigTrust`] operation that couldn't be completed.
///
/// Distinct from `TrustState::Untrusted`/`TrustState::Stale` (see
/// [`TrustState`]), which are expected, actionable *outcomes* of a
/// successful check, not failures — this type means the check (or the
/// write) itself didn't complete. `thiserror`-only, no
/// `miette::Diagnostic`: internal plumbing, always wrapped by
/// [`super::builder::ConfigBuilderError::TrustCheckFailed`] before it
/// reaches anything CLI-facing.
///
/// `pub` (not `pub(super)`) for the same reason as [`StoreError`]:
/// [`super::builder::ConfigBuilderError::TrustCheckFailed`] carries it as
/// a `#[source]` field, and [`super::service::ConfigService::trust`]/
/// [`super::service::ConfigService::is_trusted`] return it directly.
#[derive(Debug, Error)]
pub enum TrustError {
    /// The underlying path-hash trust store operation failed.
    #[error(transparent)]
    Store(#[from] StoreError),
    /// Hashing the config file's current content failed.
    #[error(transparent)]
    Hash(#[from] HashError),
    /// The content-hash companion record could not be written.
    #[error("failed to write the content-hash record at {path}")]
    CompanionWrite {
        /// Companion file path.
        path: PathBuf,
        /// Source I/O error.
        #[source]
        source: io::Error,
    },
}

/// Result of checking a local config candidate's trust.
///
/// Distinguishes "never trusted" from "trusted, but the config file's
/// content changed since" — a plain boolean can't tell these apart, and a
/// pure path-hash trust entry never expires on its own, so [`Self::Stale`]
/// is the signal that closes that gap. Global candidates never produce
/// this (global trust is skipped entirely; see `super::builder`'s
/// `ConfigBuilder::trust`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TrustState {
    /// No trust entry exists for this candidate's project root.
    Untrusted,
    /// A trust entry exists for the project root, but the config file's
    /// current content hash doesn't match the one recorded at trust time
    /// (or no content hash was ever recorded).
    Stale,
    /// A trust entry exists and the config file's content matches what was
    /// recorded at trust time.
    Trusted,
}

/// Records and checks the trusted-project-root store, plus a content-hash
/// companion per entry.
///
/// Thin adapter over [`ConfigFileStore`], fixing its root to the
/// OS-correct trust-store location ([`dirs::TRUSTED_CONFIGS`]).
#[derive(Clone, Debug)]
pub(super) struct ConfigTrust {
    store: ConfigFileStore,
}

impl ConfigTrust {
    /// Creates the production trust store, rooted at the OS-correct
    /// trusted-configs location under the state dir.
    #[inline]
    #[must_use]
    pub(super) fn new() -> Self {
        Self {
            store: ConfigFileStore::new(&dirs::TRUSTED_CONFIGS),
        }
    }

    /// Creates a trust store rooted at an explicit path. Test-only:
    /// production callers always want the OS-correct root from
    /// [`Self::new`].
    #[cfg(test)]
    #[must_use]
    pub(super) fn at(root: PathBuf) -> Self {
        Self {
            store: ConfigFileStore::at(root),
        }
    }

    /// Marks `root`'s canonical path as trusted, and records
    /// `config_file`'s current content hash as the baseline
    /// [`TrustState::Stale`] re-verification compares against.
    ///
    /// Idempotent on the root entry (recording an already-trusted root is
    /// a no-op there); re-running this after `config_file` changes
    /// refreshes the recorded content hash, clearing any prior staleness.
    /// Unlike [`super::tracker::ConfigTracker::track`], this propagates
    /// failures rather than logging and swallowing them: trust is a
    /// security decision, and a trust write that silently fails would
    /// leave the caller believing something is trusted when it isn't
    /// recorded at all.
    ///
    /// # Errors
    ///
    /// Returns [`TrustError`] when `root` cannot be canonicalized or
    /// recorded, `config_file` cannot be hashed, or the content-hash
    /// companion record cannot be written.
    #[inline]
    pub(super) fn trust(
        &self,
        root: &Path,
        config_file: &Path,
    ) -> Result<(), TrustError> {
        self.store.record(root)?;
        let digest = hash::hash_file_contents(config_file)?;
        let companion = companion_path(&self.store.entry_path(root)?);
        fs::write(&companion, digest.to_string()).map_err(|source| {
            TrustError::CompanionWrite {
                path: companion,
                source,
            }
        })
    }

    /// Checks whether `root` is trusted and, if so, whether
    /// `config_file`'s current content still matches what [`Self::trust`]
    /// recorded.
    ///
    /// A missing or unreadable content-hash companion — including one
    /// belonging to a root trusted before this check existed — is treated
    /// as [`TrustState::Stale`], not silently [`TrustState::Trusted`]:
    /// failing toward re-verification rather than assuming safety.
    ///
    /// # Errors
    ///
    /// Returns [`TrustError`] when the root's trust entry cannot be read,
    /// or `config_file` cannot be hashed. A missing/unreadable companion
    /// record is `Ok(TrustState::Stale)`, not an error — only the trust
    /// *check* failing (not finding a definitive answer) is an error.
    #[inline]
    pub(super) fn is_trusted(
        &self,
        root: &Path,
        config_file: &Path,
    ) -> Result<TrustState, TrustError> {
        if !self.store.contains(root)? {
            return Ok(TrustState::Untrusted);
        }
        let companion = companion_path(&self.store.entry_path(root)?);
        let Ok(recorded) = fs::read_to_string(&companion) else {
            return Ok(TrustState::Stale);
        };
        let current = hash::hash_file_contents(config_file)?;
        Ok(if recorded.trim() == current.to_string() {
            TrustState::Trusted
        } else {
            TrustState::Stale
        })
    }
}

/// The content-hash companion path for a trust `entry`: alongside it, with
/// a `.hash` suffix appended (not [`Path::with_extension`], since `entry`
/// is a bare hex filename with no existing extension to replace — either
/// would work here, but appending makes that safe-either-way property
/// explicit rather than relying on it).
fn companion_path(entry: &Path) -> std::path::PathBuf {
    let mut name = entry.as_os_str().to_owned();
    name.push(".hash");
    std::path::PathBuf::from(name)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn is_trusted_returns_untrusted_for_an_unrecorded_root() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let root = temp.path().join("project");
        fs::create_dir_all(&root).expect("create project dir");
        let config_file = root.join("config.toml");
        fs::write(&config_file, "a = 1").expect("write config");
        let trust = ConfigTrust::at(temp.path().join("trust-store"));

        assert_eq!(
            trust.is_trusted(&root, &config_file).expect("check trust"),
            TrustState::Untrusted
        );
    }

    #[test]
    fn trust_then_is_trusted_returns_trusted() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let root = temp.path().join("project");
        fs::create_dir_all(&root).expect("create project dir");
        let config_file = root.join("config.toml");
        fs::write(&config_file, "a = 1").expect("write config");
        let trust = ConfigTrust::at(temp.path().join("trust-store"));

        trust.trust(&root, &config_file).expect("trust root");

        assert_eq!(
            trust.is_trusted(&root, &config_file).expect("check trust"),
            TrustState::Trusted
        );
    }

    #[test]
    fn trust_is_idempotent_on_the_root_entry() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let root = temp.path().join("project");
        fs::create_dir_all(&root).expect("create project dir");
        let config_file = root.join("config.toml");
        fs::write(&config_file, "a = 1").expect("write config");
        let trust = ConfigTrust::at(temp.path().join("trust-store"));

        trust.trust(&root, &config_file).expect("trust root");
        trust.trust(&root, &config_file).expect("trust root again");

        assert_eq!(
            trust.is_trusted(&root, &config_file).expect("check trust"),
            TrustState::Trusted
        );
    }

    #[test]
    fn editing_the_config_file_after_trust_makes_it_stale() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let root = temp.path().join("project");
        fs::create_dir_all(&root).expect("create project dir");
        let config_file = root.join("config.toml");
        fs::write(&config_file, "a = 1").expect("write config");
        let trust = ConfigTrust::at(temp.path().join("trust-store"));
        trust.trust(&root, &config_file).expect("trust root");

        fs::write(&config_file, "a = 2").expect("edit config");

        assert_eq!(
            trust.is_trusted(&root, &config_file).expect("check trust"),
            TrustState::Stale
        );
    }

    #[test]
    fn re_trusting_after_an_edit_clears_staleness() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let root = temp.path().join("project");
        fs::create_dir_all(&root).expect("create project dir");
        let config_file = root.join("config.toml");
        fs::write(&config_file, "a = 1").expect("write config");
        let trust = ConfigTrust::at(temp.path().join("trust-store"));
        trust.trust(&root, &config_file).expect("trust root");
        fs::write(&config_file, "a = 2").expect("edit config");

        trust.trust(&root, &config_file).expect("re-trust root");

        assert_eq!(
            trust.is_trusted(&root, &config_file).expect("check trust"),
            TrustState::Trusted
        );
    }

    #[test]
    fn a_root_trusted_without_a_companion_hash_is_stale() {
        // Simulates a trust entry written before content-hash
        // re-verification existed: the root-level entry exists, but no
        // `.hash` companion was ever recorded.
        let temp = tempfile::tempdir().expect("create temp dir");
        let root = temp.path().join("project");
        fs::create_dir_all(&root).expect("create project dir");
        let config_file = root.join("config.toml");
        fs::write(&config_file, "a = 1").expect("write config");
        let trust_store_root = temp.path().join("trust-store");
        let store = ConfigFileStore::at(trust_store_root.clone());
        store.record(&root).expect("record root directly, bypassing trust()");
        let trust = ConfigTrust::at(trust_store_root);

        assert_eq!(
            trust.is_trusted(&root, &config_file).expect("check trust"),
            TrustState::Stale
        );
    }

    #[test]
    fn a_corrupted_companion_hash_is_stale_not_an_error() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let root = temp.path().join("project");
        fs::create_dir_all(&root).expect("create project dir");
        let config_file = root.join("config.toml");
        fs::write(&config_file, "a = 1").expect("write config");
        let trust_store_root = temp.path().join("trust-store");
        let store = ConfigFileStore::at(trust_store_root.clone());
        let trust = ConfigTrust::at(trust_store_root);
        trust.trust(&root, &config_file).expect("trust root");
        let companion =
            companion_path(&store.entry_path(&root).expect("entry path"));
        fs::write(&companion, "not a valid blake3 hash")
            .expect("corrupt the companion hash file");

        assert_eq!(
            trust.is_trusted(&root, &config_file).expect("check trust"),
            TrustState::Stale
        );
    }

    #[test]
    fn trust_of_a_nonexistent_root_errors() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let config_file = temp.path().join("config.toml");
        fs::write(&config_file, "a = 1").expect("write config");
        let trust = ConfigTrust::at(temp.path().join("trust-store"));

        assert!(matches!(
            trust.trust(&temp.path().join("missing-root"), &config_file),
            Err(TrustError::Store(StoreError::Canonicalize { .. }))
        ));
    }

    #[test]
    fn trust_of_a_nonexistent_config_file_errors() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let root = temp.path().join("project");
        fs::create_dir_all(&root).expect("create project dir");
        let trust = ConfigTrust::at(temp.path().join("trust-store"));

        assert!(matches!(
            trust.trust(&root, &root.join("missing-config.toml")),
            Err(TrustError::Hash(HashError::Read { .. }))
        ));
    }

    #[test]
    fn trust_propagates_a_store_write_failure_instead_of_swallowing_it() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let root = temp.path().join("project");
        fs::create_dir_all(&root).expect("create project dir");
        let config_file = root.join("config.toml");
        fs::write(&config_file, "a = 1").expect("write config");
        let trust_store_root = temp.path().join("trust-store");
        fs::write(&trust_store_root, "")
            .expect("occupy trust store root with a file");
        let trust = ConfigTrust::at(trust_store_root);

        assert!(matches!(
            trust.trust(&root, &config_file),
            Err(TrustError::Store(StoreError::StoreIo { .. }))
        ));
    }
}
