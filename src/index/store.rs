//! Redb persistence for [`FileBase`], [`Note`], and derived inlink records.
//!
//! [`IndexStore`] owns one redb connection under a project root and adapts it
//! to the file-index schema (`FILES`, `NOTES`, `LINKS` tables). Callers use
//! [`super::IndexerService`] methods instead of interacting with redb tables
//! directly.

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

/// Postcard-encoded [`FileBase`] bytes keyed by project-relative path.
const FILES: TableDefinition<&str, &[u8]> = TableDefinition::new("files");

/// Postcard-encoded [`Note`] bytes keyed by project-relative path.
const NOTES: TableDefinition<&str, &[u8]> = TableDefinition::new("notes");

/// Derived inbound-link edges: target path to every linking source path. See
/// [`super::inlinks`]. Written via [`IndexStore::replace_all`] (full rewrite)
/// or [`IndexStore::persist_index`]'s incremental path, which patches only
/// [`super::builder::IndexDelta::Incremental`]'s changed targets instead of
/// rewriting the whole table.
const LINKS: MultimapTableDefinition<&str, &str> =
    MultimapTableDefinition::new("links");

/// Atomically read snapshot of persisted [`FileBase`] and [`Note`] records
/// (sorted by path) plus derived inlink edges (target-keyed, unordered).
type IndexSnapshot = (Vec<FileBase>, Vec<Note>, InlinkMap);

/// Redb-backed handle to one project root's index database.
///
/// Owns one redb connection under a project root and the generic table
/// store/load mechanics every domain module builds its own table-specific
/// persistence on top of. Table definitions and their read/write semantics stay
/// with the domain that owns them (this module owns File/Note/Inlink tables);
/// this struct owns only "open the file, run a transaction,
/// serialize/deserialize a value or multimap table" — mechanics with no domain
/// knowledge.
///
/// Created by [`Self::open`]. Callers interact through
/// [`super::IndexerService`] methods, not directly.
#[derive(Debug)]
pub(super) struct IndexStore {
    db: redb::Database,
    path: PathBuf,
}

impl IndexStore {
    /// Opens the index database under `root`, creating it if absent.
    ///
    /// # Errors
    ///
    /// - [`IndexError::Store`] ([`DbError::Io`]) if the database's parent
    ///   directory cannot be created.
    /// - [`IndexError::Store`] ([`DbError::Redb`]) if the database file cannot
    ///   be opened.
    pub(super) fn open(root: &Path) -> Result<Self, IndexError> {
        let path = root.join(INDEX_FILE);
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

    /// Atomically replaces every stored [`FileBase`], [`Note`], and derived
    /// inlink edge.
    ///
    /// All three redb tables are cleared and rewritten in one write
    /// transaction, so readers never observe one table refreshed while another
    /// remains stale.
    ///
    /// # Errors
    ///
    /// - [`IndexError::Store`] ([`DbError::Redb`]) if the transaction fails.
    /// - [`IndexError::Store`] ([`DbError::Serialize`]) if a record cannot be
    ///   encoded.
    pub(super) fn replace_all(
        &self,
        bases: &[FileBase],
        notes: &[Note],
        links: &InlinkMap,
    ) -> Result<(), IndexError> {
        let write_txn = self.begin_write()?;
        write_txn
            .delete_table(FILES)
            .map_err(|source| self.store_error(source))?;
        write_txn
            .delete_table(NOTES)
            .map_err(|source| self.store_error(source))?;
        write_txn
            .delete_multimap_table(LINKS)
            .map_err(|source| self.store_error(source))?;
        self.store_table(&write_txn, FILES, bases, FileBase::path)?;
        self.store_table(&write_txn, NOTES, notes, Note::path)?;
        self.store_links(&write_txn, LINKS, links)?;
        write_txn.commit().map_err(|source| self.store_error(source))?;
        Ok(())
    }

    /// Loads every stored [`FileBase`] and [`Note`] (sorted by path) and every
    /// derived inlink edge (target-keyed, unordered).
    ///
    /// # Errors
    ///
    /// - [`IndexError::Store`] ([`DbError::Redb`]) if a table cannot be read.
    /// - [`IndexError::Store`] ([`DbError::Deserialize`]) if stored bytes are
    ///   not a valid record.
    pub(super) fn load_all(&self) -> Result<IndexSnapshot, IndexError> {
        let read_txn = self.begin_read()?;
        let bases = self.load_table(&read_txn, FILES, FileBase::path)?;
        let notes = self.load_table(&read_txn, NOTES, Note::path)?;
        let links = self.load_links(&read_txn, LINKS)?;
        Ok((bases, notes, links))
    }

    /// Reads and deserializes exactly one [`Note`] from the `NOTES` table by
    /// path, without loading any other row — the point-lookup redb's zero-copy
    /// `AccessGuard` is designed for, used by
    /// [`super::builder::IndexBuilder::build_with_reuse`] to recall an
    /// unchanged Note's previous value without deserializing every persisted
    /// Note upfront.
    ///
    /// # Errors
    ///
    /// - [`IndexError::Store`] ([`DbError::Redb`]) if the table cannot be read.
    /// - [`IndexError::Store`] ([`DbError::Deserialize`]) if the stored bytes
    ///   are corrupt.
    pub(super) fn load_note(
        &self,
        read_txn: &ReadTransaction,
        path: &Path,
    ) -> Result<Option<Note>, IndexError> {
        let table = match read_txn.open_table(NOTES) {
            Ok(table) => table,
            Err(redb::TableError::TableDoesNotExist(_)) => return Ok(None),
            Err(source) => return Err(self.store_error(source).into()),
        };
        let key = path.to_string_lossy();
        match table.get(&*key).map_err(|source| self.store_error(source))? {
            None => Ok(None),
            Some(guard) => {
                let note =
                    postcard::from_bytes(guard.value()).map_err(|source| {
                        DbError::Deserialize {
                            path: path.to_path_buf(),
                            source,
                        }
                    })?;
                Ok(Some(note))
            }
        }
    }

    /// Loads every persisted [`FileBase`] (sorted by path) and inlink edge,
    /// without touching `NOTES` — the comparatively heavy per-note table.
    /// [`super::IndexerService::refresh`] uses this instead of
    /// [`Self::load_all`] so unchanged Notes are recalled lazily via
    /// [`Self::load_note`] instead of every persisted Note being deserialized
    /// upfront regardless of whether it changed.
    ///
    /// # Errors
    ///
    /// - [`IndexError::Store`] ([`DbError::Redb`]) if a table cannot be read.
    /// - [`IndexError::Store`] ([`DbError::Deserialize`]) if stored bytes are
    ///   not a valid record.
    pub(super) fn load_bases_and_links(
        &self,
    ) -> Result<(Vec<FileBase>, InlinkMap), IndexError> {
        let read_txn = self.begin_read()?;
        let bases = self.load_table(&read_txn, FILES, FileBase::path)?;
        let links = self.load_links(&read_txn, LINKS)?;
        Ok((bases, links))
    }

    /// Persists `index`, choosing a full [`Self::replace_all`] rewrite when its
    /// delta is [`super::builder::IndexDelta::Full`] (no previous state to diff
    /// against), or a row-level incremental write for
    /// [`super::builder::IndexDelta::Incremental`]'s changed paths only.
    ///
    /// # Errors
    ///
    /// Same as [`Self::replace_all`]/the incremental write path: transaction
    /// failure or serialization failure.
    pub(super) fn persist_index(
        &self,
        index: &super::FileIndex,
    ) -> Result<(), IndexError> {
        match index.delta() {
            super::builder::IndexDelta::Full => {
                self.replace_all(index.bases(), index.notes(), index.inlinks())
            }
            super::builder::IndexDelta::Incremental(_) => {
                self.persist_incremental(index)
            }
        }
    }

    /// Row-level incremental write for
    /// [`super::builder::IndexDelta::Incremental`].
    ///
    /// Falls back to [`Self::replace_all`] if `index`'s delta turns out to be
    /// [`super::builder::IndexDelta::Full`] — defensive only; every caller
    /// routes through [`Self::persist_index`], which never reaches this
    /// branch for a full delta.
    fn persist_incremental(
        &self,
        index: &super::FileIndex,
    ) -> Result<(), IndexError> {
        let super::builder::IndexDelta::Incremental(delta) = index.delta()
        else {
            return self.replace_all(
                index.bases(),
                index.notes(),
                index.inlinks(),
            );
        };
        let super::builder::IncrementalDelta {
            upserted,
            deleted,
            links_upserted,
            links_deleted,
        } = delta.as_ref();
        let write_txn = self.begin_write()?;
        self.upsert_files_and_notes(&write_txn, index, upserted, deleted)?;
        if let Some(links_upserted) = links_upserted {
            self.upsert_links(
                &write_txn,
                index,
                links_upserted,
                links_deleted,
            )?;
        }
        write_txn.commit().map_err(|source| self.store_error(source))?;
        Ok(())
    }

    /// Deletes `deleted` paths from `FILES`/`NOTES`, then upserts each
    /// `upserted` path's current [`FileBase`] (and [`Note`], if present) by
    /// binary-searching `index`'s path-sorted slices.
    fn upsert_files_and_notes(
        &self,
        write_txn: &WriteTransaction,
        index: &super::FileIndex,
        upserted: &[PathBuf],
        deleted: &[PathBuf],
    ) -> Result<(), IndexError> {
        let mut files = write_txn
            .open_table(FILES)
            .map_err(|source| self.store_error(source))?;
        let mut notes_table = write_txn
            .open_table(NOTES)
            .map_err(|source| self.store_error(source))?;
        for path in deleted {
            let key = path.to_string_lossy();
            files.remove(&*key).map_err(|source| self.store_error(source))?;
            notes_table
                .remove(&*key)
                .map_err(|source| self.store_error(source))?;
        }
        for path in upserted {
            if let Ok(idx) =
                index.bases().binary_search_by(|b| b.path().cmp(path))
                && let Some(base) = index.bases().get(idx)
            {
                self.upsert_row(&mut files, path, base)?;
            }
            if let Ok(idx) =
                index.notes().binary_search_by(|n| n.path().cmp(path))
                && let Some(note) = index.notes().get(idx)
            {
                self.upsert_row(&mut notes_table, path, note)?;
            }
        }
        Ok(())
    }

    /// Serializes `value` with postcard and upserts it into `table` at
    /// `path`. Shared by [`Self::upsert_files_and_notes`] for both the
    /// `FILES` and `NOTES` tables, which share the same key/value shape.
    fn upsert_row<T: Serialize>(
        &self,
        table: &mut redb::Table<'_, &str, &[u8]>,
        path: &Path,
        value: &T,
    ) -> Result<(), IndexError> {
        let key = path.to_string_lossy();
        let bytes = postcard::to_allocvec(value).map_err(|source| {
            DbError::Serialize {
                path: path.to_path_buf(),
                source,
            }
        })?;
        table
            .insert(&*key, bytes.as_slice())
            .map_err(|source| self.store_error(source))?;
        Ok(())
    }

    /// Removes `links_deleted` targets from `LINKS`, then rewrites each
    /// `links_upserted` target's full source set from `index`'s inlink map.
    fn upsert_links(
        &self,
        write_txn: &WriteTransaction,
        index: &super::FileIndex,
        links_upserted: &[PathBuf],
        links_deleted: &[PathBuf],
    ) -> Result<(), IndexError> {
        let mut links = write_txn
            .open_multimap_table(LINKS)
            .map_err(|source| self.store_error(source))?;
        for target in links_deleted {
            links
                .remove_all(&*target.to_string_lossy())
                .map_err(|source| self.store_error(source))?;
        }
        for target in links_upserted {
            self.upsert_link_target(&mut links, index, target)?;
        }
        Ok(())
    }

    /// Rewrites one target's full source set in `links` from `index`'s
    /// inlink map, replacing whatever was previously stored for it.
    fn upsert_link_target(
        &self,
        links: &mut redb::MultimapTable<'_, &str, &str>,
        index: &super::FileIndex,
        target: &Path,
    ) -> Result<(), IndexError> {
        let target_key = target.to_string_lossy();
        links
            .remove_all(&*target_key)
            .map_err(|source| self.store_error(source))?;
        let Some(sources) = index.inlinks().get(target) else {
            return Ok(());
        };
        for source in sources {
            links
                .insert(&*target_key, &*source.to_string_lossy())
                .map_err(|source| self.store_error(source))?;
        }
        Ok(())
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
        let db = IndexStore::open(temp.path()).expect("open db");
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
        let db = IndexStore::open(temp.path()).expect("open db");
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

            assert!(matches!(error, IndexError::Store(DbError::Redb { .. })));
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

            assert!(matches!(error, IndexError::Store(DbError::Io { .. })));
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
                IndexError::Store(DbError::Deserialize { path, .. })
                    if path == Path::new("bad.md")
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
