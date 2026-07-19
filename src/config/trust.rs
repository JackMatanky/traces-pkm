//! Trust adapter: records and checks trusted project roots, plus a
//! content-hash companion for the config file within a trusted root, so an
//! edit to that file after trust was granted is detected rather than
//! silently accepted forever.
//!
//! Global config trust is skipped entirely by the caller (see
//! `super::builder`). This type is only exercised for local config files,
//! anchored at their project root, not at the config file's own path or its
//! parent `.traces` directory. Unlike tracking, trust propagates errors instead
//! of swallowing them (see [`ConfigTrust::trust`]'s doc for why).

use std::{
    fs, io,
    path::{Path, PathBuf},
};

use thiserror::Error;

use super::{
    dirs,
    file::{ConfigFile, ConfigFileError, Discovered, Tracked},
    store::{ConfigFileStore, StoreError},
};
use crate::{Blake3FileHash, hash::HashError};

/// Errors from a [`ConfigTrust`] operation that couldn't be completed.
///
/// Distinct from `TrustState::Untrusted`/`TrustState::Stale` (see
/// [`TrustState`]), which are expected, actionable *outcomes* of a
/// successful check, not failures — this type means the check (or the
/// write) itself didn't complete. `thiserror`-only, no
/// `miette::Diagnostic`: internal plumbing. Config-file trust transitions
/// wrap this as [`super::file::ConfigFileTrustError::TrustCheckFailed`],
/// then as [`super::file::ConfigFileError::Trust`].
///
/// `pub(crate)` (not `pub(super)`) for the same reason as [`StoreError`]:
/// config-file trust errors carry it as a `#[source]` field, and
/// [`super::service::ConfigService::trust`]/
/// [`super::service::ConfigService::is_trusted`] return it directly.
#[derive(Debug, Error)]
pub(crate) enum TrustError {
    /// The underlying path-hash trust store operation failed.
    #[error(transparent)]
    Store(#[from] StoreError),
    /// Hashing the config file's current content failed.
    #[error(transparent)]
    Hash(#[from] HashError),
    /// Config-file path validation failed.
    #[error(transparent)]
    ConfigFile(#[from] ConfigFileError),
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
pub(crate) enum TrustState {
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

impl TrustState {
    /// Stable lowercase label used by CLI status output.
    #[inline]
    #[must_use]
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Untrusted => "untrusted",
            Self::Stale => "stale",
            Self::Trusted => "trusted",
        }
    }
}

/// What [`ConfigTrust::trust`] is being asked to trust.
///
/// Replaces two positional `&Path` parameters that were easy to pass in
/// the wrong order. Discovery resolves user input before this type is
/// constructed; trust only records concrete local config files.
#[derive(Clone, Copy, Debug)]
pub(crate) enum TrustTarget<'a> {
    /// Trust `root` alone. No config file exists yet to content-hash.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "covered by trust-store tests; no production route \
                      creates bare roots yet"
        )
    )]
    Directory(&'a Path),
    /// Resolve and trust this local `.traces/config.toml` file.
    File(&'a Path),
    /// Trust an already tracked local config file.
    ConfigFile(&'a ConfigFile<Tracked>),
}

impl<'a> From<&'a ConfigFile<Tracked>> for TrustTarget<'a> {
    #[inline]
    fn from(file: &'a ConfigFile<Tracked>) -> Self {
        Self::ConfigFile(file)
    }
}

/// The `.hash` companion file's suffix, appended to a trust entry's
/// filename via [`ConfigFileStore::companion_path`].
const COMPANION_SUFFIX: &str = ".hash";

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

    /// Marks `target`'s workspace root as trusted and records its config
    /// file's current content hash as the baseline
    /// [`TrustState::Stale`] re-verification compares against.
    ///
    /// Idempotent on the root entry (recording an already-trusted root is
    /// a no-op there); re-running this after the config file changes
    /// refreshes the recorded content hash, clearing any prior staleness.
    /// Unlike [`super::tracker::ConfigTracker::track`], this propagates
    /// failures rather than logging and swallowing them: trust is a
    /// security decision, and a trust write that silently fails would
    /// leave the caller believing something is trusted when it isn't
    /// recorded at all.
    ///
    /// # Errors
    ///
    /// Returns [`TrustError`] when the root cannot be canonicalized or
    /// recorded, the config file cannot be hashed, or the content-hash
    /// companion record cannot be written.
    #[inline]
    pub(super) fn trust(
        &self,
        target: TrustTarget<'_>,
    ) -> Result<(), TrustError> {
        let (root, config_file) = target_paths(target)?;
        self.store.record(&root)?;
        let Some(config_file) = config_file else {
            return Ok(());
        };
        let companion = companion_path(&self.store.entry_path(&root)?);
        let digest = Blake3FileHash::new(&config_file)?;
        fs::write(&companion, digest.to_string()).map_err(|source| {
            TrustError::CompanionWrite {
                path: companion,
                source,
            }
        })
    }

    /// Removes `root`'s trust entry and its content-hash companion if present.
    /// Returns the number of root entries removed.
    ///
    /// # Errors
    ///
    /// Returns [`TrustError`] when `root` cannot be canonicalized or a store
    /// removal fails.
    #[inline]
    pub(super) fn untrust(&self, root: &Path) -> Result<usize, TrustError> {
        self.store
            .remove_with_companion(root, COMPANION_SUFFIX)
            .map_err(Into::into)
    }

    /// Lists the canonical paths of all currently trusted roots.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when the store directory exists but cannot
    /// be read.
    #[inline]
    pub(super) fn list_all(&self) -> Result<Vec<PathBuf>, StoreError> {
        self.store.list_all()
    }

    /// Removes dangling root entries (target directory deleted or moved),
    /// plus each removed entry's content-hash companion, if one exists.
    /// Returns the number of root entries removed — a companion isn't
    /// counted separately, since it's a 1:1 accessory to its root entry,
    /// not a user-visible unit.
    ///
    /// Delegates entirely to [`ConfigFileStore::clean_with_companion`],
    /// which owns both "regular cleaning" and "cleaning with a companion
    /// suffix" as one parameterised mechanism — this method only supplies
    /// which suffix means "companion" for trust specifically.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when the store directory exists but cannot
    /// be read, a stale root entry cannot be removed, or an existing
    /// companion cannot be removed.
    #[inline]
    pub(super) fn clean(&self) -> Result<usize, StoreError> {
        self.store.clean_with_companion(COMPANION_SUFFIX)
    }

    /// Checks whether `target` is trusted and, for config-file targets, whether
    /// the current content still matches what [`Self::trust`] recorded.
    ///
    /// A missing or unreadable content-hash companion — including one
    /// belonging to a root trusted before this check existed — is treated
    /// as [`TrustState::Stale`] for config-file targets, not silently
    /// [`TrustState::Trusted`]: failing toward re-verification rather than
    /// assuming safety. [`TrustTarget::Directory`] checks only the bare root
    /// entry because no config file exists to hash.
    ///
    /// # Errors
    ///
    /// Returns [`TrustError`] when the root's trust entry cannot be read,
    /// the target path is not a local config file, or config-file content
    /// cannot be hashed. A missing/unreadable companion record is
    /// `Ok(TrustState::Stale)`, not an error — only the trust *check*
    /// failing (not finding a definitive answer) is an error.
    #[inline]
    pub(super) fn is_trusted(
        &self,
        target: TrustTarget<'_>,
    ) -> Result<TrustState, TrustError> {
        let (root, config_file) = target_paths(target)?;
        if !self.store.contains(&root)? {
            return Ok(TrustState::Untrusted);
        }
        let Some(config_file) = config_file else {
            return Ok(TrustState::Trusted);
        };
        let companion = companion_path(&self.store.entry_path(&root)?);
        let Ok(recorded) = fs::read_to_string(&companion) else {
            return Ok(TrustState::Stale);
        };
        let current = Blake3FileHash::new(&config_file)?;
        Ok(if recorded.trim() == current.to_string() {
            TrustState::Trusted
        } else {
            TrustState::Stale
        })
    }
}

fn target_paths(
    target: TrustTarget<'_>,
) -> Result<(PathBuf, Option<PathBuf>), TrustError> {
    match target {
        TrustTarget::Directory(root) => Ok((root.to_path_buf(), None)),
        TrustTarget::File(path) => {
            let file = ConfigFile::<Discovered>::local(path.to_path_buf())?;
            Ok((file.root().to_path_buf(), Some(file.path().to_path_buf())))
        }
        TrustTarget::ConfigFile(file) => {
            Ok((file.root().to_path_buf(), Some(file.path().to_path_buf())))
        }
    }
}

/// The content-hash companion path for a trust `entry`. Thin wrapper
/// naming the concrete suffix over [`ConfigFileStore::companion_path`],
/// which owns the actual path-building formula — shared with
/// [`ConfigFileStore::clean_with_companion`] so the two can't drift apart.
fn companion_path(entry: &Path) -> PathBuf {
    ConfigFileStore::companion_path(entry, COMPANION_SUFFIX)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use pretty_assertions::assert_eq;

    use super::*;
    use crate::config::LOCAL_CONFIG_FILE;

    /// Shorthand for the common case: trusting `root` together with an
    /// existing `config_file`.
    fn config_file_target<'a>(
        _root: &'a Path,
        config_file: &'a Path,
    ) -> TrustTarget<'a> {
        TrustTarget::File(config_file)
    }

    #[test]
    fn trust_target_from_tracked_config_file_is_config_file() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let root = temp.path().join("project");
        let config_file = root.join(".traces/config.toml");
        fs::create_dir_all(config_file.parent().expect("config parent"))
            .expect("create config parent");
        fs::write(&config_file, "").expect("write config");
        let discovered = ConfigFile::<Discovered>::local(config_file.clone())
            .expect("valid local config");
        let tracker = crate::config::tracker::ConfigTracker::at(
            temp.path().join("tracked-store"),
        );
        let tracked = ConfigFile::<Tracked>::from((discovered, &tracker));

        let target = TrustTarget::from(&tracked);

        assert!(matches!(
            target,
            TrustTarget::ConfigFile(file) if file.root() == root && file.path() == config_file
        ));
    }

    #[test]
    fn is_trusted_returns_untrusted_for_an_unrecorded_root() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let root = temp.path().join("project");
        fs::create_dir_all(&root).expect("create project dir");
        let config_file = root.join(".traces/config.toml");
        fs::create_dir_all(config_file.parent().expect("config parent"))
            .expect("create config parent");
        fs::write(&config_file, "a = 1").expect("write config");
        let trust = ConfigTrust::at(temp.path().join("trust-store"));

        assert_eq!(
            trust
                .is_trusted(config_file_target(&root, &config_file))
                .expect("check trust"),
            TrustState::Untrusted
        );
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
        let trust = ConfigTrust::at(temp.path().join("trust-store"));

        trust
            .trust(config_file_target(&root, &config_file))
            .expect("trust root");

        assert_eq!(
            trust
                .is_trusted(config_file_target(&root, &config_file))
                .expect("check trust"),
            TrustState::Trusted
        );
    }

    #[test]
    fn trust_is_idempotent_on_the_root_entry() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let root = temp.path().join("project");
        fs::create_dir_all(&root).expect("create project dir");
        let config_file = root.join(".traces/config.toml");
        fs::create_dir_all(config_file.parent().expect("config parent"))
            .expect("create config parent");
        fs::write(&config_file, "a = 1").expect("write config");
        let trust = ConfigTrust::at(temp.path().join("trust-store"));

        trust
            .trust(config_file_target(&root, &config_file))
            .expect("trust root");
        trust
            .trust(config_file_target(&root, &config_file))
            .expect("trust root again");

        assert_eq!(
            trust
                .is_trusted(config_file_target(&root, &config_file))
                .expect("check trust"),
            TrustState::Trusted
        );
    }

    #[test]
    fn editing_the_config_file_after_trust_makes_it_stale() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let root = temp.path().join("project");
        fs::create_dir_all(&root).expect("create project dir");
        let config_file = root.join(".traces/config.toml");
        fs::create_dir_all(config_file.parent().expect("config parent"))
            .expect("create config parent");
        fs::write(&config_file, "a = 1").expect("write config");
        let trust = ConfigTrust::at(temp.path().join("trust-store"));
        trust
            .trust(config_file_target(&root, &config_file))
            .expect("trust root");

        fs::create_dir_all(config_file.parent().expect("config parent"))
            .expect("create config parent");
        fs::write(&config_file, "a = 2").expect("edit config");

        assert_eq!(
            trust
                .is_trusted(config_file_target(&root, &config_file))
                .expect("check trust"),
            TrustState::Stale
        );
    }

    #[test]
    fn re_trusting_after_an_edit_clears_staleness() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let root = temp.path().join("project");
        fs::create_dir_all(&root).expect("create project dir");
        let config_file = root.join(".traces/config.toml");
        fs::create_dir_all(config_file.parent().expect("config parent"))
            .expect("create config parent");
        fs::write(&config_file, "a = 1").expect("write config");
        let trust = ConfigTrust::at(temp.path().join("trust-store"));
        trust
            .trust(config_file_target(&root, &config_file))
            .expect("trust root");
        fs::create_dir_all(config_file.parent().expect("config parent"))
            .expect("create config parent");
        fs::write(&config_file, "a = 2").expect("edit config");

        trust
            .trust(config_file_target(&root, &config_file))
            .expect("re-trust root");

        assert_eq!(
            trust
                .is_trusted(config_file_target(&root, &config_file))
                .expect("check trust"),
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
        let config_file = root.join(".traces/config.toml");
        fs::create_dir_all(config_file.parent().expect("config parent"))
            .expect("create config parent");
        fs::write(&config_file, "a = 1").expect("write config");
        let trust_store_root = temp.path().join("trust-store");
        let store = ConfigFileStore::at(trust_store_root.clone());
        store.record(&root).expect("record root directly, bypassing trust()");
        let trust = ConfigTrust::at(trust_store_root);

        assert_eq!(
            trust
                .is_trusted(config_file_target(&root, &config_file))
                .expect("check trust"),
            TrustState::Stale
        );
    }

    #[test]
    fn a_corrupted_companion_hash_is_stale_not_an_error() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let root = temp.path().join("project");
        fs::create_dir_all(&root).expect("create project dir");
        let config_file = root.join(".traces/config.toml");
        fs::create_dir_all(config_file.parent().expect("config parent"))
            .expect("create config parent");
        fs::write(&config_file, "a = 1").expect("write config");
        let trust_store_root = temp.path().join("trust-store");
        let store = ConfigFileStore::at(trust_store_root.clone());
        let trust = ConfigTrust::at(trust_store_root);
        trust
            .trust(config_file_target(&root, &config_file))
            .expect("trust root");
        let companion =
            companion_path(&store.entry_path(&root).expect("entry path"));
        fs::write(&companion, "not a valid blake3 hash")
            .expect("corrupt the companion hash file");

        assert_eq!(
            trust
                .is_trusted(config_file_target(&root, &config_file))
                .expect("check trust"),
            TrustState::Stale
        );
    }

    #[test]
    fn trust_of_a_nonexistent_root_errors() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let missing_root = temp.path().join("missing-root");
        let config_file = missing_root.join(LOCAL_CONFIG_FILE);
        let trust = ConfigTrust::at(temp.path().join("trust-store"));

        assert!(matches!(
            trust.trust(config_file_target(&missing_root, &config_file)),
            Err(TrustError::Store(StoreError::Canonicalize { .. }))
        ));
    }

    #[test]
    fn trust_of_a_directory_records_the_root_without_a_companion() {
        // `traces trust <path>` before `traces init` has created a config
        // file is a valid flow (see ADR 0002's issue-05 amendment): the
        // root is recorded, but there's no content to hash yet, so no
        // companion is written.
        let temp = tempfile::tempdir().expect("create temp dir");
        let root = temp.path().join("project");
        fs::create_dir_all(&root).expect("create project dir");
        let store = ConfigFileStore::at(temp.path().join("trust-store"));
        let trust = ConfigTrust::at(temp.path().join("trust-store"));

        trust.trust(TrustTarget::Directory(&root)).expect("trust root only");

        assert!(store.contains(&root).expect("check root recorded"));
        let companion =
            companion_path(&store.entry_path(&root).expect("entry path"));
        assert!(!companion.exists());
    }

    #[test]
    fn is_trusted_is_stale_for_a_root_trusted_without_a_config_file_even_after_one_appears()
     {
        let temp = tempfile::tempdir().expect("create temp dir");
        let root = temp.path().join("project");
        fs::create_dir_all(&root).expect("create project dir");
        let config_file = root.join(".traces/config.toml");
        let trust = ConfigTrust::at(temp.path().join("trust-store"));
        trust.trust(TrustTarget::Directory(&root)).expect("trust root only");

        fs::create_dir_all(config_file.parent().expect("config parent"))
            .expect("create config parent");
        fs::write(&config_file, "a = 1").expect("create config after trust");

        assert_eq!(
            trust
                .is_trusted(config_file_target(&root, &config_file))
                .expect("check trust"),
            TrustState::Stale
        );
    }

    #[test]
    fn re_trusting_after_the_config_file_appears_clears_staleness() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let root = temp.path().join("project");
        fs::create_dir_all(&root).expect("create project dir");
        let config_file = root.join(".traces/config.toml");
        let trust = ConfigTrust::at(temp.path().join("trust-store"));
        trust.trust(TrustTarget::Directory(&root)).expect("trust root only");
        fs::create_dir_all(config_file.parent().expect("config parent"))
            .expect("create config parent");
        fs::write(&config_file, "a = 1").expect("create config after trust");

        trust
            .trust(config_file_target(&root, &config_file))
            .expect("re-trust with config file");

        assert_eq!(
            trust
                .is_trusted(config_file_target(&root, &config_file))
                .expect("check trust"),
            TrustState::Trusted
        );
    }

    #[test]
    fn list_all_lists_trusted_roots() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let root = temp.path().join("project");
        fs::create_dir_all(&root).expect("create project dir");
        let config_file = root.join(".traces/config.toml");
        fs::create_dir_all(config_file.parent().expect("config parent"))
            .expect("create config parent");
        fs::write(&config_file, "a = 1").expect("write config");
        let trust = ConfigTrust::at(temp.path().join("trust-store"));
        trust
            .trust(config_file_target(&root, &config_file))
            .expect("trust root");

        assert_eq!(trust.list_all().expect("list trusted roots"), vec![
            root.canonicalize().expect("canonicalize root")
        ]);
    }

    #[test]
    fn list_all_on_an_empty_store_is_empty() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let trust = ConfigTrust::at(temp.path().join("trust-store"));

        assert!(trust.list_all().expect("list trusted roots").is_empty());
    }

    #[test]
    fn clean_removes_a_dangling_root_entry_and_its_companion() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let root = temp.path().join("project");
        fs::create_dir_all(&root).expect("create project dir");
        let config_file = root.join(".traces/config.toml");
        fs::create_dir_all(config_file.parent().expect("config parent"))
            .expect("create config parent");
        fs::write(&config_file, "a = 1").expect("write config");
        let trust_store_root = temp.path().join("trust-store");
        let store = ConfigFileStore::at(trust_store_root.clone());
        let trust = ConfigTrust::at(trust_store_root);
        trust
            .trust(config_file_target(&root, &config_file))
            .expect("trust root");
        let companion =
            companion_path(&store.entry_path(&root).expect("entry path"));
        assert!(companion.exists(), "companion should exist before clean");
        fs::remove_dir_all(&root).expect("delete project dir");

        let removed = trust.clean().expect("clean trust store");

        assert_eq!(removed, 1);
        assert!(trust.list_all().expect("list trusted roots").is_empty());
        assert!(
            !companion.exists(),
            "companion should be removed alongside its dangling root entry"
        );
    }

    #[test]
    fn clean_removes_a_dangling_root_entry_with_no_companion_without_erroring()
    {
        // Covers a root trusted before its config file ever existed (no
        // companion was ever written), then the root itself disappears.
        let temp = tempfile::tempdir().expect("create temp dir");
        let root = temp.path().join("project");
        fs::create_dir_all(&root).expect("create project dir");
        let trust = ConfigTrust::at(temp.path().join("trust-store"));
        trust.trust(TrustTarget::Directory(&root)).expect("trust root only");
        fs::remove_dir_all(&root).expect("delete project dir");

        let removed = trust.clean().expect("clean trust store");

        assert_eq!(removed, 1);
    }

    #[test]
    fn clean_on_a_store_with_no_entries_yet_removes_nothing() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let trust = ConfigTrust::at(temp.path().join("trust-store"));

        assert_eq!(trust.clean().expect("clean trust store"), 0);
    }

    #[test]
    fn clean_leaves_a_live_trusted_root_and_its_companion_untouched() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let root = temp.path().join("project");
        fs::create_dir_all(&root).expect("create project dir");
        let config_file = root.join(".traces/config.toml");
        fs::create_dir_all(config_file.parent().expect("config parent"))
            .expect("create config parent");
        fs::write(&config_file, "a = 1").expect("write config");
        let trust = ConfigTrust::at(temp.path().join("trust-store"));
        trust
            .trust(config_file_target(&root, &config_file))
            .expect("trust root");

        let removed = trust.clean().expect("clean trust store");

        assert_eq!(removed, 0);
        assert_eq!(
            trust
                .is_trusted(config_file_target(&root, &config_file))
                .expect("check trust"),
            TrustState::Trusted
        );
    }

    #[test]
    fn trust_propagates_a_store_write_failure_instead_of_swallowing_it() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let root = temp.path().join("project");
        fs::create_dir_all(&root).expect("create project dir");
        let config_file = root.join(".traces/config.toml");
        fs::create_dir_all(config_file.parent().expect("config parent"))
            .expect("create config parent");
        fs::write(&config_file, "a = 1").expect("write config");
        let trust_store_root = temp.path().join("trust-store");
        fs::write(&trust_store_root, "")
            .expect("occupy trust store root with a file");
        let trust = ConfigTrust::at(trust_store_root);

        assert!(matches!(
            trust.trust(config_file_target(&root, &config_file)),
            Err(TrustError::Store(StoreError::StoreIo { .. }))
        ));
    }
}
