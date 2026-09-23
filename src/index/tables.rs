//! Redb table definitions and rebuild delete policies for the index store.
//!
//! [`IndexStore`] owns connections and row payloads; this module owns the
//! static schema: seven [`TableSpec`] entries pairing each table's definition
//! with its rebuild wipe policy.
//!
//! [`IndexStore`]: super::store::IndexStore

use redb::{
    MultimapTableDefinition, ReadTransaction, TableDefinition, WriteTransaction,
};

use super::{error::StoreResult, store::IndexStore};

/// File metadata table.
///
/// Key: project-relative path as UTF-8 bytes
/// Value: serialized [`crate::FileBase`]
pub(super) const FILES: TableDefinition<'static, &'static [u8], &'static [u8]> =
    TableDefinition::new("files");

/// Parsed note metadata table.
///
/// Key: project-relative path as UTF-8 bytes
/// Value: serialized [`crate::Note`]
pub(super) const NOTES: TableDefinition<'static, &'static [u8], &'static [u8]> =
    TableDefinition::new("notes");

/// Inbound link multimap table.
///
/// Key: target note path as UTF-8 bytes
/// Value: one source note path per entry
pub(super) const LINKS: MultimapTableDefinition<
    'static,
    &'static [u8],
    &'static [u8],
> = MultimapTableDefinition::new("links");

/// Path lookup by tag multimap table.
pub(super) const PATHS_BY_TAG: MultimapTableDefinition<
    'static,
    &'static [u8],
    &'static [u8],
> = MultimapTableDefinition::new("paths_by_tag");

/// Path lookup by file class multimap table.
pub(super) const PATHS_BY_FILE_CLASS: MultimapTableDefinition<
    'static,
    &'static [u8],
    &'static [u8],
> = MultimapTableDefinition::new("paths_by_file_class");

/// Reverse tag index: path -> current normalized tags.
///
/// Lets incremental upserts and deletes touch O(path tag count) entries instead
/// of scanning [`PATHS_BY_TAG`].
pub(super) const TAGS_BY_PATH: MultimapTableDefinition<
    'static,
    &'static [u8],
    &'static [u8],
> = MultimapTableDefinition::new("tags_by_path");

/// Reverse file-class index: path -> current normalized classes.
pub(super) const FILE_CLASSES_BY_PATH: MultimapTableDefinition<
    'static,
    &'static [u8],
    &'static [u8],
> = MultimapTableDefinition::new("classes_by_path");

/// Persisted configuration epoch markers.
///
/// Keys: `b"parse"` and `b"class"`
/// Value: postcard-serialized epoch snapshot
pub(super) const EPOCHS: TableDefinition<
    'static,
    &'static [u8],
    &'static [u8],
> = TableDefinition::new("epochs");

/// One of the eight schema tables: plain row or multimap.
enum TableDef {
    Row(TableDefinition<'static, &'static [u8], &'static [u8]>),
    Multimap(MultimapTableDefinition<'static, &'static [u8], &'static [u8]>),
}

/// How a table's rows are treated during a rebuild wipe.
#[derive(Copy, Clone, Eq, PartialEq)]
enum DeletePolicy {
    /// A delete error fails the rebuild.
    Required,
    /// Only a missing table is tolerated; other errors propagate.
    BestEffort,
}

/// One schema fact: a table definition plus its rebuild delete policy.
pub(super) struct TableSpec {
    definition: TableDef,
    policy: DeletePolicy,
}

/// Every table in the schema, in declaration order.
pub(super) const TABLES: [TableSpec; 8] = [
    TableSpec {
        definition: TableDef::Row(FILES),
        policy: DeletePolicy::Required,
    },
    TableSpec {
        definition: TableDef::Row(NOTES),
        policy: DeletePolicy::Required,
    },
    TableSpec {
        definition: TableDef::Multimap(LINKS),
        policy: DeletePolicy::Required,
    },
    TableSpec {
        definition: TableDef::Multimap(PATHS_BY_TAG),
        policy: DeletePolicy::BestEffort,
    },
    TableSpec {
        definition: TableDef::Multimap(PATHS_BY_FILE_CLASS),
        policy: DeletePolicy::BestEffort,
    },
    TableSpec {
        definition: TableDef::Multimap(TAGS_BY_PATH),
        policy: DeletePolicy::BestEffort,
    },
    TableSpec {
        definition: TableDef::Multimap(FILE_CLASSES_BY_PATH),
        policy: DeletePolicy::BestEffort,
    },
    TableSpec {
        definition: TableDef::Row(EPOCHS),
        policy: DeletePolicy::BestEffort,
    },
];

impl TableDef {
    /// Probes the definition against `txn` without reading rows.
    fn probe(&self, txn: &ReadTransaction) -> Result<(), redb::TableError> {
        match self {
            Self::Row(def) => txn.open_table(*def).map(|_| ()),
            Self::Multimap(def) => txn.open_multimap_table(*def).map(|_| ()),
        }
    }
}

impl TableSpec {
    /// Probes this table's definition against `txn` without reading rows.
    pub(super) fn probe(
        &self,
        txn: &ReadTransaction,
    ) -> Result<(), redb::TableError> {
        self.definition.probe(txn)
    }

    /// Deletes this table's contents per its [`DeletePolicy`].
    ///
    /// `Required` propagates every storage error; `BestEffort` tolerates only a
    /// missing table (the fresh-database case) and propagates the rest.
    pub(super) fn delete(
        &self,
        store: &IndexStore,
        txn: &WriteTransaction,
    ) -> StoreResult<()> {
        let run = || match &self.definition {
            TableDef::Row(def) => txn.delete_table(*def).map(|_| ()),
            TableDef::Multimap(def) => {
                txn.delete_multimap_table(*def).map(|_| ())
            }
        };
        match run() {
            Ok(()) => Ok(()),
            Err(redb::TableError::TableDoesNotExist(_))
                if self.policy == DeletePolicy::BestEffort =>
            {
                Ok(())
            }
            Err(source) => Err(store.wrap_redb_error(source)),
        }
    }
}
