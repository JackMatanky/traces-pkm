//! Config tracking store: records which config files
//! [`ConfigService`](super::ConfigService) has loaded, across all projects.
//!
//! Each recorded path is canonicalized, hashed with SHA-256, and represented
//! as a symlink (a plain file on Windows) named by the hex-encoded hash under
//! the caller-provided store root, pointing back at the canonical path. Mirrors
//! mise's `config/tracking.rs` `Tracker` — see ADR 0002.

use std::{
    fmt::Write as _,
    fs, io,
    path::{Path, PathBuf},
};

use miette::Diagnostic;
use sha2::{Digest, Sha256};
use thiserror::Error;

/// Errors from [`ConfigTracker`] operations.
#[derive(Debug, Diagnostic, Error)]
pub(crate) enum TrackerError {
    /// The recorded path could not be canonicalized.
    #[error("failed to canonicalize path {path}")]
    #[diagnostic(code(traces::config::tracker::canonicalize))]
    Canonicalize {
        /// Path that could not be canonicalized.
        path: PathBuf,
        /// Source I/O error.
        #[source]
        source: io::Error,
    },
    /// A tracking store operation on `path` failed.
    #[error("tracking store operation failed for {path}")]
    #[diagnostic(code(traces::config::tracker::io))]
    Io {
        /// Path the failing operation targeted (a directory or an entry).
        path: PathBuf,
        /// Source I/O error.
        #[source]
        source: io::Error,
    },
}

/// Records, lists, and cleans the config tracking store.
///
/// A namespace, not a value — there is no per-instance state (mirrors
/// mise's `Tracker`, an empty struct grouping associated functions).
pub(crate) struct ConfigTracker;

impl ConfigTracker {
    /// Records `target`'s canonical path under `dir`.
    ///
    /// Idempotent: recording an already-tracked path is a no-op.
    ///
    /// # Errors
    ///
    /// Returns [`TrackerError`] when `target` cannot be canonicalized, the
    /// store root cannot be created, or the entry cannot be written.
    #[inline]
    #[allow(
        clippy::disallowed_methods,
        reason = "canonicalize-then-hash is the strictly-necessary path \
                  resolution this lint carves out an exception for"
    )]
    pub(crate) fn track(dir: &Path, target: &Path) -> Result<(), TrackerError> {
        let canonical = fs::canonicalize(target).map_err(|source| {
            TrackerError::Canonicalize {
                path: target.to_path_buf(),
                source,
            }
        })?;
        let entry = dir.join(hash_path(&canonical));
        if entry.exists() {
            return Ok(());
        }
        fs::create_dir_all(dir).map_err(|source| TrackerError::Io {
            path: dir.to_path_buf(),
            source,
        })?;
        link(&canonical, &entry).map_err(|source| TrackerError::Io {
            path: entry,
            source,
        })
    }

    /// Lists the canonical paths of all live entries under `dir`.
    ///
    /// An entry is live when its target still exists on disk. Dangling
    /// entries are silently omitted. An absent or non-directory `dir` is an
    /// empty list, not an error.
    ///
    /// # Errors
    ///
    /// Returns [`TrackerError`] when `dir` exists but cannot be read.
    #[inline]
    #[allow(
        dead_code,
        reason = "no caller yet within this crate; required by issue 03's \
                  acceptance criteria as the programmatic capability, ahead \
                  of the CLI/cross-project surface that consumes it"
    )]
    pub(crate) fn list_all(dir: &Path) -> Result<Vec<PathBuf>, TrackerError> {
        if !dir.is_dir() {
            return Ok(Vec::new());
        }
        let mut targets = Vec::new();
        for entry in read_dir_entries(dir)? {
            if let Some(target) = resolve(&entry)
                && target.exists()
            {
                targets.push(target);
            }
        }
        Ok(targets)
    }

    /// Removes entries under `dir` whose target no longer exists. Returns
    /// the number of entries removed.
    ///
    /// # Errors
    ///
    /// Returns [`TrackerError`] when `dir` exists but cannot be read, or a
    /// stale entry cannot be removed.
    #[inline]
    #[allow(
        dead_code,
        reason = "no caller yet within this crate; required by issue 03's \
                  acceptance criteria as the programmatic capability, ahead \
                  of the CLI/cross-project surface that consumes it"
    )]
    pub(crate) fn clean(dir: &Path) -> Result<usize, TrackerError> {
        if !dir.is_dir() {
            return Ok(0);
        }
        let mut removed = 0_usize;
        for entry in read_dir_entries(dir)? {
            let live = resolve(&entry).is_some_and(|target| target.exists());
            if live {
                continue;
            }
            fs::remove_file(&entry).map_err(|source| TrackerError::Io {
                path: entry,
                source,
            })?;
            removed = removed.saturating_add(1);
        }
        Ok(removed)
    }
}

/// Reads the entry paths directly under `dir`.
fn read_dir_entries(dir: &Path) -> Result<Vec<PathBuf>, TrackerError> {
    fs::read_dir(dir)
        .map_err(|source| TrackerError::Io {
            path: dir.to_path_buf(),
            source,
        })?
        .map(|entry| {
            entry.map(|entry| entry.path()).map_err(|source| TrackerError::Io {
                path: dir.to_path_buf(),
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

#[cfg(unix)]
fn link(target: &Path, entry: &Path) -> io::Result<()> {
    std::os::unix::fs::symlink(target, entry)
}

#[cfg(windows)]
fn link(target: &Path, entry: &Path) -> io::Result<()> {
    fs::write(entry, target.as_os_str().to_string_lossy().as_bytes())
}

#[cfg(unix)]
fn resolve(entry: &Path) -> Option<PathBuf> {
    fs::read_link(entry).ok()
}

#[cfg(windows)]
fn resolve(entry: &Path) -> Option<PathBuf> {
    fs::read_to_string(entry).ok().map(PathBuf::from)
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
    fn track_creates_an_entry() -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let target = temp.path().join("config.toml");
        fs::write(&target, "")?;
        let dir = temp.path().join("store");

        ConfigTracker::track(&dir, &target)?;

        assert_eq!(
            ConfigTracker::list_all(&dir)?,
            vec![target.canonicalize()?]
        );
        Ok(())
    }

    #[test]
    fn re_tracking_the_same_path_is_idempotent()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let target = temp.path().join("config.toml");
        fs::write(&target, "")?;
        let dir = temp.path().join("store");

        ConfigTracker::track(&dir, &target)?;
        ConfigTracker::track(&dir, &target)?;

        assert_eq!(ConfigTracker::list_all(&dir)?.len(), 1);
        Ok(())
    }

    #[test]
    fn re_tracking_after_target_deleted_and_recreated_is_idempotent()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let target = temp.path().join("config.toml");
        fs::write(&target, "")?;
        let dir = temp.path().join("store");

        ConfigTracker::track(&dir, &target)?;
        fs::remove_file(&target)?;
        fs::write(&target, "")?;

        ConfigTracker::track(&dir, &target)?;

        assert_eq!(
            ConfigTracker::list_all(&dir)?,
            vec![target.canonicalize()?]
        );
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
        let dir = temp.path().join("store");
        ConfigTracker::track(&dir, &kept)?;
        ConfigTracker::track(&dir, &deleted)?;
        fs::remove_file(&deleted)?;

        assert_eq!(ConfigTracker::list_all(&dir)?, vec![kept.canonicalize()?]);
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
        let dir = temp.path().join("store");
        ConfigTracker::track(&dir, &kept)?;
        ConfigTracker::track(&dir, &deleted)?;
        fs::remove_file(&deleted)?;

        let removed = ConfigTracker::clean(&dir)?;

        assert_eq!(removed, 1);
        assert_eq!(ConfigTracker::list_all(&dir)?, vec![kept.canonicalize()?]);
        Ok(())
    }

    #[test]
    fn list_all_on_a_store_with_no_entries_yet_is_empty()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let dir = temp.path().join("store");

        assert!(ConfigTracker::list_all(&dir)?.is_empty());
        Ok(())
    }

    #[test]
    fn clean_on_a_store_with_no_entries_yet_removes_nothing()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let dir = temp.path().join("store");

        assert_eq!(ConfigTracker::clean(&dir)?, 0);
        Ok(())
    }

    #[test]
    fn track_of_a_nonexistent_target_errors()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let dir = temp.path().join("store");

        let err = ConfigTracker::track(&dir, &temp.path().join("missing.toml"))
            .expect_err("canonicalizing a missing path should fail");

        assert!(matches!(err, TrackerError::Canonicalize { .. }));
        Ok(())
    }

    #[test]
    fn track_when_store_root_is_a_file_errors_with_io()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let target = temp.path().join("config.toml");
        fs::write(&target, "")?;
        let dir = temp.path().join("store");
        fs::write(&dir, "")?;

        let err = ConfigTracker::track(&dir, &target)
            .expect_err("store root occupied by a file should fail");

        assert!(matches!(err, TrackerError::Io { .. }));
        Ok(())
    }
}
