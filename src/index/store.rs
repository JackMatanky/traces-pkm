//! redb-backed persistence for [`FileRecord`]s, keyed by project-relative
//! path.
//!
//! The sole seam that knows about redb tables — [`super::FileIndex`] and its
//! callers never see a [`TableDefinition`] or transaction.

use std::{
    fs,
    path::{Path, PathBuf},
    str,
};

use redb::{
    Database, ReadableDatabase as _, ReadableTable as _, TableDefinition,
};

use super::{INDEX_FILE, domain::FileRecord, error::FileIndexError};

/// Path → TOML-encoded [`FileRecord`] bytes.
///
/// A byte-slice value (not a typed redb value) keeps the redb schema
/// independent of `FileRecord`'s shape; encoding is TOML because `toml` is
/// already a dependency and File Records are small, infrequently-written
/// values where a self-describing text format costs nothing observable.
const FILE_RECORDS: TableDefinition<&str, &[u8]> =
    TableDefinition::new("file_records");

/// Opens the redb database backing one project root's `FileIndex`.
pub(super) struct IndexStore {
    db: Database,
    /// The database's own path, kept for error context.
    path: PathBuf,
}

impl IndexStore {
    /// Opens (creating if absent) the index database under `root`.
    ///
    /// # Errors
    ///
    /// - [`FileIndexError::Io`] if the database's parent directory cannot be
    ///   created
    /// - [`FileIndexError::Store`] if the database file cannot be opened
    pub(super) fn open(root: &Path) -> Result<Self, FileIndexError> {
        let path = root.join(INDEX_FILE);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|source| {
                FileIndexError::Io {
                    path: parent.to_path_buf(),
                    source,
                }
            })?;
        }
        let db = Database::create(&path).map_err(|source| {
            FileIndexError::Store {
                path: path.clone(),
                source: Box::new(source.into()),
            }
        })?;
        Ok(Self {
            db,
            path,
        })
    }

    /// Replaces every stored File Record with `records`.
    ///
    /// # Errors
    ///
    /// - [`FileIndexError::Store`] if the transaction fails
    /// - [`FileIndexError::Serialize`] if a record cannot be TOML-encoded
    pub(super) fn replace_all(
        &self,
        records: &[FileRecord],
    ) -> Result<(), FileIndexError> {
        let write_txn =
            self.db.begin_write().map_err(|source| self.store_error(source))?;
        write_txn
            .delete_table(FILE_RECORDS)
            .map_err(|source| self.store_error(source))?;
        {
            let mut table = write_txn
                .open_table(FILE_RECORDS)
                .map_err(|source| self.store_error(source))?;
            for record in records {
                let key = record.path().to_string_lossy();
                let value = toml::to_string(record).map_err(|source| {
                    FileIndexError::Serialize {
                        path: record.path().to_path_buf(),
                        source,
                    }
                })?;
                table
                    .insert(&*key, value.as_bytes())
                    .map_err(|source| self.store_error(source))?;
            }
        }
        write_txn.commit().map_err(|source| self.store_error(source))
    }

    /// Loads every stored File Record.
    ///
    /// # Errors
    ///
    /// - [`FileIndexError::Store`] if the table cannot be read
    /// - [`FileIndexError::Corrupt`] if stored bytes aren't valid UTF-8
    /// - [`FileIndexError::Deserialize`] if stored text isn't a valid
    ///   [`FileRecord`]
    pub(super) fn load_all(&self) -> Result<Vec<FileRecord>, FileIndexError> {
        let read_txn =
            self.db.begin_read().map_err(|source| self.store_error(source))?;
        let table = match read_txn.open_table(FILE_RECORDS) {
            Ok(table) => table,
            Err(redb::TableError::TableDoesNotExist(_)) => {
                return Ok(Vec::new());
            }
            Err(source) => return Err(self.store_error(source)),
        };

        let mut records = Vec::new();
        for entry in table.iter().map_err(|source| self.store_error(source))? {
            let (_, value) =
                entry.map_err(|source| self.store_error(source))?;
            let text = str::from_utf8(value.value()).map_err(|source| {
                FileIndexError::Corrupt {
                    source,
                }
            })?;
            records.push(toml::from_str(text).map_err(|source| {
                FileIndexError::Deserialize {
                    source,
                }
            })?);
        }
        Ok(records)
    }

    /// Wraps a redb error with this store's database path.
    fn store_error(&self, source: impl Into<redb::Error>) -> FileIndexError {
        FileIndexError::Store {
            path: self.path.clone(),
            source: Box::new(source.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::scan::scan_root;

    mod persistence {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn load_all_on_a_freshly_opened_database_is_empty() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let store = IndexStore::open(temp.path()).expect("open store");

            let records = store.load_all().expect("load empty database");

            assert_eq!(records.len(), 0);
        }

        #[test]
        fn replace_all_then_load_all_round_trips_records() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("note.md"), "content")
                .expect("write note");
            let records = scan_root(temp.path()).expect("scan root");
            let store = IndexStore::open(temp.path()).expect("open store");

            store.replace_all(&records).expect("persist records");
            let loaded = store.load_all().expect("load records");

            assert_eq!(loaded, records);
        }

        #[test]
        fn replace_all_drops_records_absent_from_the_new_set() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("stale.md"), "old")
                .expect("write stale");
            let stale = scan_root(temp.path()).expect("scan stale");
            let store = IndexStore::open(temp.path()).expect("open store");
            store.replace_all(&stale).expect("persist stale");
            fs::remove_file(temp.path().join("stale.md"))
                .expect("remove stale");
            fs::write(temp.path().join("fresh.md"), "new")
                .expect("write fresh");
            let fresh = scan_root(temp.path()).expect("scan fresh");

            store.replace_all(&fresh).expect("persist fresh");
            let loaded = store.load_all().expect("load records");

            assert_eq!(loaded, fresh);
        }

        #[test]
        fn replace_all_with_no_records_persists_an_empty_table() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let store = IndexStore::open(temp.path()).expect("open store");

            store.replace_all(&[]).expect("persist an empty record set");
            let loaded = store.load_all().expect("load records");

            assert_eq!(loaded.len(), 0);
        }

        #[test]
        fn round_trips_a_record_with_a_unicode_path() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("café ☕.md"), "content")
                .expect("write unicode-named file");
            let records = scan_root(temp.path()).expect("scan root");
            let store = IndexStore::open(temp.path()).expect("open store");

            store.replace_all(&records).expect("persist records");
            let loaded = store.load_all().expect("load records");

            assert_eq!(loaded, records);
        }
    }

    mod load_all {
        use super::*;

        /// Bypasses [`IndexStore::replace_all`] to write a raw, possibly
        /// invalid value directly into the `file_records` table - the only
        /// way to deterministically simulate a corrupted index database.
        fn write_raw_value(store: &IndexStore, key: &str, value: &[u8]) {
            let write_txn = store.db.begin_write().expect("begin write txn");
            {
                let mut table =
                    write_txn.open_table(FILE_RECORDS).expect("open table");
                table.insert(key, value).expect("insert raw bytes");
            }
            write_txn.commit().expect("commit raw insert");
        }

        #[test]
        fn returns_corrupt_when_stored_bytes_are_not_utf8() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let store = IndexStore::open(temp.path()).expect("open store");
            write_raw_value(&store, "bad.md", &[0xFF, 0xFE]);

            let error =
                store.load_all().expect_err("non-UTF8 bytes fail to load");

            assert!(matches!(error, FileIndexError::Corrupt { .. }));
        }

        #[test]
        fn returns_deserialize_error_when_stored_text_is_not_valid_toml() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let store = IndexStore::open(temp.path()).expect("open store");
            write_raw_value(&store, "bad.md", b"not valid toml {{{");

            let error =
                store.load_all().expect_err("invalid TOML text fails to load");

            assert!(matches!(error, FileIndexError::Deserialize { .. }));
        }
    }
}
