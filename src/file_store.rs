//! Generic hash-keyed file/path state store.
//!
//! Stores canonical paths as BLAKE3-named entries under a caller-provided root:
//! symlinks on Unix, plain files containing the path on Windows. Domain modules
//! choose which root to use; this module owns only the cross-platform storage
//! mechanics.

use std::{
    fs, io,
    path::{Path, PathBuf},
};

use thiserror::Error;

use crate::{Blake3PathHash, dirs::StateDirRoot};

/// Errors from [`FileStateStore`] operations.
#[derive(Debug, Error)]
pub(crate) enum FileStateStoreError {
    /// The target path could not be canonicalized before store-key hashing.
    #[error("failed to canonicalize path {path}")]
    Canonicalize {
        /// Path that could not be canonicalized.
        path: PathBuf,
        /// Source I/O error.
        #[source]
        source: io::Error,
    },
    /// A store operation on `path` failed.
    #[error("file state store operation failed for {path}")]
    StoreIo {
        /// Path the failing operation targeted (a directory or an entry).
        path: PathBuf,
        /// Source I/O error.
        #[source]
        source: io::Error,
    },
    /// A companion file could not be read.
    #[error("failed to read companion file {path}")]
    CompanionRead {
        /// Companion file path.
        path: PathBuf,
        /// Source I/O error.
        #[source]
        source: io::Error,
    },
    /// A companion file could not be written.
    #[error("failed to write companion file {path}")]
    CompanionWrite {
        /// Companion file path.
        path: PathBuf,
        /// Source I/O error.
        #[source]
        source: io::Error,
    },
    /// A companion file could not be removed.
    #[error("failed to remove companion file {path}")]
    CompanionRemove {
        /// Companion file path.
        path: PathBuf,
        /// Source I/O error.
        #[source]
        source: io::Error,
    },
}

/// Records, lists, and cleans one hash-keyed file/path state store.
#[derive(Clone, Debug)]
pub(crate) struct FileStateStore {
    root: StateDirRoot,
}

/// Stale-entry cleanup policy for [`FileStateStore`].
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum FileStoreCleanMode<'a> {
    /// Remove only stale root entries.
    EntriesOnly,
    /// Remove stale root entries and same-named companion files with these
    /// suffixes appended.
    WithCompanions(&'a [&'a str]),
}

struct StoreEntry {
    canonical_target: PathBuf,
    hash: Blake3PathHash,
}

impl StoreEntry {
    #[inline]
    fn path_in(&self, root: &Path) -> PathBuf {
        root.join(self.hash.as_str())
    }
}

impl TryFrom<&Path> for StoreEntry {
    type Error = FileStateStoreError;

    #[inline]
    #[expect(
        clippy::disallowed_methods,
        reason = "file-store entries must canonicalize targets before hashing"
    )]
    fn try_from(target: &Path) -> Result<Self, Self::Error> {
        let canonical_target = fs::canonicalize(target).map_err(|source| {
            FileStateStoreError::Canonicalize {
                path: target.to_path_buf(),
                source,
            }
        })?;
        let hash = Blake3PathHash::new(&canonical_target);
        Ok(Self {
            canonical_target,
            hash,
        })
    }
}

impl FileStateStore {
    /// Creates a store rooted at `root`.
    #[inline]
    #[must_use]
    pub(crate) fn new(root: StateDirRoot) -> Self {
        Self {
            root,
        }
    }

    /// Creates a store rooted at an arbitrary path for tests.
    #[cfg(test)]
    #[inline]
    #[must_use]
    pub(crate) fn at(root: PathBuf) -> Self {
        Self {
            root: StateDirRoot::from_path(root),
        }
    }

    /// Records `target`'s canonical path in this store.
    ///
    /// Idempotent: recording an already-stored path is a no-op.
    ///
    /// # Errors
    ///
    /// Returns [`FileStateStoreError`] when `target` cannot be canonicalized,
    /// the store root cannot be created, or the entry cannot be written.
    #[inline]
    pub(crate) fn record(
        &self,
        target: &Path,
    ) -> Result<(), FileStateStoreError> {
        let entry = StoreEntry::try_from(target)?;
        let entry_path = entry.path_in(&self.root);
        if entry_path.exists() {
            return Ok(());
        }
        fs::create_dir_all(&self.root).map_err(|source| {
            FileStateStoreError::StoreIo {
                path: self.root.to_path_buf(),
                source,
            }
        })?;

        #[cfg(unix)]
        let write_entry =
            std::os::unix::fs::symlink(&entry.canonical_target, &entry_path);
        #[cfg(windows)]
        let write_entry = fs::write(
            &entry_path,
            entry.canonical_target.as_os_str().to_string_lossy().as_bytes(),
        );

        write_entry.map_err(|source| FileStateStoreError::StoreIo {
            path: entry_path,
            source,
        })
    }

    /// Returns whether `target`'s canonical path has a live entry in this
    /// store.
    ///
    /// # Errors
    ///
    /// Returns [`FileStateStoreError`] when `target` cannot be canonicalized or
    /// the entry's existence cannot be determined.
    #[inline]
    pub(crate) fn contains(
        &self,
        target: &Path,
    ) -> Result<bool, FileStateStoreError> {
        let entry = StoreEntry::try_from(target)?;
        let entry_path = entry.path_in(&self.root);
        entry_path.try_exists().map_err(|source| FileStateStoreError::StoreIo {
            path: entry_path,
            source,
        })
    }

    /// Lists the canonical paths of all live entries in this store.
    ///
    /// An entry is live when its target path can be read from the entry and
    /// still exists on disk. Dangling or unreadable entries are silently
    /// omitted. An absent or non-directory root is an empty list, not an error.
    ///
    /// # Errors
    ///
    /// Returns [`FileStateStoreError`] when the store root exists but cannot be
    /// read.
    #[inline]
    pub(crate) fn list_all(&self) -> Result<Vec<PathBuf>, FileStateStoreError> {
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
    /// longer exists. Returns the number of root entries removed. An absent or
    /// non-directory root removes nothing.
    ///
    /// # Errors
    ///
    /// Returns [`FileStateStoreError`] when the store root exists but cannot be
    /// read, a stale entry cannot be removed, or an existing companion cannot
    /// be removed.
    #[inline]
    pub(crate) fn clean(
        &self,
        mode: FileStoreCleanMode<'_>,
    ) -> Result<usize, FileStateStoreError> {
        let removed = self.clean_reporting()?;
        let FileStoreCleanMode::WithCompanions(suffixes) = mode else {
            return Ok(removed.len());
        };
        for entry in &removed {
            for suffix in suffixes {
                let companion = companion_path(entry, suffix);
                match fs::remove_file(&companion) {
                    Ok(()) => {}
                    Err(source) if source.kind() == io::ErrorKind::NotFound => {
                    }
                    Err(source) => {
                        return Err(FileStateStoreError::CompanionRemove {
                            path: companion,
                            source,
                        });
                    }
                }
            }
        }
        Ok(removed.len())
    }

    /// Writes a companion file next to `target`'s store entry.
    ///
    /// # Errors
    ///
    /// Returns [`FileStateStoreError`] when `target` cannot be canonicalized or
    /// the companion cannot be written.
    #[inline]
    pub(crate) fn write_companion(
        &self,
        target: &Path,
        suffix: &str,
        contents: impl AsRef<[u8]>,
    ) -> Result<(), FileStateStoreError> {
        let entry = StoreEntry::try_from(target)?;
        let entry_path = entry.path_in(&self.root);
        let companion = companion_path(&entry_path, suffix);
        fs::write(&companion, contents).map_err(|source| {
            FileStateStoreError::CompanionWrite {
                path: companion,
                source,
            }
        })
    }

    /// Reads a companion file next to `target`'s store entry, if present.
    ///
    /// # Errors
    ///
    /// Returns [`FileStateStoreError`] when `target` cannot be canonicalized or
    /// the companion cannot be read.
    #[inline]
    pub(crate) fn read_companion(
        &self,
        target: &Path,
        suffix: &str,
    ) -> Result<Option<String>, FileStateStoreError> {
        let entry = StoreEntry::try_from(target)?;
        let entry_path = entry.path_in(&self.root);
        let companion = companion_path(&entry_path, suffix);
        match fs::read_to_string(&companion) {
            Ok(contents) => Ok(Some(contents)),
            Err(source) if source.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(source) => Err(FileStateStoreError::CompanionRead {
                path: companion,
                source,
            }),
        }
    }

    /// Removes `target`'s entry plus companion files, returning `1` when the
    /// root entry was removed and `0` when it was already absent.
    ///
    /// # Errors
    ///
    /// Returns [`FileStateStoreError`] when `target` cannot be canonicalized,
    /// the root entry cannot be removed, or a companion cannot be removed.
    #[inline]
    pub(crate) fn remove_with_companions(
        &self,
        target: &Path,
        suffixes: &[&str],
    ) -> Result<usize, FileStateStoreError> {
        let entry = StoreEntry::try_from(target)?;
        let entry_path = entry.path_in(&self.root);
        let removed = match fs::remove_file(&entry_path) {
            Ok(()) => 1,
            Err(source) if source.kind() == io::ErrorKind::NotFound => 0,
            Err(source) => {
                return Err(FileStateStoreError::StoreIo {
                    path: entry_path,
                    source,
                });
            }
        };
        for suffix in suffixes {
            let companion = companion_path(&entry_path, suffix);
            match fs::remove_file(&companion) {
                Ok(()) => {}
                Err(source) if source.kind() == io::ErrorKind::NotFound => {}
                Err(source) => {
                    return Err(FileStateStoreError::CompanionRemove {
                        path: companion,
                        source,
                    });
                }
            }
        }
        Ok(removed)
    }

    /// Removes stale entries and returns the full path of each removed entry.
    fn clean_reporting(&self) -> Result<Vec<PathBuf>, FileStateStoreError> {
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
            match fs::remove_file(&entry) {
                Ok(()) => removed.push(entry),
                Err(source) => {
                    return Err(FileStateStoreError::StoreIo {
                        path: entry,
                        source,
                    });
                }
            }
        }
        Ok(removed)
    }
}

fn companion_path(entry: &Path, suffix: &str) -> PathBuf {
    let mut name = entry.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

/// Reads `entry`'s recorded target path, if `entry` was written by
/// [`FileStateStore::record`].
fn recorded_target(entry: &Path) -> Option<PathBuf> {
    #[cfg(unix)]
    let target = fs::read_link(entry);
    #[cfg(windows)]
    let target = fs::read_to_string(entry).map(PathBuf::from);
    target.ok()
}

/// Reads the entry paths directly under `root`.
fn read_dir_entries(root: &Path) -> Result<Vec<PathBuf>, FileStateStoreError> {
    fs::read_dir(root)
        .map_err(|source| FileStateStoreError::StoreIo {
            path: root.to_path_buf(),
            source,
        })?
        .map(|entry| {
            entry.map(|entry| entry.path()).map_err(|source| {
                FileStateStoreError::StoreIo {
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
        let store = FileStateStore::at(temp.path().join("store"));

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
        let store = FileStateStore::at(temp.path().join("store"));

        store.record(&target).expect("record config");
        store.record(&target).expect("record config again");

        assert_eq!(store.list_all().expect("list configs").len(), 1);
    }

    #[test]
    fn write_companion_writes_next_to_entry() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let target = temp.path().join("target");
        fs::create_dir_all(&target).expect("create target dir");
        let store = FileStateStore::at(temp.path().join("store"));
        store.record(&target).expect("record target");
        let companion = companion_path(
            &StoreEntry::try_from(target.as_path())
                .expect("resolve entry")
                .path_in(&store.root),
            ".hash",
        );

        store
            .write_companion(&target, ".hash", "hash")
            .expect("write companion");

        assert_eq!(
            fs::read_to_string(companion).expect("read companion"),
            "hash"
        );
    }

    #[test]
    fn remove_with_companions_removes_existing_companion() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let target = temp.path().join("target");
        fs::create_dir_all(&target).expect("create target dir");
        let store = FileStateStore::at(temp.path().join("store"));
        store.record(&target).expect("record target");
        store
            .write_companion(&target, ".hash", "hash")
            .expect("write companion");
        let companion = companion_path(
            &StoreEntry::try_from(target.as_path())
                .expect("resolve entry")
                .path_in(&store.root),
            ".hash",
        );

        store
            .remove_with_companions(&target, &[".hash"])
            .expect("remove companion");

        assert!(!companion.exists());
    }

    #[test]
    fn list_all_omits_entries_whose_target_was_deleted() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let kept = temp.path().join("kept.toml");
        let deleted = temp.path().join("deleted.toml");
        fs::write(&kept, "").expect("write kept config");
        fs::write(&deleted, "").expect("write deleted config");
        let store = FileStateStore::at(temp.path().join("store"));
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
        let store = FileStateStore::at(temp.path().join("store"));
        store.record(&kept).expect("record kept config");
        store.record(&deleted).expect("record deleted config");
        fs::remove_file(&deleted).expect("remove deleted config");

        let removed =
            store.clean(FileStoreCleanMode::EntriesOnly).expect("clean store");

        assert_eq!(removed, 1);
        assert_eq!(store.list_all().expect("list configs"), vec![
            kept.canonicalize().expect("canonicalize kept config")
        ]);
    }

    #[test]
    fn list_all_on_a_store_with_no_entries_yet_is_empty() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let store = FileStateStore::at(temp.path().join("store"));

        assert!(store.list_all().expect("list configs").is_empty());
    }

    #[test]
    fn clean_on_a_store_with_no_entries_yet_removes_nothing() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let store = FileStateStore::at(temp.path().join("store"));

        assert_eq!(
            store.clean(FileStoreCleanMode::EntriesOnly).expect("clean store"),
            0
        );
    }

    #[cfg(unix)]
    #[test]
    fn clean_leaves_a_non_symlink_file_in_the_store_root_untouched() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let store = FileStateStore::at(temp.path().join("store"));
        fs::create_dir_all(&store.root).expect("create store root");
        let stray = store.root.join("not-a-store-entry");
        fs::write(&stray, "not a symlink").expect("write stray file");

        let removed =
            store.clean(FileStoreCleanMode::EntriesOnly).expect("clean store");

        assert_eq!(removed, 0);
        assert!(stray.exists(), "stray non-entry file must survive clean");
    }

    #[test]
    fn companion_path_appends_the_suffix_to_the_entrys_filename() {
        let entry = Path::new("/store/abc123");

        assert_eq!(
            companion_path(entry, ".hash"),
            Path::new("/store/abc123.hash")
        );
    }

    #[test]
    fn clean_with_companion_mode_removes_a_dangling_entry_and_its_companion() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let target = temp.path().join("target");
        fs::create_dir_all(&target).expect("create target dir");
        let store = FileStateStore::at(temp.path().join("store"));
        store.record(&target).expect("record target");
        let entry = StoreEntry::try_from(target.as_path())
            .expect("resolve entry path")
            .path_in(&store.root);
        let companion = companion_path(&entry, ".hash");
        fs::write(&companion, "hash").expect("write companion");
        fs::remove_dir_all(&target).expect("delete target dir");

        let removed = store
            .clean(FileStoreCleanMode::WithCompanions(&[".hash"]))
            .expect("clean");

        assert_eq!(removed, 1);
        assert!(!companion.exists(), "companion should be removed too");
    }

    #[test]
    fn clean_with_companion_mode_removes_a_dangling_entry_with_no_companion() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let target = temp.path().join("target");
        fs::create_dir_all(&target).expect("create target dir");
        let store = FileStateStore::at(temp.path().join("store"));
        store.record(&target).expect("record target");
        fs::remove_dir_all(&target).expect("delete target dir");

        let removed = store
            .clean(FileStoreCleanMode::WithCompanions(&[".hash"]))
            .expect("clean");

        assert_eq!(removed, 1);
    }

    #[test]
    fn clean_with_companion_mode_leaves_a_live_entrys_companion_untouched() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let target = temp.path().join("target");
        fs::create_dir_all(&target).expect("create target dir");
        let store = FileStateStore::at(temp.path().join("store"));
        store.record(&target).expect("record target");
        let entry = StoreEntry::try_from(target.as_path())
            .expect("resolve entry path")
            .path_in(&store.root);
        let companion = companion_path(&entry, ".hash");
        fs::write(&companion, "hash").expect("write companion");

        let removed = store
            .clean(FileStoreCleanMode::WithCompanions(&[".hash"]))
            .expect("clean");

        assert_eq!(removed, 0);
        assert!(companion.exists(), "live entry's companion must survive");
    }

    #[test]
    fn record_of_a_nonexistent_target_errors() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let store = FileStateStore::at(temp.path().join("store"));

        assert!(matches!(
            store.record(&temp.path().join("missing.toml")),
            Err(FileStateStoreError::Canonicalize { .. })
        ));
    }

    #[test]
    fn record_when_store_root_is_a_file_errors_with_io() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let target = temp.path().join("config.toml");
        fs::write(&target, "").expect("write config");
        let root = temp.path().join("store");
        fs::write(&root, "").expect("write store root file");
        let store = FileStateStore::at(root);

        assert!(matches!(
            store.record(&target),
            Err(FileStateStoreError::StoreIo { .. })
        ));
    }

    #[test]
    fn store_entry_matches_where_record_actually_writes_the_entry() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let target = temp.path().join("config.toml");
        fs::write(&target, "").expect("write config");
        let store = FileStateStore::at(temp.path().join("store"));

        let entry = StoreEntry::try_from(target.as_path())
            .expect("resolve entry path")
            .path_in(&store.root);
        store.record(&target).expect("record target");

        assert!(entry.exists());
    }

    #[test]
    fn store_entry_of_a_nonexistent_target_errors() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let _store = FileStateStore::at(temp.path().join("store"));

        assert!(matches!(
            StoreEntry::try_from(temp.path().join("missing.toml").as_path()),
            Err(FileStateStoreError::Canonicalize { .. })
        ));
    }

    #[test]
    fn contains_returns_false_for_a_target_not_yet_recorded() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let target = temp.path().join("config.toml");
        fs::write(&target, "").expect("write config");
        let store = FileStateStore::at(temp.path().join("store"));

        assert!(!store.contains(&target).expect("check containment"));
    }

    #[test]
    fn contains_returns_true_after_recording_the_target() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let target = temp.path().join("config.toml");
        fs::write(&target, "").expect("write config");
        let store = FileStateStore::at(temp.path().join("store"));
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
        let store = FileStateStore::at(temp.path().join("store"));
        store.record(&target).expect("record target");

        let relative = dir.join(".").join("config.toml");
        assert!(store.contains(&relative).expect("check containment"));
    }

    #[test]
    fn contains_of_a_nonexistent_target_errors_with_canonicalize() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let store = FileStateStore::at(temp.path().join("store"));

        assert!(matches!(
            store.contains(&temp.path().join("missing.toml")),
            Err(FileStateStoreError::Canonicalize { .. })
        ));
    }
}
