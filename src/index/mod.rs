//! Persistent file indexing, metadata caching, and incremental refresh for a
//! project root.
//!
//! The index is an in-memory snapshot of every file, its parsed metadata, and
//! derived inbound links. [`IndexerService`] owns the full lifecycle: scan,
//! parse, persist, load, and refresh. [`FileIndex`] is the value it produces.
//!
//! [`FileIndex`] carries no `&Path` of its own. Construction and persistence
//! flow through [`IndexerService`], while [`FileIndex::entries`] exposes sorted
//! data for direct inspection.
//!
//! Query execution lives in [`crate::query`]. [`crate::query::QueryService`]
//! borrows a [`FileIndex`] through its entry view, keeping `index` focused on
//! data and `query` focused on evaluation.
//!
//! Persistence uses a redb-backed database managed by the [`store`] submodule;
//! callers use [`IndexerService`]'s methods instead of touching redb tables
//! directly.
//!
//! Inbound links between notes are derived from outlinks during build and
//! refresh, then persisted alongside them; see [`inlinks`].
//!
//! The build pipeline is composed internally by [`builder::IndexBuilder`],
//! which holds a scan result and reuse directive, deferring note parsing,
//! sorting, and inlink derivation to build time.
//!
//! # Lifecycle
//!
//! | Step | Entry point |
//! |------|-------------|
//! | Build a fresh index | [`IndexerService::build`] |
//! | Persist to disk | [`IndexerService::persist`] |
//! | Load from disk | [`IndexerService::load`] |
//! | Refresh against filesystem | [`IndexerService::refresh`] |
//!
//! [`store`]: mod@store
//! [`inlinks`]: mod@inlinks
//! [`builder::IndexBuilder`]: mod@builder

mod builder;
mod cache;
mod codec;
mod delta;
mod entry;
mod error;
mod inlinks;
mod service;
mod store;

#[cfg(not(any(test, feature = "test-utils")))]
pub(crate) use codec::path;
#[cfg(any(test, feature = "test-utils"))]
pub use codec::path;
#[cfg(any(test, feature = "test-utils"))]
pub use entry::ListEntry;
pub(crate) use entry::RowIndex;
pub use entry::{FileEntry, FileIndex};
#[cfg(test)]
pub(crate) use error::IndexBuilderError;
pub(crate) use error::{IndexError, IndexResult};
#[cfg(any(test, feature = "test-utils"))]
pub use inlinks::derive_inlinks;
pub use service::IndexerService;

pub(crate) use crate::file::FileFormat;

/// Project-relative path to the index database.
///
/// Stored at `.traces/index.redb`. Callers should use [`IndexerService`]
/// methods instead of opening this path directly.
const INDEX_FILE: &str = ".traces/index.redb";

#[cfg(test)]
mod tests {
    /// Shared test fixtures live here so `service.rs` and `store.rs` tests can
    /// import them without duplicating the definitions.
    pub(crate) mod fixtures {
        use std::{fs, path::Path};

        /// Restores a locked directory's permissions on drop, even if the
        /// test panics. Otherwise, a `0o000` or `0o500` directory blocks the
        /// tempdir's own cleanup.
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
