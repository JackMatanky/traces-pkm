//! State-dir-backed store root locations.
//!
//! Named `paths`, not `dirs`, to avoid colliding with the external `dirs`
//! crate this module resolves through (`dirs::state_dir()`) — this repo
//! already calls that crate directly elsewhere (e.g. `discovery.rs`'s
//! `dirs::config_dir()`), so a crate-internal module also named `dirs`
//! would be a confusing near-shadow.

use std::{
    ops::Deref,
    path::{Path, PathBuf},
    sync::LazyLock,
};

/// A path guaranteed to be one of this crate's known state-dir-rooted store
/// locations.
///
/// The constructor is private to this module, so [`TRACKED_CONFIGS`] and
/// [`TRUSTED_CONFIGS`] are the only two values that will ever exist.
/// [`ConfigFileStore::new`](super::store::ConfigFileStore::new) accepts only
/// this type (not a raw path or a name string), so a production caller
/// cannot point a store at an arbitrary or typo'd directory — the only
/// values it can ever pass through are the two below.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct StateDirRoot(PathBuf);

impl StateDirRoot {
    fn new(name: &str) -> Self {
        Self(state_root().join(name))
    }
}

impl Deref for StateDirRoot {
    type Target = Path;

    #[inline]
    fn deref(&self) -> &Path {
        &self.0
    }
}

/// The config tracking store root, under the XDG state dir.
pub(super) static TRACKED_CONFIGS: LazyLock<StateDirRoot> =
    LazyLock::new(|| StateDirRoot::new("tracked-configs"));

/// The trust store root, under the XDG state dir.
///
/// Not yet consumed within this crate — reserved for the trust store
/// (issue 04). Whether trust reuses
/// [`ConfigFileStore`](super::store::ConfigFileStore) with this root or needs
/// its own logic is an issue-04 decision, not this one.
#[allow(
    dead_code,
    reason = "consumed by the trust store (issue 04); this root constant is \
              requested by issue 03's key interfaces"
)]
pub(super) static TRUSTED_CONFIGS: LazyLock<StateDirRoot> =
    LazyLock::new(|| StateDirRoot::new("trusted-configs"));

/// Under test, redirect to the OS temp dir rather than the real state dir —
/// otherwise every test exercising the `Tracked` builder stage would write
/// symlinks into the developer's actual `~/.local/state/traces/`. No test
/// asserts on `TRACKED_CONFIGS`'s contents (`ConfigFileStore` is tested
/// directly against explicit temp roots), so sharing one scratch location
/// across test threads is safe.
#[cfg(test)]
fn state_root() -> PathBuf {
    std::env::temp_dir().join("traces-pkm-test-state")
}

#[cfg(not(test))]
fn state_root() -> PathBuf {
    // ponytail: falls back to a relative "traces" dir in cwd if a platform
    // has neither a state dir nor a data dir (rare — no HOME on a minimal
    // CI image). Upgrade to a hard error if that turns out to bite in
    // practice.
    dirs::state_dir().or_else(dirs::data_dir).unwrap_or_default().join("traces")
}

#[cfg(test)]
mod tests {
    use pretty_assertions::{assert_eq, assert_ne};

    use super::*;

    #[test]
    fn tracked_and_trusted_roots_are_distinct_siblings() {
        assert_ne!(*TRACKED_CONFIGS, *TRUSTED_CONFIGS);
        assert_eq!(TRACKED_CONFIGS.parent(), TRUSTED_CONFIGS.parent());
        assert_eq!(
            TRACKED_CONFIGS.file_name(),
            Some("tracked-configs".as_ref())
        );
        assert_eq!(
            TRUSTED_CONFIGS.file_name(),
            Some("trusted-configs".as_ref())
        );
    }
}
