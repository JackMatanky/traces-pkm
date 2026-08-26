//! Redb persistence for [`FileBase`], [`Note`], and derived inlink records.
//!
//! [`IndexStore`] owns one redb connection under a project root and adapts it
//! to the file-index schema (`FILES`, `NOTES`, `LINKS` tables). Callers use
//! [`super::IndexerService`] methods instead of interacting with redb tables
//! directly.
//!
//! `FILES`/`NOTES` values stay plain `&[u8]` in their `TableDefinition`s
//! rather than a `Postcard<T>` wrapper implementing `redb::Value`:
//! `redb::Value::from_bytes` is infallible (cannot return a `Result`), so a
//! corrupted row's postcard-decode failure could only surface as a panic,
//! incompatible with this crate's `Cargo.toml` denying
//! `clippy::panic`/`unwrap_used`/`expect_used` and with
//! [`DbError::Deserialize`]'s existing per-row `Result` error path.
//! [`encode_row`]/[`decode_row`] do the postcard call plus [`DbError`] wrap
//! explicitly at each read/write site instead.

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

use super::{
    FileIndex, INDEX_FILE,
    delta::{IncrementalDelta, IndexDelta},
    error::{DbError, IndexError},
    inlinks::InlinkMap,
};
use crate::{file::FileBase, note::Note};

/// Postcard-encoded [`FileBase`] bytes keyed by project-relative path.
const FILES: TableDefinition<&[u8], &[u8]> = TableDefinition::new("files");

/// Postcard-encoded [`Note`] bytes keyed by project-relative path.
const NOTES: TableDefinition<&[u8], &[u8]> = TableDefinition::new("notes");

/// Derived inbound-link edges: target path to every linking source path. See
/// [`super::inlinks`]. Written via [`IndexStore::replace_all`] (full rewrite)
/// or [`IndexStore::persist_index`]'s incremental path, which patches only
/// [`super::delta::IndexDelta::Incremental`]'s changed targets instead of
/// rewriting the whole table.
const LINKS: MultimapTableDefinition<&[u8], &[u8]> =
    MultimapTableDefinition::new("links");

/// Postcard-encodes `value` for a row keyed by `path`, wrapping a failure
/// as [`DbError::Serialize`]. Shared by [`IndexStore::write_table`]'s loop
/// and [`IndexStore::upsert_row`], previously duplicated inline.
fn encode_row<T: Serialize>(
    path: &Path,
    value: &T,
) -> Result<Vec<u8>, DbError> {
    postcard::to_allocvec(value).map_err(|source| DbError::Serialize {
        path: path.to_path_buf(),
        source,
    })
}

/// Postcard-decodes `bytes` for a row keyed by `path`, wrapping a failure
/// as [`DbError::Deserialize`]. Shared by [`IndexStore::read_table`]'s loop
/// and [`IndexStore::read_note`], previously duplicated inline. Not a
/// `redb::Value` impl: see this file's module-level correction note.
fn decode_row<T: DeserializeOwned>(
    path: &Path,
    bytes: &[u8],
) -> Result<T, DbError> {
    postcard::from_bytes(bytes).map_err(|source| DbError::Deserialize {
        path: path.to_path_buf(),
        source,
    })
}

/// Recovers a `PathBuf` from raw key/value bytes: an exact UTF-8 decode,
/// falling back to a lossy decode only for non-Unicode paths. No `unsafe`:
/// `OsStr::from_encoded_bytes_unchecked` would be exact for every input but
/// requires an `unsafe` block this crate avoids; the lossy fallback degrades
/// only non-Unicode filenames, matching this crate's pre-migration
/// `Path::to_string_lossy` behavior for the same edge case (full byte-exact
/// fidelity is ticket 16, deliberately deferred). Used by [`read_table`]'s
/// per-row deserialize-error path and [`read_links`]'s target/source
/// reconstruction.
///
/// [`read_table`]: IndexStore::read_table
/// [`read_links`]: IndexStore::read_links
fn path_from_bytes(bytes: &[u8]) -> PathBuf {
    str::from_utf8(bytes).map_or_else(
        |_| PathBuf::from(String::from_utf8_lossy(bytes).into_owned()),
        PathBuf::from,
    )
}

/// Atomically read snapshot of persisted [`FileBase`] and [`Note`] records
/// (sorted by path) plus derived inlink edges (target-keyed, unordered).
type IndexSnapshot = (Vec<FileBase>, Vec<Note>, InlinkMap);

/// One raw `LINKS` multimap-table iterator entry: a target key's
/// `AccessGuard` paired with its source-set `MultimapValue`, or the
/// `redb::StorageError` reading it failed with. Named to satisfy
/// `clippy::type_complexity`; used only by
/// [`IndexStore::process_link_entry`].
type LinkEntry<'a> = Result<
    (
        redb::AccessGuard<'a, &'static [u8]>,
        redb::MultimapValue<'a, &'static [u8]>,
    ),
    redb::StorageError,
>;

/// Redb-backed handle to one project root's index database.
///
/// Owns one redb connection under a project root and the generic table
/// store/load mechanics every domain module builds its own table-specific
/// persistence on top of. Table definitions and their read/write semantics
/// stay with the domain that owns them (this module owns File/Note/Inlink
/// tables); this struct owns only "open the file, run a transaction,
/// serialize/deserialize a value or multimap table", mechanics with no
/// domain knowledge.
///
/// Created by [`Self::open`]. Callers interact through
/// [`super::IndexerService`] methods, not directly.
///
/// [`super::IndexerService`]: super::IndexerService
#[derive(Debug)]
pub(super) struct IndexStore {
    db: redb::Database,
    path: PathBuf,
}

impl IndexStore {
    /// Opens the index database under `root`, creating it if absent.
    ///
    /// Recovers by wipe-and-recreate if the existing file is corrupted or
    /// schema-mismatched.
    ///
    /// # Errors
    ///
    /// - [`IndexError::Store`] ([`DbError::Io`]) if the database's parent
    ///   directory cannot be created, or if a corrupted or schema-mismatched
    ///   file cannot be deleted during recovery.
    /// - [`IndexError::Store`] ([`DbError::Redb`]) if the database file cannot
    ///   be opened, or a post-recovery re-create fails.
    pub(super) fn open(root: &Path) -> Result<Self, IndexError> {
        let path = root.join(INDEX_FILE);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|source| DbError::Io {
                path: parent.to_path_buf(),
                source,
            })?;
        }
        let db = Self::create_db(&path)?;
        let db = if Self::should_rebuild(&db, &path)? {
            drop(db);
            fs::remove_file(&path).map_err(|source| DbError::Io {
                path: path.clone(),
                source,
            })?;
            Self::create_db(&path)?
        } else {
            db
        };
        Ok(Self {
            db,
            path,
        })
    }

    /// Opens (or creates) the database file.
    ///
    /// Recovers by wipe-and-recreate if `Database::create` itself reports
    /// container-level corruption.
    fn create_db(path: &Path) -> Result<redb::Database, DbError> {
        let wrap = |source: redb::DatabaseError| DbError::Redb {
            path: path.to_path_buf(),
            source: Box::new(source.into()),
        };
        match redb::Database::create(path) {
            Ok(db) => Ok(db),
            Err(redb::DatabaseError::Storage(
                redb::StorageError::Corrupted(_),
            )) => {
                fs::remove_file(path).map_err(|source| DbError::Io {
                    path: path.to_path_buf(),
                    source,
                })?;
                redb::Database::create(path).map_err(wrap)
            }
            Err(source) => Err(wrap(source)),
        }
    }

    /// True if `FILES`/`NOTES`/`LINKS` show schema drift or per-table
    /// structural corruption against this process's compiled-in table
    /// definitions.
    ///
    /// A fresh, still-tableless database (first-ever open) reports
    /// `TableDoesNotExist` for all three, which is not a rebuild trigger.
    fn should_rebuild(
        db: &redb::Database,
        path: &Path,
    ) -> Result<bool, DbError> {
        let read_txn = db.begin_read().map_err(|source| DbError::Redb {
            path: path.to_path_buf(),
            source: Box::new(source.into()),
        })?;
        for probe in [
            read_txn.open_table(FILES).err(),
            read_txn.open_table(NOTES).err(),
            read_txn.open_multimap_table(LINKS).err(),
        ] {
            let Some(error) = probe else {
                continue;
            };
            if Self::is_rebuild_trigger(&error) {
                return Ok(true);
            }
            if !matches!(error, redb::TableError::TableDoesNotExist(_)) {
                return Err(DbError::Redb {
                    path: path.to_path_buf(),
                    source: Box::new(error.into()),
                });
            }
        }
        Ok(false)
    }

    /// Schema drift or structural corruption this store recovers from by
    /// wiping and rebuilding, deliberately narrower than "any unexpected
    /// `TableError`": an unrelated I/O failure should propagate, not
    /// trigger a destructive wipe that won't fix it.
    fn is_rebuild_trigger(error: &redb::TableError) -> bool {
        matches!(
            error,
            redb::TableError::TableTypeMismatch { .. }
                | redb::TableError::TypeDefinitionChanged { .. }
                | redb::TableError::Storage(redb::StorageError::Corrupted(_))
        )
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
    pub(super) fn write_table<T: Serialize>(
        &self,
        txn: &WriteTransaction,
        table: TableDefinition<&[u8], &[u8]>,
        items: &[T],
        path_of: impl Fn(&T) -> &Path,
    ) -> Result<(), DbError> {
        let mut table =
            txn.open_table(table).map_err(|source| self.store_error(source))?;
        for item in items {
            let path = path_of(item);
            let key = path.as_os_str().as_encoded_bytes();
            let value = encode_row(path, item)?;
            table
                .insert(key, value.as_slice())
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
    pub(super) fn read_table<T: DeserializeOwned>(
        &self,
        txn: &ReadTransaction,
        table: TableDefinition<&[u8], &[u8]>,
        path_of: impl Fn(&T) -> &Path,
    ) -> Result<Vec<T>, DbError> {
        let mut items: Vec<T> = match txn.open_table(table) {
            Ok(table) => {
                let mut items = Vec::new();
                for entry in
                    table.iter().map_err(|source| self.store_error(source))?
                {
                    let (key, value) =
                        entry.map_err(|source| self.store_error(source))?;
                    let path = path_from_bytes(key.value());
                    items.push(decode_row(&path, value.value())?);
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
    pub(super) fn write_links(
        &self,
        txn: &WriteTransaction,
        table: MultimapTableDefinition<&[u8], &[u8]>,
        links: &HashMap<PathBuf, Vec<PathBuf>>,
    ) -> Result<(), DbError> {
        let mut table = txn
            .open_multimap_table(table)
            .map_err(|source| self.store_error(source))?;
        for (target, sources) in links {
            let target_key = target.as_os_str().as_encoded_bytes();
            for source in sources {
                table
                    .insert(target_key, source.as_os_str().as_encoded_bytes())
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
    pub(super) fn read_links(
        &self,
        txn: &ReadTransaction,
        table: MultimapTableDefinition<&[u8], &[u8]>,
    ) -> Result<HashMap<PathBuf, Vec<PathBuf>>, DbError> {
        let table = match txn.open_multimap_table(table) {
            Ok(table) => table,
            Err(redb::TableError::TableDoesNotExist(_)) => {
                return Ok(HashMap::new());
            }
            Err(source) => return Err(self.store_error(source)),
        };
        let mut links = HashMap::new();
        for entry in table.iter().map_err(|source| self.store_error(source))? {
            let (target, sources) = self.process_link_entry(entry)?;
            links.insert(target, sources);
        }
        Ok(links)
    }

    /// Extracts one `target -> sources` row from a `LINKS` multimap
    /// iterator entry. Split from [`Self::read_links`]'s loop body to
    /// reduce that function's stack frame
    /// (`clippy::large_stack_frames`).
    fn process_link_entry(
        &self,
        entry: LinkEntry<'_>,
    ) -> Result<(PathBuf, Vec<PathBuf>), DbError> {
        let (target, sources) =
            entry.map_err(|source| self.store_error(source))?;
        let sources = self.collect_sources(sources)?;
        Ok((path_from_bytes(target.value()), sources))
    }

    /// Drains one target's `MultimapValue` iterator of source paths.
    fn collect_sources(
        &self,
        sources: redb::MultimapValue<'_, &[u8]>,
    ) -> Result<Vec<PathBuf>, DbError> {
        let mut values = Vec::new();
        for source in sources {
            let source = source.map_err(|source| self.store_error(source))?;
            values.push(path_from_bytes(source.value()));
        }
        Ok(values)
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
        files: &[FileBase],
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
        self.write_table(&write_txn, FILES, files, FileBase::path)?;
        self.write_table(&write_txn, NOTES, notes, Note::path)?;
        self.write_links(&write_txn, LINKS, links)?;
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
    pub(super) fn read_all(&self) -> Result<IndexSnapshot, IndexError> {
        let txn = self.begin_read()?;
        let files = self.read_table(&txn, FILES, FileBase::path)?;
        let notes = self.read_table(&txn, NOTES, Note::path)?;
        let links = self.read_links(&txn, LINKS)?;
        Ok((files, notes, links))
    }

    /// Reads and deserializes exactly one [`Note`] from the `NOTES` table by
    /// path, without loading any other row; the point-lookup redb's zero-copy
    /// `AccessGuard` is designed for, used by
    /// [`super::cache::RefreshCache::reconcile_note`] to recall an
    /// unchanged Note's previous value without deserializing every persisted
    /// Note upfront.
    ///
    /// # Errors
    ///
    /// - [`IndexError::Store`] ([`DbError::Redb`]) if the table cannot be read.
    /// - [`IndexError::Store`] ([`DbError::Deserialize`]) if the stored bytes
    ///   are corrupt.
    pub(super) fn read_note(
        &self,
        txn: &ReadTransaction,
        path: &Path,
    ) -> Result<Option<Note>, IndexError> {
        let table = match txn.open_table(NOTES) {
            Ok(table) => table,
            Err(redb::TableError::TableDoesNotExist(_)) => return Ok(None),
            Err(source) => return Err(self.store_error(source).into()),
        };
        let key = path.as_os_str().as_encoded_bytes();
        match table.get(key).map_err(|source| self.store_error(source))? {
            None => Ok(None),
            Some(guard) => Ok(Some(decode_row(path, guard.value())?)),
        }
    }

    /// Loads every persisted [`FileBase`] (sorted by path) and inlink edge,
    /// without touching `NOTES`, the comparatively heavy per-note table.
    /// [`super::IndexerService::refresh`] uses this instead of
    /// [`Self::read_all`] so unchanged Notes are recalled lazily via
    /// [`Self::read_note`] instead of every persisted Note being deserialized
    /// upfront regardless of whether it changed. Takes the caller's own
    /// read transaction rather than opening one via [`Self::begin_read`] so
    /// [`super::cache::RefreshCache::load`] can share one transaction
    /// with the point lookups it backs.
    ///
    /// # Errors
    ///
    /// - [`IndexError::Store`] ([`DbError::Redb`]) if a table cannot be read.
    /// - [`IndexError::Store`] ([`DbError::Deserialize`]) if stored bytes are
    ///   not a valid record.
    pub(super) fn read_files_and_links_via(
        &self,
        txn: &ReadTransaction,
    ) -> Result<(Vec<FileBase>, InlinkMap), IndexError> {
        let files = self.read_table(txn, FILES, FileBase::path)?;
        let links = self.read_links(txn, LINKS)?;
        Ok((files, links))
    }

    /// Persists `index`, choosing a full [`Self::replace_all`] rewrite when its
    /// delta is [`super::delta::IndexDelta::Full`] (no previous state to diff
    /// against), or a row-level incremental write for
    /// [`super::delta::IndexDelta::Incremental`]'s changed paths only.
    ///
    /// # Errors
    ///
    /// Same as [`Self::replace_all`]/the incremental write path: transaction
    /// failure or serialization failure.
    pub(super) fn persist_index(
        &self,
        index: &FileIndex,
    ) -> Result<(), IndexError> {
        match index.delta() {
            IndexDelta::Full => {
                self.replace_all(index.bases(), index.notes(), index.inlinks())
            }
            IndexDelta::Incremental(_) => self.persist_incremental(index),
        }
    }

    /// Row-level incremental write for
    /// [`super::delta::IndexDelta::Incremental`].
    ///
    /// Falls back to [`Self::replace_all`] if `index`'s delta turns out to be
    /// [`super::delta::IndexDelta::Full`], defensive only; every caller
    /// routes through [`Self::persist_index`], which never reaches this
    /// branch for a full delta.
    fn persist_incremental(&self, index: &FileIndex) -> Result<(), IndexError> {
        let IndexDelta::Incremental(delta) = index.delta() else {
            return self.replace_all(
                index.bases(),
                index.notes(),
                index.inlinks(),
            );
        };
        if delta.is_empty() {
            return Ok(());
        }
        let IncrementalDelta {
            upserted,
            deleted,
            links_upserted,
            links_deleted,
        } = delta.as_ref();
        let mut write_txn = self.begin_write()?;
        write_txn
            .set_durability(redb::Durability::None)
            .map_err(|source| self.store_error(source))?;
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
        index: &FileIndex,
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
            let key = path.as_os_str().as_encoded_bytes();
            files.remove(key).map_err(|source| self.store_error(source))?;
            notes_table
                .remove(key)
                .map_err(|source| self.store_error(source))?;
        }
        for path in upserted {
            if let Ok(idx) =
                index.bases().binary_search_by(|f| f.path().cmp(path))
                && let Some(file) = index.bases().get(idx)
            {
                self.upsert_row(&mut files, path, file)?;
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
        table: &mut redb::Table<'_, &[u8], &[u8]>,
        path: &Path,
        value: &T,
    ) -> Result<(), IndexError> {
        let key = path.as_os_str().as_encoded_bytes();
        let bytes = encode_row(path, value)?;
        table
            .insert(key, bytes.as_slice())
            .map_err(|source| self.store_error(source))?;
        Ok(())
    }

    /// Removes `links_deleted` targets from `LINKS`, then rewrites each
    /// `links_upserted` target's full source set from `index`'s inlink map.
    fn upsert_links(
        &self,
        write_txn: &WriteTransaction,
        index: &FileIndex,
        links_upserted: &[PathBuf],
        links_deleted: &[PathBuf],
    ) -> Result<(), IndexError> {
        let mut links = write_txn
            .open_multimap_table(LINKS)
            .map_err(|source| self.store_error(source))?;
        for target in links_deleted {
            links
                .remove_all(target.as_os_str().as_encoded_bytes())
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
        links: &mut redb::MultimapTable<'_, &[u8], &[u8]>,
        index: &FileIndex,
        target: &Path,
    ) -> Result<(), IndexError> {
        let target_key = target.as_os_str().as_encoded_bytes();
        links
            .remove_all(target_key)
            .map_err(|source| self.store_error(source))?;
        let Some(sources) = index.inlinks().get(target) else {
            return Ok(());
        };
        for source in sources {
            links
                .insert(target_key, source.as_os_str().as_encoded_bytes())
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
    use crate::{index::IndexerService, note::parse_markdown};

    const TEST_TABLE: TableDefinition<&[u8], &[u8]> =
        TableDefinition::new("test_table");

    /// Writes raw, possibly-invalid bytes directly into `table_def` at
    /// `key`, bypassing postcard encoding, used to simulate corrupted
    /// stored rows.
    fn write_raw_value(
        store: &IndexStore,
        table_def: TableDefinition<&[u8], &[u8]>,
        key: &str,
        value: &[u8],
    ) {
        let write_txn = store.db.begin_write().expect("begin write txn");
        {
            let mut table =
                write_txn.open_table(table_def).expect("open table");
            table.insert(key.as_bytes(), value).expect("insert raw bytes");
        }
        write_txn.commit().expect("commit raw insert");
    }

    #[test]
    fn persists_records_as_postcard_bytes_not_toml_text() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let db = IndexStore::open(temp.path()).expect("open db");
        let write_txn = db.begin_write().expect("begin write");
        db.write_table(&write_txn, TEST_TABLE, &["hello".to_owned()], |s| {
            Path::new(s.as_str())
        })
        .expect("write table");
        write_txn.commit().expect("commit");

        let read_txn = db.begin_read().expect("begin read");
        let loaded: Vec<String> = db
            .read_table(&read_txn, TEST_TABLE, |s: &String| {
                Path::new(s.as_str())
            })
            .expect("read table");
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
                .insert("corrupt.md".as_bytes(), [0xFF, 0xFF].as_slice())
                .expect("insert corrupt");
        }
        write_txn.commit().expect("commit");

        let read_txn = db.begin_read().expect("begin read");
        let result: Result<Vec<String>, DbError> =
            db.read_table(&read_txn, TEST_TABLE, |s: &String| {
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

            let (files, notes, links) =
                store.read_all().expect("load empty database");

            assert_eq!(files.len(), 0);
            assert_eq!(notes.len(), 0);
            assert_eq!(links.len(), 0);
        }

        #[test]
        fn replace_all_then_read_all_round_trips_records_and_notes() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(
                temp.path().join("note.md"),
                "---\ntitle: Hello\n---\nPriority:: 5\n- [ ] task",
            )
            .expect("write note");
            let files =
                IndexerService::new(temp.path()).scan().expect("scan root");
            let note = parse_markdown(
                "note.md",
                "---\ntitle: Hello\n---\nPriority:: 5\n- [ ] task",
            );
            let notes = vec![note];
            let store = IndexStore::open(temp.path()).expect("open store");

            store
                .replace_all(&files, &notes, &HashMap::new())
                .expect("persist records");
            let (loaded_records, loaded_notes, _) =
                store.read_all().expect("load records");

            assert_eq!(loaded_records, files);
            assert_eq!(loaded_notes, notes);
        }

        #[test]
        fn replace_all_then_read_all_round_trips_links() {
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
            let (_, _, loaded_links) = store.read_all().expect("load links");

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
            let (_, _, loaded_links) = store.read_all().expect("load links");

            assert_eq!(loaded_links.len(), 0);
        }

        #[test]
        fn replace_all_drops_records_absent_from_the_new_set() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("stale.md"), "old")
                .expect("write stale");
            let stale =
                IndexerService::new(temp.path()).scan().expect("scan stale");
            let store = IndexStore::open(temp.path()).expect("open store");
            store
                .replace_all(&stale, &[], &HashMap::new())
                .expect("persist stale");
            fs::remove_file(temp.path().join("stale.md"))
                .expect("remove stale");
            fs::write(temp.path().join("fresh.md"), "new")
                .expect("write fresh");
            let fresh =
                IndexerService::new(temp.path()).scan().expect("scan fresh");

            store
                .replace_all(&fresh, &[], &HashMap::new())
                .expect("persist fresh");
            let (loaded_records, _loaded_notes, _) =
                store.read_all().expect("load records");

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
                store.read_all().expect("load records");

            assert_eq!(loaded_records.len(), 0);
            assert_eq!(loaded_notes.len(), 0);
        }

        #[test]
        fn round_trips_a_record_with_a_unicode_path() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("café ☕.md"), "content")
                .expect("write unicode-named file");
            let files =
                IndexerService::new(temp.path()).scan().expect("scan root");
            let store = IndexStore::open(temp.path()).expect("open store");

            store
                .replace_all(&files, &[], &HashMap::new())
                .expect("persist records");
            let (loaded_records, ..) = store.read_all().expect("load records");

            assert_eq!(loaded_records, files);
        }

        #[test]
        fn returns_records_in_path_sort_order() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::create_dir_all(temp.path().join("a-b")).expect("mkdir a-b");
            fs::create_dir_all(temp.path().join("a")).expect("mkdir a");
            fs::write(temp.path().join("a-b/c.md"), "1")
                .expect("write a-b/c.md");
            fs::write(temp.path().join("a/z.md"), "2").expect("write a/z.md");
            let files =
                IndexerService::new(temp.path()).scan().expect("scan root");
            let store = IndexStore::open(temp.path()).expect("open store");
            store
                .replace_all(&files, &[], &HashMap::new())
                .expect("persist records");

            let (loaded_records, ..) = store.read_all().expect("load records");

            assert_eq!(loaded_records, files);
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

        #[test]
        fn recovers_by_rebuilding_when_the_files_table_has_the_old_str_key_schema()
         {
            let temp = tempfile::tempdir().expect("create temp dir");
            let root = temp.path();
            let db_path = root.join(INDEX_FILE);
            fs::create_dir_all(
                db_path.parent().expect("index file path has a parent"),
            )
            .expect("create .traces dir");
            {
                const OLD_FILES: TableDefinition<&str, &[u8]> =
                    TableDefinition::new("files");
                let db =
                    redb::Database::create(&db_path).expect("create raw db");
                let write_txn = db.begin_write().expect("begin write");
                {
                    let mut table = write_txn
                        .open_table(OLD_FILES)
                        .expect("open old table");
                    table
                        .insert("old.md", [1u8, 2, 3].as_slice())
                        .expect("insert old row");
                }
                write_txn.commit().expect("commit old schema");
            }

            let store = IndexStore::open(root)
                .expect("open recovers from schema mismatch");
            let (files, notes, links) =
                store.read_all().expect("load after recovery");

            assert!(files.is_empty());
            assert!(notes.is_empty());
            assert!(links.is_empty());
        }
    }

    mod is_rebuild_trigger {
        use super::*;

        #[test]
        fn accepts_table_type_mismatch_as_a_trigger() {
            let error = redb::TableError::TableTypeMismatch {
                table: "files".to_owned(),
                key: redb::TypeName::new("&str"),
                value: redb::TypeName::new("&[u8]"),
            };

            assert!(IndexStore::is_rebuild_trigger(&error));
        }

        #[test]
        fn accepts_type_definition_changed_as_a_trigger() {
            let error = redb::TableError::TypeDefinitionChanged {
                name: redb::TypeName::new("&[u8]"),
                alignment: 1,
                width: None,
            };

            assert!(IndexStore::is_rebuild_trigger(&error));
        }

        #[test]
        fn accepts_storage_corrupted_as_a_trigger() {
            // The one arm added beyond the ticket's literal
            // `TableTypeMismatch`/`TypeDefinitionChanged` pair: per-table
            // structural corruption surfaced by `open_table`/
            // `open_multimap_table` itself, distinct from the
            // container-level `DatabaseError::Storage(StorageError::
            // Corrupted)` `create_db` already catches. Exercising this
            // through a real `IndexStore::open` call would require
            // hand-crafting a redb file corrupted at exactly one table's
            // B-tree while leaving the container header/checksums valid —
            // infeasible to construct reliably without redb's own
            // on-disk-format internals, so the predicate is proven
            // directly here instead.
            let error = redb::TableError::Storage(
                redb::StorageError::Corrupted("simulated".to_owned()),
            );

            assert!(IndexStore::is_rebuild_trigger(&error));
        }

        #[test]
        fn rejects_table_does_not_exist() {
            let error = redb::TableError::TableDoesNotExist("files".to_owned());

            assert!(!IndexStore::is_rebuild_trigger(&error));
        }

        #[test]
        fn rejects_a_non_corrupted_storage_error() {
            let error =
                redb::TableError::Storage(redb::StorageError::DatabaseClosed);

            assert!(!IndexStore::is_rebuild_trigger(&error));
        }
    }

    mod read_all {
        use rstest::rstest;

        use super::*;

        #[rstest]
        #[case::file_records(FILES)]
        #[case::notes(NOTES)]
        fn returns_deserialize_error_when_stored_bytes_are_invalid(
            #[case] table_def: TableDefinition<&[u8], &[u8]>,
        ) {
            let temp = tempfile::tempdir().expect("create temp dir");
            let store = IndexStore::open(temp.path()).expect("open store");
            write_raw_value(&store, table_def, "bad.md", &[0xFF, 0xFE]);

            let error =
                store.read_all().expect_err("invalid bytes fail to load");

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
            let files =
                IndexerService::new(temp.path()).scan().expect("scan root");
            let store = IndexStore::open(temp.path()).expect("open store");
            store
                .replace_all(&files, &[], &HashMap::new())
                .expect("persist records");

            let read_txn = store.db.begin_read().expect("begin read txn");
            let table = read_txn.open_table(FILES).expect("open table");
            let raw = table
                .get("note.md".as_bytes())
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

    mod backdating {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn refresh_fails_open_when_a_previous_notes_row_is_corrupted() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("note.md"), "# Draft")
                .expect("write note");
            let indexer = crate::index::IndexerService::new(temp.path());
            indexer
                .persist(&indexer.build().expect("build index"))
                .expect("persist index");

            let store = IndexStore::open(temp.path()).expect("reopen store");
            write_raw_value(&store, NOTES, "note.md", &[0xFF, 0xFE]);
            drop(store); // release the db handle before indexer.refresh() opens its own

            fs::write(temp.path().join("note.md"), "# Revised")
                .expect("rewrite note");

            let refreshed = indexer.refresh().expect(
                "refresh must fail open on a corrupted previous note during \
                 backdating, not error",
            );
            assert_eq!(refreshed.notes().len(), 1);
        }
    }
}
