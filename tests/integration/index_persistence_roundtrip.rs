//! Promotes `src/index/mod.rs`'s internal
//! `persist_then_load_recovers_the_same_records_and_notes` test to prove the
//! public round-trip contract: `FileIndex::build` → `persist` → `load` in a
//! fresh `FileIndex` value, simulating a new process.

use std::fs;

use pretty_assertions::assert_eq;
use traces_pkm::FileIndex;

/// Builds an index, persists it, and reloads it into a fresh `FileIndex`,
/// checking records survive intact.
///
/// `src/index/mod.rs` covers the identical round trip with an internal
/// unit test. This is the only test proving `build`/`persist`/`load` still
/// work when called only through their `pub` signatures.
#[test]
fn persist_then_load_recovers_the_same_record_count_and_paths() {
    let temp = tempfile::tempdir().expect("create temp dir");
    fs::write(temp.path().join("a.md"), "# A\n").expect("write a.md");
    fs::write(temp.path().join("b.md"), "# B\n").expect("write b.md");
    let built = FileIndex::build(temp.path()).expect("build index");
    built.persist(temp.path()).expect("persist index");

    let loaded = FileIndex::load(temp.path()).expect("load persisted index");

    assert_eq!(loaded.records().len(), built.records().len());
    let mut loaded_paths: Vec<_> =
        loaded.records().iter().map(|r| r.path().to_path_buf()).collect();
    loaded_paths.sort();
    assert_eq!(loaded_paths, vec![
        std::path::PathBuf::from("a.md"),
        std::path::PathBuf::from("b.md"),
    ]);
}
