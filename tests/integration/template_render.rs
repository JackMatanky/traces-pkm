//! Proves `TemplateService::render_to_file` crosses `template` + `index` +
//! `note` end-to-end (real files → `WorkspaceIndex` → minijinja `query` global
//! → rendered file content on disk), through the public surface only.

use std::sync::Arc;

use pretty_assertions::{assert_eq, assert_ne};
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

/// Proves template rendering under `TemplateService` with `tasks.from(...)` and
/// `lists.from(...)` pipelines across transforms (`where`, `sort`, `limit`) and
/// terminal formatters (`task_list`, `table`, `count`), with note frontmatter
/// inheritance and inline field overrides.
#[test]
fn renders_tasks_and_lists_pipelines_with_transforms_and_formatters() {
    let temp = tempfile::tempdir().expect("create temp dir");
    let project = TestProject::trusted(temp.path().join("project"));

    let work_md = r"---
project: alpha
priority: low
---
# Work Notes

- [ ] Task Bravo [priority:: urgent]
- [x] Task Charlie
- [ ] Task Alpha
- Plain bullet in work
";

    let personal_md = r"---
project: personal
priority: normal
---
# Personal Notes

- [ ] Task Delta
- Plain bullet in personal
";

    project.write_note("notes/work.md", work_md);
    project.write_note("notes/personal.md", personal_md);

    let template_body = r#"# Dashboard

Counts:
- Total Tasks: {{ tasks.from('notes/') | count }}
- Alpha Tasks: {{ tasks.from('notes/').where('project == "alpha"') | count }}
- Urgent Tasks: {{ tasks.from('notes/').where('priority == "urgent"') | count }}
- Total Lists: {{ lists.from('notes/') | count }}
- Personal Lists: {{ lists.from('notes/personal.md') | count }}

Task List:
{{ tasks.from('notes/').where('list.completed == false').sort('list.text', false).limit(2).task_list() }}

Table:
{{ tasks.from('notes/').where('project == "alpha"').sort('list.text', false).table(['Task', 'Priority', 'Project'], ['list.text', 'priority', 'project']) }}
"#;
    project.write_template("dashboard.md", template_body);

    let config = project.config();
    let template_service =
        TemplateService::new(&config, Arc::new(PresetDialogProvider::new()))
            .expect("valid template service");
    let input = TemplatePathInput::parse(std::path::Path::new("dashboard"))
        .expect("valid template input");

    let outcome = template_service
        .render_to_file(
            &input,
            None,
            WriteMode::Commit(CommitPolicy::CreateNew),
        )
        .expect("render and write dashboard");

    let written_path = match outcome {
        WriteOutcome::Written(path) => Some(path),
        WriteOutcome::Previewed(_) => None,
    }
    .expect("commit mode must write file");

    let content = std::fs::read_to_string(&written_path)
        .expect("read rendered dashboard");

    // 1. Pipeline counts:
    // Total Tasks: work (3) + personal (1) = 4
    assert!(content.contains("- Total Tasks: 4"), "content: {content}");
    // Alpha Tasks: work (3) inherit project == alpha
    assert!(content.contains("- Alpha Tasks: 3"), "content: {content}");
    // Urgent Tasks: Task Bravo overrides inline priority == urgent
    assert!(content.contains("- Urgent Tasks: 1"), "content: {content}");
    // Total Lists: work (4) + personal (2) = 6
    assert!(content.contains("- Total Lists: 6"), "content: {content}");
    // Personal Lists: 2 in personal.md
    assert!(content.contains("- Personal Lists: 2"), "content: {content}");

    // 2. Terminal task_list() output:
    // Incomplete tasks sorted ascending by text: Task Alpha, Task Bravo
    // (limited to 2)
    assert!(content.contains("- [ ] Task Alpha"), "content: {content}");
    assert!(content.contains("- [ ] Task Bravo"), "content: {content}");
    // Task Delta is excluded by limit(2)
    assert!(!content.contains("- [ ] Task Delta"), "content: {content}");

    // 3. Terminal table(...) output:
    // Alpha tasks sorted ascending: Task Alpha, Task Bravo, Task Charlie
    assert!(
        content.contains("Task")
            && content.contains("Priority")
            && content.contains("Project"),
        "content: {content}"
    );
    assert!(
        content.contains("Task Alpha")
            && content.contains("low")
            && content.contains("alpha"),
        "content: {content}"
    );
    assert!(
        content.contains("Task Bravo") && content.contains("urgent"),
        "content: {content}"
    );
    assert!(content.contains("Task Charlie"), "content: {content}");
    assert_ne!(content, template_body);
}

/// Proves `tasks.from(...).where(...)` rejects obsolete `task.<field>`
/// syntax with the same actionable diagnostic proven at the library level in
/// `tests/integration/index_query.rs`, confirming the `tasks` template
/// global routes through the identical `QueryBuilder::filter` validation as
/// the `query` and `list.<field>` paths, rather than a namespace-specific
/// bypass.
#[test]
fn tasks_pipeline_rejects_obsolete_task_field_syntax_with_diagnostic() {
    use std::error::Error as StdError;

    let temp = tempfile::tempdir().expect("create temp dir");
    let project = TestProject::trusted(temp.path().join("project"));
    project.write_note("todo.md", "- [ ] one\n");
    project.write_template(
        "broken.md",
        "{{ tasks.from().where('task.completed == true') | count }}",
    );

    let config = project.config();
    let template_service =
        TemplateService::new(&config, Arc::new(PresetDialogProvider::new()))
            .expect("valid template service");
    let input = TemplatePathInput::parse(std::path::Path::new("broken"))
        .expect("valid template input");

    let err = template_service
        .render_to_file(&input, None, WriteMode::DryRun)
        .expect_err(
            "obsolete task.<field> inside tasks.from().where() must be \
             rejected",
        );

    let minijinja_source =
        err.source().expect("Render variant carries a minijinja source");
    let message = minijinja_source
        .source()
        .expect("minijinja error carries the query source")
        .to_string();
    assert!(
        message.contains("did you mean `list.completed`?"),
        "message: {message}"
    );
}

/// Proves `lists.from(...).task_list()` rejects page-level (non-task) rows
/// with `QueryError::TaskListRequiresTaskRows`, confirming the `lists`
/// namespace's generic rows cannot silently pass through a formatter that
/// requires task fields.
#[test]
fn lists_pipeline_rejects_task_list_formatter_on_non_task_rows() {
    use std::error::Error as StdError;

    let temp = tempfile::tempdir().expect("create temp dir");
    let project = TestProject::trusted(temp.path().join("project"));
    project.write_note("todo.md", "- Plain bullet\n");
    project.write_template("broken.md", "{{ lists.from().task_list() }}");

    let config = project.config();
    let template_service =
        TemplateService::new(&config, Arc::new(PresetDialogProvider::new()))
            .expect("valid template service");
    let input = TemplatePathInput::parse(std::path::Path::new("broken"))
        .expect("valid template input");

    let err = template_service
        .render_to_file(&input, None, WriteMode::DryRun)
        .expect_err(
            "task_list() on page-level (non-task) list rows must be rejected",
        );

    let minijinja_source =
        err.source().expect("Render variant carries a minijinja source");
    let message = minijinja_source
        .source()
        .expect("minijinja error carries the query source")
        .to_string();
    assert!(
        message.contains("task_list requires task-level records"),
        "message: {message}"
    );
}
