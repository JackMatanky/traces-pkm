//! Hash-keyed config file store.
//!
//! Stores canonical paths as SHA-256-named symlinks (plain files on Windows)
//! under a caller-provided root. This module owns the cross-platform storage
//! mechanics; domain modules choose which root to use.

use std::{
    fmt::Write as _,
    fs, io,
    path::{Path, PathBuf},
};

use miette::Diagnostic;
use sha2::{Digest, Sha256};
use thiserror::Error;

/// Errors from [`ConfigFileStore`] operations.
#[derive(Debug, Diagnostic, Error)]
pub(crate) enum StoreError {
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

/// Records, lists, and cleans hash-keyed config file stores.
pub(crate) struct ConfigFileStore;

impl ConfigFileStore {
    /// Records `target`'s canonical path under `root`.
    ///
    /// Idempotent: recording an already-stored path is a no-op.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when `target` cannot be canonicalized, `root`
    /// cannot be created, or the entry cannot be written.
    #[inline]
    #[allow(
        clippy::disallowed_methods,
        reason = "canonicalize-then-hash is the strictly-necessary path \
                  resolution this lint carves out an exception for"
    )]
    pub(crate) fn record(root: &Path, target: &Path) -> Result<(), StoreError> {
        let canonical = fs::canonicalize(target).map_err(|source| {
            StoreError::Canonicalize {
                path: target.to_path_buf(),
                source,
            }
        })?;
        let entry = root.join(hash_path(&canonical));
        if entry.exists() {
            return Ok(());
        }
        fs::create_dir_all(root).map_err(|source| StoreError::Io {
            path: root.to_path_buf(),
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

    /// Lists the canonical paths of all live entries under `root`.
    ///
    /// An entry is live when its target still exists on disk. Dangling
    /// entries are silently omitted. An absent or non-directory `root` is an
    /// empty list, not an error.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when `root` exists but cannot be read.
    #[inline]
    pub(crate) fn list_all(root: &Path) -> Result<Vec<PathBuf>, StoreError> {
        if !root.is_dir() {
            return Ok(Vec::new());
        }
        let mut targets = Vec::new();
        for entry in read_dir_entries(root)? {
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

    /// Removes entries under `root` whose target no longer exists. Returns
    /// the number of entries removed.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when `root` exists but cannot be read, or a
    /// stale entry cannot be removed.
    #[inline]
    pub(crate) fn clean(root: &Path) -> Result<usize, StoreError> {
        if !root.is_dir() {
            return Ok(0);
        }
        let mut removed = 0_usize;
        for entry in read_dir_entries(root)? {
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
    #![allow(
        clippy::indexing_slicing,
        clippy::panic_in_result_fn,
        clippy::unwrap_used,
        reason = "test code uses direct assertions and temp-file setup"
    )]

    use std::fs;

    use super::*;

    #[test]
    fn record_creates_an_entry() -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let target = temp.path().join("config.toml");
        fs::write(&target, "")?;
        let root = temp.path().join("store");

        ConfigFileStore::record(&root, &target)?;

        assert_eq!(ConfigFileStore::list_all(&root)?, vec![
            target.canonicalize()?
        ]);
        Ok(())
    }

    #[test]
    fn re_recording_the_same_path_is_idempotent()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let target = temp.path().join("config.toml");
        fs::write(&target, "")?;
        let root = temp.path().join("store");

        ConfigFileStore::record(&root, &target)?;
        ConfigFileStore::record(&root, &target)?;

        assert_eq!(ConfigFileStore::list_all(&root)?.len(), 1);
        Ok(())
    }

    #[test]
    fn re_recording_after_target_deleted_and_recreated_is_idempotent()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let target = temp.path().join("config.toml");
        fs::write(&target, "")?;
        let root = temp.path().join("store");

        ConfigFileStore::record(&root, &target)?;
        fs::remove_file(&target)?;
        fs::write(&target, "")?;

        ConfigFileStore::record(&root, &target)?;

        assert_eq!(ConfigFileStore::list_all(&root)?, vec![
            target.canonicalize()?
        ]);
        Ok(())
    }

    #[test]
    fn list_all_omits_entries_whose_target_was_deleted()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let kept = temp.path().join("kept.toml");
        let deleted = temp.path().join("deleted.toml");
        fs::write(&kept, "")?;
        fs::write(&deleted, "")?;
        let root = temp.path().join("store");
        ConfigFileStore::record(&root, &kept)?;
        ConfigFileStore::record(&root, &deleted)?;
        fs::remove_file(&deleted)?;

        assert_eq!(ConfigFileStore::list_all(&root)?, vec![
            kept.canonicalize()?
        ]);
        Ok(())
    }

    #[test]
    fn clean_prunes_stale_entries_and_reports_the_count()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let kept = temp.path().join("kept.toml");
        let deleted = temp.path().join("deleted.toml");
        fs::write(&kept, "")?;
        fs::write(&deleted, "")?;
        let root = temp.path().join("store");
        ConfigFileStore::record(&root, &kept)?;
        ConfigFileStore::record(&root, &deleted)?;
        fs::remove_file(&deleted)?;

        let removed = ConfigFileStore::clean(&root)?;

        assert_eq!(removed, 1);
        assert_eq!(ConfigFileStore::list_all(&root)?, vec![
            kept.canonicalize()?
        ]);
        Ok(())
    }

    #[test]
    fn list_all_on_a_store_with_no_entries_yet_is_empty()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("store");

        assert!(ConfigFileStore::list_all(&root)?.is_empty());
        Ok(())
    }

    #[test]
    fn clean_on_a_store_with_no_entries_yet_removes_nothing()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("store");

        assert_eq!(ConfigFileStore::clean(&root)?, 0);
        Ok(())
    }

    #[test]
    fn record_of_a_nonexistent_target_errors()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("store");

        let err =
            ConfigFileStore::record(&root, &temp.path().join("missing.toml"))
                .expect_err("canonicalizing a missing path should fail");

        assert!(matches!(err, StoreError::Canonicalize { .. }));
        Ok(())
    }

    #[test]
    fn record_when_store_root_is_a_file_errors_with_io()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let target = temp.path().join("config.toml");
        fs::write(&target, "")?;
        let root = temp.path().join("store");
        fs::write(&root, "")?;

        let err = ConfigFileStore::record(&root, &target)
            .expect_err("store root occupied by a file should fail");

        assert!(matches!(err, StoreError::Io { .. }));
        Ok(())
    }
}
