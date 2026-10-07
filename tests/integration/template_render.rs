//! Proves `TemplateService::render_to_file` crosses `template` + `index` +
//! `note` end-to-end (real files → `WorkspaceIndex` → minijinja `query` global
//! → rendered file content on disk), through the public surface only.

use std::sync::Arc;

use pretty_assertions::{assert_eq, assert_ne};
use rstest::rstest;
use traces_pkm::{
    CommitPolicy, PresetDialogProvider, TemplatePathInput, TemplateService,
    TestProject, TzGuard, WriteMode, WriteOutcome,
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

/// Query and template arithmetic share the calendar owner for a clipped month.
#[test]
fn query_date_arithmetic_agrees_with_template_calendar_shift() {
    let temp = tempfile::tempdir().expect("create temp dir");
    let project = TestProject::trusted(temp.path().join("project"));
    project.write_note("notes/jan.md", "---\nwhen: 2026-01-31\n---\n");
    project.write_note("notes/feb.md", "---\nwhen: 2026-02-15\n---\n");
    project.write_template(
        "calendar.md",
        r#"{{ query.from("notes/").where('when + dur("1 month") == "2026-02-28"') | length }}|{{ "2026-01-31" | date_add(1, unit="months") }}"#,
    );
    let config = project.config();
    let service =
        TemplateService::new(&config, Arc::new(PresetDialogProvider::new()))
            .expect("valid template service");
    let input = TemplatePathInput::parse(std::path::Path::new("calendar"))
        .expect("valid template input");

    let written = service
        .render_to_file(
            &input,
            None,
            WriteMode::Commit(CommitPolicy::CreateNew),
        )
        .expect("render temporal comparison");
    let path = match written {
        WriteOutcome::Written(path) => Some(path),
        WriteOutcome::Previewed(_) => None,
    }
    .expect("commit mode must write");
    assert_eq!(
        std::fs::read_to_string(path).expect("read report"),
        "1|2026-02-28"
    );
}

/// Proves template shorthands accept ISO duration offsets and reference
/// formats.
#[test]
fn template_shorthands_support_iso_offsets_and_reference_format() {
    let temp = tempfile::tempdir().expect("create temp dir");
    let project = TestProject::trusted(temp.path().join("project"));
    project.write_template(
        "offsets.md",
        r#"{{ date.now("YYYY-MM-DD", "P1M", "2026-05-15", "%Y-%m-%d") }}|{{ date.now("YYYY-MM-DD", "P-1M", "2026-05-15", "%Y-%m-%d") }}|{{ date.sow(reference="2026-02-01", reference_format="%Y-%m-%d") }}|{{ date.eow(reference="2026-02-01", reference_format="%Y-%m-%d") }}"#,
    );
    let config = project.config();
    let service =
        TemplateService::new(&config, Arc::new(PresetDialogProvider::new()))
            .expect("valid template service");
    let input = TemplatePathInput::parse(std::path::Path::new("offsets"))
        .expect("valid template input");

    let written = service
        .render_to_file(
            &input,
            None,
            WriteMode::Commit(CommitPolicy::CreateNew),
        )
        .expect("render offsets");
    let path = match written {
        WriteOutcome::Written(path) => Some(path),
        WriteOutcome::Previewed(_) => None,
    }
    .expect("commit mode must write");
    assert_eq!(
        std::fs::read_to_string(path).expect("read report"),
        "2026-06-15|2026-04-15|2026-01-26|2026-02-01"
    );
}

/// Proves template filters support bucketing aliases and weekday arguments.
#[test]
fn template_filters_support_bucketing_aliases_and_weekday_arg() {
    let temp = tempfile::tempdir().expect("create temp dir");
    let project = TestProject::trusted(temp.path().join("project"));
    project.write_template(
        "bucketing.md",
        r#"{{ "2026-02-01" | sow }}|{{ "2026-02-01" | eow }}|{{ "2026-07-29" | som }}|{{ "2026-07-29" | eom }}|{{ "2026-07-29" | soy }}|{{ "2026-07-29" | eoy }}|{{ "2026-07-29" | weekday(1) }}|{{ "2026-07-29" | weekday(7) }}"#,
    );
    let config = project.config();
    let service =
        TemplateService::new(&config, Arc::new(PresetDialogProvider::new()))
            .expect("valid template service");
    let input = TemplatePathInput::parse(std::path::Path::new("bucketing"))
        .expect("valid template input");

    let written = service
        .render_to_file(
            &input,
            None,
            WriteMode::Commit(CommitPolicy::CreateNew),
        )
        .expect("render bucketing");
    let path = match written {
        WriteOutcome::Written(path) => Some(path),
        WriteOutcome::Previewed(_) => None,
    }
    .expect("commit mode must write");
    assert_eq!(
        std::fs::read_to_string(path).expect("read report"),
        "2026-01-26|2026-02-01|2026-07-01|2026-07-31|2026-01-01|2026-12-31|2026-07-27|2026-08-02"
    );
}

/// Proves template weekday rejects out-of-range argument.
#[rstest]
#[case::above_range(8)]
#[case::negative(-1)]
#[case::far_out_of_range(99)]
fn template_weekday_rejects_out_of_range_day_index(#[case] invalid_idx: i64) {
    let temp = tempfile::tempdir().expect("create temp dir");
    let project = TestProject::trusted(temp.path().join("project"));
    project.write_template(
        "invalid_weekday.md",
        &format!(r#"{{{{ "2026-07-29" | weekday({invalid_idx}) }}}}"#),
    );
    let config = project.config();
    let service =
        TemplateService::new(&config, Arc::new(PresetDialogProvider::new()))
            .expect("valid template service");
    let input =
        TemplatePathInput::parse(std::path::Path::new("invalid_weekday"))
            .expect("valid template input");

    assert!(service.render_to_file(&input, None, WriteMode::DryRun).is_err());
}

/// Proves file.day is exposed in query filters with filename precedence.
#[test]
fn file_day_resolves_in_query_with_filename_precedence() {
    let temp = tempfile::tempdir().expect("create temp dir");
    let project = TestProject::trusted(temp.path().join("project"));
    project.write_note("notes/2026-07-29-daily.md", "---\ntitle: Daily\n---\n");
    project.write_note("notes/plain.md", "---\ntitle: Plain\n---\n");
    project.write_template(
        "file_day.md",
        r#"{{ query.from("notes/").where('file.day == "2026-07-29"') | length }}"#,
    );
    let config = project.config();
    let service =
        TemplateService::new(&config, Arc::new(PresetDialogProvider::new()))
            .expect("valid template service");
    let input = TemplatePathInput::parse(std::path::Path::new("file_day"))
        .expect("valid template input");

    let written = service
        .render_to_file(
            &input,
            None,
            WriteMode::Commit(CommitPolicy::CreateNew),
        )
        .expect("render file day");
    let path = match written {
        WriteOutcome::Written(path) => Some(path),
        WriteOutcome::Previewed(_) => None,
    }
    .expect("commit mode must write");
    assert_eq!(std::fs::read_to_string(path).expect("read report"), "1");
}
/// Proves template shorthands reject malformed ISO duration offsets.
#[test]
fn template_shorthands_reject_malformed_iso_offsets() {
    let temp = tempfile::tempdir().expect("create temp dir");
    let project = TestProject::trusted(temp.path().join("project"));
    project.write_template(
        "invalid_offset.md",
        r#"{{ date.now("YYYY-MM-DD", "P1Q2D", "2026-05-15", "%Y-%m-%d") }}"#,
    );
    let config = project.config();
    let service =
        TemplateService::new(&config, Arc::new(PresetDialogProvider::new()))
            .expect("valid template service");
    let input =
        TemplatePathInput::parse(std::path::Path::new("invalid_offset"))
            .expect("valid template input");

    let result = service.render_to_file(&input, None, WriteMode::DryRun);
    assert!(result.is_err());
}

/// Proves file.day persists at index time and does not drift under query-time
/// timezone changes.
#[test]
fn file_day_persists_at_index_time_across_query_time_zone_changes() {
    let temp = tempfile::tempdir().expect("create temp dir");
    let project = TestProject::trusted(temp.path().join("project"));
    project.write_note("notes/2026-07-29-daily.md", "---\ntitle: Daily\n---\n");
    project.write_template(
        "file_day_tz.md",
        r#"{{ query.from("notes/").where('file.day == "2026-07-29"') | length }}"#,
    );
    let config = project.config();
    let service =
        TemplateService::new(&config, Arc::new(PresetDialogProvider::new()))
            .expect("valid template service");
    let input = TemplatePathInput::parse(std::path::Path::new("file_day_tz"))
        .expect("valid template input");

    // Query under UTC+14
    {
        TzGuard::set("Pacific/Kiritimati");
        let written = service
            .render_to_file(
                &input,
                None,
                WriteMode::Commit(CommitPolicy::CreateNew),
            )
            .expect("render file day under Kiritimati");
        let path = match written {
            WriteOutcome::Written(path) => Some(path),
            WriteOutcome::Previewed(_) => None,
        }
        .expect("commit mode must write");
        assert_eq!(std::fs::read_to_string(&path).expect("read report"), "1");
        std::fs::remove_file(path).expect("remove temp report");
    }

    // Query under UTC-12
    {
        TzGuard::set("Etc/GMT+12");
        let written = service
            .render_to_file(
                &input,
                None,
                WriteMode::Commit(CommitPolicy::CreateNew),
            )
            .expect("render file day under GMT+12");
        let path = match written {
            WriteOutcome::Written(path) => Some(path),
            WriteOutcome::Previewed(_) => None,
        }
        .expect("commit mode must write");
        assert_eq!(std::fs::read_to_string(path).expect("read report"), "1");
    }
}
/// Proves date.now renders local wall clock while storage stays UTC under
/// injected TZ.
#[test]
fn template_seam_renders_local_clock_under_injected_tz() {
    TzGuard::set("Etc/GMT-5"); // UTC+05:00
    let temp = tempfile::tempdir().expect("create temp dir");
    let project = TestProject::trusted(temp.path().join("project"));
    project.write_template("clock.md", r#"{{ date.now("%Y") }}"#);
    let config = project.config();
    let service =
        TemplateService::new(&config, Arc::new(PresetDialogProvider::new()))
            .expect("valid template service");
    let input = TemplatePathInput::parse(std::path::Path::new("clock"))
        .expect("valid template input");

    let written = service
        .render_to_file(
            &input,
            None,
            WriteMode::Commit(CommitPolicy::CreateNew),
        )
        .expect("render clock");
    let path = match written {
        WriteOutcome::Written(path) => Some(path),
        WriteOutcome::Previewed(_) => None,
    }
    .expect("commit mode must write");
    let content = std::fs::read_to_string(path).expect("read report");
    assert_eq!(content.len(), 4);
}
