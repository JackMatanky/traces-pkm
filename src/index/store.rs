//! Redb persistence for [`FileBase`], [`Note`], [`ListEntry`], and derived
//! inlink records.
//!
//! [`IndexStore`] owns one redb connection and adapts it to the file-index
//! schema (`FILES`, `NOTES`, `LINKS`, `LISTS` tables). Callers use
//! [`super::IndexerService`] methods instead of interacting with tables
//! directly.

use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
};

use rayon::prelude::*;
use redb::{
    MultimapTableDefinition, ReadTransaction, ReadableDatabase as _,
    ReadableMultimapTable as _, ReadableTable as _, TableDefinition,
    WriteTransaction,
};
use serde::{Serialize, de::DeserializeOwned};

use super::{
    FileIndex, INDEX_FILE,
    codec::{decode_row, encode_row, path_from_bytes},
    delta::{FileDiff, InlinkDelta},
    entry::{FileEntry, ListEntry, ListEntryRef},
    error::{DbError, DbResult, IndexError, IndexResult},
    inlinks::InlinkMap,
};
use crate::{FileBase, Note, SourceLine};

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
pub(super) const NOTES: TableDefinition<&[u8], &[u8]> =
    TableDefinition::new("notes");

/// Inbound link multimap table.
///
/// Maps each note to every other note whose outlinks resolve to it. See
/// [`super::inlinks`].
///
/// Key: target note path as UTF-8 bytes
/// Value: one source note path per entry
const LINKS: MultimapTableDefinition<&[u8], &[u8]> =
    MultimapTableDefinition::new("links");

/// Parsed list items table.
///
/// Stores [`ListEntry`]s for every list item in every Markdown file.
///
/// Key: project-relative path as UTF-8 bytes concatenated with 4-byte
/// big-endian [`SourceLine`]
/// Value: serialized [`ListEntry`]
const LISTS: TableDefinition<&[u8], &[u8]> = TableDefinition::new("lists");

/// Path lookup by tag multimap table.
const PATHS_BY_TAG: MultimapTableDefinition<&[u8], &[u8]> =
    MultimapTableDefinition::new("paths_by_tag");

/// Path lookup by file class multimap table.
const PATHS_BY_FILE_CLASS: MultimapTableDefinition<&[u8], &[u8]> =
    MultimapTableDefinition::new("paths_by_file_class");

/// Persisted snapshot of [`FileBase`]s, [`Note`]s (sorted by path), and inbound
/// link edges (target-keyed, unordered).
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

/// A pair of raw key and value bytes from a multimap table.
type MultimapPair = (Vec<u8>, Vec<u8>);

/// A boxed iterator over a table range.
type BoxedRange<'a> = Box<redb::Range<'a, &'static [u8], &'static [u8]>>;

/// Redb-backed handle to one project root's index database.
///
/// Created by [`Self::open`]. Callers interact through [`IndexerService`]
/// methods, not directly.
///
/// [`IndexerService`]: super::IndexerService
#[derive(Debug)]
pub(crate) struct IndexStore {
    db: redb::Database,
    path: PathBuf,
}

impl IndexStore {
    /// Constructs a `LISTS` table key by concatenating `path`'s UTF-8 bytes
    /// and `line` as a 4-byte big-endian integer.
    #[inline]
    #[must_use]
    fn list_key(path: &str, line: SourceLine) -> Vec<u8> {
        let mut key = Vec::with_capacity(path.len().saturating_add(4));
        key.extend_from_slice(path.as_bytes());
        key.extend_from_slice(&u32::from(line).to_be_bytes());
        key
    }

    /// Returns the inclusive `(start, end)` `LISTS` key bounds spanning every
    /// possible line for `path`.
    #[inline]
    #[must_use]
    fn list_key_bounds(path: &str) -> (Vec<u8>, Vec<u8>) {
        (
            Self::list_key(path, SourceLine::new(0)),
            Self::list_key(path, SourceLine::new(u32::MAX)),
        )
    }

    /// True if `key_bytes` is a `LISTS` key belonging to `path`: the range
    /// bounds from [`Self::list_key_bounds`] can also match a longer
    /// sibling path, so callers must still check this per key.
    #[inline]
    #[must_use]
    fn list_key_matches_path(key_bytes: &[u8], path: &str) -> bool {
        let path_bytes = path.as_bytes();
        key_bytes.len() == path_bytes.len().saturating_add(4)
            && key_bytes.starts_with(path_bytes)
    }

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
    pub(crate) fn open(root: &Path) -> IndexResult<Self> {
        let path = root.join(INDEX_FILE);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|source| DbError::Io {
                path: parent.to_path_buf(),
                source,
            })?;
        }
        let db = Self::create_db(&path)?;
        let db = if Self::check_rebuild_needed(&db, &path)? {
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

    fn check_rebuild_needed(
        db: &redb::Database,
        path: &Path,
    ) -> DbResult<bool> {
        let read_txn = match db.begin_read() {
            Ok(txn) => txn,
            Err(redb::TransactionError::Storage(
                redb::StorageError::Corrupted(_),
            )) => {
                return Ok(true);
            }
            Err(source) => {
                return Err(DbError::Redb {
                    path: path.to_path_buf(),
                    source: Box::new(source.into()),
                });
            }
        };
        for probe in [
            read_txn.open_table(FILES).err(),
            read_txn.open_table(NOTES).err(),
            read_txn.open_multimap_table(LINKS).err(),
            read_txn.open_table(LISTS).err(),
        ] {
            let Some(error) = probe else {
                continue;
            };
            if Self::is_rebuild_trigger(&error) {
                return Ok(true);
            }
        }
        Ok(false)
    }

    pub(super) fn is_rebuild_trigger(error: &redb::TableError) -> bool {
        matches!(
            error,
            redb::TableError::TableTypeMismatch { .. }
                | redb::TableError::TypeDefinitionChanged { .. }
                | redb::TableError::Storage(redb::StorageError::Corrupted(_))
        )
    }

    /// Checks database integrity.
    ///
    /// # Errors
    ///
    /// Returns [`IndexError`] if database integrity check fails.
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "part of IndexStore surface")
    )]
    pub(super) fn check_health(&mut self) -> IndexResult<()> {
        self.db.check_integrity().map_err(|source| {
            IndexError::from(self.raise_source_error(source))
        })?;
        Ok(())
    }

    /// Point-reads entries within a single read transaction, decodes each, and
    /// drops the transaction immediately. Fails-open with `tracing::warn!` on
    /// corrupted rows.
    ///
    /// # Errors
    ///
    /// Returns [`IndexError`] if opening the transaction or table fails.
    pub(crate) fn read_notes_batch<'a>(
        &self,
        paths: impl IntoIterator<Item = &'a Path>,
    ) -> IndexResult<Vec<Note>> {
        let txn = self.begin_read()?;
        let table = match txn.open_table(NOTES) {
            Ok(t) => t,
            Err(redb::TableError::TableDoesNotExist(_)) => {
                return Ok(Vec::new());
            }
            Err(source) => return Err(self.raise_source_error(source).into()),
        };
        Ok(Self::fetch_notes_batch(&table, paths))
    }

    fn fetch_notes_batch<'a>(
        table: &redb::ReadOnlyTable<&[u8], &[u8]>,
        paths: impl IntoIterator<Item = &'a Path>,
    ) -> Vec<Note> {
        let mut notes = Vec::new();
        for path in paths {
            let key = path.as_os_str().as_encoded_bytes();
            if let Ok(Some(guard)) = table.get(key) {
                match decode_row::<Note>(path, guard.value()) {
                    Ok(note) => notes.push(note),
                    Err(err) => {
                        tracing::warn!(
                            path = %path.display(),
                            %err,
                            "skipping corrupted note row"
                        );
                    }
                }
            }
        }
        notes
    }

    /// Loads persisted file metadata from the `FILES` table.
    ///
    /// # Errors
    ///
    /// Returns [`IndexError`] if reading fails.
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "part of IndexStore surface")
    )]
    pub(super) fn load_file_metadata(&self) -> IndexResult<Vec<FileBase>> {
        let txn = self.begin_read()?;
        Ok(self.read_table(&txn, FILES, FileBase::path)?)
    }

    /// Returns sorted paths carrying `tag` or a child of `tag`.
    ///
    /// # Errors
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "part of storage query acceleration surface"
        )
    )]
    pub(crate) fn paths_with_tag(
        &self,
        tag: &str,
    ) -> IndexResult<Box<[PathBuf]>> {
        let normalized = if tag.starts_with('#') {
            tag.to_lowercase()
        } else {
            format!("#{tag}").to_lowercase()
        };
        self.paths_from_multimap(PATHS_BY_TAG, normalized.as_bytes())
    }

    /// Returns sorted paths carrying the file class `class`.
    ///
    /// # Errors
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "part of storage query acceleration surface"
        )
    )]
    pub(crate) fn paths_with_file_class(
        &self,
        class: &str,
    ) -> IndexResult<Box<[PathBuf]>> {
        self.paths_from_multimap(
            PATHS_BY_FILE_CLASS,
            class.to_lowercase().as_bytes(),
        )
    }

    fn paths_from_multimap(
        &self,
        table_def: MultimapTableDefinition<&[u8], &[u8]>,
        key: &[u8],
    ) -> IndexResult<Box<[PathBuf]>> {
        let txn = self.begin_read()?;
        let table = match txn.open_multimap_table(table_def) {
            Ok(t) => t,
            Err(redb::TableError::TableDoesNotExist(_)) => {
                return Ok(Box::new([]));
            }
            Err(e) => return Err(self.raise_source_error(e).into()),
        };
        let mut paths = Self::collect_multimap_key_paths(&table, key)?;
        paths.sort();
        paths.dedup();
        Ok(paths.into_boxed_slice())
    }

    fn collect_multimap_key_paths(
        table: &redb::ReadOnlyMultimapTable<&[u8], &[u8]>,
        key: &[u8],
    ) -> IndexResult<Vec<PathBuf>> {
        let mut paths = Vec::new();
        if let Ok(iter) = table.get(key) {
            for entry in iter {
                let guard = entry.map_err(|e| DbError::Redb {
                    path: PathBuf::new(),
                    source: Box::new(e.into()),
                })?;
                paths.push(path_from_bytes(guard.value()));
            }
        }
        Ok(paths)
    }

    /// Returns sorted paths within `folder`.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "part of storage query acceleration surface"
        )
    )]
    pub(crate) fn paths_in_folder(
        &self,
        folder: &Path,
    ) -> IndexResult<Box<[PathBuf]>> {
        let txn = self.begin_read()?;
        let table = match txn.open_table(FILES) {
            Ok(t) => t,
            Err(redb::TableError::TableDoesNotExist(_)) => {
                return Ok(Box::new([]));
            }
            Err(e) => return Err(self.raise_source_error(e).into()),
        };
        self.collect_folder_paths(&table, folder)
    }

    fn collect_folder_paths(
        &self,
        table: &redb::ReadOnlyTable<&[u8], &[u8]>,
        folder: &Path,
    ) -> IndexResult<Box<[PathBuf]>> {
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

    fn collect_all_folder_paths(
        &self,
        table: &redb::ReadOnlyTable<&[u8], &[u8]>,
    ) -> IndexResult<Box<[PathBuf]>> {
        let iter = self.open_table_iter(table)?;
        let mut paths = Vec::new();
        for entry in iter {
            let (k, _) = entry.map_err(|e| self.raise_source_error(e))?;
            paths.push(path_from_bytes(k.value()));
        }
        paths.sort();
        Ok(paths.into_boxed_slice())
    }

    fn collect_prefixed_folder_paths(
        &self,
        table: &redb::ReadOnlyTable<&[u8], &[u8]>,
        prefix: &str,
    ) -> IndexResult<Box<[PathBuf]>> {
        let prefix_bytes = prefix.as_bytes();
        let mut end_prefix = prefix_bytes.to_vec();
        if let Some(last) = end_prefix.last_mut() {
            *last = last.saturating_add(1);
        }
        let range = Box::new(
            table
                .range(prefix_bytes..end_prefix.as_slice())
                .map_err(|e| self.raise_source_error(e))?,
        );
        let mut paths = Vec::new();
        for entry in range {
            let (k, _) = entry.map_err(|e| self.raise_source_error(e))?;
            paths.push(path_from_bytes(k.value()));
        }
        paths.sort();
        Ok(paths.into_boxed_slice())
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

    fn open_table_iter<'a>(
        &self,
        table: &'a redb::ReadOnlyTable<&[u8], &[u8]>,
    ) -> DbResult<BoxedRange<'a>> {
        let iter =
            table.iter().map_err(|source| self.raise_source_error(source))?;
        Ok(Box::new(iter))
    }

    fn open_list_range<'a>(
        &self,
        table: &'a redb::ReadOnlyTable<&[u8], &[u8]>,
        path: &str,
    ) -> DbResult<BoxedRange<'a>> {
        let (start, end) = Self::list_key_bounds(path);
        let range = table
            .range(start.as_slice()..=end.as_slice())
            .map_err(|source| self.raise_source_error(source))?;
        Ok(Box::new(range))
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
        let mut items = self.read_table_raw(txn, table)?;
        items.sort_by(|a, b| path_of(a).cmp(path_of(b)));
        Ok(items)
    }

    fn read_table_raw<T: DeserializeOwned>(
        &self,
        txn: &ReadTransaction,
        table: TableDefinition<&[u8], &[u8]>,
    ) -> DbResult<Vec<T>> {
        match txn.open_table(table) {
            Ok(table) => self.decode_table_rows(&table),
            Err(redb::TableError::TableDoesNotExist(_)) => Ok(Vec::new()),
            Err(source) => Err(self.raise_source_error(source)),
        }
    }

    fn decode_table_rows<T: DeserializeOwned>(
        &self,
        table: &redb::ReadOnlyTable<&[u8], &[u8]>,
    ) -> DbResult<Vec<T>> {
        let mut items = Vec::new();
        let iter = Box::new(
            table.iter().map_err(|source| self.raise_source_error(source))?,
        );
        for entry in iter {
            let (key, value) =
                entry.map_err(|source| self.raise_source_error(source))?;
            let path = path_from_bytes(key.value());
            items.push(decode_row(&path, value.value())?);
        }
        Ok(items)
    }

    /// Reads all [`ListEntry`]s currently stored in the `LISTS` table.
    ///
    /// # Errors
    ///
    /// - [`DbError::Redb`] if the table cannot be read.
    /// - [`DbError::Deserialize`] if stored bytes are corrupt.
    pub(super) fn read_lists(
        &self,
        txn: &ReadTransaction,
    ) -> DbResult<Vec<ListEntry>> {
        match txn.open_table(LISTS) {
            Ok(table) => self.decode_list_rows(&table),
            Err(redb::TableError::TableDoesNotExist(_)) => Ok(Vec::new()),
            Err(source) => Err(self.raise_source_error(source)),
        }
    }

    fn decode_list_rows(
        &self,
        table: &redb::ReadOnlyTable<&[u8], &[u8]>,
    ) -> DbResult<Vec<ListEntry>> {
        let mut items = Vec::new();
        let iter = self.open_table_iter(table)?;
        for entry in iter {
            let (key, value) =
                entry.map_err(|source| self.raise_source_error(source))?;
            items.push(Self::decode_list_row(key.value(), value.value())?);
        }
        Ok(items)
    }

    fn decode_list_row(key: &[u8], value: &[u8]) -> DbResult<ListEntry> {
        let path_bytes = key
            .len()
            .checked_sub(4)
            .and_then(|prefix_len| key.get(..prefix_len))
            .unwrap_or(key);
        let path = path_from_bytes(path_bytes);
        decode_row(&path, value)
    }

    /// Reads all [`ListEntry`]s currently stored in the `LISTS` table for
    /// `path`.
    ///
    /// # Errors
    ///
    /// - [`DbError::Redb`] if the table cannot be read.
    /// - [`DbError::Deserialize`] if stored bytes are corrupt.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "consumed by task queries added in issue 08"
        )
    )]
    pub(super) fn read_lists_for_path(
        &self,
        txn: &ReadTransaction,
        path: &str,
    ) -> DbResult<Vec<ListEntry>> {
        let table = match txn.open_table(LISTS) {
            Ok(table) => table,
            Err(redb::TableError::TableDoesNotExist(_)) => {
                return Ok(Vec::new());
            }
            Err(source) => return Err(self.raise_source_error(source)),
        };
        self.collect_lists_for_path(&table, path)
    }

    fn collect_lists_for_path(
        &self,
        table: &redb::ReadOnlyTable<&[u8], &[u8]>,
        path: &str,
    ) -> DbResult<Vec<ListEntry>> {
        let range = self.open_list_range(table, path)?;
        let mut items = Vec::new();
        for entry in range {
            let (key, value) =
                entry.map_err(|source| self.raise_source_error(source))?;
            if Self::list_key_matches_path(key.value(), path) {
                let path_obj = Path::new(path);
                items.push(decode_row(path_obj, value.value())?);
            }
        }
        Ok(items)
    }

    /// Reads all persisted [`ListEntry`]s from `LISTS`.
    #[cfg_attr(
        not(any(test, feature = "test-utils")),
        expect(
            dead_code,
            reason = "consumed by task queries added in issue 08"
        )
    )]
    pub(super) fn read_all_lists(&self) -> DbResult<Vec<ListEntry>> {
        let txn = self.begin_read()?;
        self.read_lists(&txn)
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
        let notes = self.read_notes_parallel(&txn)?;
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

    fn read_notes_parallel(
        &self,
        txn: &ReadTransaction,
    ) -> DbResult<Vec<Note>> {
        let table = match txn.open_table(NOTES) {
            Ok(table) => table,
            Err(redb::TableError::TableDoesNotExist(_)) => {
                return Ok(Vec::new());
            }
            Err(source) => return Err(self.raise_source_error(source)),
        };
        let mut raw_entries = Vec::new();
        let iter = self.open_table_iter(&table)?;
        for entry in iter {
            let (key, value) =
                entry.map_err(|source| self.raise_source_error(source))?;
            let path = path_from_bytes(key.value());
            raw_entries.push((path, value.value().to_vec()));
        }
        Self::decode_raw_notes_parallel(raw_entries)
    }

    fn decode_raw_notes_parallel(
        raw_entries: Vec<(PathBuf, Vec<u8>)>,
    ) -> DbResult<Vec<Note>> {
        let results: Vec<DbResult<Note>> = raw_entries
            .into_par_iter()
            .map(|(path, bytes)| decode_row(&path, &bytes))
            .collect();
        let mut notes = Vec::with_capacity(results.len());
        for res in results {
            notes.push(res?);
        }
        notes.sort_by(|a, b| a.path().cmp(b.path()));
        Ok(notes)
    }

    /// Loads every persisted [`FileBase`] (sorted by path) and inlink edge.
    ///
    /// # Errors
    ///
    /// Returns [`IndexError`] if reading fails.
    pub(crate) fn read_files_and_links(
        &self,
    ) -> IndexResult<(Vec<FileBase>, InlinkMap)> {
        let txn = self.begin_read()?;
        self.read_files_and_links_via(&txn)
    }

    /// Loads every persisted [`FileBase`] (sorted by path) and inlink edge,
    /// without loading the `NOTES` table. Used by
    /// [`super::cache::RefreshCache::load`], which separately bulk-loads and
    /// parallel-decodes every persisted Note via [`Self::read_notes_chunked`]
    /// rather than recalling notes one at a time.
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
        self.collect_multimap_links(&table, &resolve)
    }

    fn collect_multimap_links(
        &self,
        table: &redb::ReadOnlyMultimapTable<&[u8], &[u8]>,
        resolve: &impl Fn(&[u8]) -> Option<PathBuf>,
    ) -> DbResult<HashMap<PathBuf, Vec<PathBuf>>> {
        let mut links = HashMap::new();
        let iter = Box::new(
            table.iter().map_err(|source| self.raise_source_error(source))?,
        );
        for entry in iter {
            if let Some((target, sources)) =
                self.process_link_entry(entry, resolve)?
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

    /// Removes every `LISTS` entry belonging to `path`.
    fn remove_lists_for_path(
        &self,
        table: &mut redb::Table<'_, &[u8], &[u8]>,
        path: &str,
    ) -> IndexResult<()> {
        let (start, end) = Self::list_key_bounds(path);
        table
            .retain_in(start.as_slice()..=end.as_slice(), |k, _| {
                !Self::list_key_matches_path(k, path)
            })
            .map_err(|source| self.raise_source_error(source))?;
        Ok(())
    }

    /// Writes every list item from `note` into `LISTS`.
    ///
    /// Each item's descendant lists are cleared via
    /// [`crate::ListItem::without_children`] before serializing, into a local
    /// value [`ListEntryRef`] then borrows: a `LISTS` row is one list item, not
    /// its subtree, and each descendant is written as its own, independent row.
    /// [`ListEntryRef`] and [`ListEntry`] have identical field order and types,
    /// so postcard's positional encoding is byte-for-byte identical and
    /// [`decode_row`] transparently reads either back as an owned
    /// [`ListEntry`].
    fn write_lists_for_note(
        &self,
        table: &mut redb::Table<'_, &[u8], &[u8]>,
        note: &Note,
    ) -> IndexResult<()> {
        let path_str = note.path().to_string_lossy();
        for item in note.list_items() {
            let key = Self::list_key(&path_str, item.line());
            let leaf = item.without_children();
            let entry = ListEntryRef {
                path: &path_str,
                item: &leaf,
            };
            let bytes = encode_row(note.path(), &entry)?;
            table
                .insert(key.as_slice(), bytes.as_slice())
                .map_err(|source| self.raise_source_error(source))?;
        }
        Ok(())
    }

    /// Atomically replaces every stored [`FileBase`], [`Note`], [`ListEntry`],
    /// and derived inlink edge.
    ///
    /// # Errors
    ///
    /// - [`DbError::Redb`] if the transaction fails.
    /// - [`DbError::Serialize`] if a record cannot be encoded.
    pub(super) fn write_all(&self, entries: &[FileEntry]) -> IndexResult<()> {
        let write_txn = self.prepare_write_txn()?;
        self.write_all_parallel(&write_txn, entries)?;
        write_txn.commit().map_err(|source| self.raise_source_error(source))?;
        Ok(())
    }

    fn prepare_write_txn(&self) -> DbResult<Box<WriteTransaction>> {
        let txn = Box::new(self.begin_write()?);
        self.clear_tables_for_write(&txn)?;
        Ok(txn)
    }

    fn clear_tables_for_write(&self, txn: &WriteTransaction) -> DbResult<()> {
        txn.delete_table(FILES)
            .map_err(|source| self.raise_source_error(source))?;
        txn.delete_table(NOTES)
            .map_err(|source| self.raise_source_error(source))?;
        txn.delete_multimap_table(LINKS)
            .map_err(|source| self.raise_source_error(source))?;
        txn.delete_table(LISTS)
            .map_err(|source| self.raise_source_error(source))?;
        let _ = txn.delete_multimap_table(PATHS_BY_TAG);
        let _ = txn.delete_multimap_table(PATHS_BY_FILE_CLASS);
        Ok(())
    }

    fn write_tags(
        &self,
        txn: &WriteTransaction,
        entries: &[FileEntry],
    ) -> IndexResult<()> {
        let mut table = txn
            .open_multimap_table(PATHS_BY_TAG)
            .map_err(|source| self.raise_source_error(source))?;
        for entry in entries {
            if let Some(note) = entry.note() {
                Self::insert_note_tags(&mut table, note)
                    .map_err(|source| self.raise_source_error(source))?;
            }
        }
        Ok(())
    }

    fn insert_note_tags(
        table: &mut redb::MultimapTable<'_, &[u8], &[u8]>,
        note: &Note,
    ) -> Result<(), redb::StorageError> {
        let path_bytes = note.path().as_os_str().as_encoded_bytes();
        for tag in note.tags() {
            for segment in tag.segments() {
                let normalized = segment.to_lowercase();
                table.insert(normalized.as_bytes(), path_bytes)?;
            }
        }
        Ok(())
    }

    fn write_file_classes(
        &self,
        txn: &WriteTransaction,
        entries: &[FileEntry],
    ) -> IndexResult<()> {
        let mut table = txn
            .open_multimap_table(PATHS_BY_FILE_CLASS)
            .map_err(|source| self.raise_source_error(source))?;
        for entry in entries {
            if let Some(note) = entry.note() {
                Self::insert_note_classes(&mut table, note)
                    .map_err(|source| self.raise_source_error(source))?;
            }
        }
        Ok(())
    }

    fn insert_note_classes(
        table: &mut redb::MultimapTable<'_, &[u8], &[u8]>,
        note: &Note,
    ) -> Result<(), redb::StorageError> {
        let Some(fm) = note.frontmatter() else {
            return Ok(());
        };
        let path_bytes = note.path().as_os_str().as_encoded_bytes();
        for key in ["fileClass", "file_class", "class", "classes"] {
            for val in fm.get_values(key) {
                if let Some(s) = val.as_str() {
                    let normalized = s.to_lowercase();
                    table.insert(normalized.as_bytes(), path_bytes)?;
                }
            }
        }
        Ok(())
    }

    fn write_lists(
        &self,
        txn: &WriteTransaction,
        entries: &[FileEntry],
    ) -> IndexResult<()> {
        let mut lists_table = txn
            .open_table(LISTS)
            .map_err(|source| self.raise_source_error(source))?;
        for note in entries.iter().filter_map(FileEntry::note) {
            self.write_lists_for_note(&mut lists_table, note)?;
        }
        Ok(())
    }

    fn write_all_parallel(
        &self,
        write_txn: &WriteTransaction,
        entries: &[FileEntry],
    ) -> IndexResult<()> {
        let (tx_files, rx_files) = std::sync::mpsc::channel();
        let (tx_notes, rx_notes) = std::sync::mpsc::channel();
        let (tx_links, rx_links) = std::sync::mpsc::channel();
        let (tx_lists, rx_lists) = std::sync::mpsc::channel();
        let (tx_tags, rx_tags) = std::sync::mpsc::channel();
        let (tx_classes, rx_classes) = std::sync::mpsc::channel();

        rayon::scope(|s| {
            s.spawn(|_| {
                let _ = tx_files.send(self.write_table(
                    write_txn,
                    FILES,
                    entries.iter().map(FileEntry::file),
                    FileBase::path,
                ));
            });
            s.spawn(|_| {
                let _ = tx_notes.send(self.write_table(
                    write_txn,
                    NOTES,
                    entries.iter().filter_map(FileEntry::note),
                    Note::path,
                ));
            });
            s.spawn(|_| {
                let _ =
                    tx_links.send(self.write_links(write_txn, LINKS, entries));
            });
            s.spawn(|_| {
                let _ = tx_lists.send(self.write_lists(write_txn, entries));
            });
            s.spawn(|_| {
                let _ = tx_tags.send(self.write_tags(write_txn, entries));
            });
            s.spawn(|_| {
                let _ = tx_classes
                    .send(self.write_file_classes(write_txn, entries));
            });
        });

        rx_files.recv().map_err(|_| {
            self.raise_source_error(redb::Error::Corrupted(
                "worker died".into(),
            ))
        })??;
        rx_notes.recv().map_err(|_| {
            self.raise_source_error(redb::Error::Corrupted(
                "worker died".into(),
            ))
        })??;
        rx_links.recv().map_err(|_| {
            self.raise_source_error(redb::Error::Corrupted(
                "worker died".into(),
            ))
        })??;
        rx_lists.recv().map_err(|_| {
            self.raise_source_error(redb::Error::Corrupted(
                "worker died".into(),
            ))
        })??;
        rx_tags.recv().map_err(|_| {
            self.raise_source_error(redb::Error::Corrupted(
                "worker died".into(),
            ))
        })??;
        rx_classes.recv().map_err(|_| {
            self.raise_source_error(redb::Error::Corrupted(
                "worker died".into(),
            ))
        })??;
        Ok(())
    }

    /// Persists `index` by writing all entries.
    ///
    /// # Errors
    ///
    /// Transaction failure or serialization failure.
    pub(super) fn persist_index(&self, index: &FileIndex) -> IndexResult<()> {
        self.write_all(index.entries())
    }

    /// Row-level incremental write for changes between scans.
    ///
    /// # Errors
    ///
    /// Transaction failure or serialization failure.
    pub(super) fn persist_incremental(
        &self,
        diff: &FileDiff,
        modified_notes: &[Note],
        inlink_delta: &InlinkDelta,
    ) -> IndexResult<()> {
        if diff.is_empty()
            && modified_notes.is_empty()
            && inlink_delta.is_empty()
        {
            return Ok(());
        }
        let write_txn = self.prepare_incremental_txn()?;
        self.apply_diff_deletions(&write_txn, &diff.deleted)?;
        self.apply_diff_upserts(&write_txn, &diff.upserted)?;
        self.apply_modified_notes(&write_txn, modified_notes)?;
        self.apply_inlink_delta(&write_txn, inlink_delta)?;
        write_txn.commit().map_err(|source| self.raise_source_error(source))?;
        Ok(())
    }

    fn apply_diff_deletions(
        &self,
        write_txn: &WriteTransaction,
        deleted: &[FileBase],
    ) -> IndexResult<()> {
        if deleted.is_empty() {
            return Ok(());
        }
        self.delete_files_and_notes(write_txn, deleted)?;
        self.delete_lists_for_paths(write_txn, deleted)?;
        self.delete_tags_and_classes_for_paths(write_txn, deleted)?;
        Ok(())
    }

    fn delete_files_and_notes(
        &self,
        write_txn: &WriteTransaction,
        deleted: &[FileBase],
    ) -> IndexResult<()> {
        let mut files_table = write_txn
            .open_table(FILES)
            .map_err(|source| self.raise_source_error(source))?;
        let mut notes_table = write_txn
            .open_table(NOTES)
            .map_err(|source| self.raise_source_error(source))?;
        for del in deleted {
            let key = del.path().as_os_str().as_encoded_bytes();
            files_table
                .remove(key)
                .map_err(|source| self.raise_source_error(source))?;
            notes_table
                .remove(key)
                .map_err(|source| self.raise_source_error(source))?;
        }
        Ok(())
    }

    fn delete_lists_for_paths(
        &self,
        write_txn: &WriteTransaction,
        deleted: &[FileBase],
    ) -> IndexResult<()> {
        let mut lists_table = write_txn
            .open_table(LISTS)
            .map_err(|source| self.raise_source_error(source))?;
        for del in deleted {
            self.remove_lists_for_path(
                &mut lists_table,
                &del.path().to_string_lossy(),
            )?;
        }
        Ok(())
    }

    fn delete_tags_and_classes_for_paths(
        &self,
        write_txn: &WriteTransaction,
        deleted: &[FileBase],
    ) -> IndexResult<()> {
        let mut tags_table = match write_txn.open_multimap_table(PATHS_BY_TAG) {
            Ok(t) => t,
            Err(redb::TableError::TableDoesNotExist(_)) => return Ok(()),
            Err(e) => return Err(self.raise_source_error(e).into()),
        };
        let mut classes_table =
            match write_txn.open_multimap_table(PATHS_BY_FILE_CLASS) {
                Ok(t) => t,
                Err(redb::TableError::TableDoesNotExist(_)) => return Ok(()),
                Err(e) => return Err(self.raise_source_error(e).into()),
            };
        for del in deleted {
            let key = del.path().as_os_str().as_encoded_bytes();
            Self::remove_multimap_value(&mut tags_table, key)
                .map_err(|source| self.raise_source_error(source))?;
            Self::remove_multimap_value(&mut classes_table, key)
                .map_err(|source| self.raise_source_error(source))?;
        }
        Ok(())
    }

    fn apply_diff_upserts(
        &self,
        write_txn: &WriteTransaction,
        upserted: &[FileBase],
    ) -> IndexResult<()> {
        let mut files_table = write_txn
            .open_table(FILES)
            .map_err(|source| self.raise_source_error(source))?;
        for file in upserted {
            self.upsert_row(&mut files_table, file.path(), file)?;
        }
        Ok(())
    }

    fn apply_modified_notes(
        &self,
        write_txn: &WriteTransaction,
        modified_notes: &[Note],
    ) -> IndexResult<()> {
        if modified_notes.is_empty() {
            return Ok(());
        }
        self.upsert_notes(write_txn, modified_notes)?;
        self.upsert_lists_for_notes(write_txn, modified_notes)?;
        self.upsert_tags_and_classes(write_txn, modified_notes)?;
        Ok(())
    }

    fn upsert_notes(
        &self,
        write_txn: &WriteTransaction,
        modified_notes: &[Note],
    ) -> IndexResult<()> {
        let mut notes_table = write_txn
            .open_table(NOTES)
            .map_err(|source| self.raise_source_error(source))?;
        for note in modified_notes {
            self.upsert_row(&mut notes_table, note.path(), note)?;
        }
        Ok(())
    }

    fn upsert_lists_for_notes(
        &self,
        write_txn: &WriteTransaction,
        modified_notes: &[Note],
    ) -> IndexResult<()> {
        let mut lists_table = write_txn
            .open_table(LISTS)
            .map_err(|source| self.raise_source_error(source))?;
        for note in modified_notes {
            self.remove_lists_for_path(
                &mut lists_table,
                &note.path().to_string_lossy(),
            )?;
            self.write_lists_for_note(&mut lists_table, note)?;
        }
        Ok(())
    }

    fn upsert_tags_and_classes(
        &self,
        write_txn: &WriteTransaction,
        modified_notes: &[Note],
    ) -> IndexResult<()> {
        self.upsert_tags(write_txn, modified_notes)?;
        self.upsert_file_classes(write_txn, modified_notes)?;
        Ok(())
    }

    fn upsert_tags(
        &self,
        write_txn: &WriteTransaction,
        modified_notes: &[Note],
    ) -> IndexResult<()> {
        let mut tags_table = match write_txn.open_multimap_table(PATHS_BY_TAG) {
            Ok(t) => t,
            Err(redb::TableError::TableDoesNotExist(_)) => write_txn
                .open_multimap_table(PATHS_BY_TAG)
                .map_err(|source| self.raise_source_error(source))?,
            Err(e) => return Err(self.raise_source_error(e).into()),
        };
        for note in modified_notes {
            let path_bytes = note.path().as_os_str().as_encoded_bytes();
            Self::remove_multimap_value(&mut tags_table, path_bytes)
                .map_err(|source| self.raise_source_error(source))?;
            Self::insert_note_tags(&mut tags_table, note)
                .map_err(|source| self.raise_source_error(source))?;
        }
        Ok(())
    }

    fn upsert_file_classes(
        &self,
        write_txn: &WriteTransaction,
        modified_notes: &[Note],
    ) -> IndexResult<()> {
        let mut classes_table =
            match write_txn.open_multimap_table(PATHS_BY_FILE_CLASS) {
                Ok(t) => t,
                Err(redb::TableError::TableDoesNotExist(_)) => write_txn
                    .open_multimap_table(PATHS_BY_FILE_CLASS)
                    .map_err(|source| self.raise_source_error(source))?,
                Err(e) => return Err(self.raise_source_error(e).into()),
            };
        for note in modified_notes {
            let path_bytes = note.path().as_os_str().as_encoded_bytes();
            Self::remove_multimap_value(&mut classes_table, path_bytes)
                .map_err(|source| self.raise_source_error(source))?;
            Self::insert_note_classes(&mut classes_table, note)
                .map_err(|source| self.raise_source_error(source))?;
        }
        Ok(())
    }

    fn apply_inlink_delta(
        &self,
        write_txn: &WriteTransaction,
        inlink_delta: &InlinkDelta,
    ) -> IndexResult<()> {
        if inlink_delta.is_empty() {
            return Ok(());
        }
        let mut links_table = write_txn
            .open_multimap_table(LINKS)
            .map_err(|source| self.raise_source_error(source))?;
        for (target, src) in &inlink_delta.deleted {
            let target_key = target.as_os_str().as_encoded_bytes();
            let source_key = src.as_os_str().as_encoded_bytes();
            links_table
                .remove(target_key, source_key)
                .map_err(|err| self.raise_source_error(err))?;
        }
        for (target, src) in &inlink_delta.upserted {
            let target_key = target.as_os_str().as_encoded_bytes();
            let source_key = src.as_os_str().as_encoded_bytes();
            links_table
                .insert(target_key, source_key)
                .map_err(|err| self.raise_source_error(err))?;
        }
        Ok(())
    }

    fn prepare_incremental_txn(&self) -> DbResult<Box<WriteTransaction>> {
        let mut txn = Box::new(self.begin_write()?);
        txn.set_durability(redb::Durability::None)
            .map_err(|source| self.raise_source_error(source))?;
        Ok(txn)
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

    fn remove_multimap_value(
        table: &mut redb::MultimapTable<'_, &[u8], &[u8]>,
        target_value: &[u8],
    ) -> Result<(), redb::StorageError> {
        let to_remove =
            Self::collect_matching_multimap_pairs(table, target_value)?;
        for (k, v) in to_remove {
            table.remove(k.as_slice(), v.as_slice())?;
        }
        Ok(())
    }

    fn collect_matching_multimap_pairs(
        table: &redb::MultimapTable<'_, &[u8], &[u8]>,
        target_value: &[u8],
    ) -> Result<Vec<MultimapPair>, redb::StorageError> {
        let mut to_remove = Vec::new();
        Self::scan_multimap_for_target(table, target_value, &mut to_remove)?;
        Ok(to_remove)
    }

    fn scan_multimap_for_target(
        table: &redb::MultimapTable<'_, &[u8], &[u8]>,
        target_value: &[u8],
        to_remove: &mut Vec<MultimapPair>,
    ) -> Result<(), redb::StorageError> {
        let iter = Box::new(table.iter()?);
        for entry in iter {
            Self::check_multimap_entry(entry, target_value, to_remove)?;
        }
        Ok(())
    }

    fn check_multimap_entry(
        entry: LinkEntry<'_>,
        target_value: &[u8],
        to_remove: &mut Vec<MultimapPair>,
    ) -> Result<(), redb::StorageError> {
        let (k, values) = entry?;
        let k_bytes = k.value().to_vec();
        for val in values {
            let v = val?;
            if v.value() == target_value {
                to_remove.push((k_bytes.clone(), v.value().to_vec()));
            }
        }
        Ok(())
    }

    /// Wraps a redb error with this store's database path.
    pub(super) fn raise_source_error(
        &self,
        source: impl Into<redb::Error>,
    ) -> DbError {
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
        IndexerService,
        note::{MarkdownParserInput, parse_markdown},
    };

    fn parse(path: impl AsRef<Path>, src: &str) -> Note {
        let input = MarkdownParserInput::for_test(path.as_ref(), src);
        parse_markdown(&input)
    }
    #[test]
    fn check_health_passes_on_healthy_database() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let mut store = IndexStore::open(temp.path()).expect("open store");
        assert!(store.check_health().is_ok());
    }

    #[test]
    fn load_file_metadata_returns_persisted_records() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let store = IndexStore::open(temp.path()).expect("open store");
        let files = vec![FileBase::new_test(
            PathBuf::from("a.md"),
            PathBuf::new(),
            crate::file::FileFormat::Note,
        )];
        let write_txn = store.begin_write().expect("write txn");
        store
            .write_table(&write_txn, FILES, &files, FileBase::path)
            .expect("write files");
        write_txn.commit().expect("commit");
        let loaded = store.load_file_metadata().expect("load metadata");
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded.first().map(FileBase::path), Some(Path::new("a.md")));
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
        use chrono::NaiveDate;
        use pretty_assertions::assert_eq;

        use super::*;
        use crate::TaskStatusType;
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
        fn write_all_persists_lists_table() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let store = IndexStore::open(temp.path()).expect("open store");
            let note = parse(
                "note.md",
                "- [ ] Todo task 📅 2025-01-15\n- Plain bullet\n  - [x] Child \
                 task",
            );
            let files = note_files(&["note.md"]);
            write_all_parts(&store, &files, &[note], &HashMap::new())
                .expect("persist");

            let lists = store.read_all_lists().expect("read lists");
            assert_eq!(lists.len(), 3);
            let rec0 = lists.first().expect("first item");
            assert_eq!(rec0.path(), "note.md");
            assert_eq!(rec0.clean_text(), "Todo task");
            assert_eq!(rec0.status_type(), Some(TaskStatusType::Todo));
            assert_eq!(rec0.due_date(), NaiveDate::from_ymd_opt(2025, 1, 15));
            assert_eq!(rec0.line(), SourceLine::new(1));
            assert_eq!(rec0.depth(), 0);

            let rec1 = lists.get(1).expect("second item");
            assert_eq!(rec1.clean_text(), "Plain bullet");
            assert_eq!(rec1.status_type(), None);
            assert_eq!(rec1.line(), SourceLine::new(2));
            assert_eq!(rec1.depth(), 0);

            let rec2 = lists.get(2).expect("third item");
            assert_eq!(rec2.clean_text(), "Child task");
            assert_eq!(rec2.status_type(), Some(TaskStatusType::Done));
            assert_eq!(rec2.line(), SourceLine::new(3));
            assert_eq!(rec2.depth(), 1);
            assert_eq!(rec2.parent_line(), Some(SourceLine::new(2)));
        }

        #[test]
        fn read_lists_for_path_returns_only_matching_note_items() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let store = IndexStore::open(temp.path()).expect("open store");
            let note_a = parse("a.md", "- [ ] Task in A");
            let note_b = parse("b.md", "- [ ] Task in B\n- Plain in B");
            let files = note_files(&["a.md", "b.md"]);
            write_all_parts(&store, &files, &[note_a, note_b], &HashMap::new())
                .expect("persist");

            let txn = store.begin_read().expect("begin read");
            let a_lists =
                store.read_lists_for_path(&txn, "a.md").expect("read a lists");
            let a_item = a_lists.first().expect("first a item");
            assert_eq!(a_item.clean_text(), "Task in A");

            let b_lists =
                store.read_lists_for_path(&txn, "b.md").expect("read b lists");
            assert_eq!(b_lists.len(), 2);
            let b_first = b_lists.first().expect("first b item");
            let b_second = b_lists.get(1).expect("second b item");
            assert_eq!(b_first.clean_text(), "Task in B");
            assert_eq!(b_second.clean_text(), "Plain in B");
            let c_lists =
                store.read_lists_for_path(&txn, "c.md").expect("read c lists");
            assert!(c_lists.is_empty());
        }

        #[test]
        fn incremental_persistence_updates_and_deletes_lists() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let service = IndexerService::new(temp.path());

            fs::write(
                temp.path().join("a.md"),
                "- [ ] A task 1\n- [ ] A task 2",
            )
            .expect("write a.md");
            fs::write(temp.path().join("b.md"), "- [ ] B task 1")
                .expect("write b.md");

            let index = service.build().expect("build index");
            service.persist(&index).expect("persist initial index");

            let initial_lists = service.read_lists().expect("read lists");
            assert_eq!(initial_lists.len(), 3);

            fs::remove_file(temp.path().join("a.md")).expect("remove a.md");
            fs::write(
                temp.path().join("b.md"),
                "- [x] B updated 1\n- [ ] B updated 2",
            )
            .expect("modify b.md");

            let refreshed = service.refresh().expect("refresh index");
            service.persist(&refreshed).expect("persist incremental");

            let updated_lists = service.read_lists().expect("read lists");
            let updated_first =
                updated_lists.first().expect("first updated item");
            assert_eq!(updated_first.path(), "b.md");
            assert_eq!(updated_first.clean_text(), "B updated 1");
            assert_eq!(updated_first.status_type(), Some(TaskStatusType::Done));
            let updated_second =
                updated_lists.get(1).expect("second updated item");
            assert_eq!(updated_second.path(), "b.md");
            assert_eq!(updated_second.clean_text(), "B updated 2");
            assert_eq!(
                updated_second.status_type(),
                Some(TaskStatusType::Todo)
            );
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
            // persisted edge survives, proving the orphans are on disk and
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

        #[test]
        fn recovers_by_rebuilding_when_the_lists_table_has_the_old_str_key_schema()
         {
            let temp = tempfile::tempdir().expect("create temp dir");
            let root = temp.path();
            let db_path = root.join(INDEX_FILE);
            fs::create_dir_all(
                db_path.parent().expect("index file path has a parent"),
            )
            .expect("create .traces dir");
            {
                const OLD_LISTS: TableDefinition<&str, &[u8]> =
                    TableDefinition::new("lists");
                let db =
                    redb::Database::create(&db_path).expect("create raw db");
                let write_txn = db.begin_write().expect("begin write");
                {
                    let mut table = write_txn
                        .open_table(OLD_LISTS)
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
            // B-tree while leaving the container header/checksums valid;
            // this is infeasible to construct reliably without redb's own
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
            let indexer = crate::IndexerService::new(temp.path());
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
