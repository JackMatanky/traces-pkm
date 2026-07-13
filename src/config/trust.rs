//! Trust adapter: records and checks trusted directories, following the
//! same shape as [`ConfigTracker`](super::tracker::ConfigTracker) but over
//! [`dirs::TRUSTED_CONFIGS`].
//!
//! Unlike tracking, trust is a security gate: [`ConfigTrust::trust`]
//! propagates store failures instead of swallowing them (see its doc for
//! why a silent trust-store failure is worse than a crash).

use std::path::Path;
#[cfg(test)]
use std::path::PathBuf;

use super::{
    dirs,
    store::{ConfigFileStore, StoreError},
};

/// Records and checks the trusted-directory store.
///
/// Thin adapter over [`ConfigFileStore`], fixing its root to the
/// OS-correct trust-store location ([`dirs::TRUSTED_CONFIGS`]).
#[derive(Clone, Debug)]
pub(super) struct ConfigTrust {
    store: ConfigFileStore,
}

impl ConfigTrust {
    /// Creates the production trust store, rooted at the OS-correct
    /// trusted-configs location under the state dir.
    #[inline]
    #[must_use]
    pub(super) fn new() -> Self {
        Self {
            store: ConfigFileStore::new(&dirs::TRUSTED_CONFIGS),
        }
    }

    /// Creates a trust store rooted at an explicit path. Test-only:
    /// production callers always want the OS-correct root from
    /// [`Self::new`].
    #[cfg(test)]
    #[must_use]
    pub(super) fn at(root: PathBuf) -> Self {
        Self {
            store: ConfigFileStore::at(root),
        }
    }

    /// Marks `dir`'s canonical path as trusted.
    ///
    /// Idempotent: trusting an already-trusted directory is a no-op.
    /// Unlike [`ConfigTracker::track`](super::tracker::ConfigTracker::track),
    /// this propagates store failures rather than logging and swallowing
    /// them: trust is a security decision, and a trust-store write that
    /// silently fails would leave the caller believing a directory is
    /// trusted when it isn't recorded at all.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when `dir` cannot be canonicalized or the
    /// trust entry cannot be written.
    #[inline]
    pub(super) fn trust(&self, dir: &Path) -> Result<(), StoreError> {
        self.store.record(dir)
    }

    /// Returns whether `dir`'s canonical path has a trust entry.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when `dir` cannot be canonicalized or the
    /// trust entry's existence cannot be determined.
    #[inline]
    pub(super) fn is_trusted(&self, dir: &Path) -> Result<bool, StoreError> {
        self.store.contains(dir)
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn is_trusted_returns_false_for_an_untrusted_directory() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let dir = temp.path().join("project");
        fs::create_dir_all(&dir).expect("create project dir");
        let trust = ConfigTrust::at(temp.path().join("trust-store"));

        assert!(!trust.is_trusted(&dir).expect("check trust"));
    }

    #[test]
    fn trust_then_is_trusted_returns_true() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let dir = temp.path().join("project");
        fs::create_dir_all(&dir).expect("create project dir");
        let trust = ConfigTrust::at(temp.path().join("trust-store"));

        trust.trust(&dir).expect("trust directory");

        assert!(trust.is_trusted(&dir).expect("check trust"));
    }

    #[test]
    fn trust_is_idempotent() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let dir = temp.path().join("project");
        fs::create_dir_all(&dir).expect("create project dir");
        let trust = ConfigTrust::at(temp.path().join("trust-store"));

        trust.trust(&dir).expect("trust directory");
        trust.trust(&dir).expect("trust directory again");

        assert!(trust.is_trusted(&dir).expect("check trust"));
    }

    #[test]
    fn trust_of_a_nonexistent_directory_errors() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let trust = ConfigTrust::at(temp.path().join("trust-store"));

        assert!(matches!(
            trust.trust(&temp.path().join("missing")),
            Err(StoreError::Canonicalize { .. })
        ));
    }

    #[test]
    fn trust_propagates_a_store_write_failure_instead_of_swallowing_it() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let dir = temp.path().join("project");
        fs::create_dir_all(&dir).expect("create project dir");
        let root = temp.path().join("trust-store");
        fs::write(&root, "").expect("occupy trust store root with a file");
        let trust = ConfigTrust::at(root);

        assert!(matches!(trust.trust(&dir), Err(StoreError::Io { .. })));
    }
}
