//! Persistent file indexing, metadata caching, and incremental refresh for a
//! project root.
//!
//! [`IndexerService`] scans the filesystem, parses Markdown notes, derives
//! inbound links, and stores the resulting [`WorkspaceIndex`] through the
//! redb-backed [`store`] module.
//!
//! [`WorkspaceIndex`] is an in-memory snapshot of file entries, parsed
//! metadata, and link data. It carries no root path; callers inspect its sorted
//! [`WorkspaceIndex::entries`] view and use [`crate::query::QueryService`] for
//! evaluation.
//!
//! Fresh builds run through [`IndexerService::build`]; cold CLI reads use
//! [`IndexerService::current_store`](service::IndexerService::current_store) to
//! load and refresh an existing store.
//!
//! [`store`]: mod@store
mod codec;
mod delta;
mod entry;
mod error;
mod inlinks;
mod refresh;
mod service;
mod sort;
mod store;
mod tables;
mod trie;

pub(crate) use entry::RowIndex;
pub use entry::{FileEntry, WorkspaceIndex};
pub(crate) use error::{IndexError, IndexResult};
#[cfg(any(test, feature = "test-utils"))]
pub use inlinks::InlinkMap;
#[cfg(any(test, feature = "test-utils"))]
pub use refresh::RefreshReport;
pub use service::IndexerService;
pub(crate) use sort::SortedByPath;
pub(crate) use store::IndexStore;

/// Project-relative index database path.
const INDEX_FILE: &str = ".traces/index.redb";

#[cfg(test)]
mod tests {
    /// Shared fixtures imported by `service.rs` and `store.rs` tests.
    pub(crate) mod fixtures {
        use std::{fs, path::Path};

        /// Restores locked directory permissions on drop so tempdir cleanup
        /// works.
        #[cfg(unix)]
        pub struct RestorePermissions<'a>(pub &'a Path);

        #[cfg(unix)]
        impl Drop for RestorePermissions<'_> {
            fn drop(&mut self) {
                use std::os::unix::fs::PermissionsExt as _;

                let _ = fs::set_permissions(
                    self.0,
                    fs::Permissions::from_mode(0o700),
                );
            }
        }
    }
}
