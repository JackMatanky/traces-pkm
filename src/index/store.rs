//! Redb persistence for [`FileBase`], [`Note`], and derived inlink records.
//!
//! [`IndexStore`] owns one redb connection and adapts it to the file-index
//! schema (`FILES`, `NOTES`, `LINKS` tables). Callers use
//! [`super::IndexerService`] methods instead of interacting with tables
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

use super::{
    FileIndex, INDEX_FILE,
    codec::{decode_row, encode_row, path_from_bytes},
    delta::{IncrementalDelta, IndexDelta},
    entry::FileEntry,
    error::{DbError, DbResult, IndexResult},
    inlinks::InlinkMap,
};
use crate::{file::FileBase, note::Note};

/// File metadata table.
///
/// Stores [`FileBase`] records for every regular file under the project root.
///
/// Key: project-relative path as UTF-8 bytes
/// Value: serialized [`FileBase`]
const FILES: TableDefinition<&[u8], &[u8]> = TableDefinition::new("files");

/// Parsed note metadata table.
///
/// Stores [`Note`] records for every Markdown file under the project root.
///
/// Key: project-relative path as UTF-8 bytes
/// Value: serialized [`Note`]
const NOTES: TableDefinition<&[u8], &[u8]> = TableDefinition::new("notes");

/// Inbound link multimap table.
///
/// Maps each note to every other note whose outlinks resolve to it. See
/// [`super::inlinks`].
///
/// Key: target note path as UTF-8 bytes
/// Value: one source note path per entry
const LINKS: MultimapTableDefinition<&[u8], &[u8]> =
    MultimapTableDefinition::new("links");

/// Persisted snapshot of [`FileBase`]s, [`Note`]s (sorted by path), and
/// inbound link edges (target-keyed, unordered).
pub(super) type IndexSnapshot = (Vec<FileBase>, Vec<Note>, InlinkMap);

/// One raw `LINKS` multimap-table iterator entry: a target key's `AccessGuard`
/// paired with its source-set `MultimapValue`, or the `redb::StorageError`
/// reading it failed with.
type LinkEntry<'a> = Result<
    (
        redb::AccessGuard<'a, &'static [u8]>,
        redb::MultimapValue<'a, &'static [u8]>,
    ),
    redb::StorageError,
>;

/// One resolved `LINKS` row: a target path and its surviving source paths, or
/// `None` when the target resolved to no loaded note or all its sources
/// dropped.
type ResolvedLink = Option<(PathBuf, Vec<PathBuf>)>;

/// Redb-backed handle to one project root's index database.
///
/// Created by [`Self::open`]. Callers interact through [`IndexerService`]
/// methods, not directly.
///
/// [`IndexerService`]: super::IndexerService
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
    /// - [`DbError::Io`] if the database's parent directory cannot be created,
    ///   or if a corrupted or schema-mismatched file cannot be deleted during
    ///   recovery.
    /// - [`DbError::Redb`] if the database file cannot be opened, or a
    ///   post-recovery re-create fails.
    pub(super) fn open(root: &Path) -> IndexResult<Self> {
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
    fn create_db(path: &Path) -> DbResult<redb::Database> {
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
    fn should_rebuild(db: &redb::Database, path: &Path) -> DbResult<bool> {
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

    /// Returns `true` for schema drift or structural corruption that warrants
    /// a wipe-and-rebuild.
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
    pub(super) fn begin_read(&self) -> DbResult<ReadTransaction> {
        self.db.begin_read().map_err(|source| self.raise_source_error(source))
    }

    /// Begins a write transaction.
    ///
    /// # Errors
    ///
    /// - [`DbError::Redb`] if the transaction cannot be started.
    pub(super) fn begin_write(&self) -> DbResult<WriteTransaction> {
        self.db.begin_write().map_err(|source| self.raise_source_error(source))
    }

    /// Deserializes every value in `table` and sorts by `path_of`.
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
    ) -> DbResult<Vec<T>> {
        let mut items: Vec<T> = match txn.open_table(table) {
            Ok(table) => {
                let mut items = Vec::new();
                for entry in table
                    .iter()
                    .map_err(|source| self.raise_source_error(source))?
                {
                    let (key, value) = entry
                        .map_err(|source| self.raise_source_error(source))?;
                    let path = path_from_bytes(key.value());
                    items.push(decode_row(&path, value.value())?);
                }
                items
            }
            Err(redb::TableError::TableDoesNotExist(_)) => Vec::new(),
            Err(source) => return Err(self.raise_source_error(source)),
        };
        items.sort_by(|a, b| path_of(a).cmp(path_of(b)));
        Ok(items)
    }

    /// Loads every stored [`FileBase`] and [`Note`] (sorted by path) and every
    /// derived inlink edge. Stale or orphaned edges are dropped.
    ///
    /// # Errors
    ///
    /// - [`DbError::Redb`] if a table cannot be read.
    /// - [`DbError::Deserialize`] if stored bytes are not a valid record.
    pub(super) fn read_all(&self) -> IndexResult<IndexSnapshot> {
        let txn = self.begin_read()?;
        let files = self.read_table(&txn, FILES, FileBase::path)?;
        let notes = self.read_table(&txn, NOTES, Note::path)?;
        let links = {
            let by_bytes: HashMap<&[u8], &Path> = notes
                .iter()
                .map(|note| {
                    (note.path().as_os_str().as_encoded_bytes(), note.path())
                })
                .collect();
            self.read_links(&txn, LINKS, |bytes| {
                by_bytes.get(bytes).map(|path| path.to_path_buf())
            })?
        };
        Ok((files, notes, links))
    }

    /// Loads every persisted [`FileBase`] (sorted by path) and inlink edge,
    /// without loading the `NOTES` table.
    ///
    /// Used by [`super::IndexerService::refresh`] for lazy per-note recall via
    /// [`Self::read_note`].
    ///
    /// # Errors
    ///
    /// - [`DbError::Redb`] if a table cannot be read.
    /// - [`DbError::Deserialize`] if stored bytes are not a valid record.
    pub(super) fn read_files_and_links_via(
        &self,
        txn: &ReadTransaction,
    ) -> IndexResult<(Vec<FileBase>, InlinkMap)> {
        let files = self.read_table(txn, FILES, FileBase::path)?;
        let links =
            self.read_links(txn, LINKS, |bytes| Some(path_from_bytes(bytes)))?;
        Ok((files, links))
    }

    /// Reads exactly one [`Note`] by path, used by
    /// [`super::cache::RefreshCache::reconcile_note`] to recall an unchanged
    /// Note.
    ///
    /// # Errors
    ///
    /// - [`DbError::Redb`] if the table cannot be read.
    /// - [`DbError::Deserialize`] if the stored bytes are corrupt.
    pub(super) fn read_note(
        &self,
        txn: &ReadTransaction,
        path: &Path,
    ) -> IndexResult<Option<Note>> {
        let table = match txn.open_table(NOTES) {
            Ok(table) => table,
            Err(redb::TableError::TableDoesNotExist(_)) => return Ok(None),
            Err(source) => return Err(self.raise_source_error(source).into()),
        };
        let key = path.as_os_str().as_encoded_bytes();
        match table
            .get(key)
            .map_err(|source| self.raise_source_error(source))?
        {
            None => Ok(None),
            Some(guard) => Ok(Some(decode_row(path, guard.value())?)),
        }
    }

    /// Deserializes every `target -> sources` edge from the `links` multimap
    /// table, resolving each stored path's raw bytes through `resolve`.
    ///
    /// `resolve` maps a stored key/value's raw bytes to the authoritative path
    /// to use, or `None` to drop it. A target that resolves to `None` drops its
    /// whole edge set; a source that resolves to `None` is skipped; an entry
    /// left with no surviving sources is omitted entirely.
    ///
    /// # Errors
    ///
    /// - [`DbError::Redb`] if the table cannot be read.
    pub(super) fn read_links(
        &self,
        txn: &ReadTransaction,
        table: MultimapTableDefinition<&[u8], &[u8]>,
        resolve: impl Fn(&[u8]) -> Option<PathBuf>,
    ) -> DbResult<HashMap<PathBuf, Vec<PathBuf>>> {
        let table = match txn.open_multimap_table(table) {
            Ok(table) => table,
            Err(redb::TableError::TableDoesNotExist(_)) => {
                return Ok(HashMap::new());
            }
            Err(source) => return Err(self.raise_source_error(source)),
        };
        let mut links = HashMap::new();
        for entry in
            table.iter().map_err(|source| self.raise_source_error(source))?
        {
            if let Some((target, sources)) =
                self.process_link_entry(entry, &resolve)?
            {
                links.insert(target, sources);
            }
        }
        Ok(links)
    }

    /// Extracts one `target -> sources` row from a `LINKS` multimap iterator
    /// entry, resolving raw bytes through `resolve`. Returns `None` when the
    /// target resolves to no path or when every source dropped.
    fn process_link_entry(
        &self,
        entry: LinkEntry<'_>,
        resolve: &impl Fn(&[u8]) -> Option<PathBuf>,
    ) -> DbResult<ResolvedLink> {
        let (target, sources) =
            entry.map_err(|source| self.raise_source_error(source))?;
        let Some(target) = resolve(target.value()) else {
            return Ok(None);
        };
        let sources = self.collect_sources(sources, resolve)?;
        if sources.is_empty() {
            return Ok(None);
        }
        Ok(Some((target, sources)))
    }

    /// Collects source paths from a `MultimapValue` iterator, skipping
    /// unresolvable entries.
    fn collect_sources(
        &self,
        sources: redb::MultimapValue<'_, &[u8]>,
        resolve: &impl Fn(&[u8]) -> Option<PathBuf>,
    ) -> DbResult<Vec<PathBuf>> {
        let mut values = Vec::new();
        for source in sources {
            let source =
                source.map_err(|source| self.raise_source_error(source))?;
            if let Some(path) = resolve(source.value()) {
                values.push(path);
            }
        }
        Ok(values)
    }

    /// Serializes `items` into `table`, keyed by `path_of`.
    ///
    /// # Errors
    ///
    /// - [`DbError::Redb`] if the table cannot be opened or written.
    /// - [`DbError::Serialize`] if an item cannot be encoded.
    pub(super) fn write_table<'a, T: Serialize + 'a>(
        &self,
        txn: &WriteTransaction,
        table: TableDefinition<&[u8], &[u8]>,
        items: impl IntoIterator<Item = &'a T>,
        path_of: impl Fn(&T) -> &Path,
    ) -> DbResult<()> {
        let mut table = txn
            .open_table(table)
            .map_err(|source| self.raise_source_error(source))?;
        for item in items {
            let path = path_of(item);
            let key = path.as_os_str().as_encoded_bytes();
            let value = encode_row(path, item)?;
            table
                .insert(key, value.as_slice())
                .map_err(|source| self.raise_source_error(source))?;
        }
        Ok(())
    }

    /// Writes every [`FileEntry`]'s inbound-link edges into the `links`
    /// multimap table.
    ///
    /// # Errors
    ///
    /// - [`DbError::Redb`] if the table cannot be opened or written.
    pub(super) fn write_links(
        &self,
        txn: &WriteTransaction,
        table: MultimapTableDefinition<&[u8], &[u8]>,
        entries: &[FileEntry],
    ) -> DbResult<()> {
        let mut table = txn
            .open_multimap_table(table)
            .map_err(|source| self.raise_source_error(source))?;
        for entry in entries {
            let inlinks = entry.inlinks();
            if inlinks.is_empty() {
                continue;
            }
            let target_key = entry.file().path().as_os_str().as_encoded_bytes();
            for source in inlinks {
                table
                    .insert(target_key, source.as_os_str().as_encoded_bytes())
                    .map_err(|source| self.raise_source_error(source))?;
            }
        }
        Ok(())
    }

    /// Atomically replaces every stored [`FileBase`], [`Note`], and derived
    /// inlink edge.
    ///
    /// # Errors
    ///
    /// - [`DbError::Redb`] if the transaction fails.
    /// - [`DbError::Serialize`] if a record cannot be encoded.
    pub(super) fn write_all(&self, entries: &[FileEntry]) -> IndexResult<()> {
        let write_txn = self.begin_write()?;
        write_txn
            .delete_table(FILES)
            .map_err(|source| self.raise_source_error(source))?;
        write_txn
            .delete_table(NOTES)
            .map_err(|source| self.raise_source_error(source))?;
        write_txn
            .delete_multimap_table(LINKS)
            .map_err(|source| self.raise_source_error(source))?;
        self.write_table(
            &write_txn,
            FILES,
            entries.iter().map(FileEntry::file),
            FileBase::path,
        )?;
        self.write_table(
            &write_txn,
            NOTES,
            entries.iter().filter_map(FileEntry::note),
            Note::path,
        )?;
        self.write_links(&write_txn, LINKS, entries)?;
        write_txn.commit().map_err(|source| self.raise_source_error(source))?;
        Ok(())
    }

    /// Persists `index`, choosing a full [`Self::write_all`] rewrite when its
    /// delta is [`IndexDelta::Full`], or a row-level incremental write for
    /// [`IndexDelta::Incremental`]'s changed paths only.
    ///
    /// # Errors
    ///
    /// Transaction failure or serialization failure, same as
    /// [`Self::write_all`]/the incremental write path.
    ///
    /// [`IndexDelta::Full`]: super::delta::IndexDelta::Full
    /// [`IndexDelta::Incremental`]: super::delta::IndexDelta::Incremental
    pub(super) fn persist_index(&self, index: &FileIndex) -> IndexResult<()> {
        match index.delta() {
            IndexDelta::Full => self.write_all(index.entries()),
            IndexDelta::Incremental(_) => self.persist_incremental(index),
        }
    }

    /// Row-level incremental write for [`IndexDelta::Incremental`].
    ///
    /// Falls back to [`Self::write_all`] if `index`'s delta turns out to be
    /// [`IndexDelta::Full`].
    ///
    /// [`IndexDelta::Full`]: super::delta::IndexDelta::Full
    /// [`IndexDelta::Incremental`]: super::delta::IndexDelta::Incremental
    fn persist_incremental(&self, index: &FileIndex) -> IndexResult<()> {
        let IndexDelta::Incremental(delta) = index.delta() else {
            return self.write_all(index.entries());
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
            .map_err(|source| self.raise_source_error(source))?;
        self.upsert_files_and_notes(&write_txn, index, upserted, deleted)?;
        if let Some(links_upserted) = links_upserted {
            self.upsert_links(
                &write_txn,
                index,
                links_upserted,
                links_deleted,
            )?;
        }
        write_txn.commit().map_err(|source| self.raise_source_error(source))?;
        Ok(())
    }

    /// Deletes `deleted` paths from `FILES`/`NOTES`, then upserts each
    /// `upserted` path's current [`FileBase`] and [`Note`] (if present).
    fn upsert_files_and_notes(
        &self,
        write_txn: &WriteTransaction,
        index: &FileIndex,
        upserted: &[PathBuf],
        deleted: &[PathBuf],
    ) -> IndexResult<()> {
        let mut files = write_txn
            .open_table(FILES)
            .map_err(|source| self.raise_source_error(source))?;
        let mut notes_table = write_txn
            .open_table(NOTES)
            .map_err(|source| self.raise_source_error(source))?;
        for path in deleted {
            let key = path.as_os_str().as_encoded_bytes();
            files
                .remove(key)
                .map_err(|source| self.raise_source_error(source))?;
            notes_table
                .remove(key)
                .map_err(|source| self.raise_source_error(source))?;
        }
        for path in upserted {
            if let Some(entry) = index
                .entries()
                .binary_search_by(|e| e.file().path().cmp(path))
                .ok()
                .and_then(|idx| index.entries().get(idx))
            {
                self.upsert_row(&mut files, path, entry.file())?;
                if let Some(note) = entry.note() {
                    self.upsert_row(&mut notes_table, path, note)?;
                }
            }
        }
        Ok(())
    }

    /// Serializes `value` and upserts it into `table` at `path`.
    fn upsert_row<T: Serialize>(
        &self,
        table: &mut redb::Table<'_, &[u8], &[u8]>,
        path: &Path,
        value: &T,
    ) -> IndexResult<()> {
        let key = path.as_os_str().as_encoded_bytes();
        let bytes = encode_row(path, value)?;
        table
            .insert(key, bytes.as_slice())
            .map_err(|source| self.raise_source_error(source))?;
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
    ) -> IndexResult<()> {
        let mut links = write_txn
            .open_multimap_table(LINKS)
            .map_err(|source| self.raise_source_error(source))?;
        for target in links_deleted {
            links
                .remove_all(target.as_os_str().as_encoded_bytes())
                .map_err(|source| self.raise_source_error(source))?;
        }
        for target in links_upserted {
            self.upsert_link_target(&mut links, index, target)?;
        }
        Ok(())
    }

    /// Rewrites one target's source set in `links` from `index`'s entries.
    fn upsert_link_target(
        &self,
        links: &mut redb::MultimapTable<'_, &[u8], &[u8]>,
        index: &FileIndex,
        target: &Path,
    ) -> IndexResult<()> {
        let target_key = target.as_os_str().as_encoded_bytes();
        links
            .remove_all(target_key)
            .map_err(|source| self.raise_source_error(source))?;
        let sources = index
            .entries()
            .binary_search_by(|e| e.file().path().cmp(target))
            .ok()
            .and_then(|idx| index.entries().get(idx))
            .map_or(&[][..], FileEntry::inlinks);
        if sources.is_empty() {
            return Ok(());
        }
        for source in sources {
            links
                .insert(target_key, source.as_os_str().as_encoded_bytes())
                .map_err(|source| self.raise_source_error(source))?;
        }
        Ok(())
    }

    /// Wraps a redb error with this store's database path.
    fn raise_source_error(&self, source: impl Into<redb::Error>) -> DbError {
        DbError::Redb {
            path: self.path.clone(),
            source: Box::new(source.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::HashMap,
        fs,
        path::{Path, PathBuf},
    };

    use super::{super::IndexError, *};
    #[cfg(unix)]
    use crate::index::tests::fixtures::RestorePermissions;
    use crate::{
        index::IndexerService,
        note::{MarkdownParserInput, parse_markdown},
    };

    fn parse(path: impl AsRef<Path>, src: &str) -> Note {
        let input = MarkdownParserInput::for_test(path.as_ref(), src);
        parse_markdown(&input)
    }

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

    /// Persists the three-table tuple the way production does, by assembling
    /// `FileEntry` rows first. Test-side mirror of the old three-argument
    /// `write_all`; `files` and `notes` must both be path-sorted and every
    /// `links` target must name a Note in `notes`.
    fn write_all_parts(
        store: &IndexStore,
        files: &[FileBase],
        notes: &[Note],
        links: &HashMap<PathBuf, Vec<PathBuf>>,
    ) -> IndexResult<()> {
        let entries = crate::index::entry::assemble_entries(
            files.to_vec(),
            notes.to_vec(),
            links.clone(),
        );
        store.write_all(&entries)
    }

    /// Writes one raw `LINKS` row directly, bypassing entry assembly, used
    /// to persist orphan edges the entry-shaped `write_all` structurally
    /// cannot express.
    fn write_raw_link(store: &IndexStore, target: &Path, source: &Path) {
        let write_txn = store.db.begin_write().expect("begin write txn");
        {
            let mut table =
                write_txn.open_multimap_table(LINKS).expect("open links table");
            table
                .insert(
                    target.as_os_str().as_encoded_bytes(),
                    source.as_os_str().as_encoded_bytes(),
                )
                .expect("insert raw link");
        }
        write_txn.commit().expect("commit raw link");
    }

    /// Builds one [`FileBase`] per path, in the given (path-sorted) order,
    /// pairing with `parse_markdown` notes for entry assembly in tests.
    fn note_files(paths: &[&str]) -> Vec<FileBase> {
        paths
            .iter()
            .map(|p| {
                FileBase::new_test(
                    PathBuf::from(*p),
                    PathBuf::new(),
                    crate::file::FileFormat::Note,
                )
            })
            .collect()
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
        fn non_unicode_path() -> PathBuf {
            #[cfg(unix)]
            {
                use std::os::unix::ffi::OsStringExt as _;
                PathBuf::from(std::ffi::OsString::from_vec(
                    b"weird\xFF.md".to_vec(),
                ))
            }
            #[cfg(windows)]
            {
                use std::os::windows::ffi::OsStringExt as _;
                PathBuf::from(std::ffi::OsString::from_wide(&[
                    119, 101, 105, 114, 100, 0xD800, 46, 109, 100,
                ]))
            }
            #[cfg(not(any(unix, windows)))]
            {
                PathBuf::from("weird.md")
            }
        }

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
        fn write_all_then_read_all_round_trips_records_and_notes() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(
                temp.path().join("note.md"),
                "---\ntitle: Hello\n---\nPriority:: 5\n- [ ] task",
            )
            .expect("write note");
            let files =
                IndexerService::new(temp.path()).scan().expect("scan root");
            let note = parse(
                "note.md",
                "---\ntitle: Hello\n---\nPriority:: 5\n- [ ] task",
            );
            let notes = vec![note];
            let store = IndexStore::open(temp.path()).expect("open store");

            write_all_parts(&store, &files, &notes, &HashMap::new())
                .expect("persist records");
            let (loaded_records, loaded_notes, _) =
                store.read_all().expect("load records");

            assert_eq!(loaded_records, files);
            assert_eq!(loaded_notes, notes);
        }

        #[test]
        fn write_all_then_read_all_round_trips_links() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let store = IndexStore::open(temp.path()).expect("open store");
            let links = HashMap::from([
                (PathBuf::from("target.md"), vec![
                    PathBuf::from("a.md"),
                    PathBuf::from("b.md"),
                ]),
                (PathBuf::from("other.md"), vec![PathBuf::from("a.md")]),
            ]);

            let notes: Vec<_> = ["a.md", "b.md", "other.md", "target.md"]
                .iter()
                .map(|p| parse(*p, ""))
                .collect();
            let files: Vec<_> = ["a.md", "b.md", "other.md", "target.md"]
                .iter()
                .map(|p| {
                    FileBase::new_test(
                        PathBuf::from(*p),
                        PathBuf::new(),
                        crate::file::FileFormat::Note,
                    )
                })
                .collect();
            write_all_parts(&store, &files, &notes, &links)
                .expect("persist links");
            let (_, _, loaded_links) = store.read_all().expect("load links");

            assert_eq!(loaded_links, links);
        }

        #[test]
        fn read_all_drops_link_edges_with_no_matching_note() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let store = IndexStore::open(temp.path()).expect("open store");
            let notes: Vec<_> =
                ["a.md", "target.md"].iter().map(|p| parse(*p, "")).collect();
            // The valid edge rides through entry assembly; the orphan
            // edges must reach disk raw, since `write_all` structurally
            // cannot express a link to an unindexed target or source.
            write_all_parts(
                &store,
                &note_files(&["a.md", "target.md"]),
                &notes,
                &HashMap::from([(PathBuf::from("target.md"), vec![
                    PathBuf::from("a.md"),
                ])]),
            )
            .expect("persist valid links");
            write_raw_link(
                &store,
                Path::new("target.md"),
                Path::new("ghost.md"),
            );
            write_raw_link(
                &store,
                Path::new("ghost-target.md"),
                Path::new("a.md"),
            );
            let (_, _, loaded_links) = store.read_all().expect("load links");

            assert_eq!(
                loaded_links,
                HashMap::from([(PathBuf::from("target.md"), vec![
                    PathBuf::from("a.md"),
                ])])
            );
        }

        #[test]
        fn read_all_drops_a_target_whose_sources_are_all_orphaned() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let store = IndexStore::open(temp.path()).expect("open store");
            let notes: Vec<_> =
                ["a.md", "b.md"].iter().map(|p| parse(*p, "")).collect();
            // The valid edge rides through entry assembly; the orphan
            // source must reach disk raw.
            write_all_parts(
                &store,
                &note_files(&["a.md", "b.md"]),
                &notes,
                &HashMap::from([(PathBuf::from("b.md"), vec![PathBuf::from(
                    "a.md",
                )])]),
            )
            .expect("persist valid links");
            write_raw_link(&store, Path::new("a.md"), Path::new("ghost.md"));
            let (_, _, loaded_links) = store.read_all().expect("load links");

            assert_eq!(
                loaded_links,
                HashMap::from([(PathBuf::from("b.md"), vec![PathBuf::from(
                    "a.md"
                ),])])
            );
        }

        #[test]
        fn read_files_and_links_via_keeps_orphaned_edges_that_read_all_drops() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let store = IndexStore::open(temp.path()).expect("open store");
            let notes: Vec<_> =
                ["a.md", "target.md"].iter().map(|p| parse(*p, "")).collect();
            let links = HashMap::from([
                (PathBuf::from("target.md"), vec![
                    PathBuf::from("a.md"),
                    PathBuf::from("ghost.md"),
                ]),
                (PathBuf::from("ghost-target.md"), vec![PathBuf::from("a.md")]),
            ]);
            write_all_parts(
                &store,
                &note_files(&["a.md", "target.md"]),
                &notes,
                &HashMap::from([(PathBuf::from("target.md"), vec![
                    PathBuf::from("a.md"),
                ])]),
            )
            .expect("persist valid links");
            write_raw_link(
                &store,
                Path::new("target.md"),
                Path::new("ghost.md"),
            );
            write_raw_link(
                &store,
                Path::new("ghost-target.md"),
                Path::new("a.md"),
            );

            // The refresh path reconstructs without correlating, so every
            // persisted edge survives — proving the orphans are on disk and
            // that only read_all's correlation drops them.
            let txn = store.begin_read().expect("read txn");
            let (_, reconstructed) =
                store.read_files_and_links_via(&txn).expect("reconstruct load");

            assert_eq!(reconstructed, links);
        }

        #[test]
        fn write_all_drops_links_absent_from_the_new_set() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let store = IndexStore::open(temp.path()).expect("open store");
            write_raw_link(&store, Path::new("target.md"), Path::new("a.md"));

            write_all_parts(&store, &[], &[], &HashMap::new())
                .expect("persist empty links");
            let (_, _, loaded_links) = store.read_all().expect("load links");

            assert_eq!(loaded_links.len(), 0);
        }

        #[test]
        fn write_all_drops_records_absent_from_the_new_set() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("stale.md"), "old")
                .expect("write stale");
            let stale =
                IndexerService::new(temp.path()).scan().expect("scan stale");
            let store = IndexStore::open(temp.path()).expect("open store");
            write_all_parts(&store, &stale, &[], &HashMap::new())
                .expect("persist stale");
            fs::remove_file(temp.path().join("stale.md"))
                .expect("remove stale");
            fs::write(temp.path().join("fresh.md"), "new")
                .expect("write fresh");
            let fresh =
                IndexerService::new(temp.path()).scan().expect("scan fresh");

            write_all_parts(&store, &fresh, &[], &HashMap::new())
                .expect("persist fresh");
            let (loaded_records, _loaded_notes, _) =
                store.read_all().expect("load records");

            assert_eq!(loaded_records, fresh);
        }

        #[test]
        fn write_all_with_no_records_persists_an_empty_table() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let store = IndexStore::open(temp.path()).expect("open store");

            write_all_parts(&store, &[], &[], &HashMap::new())
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

            write_all_parts(&store, &files, &[], &HashMap::new())
                .expect("persist records");
            let (loaded_records, ..) = store.read_all().expect("load records");

            assert_eq!(loaded_records, files);
        }

        #[test]
        fn round_trips_a_record_with_a_non_unicode_path() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let store = IndexStore::open(temp.path()).expect("open store");

            let weird_path = non_unicode_path();

            let file = FileBase::new_test(
                weird_path.clone(),
                PathBuf::new(),
                crate::file::FileFormat::Note,
            );

            let note = parse(&weird_path, "content");
            let files = vec![file];
            let notes = vec![note];

            write_all_parts(&store, &files, &notes, &HashMap::new())
                .expect("persist records");
            let (loaded_records, loaded_notes, _) =
                store.read_all().expect("load records");

            assert_eq!(loaded_records, files);
            assert_eq!(loaded_notes, notes);
        }

        #[test]
        fn load_returns_byte_exact_inlinks_for_a_non_unicode_note() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let store = IndexStore::open(temp.path()).expect("open store");
            let weird = non_unicode_path();
            let normal = PathBuf::from("normal.md");
            let mut files = vec![
                FileBase::new_test(
                    weird.clone(),
                    PathBuf::new(),
                    crate::file::FileFormat::Note,
                ),
                FileBase::new_test(
                    normal.clone(),
                    PathBuf::new(),
                    crate::file::FileFormat::Note,
                ),
            ];
            files.sort_by(|a, b| a.path().cmp(b.path()));
            let mut notes = vec![
                parse(&weird, "link to [[normal]]"),
                parse(&normal, "link to [[weird]]"),
            ];
            notes.sort_by(|a, b| a.path().cmp(b.path()));
            let links = HashMap::from([
                (weird.clone(), vec![normal.clone()]),
                (normal.clone(), vec![weird.clone()]),
            ]);
            write_all_parts(&store, &files, &notes, &links).expect("persist");
            drop(store);

            let loaded =
                IndexerService::new(temp.path()).load().expect("load index");
            let inlinks_of = |target: &Path| {
                loaded
                    .entries()
                    .iter()
                    .find(|entry| entry.file().path() == target)
                    .map_or_else(Vec::new, |entry| entry.inlinks().to_vec())
            };
            assert_eq!(inlinks_of(weird.as_path()), vec![normal.clone()]);
            assert_eq!(inlinks_of(normal.as_path()), vec![weird]);
        }

        #[cfg(target_os = "linux")]
        #[test]
        fn scan_preserves_a_non_unicode_path() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let weird = non_unicode_path();
            fs::write(temp.path().join(&weird), "# weird")
                .expect("write weird note");

            let files =
                IndexerService::new(temp.path()).scan().expect("scan root");

            assert!(files.iter().any(|file| file.path() == weird));
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
            write_all_parts(&store, &files, &[], &HashMap::new())
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
            write_all_parts(&store, &files, &[], &HashMap::new())
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
            assert_eq!(
                refreshed
                    .entries()
                    .iter()
                    .filter(|entry| entry.note().is_some())
                    .count(),
                1
            );
        }
    }
}
