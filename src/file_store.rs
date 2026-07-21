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
    use super::*;

    struct Fixture {
        temp: tempfile::TempDir,
        store: FileStateStore,
    }

    impl Fixture {
        fn new() -> Self {
            let temp = tempfile::tempdir().expect("create temp dir");
            let store = FileStateStore::at(temp.path().join("store"));
            Self { temp, store }
        }

        fn target(&self, name: &str) -> PathBuf {
            let path = self.temp.path().join(name);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).expect("create parent");
            }
            fs::write(&path, "").expect("write target");
            path
        }

        fn entry_path_for(&self, target: &Path) -> PathBuf {
            StoreEntry::try_from(target)
                .expect("resolve entry")
                .path_in(&self.store.root)
        }
    }

    mod entry {
        use super::*;

        #[test]
        fn companion_path_appends_suffix() {
            // Arrange
            let base = Path::new("/store/abc123");

            // Act
            let result = companion_path(base, ".hash");

            // Assert
            assert_eq!(result, Path::new("/store/abc123.hash"));
        }

        #[test]
        fn errors_on_nonexistent_target() {
            // Arrange
            let fixture = Fixture::new();
            let missing = fixture.temp.path().join("missing");

            // Act
            let result = StoreEntry::try_from(missing.as_path());

            // Assert
            assert!(matches!(result, Err(FileStateStoreError::Canonicalize { .. })));
        }
    }

    mod record {
        use super::*;

        #[test]
        fn creates_entry_in_store_root() {
            // Arrange
            let fixture = Fixture::new();
            let target = fixture.target("target");

            // Act
            let result = fixture.store.record(&target);

            // Assert
            assert!(result.is_ok());
            assert!(fixture.entry_path_for(&target).exists());
        }

        #[test]
        fn is_idempotent() {
            // Arrange
            let fixture = Fixture::new();
            let target = fixture.target("target");
            fixture.store.record(&target).expect("first record");

            // Act
            let result = fixture.store.record(&target);

            // Assert
            assert!(result.is_ok());
        }

        #[test]
        fn errors_on_nonexistent_target() {
            // Arrange
            let fixture = Fixture::new();
            let missing = fixture.temp.path().join("missing");

            // Act
            let result = fixture.store.record(&missing);

            // Assert
            assert!(matches!(result, Err(FileStateStoreError::Canonicalize { .. })));
        }

        #[test]
        fn errors_when_store_root_is_a_file() {
            // Arrange
            let fixture = Fixture::new();
            let target = fixture.target("target");
            fs::write(&fixture.store.root, "not a dir").expect("write file");

            // Act
            let result = fixture.store.record(&target);

            // Assert
            assert!(matches!(result, Err(FileStateStoreError::StoreIo { .. })));
        }
    }

    mod contains {
        use super::*;

        #[test]
        fn returns_false_for_unrecorded_target() {
            // Arrange
            let fixture = Fixture::new();
            let target = fixture.target("target");

            // Act
            let result = fixture.store.contains(&target);

            // Assert
            assert_eq!(result.unwrap(), false);
        }

        #[test]
        fn returns_true_for_recorded_target() {
            // Arrange
            let fixture = Fixture::new();
            let target = fixture.target("target");
            fixture.store.record(&target).expect("record");

            // Act
            let result = fixture.store.contains(&target);

            // Assert
            assert_eq!(result.unwrap(), true);
        }

        #[test]
        fn reflects_canonical_path_regardless_of_relative_input() {
            // Arrange
            let fixture = Fixture::new();
            let target = fixture.target("nested/target");
            fixture.store.record(&target).expect("record");
            let relative = fixture.temp.path().join("nested/./target");

            // Act
            let result = fixture.store.contains(&relative);

            // Assert
            assert_eq!(result.unwrap(), true);
        }

        #[test]
        fn errors_on_nonexistent_target() {
            // Arrange
            let fixture = Fixture::new();
            let missing = fixture.temp.path().join("missing");

            // Act
            let result = fixture.store.contains(&missing);

            // Assert
            assert!(matches!(result, Err(FileStateStoreError::Canonicalize { .. })));
        }
    }

    mod list_all {
        use super::*;

        #[test]
        fn returns_empty_when_store_root_absent() {
            // Arrange
            let fixture = Fixture::new();

            // Act
            let result = fixture.store.list_all();

            // Assert
            assert_eq!(result.unwrap(), Vec::<PathBuf>::new());
        }

        #[test]
        fn returns_empty_when_store_has_no_entries() {
            // Arrange
            let fixture = Fixture::new();
            fs::create_dir_all(&fixture.store.root).expect("create store");

            // Act
            let result = fixture.store.list_all();

            // Assert
            assert_eq!(result.unwrap(), Vec::<PathBuf>::new());
        }

        #[test]
        fn returns_recorded_targets() {
            // Arrange
            let fixture = Fixture::new();
            let target1 = fixture.target("target1");
            let target2 = fixture.target("target2");
            fixture.store.record(&target1).expect("record 1");
            fixture.store.record(&target2).expect("record 2");

            // Act
            let result = fixture.store.list_all();

            // Assert
            let mut list = result.unwrap();
            list.sort();
            let mut expected = vec![
                target1.canonicalize().unwrap(),
                target2.canonicalize().unwrap(),
            ];
            expected.sort();
            assert_eq!(list, expected);
        }

        #[test]
        fn omits_entries_whose_targets_were_deleted() {
            // Arrange
            let fixture = Fixture::new();
            let kept = fixture.target("kept");
            let deleted = fixture.target("deleted");
            fixture.store.record(&kept).expect("record kept");
            fixture.store.record(&deleted).expect("record deleted");
            fs::remove_file(&deleted).expect("delete target");

            // Act
            let result = fixture.store.list_all();

            // Assert
            assert_eq!(result.unwrap(), vec![kept.canonicalize().unwrap()]);
        }
    }

    mod clean {
        use super::*;

        #[test]
        fn returns_zero_when_store_root_absent() {
            // Arrange
            let fixture = Fixture::new();

            // Act
            let result = fixture.store.clean(FileStoreCleanMode::EntriesOnly);

            // Assert
            assert_eq!(result.unwrap(), 0);
        }

        #[cfg(unix)]
        #[test]
        fn leaves_non_entry_files_untouched() {
            // Arrange
            let fixture = Fixture::new();
            fs::create_dir_all(&fixture.store.root).expect("create store root");
            let stray = fixture.store.root.join("stray");
            fs::write(&stray, "not a symlink").expect("write stray");

            // Act
            let result = fixture.store.clean(FileStoreCleanMode::EntriesOnly);

            // Assert
            assert_eq!(result.unwrap(), 0);
            assert!(stray.exists());
        }

        #[test]
        fn returns_count_of_removed_stale_entries() {
            // Arrange
            let fixture = Fixture::new();
            let kept = fixture.target("kept");
            let deleted = fixture.target("deleted");
            fixture.store.record(&kept).expect("record kept");
            fixture.store.record(&deleted).expect("record deleted");
            fs::remove_file(&deleted).expect("delete target");

            // Act
            let result = fixture.store.clean(FileStoreCleanMode::EntriesOnly);

            // Assert
            assert_eq!(result.unwrap(), 1);
        }

        #[test]
        fn removes_stale_entries_from_disk() {
            // Arrange
            let fixture = Fixture::new();
            let deleted = fixture.target("deleted");
            fixture.store.record(&deleted).expect("record deleted");
            let entry_path = fixture.entry_path_for(&deleted);
            fs::remove_file(&deleted).expect("delete target");

            // Act
            let result = fixture.store.clean(FileStoreCleanMode::EntriesOnly);

            // Assert
            assert!(result.is_ok());
            assert!(!entry_path.exists());
        }

        #[test]
        fn leaves_live_entries_untouched() {
            // Arrange
            let fixture = Fixture::new();
            let kept = fixture.target("kept");
            fixture.store.record(&kept).expect("record kept");
            let entry_path = fixture.entry_path_for(&kept);

            // Act
            let result = fixture.store.clean(FileStoreCleanMode::EntriesOnly);

            // Assert
            assert!(result.is_ok());
            assert!(entry_path.exists());
        }

        #[test]
        fn with_companions_removes_dangling_companion() {
            // Arrange
            let fixture = Fixture::new();
            let deleted = fixture.target("deleted");
            fixture.store.record(&deleted).expect("record deleted");
            fixture.store.write_companion(&deleted, ".hash", "content").expect("write companion");
            let companion = companion_path(&fixture.entry_path_for(&deleted), ".hash");
            fs::remove_file(&deleted).expect("delete target");

            // Act
            let result = fixture.store.clean(FileStoreCleanMode::WithCompanions(&[".hash"]));

            // Assert
            assert!(result.is_ok());
            assert!(!companion.exists());
        }

        #[test]
        fn with_companions_leaves_live_companion() {
            // Arrange
            let fixture = Fixture::new();
            let kept = fixture.target("kept");
            fixture.store.record(&kept).expect("record kept");
            fixture.store.write_companion(&kept, ".hash", "content").expect("write companion");
            let companion = companion_path(&fixture.entry_path_for(&kept), ".hash");

            // Act
            let result = fixture.store.clean(FileStoreCleanMode::WithCompanions(&[".hash"]));

            // Assert
            assert!(result.is_ok());
            assert!(companion.exists());
        }
    }

    mod remove {
        use super::*;

        #[test]
        fn returns_zero_when_entry_already_absent() {
            // Arrange
            let fixture = Fixture::new();
            let target = fixture.target("target");

            // Act
            let result = fixture.store.remove_with_companions(&target, &[".hash"]);

            // Assert
            assert_eq!(result.unwrap(), 0);
        }

        #[test]
        fn returns_one_when_entry_removed() {
            // Arrange
            let fixture = Fixture::new();
            let target = fixture.target("target");
            fixture.store.record(&target).expect("record");

            // Act
            let result = fixture.store.remove_with_companions(&target, &[".hash"]);

            // Assert
            assert_eq!(result.unwrap(), 1);
        }

        #[test]
        fn removes_entry_from_disk() {
            // Arrange
            let fixture = Fixture::new();
            let target = fixture.target("target");
            fixture.store.record(&target).expect("record");

            // Act
            let result = fixture.store.remove_with_companions(&target, &[]);

            // Assert
            assert!(result.is_ok());
            assert!(!fixture.entry_path_for(&target).exists());
        }

        #[test]
        fn removes_companions_from_disk() {
            // Arrange
            let fixture = Fixture::new();
            let target = fixture.target("target");
            fixture.store.record(&target).expect("record");
            fixture.store.write_companion(&target, ".hash", "content").expect("write");
            let companion = companion_path(&fixture.entry_path_for(&target), ".hash");

            // Act
            let result = fixture.store.remove_with_companions(&target, &[".hash"]);

            // Assert
            assert!(result.is_ok());
            assert!(!companion.exists());
        }

        #[test]
        fn returns_one_even_if_companions_already_absent() {
            // Arrange
            let fixture = Fixture::new();
            let target = fixture.target("target");
            fixture.store.record(&target).expect("record");

            // Act
            let result = fixture.store.remove_with_companions(&target, &[".hash"]);

            // Assert
            assert_eq!(result.unwrap(), 1);
        }
    }

    mod companions {
        use super::*;

        #[test]
        fn write_creates_companion_file() {
            // Arrange
            let fixture = Fixture::new();
            let target = fixture.target("target");
            fixture.store.record(&target).expect("record");

            // Act
            let result = fixture.store.write_companion(&target, ".hash", "content");

            // Assert
            assert!(result.is_ok());
            let companion = companion_path(&fixture.entry_path_for(&target), ".hash");
            assert_eq!(fs::read_to_string(companion).unwrap(), "content");
        }

        #[test]
        fn write_errors_when_store_root_absent() {
            // Arrange
            let fixture = Fixture::new();
            let target = fixture.target("target");
            // Do not record, so store.root does not exist

            // Act
            let result = fixture.store.write_companion(&target, ".hash", "content");

            // Assert
            assert!(matches!(
                result,
                Err(FileStateStoreError::CompanionWrite { .. })
            ));
        }

        #[test]
        fn read_returns_contents_when_present() {
            // Arrange
            let fixture = Fixture::new();
            let target = fixture.target("target");
            fixture.store.record(&target).expect("record");
            fixture.store.write_companion(&target, ".hash", "content").expect("write");

            // Act
            let result = fixture.store.read_companion(&target, ".hash");

            // Assert
            assert_eq!(result.unwrap(), Some("content".to_string()));
        }

        #[test]
        fn read_returns_none_when_absent() {
            // Arrange
            let fixture = Fixture::new();
            let target = fixture.target("target");
            fixture.store.record(&target).expect("record");

            // Act
            let result = fixture.store.read_companion(&target, ".hash");

            // Assert
            assert_eq!(result.unwrap(), None);
        }

        #[test]
        fn read_errors_on_nonexistent_target() {
            // Arrange
            let fixture = Fixture::new();
            let missing = fixture.temp.path().join("missing");

            // Act
            let result = fixture.store.read_companion(&missing, ".hash");

            // Assert
            assert!(matches!(
                result,
                Err(FileStateStoreError::Canonicalize { .. })
            ));
        }
    }
}