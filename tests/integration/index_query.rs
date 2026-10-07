//! Proves `WorkspaceIndex::build` → `QueryBuilder` execution works across real
//! files through the test-utils surface alone. Unit coverage inside
//! `src/query/` exercises crate-internal transforms.

use std::{path::Path, sync::Arc};

use pretty_assertions::{assert_eq, assert_ne};
use traces_pkm::{
    IndexerService, QueryBuilder, QueryBuilderError, QueryService, SourceLine,
    SourceSelector, TaskConfig, TestProject,
};

/// Checks a page request returns every indexed note without consuming the
/// borrowed index.
#[test]
fn page_query_returns_real_indexed_notes() {
    let temp = tempfile::tempdir().expect("create temp dir");
    let project = TestProject::trusted(temp.path().join("project"));
    project.write_note("a.md", "---\nrating: 3\n---\n");
    project.write_note("b.md", "---\nrating: 9\n---\n");
    project.write_note("c.md", "---\nrating: 5\n---\n");
    let index = Arc::new(project.build_index());
    let rows = QueryService::new("class")
        .run(&index, QueryBuilder::pages(SourceSelector::All));

    assert_eq!(rows.len(), 3);
    let paths: Vec<_> = (&rows)
        .into_iter()
        .map(|row| row.file().path().to_path_buf())
        .collect();
    assert_eq!(paths, [
        Path::new("a.md"),
        Path::new("b.md"),
        Path::new("c.md")
    ]);
}

/// Checks a date-typed frontmatter field, parsed through the real note
/// index (not a hand-built [`traces_pkm::NoteFieldValue`]), sorts
/// chronologically through the public sort API. Mixes a bare ISO date with
/// an RFC 3339 datetime carrying a `Z` offset, proving both accepted shapes
/// resolve to the same comparable field through the full pipeline.
#[test]
fn sorts_pages_by_a_typed_date_frontmatter_field() {
    let temp = tempfile::tempdir().expect("create temp dir");
    let project = TestProject::trusted(temp.path().join("project"));
    project.write_note("late.md", "---\ndue: 2026-07-29T14:30:00Z\n---\n");
    project.write_note("early.md", "---\ndue: 2026-01-01\n---\n");
    project.write_note("none.md", "no frontmatter");
    let index = Arc::new(project.build_index());
    let query = QueryBuilder::pages(SourceSelector::All)
        .sort("due", false)
        .expect("valid sort");
    let sorted = QueryService::new("class").run(&index, query);
    let paths: Vec<_> = (&sorted)
        .into_iter()
        .map(|row| row.file().path().to_path_buf())
        .collect();

    assert_eq!(paths, [
        Path::new("none.md"),
        Path::new("early.md"),
        Path::new("late.md")
    ]);
}

/// Checks `list.priority` sorts tasks by severity rank (lowest to highest)
/// rather than alphabetically by status name, with tasks lacking a priority
/// sorting first as nulls.
#[test]
fn sorts_tasks_by_priority_severity_rank_with_nulls_first() {
    let temp = tempfile::tempdir().expect("create temp dir");
    let project = TestProject::trusted(temp.path().join("project"));
    project.write_note("p1.md", "- [ ] high task ⏫\n");
    project.write_note("p2.md", "- [ ] low task 🔽\n");
    project.write_note("p3.md", "- [ ] highest task 🔺\n");
    project.write_note("p4.md", "- [ ] lowest task ⏬\n");
    project.write_note("p5.md", "- [ ] plain task\n");
    let index = Arc::new(project.build_index());
    let service = QueryService::new("class");

    let ascending = QueryBuilder::tasks(SourceSelector::All)
        .sort("list.priority", false)
        .expect("valid sort");
    let asc_rows = service.run(&index, ascending);
    let asc_texts: Vec<&str> = (&asc_rows)
        .into_iter()
        .map(|row| row.task_text().expect("task row"))
        .collect();
    assert_eq!(asc_texts, [
        "plain task",
        "lowest task",
        "low task",
        "high task",
        "highest task",
    ]);

    let descending = QueryBuilder::tasks(SourceSelector::All)
        .sort("list.priority", true)
        .expect("valid sort");
    let desc_rows = service.run(&index, descending);
    let desc_texts: Vec<&str> = (&desc_rows)
        .into_iter()
        .map(|row| row.task_text().expect("task row"))
        .collect();
    assert_eq!(desc_texts, [
        "highest task",
        "high task",
        "low task",
        "lowest task",
        "plain task",
    ]);
}

/// Checks task queries flatten two tasks in one note into two rows, each with
/// the correct completion state.
///
/// Proves `QueryRow::task_completed` works from outside the crate.
#[test]
fn query_tasks_returns_task_level_rows_distinct_from_page_level_query() {
    let temp = tempfile::tempdir().expect("create temp dir");
    let project = TestProject::trusted(temp.path().join("project"));
    project.write_note("todo.md", "- [ ] one\n- [x] two\n");
    let index = Arc::new(project.build_index());
    let tasks = QueryService::new("class")
        .run(&index, QueryBuilder::tasks(SourceSelector::All));
    assert_eq!(tasks.len(), 2);
    let completed: Vec<bool> = (0..tasks.len())
        .map(|i| {
            tasks
                .get(i)
                .expect("row")
                .task_completed()
                .expect("task row has a completion state")
        })
        .collect();
    assert_eq!(completed, vec![false, true]);
}

/// Checks one borrowed index can answer page and task requests without being
/// consumed between executions.
#[test]
fn query_builder_reuses_one_index_for_page_and_task_queries() {
    let temp = tempfile::tempdir().expect("create temp dir");
    let project = TestProject::trusted(temp.path().join("project"));
    project.write_note(
        "book.md",
        "---\nrating: 9\n---\n#book [[todo]]\n- [ ] read chapter\n",
    );
    project.write_note("todo.md", "---\nrating: 1\n---\n");
    let index = Arc::new(project.build_index());
    let service = QueryService::new("class");

    let pages = service.run(&index, QueryBuilder::pages(SourceSelector::All));
    let tasks = service.run(&index, QueryBuilder::tasks(SourceSelector::All));
    let pages_again =
        service.run(&index, QueryBuilder::pages(SourceSelector::All));

    let page_paths: Vec<_> = (&pages)
        .into_iter()
        .map(|row| row.file().path().to_path_buf())
        .collect();
    assert_eq!(page_paths, [Path::new("book.md"), Path::new("todo.md")]);
    assert_eq!(tasks.len(), 1);
    assert_eq!(pages_again.len(), 2);
}

/// Proves that `QueryBuilder::lists` yields all list items (bullets,
/// checkboxes, and tasks) with structural metadata (`depth`, `line`, `parent`),
/// whereas `QueryBuilder::tasks` yields only tag-matching tasks.
#[test]
fn evaluates_query_modes_distinguishing_lists_and_tasks_with_structural_metadata()
 {
    let temp = tempfile::tempdir().expect("create temp dir");
    let project = TestProject::trusted(temp.path().join("project"));

    let markdown = r"# Planning

- Plain bullet outline
- [ ] Top-level task #task
  - [x] Child task done #task
  - [ ] Untagged checkbox
- [ ] Second task #task
";
    project.write_note("planning.md", markdown);

    let config = project.config().with_tasks(TaskConfig::from_tags(&["#task"]));
    let index =
        Arc::new(IndexerService::from(&config).build().expect("build index"));
    let service = QueryService::new("class");

    // 1. Lists mode: returns all 5 items with structural fields.
    let lists = service.run(&index, QueryBuilder::lists(SourceSelector::All));
    assert_eq!(lists.len(), 5);

    // Structural metadata verification across all items:
    // Item 0: Plain bullet outline (depth 0, line 3, parent None, not a task)
    let row0 = lists.get(0).expect("row 0");
    assert_eq!(row0.depth(), 0);
    assert_eq!(row0.line(), SourceLine::new(3));
    assert_eq!(row0.parent(), None);
    assert_eq!(row0.task_text(), None);

    // Item 1: Top-level task (depth 0, line 4, parent None, task)
    let row1 = lists.get(1).expect("row 1");
    assert_eq!(row1.depth(), 0);
    assert_eq!(row1.line(), SourceLine::new(4));
    assert_eq!(row1.parent(), None);
    assert_eq!(row1.task_text(), Some("Top-level task"));

    // Item 2: Child task done (depth 1, line 5, parent 4, task)
    let row2 = lists.get(2).expect("row 2");
    assert_eq!(row2.depth(), 1);
    assert_eq!(row2.line(), SourceLine::new(5));
    assert_eq!(row2.parent(), SourceLine::new(4));
    assert_eq!(row2.task_text(), Some("Child task done"));
    assert_eq!(row2.task_completed(), Some(true));

    // Item 3: Untagged checkbox (depth 1, line 6, parent 4, not a task)
    let row3 = lists.get(3).expect("row 3");
    assert_eq!(row3.depth(), 1);
    assert_eq!(row3.line(), SourceLine::new(6));
    assert_eq!(row3.parent(), SourceLine::new(4));
    assert_eq!(row3.task_text(), None);

    // Item 4: Second task (depth 0, line 7, parent None, task)
    let row4 = lists.get(4).expect("row 4");
    assert_eq!(row4.depth(), 0);
    assert_eq!(row4.line(), SourceLine::new(7));
    assert_eq!(row4.parent(), None);
    assert_eq!(row4.task_text(), Some("Second task"));

    // 2. Tasks mode: returns only the 3 tag-matching tasks.
    let tasks = service.run(&index, QueryBuilder::tasks(SourceSelector::All));
    assert_eq!(tasks.len(), 3);
    let task_texts: Vec<&str> =
        tasks.iter().filter_map(|r| r.task_text()).collect();
    assert_eq!(task_texts, [
        "Top-level task",
        "Child task done",
        "Second task"
    ]);
}

/// Proves acceptance of canonical `list.<field>` filter syntax and rejection of
/// obsolete `task.<field>` syntax with an actionable diagnostic hint.
#[test]
fn accepts_canonical_list_paths_and_rejects_obsolete_task_paths_with_diagnostic()
 {
    let temp = tempfile::tempdir().expect("create temp dir");
    let project = TestProject::trusted(temp.path().join("project"));

    let markdown = r"# Work

- [ ] Root item #task
  - [x] Finished subtask #task
  - [ ] Pending subtask #task
";
    project.write_note("work.md", markdown);

    let config = project.config().with_tasks(TaskConfig::from_tags(&["#task"]));
    let index =
        Arc::new(IndexerService::from(&config).build().expect("build index"));
    let service = QueryService::new("class");

    // 1. Canonical list.<field> succeeds:
    let depth_filter = QueryBuilder::lists(SourceSelector::All)
        .filter("list.depth >= 1")
        .expect("valid canonical list.depth filter");
    let depth_rows = service.run(&index, depth_filter);
    assert_eq!(depth_rows.len(), 2);

    let completed_filter = QueryBuilder::tasks(SourceSelector::All)
        .filter("list.completed == true")
        .expect("valid canonical list.completed filter");
    let completed_rows = service.run(&index, completed_filter);
    assert_eq!(completed_rows.len(), 1);
    assert_eq!(
        completed_rows.get(0).and_then(|r| r.task_text()),
        Some("Finished subtask")
    );

    // 2. Obsolete task.<field> is rejected with an actionable diagnostic
    //    suggestion:
    let task_completed_err = QueryBuilder::tasks(SourceSelector::All)
        .filter("task.completed == true")
        .expect_err("obsolete task.completed must be rejected");
    assert!(matches!(task_completed_err, QueryBuilderError::FieldPath(_)));
    let msg = task_completed_err.to_string();
    assert!(msg.contains("did you mean `list.completed`?"), "message: {msg}");

    let task_text_err = QueryBuilder::lists(SourceSelector::All)
        .filter("task.text == \"Finished subtask\"")
        .expect_err("obsolete task.text must be rejected");
    assert!(matches!(task_text_err, QueryBuilderError::FieldPath(_)));
    let msg2 = task_text_err.to_string();
    assert!(msg2.contains("did you mean `list.text`?"), "message: {msg2}");
    assert_ne!(msg, msg2);
}

/// Proves note frontmatter inheritance on list rows and inline field overrides.
#[test]
fn inherits_note_frontmatter_on_list_rows_with_inline_field_override() {
    let temp = tempfile::tempdir().expect("create temp dir");
    let project = TestProject::trusted(temp.path().join("project"));
    let markdown = r"---
category: project
priority: normal
---
# Planning

- [ ] Item one inherits note frontmatter #task
- [ ] Item two overrides inline [priority:: urgent] #task
- Plain bullet inherits note frontmatter
";
    project.write_note("planning.md", markdown);

    let config = project.config().with_tasks(TaskConfig::from_tags(&["#task"]));
    let index =
        Arc::new(IndexerService::from(&config).build().expect("build index"));
    let service = QueryService::new("class");
    // 1. All rows inherit frontmatter field `category == "project"`
    let category_query = QueryBuilder::lists(SourceSelector::All)
        .filter("category == \"project\"")
        .expect("valid category filter");
    let category_rows = service.run(&index, category_query);
    assert_eq!(category_rows.len(), 3);

    // 2. Inline field override: `priority == "urgent"` matches only Item two
    let urgent_query = QueryBuilder::lists(SourceSelector::All)
        .filter("priority == \"urgent\"")
        .expect("valid priority filter");
    let urgent_rows = service.run(&index, urgent_query);
    assert_eq!(urgent_rows.len(), 1);
    assert_eq!(
        urgent_rows.get(0).and_then(|r| r.task_text()),
        Some("Item two overrides inline")
    );

    // 3. Inherited frontmatter `priority == "normal"` matches rows without the
    //    override
    let normal_query = QueryBuilder::lists(SourceSelector::All)
        .filter("priority == \"normal\"")
        .expect("valid priority filter");
    let normal_rows = service.run(&index, normal_query);
    assert_eq!(normal_rows.len(), 2);
    assert_eq!(
        normal_rows.get(0).and_then(|r| r.task_text()),
        Some("Item one inherits note frontmatter")
    );
    assert_eq!(normal_rows.get(1).and_then(|r| r.task_text()), None);
}

/// Proves `sort` + `limit` on the builder returns only the top-k rows
/// (ascending numeric sort, limit 3 out of 10 notes).
#[test]
fn sort_then_limit_returns_top_k_rows() {
    let temp = tempfile::tempdir().expect("create temp dir");
    let project = TestProject::trusted(temp.path().join("project"));
    for i in 0..10 {
        project.write_note(
            format!("note{i}.md"),
            &format!("---\nrating: {i}\n---\n"),
        );
    }
    let index = Arc::new(project.build_index());
    let query = QueryBuilder::pages(SourceSelector::All)
        .sort("rating", false)
        .expect("valid sort")
        .limit(3)
        .expect("valid limit");
    let top = QueryService::new("class").run(&index, query);
    assert_eq!(top.len(), 3);
    let paths: Vec<_> =
        top.iter().map(|r| r.file().path().to_path_buf()).collect();
    assert_eq!(paths, [
        Path::new("note0.md"),
        Path::new("note1.md"),
        Path::new("note2.md"),
    ]);
}

/// Proves sorting an empty result set (filter matches nothing) is a no-op
/// and returns an empty set without panicking.
#[test]
fn sorting_an_empty_query_set_returns_empty() {
    let temp = tempfile::tempdir().expect("create temp dir");
    let project = TestProject::trusted(temp.path().join("project"));
    project.write_note("a.md", "---\nrating: 5\n---\n");
    let index = Arc::new(project.build_index());
    let query = QueryBuilder::pages(SourceSelector::All)
        .filter("rating > 100")
        .expect("valid filter")
        .sort("rating", false)
        .expect("valid sort");
    let result = QueryService::new("class").run(&index, query);
    assert!(result.is_empty());
}

/// Proves descending sort on a text field orders pages alphabetically
/// in reverse (Z→A).
#[test]
fn sorts_pages_descending_by_text_field() {
    let temp = tempfile::tempdir().expect("create temp dir");
    let project = TestProject::trusted(temp.path().join("project"));
    project.write_note("alpha.md", "---\nclass: Alpha\n---\n");
    project.write_note("beta.md", "---\nclass: Beta\n---\n");
    project.write_note("gamma.md", "---\nclass: Gamma\n---\n");
    let index = Arc::new(project.build_index());
    let query = QueryBuilder::pages(SourceSelector::All)
        .sort("class", true)
        .expect("valid sort");
    let sorted = QueryService::new("class").run(&index, query);
    let paths: Vec<_> =
        sorted.iter().map(|r| r.file().path().to_path_buf()).collect();
    assert_eq!(paths, [
        Path::new("gamma.md"),
        Path::new("beta.md"),
        Path::new("alpha.md"),
    ]);
}
/// Proves query filter evaluation of date arithmetic (`date + duration`,
/// `date - duration`, `date - date` -> duration) and duration scaling
/// across real indexed notes.
#[test]
fn filters_pages_using_temporal_arithmetic_and_duration_scaling() {
    let temp = tempfile::tempdir().expect("create temp dir");
    let project = TestProject::trusted(temp.path().join("project"));
    project.write_note("note_a.md", "---\ndue: 2026-07-29\n---\n");
    project.write_note("note_b.md", "---\ndue: 2026-08-15\n---\n");
    let index = Arc::new(project.build_index());
    let service = QueryService::new("class");

    // date + duration
    let plus_query = QueryBuilder::pages(SourceSelector::All)
        .filter("due + dur(\"17d\") == \"2026-08-15\"")
        .expect("valid plus query");
    let plus_rows = service.run(&index, plus_query);
    assert_eq!(plus_rows.len(), 1);
    assert_eq!(
        plus_rows.get(0).expect("row").file().path(),
        Path::new("note_a.md")
    );

    // date - duration
    let minus_query = QueryBuilder::pages(SourceSelector::All)
        .filter("due - dur(\"17d\") == \"2026-07-29\"")
        .expect("valid minus query");
    let minus_rows = service.run(&index, minus_query);
    assert_eq!(minus_rows.len(), 1);
    assert_eq!(
        minus_rows.get(0).expect("row").file().path(),
        Path::new("note_b.md")
    );

    // duration scaling: duration * number
    let scale_query = QueryBuilder::pages(SourceSelector::All)
        .filter("dur(\"1d\") * 17 == dur(\"17d\")")
        .expect("valid scale query");
    let scale_rows = service.run(&index, scale_query);
    assert_eq!(scale_rows.len(), 2);
    // date - date -> duration
    let diff_query = QueryBuilder::pages(SourceSelector::All)
        .filter("due - \"2026-07-29\" == dur(\"17d\")")
        .expect("valid diff query");
    let diff_rows = service.run(&index, diff_query);
    assert_eq!(diff_rows.len(), 1);
    assert_eq!(
        diff_rows.get(0).expect("row").file().path(),
        Path::new("note_b.md")
    );
}

/// Proves query temporal registry functions (`date_add`, `date_diff`,
/// `date_component`) evaluate with reference semantics across indexed notes.
#[test]
fn filters_pages_using_temporal_registry_functions() {
    let temp = tempfile::tempdir().expect("create temp dir");
    let project = TestProject::trusted(temp.path().join("project"));
    project.write_note("note_a.md", "---\ndue: 2026-07-29\n---\n");
    project.write_note("note_b.md", "---\ndue: 2026-08-15\n---\n");
    let index = Arc::new(project.build_index());
    let service = QueryService::new("class");

    // date_add with unit name
    let add_query = QueryBuilder::pages(SourceSelector::All)
        .filter("date_add(due, 17, \"days\") == \"2026-08-15\"")
        .expect("valid date_add query");
    let add_rows = service.run(&index, add_query);
    assert_eq!(add_rows.len(), 1);
    assert_eq!(
        add_rows.get(0).expect("row").file().path(),
        Path::new("note_a.md")
    );

    // date_diff forward difference: target - start
    let fwd_diff = QueryBuilder::pages(SourceSelector::All)
        .filter("date_diff(\"2026-07-29\", due, \"days\") == 17")
        .expect("valid date_diff query");
    let fwd_rows = service.run(&index, fwd_diff);
    assert_eq!(fwd_rows.len(), 1);
    assert_eq!(
        fwd_rows.get(0).expect("row").file().path(),
        Path::new("note_b.md")
    );

    // date_diff backward difference: target - start
    let bwd_diff = QueryBuilder::pages(SourceSelector::All)
        .filter("date_diff(due, \"2026-07-29\", \"days\") == -17")
        .expect("valid date_diff query");
    let bwd_rows = service.run(&index, bwd_diff);
    assert_eq!(bwd_rows.len(), 1);
    assert_eq!(
        bwd_rows.get(0).expect("row").file().path(),
        Path::new("note_b.md")
    );

    // date_component: month, weekday (ISO 1=Mon, 3=Wed), ISO week
    let comp_query = QueryBuilder::pages(SourceSelector::All)
        .filter(
            "date_component(due, \"month\") == 7 and date_component(due, \
             \"weekday\") == 3 and date_component(due, \"week\") == 31",
        )
        .expect("valid date_component query");
    let comp_rows = service.run(&index, comp_query);
    assert_eq!(comp_rows.len(), 1);
    assert_eq!(
        comp_rows.get(0).expect("row").file().path(),
        Path::new("note_a.md")
    );
}

/// Proves bucketing helpers (`sow`, `eow`, `som`, `eom`, `soy`, `eoy`)
/// partition notes into periods across indexed frontmatter.
#[test]
fn filters_pages_using_temporal_bucketing_helpers() {
    let temp = tempfile::tempdir().expect("create temp dir");
    let project = TestProject::trusted(temp.path().join("project"));
    project.write_note("note_a.md", "---\ndue: 2026-07-29\n---\n");
    let index = Arc::new(project.build_index());
    let service = QueryService::new("class");

    for expr in [
        "sow(due) == \"2026-07-27\"",
        "eow(due) == \"2026-08-02\"",
        "som(due) == \"2026-07-01\"",
        "eom(due) == \"2026-07-31\"",
        "soy(due) == \"2026-01-01\"",
        "eoy(due) == \"2026-12-31\"",
    ] {
        let query = QueryBuilder::pages(SourceSelector::All)
            .filter(expr)
            .expect("valid bucketing filter");
        let rows = service.run(&index, query);
        assert_eq!(rows.len(), 1, "failed on {expr}");
    }
}

/// Proves static validation catches invalid temporal arguments at query build
/// time.
#[test]
fn filters_pages_reject_static_invalid_temporal_arguments() {
    for invalid in [
        "date_component(due, \"not_a_component\") == 1",
        "date_add(due, 1, \"fortnights\") == \"2026-08-01\"",
        "date_diff(due, \"2026-08-01\", \"invalid_unit\") == 1",
        "dur(\"not a duration\") == dur(\"1d\")",
    ] {
        let err = QueryBuilder::pages(SourceSelector::All)
            .filter(invalid)
            .expect_err("must reject invalid static temporal arg");
        assert!(matches!(err, QueryBuilderError::Syntax(_)), "{invalid}");
    }
}

/// Proves missing fields and null values in temporal operations evaluate
/// gracefully to null and do not match inequality comparisons.
#[test]
fn filters_pages_handle_null_and_missing_temporal_operands() {
    let temp = tempfile::tempdir().expect("create temp dir");
    let project = TestProject::trusted(temp.path().join("project"));
    project.write_note("note_null.md", "---\ndue: null\n---\n");
    project.write_note("note_missing.md", "---\ntitle: Missing\n---\n");
    let index = Arc::new(project.build_index());
    let service = QueryService::new("class");

    // null + duration produces null, which cannot satisfy inequality
    let inequality_query = QueryBuilder::pages(SourceSelector::All)
        .filter("date_add(due, dur(\"1d\")) > \"2026-01-01\"")
        .expect("valid filter");
    let rows = service.run(&index, inequality_query);
    assert!(rows.is_empty(), "null must never satisfy inequality");

    // null comparison with null equality matches both null and missing notes
    let null_eq_query = QueryBuilder::pages(SourceSelector::All)
        .filter("date_add(due, dur(\"1d\")) == null")
        .expect("valid filter");
    let null_eq_rows = service.run(&index, null_eq_query);
    assert_eq!(null_eq_rows.len(), 2);
}
