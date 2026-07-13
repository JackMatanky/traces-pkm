//! Hash-keyed config file store.
//!
//! Stores canonical paths as SHA-256-named entries under a caller-provided
//! root: symlinks on Unix, plain files containing the path on Windows. This
//! module owns the cross-platform storage mechanics; domain modules choose
//! which root to use.

use std::{
    fmt::Write as _,
    fs, io,
    path::{Path, PathBuf},
};

use miette::Diagnostic;
use sha2::{Digest, Sha256};
use thiserror::Error;

use super::dirs;

/// Errors from [`ConfigFileStore`] operations.
///
/// Public so callers outside `config::store` (e.g.
/// [`super::domain::ConfigError`]) can wrap it as a `#[source]`/`#[from]`
/// without a private-type-in-public-API mismatch.
#[derive(Debug, Diagnostic, Error)]
pub enum StoreError {
    /// The recorded path could not be canonicalized.
    #[error("failed to canonicalize path {path}")]
    #[diagnostic(code(traces::config::store::canonicalize))]
    Canonicalize {
        /// Path that could not be canonicalized.
        path: PathBuf,
        /// Source I/O error.
        #[source]
        source: io::Error,
    },
    /// A store operation on `path` failed.
    #[error("config file store operation failed for {path}")]
    #[diagnostic(code(traces::config::store::io))]
    Io {
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

    /// Records `target`'s canonical path in this store.
    ///
    /// Idempotent: recording an already-stored path is a no-op.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when `target` cannot be canonicalized, the
    /// store root cannot be created, or the entry cannot be written.
    #[inline]
    #[allow(
        clippy::disallowed_methods,
        reason = "canonicalize-then-hash is the strictly-necessary path \
                  resolution this lint carves out an exception for"
    )]
    pub(super) fn record(&self, target: &Path) -> Result<(), StoreError> {
        let canonical = fs::canonicalize(target).map_err(|source| {
            StoreError::Canonicalize {
                path: target.to_path_buf(),
                source,
            }
        })?;
        let entry = self.root.join(hash_path(&canonical));
        if entry.exists() {
            return Ok(());
        }
        fs::create_dir_all(&self.root).map_err(|source| StoreError::Io {
            path: self.root.clone(),
            source,
        })?;
        #[cfg(unix)]
        let write_entry = std::os::unix::fs::symlink(&canonical, &entry);
        #[cfg(windows)]
        let write_entry = fs::write(
            &entry,
            canonical.as_os_str().to_string_lossy().as_bytes(),
        );

        write_entry.map_err(|source| StoreError::Io {
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
            #[cfg(unix)]
            let target = fs::read_link(&entry).ok();
            #[cfg(windows)]
            let target = fs::read_to_string(&entry).ok().map(PathBuf::from);

            if let Some(target) = target
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
        if !self.root.is_dir() {
            return Ok(0);
        }
        let mut removed = 0_usize;
        for entry in read_dir_entries(&self.root)? {
            #[cfg(unix)]
            let target = fs::read_link(&entry).ok();
            #[cfg(windows)]
            let target = fs::read_to_string(&entry).ok().map(PathBuf::from);

            let live = target.is_some_and(|target| target.exists());
            if live {
                continue;
            }
            fs::remove_file(&entry).map_err(|source| StoreError::Io {
                path: entry,
                source,
            })?;
            removed = removed.saturating_add(1);
        }
        Ok(removed)
    }
}

/// Reads the entry paths directly under `root`.
fn read_dir_entries(root: &Path) -> Result<Vec<PathBuf>, StoreError> {
    fs::read_dir(root)
        .map_err(|source| StoreError::Io {
            path: root.to_path_buf(),
            source,
        })?
        .map(|entry| {
            entry.map(|entry| entry.path()).map_err(|source| StoreError::Io {
                path: root.to_path_buf(),
                source,
            })
        })
        .collect()
}

fn hash_path(path: &Path) -> String {
    let digest = Sha256::digest(path.as_os_str().as_encoded_bytes());
    let mut hex = String::with_capacity(64);
    for byte in digest {
        let _: Result<(), std::fmt::Error> = write!(hex, "{byte:02x}");
    }
    hex
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

        assert!(matches!(store.record(&target), Err(StoreError::Io { .. })));
    }
}
