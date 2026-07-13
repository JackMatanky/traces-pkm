//! Config tracking adapter: records config file candidates seen by
//! [`ConfigService`](super::ConfigService), across all projects.

use std::path::{Path, PathBuf};

use super::{
    dirs,
    store::{ConfigFileStore, StoreError},
};

/// Records, lists, and cleans the tracked-config store.
///
/// Thin adapter over [`ConfigFileStore`], fixing its root to the OS-correct
/// tracked-configs location ([`dirs::TRACKED_CONFIGS`]) and owning the
/// tracking-specific policy that the shared store itself doesn't know
/// about: a write failure is bookkeeping, not a reason to fail config
/// loading, so [`Self::track`] logs and swallows rather than returning a
/// `Result` (see [`Self::track`]'s doc for why that belongs here and not in
/// [`super::builder`]).
#[derive(Clone, Debug)]
pub(super) struct ConfigTracker {
    store: ConfigFileStore,
}

impl ConfigTracker {
    /// Creates the production tracker, rooted at the OS-correct
    /// tracked-configs location under the state dir.
    #[inline]
    #[must_use]
    pub(super) fn new() -> Self {
        Self {
            store: ConfigFileStore::new(&dirs::TRACKED_CONFIGS),
        }
    }

    /// Creates a tracker rooted at an explicit path. Test-only: production
    /// callers always want the OS-correct root from [`Self::new`].
    #[cfg(test)]
    #[must_use]
    pub(super) fn at(root: PathBuf) -> Self {
        Self {
            store: ConfigFileStore::at(root),
        }
    }

    /// Records `target`'s canonical path in the tracked-config store.
    ///
    /// Idempotent: recording an already-tracked path is a no-op. Best-effort:
    /// tracking is bookkeeping, not a precondition for loading a config, so a
    /// store write failure is logged via `tracing::warn!` and swallowed here
    /// rather than returned — this method's non-`Result` signature is the
    /// guarantee that a caller (the `Tracked` builder stage) can never turn a
    /// tracking failure into a config-loading failure.
    #[inline]
    pub(super) fn track(&self, target: &Path) {
        if let Err(error) = self.store.record(target) {
            tracing::warn!(
                path = %target.display(),
                %error,
                "failed to record tracked config"
            );
        }
    }

    /// Lists the canonical paths of all live entries in the tracked-config
    /// store.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when the store directory exists but cannot be
    /// read.
    #[inline]
    pub(super) fn list_all(&self) -> Result<Vec<PathBuf>, StoreError> {
        self.store.list_all()
    }

    /// Removes dangling tracked-config entries (target deleted or moved).
    /// Returns the number of entries removed.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when the store directory exists but cannot be
    /// read, or a stale entry cannot be removed.
    #[inline]
    pub(super) fn clean(&self) -> Result<usize, StoreError> {
        self.store.clean()
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn track_logs_and_swallows_a_store_write_failure() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let root = temp.path().join("store-root");
        fs::write(&root, "").expect("occupy store root with a file");
        let target = temp.path().join("config.toml");
        fs::write(&target, "").expect("write config");
        let tracker = ConfigTracker::at(root);

        // Must not panic: a store write failure is logged, not propagated.
        tracker.track(&target);
    }

    #[test]
    fn track_records_a_valid_target() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let target = temp.path().join("config.toml");
        fs::write(&target, "").expect("write config");
        let tracker = ConfigTracker::at(temp.path().join("store"));

        tracker.track(&target);

        assert_eq!(tracker.list_all().expect("list tracked configs"), vec![
            target.canonicalize().expect("canonicalize target")
        ]);
    }
}
