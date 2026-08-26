//! Scan, persist, load, and refresh a file index over a project root.
//!
//! [`IndexerService`] owns a project root and drives the index lifecycle:
//! build, persist, load, and refresh. [`FileIndex`] is the value it
//! produces, a snapshot of every indexed [`crate::file::FileBase`] (from
//! [`crate::file`]), each Markdown file's parsed [`crate::note::Note`], and
//! derived inbound links. `FileIndex` carries no `&Path` of its own;
//! construction and persistence flow entirely through [`IndexerService`].
//!
//! Query execution lives in [`crate::query`]: `QueryService` borrows a
//! [`FileIndex`] through its entry view, keeping `index` focused on indexed
//! data and `query` focused on query semantics.
//!
//! Persistence uses a redb-backed database managed by the [`store`]
//! submodule; callers use [`IndexerService`]'s methods instead of touching
//! redb tables directly.
//!
//! Inbound links between Notes are derived from outlinks during build and
//! refresh, then persisted alongside them; see [`inlinks`].
//!
//! The build pipeline is composed internally by [`builder::IndexBuilder`],
//! which holds a scan result and reuse directive, deferring note parsing,
//! sorting, and inlink derivation to build time.
//!
//! # Lifecycle
//!
//! - Build a fresh index: [`IndexerService::build`]
//! - Persist to disk: [`IndexerService::persist`]
//! - Load from disk: [`IndexerService::load`]
//! - Refresh against the filesystem: [`IndexerService::refresh`]
//!
//! - [`FileIndex::bases`] and [`FileIndex::notes`] expose sorted indexed data
//!   for direct inspection.
//! - [`FileIndex::entries`] exposes a borrowed, allocation-free view pairing
//!   records with optional Notes and inbound links for query execution.
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

pub(crate) use codec::path;
pub use entry::FileIndex;
pub(crate) use entry::FileIndexEntry;
#[allow(unused_imports, reason = "re-exported for downstream callers")]
pub use error::{IndexBuilderError, IndexError};
pub(crate) use inlinks::InlinkMap;
pub use service::IndexerService;

pub(crate) use crate::file::FileFormat;

/// Project-relative path of the persisted [`FileIndex`] database,
/// `.traces/index.redb`, relative to the project root.
const INDEX_FILE: &str = ".traces/index.redb";

#[cfg(test)]
mod tests {
    /// Shared test fixtures live here so `service.rs` and `store.rs` tests
    /// can import them without duplicating the definitions.
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
