//! Promotes `src/index/mod.rs`'s internal
//! `persist_then_load_recovers_the_same_records_and_notes` test to prove the
//! public round-trip contract: `FileIndex::build` → `persist` → `load` in a
//! fresh `FileIndex` value, simulating a new process.

use chrono::NaiveDate;
use pretty_assertions::assert_eq;
use traces_pkm::{
    FileEntry, ListItem, ListItemType, SourceLine, TaskConfig, TaskListItem,
    TaskPriority, TaskStatusType, TestProject,
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
    let project = TestProject::trusted(temp.path().join("project"));
    project.write_note("a.md", "# A\n");
    project.write_note("b.md", "# B\n");
    let (indexer, built) = project.persist_index();

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
    let root = temp.path().join("project");
    let markdown = "\
- [ ] Root task #task 📅 2025-06-01
  - [x] Child completed task #task
- [ ] Non-task checkbox
- Plain bullet point
";
    let project = TestProject::trusted(&root);
    project.write_note("todo.md", markdown);

    let config = project.config().with_tasks(TaskConfig::from_tags(&["#task"]));
    let index =
        project.indexer().with_config(&config).build().expect("build index");
    let note =
        index.entries().iter().find_map(FileEntry::note).expect("indexed note");

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

/// Proves a note with tasks persists correct list items inside `Note` in the
/// `NOTES` table.
#[test]
fn note_with_tasks_persists_correct_records_in_notes_table() {
    let temp = tempfile::tempdir().expect("create temp dir");
    let project = TestProject::trusted(temp.path().join("project"));
    let markdown = "\
- [ ] Root task 📅 2025-06-01 🔺
  - [ ] Subtask
    - [x] Grandchild completed
- Plain bullet
";
    project.write_note("tasks.md", markdown);

    let (indexer, _) = project.persist_index();

    let loaded = indexer.load().expect("load persisted index");
    let note = loaded
        .entries()
        .iter()
        .find_map(FileEntry::note)
        .expect("note present");
    let list_records: Vec<_> = note.list_items().collect();

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
        assert_eq!(record.clean_text(), clean_text, "record {index} text");
        assert_eq!(
            record.kind().as_task().map(|t| t.status().kind()),
            status_type,
            "record {index} status_type"
        );
        assert_eq!(record.depth(), depth, "record {index} depth");
        assert_eq!(record.line(), SourceLine::new(line), "record {index} line");
        assert_eq!(
            record.parent(),
            parent_line.and_then(SourceLine::new),
            "record {index} parent_line"
        );
    }

    // Task-only fields: present on the root task, absent on the plain bullet.
    let root = list_records.first().expect("root task record");
    assert_eq!(
        root.kind().as_task().and_then(|t| t.dates().due).map(Into::into),
        NaiveDate::from_ymd_opt(2025, 6, 1)
    );
    assert_eq!(
        root.kind().as_task().and_then(TaskListItem::priority),
        Some(TaskPriority::Highest)
    );
    assert_eq!(
        root.kind().as_task().map(TaskListItem::is_fully_complete),
        Some(false)
    );

    let plain = list_records.get(3).expect("plain bullet record");
    assert_eq!(plain.kind().as_task().and_then(|t| t.dates().due), None);
    assert_eq!(plain.kind().as_task().and_then(TaskListItem::priority), None);
    assert_eq!(
        plain.kind().as_task().map(TaskListItem::is_fully_complete),
        None
    );
}

/// Proves index persistence round-trip preserves all list-derived fields
/// across process recreation (build → persist → fresh service load).
#[test]
fn index_persistence_roundtrip_includes_lists_derived_fields() {
    let temp = tempfile::tempdir().expect("create temp dir");
    let project = TestProject::trusted(temp.path().join("project"));
    let markdown = "\
- [x] Completed task 📅 2025-12-31 🔽
- Plain item
";
    project.write_note("items.md", markdown);

    // 1. Build and persist
    let (_, _built) = project.persist_index();

    // 2. Fresh service instance simulates a new process
    let indexer2 = project.indexer();
    let loaded = indexer2.load().expect("load index from fresh service");
    let note = loaded
        .entries()
        .iter()
        .find_map(FileEntry::note)
        .expect("note present");
    let records: Vec<_> = note.list_items().collect();
    assert_eq!(records.len(), 2);

    let task_rec = records.first().expect("task record");
    assert_eq!(task_rec.clean_text(), "Completed task");
    assert_eq!(task_rec.raw_text(), "Completed task 📅 2025-12-31 🔽");
    assert_eq!(
        task_rec.kind().as_task().map(|t| t.status().kind()),
        Some(TaskStatusType::Done)
    );
    assert_eq!(
        task_rec.kind().as_task().and_then(|t| t.dates().due).map(Into::into),
        NaiveDate::from_ymd_opt(2025, 12, 31)
    );
    assert_eq!(
        task_rec.kind().as_task().and_then(TaskListItem::priority),
        Some(TaskPriority::Low)
    );
    assert_eq!(
        task_rec.kind().as_task().map(TaskListItem::is_fully_complete),
        Some(true)
    );
    assert_eq!(task_rec.line(), SourceLine::new(1));
    assert_eq!(task_rec.depth(), 0);

    let plain_rec = records.get(1).expect("plain record");
    assert_eq!(plain_rec.clean_text(), "Plain item");
    assert_eq!(plain_rec.kind().as_task().map(|t| t.status().kind()), None);
    assert_eq!(
        plain_rec.kind().as_task().map(TaskListItem::is_fully_complete),
        None
    );
    assert_eq!(plain_rec.line(), SourceLine::new(2));
    assert_eq!(plain_rec.depth(), 0);
}
