//! Redb persistence for [`FileBase`], [`Note`], and derived inlinks.
//!
//! [`IndexStore`] owns the database connection and drives the schema in
//! `super::tables`; callers use [`super::IndexerService`] rather than direct
//! table access.

use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
};

use rayon::prelude::*;
use redb::{
    MultimapTableDefinition, ReadTransaction, ReadableDatabase as _,
    ReadableMultimapTable as _, ReadableTable as _, ReadableTableMetadata as _,
    TableDefinition, WriteTransaction,
};
use rustc_hash::{FxBuildHasher, FxHashMap};
use serde::{Serialize, de::DeserializeOwned};

use super::{
    INDEX_FILE,
    codec::{PathKey, decode_row, encode_row, path_from_bytes},
    delta::{FileDelta, InlinkDelta},
    entry::FileEntry,
    error::{IndexError, IndexResult, StoreError, StoreResult},
    inlinks::InlinkMap,
    sort::SortedByPath,
    tables::{
        FILE_CLASSES_BY_PATH, FILES, LINKS, NOTES, PATHS_BY_FILE_CLASS,
        PATHS_BY_TAG, TABLES, TAGS_BY_PATH,
    },
};
use crate::{FileBase, Note, Tag, file::FileFormat};

/// Stored files, path-sorted notes, and target-keyed inlinks loaded together.
pub(super) type StoreSnapshot =
    (SortedByPath<FileBase>, SortedByPath<Note>, InlinkMap);

/// Store-owned persistence request for full rebuilds and incremental refreshes.
pub(super) struct PersistRequest<'a> {
    axes: IndexAxes,
    rows: PersistRows<'a>,
}

impl<'a> PersistRequest<'a> {
    /// Replaces every persisted row from an assembled index.
    #[inline]
    pub(super) const fn rebuild(
        axes: IndexAxes,
        entries: &'a [FileEntry],
    ) -> Self {
        Self {
            axes,
            rows: PersistRows::Rebuild {
                entries,
            },
        }
    }

    /// Applies row-level changes from one refresh pass.
    #[inline]
    pub(super) const fn incremental(
        axes: IndexAxes,
        delta: &'a FileDelta,
        notes: &'a [&'a Note],
        edges: &'a InlinkDelta,
    ) -> Self {
        Self {
            axes,
            rows: PersistRows::Incremental {
                delta,
                notes,
                edges,
            },
        }
    }
}

/// Incremental row references passed from [`IndexStore::persist`] to its
/// transaction body.
struct IncrementalRows<'a> {
    delta: &'a FileDelta,
    notes: &'a [&'a Note],
    edges: &'a InlinkDelta,
}

/// Rows affected by a persistence request.
pub(super) enum PersistRows<'a> {
    /// Full cache rebuild from assembled entries.
    Rebuild {
        entries: &'a [FileEntry],
    },
    /// Incremental file, note, axis, and inlink changes.
    Incremental {
        delta: &'a FileDelta,
        notes: &'a [&'a Note],
        edges: &'a InlinkDelta,
    },
}

/// Raw `LINKS` iterator entry before path-byte resolution.
type LinkEntry<'a> = Result<
    (
        redb::AccessGuard<'a, &'static [u8]>,
        redb::MultimapValue<'a, &'static [u8]>,
    ),
    redb::StorageError,
>;

/// Resolved `LINKS` row, or `None` when target/source filtering empties it.
type ResolvedLink = Option<(PathBuf, Box<[PathBuf]>)>;

/// Raw note rows collected before parallel decoding.
type RawNoteRows = Vec<(PathBuf, Vec<u8>)>;

/// Read-only table typed for raw byte keys and values.
type BytesReadTable = redb::ReadOnlyTable<&'static [u8], &'static [u8]>;

/// Read-only multimap table typed for raw byte keys and values.
type BytesReadMultimapTable =
    redb::ReadOnlyMultimapTable<&'static [u8], &'static [u8]>;

/// Unboxed range iterator over redb table entries.
type TableRange<'a> = redb::Range<'a, &'static [u8], &'static [u8]>;

/// A multimap table typed for raw byte keys and values.
type BytesMultimapTable<'txn> =
    redb::MultimapTable<'txn, &'static [u8], &'static [u8]>;

/// Redb-backed handle to one project root's index database.
#[derive(Debug)]
pub(crate) struct IndexStore {
    db: redb::Database,
    path: PathBuf,
}

impl IndexStore {
    // --- Construction ------------------------------------------------

    /// Opens the index database under `root`, creating it if absent.
    ///
    /// Recovers by wipe-and-recreate if the existing file is corrupted or
    /// schema-mismatched.
    ///
    /// # Errors
    ///
    /// - [`Store`] if the database's parent directory cannot be created, the
    ///   database file cannot be opened, or a corrupted or schema-mismatched
    ///   file cannot be replaced during recovery.
    ///
    /// [`Store`]: IndexError::Store
    pub(crate) fn open(root: &Path) -> IndexResult<Self> {
        let path = root.join(INDEX_FILE);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|source| StoreError::Io {
                path: parent.to_path_buf(),
                source,
            })?;
        }
        let db = Self::create_db(&path)?;
        let db = if Self::rebuild_needed(&db, &path)? {
            drop(db);
            fs::remove_file(&path).map_err(|source| StoreError::Io {
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

    // --- Batch reads -------------------------------------------------

    /// Point-reads notes in one transaction, warning and skipping corrupted
    /// rows.
    ///
    /// # Errors
    ///
    /// - [`Store`] if opening the transaction or table fails.
    ///
    /// [`Store`]: IndexError::Store
    pub(crate) fn read_notes_batch<'a>(
        &self,
        paths: impl IntoIterator<Item = &'a Path>,
    ) -> IndexResult<Vec<Note>> {
        self.read_batch(ReadSource::Notes, paths)
    }

    /// Point-reads file metadata in one transaction, warning and skipping
    /// corrupted rows.
    ///
    /// # Errors
    ///
    /// - [`Store`] if opening the transaction or table fails.
    ///
    /// [`Store`]: IndexError::Store
    pub(crate) fn read_files_batch<'a>(
        &self,
        paths: impl IntoIterator<Item = &'a Path>,
    ) -> IndexResult<Vec<FileBase>> {
        self.read_batch(ReadSource::Files, paths)
    }

    /// Point-reads inbound-link edges for `targets`.
    ///
    /// Performs one indexed `LINKS` lookup per target, O(each target's source
    /// count), not a table scan. Query execution uses this for the handful of
    /// files it matched.
    ///
    /// # Errors
    ///
    /// - [`Store`] if opening the transaction or table fails.
    ///
    /// [`Store`]: IndexError::Store
    pub(crate) fn read_links_for_targets<'a>(
        &self,
        targets: impl IntoIterator<Item = &'a Path>,
    ) -> IndexResult<InlinkMap> {
        let txn = self.begin_read()?;
        let Some(table) = self.open_multimap_for_read(&txn, LINKS)? else {
            return Ok(InlinkMap::default());
        };
        let mut edges = HashMap::new();
        for target in targets {
            let key = PathKey::new(target).as_bytes();
            let sources = self.collect_stored_paths(&table, key)?;
            if !sources.is_empty() {
                edges.insert(target.to_path_buf(), sources.into_boxed_slice());
            }
        }
        Ok(InlinkMap::from_raw(edges))
    }

    // --- Path queries ------------------------------------------------

    /// Returns sorted paths carrying `tag` or a child of `tag`.
    ///
    /// # Errors
    ///
    /// - [`Store`] if opening the database or reading the table fails.
    ///
    /// [`Store`]: IndexError::Store
    pub(crate) fn paths_with_tag(
        &self,
        tag: &str,
    ) -> IndexResult<Box<[PathBuf]>> {
        if tag.starts_with('#') {
            with_lowercased(tag, |lower| {
                self.paths_from_multimap(PATHS_BY_TAG, lower.as_bytes())
            })
        } else {
            with_lowercased(tag, |lower| {
                let normalized = format!("#{lower}");
                self.paths_from_multimap(PATHS_BY_TAG, normalized.as_bytes())
            })
        }
    }

    /// Returns sorted paths carrying the file class `class`.
    ///
    /// # Errors
    ///
    /// - [`Store`] if opening the database or reading the table fails.
    ///
    /// [`Store`]: IndexError::Store
    pub(crate) fn paths_with_file_class(
        &self,
        class: &str,
    ) -> IndexResult<Box<[PathBuf]>> {
        with_lowercased(class, |lower| {
            self.paths_from_multimap(PATHS_BY_FILE_CLASS, lower.as_bytes())
        })
    }

    /// Returns sorted paths within `folder`.
    ///
    /// # Errors
    ///
    /// - [`Store`] if opening the database or reading the table fails.
    ///
    /// [`Store`]: IndexError::Store
    pub(crate) fn paths_in_folder(
        &self,
        folder: &Path,
    ) -> IndexResult<Box<[PathBuf]>> {
        let txn = self.begin_read()?;
        let Some(table) = self.open_table_for_read(&txn, FILES)? else {
            return Ok(Box::default());
        };
        Ok(self.collect_folder_paths(&table, folder)?)
    }

    // --- Transaction lifecycle ----------------------------------------

    /// Begins a read transaction.
    ///
    /// # Errors
    ///
    /// - [`StoreError::Redb`] if the transaction cannot be started.
    fn begin_read(&self) -> StoreResult<ReadTransaction> {
        self.db.begin_read().map_err(|source| self.wrap_redb_error(source))
    }

    /// Begins a write transaction.
    ///
    /// # Errors
    ///
    /// - [`StoreError::Redb`] if the transaction cannot be started.
    #[inline(never)]
    fn begin_write(&self) -> StoreResult<WriteTransaction> {
        self.db.begin_write().map_err(|source| self.wrap_redb_error(source))
    }

    #[inline(never)]
    fn commit(&self, txn: WriteTransaction) -> StoreResult<()> {
        txn.commit().map_err(|source| self.wrap_redb_error(source))
    }

    #[inline(never)]
    fn configure_cache_txn(
        &self,
        mut txn: WriteTransaction,
    ) -> StoreResult<WriteTransaction> {
        txn.set_durability(redb::Durability::None)
            .map_err(|source| self.wrap_redb_error(source))?;
        Ok(txn)
    }

    /// Replaces one note row with invalid bytes for fault-injection tests.
    #[cfg(test)]
    pub(super) fn poison_note_row(&self, path: &Path) -> IndexResult<()> {
        self.poison_note_row_in(self.begin_write()?, path)
    }

    #[cfg(test)]
    #[inline(never)]
    fn poison_note_row_in(
        &self,
        txn: WriteTransaction,
        path: &Path,
    ) -> IndexResult<()> {
        {
            let mut notes = txn
                .open_table(NOTES)
                .map_err(|source| self.wrap_redb_error(source))?;
            notes
                .insert(PathKey::new(path).as_bytes(), &b"\xff\xff\xff"[..])
                .map_err(|source| self.wrap_redb_error(source))?;
        }
        self.commit(txn)?;
        Ok(())
    }

    /// Opens a row table for reads; a missing table is an empty cache.
    fn open_table_for_read(
        &self,
        txn: &ReadTransaction,
        def: TableDefinition<'static, &'static [u8], &'static [u8]>,
    ) -> StoreResult<Option<BytesReadTable>> {
        match txn.open_table(def) {
            Ok(table) => Ok(Some(table)),
            Err(redb::TableError::TableDoesNotExist(_)) => Ok(None),
            Err(source) => Err(self.wrap_redb_error(source)),
        }
    }

    /// Opens a multimap for reads; a missing table is an empty cache.
    fn open_multimap_for_read(
        &self,
        txn: &ReadTransaction,
        def: MultimapTableDefinition<'static, &'static [u8], &'static [u8]>,
    ) -> StoreResult<Option<BytesReadMultimapTable>> {
        match txn.open_multimap_table(def) {
            Ok(table) => Ok(Some(table)),
            Err(redb::TableError::TableDoesNotExist(_)) => Ok(None),
            Err(source) => Err(self.wrap_redb_error(source)),
        }
    }

    /// Maps each path to its encoded key bytes for link-row resolution.
    fn path_by_key<'a>(
        paths: impl IntoIterator<Item = &'a Path>,
        capacity: usize,
    ) -> FxHashMap<&'a [u8], &'a Path> {
        let mut map =
            FxHashMap::with_capacity_and_hasher(capacity, FxBuildHasher);
        for path in paths {
            map.insert(PathKey::new(path).as_bytes(), path);
        }
        map
    }

    // --- Full-table reads ---------------------------------------------

    /// Deserializes every value in `table`, sorted by path.
    ///
    /// # Errors
    ///
    /// - [`Store`] if the table cannot be read or stored bytes are not a valid
    ///   row.
    ///
    /// [`Store`]: IndexError::Store
    fn read_table<T>(
        &self,
        txn: &ReadTransaction,
        def: TableDefinition<'static, &'static [u8], &'static [u8]>,
    ) -> IndexResult<SortedByPath<T>>
    where
        T: DeserializeOwned + crate::path::HasPath,
    {
        let Some(table) = self.open_table_for_read(txn, def)? else {
            return Ok(SortedByPath::assumed_sorted(Vec::new()));
        };
        let items = self.decode_table_rows(&table)?;
        Ok(SortedByPath::sorted(items))
    }

    /// Loads every stored [`FileBase`] and [`Note`] (sorted by path) and every
    /// derived inlink edge. Stale or orphaned edges are dropped.
    ///
    /// Targets resolve through every stored [`FileBase`] because attachments
    /// can carry inlinks; sources resolve through stored [`Note`] rows only.
    ///
    /// # Errors
    ///
    /// - [`Store`] if a table cannot be read or stored bytes are not a valid
    ///   row.
    ///
    /// [`Store`]: IndexError::Store
    pub(super) fn read_all(&self) -> IndexResult<StoreSnapshot> {
        let txn = self.begin_read()?;
        let (files_result, notes_result) = rayon::join(
            || self.read_table(&txn, FILES),
            || self.collect_notes(&txn),
        );
        let files: SortedByPath<FileBase> = files_result?;
        let notes = notes_result?;
        let target_paths = Self::path_by_key(
            files.as_slice().iter().map(FileBase::path),
            files.as_slice().len(),
        );
        let source_paths = Self::path_by_key(
            notes.as_slice().iter().map(Note::path),
            notes.as_slice().len(),
        );
        let links =
            self.read_links(&txn, LINKS, &target_paths, &source_paths)?;
        Ok((files, notes, links))
    }

    /// Reads every persisted [`Note`], decoded in parallel.
    ///
    /// Bulk-iterates `NOTES` once instead of point-reading paths; used when a
    /// caller needs nearly every persisted note.
    ///
    /// # Errors
    ///
    /// - [`Store`] if opening the transaction or table fails.
    ///
    /// [`Store`]: IndexError::Store
    pub(super) fn read_all_notes(&self) -> IndexResult<SortedByPath<Note>> {
        let txn = self.begin_read()?;
        Ok(self.collect_notes(&txn)?)
    }

    /// Reads every persisted [`FileBase`], sorted by path.
    ///
    /// # Errors
    ///
    /// - [`Store`] if opening the transaction or reading `FILES` fails.
    ///
    /// [`Store`]: IndexError::Store
    pub(super) fn read_all_files(&self) -> IndexResult<SortedByPath<FileBase>> {
        let txn = self.begin_read()?;
        self.read_table(&txn, FILES)
    }

    /// Reads all persisted inlink edges, resolving path bytes against `files`.
    ///
    /// # Errors
    ///
    /// - [`Store`] if opening the transaction or reading `LINKS` fails.
    ///
    /// [`Store`]: IndexError::Store
    pub(super) fn read_all_links(
        &self,
        files: &SortedByPath<FileBase>,
    ) -> IndexResult<InlinkMap> {
        let txn = self.begin_read()?;
        let files_slice = files.as_slice();
        let target_paths = Self::path_by_key(
            files_slice.iter().map(FileBase::path),
            files_slice.len(),
        );
        let source_paths = Self::path_by_key(
            files_slice
                .iter()
                .filter(|file| file.format() == FileFormat::Note)
                .map(FileBase::path),
            files_slice.len(),
        );
        self.read_links(&txn, LINKS, &target_paths, &source_paths)
    }

    /// Deserializes every `target -> sources` edge from the `links` multimap
    /// table, resolving stored path bytes through `target_paths` and
    /// `source_paths`.
    ///
    /// A target missing from `target_paths` drops its whole edge set; a source
    /// missing from `source_paths` is skipped; an entry left with no surviving
    /// sources is omitted entirely.
    ///
    /// # Errors
    ///
    /// - [`Store`] if the table cannot be read.
    ///
    /// [`Store`]: IndexError::Store
    fn read_links(
        &self,
        txn: &ReadTransaction,
        def: MultimapTableDefinition<'static, &'static [u8], &'static [u8]>,
        target_paths: &FxHashMap<&[u8], &Path>,
        source_paths: &FxHashMap<&[u8], &Path>,
    ) -> IndexResult<InlinkMap> {
        let Some(table) = self.open_multimap_for_read(txn, def)? else {
            return Ok(InlinkMap::default());
        };
        Ok(self.collect_multimap_links(&table, target_paths, source_paths)?)
    }

    // --- Write primitives ---------------------------------------------

    /// Serializes `items` into `table`, keyed by `path_of`.
    ///
    /// # Errors
    ///
    /// - [`Store`] if the table cannot be opened or written, or an item cannot
    ///   be encoded.
    ///
    /// [`Store`]: IndexError::Store
    fn write_table<'a, T: Serialize + 'a>(
        &self,
        txn: &WriteTransaction,
        def: TableDefinition<&[u8], &[u8]>,
        items: impl IntoIterator<Item = &'a T>,
        path_of: impl Fn(&T) -> &Path,
    ) -> IndexResult<()> {
        let mut table = txn
            .open_table(def)
            .map_err(|source| self.wrap_redb_error(source))?;
        let mut buf = Vec::new();
        for item in items {
            let path = path_of(item);
            let key = PathKey::new(path);
            let value = encode_row(key.path(), item, &mut buf)?;
            table
                .insert(key.as_bytes(), value)
                .map_err(|source| self.wrap_redb_error(source))?;
        }
        Ok(())
    }

    /// Writes every [`FileEntry`]'s inbound-link edges into the `links`
    /// multimap table.
    ///
    /// # Errors
    ///
    /// - [`Store`] if the table cannot be opened or written.
    ///
    /// [`Store`]: IndexError::Store
    fn write_links(
        &self,
        txn: &WriteTransaction,
        def: MultimapTableDefinition<&[u8], &[u8]>,
        entries: &[FileEntry],
    ) -> IndexResult<()> {
        let mut table = txn
            .open_multimap_table(def)
            .map_err(|source| self.wrap_redb_error(source))?;
        for entry in entries {
            let inlinks = entry.inlinks();
            if inlinks.is_empty() {
                continue;
            }
            let target_key = PathKey::new(entry.file().path());
            for src in inlinks {
                let source_key = PathKey::new(src);
                table
                    .insert(target_key.as_bytes(), source_key.as_bytes())
                    .map_err(|source| self.wrap_redb_error(source))?;
            }
        }
        Ok(())
    }

    // --- Persistence entry points -------------------------------------

    /// Persists a rebuild or a non-empty incremental refresh.
    ///
    /// Incremental requests must contain at least one file, note, or edge
    /// change. `IndexerService::prepare_pass` filters empty passes before they
    /// reach this method.
    ///
    /// # Errors
    ///
    /// - [`Store`] if the transaction fails or a row cannot be encoded.
    ///
    /// [`Store`]: IndexError::Store
    pub(super) fn persist(
        &self,
        request: &PersistRequest<'_>,
    ) -> IndexResult<()> {
        match &request.rows {
            PersistRows::Rebuild {
                entries,
            } => self.apply_rebuild(entries, &request.axes),
            PersistRows::Incremental {
                delta,
                notes,
                edges,
            } => {
                if delta.is_empty() && notes.is_empty() && edges.is_empty() {
                    return Ok(());
                }
                debug_assert!(
                    !delta.is_empty() || !notes.is_empty() || !edges.is_empty(),
                    "prepare_pass gates empty passes"
                );
                self.apply_incremental(&request.axes, &IncrementalRows {
                    delta,
                    notes,
                    edges,
                })
            }
        }
    }

    /// Rebuild arm of [`Self::persist`]: clears and rewrites every table.
    fn apply_rebuild(
        &self,
        entries: &[FileEntry],
        axes: &IndexAxes,
    ) -> IndexResult<()> {
        self.apply_rebuild_in(self.begin_cache_txn()?, entries, axes)
    }

    #[inline(never)]
    fn apply_rebuild_in(
        &self,
        txn: WriteTransaction,
        entries: &[FileEntry],
        axes: &IndexAxes,
    ) -> IndexResult<()> {
        self.delete_tables(&txn)?;
        self.write_all_parallel(&txn, entries)?;
        self.write_axes_parallel(&txn, entries, axes)?;
        self.commit(txn)?;
        Ok(())
    }

    /// Incremental arm of [`Self::persist`]: applies row-level changes.
    fn apply_incremental(
        &self,
        axes: &IndexAxes,
        rows: &IncrementalRows<'_>,
    ) -> IndexResult<()> {
        self.apply_incremental_in(self.begin_cache_txn()?, axes, rows)
    }

    #[inline(never)]
    fn apply_incremental_in(
        &self,
        txn: WriteTransaction,
        axes: &IndexAxes,
        rows: &IncrementalRows<'_>,
    ) -> IndexResult<()> {
        self.apply_diff_deletions(&txn, rows.delta.deleted(), axes)?;
        self.apply_diff_upserts(&txn, rows.delta.upserted())?;
        self.apply_modified_notes(&txn, rows.notes, axes)?;
        self.apply_inlink_delta(&txn, rows.edges)?;
        self.commit(txn)?;
        Ok(())
    }

    // --- Error -------------------------------------------------------

    /// Wraps a redb error with this store's database path.
    #[cold]
    #[inline(never)]
    pub(super) fn wrap_redb_error(
        &self,
        source: impl Into<redb::Error>,
    ) -> StoreError {
        StoreError::Redb {
            path: self.path.clone(),
            source: Box::new(source.into()),
        }
    }

    // --- Schema ------------------------------------------------------

    /// Returns `true` if `error` indicates schema drift or corruption that only
    /// a wipe-and-recreate can fix.
    fn is_rebuild_trigger(error: &redb::TableError) -> bool {
        matches!(
            error,
            redb::TableError::TableTypeMismatch { .. }
                | redb::TableError::TableIsMultimap(_)
                | redb::TableError::TableIsNotMultimap(_)
                | redb::TableError::TypeDefinitionChanged { .. }
                | redb::TableError::Storage(redb::StorageError::Corrupted(_))
        )
    }

    // --- Construction helpers -----------------------------------------

    /// Opens (or creates) the database file.
    ///
    /// Recovers by wipe-and-recreate if `Database::create` itself reports
    /// container-level corruption.
    fn create_db(path: &Path) -> StoreResult<redb::Database> {
        let wrap = |source: redb::DatabaseError| StoreError::Redb {
            path: path.to_path_buf(),
            source: Box::new(source.into()),
        };
        match redb::Database::create(path) {
            Ok(db) => Ok(db),
            Err(redb::DatabaseError::Storage(
                redb::StorageError::Corrupted(_),
            )) => {
                fs::remove_file(path).map_err(|source| StoreError::Io {
                    path: path.to_path_buf(),
                    source,
                })?;
                redb::Database::create(path).map_err(wrap)
            }
            Err(source) => Err(wrap(source)),
        }
    }

    /// Returns `true` if any core table is missing or schema-mismatched,
    /// signaling that [`Self::open`] should wipe and recreate the database.
    fn rebuild_needed(db: &redb::Database, path: &Path) -> StoreResult<bool> {
        let read_txn = match db.begin_read() {
            Ok(txn) => txn,
            Err(redb::TransactionError::Storage(
                redb::StorageError::Corrupted(_),
            )) => {
                return Ok(true);
            }
            Err(source) => {
                return Err(StoreError::Redb {
                    path: path.to_path_buf(),
                    source: Box::new(source.into()),
                });
            }
        };
        for spec in &TABLES {
            if let Err(error) = spec.probe(&read_txn)
                && Self::is_rebuild_trigger(&error)
            {
                return Ok(true);
            }
        }
        Ok(false)
    }

    // --- Batch read helpers -------------------------------------------

    /// Point-reads and decodes `paths` from `kind`'s table, warning and
    /// skipping corrupted rows.
    fn read_batch<'a, T: DeserializeOwned>(
        &self,
        kind: ReadSource,
        paths: impl IntoIterator<Item = &'a Path>,
    ) -> IndexResult<Vec<T>> {
        let txn = self.begin_read()?;
        let Some(table) = self.open_table_for_read(&txn, kind.table())? else {
            return Ok(Vec::new());
        };
        let mut items = Vec::new();
        for path in paths {
            let key = PathKey::new(path).as_bytes();
            if let Some(guard) =
                table.get(key).map_err(|source| self.wrap_redb_error(source))?
            {
                match decode_row::<T>(path, guard.value()) {
                    Ok(item) => items.push(item),
                    Err(source) => {
                        tracing::warn!(
                            path = %path.display(),
                            table = kind.label(),
                            err = %source,
                            "skipping corrupted row"
                        );
                    }
                }
            }
        }
        Ok(items)
    }

    /// Collects path values stored under `key` and propagates lookup errors.
    #[inline(never)]
    fn collect_stored_paths(
        &self,
        table: &BytesReadMultimapTable,
        key: &[u8],
    ) -> IndexResult<Vec<PathBuf>> {
        let iter =
            table.get(key).map_err(|source| self.wrap_redb_error(source))?;
        iter.map(|entry| {
            let guard = entry.map_err(|source| self.wrap_redb_error(source))?;
            Ok(path_from_bytes(guard.value()))
        })
        .collect()
    }

    // --- Path query helpers -------------------------------------------

    /// Reads, sorts, and deduplicates every path stored under `key` in
    /// `def`.
    fn paths_from_multimap(
        &self,
        def: MultimapTableDefinition<'static, &'static [u8], &'static [u8]>,
        key: &[u8],
    ) -> IndexResult<Box<[PathBuf]>> {
        let txn = self.begin_read()?;
        let Some(table) = self.open_multimap_for_read(&txn, def)? else {
            return Ok(Box::default());
        };
        let mut paths = self.collect_stored_paths(&table, key)?;
        paths.sort();
        paths.dedup();
        Ok(paths.into_boxed_slice())
    }

    /// Dispatches to a full-table or prefix-range scan depending on whether
    /// `folder` is the project root.
    fn collect_folder_paths(
        &self,
        table: &redb::ReadOnlyTable<&[u8], &[u8]>,
        folder: &Path,
    ) -> StoreResult<Box<[PathBuf]>> {
        let folder_str = folder.to_string_lossy();
        if folder_str.is_empty() || folder_str == "." {
            self.collect_all_folder_paths(table)
        } else {
            let prefix = if folder_str.ends_with('/') {
                folder_str.into_owned()
            } else {
                format!("{folder_str}/")
            };
            self.collect_prefixed_folder_paths(table, &prefix)
        }
    }

    /// Collects and sorts every path in `table`.
    fn collect_all_folder_paths(
        &self,
        table: &redb::ReadOnlyTable<&[u8], &[u8]>,
    ) -> StoreResult<Box<[PathBuf]>> {
        self.collect_sorted_path_keys(self.open_table_iter(table)?)
    }

    #[inline(never)]
    fn open_prefix_iter<'a>(
        &self,
        table: &'a redb::ReadOnlyTable<&[u8], &[u8]>,
        prefix: &[u8],
        end_prefix: &[u8],
    ) -> StoreResult<TableRange<'a>> {
        table
            .range(prefix..end_prefix)
            .map_err(|source| self.wrap_redb_error(source))
    }

    /// Collects and sorts every path in `table` whose key starts with `prefix`,
    /// via a byte-range scan.
    fn collect_prefixed_folder_paths(
        &self,
        table: &redb::ReadOnlyTable<&[u8], &[u8]>,
        prefix: &str,
    ) -> StoreResult<Box<[PathBuf]>> {
        let prefix_bytes = prefix.as_bytes();
        let mut end_prefix = prefix_bytes.to_vec();
        if let Some(last) = end_prefix.last_mut() {
            *last = last.saturating_add(1);
        }
        self.collect_sorted_path_keys(self.open_prefix_iter(
            table,
            prefix_bytes,
            end_prefix.as_slice(),
        )?)
    }

    #[inline(never)]
    fn collect_sorted_path_keys(
        &self,
        iter: TableRange<'_>,
    ) -> StoreResult<Box<[PathBuf]>> {
        let mut paths = Vec::new();
        for entry in iter {
            let (key, _) =
                entry.map_err(|source| self.wrap_redb_error(source))?;
            paths.push(path_from_bytes(key.value()));
        }
        paths.sort();
        Ok(paths.into_boxed_slice())
    }

    // --- Read helpers ------------------------------------------------

    /// Opens a full-range iterator over `table`.
    #[inline(never)]
    fn open_table_iter<'a>(
        &self,
        table: &'a redb::ReadOnlyTable<&[u8], &[u8]>,
    ) -> StoreResult<TableRange<'a>> {
        table.iter().map_err(|source| self.wrap_redb_error(source))
    }

    /// Deserializes every row in an already-open `table`.
    fn decode_table_rows<T: DeserializeOwned>(
        &self,
        table: &redb::ReadOnlyTable<&[u8], &[u8]>,
    ) -> StoreResult<Vec<T>> {
        let capacity = usize::try_from(
            table.len().map_err(|source| self.wrap_redb_error(source))?,
        )
        .unwrap_or(usize::MAX);
        self.decode_rows(self.open_table_iter(table)?, capacity)
    }

    #[inline(never)]
    fn decode_rows<T: DeserializeOwned>(
        &self,
        iter: TableRange<'_>,
        capacity: usize,
    ) -> StoreResult<Vec<T>> {
        let mut items = Vec::with_capacity(capacity);
        for entry in iter {
            let (key, value) =
                entry.map_err(|source| self.wrap_redb_error(source))?;
            let path = path_from_bytes(key.value());
            items.push(decode_row(&path, value.value())?);
        }
        Ok(items)
    }

    /// Reads raw `NOTES` rows before parallel decoding.
    fn collect_notes(
        &self,
        txn: &ReadTransaction,
    ) -> StoreResult<SortedByPath<Note>> {
        let Some(table) = self.open_table_for_read(txn, NOTES)? else {
            return Ok(SortedByPath::assumed_sorted(Vec::new()));
        };
        Self::decode_note_bytes(
            self.collect_raw_note_bytes(self.open_table_iter(&table)?)?,
        )
    }

    #[inline(never)]
    fn collect_raw_note_bytes(
        &self,
        iter: TableRange<'_>,
    ) -> StoreResult<RawNoteRows> {
        let mut rows = Vec::new();
        for entry in iter {
            let (key, value) =
                entry.map_err(|source| self.wrap_redb_error(source))?;
            rows.push((path_from_bytes(key.value()), value.value().to_vec()));
        }
        Ok(rows)
    }

    /// Decodes every raw `(path, bytes)` pair in parallel and returns the
    /// results sorted by path.
    fn decode_note_bytes(
        raw_entries: RawNoteRows,
    ) -> StoreResult<SortedByPath<Note>> {
        let notes: Vec<Note> = raw_entries
            .into_par_iter()
            .map(|(path, bytes)| decode_row(&path, &bytes))
            .collect::<Result<Vec<Note>, StoreError>>()?;
        Ok(SortedByPath::sorted(notes))
    }

    /// Collects an already-open `LINKS` table into an [`InlinkMap`].
    fn collect_multimap_links(
        &self,
        table: &redb::ReadOnlyMultimapTable<&[u8], &[u8]>,
        target_paths: &FxHashMap<&[u8], &Path>,
        source_paths: &FxHashMap<&[u8], &Path>,
    ) -> StoreResult<InlinkMap> {
        self.collect_resolved_links(
            table.iter().map_err(|source| self.wrap_redb_error(source))?,
            target_paths,
            source_paths,
        )
    }

    #[inline(never)]
    fn collect_resolved_links<'a>(
        &self,
        iter: impl Iterator<Item = LinkEntry<'a>>,
        target_paths: &FxHashMap<&[u8], &Path>,
        source_paths: &FxHashMap<&[u8], &Path>,
    ) -> StoreResult<InlinkMap> {
        let mut links = HashMap::new();
        for entry in iter {
            if let Some((target, sources)) =
                self.resolve_link_entry(entry, target_paths, source_paths)?
            {
                links.insert(target, sources);
            }
        }
        Ok(InlinkMap::from_raw(links))
    }

    /// Extracts one `target -> sources` row from a `LINKS` multimap iterator
    /// entry, resolving raw bytes through the path maps. Returns `None` when
    /// the target resolves to no path or when every source dropped.
    #[inline(never)]
    fn resolve_link_entry(
        &self,
        entry: LinkEntry<'_>,
        target_paths: &FxHashMap<&[u8], &Path>,
        source_paths: &FxHashMap<&[u8], &Path>,
    ) -> StoreResult<ResolvedLink> {
        let (target, sources) =
            entry.map_err(|source| self.wrap_redb_error(source))?;
        let Some(target) =
            target_paths.get(target.value()).map(|path| path.to_path_buf())
        else {
            return Ok(None);
        };
        let sources = self.collect_source_paths(sources, source_paths)?;
        if sources.is_empty() {
            return Ok(None);
        }
        Ok(Some((target, sources)))
    }

    /// Collects source paths from a `MultimapValue` iterator, skipping
    /// unresolvable entries.
    #[inline(never)]
    fn collect_source_paths(
        &self,
        sources: redb::MultimapValue<'_, &[u8]>,
        source_paths: &FxHashMap<&[u8], &Path>,
    ) -> StoreResult<Box<[PathBuf]>> {
        let mut values = Vec::new();
        for src in sources {
            let guard = src.map_err(|source| self.wrap_redb_error(source))?;
            if let Some(path) =
                source_paths.get(guard.value()).map(|path| path.to_path_buf())
            {
                values.push(path);
            }
        }
        Ok(values.into_boxed_slice())
    }

    // --- Full-rebuild write helpers -----------------------------------

    /// Full-rebuild persistence with durability disabled.
    ///
    /// Uses [`redb::Durability::None`]: `index.redb` is a derived cache, not
    /// source data. A crash after commit can lose only this write;
    /// [`IndexerService::refresh`] or a future [`Self::open`] rebuilds by
    /// rescanning disk, and skipping redb's per-commit fsync removes the fixed
    /// cost that dominates full-index persistence.
    ///
    /// [`IndexerService::refresh`]: super::service::IndexerService::refresh
    ///
    /// # Errors
    ///
    /// - [`StoreError::Redb`] if the transaction cannot be started or the
    ///   durability hint cannot be set.
    #[inline(never)]
    fn begin_cache_txn(&self) -> StoreResult<WriteTransaction> {
        self.configure_cache_txn(self.begin_write()?)
    }

    /// Deletes every table's contents ahead of a full rebuild write.
    /// Best-effort tables tolerate only a missing table (the fresh-database
    /// case); every other storage error propagates.
    fn delete_tables(&self, txn: &WriteTransaction) -> IndexResult<()> {
        for spec in &TABLES {
            spec.delete(self, txn)?;
        }
        Ok(())
    }

    /// Runs every [`WriteTarget`] concurrently against the same write
    /// transaction.
    fn write_all_parallel(
        &self,
        txn: &WriteTransaction,
        entries: &[FileEntry],
    ) -> IndexResult<()> {
        WriteTarget::ALL
            .into_par_iter()
            .try_for_each(|target| target.run(self, txn, entries))
    }

    /// Writes every configured secondary index axis for a full rebuild.
    fn write_axes_parallel(
        &self,
        txn: &WriteTransaction,
        entries: &[FileEntry],
        axes: &IndexAxes,
    ) -> IndexResult<()> {
        axes.iter().try_for_each(|axis| {
            let (forward, reverse) = rayon::join(
                || self.write_index_axis_forward(txn, axis, entries),
                || self.write_index_axis_reverse(txn, axis, entries),
            );
            forward?;
            reverse
        })
    }

    /// Writes `index`'s forward (`value -> [paths]`) table for a full rebuild.
    ///
    /// Split from [`Self::write_index_axis_reverse`] so [`WriteTarget::ALL`]
    /// can write distinct redb tables concurrently.
    fn write_index_axis_forward(
        &self,
        txn: &WriteTransaction,
        index: &IndexDimension,
        entries: &[FileEntry],
    ) -> IndexResult<()> {
        let mut forward = self.open_multimap_for_write(txn, index.forward())?;
        for note in entries.iter().filter_map(FileEntry::note) {
            let path_bytes = PathKey::new(note.path()).as_bytes();
            index.visit_values(note, |value| {
                forward
                    .insert(value.as_bytes(), path_bytes)
                    .map_err(|source| self.wrap_redb_error(source))?;
                Ok(())
            })?;
        }
        Ok(())
    }

    /// Writes `index`'s reverse (`path -> [values]`) table for a full rebuild.
    fn write_index_axis_reverse(
        &self,
        txn: &WriteTransaction,
        index: &IndexDimension,
        entries: &[FileEntry],
    ) -> IndexResult<()> {
        let mut reverse = self.open_multimap_for_write(txn, index.reverse())?;
        for note in entries.iter().filter_map(FileEntry::note) {
            let path_bytes = PathKey::new(note.path()).as_bytes();
            index.visit_values(note, |value| {
                reverse
                    .insert(path_bytes, value.as_bytes())
                    .map_err(|source| self.wrap_redb_error(source))?;
                Ok(())
            })?;
        }
        Ok(())
    }

    /// Opens or creates a multimap table for write access.
    fn open_multimap_for_write<'txn>(
        &self,
        txn: &'txn WriteTransaction,
        def: MultimapTableDefinition<'static, &'static [u8], &'static [u8]>,
    ) -> IndexResult<BytesMultimapTable<'txn>> {
        Ok(txn
            .open_multimap_table(def)
            .map_err(|source| self.wrap_redb_error(source))?)
    }

    /// Reads `key`'s current values out of `table` into owned byte vectors.
    /// Collecting first (rather than removing while iterating) is required:
    /// redb's iterator borrows `table`, so mutating it while a read guard is
    /// still live is a borrow conflict.
    fn collect_multimap_values(
        &self,
        table: &BytesMultimapTable<'_>,
        key: &[u8],
    ) -> IndexResult<Vec<Vec<u8>>> {
        let mut values = Vec::new();
        let iter =
            table.get(key).map_err(|source| self.wrap_redb_error(source))?;
        for entry in iter {
            let guard = entry.map_err(|source| self.wrap_redb_error(source))?;
            values.push(guard.value().to_vec());
        }
        Ok(values)
    }

    // --- Incremental write helpers ------------------------------------

    /// Removes every deleted file's rows from `FILES`, `NOTES`, and the
    /// tag/class indexes.
    fn apply_diff_deletions(
        &self,
        txn: &WriteTransaction,
        deleted: &[FileBase],
        axes: &IndexAxes,
    ) -> IndexResult<()> {
        if deleted.is_empty() {
            return Ok(());
        }
        self.delete_files_and_notes(txn, deleted)?;
        self.delete_index_entries_for_paths(txn, deleted, axes)?;
        Ok(())
    }

    fn delete_files_and_notes(
        &self,
        txn: &WriteTransaction,
        deleted: &[FileBase],
    ) -> IndexResult<()> {
        let mut files_table = txn
            .open_table(FILES)
            .map_err(|source| self.wrap_redb_error(source))?;
        let mut notes_table = txn
            .open_table(NOTES)
            .map_err(|source| self.wrap_redb_error(source))?;
        for file in deleted {
            let key = PathKey::new(file.path()).as_bytes();
            files_table
                .remove(key)
                .map_err(|source| self.wrap_redb_error(source))?;
            notes_table
                .remove(key)
                .map_err(|source| self.wrap_redb_error(source))?;
        }
        Ok(())
    }

    fn delete_index_entries_for_paths(
        &self,
        txn: &WriteTransaction,
        deleted: &[FileBase],
        axes: &IndexAxes,
    ) -> IndexResult<()> {
        for index in axes.iter() {
            let mut forward =
                self.open_multimap_for_write(txn, index.forward())?;
            let mut reverse =
                self.open_multimap_for_write(txn, index.reverse())?;
            for file in deleted {
                let path_bytes = PathKey::new(file.path()).as_bytes();
                self.remove_axis_entry(&mut forward, &mut reverse, path_bytes)?;
            }
        }
        Ok(())
    }

    /// Removes `path_bytes`' current forward-table values via the reverse
    /// (path-keyed) index, then clears its reverse entry - O(that path's value
    /// count), never a full-table scan. Shared by
    /// [`Self::delete_index_entries_for_paths`] (path fully removed) and
    /// [`Self::upsert_index_axis`] (values about to be replaced).
    #[inline(never)]
    fn remove_axis_entry(
        &self,
        forward: &mut BytesMultimapTable<'_>,
        reverse: &mut BytesMultimapTable<'_>,
        path_bytes: &[u8],
    ) -> IndexResult<()> {
        for old in self.collect_multimap_values(reverse, path_bytes)? {
            forward
                .remove(old.as_slice(), path_bytes)
                .map_err(|source| self.wrap_redb_error(source))?;
        }
        reverse
            .remove_all(path_bytes)
            .map_err(|source| self.wrap_redb_error(source))?;
        Ok(())
    }

    fn apply_diff_upserts(
        &self,
        txn: &WriteTransaction,
        upserted: &[FileBase],
    ) -> IndexResult<()> {
        let mut files_table = txn
            .open_table(FILES)
            .map_err(|source| self.wrap_redb_error(source))?;
        let mut buf = Vec::new();
        for file in upserted {
            self.upsert_row(&mut files_table, file.path(), file, &mut buf)?;
        }
        Ok(())
    }

    /// Writes upserted notes' rows, list items, and tag/class index entries.
    fn apply_modified_notes(
        &self,
        txn: &WriteTransaction,
        modified_notes: &[&Note],
        axes: &IndexAxes,
    ) -> IndexResult<()> {
        if modified_notes.is_empty() {
            return Ok(());
        }
        self.upsert_notes(txn, modified_notes.iter().copied())?;
        self.upsert_tags_and_classes(txn, modified_notes, axes)?;
        Ok(())
    }

    fn upsert_notes<'a>(
        &self,
        txn: &WriteTransaction,
        modified_notes: impl Iterator<Item = &'a Note>,
    ) -> IndexResult<()> {
        let mut notes_table = txn
            .open_table(NOTES)
            .map_err(|source| self.wrap_redb_error(source))?;
        let mut buf = Vec::new();
        for note in modified_notes {
            self.upsert_row(&mut notes_table, note.path(), note, &mut buf)?;
        }
        Ok(())
    }

    fn upsert_tags_and_classes(
        &self,
        txn: &WriteTransaction,
        modified_notes: &[&Note],
        axes: &IndexAxes,
    ) -> IndexResult<()> {
        for index in axes.iter() {
            self.upsert_index_axis(txn, index, modified_notes.iter().copied())?;
        }
        Ok(())
    }

    /// Upserts each of `modified_notes`' current values into `index`'s forward
    /// table, first removing exactly this note's previous values via the
    /// reverse (path-keyed) table in O(k) time where k is this note's previous
    /// value count, rather than performing a full-table scan.
    fn upsert_index_axis<'a>(
        &self,
        txn: &WriteTransaction,
        index: &IndexDimension,
        modified_notes: impl Iterator<Item = &'a Note>,
    ) -> IndexResult<()> {
        let mut forward = self.open_multimap_for_write(txn, index.forward())?;
        let mut reverse = self.open_multimap_for_write(txn, index.reverse())?;
        for note in modified_notes {
            let path_bytes = PathKey::new(note.path()).as_bytes();
            self.remove_axis_entry(&mut forward, &mut reverse, path_bytes)?;
            index.visit_values(note, |value| {
                forward
                    .insert(value.as_bytes(), path_bytes)
                    .map_err(|source| self.wrap_redb_error(source))?;
                reverse
                    .insert(path_bytes, value.as_bytes())
                    .map_err(|source| self.wrap_redb_error(source))?;
                Ok(())
            })?;
        }
        Ok(())
    }

    fn apply_inlink_delta(
        &self,
        txn: &WriteTransaction,
        inlink_delta: &InlinkDelta,
    ) -> IndexResult<()> {
        if inlink_delta.is_empty() {
            return Ok(());
        }
        let mut links_table = txn
            .open_multimap_table(LINKS)
            .map_err(|source| self.wrap_redb_error(source))?;
        for (target, src) in inlink_delta.deleted() {
            let target_key = PathKey::new(target);
            let source_key = PathKey::new(src);
            links_table
                .remove(target_key.as_bytes(), source_key.as_bytes())
                .map_err(|source| self.wrap_redb_error(source))?;
        }
        for (target, src) in inlink_delta.upserted() {
            let target_key = PathKey::new(target);
            let source_key = PathKey::new(src);
            links_table
                .insert(target_key.as_bytes(), source_key.as_bytes())
                .map_err(|source| self.wrap_redb_error(source))?;
        }
        Ok(())
    }

    fn upsert_row<T: Serialize>(
        &self,
        table: &mut redb::Table<'_, &[u8], &[u8]>,
        path: &Path,
        value: &T,
        buf: &mut Vec<u8>,
    ) -> IndexResult<()> {
        let key = PathKey::new(path);
        let bytes = encode_row(key.path(), value, buf)?;
        table
            .insert(key.as_bytes(), bytes)
            .map_err(|source| self.wrap_redb_error(source))?;
        Ok(())
    }
}

/// Batch point-read source: maps a variant to its redb table
/// definition and structured label for [`read_batch`](IndexStore::read_batch).
#[derive(Copy, Clone, Debug)]
enum ReadSource {
    Files,
    Notes,
}

impl ReadSource {
    fn table(self) -> TableDefinition<'static, &'static [u8], &'static [u8]> {
        match self {
            Self::Files => FILES,
            Self::Notes => NOTES,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Files => "file",
            Self::Notes => "note",
        }
    }
}

/// Full-rebuild write job for one table family.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum WriteTarget {
    Files,
    Notes,
    Links,
}

impl WriteTarget {
    const ALL: [Self; 3] = [Self::Files, Self::Notes, Self::Links];

    fn run(
        self,
        store: &IndexStore,
        txn: &WriteTransaction,
        entries: &[FileEntry],
    ) -> IndexResult<()> {
        match self {
            Self::Files => store.write_table(
                txn,
                FILES,
                entries.iter().map(FileEntry::file),
                FileBase::path,
            ),
            Self::Notes => store.write_table(
                txn,
                NOTES,
                entries.iter().filter_map(FileEntry::note),
                Note::path,
            ),
            Self::Links => store.write_links(txn, LINKS, entries),
        }
    }
}

/// Store-owned dimensions written for tag and file-class indexes.
pub(super) struct IndexAxes {
    dimensions: [IndexDimension; 2],
}

impl IndexAxes {
    /// Builds the canonical index axes for one configured File Class key.
    #[must_use]
    pub(super) fn for_class_field(class_field: &str) -> Self {
        Self {
            dimensions: [IndexDimension::Tag, IndexDimension::FileClass {
                key: class_field.to_owned(),
            }],
        }
    }

    fn iter(&self) -> impl Iterator<Item = &IndexDimension> {
        self.dimensions.iter()
    }
}

/// Path-derived index dimension, pairing forward and reverse tables so
/// upsert/delete/rebuild logic is shared for tags and file classes.
#[derive(Clone, Debug, Eq, PartialEq)]
enum IndexDimension {
    Tag,
    FileClass {
        key: String,
    },
}

impl IndexDimension {
    /// Forward lookup table (`value -> [paths]`) for tag/class queries.
    const fn forward(
        &self,
    ) -> MultimapTableDefinition<'static, &'static [u8], &'static [u8]> {
        match self {
            Self::Tag => PATHS_BY_TAG,
            Self::FileClass {
                ..
            } => PATHS_BY_FILE_CLASS,
        }
    }

    /// Reverse lookup table (`path -> [values]`) for targeted updates.
    const fn reverse(
        &self,
    ) -> MultimapTableDefinition<'static, &'static [u8], &'static [u8]> {
        match self {
            Self::Tag => TAGS_BY_PATH,
            Self::FileClass {
                ..
            } => FILE_CLASSES_BY_PATH,
        }
    }

    /// Visits `note`'s current normalized values for this index: lowercased tag
    /// segments, or lowercased values of the configured File Class key.
    ///
    /// Uses `with_lowercased` to avoid heap allocations when values are already
    /// ASCII lowercase.
    fn visit_values(
        &self,
        note: &Note,
        mut visit: impl FnMut(&str) -> Result<(), IndexError>,
    ) -> Result<(), IndexError> {
        match self {
            Self::Tag => {
                let segments = note.tags().iter().flat_map(Tag::segments);
                for segment in segments {
                    with_lowercased(segment, &mut visit)?;
                }
            }
            Self::FileClass {
                key,
            } => {
                let values = note
                    .frontmatter()
                    .into_iter()
                    .flat_map(|fm| fm.get_values(key))
                    .filter_map(crate::NoteFieldValue::as_str);
                for value in values {
                    with_lowercased(value, &mut visit)?;
                }
            }
        }
        Ok(())
    }
}

/// Invokes `visit` with a lowercased view of `text`, avoiding an owned
/// allocation when `text` is already lowercase ASCII.
#[inline]
fn with_lowercased<R>(text: &str, visit: impl FnOnce(&str) -> R) -> R {
    if text.is_ascii() && !text.bytes().any(|b| b.is_ascii_uppercase()) {
        visit(text)
    } else {
        visit(&text.to_lowercase())
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
    use crate::index::tests::fixtures::PermissionsGuard;
    use crate::{IndexerService, WorkspaceIndex, parse_note as parse};
    mod multimap_paths {

        use super::*;

        #[test]
        fn repairs_a_wrong_typed_table_at_open() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let root = temp.path();
            let store = IndexStore::open(root).expect("open store");
            let txn = store.begin_write().expect("begin write txn");
            let wrong_table: TableDefinition<&[u8], &[u8]> =
                TableDefinition::new("paths_by_tag");
            txn.open_table(wrong_table).expect("open wrong table");
            txn.commit().expect("commit wrong table");
            drop(store);

            let reopened = IndexStore::open(root).expect("reopen heals drift");
            let (files, notes, _) =
                reopened.read_all().expect("read_all after recovery");
            assert_eq!(files.as_slice(), []);
            assert_eq!(notes.as_slice(), []);
            let paths =
                reopened.paths_with_tag("x").expect("paths_with_tag works");
            assert_eq!(paths.as_ref(), <&[PathBuf]>::default());
        }
    }

    mod path_queries {
        use pretty_assertions::assert_eq;
        use rstest::rstest;

        use super::*;

        fn store_with_a_tagged_note(root: &Path) -> IndexStore {
            let store = IndexStore::open(root).expect("open store");
            let files = vec![FileBase::note_for_test(Path::new("tagged.md"))];
            let notes = vec![parse("tagged.md", "# T\n\nTagged #x body.")];
            write_all_parts(&store, &files, &notes, &InlinkMap::default())
                .expect("persist tagged note");
            store
        }

        #[rstest]
        #[case::bare("x")]
        #[case::prefixed("#x")]
        fn resolves_bare_and_prefixed_tags_to_the_same_paths(
            #[case] tag: &str,
        ) {
            let temp = tempfile::tempdir().expect("create temp dir");
            let store = store_with_a_tagged_note(temp.path());

            let paths = store.paths_with_tag(tag).expect("read tag paths");

            assert_eq!(paths.as_ref(), [Path::new("tagged.md")]);
        }
    }

    const TEST_TABLE: TableDefinition<&[u8], &[u8]> =
        TableDefinition::new("test_table");

    /// Writes raw bytes into `def` to simulate corrupted rows.
    fn write_raw_value(
        store: &IndexStore,
        def: TableDefinition<&[u8], &[u8]>,
        key: &str,
        value: &[u8],
    ) {
        let txn = store.db.begin_write().expect("begin write txn");
        {
            let mut table = txn.open_table(def).expect("open table");
            table.insert(key.as_bytes(), value).expect("insert raw bytes");
        }
        txn.commit().expect("commit raw insert");
    }

    /// Builds a sorted inlink-map fixture.
    fn make_inlinks(entries: &[(PathBuf, &[PathBuf])]) -> InlinkMap {
        let mut map = HashMap::new();
        for (target, sources) in entries {
            let mut sorted = sources.to_vec();
            sorted.sort();
            map.insert(target.clone(), sorted.into_boxed_slice());
        }
        InlinkMap::from_raw(map)
    }

    fn write_all_parts(
        store: &IndexStore,
        files: &[FileBase],
        notes: &[Note],
        links: &InlinkMap,
    ) -> IndexResult<()> {
        let index = WorkspaceIndex::assemble(
            SortedByPath::sorted(files.to_vec()),
            SortedByPath::sorted(notes.to_vec()),
            links.clone(),
        );
        store.persist(&PersistRequest::rebuild(
            IndexAxes::for_class_field("class"),
            index.entries(),
        ))
    }

    /// Writes an orphanable raw `LINKS` row that `write_all_parts` cannot
    /// assemble.
    fn write_raw_link(store: &IndexStore, target: &Path, source: &Path) {
        let txn = store.db.begin_write().expect("begin write txn");
        {
            let mut table =
                txn.open_multimap_table(LINKS).expect("open links table");
            table
                .insert(
                    PathKey::new(target).as_bytes(),
                    PathKey::new(source).as_bytes(),
                )
                .expect("insert raw link");
        }
        txn.commit().expect("commit raw link");
    }

    /// Builds note `FileBase` fixtures in caller-provided order.
    fn note_files(paths: &[&str]) -> Vec<FileBase> {
        paths.iter().map(|&p| FileBase::note_for_test(p)).collect()
    }

    #[test]
    fn persists_files_as_postcard_bytes_not_toml_text() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let db = IndexStore::open(temp.path()).expect("open db");
        let txn = db.begin_write().expect("begin write");
        let rows = [FileBase::note_for_test("hello.md")];
        db.write_table(&txn, TEST_TABLE, &rows, FileBase::path)
            .expect("write table");
        txn.commit().expect("commit");

        let read_txn = db.begin_read().expect("begin read");
        let loaded: SortedByPath<FileBase> =
            db.read_table(&read_txn, TEST_TABLE).expect("read table");
        assert_eq!(loaded.as_slice(), rows);
    }

    #[test]
    fn returns_deserialize_error_when_stored_bytes_are_invalid() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let db = IndexStore::open(temp.path()).expect("open db");
        let txn = db.begin_write().expect("begin write");
        {
            let mut table = txn.open_table(TEST_TABLE).expect("open table");
            table
                .insert(b"corrupt.md".as_slice(), [0xFF, 0xFF].as_slice())
                .expect("insert corrupt");
        }
        txn.commit().expect("commit");

        let read_txn = db.begin_read().expect("begin read");
        let result: IndexResult<SortedByPath<FileBase>> =
            db.read_table(&read_txn, TEST_TABLE);

        assert!(matches!(
            result,
            Err(IndexError::Store(StoreError::Deserialize { .. }))
        ));
    }

    mod persistence {
        use pretty_assertions::assert_eq;

        use super::*;
        use crate::{SourceLine, TaskStatusType};
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

            assert_eq!(files.as_slice().len(), 0);
            assert_eq!(notes.as_slice().len(), 0);
            assert_eq!(links.len(), 0);
        }

        #[test]
        fn write_all_parts_then_read_all_round_trips_files_and_notes() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(
                temp.path().join("note.md"),
                "---\ntitle: Hello\n---\nPriority:: 5\n- [ ] task",
            )
            .expect("write note");
            let files = IndexerService::scan(temp.path()).expect("scan root");
            let note = parse(
                "note.md",
                "---\ntitle: Hello\n---\nPriority:: 5\n- [ ] task",
            );
            let notes = vec![note];
            let store = IndexStore::open(temp.path()).expect("open store");

            write_all_parts(&store, &files, &notes, &InlinkMap::default())
                .expect("persist files and notes");
            let (loaded_files, loaded_notes, _) =
                store.read_all().expect("load files and notes");

            assert_eq!(loaded_files.as_slice(), files);
            assert_eq!(loaded_notes.as_slice(), notes);
        }

        #[test]
        fn write_all_parts_persists_notes_with_lists() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let store = IndexStore::open(temp.path()).expect("open store");
            let note = parse(
                "note.md",
                "- [ ] Todo task 📅 2025-01-15\n- Plain bullet\n  - [x] Child \
                 task",
            );
            let files = note_files(&["note.md"]);
            write_all_parts(
                &store,
                &files,
                std::slice::from_ref(&note),
                &InlinkMap::default(),
            )
            .expect("persist");

            let (_, loaded_notes, _) = store.read_all().expect("read all");
            assert_eq!(loaded_notes.as_slice().len(), 1);
            let loaded_note = loaded_notes.as_slice().first().expect("note");
            let items: Vec<_> = loaded_note.list_items().collect();
            assert_eq!(items.len(), 3);
            let rec0 = items.first().expect("first item");
            assert_eq!(rec0.clean_text(), "Todo task");
            assert!(rec0.kind().is_task());
            assert_eq!(rec0.line(), SourceLine::new(1).expect("non-zero"));
            assert_eq!(rec0.depth(), 0);

            let rec1 = items.get(1).expect("second item");
            assert_eq!(rec1.clean_text(), "Plain bullet");
            assert!(!rec1.kind().is_task());
            assert_eq!(rec1.line(), SourceLine::new(2).expect("non-zero"));
            assert_eq!(rec1.depth(), 0);

            let rec2 = items.get(2).expect("third item");
            assert_eq!(rec2.clean_text(), "Child task");
            assert!(rec2.kind().is_task());
            assert_eq!(rec2.line(), SourceLine::new(3).expect("non-zero"));
            assert_eq!(rec2.depth(), 1);
            assert_eq!(
                rec2.parent(),
                Some(SourceLine::new(2).expect("non-zero"))
            );
        }

        #[test]
        fn incremental_persistence_updates_and_deletes_notes_with_lists() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let service = IndexerService::for_tests(temp.path());

            fs::write(
                temp.path().join("a.md"),
                "- [ ] A task 1\n- [ ] A task 2",
            )
            .expect("write a.md");
            fs::write(temp.path().join("b.md"), "- [ ] B task 1")
                .expect("write b.md");

            let index = service.build().expect("build index");
            service.persist(&index).expect("persist initial index");

            let initial_index = service.load().expect("load initial");
            let b_note = initial_index
                .entries()
                .iter()
                .find_map(|e| {
                    if e.file().path() == Path::new("b.md") {
                        e.note()
                    } else {
                        None
                    }
                })
                .expect("b note");
            assert_eq!(b_note.list_items().count(), 1);

            fs::remove_file(temp.path().join("a.md")).expect("remove a.md");
            fs::write(
                temp.path().join("b.md"),
                "- [x] B updated 1\n- [ ] B updated 2",
            )
            .expect("modify b.md");

            let refreshed = service.refresh().expect("refresh index");
            service.persist(&refreshed).expect("persist incremental");

            let updated_index = service.load().expect("load refreshed");
            let a_entry = updated_index
                .entries()
                .iter()
                .find(|e| e.file().path() == Path::new("a.md"));
            assert!(a_entry.is_none());

            let updated_b_note = updated_index
                .entries()
                .iter()
                .find_map(|e| {
                    if e.file().path() == Path::new("b.md") {
                        e.note()
                    } else {
                        None
                    }
                })
                .expect("updated b note");
            let updated_items: Vec<_> = updated_b_note.list_items().collect();
            assert_eq!(updated_items.len(), 2);
            let first_item = updated_items.first().expect("first item present");
            assert_eq!(first_item.clean_text(), "B updated 1");
            assert_eq!(
                first_item.kind().as_task().map(|t| t.status().kind()),
                Some(TaskStatusType::Done)
            );
            let second_item =
                updated_items.get(1).expect("second item present");
            assert_eq!(second_item.clean_text(), "B updated 2");
            assert_eq!(
                second_item.kind().as_task().map(|t| t.status().kind()),
                Some(TaskStatusType::Todo)
            );
        }
        #[test]
        fn write_all_parts_then_read_all_round_trips_links() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let store = IndexStore::open(temp.path()).expect("open store");
            let links = make_inlinks(&[
                (PathBuf::from("target.md"), &[
                    PathBuf::from("a.md"),
                    PathBuf::from("b.md"),
                ]),
                (PathBuf::from("other.md"), &[PathBuf::from("a.md")]),
            ]);

            let notes: Vec<_> = ["a.md", "b.md", "other.md", "target.md"]
                .iter()
                .map(|p| parse(*p, ""))
                .collect();
            let files: Vec<_> = ["a.md", "b.md", "other.md", "target.md"]
                .iter()
                .map(|&p| FileBase::note_for_test(p))
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
            // Orphan edges must be written raw; entry assembly cannot express
            // unindexed targets or sources.
            write_all_parts(
                &store,
                &note_files(&["a.md", "target.md"]),
                &notes,
                &make_inlinks(&[(PathBuf::from("target.md"), &[
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
                make_inlinks(&[(PathBuf::from("target.md"), &[
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
            // The orphan source must be written raw.
            write_all_parts(
                &store,
                &note_files(&["a.md", "b.md"]),
                &notes,
                &make_inlinks(&[(PathBuf::from("b.md"), &[PathBuf::from(
                    "a.md",
                )])]),
            )
            .expect("persist valid links");
            write_raw_link(&store, Path::new("a.md"), Path::new("ghost.md"));
            let (_, _, loaded_links) = store.read_all().expect("load links");

            assert_eq!(
                loaded_links,
                make_inlinks(&[(PathBuf::from("b.md"), &[PathBuf::from(
                    "a.md"
                )])])
            );
        }

        #[test]
        fn read_all_links_drops_orphaned_edges_like_read_all() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let store = IndexStore::open(temp.path()).expect("open store");
            let notes: Vec<_> =
                ["a.md", "target.md"].iter().map(|p| parse(*p, "")).collect();
            let files = note_files(&["a.md", "target.md"]);
            write_all_parts(
                &store,
                &files,
                &notes,
                &make_inlinks(&[(PathBuf::from("target.md"), &[
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

            // Proves `read_all_links` reconstructs from disk without
            // `read_all`'s note decode, under the same drop-orphaned-edges
            // policy.
            let stored_files = store.read_all_files().expect("read files");
            let reconstructed =
                store.read_all_links(&stored_files).expect("reconstruct load");
            assert_eq!(
                reconstructed,
                make_inlinks(&[(PathBuf::from("target.md"), &[
                    PathBuf::from("a.md"),
                ])])
            );
        }

        #[test]
        fn write_all_parts_drops_links_absent_from_the_new_set() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let store = IndexStore::open(temp.path()).expect("open store");
            write_raw_link(&store, Path::new("target.md"), Path::new("a.md"));

            write_all_parts(&store, &[], &[], &InlinkMap::default())
                .expect("persist empty links");
            let (_, _, loaded_links) = store.read_all().expect("load links");

            assert_eq!(loaded_links.len(), 0);
        }

        #[test]
        fn write_all_parts_drops_files_absent_from_the_new_set() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("stale.md"), "old")
                .expect("write stale");
            let stale = IndexerService::scan(temp.path()).expect("scan stale");
            let store = IndexStore::open(temp.path()).expect("open store");
            write_all_parts(&store, &stale, &[], &InlinkMap::default())
                .expect("persist stale");
            fs::remove_file(temp.path().join("stale.md"))
                .expect("remove stale");
            fs::write(temp.path().join("fresh.md"), "new")
                .expect("write fresh");
            let fresh = IndexerService::scan(temp.path()).expect("scan fresh");

            write_all_parts(&store, &fresh, &[], &InlinkMap::default())
                .expect("persist fresh");
            let (loaded_files, _loaded_notes, _) =
                store.read_all().expect("load files");

            assert_eq!(loaded_files.as_slice(), fresh);
        }

        #[test]
        fn write_all_parts_with_no_files_persists_an_empty_table() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let store = IndexStore::open(temp.path()).expect("open store");

            write_all_parts(&store, &[], &[], &InlinkMap::default())
                .expect("persist an empty row set");
            let (loaded_files, loaded_notes, _) =
                store.read_all().expect("load files and notes");

            assert_eq!(loaded_files.as_slice().len(), 0);
            assert_eq!(loaded_notes.as_slice().len(), 0);
        }

        #[test]
        fn round_trips_a_file_with_a_unicode_path() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("café ☕.md"), "content")
                .expect("write unicode-named file");
            let files = IndexerService::scan(temp.path()).expect("scan root");
            let store = IndexStore::open(temp.path()).expect("open store");

            write_all_parts(&store, &files, &[], &InlinkMap::default())
                .expect("persist files");
            let (loaded_files, ..) = store.read_all().expect("load files");

            assert_eq!(loaded_files.as_slice(), files);
        }

        #[test]
        fn round_trips_a_file_with_a_non_unicode_path() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let store = IndexStore::open(temp.path()).expect("open store");

            let weird_path = non_unicode_path();

            let file = FileBase::note_for_test(weird_path.clone());

            let note = parse(&weird_path, "content");
            let files = vec![file];
            let notes = vec![note];

            write_all_parts(&store, &files, &notes, &InlinkMap::default())
                .expect("persist files and notes");
            let (loaded_files, loaded_notes, _) =
                store.read_all().expect("load files and notes");

            assert_eq!(loaded_files.as_slice(), files);
            assert_eq!(loaded_notes.as_slice(), notes);
        }

        #[test]
        fn load_returns_byte_exact_inlinks_for_a_non_unicode_note() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let store = IndexStore::open(temp.path()).expect("open store");
            let weird = non_unicode_path();
            let normal = PathBuf::from("normal.md");
            let mut files = vec![
                FileBase::note_for_test(weird.clone()),
                FileBase::note_for_test(normal.clone()),
            ];
            files.sort_by(|a, b| a.path().cmp(b.path()));
            let mut notes = vec![
                parse(&weird, "link to [[normal]]"),
                parse(&normal, "link to [[weird]]"),
            ];
            notes.sort_by(|a, b| a.path().cmp(b.path()));
            let links = make_inlinks(&[
                (weird.clone(), std::slice::from_ref(&normal)),
                (normal.clone(), std::slice::from_ref(&weird)),
            ]);
            write_all_parts(&store, &files, &notes, &links).expect("persist");
            drop(store);

            let loaded = IndexerService::for_tests(temp.path())
                .load()
                .expect("load index");
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

        // macOS filename normalization makes the raw non-Unicode fixture
        // unreliable; run only where it materializes byte-exactly.
        #[cfg(target_os = "linux")]
        #[test]
        fn scan_preserves_a_non_unicode_path() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let weird = non_unicode_path();
            fs::write(temp.path().join(&weird), "# weird")
                .expect("write weird note");

            let files = IndexerService::scan(temp.path()).expect("scan root");

            assert!(files.iter().any(|file| file.path() == weird));
        }

        #[test]
        fn load_resolves_attachment_targets_and_note_sources() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let store = IndexStore::open(temp.path()).expect("open store");
            let attachment = PathBuf::from("attachment.png");
            let note_path = PathBuf::from("note.md");
            let files = vec![
                FileBase::for_test(attachment.clone(), FileFormat::Other),
                FileBase::note_for_test(note_path.clone()),
            ];
            let notes = vec![parse(&note_path, "# Note")];
            let links = make_inlinks(&[(
                attachment.clone(),
                std::slice::from_ref(&note_path),
            )]);
            write_all_parts(&store, &files, &notes, &links).expect("persist");

            let (_, _, loaded_links) = store.read_all().expect("load index");

            assert_eq!(loaded_links.inlinks_of(&attachment), [
                note_path.as_path()
            ]);
        }

        #[test]
        fn load_resolves_a_non_unicode_attachment_target_byte_exactly() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let store = IndexStore::open(temp.path()).expect("open store");
            let weird = non_unicode_path();
            let note_path = PathBuf::from("note.md");
            let files = vec![
                FileBase::for_test(weird.clone(), FileFormat::Other),
                FileBase::note_for_test(note_path.clone()),
            ];
            let notes = vec![parse(&note_path, "# Note")];
            let links = make_inlinks(&[(
                weird.clone(),
                std::slice::from_ref(&note_path),
            )]);
            write_all_parts(&store, &files, &notes, &links).expect("persist");

            let (loaded_files, _, loaded_links) =
                store.read_all().expect("load files and links");

            assert_eq!(loaded_links.inlinks_of(&weird), [note_path.as_path()]);
            assert!(
                loaded_files.as_slice().iter().any(|file| file.path() == weird),
                "attachment row must round-trip byte-exactly"
            );
        }

        #[test]
        fn returns_files_in_path_sort_order() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::create_dir_all(temp.path().join("a-b")).expect("mkdir a-b");
            fs::create_dir_all(temp.path().join("a")).expect("mkdir a");
            fs::write(temp.path().join("a-b/c.md"), "1")
                .expect("write a-b/c.md");
            fs::write(temp.path().join("a/z.md"), "2").expect("write a/z.md");
            let files = IndexerService::scan(temp.path()).expect("scan root");
            let store = IndexStore::open(temp.path()).expect("open store");
            write_all_parts(&store, &files, &[], &InlinkMap::default())
                .expect("persist files");

            let (loaded_files, ..) = store.read_all().expect("load files");

            assert_eq!(loaded_files.as_slice(), files);
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

            assert!(matches!(
                error,
                IndexError::Store(StoreError::Redb { .. })
            ));
        }

        #[cfg(unix)]
        #[test]
        fn returns_io_error_when_parent_dir_unwritable() {
            use std::os::unix::fs::PermissionsExt as _;

            let temp = tempfile::tempdir().expect("create temp dir");
            let root = temp.path();
            fs::set_permissions(root, fs::Permissions::from_mode(0o500))
                .expect("revoke write permission");
            let _guard = PermissionsGuard(root);

            let error = IndexStore::open(root)
                .expect_err("unwritable root fails to open store");

            assert!(matches!(error, IndexError::Store(StoreError::Io { .. })));
        }

        #[test]
        fn recovers_by_rebuilding_when_the_files_table_has_the_old_str_key_schema()
         {
            use pretty_assertions::assert_eq;
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
                let txn = db.begin_write().expect("begin write");
                {
                    let mut table =
                        txn.open_table(OLD_FILES).expect("open old table");
                    table
                        .insert("old.md", [1u8, 2, 3].as_slice())
                        .expect("insert old row");
                }
                txn.commit().expect("commit old schema");
            }

            let store = IndexStore::open(root)
                .expect("open recovers from schema mismatch");
            let (files, notes, links) =
                store.read_all().expect("load after recovery");

            assert_eq!(files.as_slice(), []);
            assert_eq!(notes.as_slice(), []);
            assert!(links.is_empty());
        }
    }

    mod is_rebuild_trigger {
        use rstest::rstest;

        use super::*;

        #[rstest]
        #[case::table_type_mismatch(redb::TableError::TableTypeMismatch {
            table: "files".to_owned(),
            key: redb::TypeName::new("&str"),
            value: redb::TypeName::new("&[u8]"),
        })]
        #[case::type_definition_changed(redb::TableError::TypeDefinitionChanged {
            name: redb::TypeName::new("&[u8]"),
            alignment: 1,
            width: None,
        })]
        // `open_table` can surface table-local corruption separately from the
        // container-level corruption `create_db` handles. A real file fixture
        // would need redb internals to corrupt one table while preserving
        // container checksums, so this tests the predicate directly.
        #[case::storage_corrupted(redb::TableError::Storage(
            redb::StorageError::Corrupted("simulated".to_owned()),
        ))]
        fn triggers_a_rebuild(#[case] error: redb::TableError) {
            assert!(IndexStore::is_rebuild_trigger(&error));
        }

        #[rstest]
        #[case::table_does_not_exist(redb::TableError::TableDoesNotExist(
            "files".to_owned(),
        ))]
        #[case::non_corrupted_storage(redb::TableError::Storage(
            redb::StorageError::DatabaseClosed,
        ))]
        fn skips_rebuild_for_absent_tables_and_closed_databases(
            #[case] error: redb::TableError,
        ) {
            assert!(!IndexStore::is_rebuild_trigger(&error));
        }
    }

    mod read_all {
        use rstest::rstest;

        use super::*;

        #[rstest]
        #[case::files(FILES)]
        #[case::notes(NOTES)]
        fn returns_deserialize_error_when_stored_bytes_are_invalid(
            #[case] def: TableDefinition<&[u8], &[u8]>,
        ) {
            let temp = tempfile::tempdir().expect("create temp dir");
            let store = IndexStore::open(temp.path()).expect("open store");
            write_raw_value(&store, def, "bad.md", &[0xFF, 0xFE]);

            let error =
                store.read_all().expect_err("invalid bytes fail to load");

            assert!(matches!(
                &error,
                IndexError::Store(StoreError::Deserialize { path, .. })
                    if path == Path::new("bad.md")
            ));
        }

        #[test]
        fn persists_files_as_postcard_bytes_not_toml_text() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("note.md"), "content")
                .expect("write note");
            let files = IndexerService::scan(temp.path()).expect("scan root");
            let store = IndexStore::open(temp.path()).expect("open store");
            write_all_parts(&store, &files, &[], &InlinkMap::default())
                .expect("persist files");

            let read_txn = store.db.begin_read().expect("begin read txn");
            let table = read_txn.open_table(FILES).expect("open table");
            let raw = table
                .get(b"note.md".as_slice())
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
            let indexer = crate::IndexerService::for_tests(temp.path());
            indexer
                .persist(&indexer.build().expect("build index"))
                .expect("persist index");

            let store = IndexStore::open(temp.path()).expect("reopen store");
            write_raw_value(&store, NOTES, "note.md", &[0xFF, 0xFE]);
            // Release the db handle before indexer.refresh() opens its own.
            drop(store);

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
