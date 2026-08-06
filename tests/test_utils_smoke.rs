//! Proves the `test-utils` feature's gated re-exports are reachable and
//! usable from outside the crate, exactly as an external bench would use
//! them. Delete or replace once a real benchmark exists.
#![cfg(feature = "test-utils")]

use traces_pkm::{Config, ConfigService, parse_markdown};

#[test]
fn test_utils_surface_is_reachable() {
    let temp = tempfile::tempdir().expect("create temp dir");
    let _service = ConfigService::at(
        temp.path().join("tracked-store"),
        temp.path().join("trust-store"),
    );
    let _config = Config::for_test(
        temp.path().to_path_buf(),
        None,
        None,
        temp.path().to_path_buf(),
    );
    let note = parse_markdown("note.md", "# Title\n\nBody text.");
    assert!(note.frontmatter().is_none());
}
