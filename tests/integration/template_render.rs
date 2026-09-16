//! Proves `TemplateService::render_to_file` crosses `template` + `index` +
//! `note` end-to-end (real files → `FileIndex` → minijinja `query` global →
//! rendered file content on disk), through the public surface only.

use std::sync::Arc;

use pretty_assertions::assert_eq;
use traces_pkm::{
    CommitPolicy, PresetDialogProvider, TemplatePathInput, TemplateService,
    TestProject, WriteMode, WriteOutcome,
};

/// Renders a template whose query counts real indexed notes, and checks
/// the written file's content.
///
/// No single module's unit tests cover this seam: `template` and `index`
/// are each tested in isolation. This is the only test proving they
/// compose correctly through the public API alone.
#[test]
fn renders_a_query_over_real_indexed_notes_and_writes_the_result() {
    let temp = tempfile::tempdir().expect("create temp dir");
    let project = TestProject::trusted(temp.path().join("project"));
    project.write_note("notes/a.md", "# A\n");
    project.write_note("notes/b.md", "# B\n");
    project.write_template(
        "report.md",
        "{{ query.from(\"notes/\") | length }} notes",
    );
    let config = project.config();
    let template_service =
        TemplateService::new(&config, Arc::new(PresetDialogProvider::new()))
            .expect("valid test schema directory");
    let input = TemplatePathInput::parse(std::path::Path::new("report"))
        .expect("valid template input");

    let outcome = template_service
        .render_to_file(
            &input,
            None,
            WriteMode::Commit(CommitPolicy::CreateNew),
        )
        .expect("render and write report");

    let written = match outcome {
        WriteOutcome::Written(path) => Some(path),
        WriteOutcome::Previewed(_) => None,
    }
    .expect("Commit mode must write");
    assert_eq!(
        std::fs::read_to_string(written).expect("read report"),
        "2 notes"
    );
}

#[test]
fn renders_a_file_sourced_select_field_in_template_rendering() {
    let temp = tempfile::tempdir().expect("create temp dir");
    let project = TestProject::trusted(temp.path().join("project"));

    project.write_schema_value(
        "categories.toml",
        "[[entries]]\nid = \"rust\"\ntitle = \"Rust Programming\"\n",
    );

    project.write_schema(
        "topic",
        r#"
        [fields.category]
        type = "select"
        values = { path = "values/categories.toml", value = "id", label = "title" }
        "#,
    );

    project.write_template(
        "topic_note.md",
        "Category: {{ schema.get('topic').field('category')[0].label }} ({{ \
         schema.get('topic').field('category')[0].value }})",
    );
    let config = project.config();
    let template_service =
        TemplateService::new(&config, Arc::new(PresetDialogProvider::new()))
            .expect("valid test schema directory");
    let input = TemplatePathInput::parse(std::path::Path::new("topic_note"))
        .expect("valid template input");

    let outcome = template_service
        .render_to_file(
            &input,
            None,
            WriteMode::Commit(CommitPolicy::CreateNew),
        )
        .expect("render and write topic note");

    let written = match outcome {
        WriteOutcome::Written(path) => Some(path),
        WriteOutcome::Previewed(_) => None,
    }
    .expect("Commit mode must write");

    assert_eq!(
        std::fs::read_to_string(written).expect("read topic note"),
        "Category: Rust Programming (rust)"
    );
}
