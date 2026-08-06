//! Proves the real (non-dry-run) template render-and-write path at the
//! process boundary — `dispatch.rs`'s `template` module only covers
//! `--dry-run`.

use pretty_assertions::assert_eq;

use super::support::Sandbox;

/// Renders without `--dry-run` and checks the rendered file lands on disk
/// at the default output path.
///
/// The only test that lets the CLI commit a write — `dispatch.rs`'s
/// `template` tests stay on `--dry-run`. Covers the highest-blast-radius
/// mutating path the CLI has.
#[test]
fn writes_the_rendered_file_to_the_default_output_path() {
    let sandbox = Sandbox::trusted();
    sandbox.write_template("daily.md", "# {{ \"Today\" }}\n");

    let template = sandbox.run(&["template", "-i", "daily", "--no-input"]);

    assert!(template.is_success(), "stderr: {}", template.stderr);
    let written = sandbox.root().join("daily.md");
    assert!(written.is_file());
    assert_eq!(
        std::fs::read_to_string(written).expect("read written file"),
        "# Today"
    );
}
