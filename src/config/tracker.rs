//! Config tracking adapter: records config file candidates seen by
//! [`ConfigService`](super::ConfigService), across all projects.

use std::path::Path;

use super::{
    paths,
    store::{ConfigFileStore, StoreError},
};

/// Records, lists, and cleans the tracked-config store.
///
/// Thin adapter over [`ConfigFileStore`] that fixes the store root to
/// [`paths::TRACKED_CONFIGS`]. A namespace, not a value: there is no
/// per-instance state.
pub(super) struct ConfigTracker;

impl ConfigTracker {
    /// Records `target`'s canonical path in the tracked-config store.
    ///
    /// Idempotent: recording an already-tracked path is a no-op.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when `target` cannot be canonicalized, the store
    /// root cannot be created, or the entry cannot be written.
    #[inline]
    pub(super) fn track(target: &Path) -> Result<(), StoreError> {
        ConfigFileStore::record(&paths::TRACKED_CONFIGS, target)
    }

    /// Lists the canonical paths of all live entries in the tracked-config
    /// store.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when the store directory exists but cannot be
    /// read.
    #[inline]
    #[allow(
        dead_code,
        reason = "no caller yet within this crate; required by issue 03's \
                  acceptance criteria as the programmatic capability, ahead \
                  of the CLI/cross-project surface that consumes it"
    )]
    pub(super) fn list_all() -> Result<Vec<std::path::PathBuf>, StoreError> {
        ConfigFileStore::list_all(&paths::TRACKED_CONFIGS)
    }

    /// Removes dangling tracked-config entries (target deleted or moved).
    /// Returns the number of entries removed.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when the store directory exists but cannot be
    /// read, or a stale entry cannot be removed.
    #[inline]
    #[allow(
        dead_code,
        reason = "no caller yet within this crate; required by issue 03's \
                  acceptance criteria as the programmatic capability, ahead \
                  of the CLI/cross-project surface that consumes it"
    )]
    pub(super) fn clean() -> Result<usize, StoreError> {
        ConfigFileStore::clean(&paths::TRACKED_CONFIGS)
    }
}
