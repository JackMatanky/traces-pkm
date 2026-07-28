//! redb-backed persistence for [`FileRecord`]s and [`Note`]s, keyed by
//! project-relative path.
//!
//! The sole seam that knows about redb tables — [`super::FileIndex`] and its
//! callers never see a [`TableDefinition`] or transaction.

use std::{
    fs,
    path::{Path, PathBuf},
    str,
};

use redb::{
    Database, ReadTransaction, ReadableDatabase as _, ReadableTable as _,
    TableDefinition, WriteTransaction,
};
use serde::{Serialize, de::DeserializeOwned};

use super::{
    INDEX_FILE, error::FileIndexError, file::FileRecord, markdown::Note,
};

/// Path → TOML-encoded [`FileRecord`] bytes.
const FILE_RECORDS: TableDefinition<&str, &[u8]> =
    TableDefinition::new("file_records");

/// Path → TOML-encoded [`Note`] bytes.
const NOTES: TableDefinition<&str, &[u8]> = TableDefinition::new("notes");

/// An atomically read snapshot of every File Record and Note Record
/// persisted in the index database, sorted by path.
type IndexSnapshot = (Vec<FileRecord>, Vec<Note>);

/// Redb-backed handle to one project root's index database.
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

    /// Replaces every stored File Record with `records` and Note Record with
    /// `notes`.
    ///
    /// # Errors
    ///
    /// - [`FileIndexError::Store`] if the transaction fails
    /// - [`FileIndexError::Serialize`] if a record cannot be TOML-encoded
    pub(super) fn replace_all(
        &self,
        records: &[FileRecord],
        notes: &[Note],
    ) -> Result<(), FileIndexError> {
        let write_txn =
            self.db.begin_write().map_err(|source| self.store_error(source))?;
        write_txn
            .delete_table(FILE_RECORDS)
            .map_err(|source| self.store_error(source))?;
        write_txn
            .delete_table(NOTES)
            .map_err(|source| self.store_error(source))?;
        self.store_table(&write_txn, FILE_RECORDS, records, FileRecord::path)?;
        self.store_table(&write_txn, NOTES, notes, Note::path)?;
        write_txn.commit().map_err(|source| self.store_error(source))
    }

    /// Loads every stored File Record and Note Record, sorted by path for
    /// deterministic output.
    ///
    /// # Errors
    ///
    /// - [`FileIndexError::Store`] if the table cannot be read
    /// - [`FileIndexError::Corrupt`] if stored bytes aren't valid UTF-8
    /// - [`FileIndexError::Deserialize`] if stored text isn't valid UTF-8/TOML
    pub(super) fn load_all(&self) -> Result<IndexSnapshot, FileIndexError> {
        let read_txn =
            self.db.begin_read().map_err(|source| self.store_error(source))?;
        let records =
            self.load_table(&read_txn, FILE_RECORDS, FileRecord::path)?;
        let notes = self.load_table(&read_txn, NOTES, Note::path)?;
        Ok((records, notes))
    }

    /// Serializes `items` as TOML into `table`, keyed by `path_of`.
    ///
    /// # Errors
    ///
    /// - [`FileIndexError::Store`] if the table cannot be opened or written
    /// - [`FileIndexError::Serialize`] if an item cannot be TOML-encoded
    fn store_table<T: Serialize>(
        &self,
        write_txn: &WriteTransaction,
        table: TableDefinition<&str, &[u8]>,
        items: &[T],
        path_of: impl Fn(&T) -> &Path,
    ) -> Result<(), FileIndexError> {
        let mut table = write_txn
            .open_table(table)
            .map_err(|source| self.store_error(source))?;
        for item in items {
            let path = path_of(item);
            let key = path.to_string_lossy();
            let value = toml::to_string(item).map_err(|source| {
                FileIndexError::Serialize {
                    path: path.to_path_buf(),
                    source,
                }
            })?;
            table
                .insert(&*key, value.as_bytes())
                .map_err(|source| self.store_error(source))?;
        }
        Ok(())
    }

    /// Deserializes every TOML value in `table` into a `Vec<T>`, sorted by
    /// `path_of` for deterministic output.
    ///
    /// # Errors
    ///
    /// - [`FileIndexError::Store`] if the table cannot be read
    /// - [`FileIndexError::Corrupt`] if stored bytes aren't valid UTF-8
    /// - [`FileIndexError::Deserialize`] if stored text isn't valid TOML
    fn load_table<T: DeserializeOwned>(
        &self,
        read_txn: &ReadTransaction,
        table: TableDefinition<&str, &[u8]>,
        path_of: impl Fn(&T) -> &Path,
    ) -> Result<Vec<T>, FileIndexError> {
        let mut items: Vec<T> = match read_txn.open_table(table) {
            Ok(table) => {
                let mut items = Vec::new();
                for entry in
                    table.iter().map_err(|source| self.store_error(source))?
                {
                    let (key, value) =
                        entry.map_err(|source| self.store_error(source))?;
                    let path = PathBuf::from(key.value());
                    let text =
                        str::from_utf8(value.value()).map_err(|source| {
                            FileIndexError::Corrupt {
                                path: path.clone(),
                                source,
                            }
                        })?;
                    items.push(toml::from_str(text).map_err(|source| {
                        FileIndexError::Deserialize {
                            path,
                            source: Box::new(source),
                        }
                    })?);
                }
                items
            }
            Err(redb::TableError::TableDoesNotExist(_)) => Vec::new(),
            Err(source) => return Err(self.store_error(source)),
        };
        items.sort_by(|a, b| path_of(a).cmp(path_of(b)));
        Ok(items)
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
    use crate::index::{markdown::parse_markdown, scan::scan_root};

    mod persistence {
        use pretty_assertions::assert_eq;

        use super::*;
        #[test]
        fn load_all_on_a_freshly_opened_database_is_empty() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let store = IndexStore::open(temp.path()).expect("open store");

            let (records, notes) =
                store.load_all().expect("load empty database");

            assert_eq!(records.len(), 0);
            assert_eq!(notes.len(), 0);
        }

        #[test]
        fn replace_all_then_load_all_round_trips_records_and_notes() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(
                temp.path().join("note.md"),
                "---\ntitle: Hello\n---\n- [ ] task",
            )
            .expect("write note");
            let records = scan_root(temp.path()).expect("scan root");
            let note =
                parse_markdown("note.md", "---\ntitle: Hello\n---\n- [ ] task");
            let notes = vec![note];
            let store = IndexStore::open(temp.path()).expect("open store");

            store.replace_all(&records, &notes).expect("persist records");
            let (loaded_records, loaded_notes) =
                store.load_all().expect("load records");

            assert_eq!(loaded_records, records);
            assert_eq!(loaded_notes, notes);
        }

        #[test]
        fn replace_all_drops_records_absent_from_the_new_set() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("stale.md"), "old")
                .expect("write stale");
            let stale = scan_root(temp.path()).expect("scan stale");
            let store = IndexStore::open(temp.path()).expect("open store");
            store.replace_all(&stale, &[]).expect("persist stale");
            fs::remove_file(temp.path().join("stale.md"))
                .expect("remove stale");
            fs::write(temp.path().join("fresh.md"), "new")
                .expect("write fresh");
            let fresh = scan_root(temp.path()).expect("scan fresh");

            store.replace_all(&fresh, &[]).expect("persist fresh");
            let (loaded_records, loaded_notes) =
                store.load_all().expect("load records");

            assert_eq!(loaded_records, fresh);
            assert_eq!(loaded_notes.len(), 0);
        }

        #[test]
        fn replace_all_with_no_records_persists_an_empty_table() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let store = IndexStore::open(temp.path()).expect("open store");

            store.replace_all(&[], &[]).expect("persist an empty record set");
            let (loaded_records, loaded_notes) =
                store.load_all().expect("load records");

            assert_eq!(loaded_records.len(), 0);
            assert_eq!(loaded_notes.len(), 0);
        }

        #[test]
        fn round_trips_a_record_with_a_unicode_path() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("café ☕.md"), "content")
                .expect("write unicode-named file");
            let records = scan_root(temp.path()).expect("scan root");
            let store = IndexStore::open(temp.path()).expect("open store");

            store.replace_all(&records, &[]).expect("persist records");
            let (loaded_records, _) = store.load_all().expect("load records");

            assert_eq!(loaded_records, records);
        }

        #[test]
        fn load_all_matches_the_path_sort_order_not_the_raw_key_order() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::create_dir_all(temp.path().join("a-b")).expect("mkdir a-b");
            fs::create_dir_all(temp.path().join("a")).expect("mkdir a");
            fs::write(temp.path().join("a-b/c.md"), "1")
                .expect("write a-b/c.md");
            fs::write(temp.path().join("a/z.md"), "2").expect("write a/z.md");
            let records = scan_root(temp.path()).expect("scan root");
            let store = IndexStore::open(temp.path()).expect("open store");
            store.replace_all(&records, &[]).expect("persist records");

            let (loaded_records, _) = store.load_all().expect("load records");

            assert_eq!(loaded_records, records);
        }
    }

    mod load_all {

        use super::*;
        fn write_raw_value(
            store: &IndexStore,
            table_def: TableDefinition<&str, &[u8]>,
            key: &str,
            value: &[u8],
        ) {
            let write_txn = store.db.begin_write().expect("begin write txn");
            {
                let mut table =
                    write_txn.open_table(table_def).expect("open table");
                table.insert(key, value).expect("insert raw bytes");
            }
            write_txn.commit().expect("commit raw insert");
        }

        #[test]
        fn returns_corrupt_when_stored_bytes_are_not_utf8() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let store = IndexStore::open(temp.path()).expect("open store");
            write_raw_value(&store, FILE_RECORDS, "bad.md", &[0xFF, 0xFE]);

            let error =
                store.load_all().expect_err("non-UTF8 bytes fail to load");

            assert!(matches!(
                &error,
                FileIndexError::Corrupt { path, .. } if path == Path::new("bad.md")
            ));
        }

        #[test]
        fn returns_deserialize_error_when_stored_text_is_not_valid_toml() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let store = IndexStore::open(temp.path()).expect("open store");
            write_raw_value(
                &store,
                FILE_RECORDS,
                "bad.md",
                b"not valid toml {{{",
            );

            let error =
                store.load_all().expect_err("invalid TOML text fails to load");

            assert!(matches!(
                &error,
                FileIndexError::Deserialize { path, .. }
                    if path == Path::new("bad.md")
            ));
        }

        #[test]
        fn returns_deserialize_error_when_note_bytes_are_not_valid_toml() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let store = IndexStore::open(temp.path()).expect("open store");
            write_raw_value(&store, NOTES, "bad.md", b"invalid note content");

            let error =
                store.load_all().expect_err("invalid note fails to load");

            assert!(matches!(
                &error,
                FileIndexError::Deserialize { path, .. }
                    if path == Path::new("bad.md")
            ));
        }
    }
}
