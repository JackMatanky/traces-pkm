//! Shared redb-backed persistence primitives for project-root-scoped domain
//! data.
//!
//! [`DbStore`] owns one redb connection under a project root and the generic
//! table store/load mechanics every domain module builds its own table-specific
//! persistence on top of. Table definitions and their read/write semantics stay
//! with the domain that owns them (see `index::store` for File/Note/Inlink
//! tables); this module owns only "open the file, run a transaction,
//! serialize/deserialize a value or multimap table" — mechanics with no domain
//! knowledge.

use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
};

use redb::{
    MultimapTableDefinition, ReadTransaction, ReadableDatabase as _,
    ReadableMultimapTable as _, ReadableTable as _, TableDefinition,
    WriteTransaction,
};
use serde::{Serialize, de::DeserializeOwned};
use thiserror::Error;

/// Generic error type for low-level redb persistence operations.
#[derive(Debug, Error)]
pub(crate) enum StoreError {
    /// A filesystem operation failed during directory creation or file setup.
    #[error("failed to access {path}")]
    Io {
        /// The path that could not be accessed.
        path: PathBuf,
        /// Source I/O error.
        #[source]
        source: std::io::Error,
    },
    /// Opening, reading, or writing the redb-backed database failed.
    #[error("failed to access the database at {path}")]
    Redb {
        /// The database file path.
        path: PathBuf,
        /// Source redb error.
        #[source]
        source: Box<redb::Error>,
    },
    /// A record could not be serialized.
    #[error("failed to serialize the record for {path}")]
    Serialize {
        /// The record's project-relative path.
        path: PathBuf,
        /// Source postcard serialization error.
        #[source]
        source: postcard::Error,
    },
    /// A stored record could not be deserialized.
    #[error("failed to deserialize the record for {path}")]
    Deserialize {
        /// The record's project-relative path.
        path: PathBuf,
        /// Source postcard deserialization error.
        #[source]
        source: postcard::Error,
    },
}

/// Redb database handle for a project root.
#[derive(Debug)]
pub(crate) struct DbStore {
    db: redb::Database,
    path: PathBuf,
}

impl DbStore {
    /// Opens or creates a redb database at `root.join(relative_path)`.
    ///
    /// # Errors
    ///
    /// - [`StoreError::Io`] if the database's parent directory cannot be
    ///   created.
    /// - [`StoreError::Redb`] if the database file cannot be opened or created.
    pub(crate) fn open(
        root: &Path,
        relative_path: &str,
    ) -> Result<Self, StoreError> {
        let path = root.join(relative_path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|source| StoreError::Io {
                path: parent.to_path_buf(),
                source,
            })?;
        }
        let db = redb::Database::create(&path).map_err(|source| {
            StoreError::Redb {
                path: path.clone(),
                source: Box::new(source.into()),
            }
        })?;
        Ok(Self {
            db,
            path,
        })
    }

    /// Returns a reference to the database path.
    #[inline]
    #[expect(
        dead_code,
        reason = "documented API for future domain table stores (e.g. \
                  task-system LISTS table)"
    )]
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    /// Begins a read transaction.
    ///
    /// # Errors
    ///
    /// - [`StoreError::Redb`] if the transaction cannot be started.
    pub(crate) fn begin_read(&self) -> Result<ReadTransaction, StoreError> {
        self.db.begin_read().map_err(|source| self.store_error(source))
    }

    /// Begins a write transaction.
    ///
    /// # Errors
    ///
    /// - [`StoreError::Redb`] if the transaction cannot be started.
    pub(crate) fn begin_write(&self) -> Result<WriteTransaction, StoreError> {
        self.db.begin_write().map_err(|source| self.store_error(source))
    }

    /// Serializes `items` with postcard into `table`, keyed by `path_of`.
    ///
    /// # Errors
    ///
    /// - [`StoreError::Redb`] if the table cannot be opened or written.
    /// - [`StoreError::Serialize`] if an item cannot be postcard-encoded.
    pub(crate) fn store_table<T: Serialize>(
        &self,
        write_txn: &WriteTransaction,
        table: TableDefinition<&str, &[u8]>,
        items: &[T],
        path_of: impl Fn(&T) -> &Path,
    ) -> Result<(), StoreError> {
        let mut table = write_txn
            .open_table(table)
            .map_err(|source| self.store_error(source))?;
        for item in items {
            let path = path_of(item);
            let key = path.to_string_lossy();
            let value = postcard::to_allocvec(item).map_err(|source| {
                StoreError::Serialize {
                    path: path.to_path_buf(),
                    source,
                }
            })?;
            table
                .insert(&*key, value.as_slice())
                .map_err(|source| self.store_error(source))?;
        }
        Ok(())
    }

    /// Deserializes every postcard value in `table` and sorts by `path_of`.
    ///
    /// # Errors
    ///
    /// - [`StoreError::Redb`] if the table cannot be read.
    /// - [`StoreError::Deserialize`] if stored bytes are corrupt or
    ///   incompatible.
    pub(crate) fn load_table<T: DeserializeOwned>(
        &self,
        read_txn: &ReadTransaction,
        table: TableDefinition<&str, &[u8]>,
        path_of: impl Fn(&T) -> &Path,
    ) -> Result<Vec<T>, StoreError> {
        let mut items: Vec<T> = match read_txn.open_table(table) {
            Ok(table) => {
                let mut items = Vec::new();
                for entry in
                    table.iter().map_err(|source| self.store_error(source))?
                {
                    let (key, value) =
                        entry.map_err(|source| self.store_error(source))?;
                    items.push(postcard::from_bytes(value.value()).map_err(
                        |source| StoreError::Deserialize {
                            path: PathBuf::from(key.value()),
                            source,
                        },
                    )?);
                }
                items
            }
            Err(redb::TableError::TableDoesNotExist(_)) => Vec::new(),
            Err(source) => return Err(self.store_error(source)),
        };
        items.sort_by(|a, b| path_of(a).cmp(path_of(b)));
        Ok(items)
    }

    /// Serializes every `target -> sources` edge into the `links` multimap
    /// table.
    ///
    /// # Errors
    ///
    /// - [`StoreError::Redb`] if the table cannot be opened or written.
    pub(crate) fn store_links(
        &self,
        write_txn: &WriteTransaction,
        table: MultimapTableDefinition<&str, &str>,
        links: &HashMap<PathBuf, Vec<PathBuf>>,
    ) -> Result<(), StoreError> {
        let mut table = write_txn
            .open_multimap_table(table)
            .map_err(|source| self.store_error(source))?;
        for (target, sources) in links {
            let target_key = target.to_string_lossy();
            for source in sources {
                table
                    .insert(&*target_key, &*source.to_string_lossy())
                    .map_err(|source| self.store_error(source))?;
            }
        }
        Ok(())
    }

    /// Deserializes every `target -> sources` edge from the `links` multimap
    /// table.
    ///
    /// # Errors
    ///
    /// - [`StoreError::Redb`] if the table cannot be read.
    pub(crate) fn load_links(
        &self,
        read_txn: &ReadTransaction,
        table: MultimapTableDefinition<&str, &str>,
    ) -> Result<HashMap<PathBuf, Vec<PathBuf>>, StoreError> {
        let table = match read_txn.open_multimap_table(table) {
            Ok(table) => table,
            Err(redb::TableError::TableDoesNotExist(_)) => {
                return Ok(HashMap::new());
            }
            Err(source) => return Err(self.store_error(source)),
        };
        let mut links = HashMap::new();
        for entry in table.iter().map_err(|source| self.store_error(source))? {
            let (target, sources) =
                entry.map_err(|source| self.store_error(source))?;
            let mut values = Vec::new();
            for source in sources {
                let source =
                    source.map_err(|source| self.store_error(source))?;
                values.push(PathBuf::from(source.value()));
            }
            links.insert(PathBuf::from(target.value()), values);
        }
        Ok(links)
    }

    /// Wraps a redb error with this store's database path.
    pub(crate) fn store_error(
        &self,
        source: impl Into<redb::Error>,
    ) -> StoreError {
        StoreError::Redb {
            path: self.path.clone(),
            source: Box::new(source.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    const TEST_TABLE: TableDefinition<&str, &[u8]> =
        TableDefinition::new("test_table");

    #[test]
    fn persists_records_as_postcard_bytes_not_toml_text() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let db = DbStore::open(temp.path(), "test.redb").expect("open db");
        let write_txn = db.begin_write().expect("begin write");
        db.store_table(&write_txn, TEST_TABLE, &["hello".to_owned()], |s| {
            Path::new(s.as_str())
        })
        .expect("store table");
        write_txn.commit().expect("commit");

        let read_txn = db.begin_read().expect("begin read");
        let loaded: Vec<String> = db
            .load_table(&read_txn, TEST_TABLE, |s: &String| {
                Path::new(s.as_str())
            })
            .expect("load table");
        assert_eq!(loaded, vec!["hello".to_owned()]);
    }

    #[test]
    fn returns_deserialize_error_when_stored_bytes_are_invalid() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let db = DbStore::open(temp.path(), "test.redb").expect("open db");
        let write_txn = db.begin_write().expect("begin write");
        {
            let mut table =
                write_txn.open_table(TEST_TABLE).expect("open table");
            table
                .insert("corrupt.md", [0xFF, 0xFF].as_slice())
                .expect("insert corrupt");
        }
        write_txn.commit().expect("commit");

        let read_txn = db.begin_read().expect("begin read");
        let result: Result<Vec<String>, StoreError> =
            db.load_table(&read_txn, TEST_TABLE, |s: &String| {
                Path::new(s.as_str())
            });

        assert!(matches!(result, Err(StoreError::Deserialize { .. })));
    }
}
