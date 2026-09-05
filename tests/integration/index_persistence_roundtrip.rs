//! Promotes `src/index/mod.rs`'s internal
//! `persist_then_load_recovers_the_same_records_and_notes` test to prove the
//! public round-trip contract: `FileIndex::build` → `persist` → `load` in a
//! fresh `FileIndex` value, simulating a new process.

use std::fs;

use chrono::NaiveDate;
use pretty_assertions::assert_eq;
use traces_pkm::{
    Config, IndexerService, ListItem, ListItemType, SourceLine, Tag,
    TaskConfig, TaskPriority, TaskStatusType,
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

/// Proves `Note.list_items()` returns every item kind (Plain, Checkbox,
/// Task) in document order, while `Note.tasks()` returns only Task items.
///
/// Configures a `#task` tag filter so a status-marked item without the tag
/// classifies as a `Checkbox`, not a `Task` — otherwise every status-marked
/// item defaults to `Task` and this test could never observe the `Checkbox`
/// variant.
#[test]
fn note_list_items_returns_all_item_kinds_and_tasks_returns_only_task_items() {
    let temp = tempfile::tempdir().expect("create temp dir");
    let root = temp.path();
    let markdown = "\
- [ ] Root task #task 📅 2025-06-01
  - [x] Child completed task #task
- [ ] Non-task checkbox
- Plain bullet point
";
    fs::write(root.join("todo.md"), markdown).expect("write todo.md");

    let tag_filters = vec![Tag::parse("#task").expect("valid tag")];
    let tasks_config = TaskConfig::for_test(tag_filters);
    let config =
        Config::for_test(root.to_path_buf(), None, None, root.to_path_buf())
            .with_tasks(tasks_config);
    let index = IndexerService::new(root)
        .with_config(&config)
        .build()
        .expect("build index");
    let note = index
        .entries()
        .iter()
        .find_map(traces_pkm::FileEntry::note)
        .expect("indexed note");

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
    assert!(matches!(
        all_items.get(2).expect("item 2").kind(),
        ListItemType::Checkbox
    ));
    assert!(matches!(
        all_items.get(3).expect("item 3").kind(),
        ListItemType::Plain
    ));

    let clean_texts: Vec<&str> =
        note.list_items().map(ListItem::clean_text).collect();
    assert_eq!(clean_texts, [
        "Root task",
        "Child completed task",
        "Non-task checkbox",
        "Plain bullet point"
    ]);

    let task_texts: Vec<&str> =
        note.tasks().map(ListItem::clean_text).collect();
    assert_eq!(task_texts, ["Root task", "Child completed task"]);
}

/// Proves a note with tasks persists correct `ListEntry`s in the `LISTS`
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

    // (index, clean_text, status_type, depth, line, parent_line)
    let expected = [
        (0, "Root task", Some(TaskStatusType::Todo), 0, 1, None),
        (1, "Subtask", Some(TaskStatusType::Todo), 1, 2, Some(1)),
        (2, "Grandchild completed", Some(TaskStatusType::Done), 2, 3, Some(2)),
        (3, "Plain bullet", None, 0, 4, None),
    ];
    for (index, clean_text, status_type, depth, line, parent_line) in expected {
        let record = list_records.get(index).expect("record in bounds");
        assert_eq!(record.path(), "tasks.md", "record {index} path");
        assert_eq!(record.clean_text(), clean_text, "record {index} text");
        assert_eq!(
            record.status_type(),
            status_type,
            "record {index} status_type"
        );
        assert_eq!(record.depth(), depth, "record {index} depth");
        assert_eq!(record.line(), SourceLine::new(line), "record {index} line");
        assert_eq!(
            record.parent_line(),
            parent_line.map(SourceLine::new),
            "record {index} parent_line"
        );
    }

    // Task-only fields: present on the root task, absent on the plain bullet.
    let root = list_records.first().expect("root task record");
    assert_eq!(root.due_date(), NaiveDate::from_ymd_opt(2025, 6, 1));
    assert_eq!(root.priority(), Some(TaskPriority::Highest));
    assert_eq!(root.is_fully_complete(), Some(false));

    let plain = list_records.get(3).expect("plain bullet record");
    assert_eq!(plain.due_date(), None);
    assert_eq!(plain.priority(), None);
    assert_eq!(plain.is_fully_complete(), None);
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
