//! `FileIndex`: a persisted cache of [`FileRecord`]s for every file under a
//! trusted project root.
//!
//! Persistence is redb-backed (see [`store`]) but that detail stays behind
//! [`FileIndex`] — callers (`cli`, later `template`) only ever see
//! build/persist/load/enumerate.

#![cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "load(), records() accessors, and most FileRecord fields are \
                  exercised by tests now but only get a non-test caller once \
                  the query surface (tickets 02-04) lands; `traces index` \
                  itself only needs build+persist"
    )
)]

use std::path::Path;

pub(crate) use domain::FileRecord;
pub(crate) use error::FileIndexError;
use store::IndexStore;

mod domain;
mod error;
mod scan;
mod store;

/// The persisted `FileIndex` database's path, relative to a project root.
const INDEX_FILE: &str = ".traces/index.redb";

/// A persisted cache of File Records for every file under a project root.
#[derive(Debug)]
pub(crate) struct FileIndex {
    records: Vec<FileRecord>,
}

impl FileIndex {
    /// Scans `root` and builds a `FileIndex` in memory. Does not persist —
    /// call [`Self::persist`] to write it to disk.
    ///
    /// # Errors
    ///
    /// Returns [`FileIndexError::Io`] if a directory cannot be read or a
    /// file's metadata cannot be inspected.
    #[inline]
    pub(crate) fn build(root: &Path) -> Result<Self, FileIndexError> {
        Ok(Self {
            records: scan::scan_root(root)?,
        })
    }

    /// Persists this `FileIndex`'s File Records to `root`'s index database,
    /// replacing any previously persisted contents.
    ///
    /// # Errors
    ///
    /// - [`FileIndexError::Io`] if the database's parent directory cannot be
    ///   created
    /// - [`FileIndexError::Store`] if the database transaction fails
    /// - [`FileIndexError::Serialize`] if a record cannot be encoded
    #[inline]
    pub(crate) fn persist(&self, root: &Path) -> Result<(), FileIndexError> {
        IndexStore::open(root)?.replace_all(&self.records)
    }

    /// Loads the `FileIndex` previously persisted for `root`. Returns an empty
    /// `FileIndex` if none was ever persisted.
    ///
    /// # Errors
    ///
    /// - [`FileIndexError::Store`] if the database cannot be read
    /// - [`FileIndexError::Corrupt`] if stored bytes aren't valid UTF-8
    /// - [`FileIndexError::Deserialize`] if stored text isn't a valid
    ///   [`FileRecord`]
    #[inline]
    pub(crate) fn load(root: &Path) -> Result<Self, FileIndexError> {
        Ok(Self {
            records: IndexStore::open(root)?.load_all()?,
        })
    }

    /// Every indexed File Record, sorted by path.
    #[inline]
    #[must_use]
    pub(crate) fn records(&self) -> &[FileRecord] {
        &self.records
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    mod build {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn finds_every_file_under_root() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::create_dir_all(temp.path().join("notes")).expect("mkdir notes");
            fs::write(temp.path().join("notes/todo.md"), "content")
                .expect("write note");
            fs::write(temp.path().join("readme.md"), "content")
                .expect("write readme");

            let index = FileIndex::build(temp.path()).expect("build index");

            assert_eq!(index.records().len(), 2);
        }
    }

    mod persistence {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn persist_then_load_recovers_the_same_records() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("note.md"), "content")
                .expect("write note");
            let built = FileIndex::build(temp.path()).expect("build index");
            built.persist(temp.path()).expect("persist index");

            let loaded = FileIndex::load(temp.path()).expect("load index");

            assert_eq!(loaded.records(), built.records());
        }

        #[test]
        fn returns_an_empty_index_when_the_root_was_never_persisted() {
            let temp = tempfile::tempdir().expect("create temp dir");

            let index = FileIndex::load(temp.path()).expect("load index");

            assert_eq!(index.records().len(), 0);
        }

        #[test]
        fn rebuilds_rather_than_appends_when_persisted_again() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("first.md"), "content")
                .expect("write first");
            FileIndex::build(temp.path())
                .expect("build first index")
                .persist(temp.path())
                .expect("persist first index");
            fs::remove_file(temp.path().join("first.md"))
                .expect("remove first");
            fs::write(temp.path().join("second.md"), "content")
                .expect("write second");

            FileIndex::build(temp.path())
                .expect("build second index")
                .persist(temp.path())
                .expect("persist second index");
            let loaded = FileIndex::load(temp.path()).expect("load index");

            assert_eq!(loaded.records().len(), 1);
            assert_eq!(
                loaded.records().first().map(FileRecord::path),
                Some(Path::new("second.md"))
            );
        }
    }
}
