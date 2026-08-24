//! Redb persistence for [`FileBase`], [`Note`], and derived inlink records.
//!
//! [`IndexStore`] adapts [`DbStore`] for the file-index schema
//! (`FILES`, `NOTES`, `LINKS` tables). Callers use [`super::IndexerService`]
//! methods instead of interacting with redb tables directly.

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

use super::{INDEX_FILE, error::DbError, inlinks::InlinkMap};
use crate::{file::FileBase, index::error::IndexError, note::Note};

/// Redb database handle for a project root.
///
/// Owns one redb connection under a project root and the generic table
/// store/load mechanics every domain module builds its own table-specific
/// persistence on top of. Table definitions and their read/write semantics
/// stay with the domain that owns them (this module owns File/Note/Inlink
/// tables); this struct owns only "open the file, run a transaction,
/// serialize/deserialize a value or multimap table" — mechanics with no domain
/// knowledge.
#[derive(Debug)]
pub(super) struct DbStore {
    db: redb::Database,
    path: PathBuf,
}

impl DbStore {
    /// Opens or creates a redb database at `root.join(relative_path)`.
    ///
    /// # Errors
    ///
    /// - [`DbError::Io`] if the database's parent directory cannot be created.
    /// - [`DbError::Redb`] if the database file cannot be opened or created.
    pub(super) fn open(
        root: &Path,
        relative_path: &str,
    ) -> Result<Self, DbError> {
        let path = root.join(relative_path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|source| DbError::Io {
                path: parent.to_path_buf(),
                source,
            })?;
        }
        let db =
            redb::Database::create(&path).map_err(|source| DbError::Redb {
                path: path.clone(),
                source: Box::new(source.into()),
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
    pub(super) fn path(&self) -> &Path {
        &self.path
    }

    /// Begins a read transaction.
    ///
    /// # Errors
    ///
    /// - [`DbError::Redb`] if the transaction cannot be started.
    pub(super) fn begin_read(&self) -> Result<ReadTransaction, DbError> {
        self.db.begin_read().map_err(|source| self.store_error(source))
    }

    /// Begins a write transaction.
    ///
    /// # Errors
    ///
    /// - [`DbError::Redb`] if the transaction cannot be started.
    pub(super) fn begin_write(&self) -> Result<WriteTransaction, DbError> {
        self.db.begin_write().map_err(|source| self.store_error(source))
    }

    /// Serializes `items` with postcard into `table`, keyed by `path_of`.
    ///
    /// # Errors
    ///
    /// - [`DbError::Redb`] if the table cannot be opened or written.
    /// - [`DbError::Serialize`] if an item cannot be postcard-encoded.
    pub(super) fn store_table<T: Serialize>(
        &self,
        write_txn: &WriteTransaction,
        table: TableDefinition<&str, &[u8]>,
        items: &[T],
        path_of: impl Fn(&T) -> &Path,
    ) -> Result<(), DbError> {
        let mut table = write_txn
            .open_table(table)
            .map_err(|source| self.store_error(source))?;
        for item in items {
            let path = path_of(item);
            let key = path.to_string_lossy();
            let value = postcard::to_allocvec(item).map_err(|source| {
                DbError::Serialize {
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
    /// - [`DbError::Redb`] if the table cannot be read.
    /// - [`DbError::Deserialize`] if stored bytes are corrupt or incompatible.
    pub(super) fn load_table<T: DeserializeOwned>(
        &self,
        read_txn: &ReadTransaction,
        table: TableDefinition<&str, &[u8]>,
        path_of: impl Fn(&T) -> &Path,
    ) -> Result<Vec<T>, DbError> {
        let mut items: Vec<T> = match read_txn.open_table(table) {
            Ok(table) => {
                let mut items = Vec::new();
                for entry in
                    table.iter().map_err(|source| self.store_error(source))?
                {
                    let (key, value) =
                        entry.map_err(|source| self.store_error(source))?;
                    items.push(postcard::from_bytes(value.value()).map_err(
                        |source| DbError::Deserialize {
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
    /// - [`DbError::Redb`] if the table cannot be opened or written.
    pub(super) fn store_links(
        &self,
        write_txn: &WriteTransaction,
        table: MultimapTableDefinition<&str, &str>,
        links: &HashMap<PathBuf, Vec<PathBuf>>,
    ) -> Result<(), DbError> {
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
    /// - [`DbError::Redb`] if the table cannot be read.
    pub(super) fn load_links(
        &self,
        read_txn: &ReadTransaction,
        table: MultimapTableDefinition<&str, &str>,
    ) -> Result<HashMap<PathBuf, Vec<PathBuf>>, DbError> {
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
    fn store_error(&self, source: impl Into<redb::Error>) -> DbError {
        DbError::Redb {
            path: self.path.clone(),
            source: Box::new(source.into()),
        }
    }
}

/// Postcard-encoded [`FileBase`] bytes keyed by project-relative path.
const FILES: TableDefinition<&str, &[u8]> = TableDefinition::new("files");

/// Postcard-encoded [`Note`] bytes keyed by project-relative path.
const NOTES: TableDefinition<&str, &[u8]> = TableDefinition::new("notes");

/// Derived inbound-link edges: target path to every linking source path.
/// See [`super::inlinks`]; rewritten in full whenever [`super::FileIndex`]
/// content changes, never patched.
const LINKS: MultimapTableDefinition<&str, &str> =
    MultimapTableDefinition::new("links");

/// Atomically read snapshot of persisted [`FileBase`] and [`Note`] records
/// (sorted by path) plus derived inlink edges (target-keyed, unordered).
type IndexSnapshot = (Vec<FileBase>, Vec<Note>, InlinkMap);

/// Redb-backed handle to one project root's index database.
///
/// Wraps [`DbStore`] with the `FILES`/`NOTES`/`LINKS` table definitions.
/// Created by [`Self::open`]. Callers interact through
/// [`super::IndexerService`] methods, not directly.
#[derive(Debug)]
pub(super) struct IndexStore {
    db: DbStore,
}

impl IndexStore {
    /// Opens the index database under `root`, creating it if absent.
    ///
    /// # Errors
    ///
    /// - [`IndexError::Io`] if the database's parent directory cannot be
    ///   created.
    /// - [`IndexError::Store`] if the database file cannot be opened.
    pub(super) fn open(root: &Path) -> Result<Self, IndexError> {
        let db = DbStore::open(root, INDEX_FILE)?;
        Ok(Self {
            db,
        })
    }

    /// Atomically replaces every stored [`FileBase`], [`Note`], and derived
    /// inlink edge.
    ///
    /// All three redb tables are cleared and rewritten in one write
    /// transaction, so readers never observe one table refreshed while another
    /// remains stale.
    ///
    /// # Errors
    ///
    /// - [`IndexError::Store`] if the transaction fails.
    /// - [`IndexError::Serialize`] if a record cannot be encoded.
    pub(super) fn replace_all(
        &self,
        bases: &[FileBase],
        notes: &[Note],
        links: &InlinkMap,
    ) -> Result<(), IndexError> {
        let write_txn = self.db.begin_write()?;
        write_txn
            .delete_table(FILES)
            .map_err(|source| self.db.store_error(source))?;
        write_txn
            .delete_table(NOTES)
            .map_err(|source| self.db.store_error(source))?;
        write_txn
            .delete_multimap_table(LINKS)
            .map_err(|source| self.db.store_error(source))?;
        self.db.store_table(&write_txn, FILES, bases, FileBase::path)?;
        self.db.store_table(&write_txn, NOTES, notes, Note::path)?;
        self.db.store_links(&write_txn, LINKS, links)?;
        write_txn.commit().map_err(|source| self.db.store_error(source))?;
        Ok(())
    }

    /// Loads every stored [`FileBase`] and [`Note`] (sorted by path) and
    /// every derived inlink edge (target-keyed, unordered).
    ///
    /// # Errors
    ///
    /// - [`IndexError::Store`] if a table cannot be read.
    /// - [`IndexError::Deserialize`] if stored bytes are not a valid record.
    pub(super) fn load_all(&self) -> Result<IndexSnapshot, IndexError> {
        let read_txn = self.db.begin_read()?;
        let bases = self.db.load_table(&read_txn, FILES, FileBase::path)?;
        let notes = self.db.load_table(&read_txn, NOTES, Note::path)?;
        let links = self.db.load_links(&read_txn, LINKS)?;
        Ok((bases, notes, links))
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::HashMap,
        fs,
        path::{Path, PathBuf},
    };

    use super::*;
    #[cfg(unix)]
    use crate::index::tests::fixtures::RestorePermissions;
    use crate::{index::scan::scan_root, note::parse_markdown};

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
        let result: Result<Vec<String>, DbError> =
            db.load_table(&read_txn, TEST_TABLE, |s: &String| {
                Path::new(s.as_str())
            });

        assert!(matches!(result, Err(DbError::Deserialize { .. })));
    }

    mod persistence {
        use pretty_assertions::assert_eq;

        use super::*;
        #[test]
        fn returns_empty_when_nothing_persisted() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let store = IndexStore::open(temp.path()).expect("open store");

            let (bases, notes, links) =
                store.load_all().expect("load empty database");

            assert_eq!(bases.len(), 0);
            assert_eq!(notes.len(), 0);
            assert_eq!(links.len(), 0);
        }

        #[test]
        fn replace_all_then_load_all_round_trips_records_and_notes() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(
                temp.path().join("note.md"),
                "---\ntitle: Hello\n---\nPriority:: 5\n- [ ] task",
            )
            .expect("write note");
            let bases = scan_root(temp.path()).expect("scan root");
            let note = parse_markdown(
                "note.md",
                "---\ntitle: Hello\n---\nPriority:: 5\n- [ ] task",
            );
            let notes = vec![note];
            let store = IndexStore::open(temp.path()).expect("open store");

            store
                .replace_all(&bases, &notes, &HashMap::new())
                .expect("persist records");
            let (loaded_records, loaded_notes, _) =
                store.load_all().expect("load records");

            assert_eq!(loaded_records, bases);
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
            let (loaded_records, _loaded_notes, _) =
                store.load_all().expect("load records");

            assert_eq!(loaded_records, fresh);
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
            let bases = scan_root(temp.path()).expect("scan root");
            let store = IndexStore::open(temp.path()).expect("open store");

            store
                .replace_all(&bases, &[], &HashMap::new())
                .expect("persist records");
            let (loaded_records, ..) = store.load_all().expect("load records");

            assert_eq!(loaded_records, bases);
        }

        #[test]
        fn returns_records_in_path_sort_order() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::create_dir_all(temp.path().join("a-b")).expect("mkdir a-b");
            fs::create_dir_all(temp.path().join("a")).expect("mkdir a");
            fs::write(temp.path().join("a-b/c.md"), "1")
                .expect("write a-b/c.md");
            fs::write(temp.path().join("a/z.md"), "2").expect("write a/z.md");
            let bases = scan_root(temp.path()).expect("scan root");
            let store = IndexStore::open(temp.path()).expect("open store");
            store
                .replace_all(&bases, &[], &HashMap::new())
                .expect("persist records");

            let (loaded_records, ..) = store.load_all().expect("load records");

            assert_eq!(loaded_records, bases);
        }
    }

    mod open {
        use super::*;

        #[test]
        fn rejects_directory_at_index_path() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let root = temp.path();
            fs::create_dir_all(root.join(INDEX_FILE))
                .expect("create directory at db path");

            let error = IndexStore::open(root)
                .expect_err("directory at db path fails to open");

            assert!(matches!(error, IndexError::Store { .. }));
        }

        #[cfg(unix)]
        #[test]
        fn returns_io_error_when_parent_dir_unwritable() {
            use std::os::unix::fs::PermissionsExt as _;

            let temp = tempfile::tempdir().expect("create temp dir");
            let root = temp.path();
            fs::set_permissions(root, fs::Permissions::from_mode(0o500))
                .expect("revoke write permission");
            let _restore = RestorePermissions(root);

            let error = IndexStore::open(root)
                .expect_err("unwritable root fails to open store");

            assert!(matches!(error, IndexError::Io { .. }));
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

        #[rstest]
        #[case::file_records(FILES)]
        #[case::notes(NOTES)]
        fn returns_deserialize_error_when_stored_bytes_are_invalid(
            #[case] table_def: TableDefinition<&str, &[u8]>,
        ) {
            let temp = tempfile::tempdir().expect("create temp dir");
            let store = IndexStore::open(temp.path()).expect("open store");
            write_raw_value(&store, table_def, "bad.md", &[0xFF, 0xFE]);

            let error =
                store.load_all().expect_err("invalid bytes fail to load");

            assert!(matches!(
                &error,
                IndexError::Deserialize { path, .. } if path == Path::new("bad.md")
            ));
        }

        #[test]
        fn persists_records_as_postcard_bytes_not_toml_text() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("note.md"), "content")
                .expect("write note");
            let bases = scan_root(temp.path()).expect("scan root");
            let store = IndexStore::open(temp.path()).expect("open store");
            store
                .replace_all(&bases, &[], &HashMap::new())
                .expect("persist records");

            let read_txn = store.db.begin_read().expect("begin read txn");
            let table = read_txn.open_table(FILES).expect("open table");
            let raw = table
                .get("note.md")
                .expect("read raw value")
                .expect("value present");
            let raw_bytes = raw.value().to_vec();

            assert!(postcard::from_bytes::<FileBase>(&raw_bytes).is_ok());
            let decodes_as_toml = str::from_utf8(&raw_bytes)
                .ok()
                .and_then(|text| toml::from_str::<FileBase>(text).ok());
            assert!(decodes_as_toml.is_none());
        }
    }
}
