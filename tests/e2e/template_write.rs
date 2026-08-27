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
#[test]
fn renders_file_sourced_select_field_in_e2e_template() {
    let sandbox = Sandbox::trusted();

    // Write values file DTO
    sandbox.write_note(
        ".traces/schemas/values/categories.toml",
        "[[entries]]\nid = \"rust\"\ntitle = \"Rust Programming\"\n",
    );

    // Write schema file referencing values file
    sandbox.write_note(
        ".traces/schemas/topic.toml",
        r#"
        [fields.category]
        type = "select"
        values = { path = "values/categories.toml", value = "id", label = "title" }
        "#,
    );

    // Write template referencing the schema select field
    sandbox.write_template(
        "topic_note.md",
        "Category: {{ schema.get('topic').field('category')[0].label }} ({{ \
         schema.get('topic').field('category')[0].value }})",
    );

    // Run the template generation command
    let run = sandbox.run(&["template", "-i", "topic_note", "--no-input"]);

    assert!(run.is_success(), "stderr: {}", run.stderr);
    let written = sandbox.root().join("topic_note.md");
    assert!(written.is_file());
    assert_eq!(
        std::fs::read_to_string(written).expect("read topic note"),
        "Category: Rust Programming (rust)"
    );
}
