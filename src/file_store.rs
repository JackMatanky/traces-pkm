//! Store canonical file paths by path hash.
//!
//! Main types:
//! - [`FileStateStore`] - Hash-keyed store for canonical file paths
//! - [`FileStateStoreError`] - I/O failure from store operations
//! - [`FileStoreCleanMode`] - Cleanup policy for stale entries
//!
//! Entries are named with [`Blake3PathHash`]. Unix entries are symlinks whose
//! targets are the recorded paths. Windows entries are plain files whose
//! contents are the recorded paths.

use std::{
    fs, io,
    path::{Path, PathBuf},
};

use thiserror::Error;

use crate::{Blake3PathHash, DirTree, DirTreeError, dirs::StateDirRoot};

/// Reports a [`FileStateStore`] operation failure.
#[derive(Debug, Error)]
pub enum FileStateStoreError {
    /// Fails before hashing when a target path cannot be canonicalized.
    #[error("failed to canonicalize path {path}")]
    Canonicalize {
        /// Path that could not be canonicalized.
        path: PathBuf,
        /// Source I/O error.
        #[source]
        source: io::Error,
    },
    /// Fails while creating, checking, reading, or removing a store entry.
    #[error("file state store operation failed for {path}")]
    StoreIo {
        /// Path the failing operation targeted (a directory or an entry).
        path: PathBuf,
        /// Source I/O error.
        #[source]
        source: io::Error,
    },
    /// Fails while reading a companion file.
    #[error("failed to read companion file {path}")]
    CompanionRead {
        /// Companion file path.
        path: PathBuf,
        /// Source I/O error.
        #[source]
        source: io::Error,
    },
    /// Fails while writing a companion file.
    #[error("failed to write companion file {path}")]
    CompanionWrite {
        /// Companion file path.
        path: PathBuf,
        /// Source I/O error.
        #[source]
        source: io::Error,
    },
    /// Fails while removing a companion file.
    #[error("failed to remove companion file {path}")]
    CompanionRemove {
        /// Companion file path.
        path: PathBuf,
        /// Source I/O error.
        #[source]
        source: io::Error,
    },
}

/// Stores canonical file paths under hash-named entries.
///
/// Entry behavior by platform:
/// - Unix creates a symbolic link named by [`Blake3PathHash`] and pointing at
///   the canonical target path
/// - Windows writes a plain file named by [`Blake3PathHash`] whose contents are
///   the canonical target path bytes
///
/// Companion files are separate files with suffixes appended to the entry path.
#[derive(Clone, Debug)]
pub(crate) struct FileStateStore {
    root: StateDirRoot,
}

impl FileStateStore {
    /// Creates a store rooted at an arbitrary path for tests.
    #[cfg(any(test, feature = "test-utils"))]
    #[inline]
    #[must_use]
    pub(crate) fn at(root: PathBuf) -> Self {
        Self {
            root: StateDirRoot::from(root),
        }
    }

    /// Records `target`'s canonical path in this store.
    ///
    /// Canonicalizes `target`, hashes the canonical path, and creates the store
    /// root if needed. Recording an already-stored path is a no-op.
    ///
    /// Platform behavior:
    /// - Unix writes a symlink from the hash entry to the canonical target
    /// - Windows writes a file at the hash entry containing the canonical
    ///   target path bytes
    ///
    /// # Errors
    ///
    /// - [`FileStateStoreError::Canonicalize`] if `target` cannot be
    ///   canonicalized
    /// - [`FileStateStoreError::StoreIo`] if the store root cannot be created
    ///   or the entry cannot be written
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
            entry.canonical_target.as_os_str().as_encoded_bytes(),
        );

        write_entry.map_err(|source| FileStateStoreError::StoreIo {
            path: entry_path,
            source,
        })
    }

    /// Checks whether `target` has an entry in this store.
    ///
    /// Canonicalizes `target` before hashing, so relative spellings of the same
    /// existing path check the same entry. The platform entry format is the
    /// same as [`Self::record`].
    ///
    /// # Errors
    ///
    /// - [`FileStateStoreError::Canonicalize`] if `target` cannot be
    ///   canonicalized
    /// - [`FileStateStoreError::StoreIo`] if the entry's existence cannot be
    ///   checked
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
    /// Reads recorded targets from symlinks on Unix and path-bearing files on
    /// Windows. A live entry has a readable target that still exists on disk.
    /// Dangling or unreadable entries are omitted. An absent or non-directory
    /// root returns an empty list.
    ///
    /// # Errors
    ///
    /// - [`FileStateStoreError::StoreIo`] if the store root exists but cannot
    ///   be read or a child entry cannot be inspected
    #[inline]
    pub(crate) fn list_all(&self) -> Result<Vec<PathBuf>, FileStateStoreError> {
        if !self.root.is_dir() {
            return Ok(Vec::new());
        }
        let mut targets = Vec::new();
        for node in DirTree::children(&self.root) {
            let node = node.map_err(store_error)?;
            let entry = node.path().to_path_buf();
            if let Some(target) = recorded_target(&entry)
                && target.exists()
            {
                targets.push(target);
            }
        }
        Ok(targets)
    }

    /// Removes stale entries and returns the number removed.
    ///
    /// Reads recorded targets from symlinks on Unix and path-bearing files on
    /// Windows. A stale entry has a readable target that no longer exists.
    /// Unreadable entries are ignored. An absent or non-directory root removes
    /// nothing.
    ///
    /// # Errors
    ///
    /// - [`FileStateStoreError::StoreIo`] if the store root cannot be read, a
    ///   child entry cannot be inspected, or a stale entry cannot be removed
    /// - [`FileStateStoreError::CompanionRemove`] if a companion file cannot be
    ///   removed
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
    /// Canonicalizes and hashes `target` to find the same entry path used by
    /// [`Self::record`]. The companion path is independent of whether the entry
    /// itself is a Unix symlink or a Windows path-bearing file.
    ///
    /// # Errors
    ///
    /// - [`FileStateStoreError::Canonicalize`] if `target` cannot be
    ///   canonicalized
    /// - [`FileStateStoreError::CompanionWrite`] if the companion cannot be
    ///   written
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

    /// Reads a companion file next to `target`'s store entry.
    ///
    /// Canonicalizes and hashes `target` to find the same entry path used by
    /// [`Self::record`]. Returns `Ok(None)` when the companion file is absent.
    /// The companion path is independent of whether the entry itself is a Unix
    /// symlink or a Windows path-bearing file.
    ///
    /// # Errors
    ///
    /// - [`FileStateStoreError::Canonicalize`] if `target` cannot be
    ///   canonicalized
    /// - [`FileStateStoreError::CompanionRead`] if the companion exists but
    ///   cannot be read
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

    /// Removes `target`'s entry and its companion files.
    ///
    /// Canonicalizes and hashes `target` to find the same entry path used by
    /// [`Self::record`]. Removes the Unix symlink or Windows path-bearing file,
    /// then removes any listed companions. Returns `1` when the root entry was
    /// removed and `0` when it was already absent.
    ///
    /// # Errors
    ///
    /// - [`FileStateStoreError::Canonicalize`] if `target` cannot be
    ///   canonicalized
    /// - [`FileStateStoreError::StoreIo`] if the root entry exists but cannot
    ///   be removed
    /// - [`FileStateStoreError::CompanionRemove`] if a companion file cannot be
    ///   removed
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

    /// Removes stale entries and returns each removed root entry path.
    ///
    /// # Errors
    ///
    /// - [`FileStateStoreError`] if the store root cannot be read or a stale
    ///   entry cannot be removed.
    fn clean_reporting(&self) -> Result<Vec<PathBuf>, FileStateStoreError> {
        if !self.root.is_dir() {
            return Ok(Vec::new());
        }
        let mut removed = Vec::new();
        for node in DirTree::children(&self.root) {
            let node = node.map_err(store_error)?;
            let entry = node.path().to_path_buf();
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

impl From<StateDirRoot> for FileStateStore {
    #[inline]
    fn from(root: StateDirRoot) -> Self {
        Self {
            root,
        }
    }
}

/// Selects how [`FileStateStore::clean`] removes stale data.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum FileStoreCleanMode<'a> {
    /// Removes only stale hash entries.
    EntriesOnly,
    /// Removes stale hash entries and companion files for each suffix.
    ///
    /// A suffix is appended to the hash entry path, such as `.hash` producing
    /// `<entry>.hash`.
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
        let hash = Blake3PathHash::from(canonical_target.as_path());
        Ok(Self {
            canonical_target,
            hash,
        })
    }
}

fn companion_path(entry: &Path, suffix: &str) -> PathBuf {
    let mut name = entry.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

/// Reads `entry`'s recorded target path.
///
/// Returns `None` if `entry` was not written by [`FileStateStore::record`] or
/// cannot be read.
fn recorded_target(entry: &Path) -> Option<PathBuf> {
    #[cfg(unix)]
    let target = fs::read_link(entry);
    #[cfg(windows)]
    let target = fs::read_to_string(entry).map(PathBuf::from);
    target.ok()
}

/// Converts a [`DirTreeError`] into a [`FileStateStoreError::StoreIo`].
fn store_error(error: DirTreeError) -> FileStateStoreError {
    let (path, source) = error.into_parts();
    FileStateStoreError::StoreIo {
        path,
        source,
    }
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
            Self {
                temp,
                store,
            }
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
        use pretty_assertions::assert_eq;

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
            assert!(matches!(
                result,
                Err(FileStateStoreError::Canonicalize { .. })
            ));
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
            assert!(matches!(
                result,
                Err(FileStateStoreError::Canonicalize { .. })
            ));
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
        use pretty_assertions::assert_eq;

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
            assert!(matches!(
                result,
                Err(FileStateStoreError::Canonicalize { .. })
            ));
        }
    }

    mod list_all {
        use pretty_assertions::assert_eq;

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
        use pretty_assertions::assert_eq;

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
            fixture
                .store
                .write_companion(&deleted, ".hash", "content")
                .expect("write companion");
            let companion =
                companion_path(&fixture.entry_path_for(&deleted), ".hash");
            fs::remove_file(&deleted).expect("delete target");

            // Act
            let result = fixture
                .store
                .clean(FileStoreCleanMode::WithCompanions(&[".hash"]));

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
            fixture
                .store
                .write_companion(&kept, ".hash", "content")
                .expect("write companion");
            let companion =
                companion_path(&fixture.entry_path_for(&kept), ".hash");

            // Act
            let result = fixture
                .store
                .clean(FileStoreCleanMode::WithCompanions(&[".hash"]));

            // Assert
            assert!(result.is_ok());
            assert!(companion.exists());
        }

        #[test]
        fn clean_succeeds_when_companion_already_removed() {
            // Arrange
            let fixture = Fixture::new();
            let target = fixture.target("target");
            fixture.store.record(&target).expect("record");
            let entry = fixture.entry_path_for(&target);
            let companion = companion_path(&entry, ".hash");
            fs::write(&companion, "hash").expect("write companion");
            fs::remove_file(&companion).expect("delete companion");
            fs::remove_file(&target).expect("delete target");

            // Act
            let result = fixture
                .store
                .clean(FileStoreCleanMode::WithCompanions(&[".hash"]));

            // Assert
            assert!(
                result.is_ok(),
                "clean must succeed when companion is missing: {result:?}"
            );
            assert_eq!(result.unwrap(), 1);
        }
    }

    mod remove {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn returns_zero_when_entry_already_absent() {
            // Arrange
            let fixture = Fixture::new();
            let target = fixture.target("target");

            // Act
            let result =
                fixture.store.remove_with_companions(&target, &[".hash"]);

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
            let result =
                fixture.store.remove_with_companions(&target, &[".hash"]);

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
            fixture
                .store
                .write_companion(&target, ".hash", "content")
                .expect("write");
            let companion =
                companion_path(&fixture.entry_path_for(&target), ".hash");

            // Act
            let result =
                fixture.store.remove_with_companions(&target, &[".hash"]);

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
            let result =
                fixture.store.remove_with_companions(&target, &[".hash"]);

            // Assert
            assert_eq!(result.unwrap(), 1);
        }
    }

    mod companions {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn write_creates_companion_file() {
            // Arrange
            let fixture = Fixture::new();
            let target = fixture.target("target");
            fixture.store.record(&target).expect("record");

            // Act
            let result =
                fixture.store.write_companion(&target, ".hash", "content");

            // Assert
            assert!(result.is_ok());
            let companion =
                companion_path(&fixture.entry_path_for(&target), ".hash");
            assert_eq!(fs::read_to_string(companion).unwrap(), "content");
        }

        #[test]
        fn write_errors_when_store_root_absent() {
            // Arrange
            let fixture = Fixture::new();
            let target = fixture.target("target");
            // Do not record, so store.root does not exist

            // Act
            let result =
                fixture.store.write_companion(&target, ".hash", "content");

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
            fixture
                .store
                .write_companion(&target, ".hash", "content")
                .expect("write");

            // Act
            let result = fixture.store.read_companion(&target, ".hash");

            // Assert
            assert_eq!(result.unwrap(), Some("content".to_owned()));
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
