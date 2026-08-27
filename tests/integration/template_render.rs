//! Proves `TemplateService::render_to_file` crosses `template` + `index` +
//! `note` end-to-end (real files → `FileIndex` → minijinja `query` global →
//! rendered file content on disk), through the public surface only.

use std::sync::Arc;

use pretty_assertions::assert_eq;
use traces_pkm::{
    CommitPolicy, Config, PresetDialogProvider, TemplatePathInput,
    TemplateService, WriteMode, WriteOutcome, create_trusted_project,
    fixture_service, write_note, write_template,
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
    let root = temp.path().join("project");
    let service = fixture_service(temp.path());
    let _ = create_trusted_project(&service, &root);
    write_note(&root, "notes/a.md", "# A\n");
    write_note(&root, "notes/b.md", "# B\n");
    write_template(
        &root,
        "report.md",
        "{{ query.from(\"notes/\") | length }} notes",
    );

    let config = Config::for_test(
        root.clone(),
        Some(root.join("templates")),
        None,
        root.clone(),
    );
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
    let root = temp.path().join("project");
    let service = fixture_service(temp.path());
    let _ = create_trusted_project(&service, &root);

    let schemas_dir = root.join(".traces/schemas");
    std::fs::create_dir_all(schemas_dir.join("values"))
        .expect("create values dir");

    std::fs::write(
        schemas_dir.join("values/categories.toml"),
        "[[entries]]\nid = \"rust\"\ntitle = \"Rust Programming\"\n",
    )
    .expect("write categories values file");

    std::fs::write(
        schemas_dir.join("topic.toml"),
        r#"
        [fields.category]
        type = "select"
        values = { path = "values/categories.toml", value = "id", label = "title" }
        "#,
    )
    .expect("write topic schema");

    write_template(
        &root,
        "topic_note.md",
        "Category: {{ schema.get('topic').field('category')[0].label }} ({{ \
         schema.get('topic').field('category')[0].value }})",
    );

    let config = Config::for_test(
        root.clone(),
        Some(root.join("templates")),
        Some(schemas_dir),
        root.clone(),
    );
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
