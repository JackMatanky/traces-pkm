//! Hash-keyed config file store.
//!
//! Stores canonical paths as BLAKE3-named entries under a caller-provided
//! root: symlinks on Unix, plain files containing the path on Windows. This
//! module owns the cross-platform storage mechanics; domain modules choose
//! which root to use.

use std::{
    fs, io,
    path::{Path, PathBuf},
};

use thiserror::Error;

use super::dirs;
use crate::hash::hash_path_to_str;

/// Errors from [`ConfigFileStore`] operations.
///
/// Internal plumbing: `thiserror`-only, no `miette::Diagnostic`. A raw
/// store I/O failure is never shown to a user or agent directly — callers
/// wrap it in a domain error (e.g. [`super::builder::ConfigBuilderError`])
/// before it reaches anything CLI-facing.
///
/// `pub` (not `pub(super)`) because [`super::service::ConfigService`]'s
/// public `list_tracked`/`clean_tracked_store` methods return this type
/// directly, and callers outside `config::store` need to be able to
/// observe it as a `#[source]`/return type without a
/// private-type-in-public-API mismatch. The `store` module itself stays
/// private, so this type is unreachable by name from outside `config` —
/// only observable through the API surfaces that expose it.
#[derive(Debug, Error)]
pub enum StoreError {
    /// The recorded path could not be canonicalized.
    #[error("failed to canonicalize path {path}")]
    Canonicalize {
        /// Path that could not be canonicalized.
        path: PathBuf,
        /// Source I/O error.
        #[source]
        source: io::Error,
    },
    /// A store operation on `path` failed.
    #[error("config file store operation failed for {path}")]
    StoreIo {
        /// Path the failing operation targeted (a directory or an entry).
        path: PathBuf,
        /// Source I/O error.
        #[source]
        source: io::Error,
    },
}

/// Records, lists, and cleans one hash-keyed config file store.
///
/// Holds the store root so callers don't have to keep threading it through
/// every call. Root-agnostic on purpose: this is the seam trust (issue 04)
/// reuses with its own root via [`ConfigFileStore::new`] — no
/// tracked/trusted-specific behavior lives here.
#[derive(Clone, Debug)]
pub(super) struct ConfigFileStore {
    root: PathBuf,
}

impl ConfigFileStore {
    /// Creates the store at a known state-dir-rooted location.
    ///
    /// Accepts only [`dirs::StateDirRoot`], whose constructor is private to
    /// [`paths`] — the only values a caller can pass are
    /// [`dirs::TRACKED_CONFIGS`] or [`dirs::TRUSTED_CONFIGS`], so this
    /// can't be pointed at an arbitrary or typo'd directory.
    #[inline]
    #[must_use]
    pub(super) fn new(root: &dirs::StateDirRoot) -> Self {
        Self {
            root: root.to_path_buf(),
        }
    }

    /// Creates a store rooted at an explicit path. Test-only: production
    /// callers always want a known location via [`Self::new`].
    #[cfg(test)]
    #[must_use]
    pub(super) fn at(root: PathBuf) -> Self {
        Self {
            root,
        }
    }

    /// Canonicalizes `target`. The single canonicalize-then-hash entry
    /// point [`Self::record`] and [`Self::contains`] both build on, so the
    /// two can never diverge on this step and silently split entries
    /// between "written by record" and "looked up by contains" for the
    /// same directory.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Canonicalize`] when `target` cannot be
    /// canonicalized.
    #[inline]
    #[allow(
        clippy::disallowed_methods,
        reason = "canonicalize-then-hash is the strictly-necessary path \
                  resolution this lint carves out an exception for"
    )]
    fn canonicalize(target: &Path) -> Result<PathBuf, StoreError> {
        fs::canonicalize(target).map_err(|source| StoreError::Canonicalize {
            path: target.to_path_buf(),
            source,
        })
    }

    /// Resolves `target` to the path its entry would live at (or does live
    /// at) in this store: canonicalize, then hash. Touches the filesystem
    /// only to canonicalize — it does not check whether the entry exists.
    ///
    /// Exposed (not just used internally by [`Self::record`]/
    /// [`Self::contains`]) so callers needing a companion file colocated
    /// with an entry — e.g. trust's (issue 04) content-hash-staleness
    /// record — can derive that location without reaching into this
    /// store's hashing internals.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Canonicalize`] when `target` cannot be
    /// canonicalized.
    #[inline]
    pub(super) fn entry_path(
        &self,
        target: &Path,
    ) -> Result<PathBuf, StoreError> {
        let canonical = Self::canonicalize(target)?;
        Ok(self.root.join(hash_path_to_str(&canonical)))
    }

    /// Records `target`'s canonical path in this store.
    ///
    /// Idempotent: recording an already-stored path is a no-op.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when `target` cannot be canonicalized, the
    /// store root cannot be created, or the entry cannot be written.
    #[inline]
    pub(super) fn record(&self, target: &Path) -> Result<(), StoreError> {
        let canonical = Self::canonicalize(target)?;
        let entry = self.root.join(hash_path_to_str(&canonical));
        if entry.exists() {
            return Ok(());
        }
        fs::create_dir_all(&self.root).map_err(|source| {
            StoreError::StoreIo {
                path: self.root.clone(),
                source,
            }
        })?;
        #[cfg(unix)]
        let write_entry = std::os::unix::fs::symlink(&canonical, &entry);
        #[cfg(windows)]
        let write_entry = fs::write(
            &entry,
            canonical.as_os_str().to_string_lossy().as_bytes(),
        );

        write_entry.map_err(|source| StoreError::StoreIo {
            path: entry,
            source,
        })
    }

    /// Returns whether `target`'s canonical path has a live entry in this
    /// store.
    ///
    /// Shares [`Self::canonicalize`] with [`Self::record`], so the same
    /// directory produces the same entry regardless of how `target` is
    /// spelled (relative, `.`-bearing, etc.) — this is the seam trust
    /// (issue 04) reuses instead of reimplementing hashing.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Canonicalize`] when `target` cannot be
    /// canonicalized. Returns [`StoreError::StoreIo`] when the entry's
    /// existence cannot be determined (permissions, I/O failure) — distinct
    /// from the entry simply not existing, which returns `Ok(false)`.
    #[inline]
    pub(super) fn contains(&self, target: &Path) -> Result<bool, StoreError> {
        let canonical = Self::canonicalize(target)?;
        let entry = self.root.join(hash_path_to_str(&canonical));
        entry.try_exists().map_err(|source| StoreError::StoreIo {
            path: entry,
            source,
        })
    }

    /// Lists the canonical paths of all live entries in this store.
    ///
    /// An entry is live when its target path can be read from the entry and
    /// still exists on disk. Dangling or unreadable entries are silently
    /// omitted. An absent or non-directory root is an empty list, not an
    /// error. Ordering follows the filesystem and is not stable.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when the store root exists but cannot be read.
    #[inline]
    pub(super) fn list_all(&self) -> Result<Vec<PathBuf>, StoreError> {
        if !self.root.is_dir() {
            return Ok(Vec::new());
        }
        let mut targets = Vec::new();
        for entry in read_dir_entries(&self.root)? {
            if let Some(target) = recorded_target(&entry)
                && target.exists()
            {
                targets.push(target);
            }
        }
        Ok(targets)
    }

    /// Removes entries in this store whose target path is unreadable or no
    /// longer exists. Returns the number of entries removed. An absent or
    /// non-directory root removes nothing.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when the store root exists but cannot be read,
    /// or a stale entry cannot be removed.
    #[inline]
    pub(super) fn clean(&self) -> Result<usize, StoreError> {
        Ok(self.clean_reporting()?.len())
    }

    /// Removes entries in this store whose target path is unreadable or no
    /// longer exists. Returns the full store-relative path of each entry
    /// removed (not just the count) — the seam trust (issue 05) needs to
    /// derive each removed entry's content-hash companion path from this,
    /// the same way [`Self::entry_path`] lets it derive a live entry's
    /// companion path. An absent or non-directory root removes nothing.
    ///
    /// A directory entry that isn't a symlink (or, on Windows, a
    /// path-bearing file this store itself wrote) was never written by
    /// [`Self::record`] — it isn't one of this store's entries at all, so
    /// it's left untouched rather than treated as dangling. This matters
    /// because a caller can colocate other files in the same root
    /// directory as this store's entries (trust's `.hash` companions,
    /// [`super::trust::ConfigTrust`]); only a *former* entry whose
    /// recorded target no longer exists counts as dangling.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when the store root exists but cannot be read,
    /// or a stale entry cannot be removed.
    #[inline]
    pub(super) fn clean_reporting(&self) -> Result<Vec<PathBuf>, StoreError> {
        if !self.root.is_dir() {
            return Ok(Vec::new());
        }
        let mut removed = Vec::new();
        for entry in read_dir_entries(&self.root)? {
            let Some(target) = recorded_target(&entry) else {
                continue;
            };
            if target.exists() {
                continue;
            }
            fs::remove_file(&entry).map_err(|source| StoreError::StoreIo {
                path: entry.clone(),
                source,
            })?;
            removed.push(entry);
        }
        Ok(removed)
    }
}

/// Reads `entry`'s recorded target path, if `entry` was actually written by
/// [`ConfigFileStore::record`] (a symlink on Unix, a path-bearing file on
/// Windows). Returns `None` when `entry` isn't one of this store's own
/// entries at all — including a caller-colocated file like trust's `.hash`
/// companion — not just when it's a genuinely broken/dangling entry;
/// [`ConfigFileStore::list_all`] and [`ConfigFileStore::clean_reporting`]
/// share this so "is this even one of our entries" stays defined once.
fn recorded_target(entry: &Path) -> Option<PathBuf> {
    #[cfg(unix)]
    let target = fs::read_link(entry);
    #[cfg(windows)]
    let target = fs::read_to_string(entry).map(PathBuf::from);
    target.ok()
}

/// Reads the entry paths directly under `root`.
fn read_dir_entries(root: &Path) -> Result<Vec<PathBuf>, StoreError> {
    fs::read_dir(root)
        .map_err(|source| StoreError::StoreIo {
            path: root.to_path_buf(),
            source,
        })?
        .map(|entry| {
            entry.map(|entry| entry.path()).map_err(|source| {
                StoreError::StoreIo {
                    path: root.to_path_buf(),
                    source,
                }
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::fs;

    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn record_creates_an_entry() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let target = temp.path().join("config.toml");
        fs::write(&target, "").expect("write config");
        let store = ConfigFileStore::at(temp.path().join("store"));

        store.record(&target).expect("record config");

        assert_eq!(store.list_all().expect("list configs"), vec![
            target.canonicalize().expect("canonicalize target")
        ]);
    }

    #[test]
    fn re_recording_the_same_path_is_idempotent() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let target = temp.path().join("config.toml");
        fs::write(&target, "").expect("write config");
        let store = ConfigFileStore::at(temp.path().join("store"));

        store.record(&target).expect("record config");
        store.record(&target).expect("record config");

        assert_eq!(store.list_all().expect("list configs").len(), 1);
    }

    #[test]
    fn re_recording_after_target_deleted_and_recreated_is_idempotent() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let target = temp.path().join("config.toml");
        fs::write(&target, "").expect("write config");
        let store = ConfigFileStore::at(temp.path().join("store"));

        store.record(&target).expect("record config");
        fs::remove_file(&target).expect("remove config");
        fs::write(&target, "").expect("write config");

        store.record(&target).expect("record config");

        assert_eq!(store.list_all().expect("list configs"), vec![
            target.canonicalize().expect("canonicalize target")
        ]);
    }

    #[test]
    fn list_all_omits_entries_whose_target_was_deleted() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let kept = temp.path().join("kept.toml");
        let deleted = temp.path().join("deleted.toml");
        fs::write(&kept, "").expect("write kept config");
        fs::write(&deleted, "").expect("write deleted config");
        let store = ConfigFileStore::at(temp.path().join("store"));
        store.record(&kept).expect("record kept config");
        store.record(&deleted).expect("record deleted config");
        fs::remove_file(&deleted).expect("remove deleted config");

        assert_eq!(store.list_all().expect("list configs"), vec![
            kept.canonicalize().expect("canonicalize kept config")
        ]);
    }

    #[test]
    fn clean_prunes_stale_entries_and_reports_the_count() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let kept = temp.path().join("kept.toml");
        let deleted = temp.path().join("deleted.toml");
        fs::write(&kept, "").expect("write kept config");
        fs::write(&deleted, "").expect("write deleted config");
        let store = ConfigFileStore::at(temp.path().join("store"));
        store.record(&kept).expect("record kept config");
        store.record(&deleted).expect("record deleted config");
        fs::remove_file(&deleted).expect("remove deleted config");

        let removed = store.clean().expect("clean store");

        assert_eq!(removed, 1);
        assert_eq!(store.list_all().expect("list configs"), vec![
            kept.canonicalize().expect("canonicalize kept config")
        ]);
    }

    #[test]
    fn list_all_on_a_store_with_no_entries_yet_is_empty() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let store = ConfigFileStore::at(temp.path().join("store"));

        assert!(store.list_all().expect("list configs").is_empty());
    }

    #[test]
    fn clean_on_a_store_with_no_entries_yet_removes_nothing() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let store = ConfigFileStore::at(temp.path().join("store"));

        assert_eq!(store.clean().expect("clean store"), 0);
    }

    #[test]
    fn clean_leaves_a_non_symlink_file_in_the_store_root_untouched() {
        // A caller can colocate other files alongside this store's entries
        // in the same root directory (e.g. trust's `.hash` companions,
        // config::trust::ConfigTrust) — clean must not treat "not one of
        // our symlink entries" as "dangling, remove it".
        let temp = tempfile::tempdir().expect("create temp dir");
        let store = ConfigFileStore::at(temp.path().join("store"));
        fs::create_dir_all(store.root.clone()).expect("create store root");
        let stray = store.root.join("not-a-store-entry");
        fs::write(&stray, "not a symlink").expect("write stray file");

        let removed = store.clean().expect("clean store");

        assert_eq!(removed, 0);
        assert!(stray.exists(), "stray non-entry file must survive clean");
    }

    #[test]
    fn clean_reporting_returns_the_removed_entries_own_paths() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let deleted = temp.path().join("deleted.toml");
        fs::write(&deleted, "").expect("write deleted config");
        let store = ConfigFileStore::at(temp.path().join("store"));
        let entry = store.entry_path(&deleted).expect("resolve entry path");
        store.record(&deleted).expect("record deleted config");
        fs::remove_file(&deleted).expect("remove deleted config");

        assert_eq!(store.clean_reporting().expect("clean store"), vec![entry]);
    }

    #[test]
    fn record_of_a_nonexistent_target_errors() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let store = ConfigFileStore::at(temp.path().join("store"));

        assert!(matches!(
            store.record(&temp.path().join("missing.toml")),
            Err(StoreError::Canonicalize { .. })
        ));
    }

    #[test]
    fn record_when_store_root_is_a_file_errors_with_io() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let target = temp.path().join("config.toml");
        fs::write(&target, "").expect("write config");
        let root = temp.path().join("store");
        fs::write(&root, "").expect("write store root file");
        let store = ConfigFileStore::at(root);

        assert!(matches!(
            store.record(&target),
            Err(StoreError::StoreIo { .. })
        ));
    }

    #[test]
    fn entry_path_matches_where_record_actually_writes_the_entry() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let target = temp.path().join("config.toml");
        fs::write(&target, "").expect("write config");
        let store = ConfigFileStore::at(temp.path().join("store"));

        let entry = store.entry_path(&target).expect("resolve entry path");
        store.record(&target).expect("record target");

        assert!(entry.exists());
    }

    #[test]
    fn entry_path_of_a_nonexistent_target_errors() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let store = ConfigFileStore::at(temp.path().join("store"));

        assert!(matches!(
            store.entry_path(&temp.path().join("missing.toml")),
            Err(StoreError::Canonicalize { .. })
        ));
    }

    #[test]
    fn contains_returns_false_for_a_target_not_yet_recorded() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let target = temp.path().join("config.toml");
        fs::write(&target, "").expect("write config");
        let store = ConfigFileStore::at(temp.path().join("store"));

        assert!(!store.contains(&target).expect("check containment"));
    }

    #[test]
    fn contains_returns_true_after_recording_the_target() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let target = temp.path().join("config.toml");
        fs::write(&target, "").expect("write config");
        let store = ConfigFileStore::at(temp.path().join("store"));
        store.record(&target).expect("record target");

        assert!(store.contains(&target).expect("check containment"));
    }

    #[test]
    fn contains_reflects_canonical_path_regardless_of_relative_input() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let dir = temp.path().join("nested");
        fs::create_dir_all(&dir).expect("create nested dir");
        let target = dir.join("config.toml");
        fs::write(&target, "").expect("write config");
        let store = ConfigFileStore::at(temp.path().join("store"));
        store.record(&target).expect("record target");

        let relative = dir.join(".").join("config.toml");
        assert!(store.contains(&relative).expect("check containment"));
    }

    #[test]
    fn contains_of_a_nonexistent_target_errors_with_canonicalize() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let store = ConfigFileStore::at(temp.path().join("store"));

        assert!(matches!(
            store.contains(&temp.path().join("missing.toml")),
            Err(StoreError::Canonicalize { .. })
        ));
    }
}
