//! State-dir-backed store root locations.
//!
//! Named `paths`, not `dirs`, to avoid colliding with the external `dirs`
//! crate this module resolves through (`dirs::state_dir()`) — this repo
//! already calls that crate directly elsewhere (e.g. `discovery.rs`'s
//! `dirs::config_dir()`), so a crate-internal module also named `dirs`
//! would be a confusing near-shadow.

use std::{path::PathBuf, sync::LazyLock};

/// The config tracking store root, under the XDG state dir.
pub(super) static TRACKED_CONFIGS: LazyLock<PathBuf> =
    LazyLock::new(|| state_root().join("tracked-configs"));

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
pub(super) static TRUSTED_CONFIGS: LazyLock<PathBuf> =
    LazyLock::new(|| state_root().join("trusted-configs"));

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
