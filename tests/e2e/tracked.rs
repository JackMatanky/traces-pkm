//! Proves `traces tracked list`/`traces tracked clean` end-to-end — currently
//! zero process-level coverage; only in-process unit tests exist in
//! `src/cli/tracked.rs`.
//!
//! The tracked-config store is populated by `ConfigService::load` (see
//! `src/cli/tracked.rs`'s module docs), not by `trust`. `Sandbox::trusted()`
//! only runs `traces trust`, so every test here runs a load-triggering
//! command (`index`) first to record the local config as a tracked entry.

use super::support::Sandbox;

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
