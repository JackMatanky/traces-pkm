//! Redb persistence for [`FileRecord`], [`Note`], and derived inlink records.
//!
//! [`IndexStore`] adapts [`crate::store::DbStore`] for the file-index schema
//! (`FILES`, `NOTES`, `LINKS` tables). Callers use [`super::IndexerService`]
//! methods instead of interacting with redb tables directly.

use std::path::Path;

use redb::{MultimapTableDefinition, TableDefinition};

use super::{INDEX_FILE, error::FileIndexError, inlinks::InlinkMap};
use crate::{file::FileRecord, note::Note, store::DbStore};

/// Postcard-encoded [`FileRecord`] bytes keyed by project-relative path.
const FILES: TableDefinition<&str, &[u8]> = TableDefinition::new("files");

/// Postcard-encoded [`Note`] bytes keyed by project-relative path.
const NOTES: TableDefinition<&str, &[u8]> = TableDefinition::new("notes");

/// Derived inbound-link edges: target path to every linking source path.
/// See [`super::inlinks`]; rewritten in full whenever [`super::FileIndex`]
/// content changes, never patched.
const LINKS: MultimapTableDefinition<&str, &str> =
    MultimapTableDefinition::new("links");

/// Atomically read snapshot of persisted [`FileRecord`] and [`Note`] records
/// (sorted by path) plus derived inlink edges (target-keyed, unordered).
type IndexSnapshot = (Vec<FileRecord>, Vec<Note>, InlinkMap);

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
    /// - [`FileIndexError::Io`] if the database's parent directory cannot be
    ///   created.
    /// - [`FileIndexError::Store`] if the database file cannot be opened.
    pub(super) fn open(root: &Path) -> Result<Self, FileIndexError> {
        let db = DbStore::open(root, INDEX_FILE)?;
        Ok(Self {
            db,
        })
    }

    /// Atomically replaces every stored [`FileRecord`], [`Note`], and derived
    /// inlink edge.
    ///
    /// All three redb tables are cleared and rewritten in one write
    /// transaction, so readers never observe one table refreshed while another
    /// remains stale.
    ///
    /// # Errors
    ///
    /// - [`FileIndexError::Store`] if the transaction fails.
    /// - [`FileIndexError::Serialize`] if a record cannot be encoded.
    pub(super) fn replace_all(
        &self,
        records: &[FileRecord],
        notes: &[Note],
        links: &InlinkMap,
    ) -> Result<(), FileIndexError> {
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
        self.db.store_table(&write_txn, FILES, records, FileRecord::path)?;
        self.db.store_table(&write_txn, NOTES, notes, Note::path)?;
        self.db.store_links(&write_txn, LINKS, links)?;
        write_txn.commit().map_err(|source| self.db.store_error(source))?;
        Ok(())
    }

    /// Loads every stored [`FileRecord`] and [`Note`] (sorted by path) and
    /// every derived inlink edge (target-keyed, unordered).
    ///
    /// # Errors
    ///
    /// - [`FileIndexError::Store`] if a table cannot be read.
    /// - [`FileIndexError::Deserialize`] if stored bytes are not a valid
    ///   record.
    pub(super) fn load_all(&self) -> Result<IndexSnapshot, FileIndexError> {
        let read_txn = self.db.begin_read()?;
        let records = self.db.load_table(&read_txn, FILES, FileRecord::path)?;
        let notes = self.db.load_table(&read_txn, NOTES, Note::path)?;
        let links = self.db.load_links(&read_txn, LINKS)?;
        Ok((records, notes, links))
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

    mod persistence {
        use pretty_assertions::assert_eq;

        use super::*;
        #[test]
        fn returns_empty_when_nothing_persisted() {
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
                "---\ntitle: Hello\n---\nPriority:: 5\n- [ ] task",
            )
            .expect("write note");
            let records = scan_root(temp.path()).expect("scan root");
            let note = parse_markdown(
                "note.md",
                "---\ntitle: Hello\n---\nPriority:: 5\n- [ ] task",
            );
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
            let records = scan_root(temp.path()).expect("scan root");
            let store = IndexStore::open(temp.path()).expect("open store");

            store
                .replace_all(&records, &[], &HashMap::new())
                .expect("persist records");
            let (loaded_records, ..) = store.load_all().expect("load records");

            assert_eq!(loaded_records, records);
        }

        #[test]
        fn returns_records_in_path_sort_order() {
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
        fn rejects_directory_at_index_path() {
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
        fn returns_io_error_when_parent_dir_unwritable() {
            use std::os::unix::fs::PermissionsExt as _;

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
                FileIndexError::Deserialize { path, .. } if path == Path::new("bad.md")
            ));
        }

        #[test]
        fn persists_records_as_postcard_bytes_not_toml_text() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("note.md"), "content")
                .expect("write note");
            let records = scan_root(temp.path()).expect("scan root");
            let store = IndexStore::open(temp.path()).expect("open store");
            store
                .replace_all(&records, &[], &HashMap::new())
                .expect("persist records");

            let read_txn = store.db.begin_read().expect("begin read txn");
            let table = read_txn.open_table(FILES).expect("open table");
            let raw = table
                .get("note.md")
                .expect("read raw value")
                .expect("value present");
            let raw_bytes = raw.value().to_vec();

            assert!(postcard::from_bytes::<FileRecord>(&raw_bytes).is_ok());
            let decodes_as_toml = str::from_utf8(&raw_bytes)
                .ok()
                .and_then(|text| toml::from_str::<FileRecord>(text).ok());
            assert!(decodes_as_toml.is_none());
        }
    }
}
