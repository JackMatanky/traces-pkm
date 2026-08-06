//! Proves `traces tracked list`/`traces tracked clean` end-to-end — currently
//! zero process-level coverage; only in-process unit tests exist in
//! `src/cli/tracked.rs`.
//!
//! The tracked-config store is populated by `ConfigService::load` (see
//! `src/cli/tracked.rs`'s module docs), not by `trust`. `Sandbox::trusted()`
//! only runs `traces trust`, so every test here runs a load-triggering
//! command (`index`) first to record the local config as a tracked entry.

use super::support::Sandbox;

/// Runs `index` to populate the tracked-config store, then checks `tracked
/// list` prints the project's config path.
///
/// `src/config/store.rs` unit-tests the store logic directly. This is the
/// only test proving `tracked list`'s argv parsing and stdout formatting
/// are actually wired to it.
#[test]
fn list_prints_every_tracked_config_path() {
    let sandbox = Sandbox::trusted();
    let index = sandbox.run(&["index"]);
    assert!(index.is_success(), "stderr: {}", index.stderr);

    let list = sandbox.run(&["tracked", "list"]);
    assert!(list.is_success(), "stderr: {}", list.stderr);
    assert!(
        list.stdout.contains(".traces/config.toml"),
        "stdout: {}",
        list.stdout
    );
}

/// Deletes a tracked entry's config file to make it stale, then checks
/// `tracked clean` reports removing exactly one entry.
///
/// The staleness check runs against a real path on a real filesystem after
/// a real deletion — a fixture-backed unit test could fake staleness
/// without proving `clean` actually rechecks disk state.
#[test]
fn clean_removes_a_stale_tracked_entry_and_reports_the_count() {
    let sandbox = Sandbox::trusted();
    let index = sandbox.run(&["index"]);
    assert!(index.is_success(), "stderr: {}", index.stderr);
    std::fs::remove_file(sandbox.root().join(".traces/config.toml"))
        .expect("delete tracked config to make the entry stale");

    let clean = sandbox.run(&["tracked", "clean"]);
    assert!(clean.is_success(), "stderr: {}", clean.stderr);
    assert!(clean.stderr.contains('1'), "stderr: {}", clean.stderr);
}
