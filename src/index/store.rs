//! Redb persistence for [`FileRecord`], [`Note`], and derived inlink records.
//!
//! [`IndexStore`] owns table definitions and transactions for the persisted
//! index database. Callers use [`super::FileIndex`] methods instead of
//! interacting with redb tables directly.

use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    str,
};

use redb::{
    Database, MultimapTableDefinition, ReadTransaction, ReadableDatabase as _,
    ReadableMultimapTable as _, ReadableTable as _, TableDefinition,
    WriteTransaction,
};
use serde::{Serialize, de::DeserializeOwned};

use super::{INDEX_FILE, error::FileIndexError, file::FileRecord};
use crate::note::Note;

/// TOML-encoded [`FileRecord`] bytes keyed by project-relative path.
const FILE_RECORDS: TableDefinition<&str, &[u8]> =
    TableDefinition::new("file_records");

/// TOML-encoded [`Note`] bytes keyed by project-relative path.
const NOTES: TableDefinition<&str, &[u8]> = TableDefinition::new("notes");

/// Derived inbound-link edges: target path to every linking source path.
/// See [`super::inlinks`]; rewritten in full whenever [`super::FileIndex`]
/// content changes, never patched.
const LINKS: MultimapTableDefinition<&str, &str> =
    MultimapTableDefinition::new("links");

/// Atomically read snapshot of persisted [`FileRecord`] and [`Note`]
/// records (sorted by path) plus derived inlink edges (target-keyed,
/// unordered).
type IndexSnapshot =
    (Vec<FileRecord>, Vec<Note>, HashMap<PathBuf, Vec<PathBuf>>);

/// Redb-backed handle to one project root's index database.
#[derive(Debug)]
pub(super) struct IndexStore {
    db: Database,
    /// The database's own path, kept for error context.
    path: PathBuf,
}

impl IndexStore {
    /// Opens the index database under `root`, creating it if absent.
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

    /// Atomically replaces every stored [`FileRecord`], [`Note`], and
    /// derived inlink edge.
    ///
    /// All three redb tables are cleared and rewritten in one write
    /// transaction, so readers never observe one table refreshed while
    /// another remains stale.
    ///
    /// # Errors
    ///
    /// - [`FileIndexError::Store`] if the transaction fails
    /// - [`FileIndexError::Serialize`] if a record cannot be TOML-encoded
    pub(super) fn replace_all(
        &self,
        records: &[FileRecord],
        notes: &[Note],
        links: &HashMap<PathBuf, Vec<PathBuf>>,
    ) -> Result<(), FileIndexError> {
        let write_txn =
            self.db.begin_write().map_err(|source| self.store_error(source))?;
        write_txn
            .delete_table(FILE_RECORDS)
            .map_err(|source| self.store_error(source))?;
        write_txn
            .delete_table(NOTES)
            .map_err(|source| self.store_error(source))?;
        write_txn
            .delete_multimap_table(LINKS)
            .map_err(|source| self.store_error(source))?;
        self.store_table(&write_txn, FILE_RECORDS, records, FileRecord::path)?;
        self.store_table(&write_txn, NOTES, notes, Note::path)?;
        self.store_links(&write_txn, links)?;
        write_txn.commit().map_err(|source| self.store_error(source))
    }

    /// Loads every stored [`FileRecord`] and [`Note`] (sorted by path) and
    /// every derived inlink edge (target-keyed, unordered).
    ///
    /// # Errors
    ///
    /// - [`FileIndexError::Store`] if a table cannot be read
    /// - [`FileIndexError::Corrupt`] if stored bytes are not valid UTF-8
    /// - [`FileIndexError::Deserialize`] if stored text is not valid UTF-8 or
    ///   TOML
    pub(super) fn load_all(&self) -> Result<IndexSnapshot, FileIndexError> {
        let read_txn =
            self.db.begin_read().map_err(|source| self.store_error(source))?;
        let records =
            self.load_table(&read_txn, FILE_RECORDS, FileRecord::path)?;
        let notes = self.load_table(&read_txn, NOTES, Note::path)?;
        let links = self.load_links(&read_txn)?;
        Ok((records, notes, links))
    }

    /// Serializes `items` as TOML into `table`, keyed by `path_of`.
    ///
    /// [`Self::replace_all`] uses this helper for both the `file_records` and
    /// `notes` tables instead of duplicating the serialize-and-insert loop.
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

    /// Serializes every `target -> sources` edge into the `links` multimap
    /// table.
    ///
    /// [`Self::replace_all`] uses this instead of [`Self::store_table`]
    /// because [`LINKS`] holds multiple values per key natively, rather than
    /// one serialized `Vec` per key.
    ///
    /// # Errors
    ///
    /// - [`FileIndexError::Store`] if the table cannot be opened or written
    fn store_links(
        &self,
        write_txn: &WriteTransaction,
        links: &HashMap<PathBuf, Vec<PathBuf>>,
    ) -> Result<(), FileIndexError> {
        let mut table = write_txn
            .open_multimap_table(LINKS)
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

    /// Deserializes every TOML value in `table` and sorts the records.
    ///
    /// [`Self::load_all`] uses this helper for both the `file_records` and
    /// `notes` tables instead of duplicating the decode-and-sort loop.
    ///
    /// # Errors
    ///
    /// - [`FileIndexError::Store`] if the table cannot be read
    /// - [`FileIndexError::Corrupt`] if stored bytes are not valid UTF-8
    /// - [`FileIndexError::Deserialize`] if stored text is not valid TOML
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

    /// Deserializes every `target -> sources` edge from the `links`
    /// multimap table.
    ///
    /// [`Self::load_all`] uses this instead of [`Self::load_table`] because
    /// [`ReadableMultimapTable::iter`] already yields each key's values
    /// sorted, so no per-key deserialize-a-`Vec` step is needed.
    ///
    /// # Errors
    ///
    /// - [`FileIndexError::Store`] if the table cannot be read
    fn load_links(
        &self,
        read_txn: &ReadTransaction,
    ) -> Result<HashMap<PathBuf, Vec<PathBuf>>, FileIndexError> {
        let table = match read_txn.open_multimap_table(LINKS) {
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
    use crate::{index::scan::scan_root, note::parse_markdown};

    mod persistence {
        use pretty_assertions::assert_eq;

        use super::*;
        #[test]
        fn load_all_on_a_freshly_opened_database_is_empty() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let store = IndexStore::open(temp.path()).expect("open store");

            let (records, notes, links) =
                store.load_all().expect("load empty database");

            assert_eq!(records.len(), 0);
            assert_eq!(notes.len(), 0);
            assert_eq!(links.len(), 0);
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

            store
                .replace_all(&records, &notes, &HashMap::new())
                .expect("persist records");
            let (loaded_records, loaded_notes, _) =
                store.load_all().expect("load records");

            assert_eq!(loaded_records, records);
            assert_eq!(loaded_notes, notes);
        }

        #[test]
        fn replace_all_then_load_all_round_trips_links() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let store = IndexStore::open(temp.path()).expect("open store");
            let links = HashMap::from([
                (PathBuf::from("target.md"), vec![
                    PathBuf::from("a.md"),
                    PathBuf::from("b.md"),
                ]),
                (PathBuf::from("other.md"), vec![PathBuf::from("a.md")]),
            ]);

            store.replace_all(&[], &[], &links).expect("persist links");
            let (_, _, loaded_links) = store.load_all().expect("load links");

            assert_eq!(loaded_links, links);
        }

        #[test]
        fn replace_all_drops_links_absent_from_the_new_set() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let store = IndexStore::open(temp.path()).expect("open store");
            let stale_links =
                HashMap::from([(PathBuf::from("target.md"), vec![
                    PathBuf::from("a.md"),
                ])]);
            store
                .replace_all(&[], &[], &stale_links)
                .expect("persist stale links");

            store
                .replace_all(&[], &[], &HashMap::new())
                .expect("persist empty links");
            let (_, _, loaded_links) = store.load_all().expect("load links");

            assert_eq!(loaded_links.len(), 0);
        }

        #[test]
        fn replace_all_drops_records_absent_from_the_new_set() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("stale.md"), "old")
                .expect("write stale");
            let stale = scan_root(temp.path()).expect("scan stale");
            let store = IndexStore::open(temp.path()).expect("open store");
            store
                .replace_all(&stale, &[], &HashMap::new())
                .expect("persist stale");
            fs::remove_file(temp.path().join("stale.md"))
                .expect("remove stale");
            fs::write(temp.path().join("fresh.md"), "new")
                .expect("write fresh");
            let fresh = scan_root(temp.path()).expect("scan fresh");

            store
                .replace_all(&fresh, &[], &HashMap::new())
                .expect("persist fresh");
            let (loaded_records, loaded_notes, _) =
                store.load_all().expect("load records");

            assert_eq!(loaded_records, fresh);
            assert_eq!(loaded_notes.len(), 0);
        }

        #[test]
        fn replace_all_with_no_records_persists_an_empty_table() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let store = IndexStore::open(temp.path()).expect("open store");

            store
                .replace_all(&[], &[], &HashMap::new())
                .expect("persist an empty record set");
            let (loaded_records, loaded_notes, _) =
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

            store
                .replace_all(&records, &[], &HashMap::new())
                .expect("persist records");
            let (loaded_records, ..) = store.load_all().expect("load records");

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
            store
                .replace_all(&records, &[], &HashMap::new())
                .expect("persist records");

            let (loaded_records, ..) = store.load_all().expect("load records");

            assert_eq!(loaded_records, records);
        }
    }

    mod open {
        use super::*;

        #[test]
        fn returns_store_error_when_the_index_path_is_a_directory() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let root = temp.path();
            fs::create_dir_all(root.join(INDEX_FILE))
                .expect("create directory at db path");

            let error = IndexStore::open(root)
                .expect_err("directory at db path fails to open");

            assert!(matches!(error, FileIndexError::Store { .. }));
        }

        #[cfg(unix)]
        #[test]
        fn returns_io_error_when_the_parent_directory_cannot_be_created() {
            use std::os::unix::fs::PermissionsExt as _;

            /// Restores a locked directory's permissions on drop, even if
            /// the test panics. Otherwise a `0o500` root blocks the tempdir's
            /// own cleanup.
            struct RestorePermissions<'a>(&'a Path);

            impl Drop for RestorePermissions<'_> {
                fn drop(&mut self) {
                    let _ = fs::set_permissions(
                        self.0,
                        fs::Permissions::from_mode(0o700),
                    );
                }
            }

            let temp = tempfile::tempdir().expect("create temp dir");
            let root = temp.path();
            fs::set_permissions(root, fs::Permissions::from_mode(0o500))
                .expect("revoke write permission");
            let _restore = RestorePermissions(root);

            let error = IndexStore::open(root)
                .expect_err("unwritable root fails to open store");

            assert!(matches!(error, FileIndexError::Io { .. }));
        }
    }

    mod load_all {

        use rstest::rstest;

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

        #[rstest]
        #[case::file_records(FILE_RECORDS)]
        #[case::notes(NOTES)]
        fn returns_deserialize_error_when_stored_text_is_not_valid_toml(
            #[case] table_def: TableDefinition<&str, &[u8]>,
        ) {
            let temp = tempfile::tempdir().expect("create temp dir");
            let store = IndexStore::open(temp.path()).expect("open store");
            write_raw_value(&store, table_def, "bad.md", b"not valid toml {{{");

            let error =
                store.load_all().expect_err("invalid TOML text fails to load");

            assert!(matches!(
                &error,
                FileIndexError::Deserialize { path, .. }
                    if path == Path::new("bad.md")
            ));
        }
    }
}
