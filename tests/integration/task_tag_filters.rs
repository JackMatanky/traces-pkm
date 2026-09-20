//! Integration tests for task tag filter configuration and classification.
//!
//! Proves that configured task tag filters correctly classify status-marked
//! Markdown list items as Tasks vs Checkboxes across real files, config
//! resolution, indexing, and query execution through the public service
//! surface.

use std::sync::Arc;

use chrono::NaiveDate;
use pretty_assertions::{assert_eq, assert_ne};
use traces_pkm::{
    ListItem, ListItemType, QueryBuilder, QueryService, QuerySet,
    SourceSelector, TaskConfig, TaskPriority, TaskStatusType, TestProject,
};

#[test]
fn config_with_tag_filters_classifies_tasks_and_checkboxes_correctly() {
    let temp = tempfile::tempdir().expect("create temp dir");
    let project = TestProject::trusted(temp.path().join("project"));

    let markdown = r"# Tasks and Checklists

- [ ] Buy groceries #task
- [x] Write report #todo
- [ ] Read book #personal
- [ ] Plain checkbox without tag
- [ ] Nested tag item #task/urgent
- Plain bullet with #task
";
    project.write_note("notes.md", markdown);

    let config =
        project.config().with_tasks(TaskConfig::from_tags(&["#task", "#todo"]));
    let index = Arc::new(
        project.indexer().with_config(&config).build().expect("build index"),
    );

    let query_service = QueryService::new("class");
    let task_rows =
        query_service.run(&index, QueryBuilder::tasks(SourceSelector::All));

    assert_eq!(task_rows.len(), 2);
    let task_texts: Vec<&str> = (&task_rows)
        .into_iter()
        .map(|row| row.task_text().unwrap_or_default())
        .collect();
    assert_eq!(task_texts, ["Buy groceries", "Write report"]);

    let note_entry = index.entries().first().expect("entry present");
    let note = note_entry.note().expect("note present");
    let items = note.lists();
    assert_eq!(items.len(), 6);
    assert_eq!(
        items.first().expect("item 0").raw_text(),
        "Buy groceries #task"
    );
    assert_eq!(items.first().expect("item 0").clean_text(), "Buy groceries");
    assert!(items.first().expect("item 0").kind().is_task());
    assert_eq!(items.get(1).expect("item 1").raw_text(), "Write report #todo");
    assert_eq!(items.get(1).expect("item 1").clean_text(), "Write report");
    assert!(items.get(1).expect("item 1").kind().is_task());
    assert_eq!(items.get(2).expect("item 2").text(), "Read book #personal");
    assert_eq!(items.get(2).expect("item 2").kind(), &ListItemType::Checkbox);
    assert_eq!(
        items.get(3).expect("item 3").text(),
        "Plain checkbox without tag"
    );
    assert_eq!(items.get(3).expect("item 3").kind(), &ListItemType::Checkbox);
    assert_eq!(
        items.get(4).expect("item 4").text(),
        "Nested tag item #task/urgent"
    );
    assert_eq!(items.get(4).expect("item 4").kind(), &ListItemType::Checkbox);
    assert_eq!(items.get(5).expect("item 5").text(), "Plain bullet with #task");
    assert_eq!(items.get(5).expect("item 5").kind(), &ListItemType::Plain);
}

#[test]
fn config_without_tag_filters_classifies_all_status_marked_items_as_tasks() {
    let temp = tempfile::tempdir().expect("create temp dir");
    let project = TestProject::trusted(temp.path().join("project"));

    let markdown = r"# Simple Tasks

- [ ] First item
- [x] Second item #other
- [?] Unknown marker item
- Plain bullet
";
    project.write_note("notes.md", markdown);

    let index = Arc::new(project.indexer().build().expect("build index"));

    let query_service = QueryService::new("class");
    let task_rows =
        query_service.run(&index, QueryBuilder::tasks(SourceSelector::All));

    assert_eq!(task_rows.len(), 3);
    let task_texts: Vec<&str> = (&task_rows)
        .into_iter()
        .map(|row| row.task_text().unwrap_or_default())
        .collect();
    assert_eq!(task_texts, [
        "First item",
        "Second item #other",
        "Unknown marker item"
    ]);
}

/// Proves that multi-note vaults with task tag filters correctly classify
/// custom and extended status markers, isolate non-task outlines, preserve
/// dates and priorities, and evaluate parent-child `fully_complete` status.
///
/// Exercises an isolated test vault across multiple notes with:
/// - Custom markers (`[/]`, `[-]`, `[!]`, and unknown marker `[?]`)
/// - Configured task tag filter (`#task`)
/// - Mixed list outlines (plain bullets, checkboxes without `#task`, tasks, and
///   nested subtasks with dates and priorities)
/// - `fully_complete` computation ignoring non-task checklist items and bullets
#[test]
fn classifies_multi_note_vault_lifecycle_with_custom_markers_and_computes_completion()
 {
    let temp = tempfile::tempdir().expect("create temp dir");
    let project = TestProject::trusted(temp.path().join("project"));

    // Note 1: Project Alpha with fully-resolved parent task, incomplete parent
    // task, and supporting non-task outline items.
    let alpha_md = r"# Project Alpha

- [ ] Resolved parent #task 📅 2026-10-01 🔺
  - [x] Child task done #task
  - [-] Child task cancelled #task
  - Plain bullet notes ignored for completion
  - [ ] Non-task checklist item ignored for completion
- [ ] Incomplete parent #task
  - [/] Child in-progress #task
  - [!] Child on-hold #task
  - [?] Child unknown marker #task
";

    // Note 2: Project Beta with nested task dates, priorities, and non-task
    // checklists.
    let beta_md = r"# Project Beta

- [x] Beta top-level #task
  - [x] Beta subtask #task [due:: 2026-10-15] [priority:: medium]
- Plain checklist without tag
  - [ ] Untagged checkbox
- Another plain bullet
";

    project.write_note("projects/alpha.md", alpha_md);
    project.write_note("projects/beta.md", beta_md);

    let config = project.config().with_tasks(TaskConfig::from_tags(&["#task"]));
    let index = Arc::new(
        project.indexer().with_config(&config).build().expect("build index"),
    );

    let query_service = QueryService::new("class");
    let task_rows =
        query_service.run(&index, QueryBuilder::tasks(SourceSelector::All));

    // Alpha has 7 task items, Beta has 2 task items -> total 9 task items.
    // Non-task checkboxes and plain bullets are not present in
    // `QueryBuilder::tasks`.
    assert_eq!(task_rows.len(), 9);

    assert_query_task_statuses(&task_rows);

    // Verify Note 1 (alpha.md) outline classification and `fully_complete`.
    let alpha_entry = index
        .entries()
        .iter()
        .find(|entry| entry.file().path().ends_with("alpha.md"))
        .expect("alpha.md entry");
    let alpha_note = alpha_entry.note().expect("alpha note");
    let alpha_items = alpha_note.lists();
    assert_eq!(alpha_items.len(), 9);
    assert_resolved_parent(alpha_items);
    assert_incomplete_parent(alpha_items);

    // Verify Note 2 (beta.md) outline classification, dates, and inline
    // priority.
    let beta_entry = index
        .entries()
        .iter()
        .find(|entry| entry.file().path().ends_with("beta.md"))
        .expect("beta.md entry");
    let beta_note = beta_entry.note().expect("beta note");
    let beta_items = beta_note.lists();
    assert_eq!(beta_items.len(), 5);

    let beta_parent = beta_items.first().expect("beta parent");
    let beta_parent_task =
        beta_parent.kind().as_task().expect("beta parent is a task");
    assert_eq!(beta_parent_task.is_fully_complete(), true);

    let beta_subtask = beta_items.get(1).expect("beta subtask");
    let beta_subtask_task =
        beta_subtask.kind().as_task().expect("beta subtask is a task");
    assert_eq!(beta_subtask.clean_text(), "Beta subtask");
    assert_eq!(
        beta_subtask_task.dates().due().map(Into::into),
        NaiveDate::from_ymd_opt(2026, 10, 15)
    );
    assert_eq!(beta_subtask_task.priority(), Some(TaskPriority::Medium));

    let beta_plain_checkbox =
        beta_items.get(3).expect("beta untagged checkbox");
    assert_eq!(beta_plain_checkbox.kind(), &ListItemType::Checkbox);
}

/// Asserts task completion tri-state and status marker preservation on query
/// rows.
fn assert_query_task_statuses(task_rows: &QuerySet) {
    let in_progress_row = task_rows
        .iter()
        .find(|row| row.task_text() == Some("Child in-progress"))
        .expect("in-progress task row");
    assert_eq!(in_progress_row.task_completed(), Some(false));
    assert_eq!(in_progress_row.status_symbol(), Some('/'.into()));

    let cancelled_row = task_rows
        .iter()
        .find(|row| row.task_text() == Some("Child task cancelled"))
        .expect("cancelled task row");
    assert_eq!(cancelled_row.task_completed(), None);
    assert_eq!(cancelled_row.status_symbol(), Some('-'.into()));

    let on_hold_row = task_rows
        .iter()
        .find(|row| row.task_text() == Some("Child on-hold"))
        .expect("on-hold task row");
    assert_eq!(on_hold_row.task_completed(), Some(false));
    assert_eq!(on_hold_row.status_symbol(), Some('!'.into()));

    let unknown_row = task_rows
        .iter()
        .find(|row| row.task_text() == Some("Child unknown marker"))
        .expect("unknown marker task row");
    assert_eq!(unknown_row.task_completed(), Some(false));
    assert_eq!(unknown_row.status_symbol(), Some('?'.into()));
}

/// Asserts item classifications and `fully_complete` evaluations on Alpha
/// outline.
fn assert_resolved_parent(alpha_items: &[ListItem]) {
    // 1. Resolved parent: task children are Done and Cancelled; plain bullet
    //    and
    // checkbox are ignored -> fully_complete must be true.
    let resolved_parent = alpha_items.first().expect("resolved parent");
    let resolved_parent_task =
        resolved_parent.kind().as_task().expect("resolved parent is a task");
    assert_eq!(resolved_parent.clean_text(), "Resolved parent");
    assert_eq!(resolved_parent_task.is_fully_complete(), true);
    assert_eq!(
        resolved_parent_task.dates().due().map(Into::into),
        NaiveDate::from_ymd_opt(2026, 10, 1)
    );
    assert_eq!(resolved_parent_task.priority(), Some(TaskPriority::Highest));

    let cancelled_child = alpha_items.get(2).expect("cancelled child");
    let cancelled_task = cancelled_child.kind().as_task().expect("task kind");
    assert_eq!(cancelled_task.status().kind(), TaskStatusType::Cancelled);
    let bullet = alpha_items.get(3).expect("bullet");
    assert_eq!(bullet.kind(), &ListItemType::Plain);

    let untagged_checkbox = alpha_items.get(4).expect("untagged checkbox");
    assert_eq!(untagged_checkbox.kind(), &ListItemType::Checkbox);
}

/// Asserts item classifications for incomplete parents.
fn assert_incomplete_parent(alpha_items: &[ListItem]) {
    // 2. Incomplete parent: has in-progress, on-hold, and unknown children ->
    //    fully_complete is false.
    let incomplete_parent = alpha_items.get(5).expect("incomplete parent");
    let incomplete_parent_task = incomplete_parent
        .kind()
        .as_task()
        .expect("incomplete parent is a task");
    assert_eq!(incomplete_parent.clean_text(), "Incomplete parent");
    assert_eq!(incomplete_parent_task.is_fully_complete(), false);
    assert_ne!(incomplete_parent_task.is_fully_complete(), true);
    let in_progress_child = alpha_items.get(6).expect("in progress child");
    let in_progress_task =
        in_progress_child.kind().as_task().expect("task kind");
    assert_eq!(in_progress_task.status().kind(), TaskStatusType::InProgress);

    let on_hold_child = alpha_items.get(7).expect("on hold child");
    let on_hold_task = on_hold_child.kind().as_task().expect("task kind");
    assert_eq!(on_hold_task.status().kind(), TaskStatusType::OnHold);

    let unknown_child = alpha_items.get(8).expect("unknown child");
    let unknown_task = unknown_child.kind().as_task().expect("task kind");
    assert_eq!(unknown_task.status().kind(), TaskStatusType::Todo);
}
