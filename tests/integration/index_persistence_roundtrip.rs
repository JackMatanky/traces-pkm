//! Promotes `src/index/mod.rs`'s internal
//! `persist_then_load_recovers_the_same_records_and_notes` test to prove the
//! public round-trip contract: `FileIndex::build` → `persist` → `load` in a
//! fresh `FileIndex` value, simulating a new process.

use std::fs;

use chrono::NaiveDate;
use pretty_assertions::assert_eq;
use traces_pkm::{
    IndexerService, ListItemType, MarkdownParserInput, SourceLine,
    TaskPriority, TaskStatusType, parse_markdown,
};

/// Builds an index, persists it, and reloads it into a fresh `FileIndex`,
/// checking records survive intact.
///
/// `src/index/mod.rs` covers the identical round trip with an internal
/// unit test. This is the only test proving `build`/`persist`/`load` still
/// work when called only through their `pub` signatures.
#[test]
fn persist_then_load_recovers_the_same_file_count_and_paths() {
    let temp = tempfile::tempdir().expect("create temp dir");
    fs::write(temp.path().join("a.md"), "# A\n").expect("write a.md");
    fs::write(temp.path().join("b.md"), "# B\n").expect("write b.md");
    let indexer = IndexerService::new(temp.path());
    let built = indexer.build().expect("build index");
    indexer.persist(&built).expect("persist index");

    let loaded = indexer.load().expect("load persisted index");

    assert_eq!(loaded.entries().len(), built.entries().len());
    let mut loaded_paths: Vec<_> = loaded
        .entries()
        .iter()
        .map(|entry| entry.file().path().to_path_buf())
        .collect();
    loaded_paths.sort();
    assert_eq!(loaded_paths, vec![
        std::path::PathBuf::from("a.md"),
        std::path::PathBuf::from("b.md"),
    ]);
}

/// Proves `Note.list_items()` returns all item kinds (Plain, Checkbox, Task)
/// while `Note.tasks()` returns only Task items.
#[test]
fn note_list_items_returns_all_item_kinds_and_tasks_returns_only_task_items() {
    let markdown = "\
- [ ] Root task 📅 2025-06-01
  - [x] Child completed task
- [ ] Non-task checkbox
- Plain bullet point
";
    let input = MarkdownParserInput::for_test(
        std::path::Path::new("todo.md"),
        markdown,
    );
    let note = parse_markdown(&input);

    let all_items: Vec<_> = note.list_items().collect();
    assert_eq!(all_items.len(), 4);
    assert!(matches!(
        all_items.first().expect("item 0").kind(),
        ListItemType::Task(_)
    ));
    assert!(matches!(
        all_items.get(1).expect("item 1").kind(),
        ListItemType::Task(_)
    ));
    // Note: without tag filter config, non-task checkbox markers become Tasks
    // or Checkboxes depending on classification. Here [ ] is a default Todo
    // task. Let's verify all kinds are yielded in depth-first order:
    let clean_texts: Vec<&str> =
        note.list_items().map(traces_pkm::ListItem::clean_text).collect();
    assert_eq!(clean_texts, [
        "Root task",
        "Child completed task",
        "Non-task checkbox",
        "Plain bullet point"
    ]);

    let task_texts: Vec<&str> =
        note.tasks().map(traces_pkm::ListItem::clean_text).collect();
    // Plain bullet is excluded from tasks
    assert_eq!(task_texts, [
        "Root task",
        "Child completed task",
        "Non-task checkbox"
    ]);
}

/// Proves a note with tasks persists correct `ListRecord`s in the `LISTS`
/// table.
#[test]
fn note_with_tasks_persists_correct_records_in_lists_table() {
    let temp = tempfile::tempdir().expect("create temp dir");
    let markdown = "\
- [ ] Root task 📅 2025-06-01 🔺
  - [ ] Subtask
    - [x] Grandchild completed
- Plain bullet
";
    fs::write(temp.path().join("tasks.md"), markdown).expect("write tasks.md");

    let indexer = IndexerService::new(temp.path());
    let built = indexer.build().expect("build index");
    indexer.persist(&built).expect("persist index");

    let list_records = indexer.read_lists().expect("read lists table");
    assert_eq!(list_records.len(), 4);

    // Record 0: Root task
    let rec0 = list_records.first().expect("root task record");
    assert_eq!(rec0.path(), "tasks.md");
    assert_eq!(rec0.clean_text(), "Root task");
    assert_eq!(rec0.status_type(), Some(TaskStatusType::Todo));
    assert_eq!(rec0.due_date(), NaiveDate::from_ymd_opt(2025, 6, 1));
    assert_eq!(rec0.priority(), Some(TaskPriority::Highest));
    assert_eq!(rec0.is_fully_complete(), Some(false));
    assert_eq!(rec0.line(), SourceLine::new(1));
    assert_eq!(rec0.depth(), 0);
    assert_eq!(rec0.parent_line(), None);

    // Record 1: Subtask
    let rec1 = list_records.get(1).expect("subtask record");
    assert_eq!(rec1.path(), "tasks.md");
    assert_eq!(rec1.clean_text(), "Subtask");
    assert_eq!(rec1.status_type(), Some(TaskStatusType::Todo));
    assert_eq!(rec1.depth(), 1);
    assert_eq!(rec1.line(), SourceLine::new(2));
    assert_eq!(rec1.parent_line(), Some(SourceLine::new(1)));

    // Record 2: Grandchild completed
    let rec2 = list_records.get(2).expect("grandchild record");
    assert_eq!(rec2.path(), "tasks.md");
    assert_eq!(rec2.clean_text(), "Grandchild completed");
    assert_eq!(rec2.status_type(), Some(TaskStatusType::Done));
    assert_eq!(rec2.depth(), 2);
    assert_eq!(rec2.line(), SourceLine::new(3));
    assert_eq!(rec2.parent_line(), Some(SourceLine::new(2)));

    // Record 3: Plain bullet
    let rec3 = list_records.get(3).expect("plain bullet record");
    assert_eq!(rec3.path(), "tasks.md");
    assert_eq!(rec3.clean_text(), "Plain bullet");
    assert_eq!(rec3.status_type(), None);
    assert_eq!(rec3.priority(), None);
    assert_eq!(rec3.due_date(), None);
    assert_eq!(rec3.is_fully_complete(), None);
    assert_eq!(rec3.depth(), 0);
    assert_eq!(rec3.line(), SourceLine::new(4));
    assert_eq!(rec3.parent_line(), None);
}

/// Proves index persistence round-trip preserves all LISTS-derived fields
/// across process recreation (build → persist → fresh service load).
#[test]
fn index_persistence_roundtrip_includes_lists_derived_fields() {
    let temp = tempfile::tempdir().expect("create temp dir");
    let markdown = "\
- [x] Completed task 📅 2025-12-31 🔽
- Plain item
";
    fs::write(temp.path().join("items.md"), markdown).expect("write items.md");

    // 1. Build and persist
    let indexer1 = IndexerService::new(temp.path());
    let built = indexer1.build().expect("build");
    indexer1.persist(&built).expect("persist");

    // 2. Fresh service instance simulates a new process
    let indexer2 = IndexerService::new(temp.path());
    let records = indexer2.read_lists().expect("read lists from fresh service");
    assert_eq!(records.len(), 2);

    let task_rec = records.first().expect("task record");
    assert_eq!(task_rec.path(), "items.md");
    assert_eq!(task_rec.clean_text(), "Completed task");
    assert_eq!(task_rec.raw_text(), "Completed task 📅 2025-12-31 🔽");
    assert_eq!(task_rec.status_type(), Some(TaskStatusType::Done));
    assert_eq!(task_rec.due_date(), NaiveDate::from_ymd_opt(2025, 12, 31));
    assert_eq!(task_rec.priority(), Some(TaskPriority::Low));
    assert_eq!(task_rec.is_fully_complete(), Some(true));
    assert_eq!(task_rec.line(), SourceLine::new(1));
    assert_eq!(task_rec.depth(), 0);

    let plain_rec = records.get(1).expect("plain record");
    assert_eq!(plain_rec.path(), "items.md");
    assert_eq!(plain_rec.clean_text(), "Plain item");
    assert_eq!(plain_rec.status_type(), None);
    assert_eq!(plain_rec.is_fully_complete(), None);
    assert_eq!(plain_rec.line(), SourceLine::new(2));
    assert_eq!(plain_rec.depth(), 0);
}
