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
        "{{ query.from_folder(\"notes\") | length }} notes",
    );

    let config = Config::for_test(
        root.clone(),
        Some(root.join("templates")),
        None,
        root.clone(),
    );
    let template_service =
        TemplateService::new(&config, Arc::new(PresetDialogProvider::new()));
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
