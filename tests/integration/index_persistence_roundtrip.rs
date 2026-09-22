//! Promotes `src/index/mod.rs`'s internal
//! `persist_then_load_recovers_the_same_records_and_notes` test to prove the
//! public round-trip contract: `FileIndex::build` → `persist` → `load` in a
//! fresh `FileIndex` value, simulating a new process. Also covers redb
//! persistence invariance without the `LISTS` table (ADR 0005) and recovery
//! from a corrupted `.traces/index.redb` file, both through the public
//! `IndexerService` surface alone.

use std::sync::Arc;

use chrono::NaiveDate;
use pretty_assertions::assert_eq;
use traces_pkm::{
    FileEntry, QueryBuilder, QueryService, QuerySet, SourceLine,
    SourceSelector, TaskListItem, TaskPriority, TaskStatusType, TestProject,
};
/// Builds an index, persists it, and reloads it into a fresh `FileIndex`,
/// checking records survive intact.
///
/// `src/index/mod.rs` covers the identical round trip with an internal unit
/// test. This is the only test proving `build`/`persist`/`load` still work when
/// called only through their `pub` signatures.
#[test]
fn persist_then_load_recovers_the_same_file_count_and_paths() {
    let temp = tempfile::tempdir().expect("create temp dir");
    let project = TestProject::trusted(temp.path().join("project"));
    project.write_note("a.md", "# A\n");
    project.write_note("b.md", "# B\n");
    let (indexer, built) = project.build_and_persist();

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

/// Proves list items with tasks persist inside `Note` in the `NOTES` table and
/// reload with full metadata and hierarchy through a fresh service instance,
/// simulating a new process.
#[test]
fn reloads_flat_list_items_with_metadata_and_hierarchy() {
    let temp = tempfile::tempdir().expect("create temp dir");
    let project = TestProject::trusted(temp.path().join("project"));
    let markdown = "\
- [ ] Root task 📅 2025-06-01 🔺
  - [ ] Subtask
    - [x] Grandchild completed
- Plain bullet
";
    project.write_note("tasks.md", markdown);

    // Arrange done; build, persist, then reload from a fresh service.
    let (_, _) = project.build_and_persist();
    let fresh_indexer = project.indexer();
    let loaded = fresh_indexer.load().expect("load persisted index");
    let note = loaded
        .entries()
        .iter()
        .find_map(FileEntry::note)
        .expect("note present");
    let records: Vec<_> = note.list_items().collect();

    assert_eq!(records.len(), 4);

    // (index, clean_text, status_type, depth, line, parent_line)
    let expected = [
        (0, "Root task", Some(TaskStatusType::Todo), 0, 1, None),
        (1, "Subtask", Some(TaskStatusType::Todo), 1, 2, Some(1)),
        (2, "Grandchild completed", Some(TaskStatusType::Done), 2, 3, Some(2)),
        (3, "Plain bullet", None, 0, 4, None),
    ];
    for (index, clean_text, status_type, depth, line, parent_line) in expected {
        let record = records.get(index).expect("record in bounds");
        assert_eq!(record.clean_text(), clean_text, "record {index} text");
        assert_eq!(
            record.kind().as_task().map(|t| t.status().kind()),
            status_type,
            "record {index} status_type"
        );
        assert_eq!(record.depth(), depth, "record {index} depth");
        assert_eq!(record.line(), line_source(line), "record {index} line");
        assert_eq!(
            record.parent(),
            parent_line.and_then(SourceLine::new),
            "record {index} parent_line"
        );
    }

    // Task-only fields: present on the root task, absent on the plain bullet.
    let root = records.first().expect("root task record");
    assert_eq!(root.raw_text(), "Root task 📅 2025-06-01 🔺");
    assert_eq!(
        root.kind().as_task().and_then(|t| t.dates().due()).map(Into::into),
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

    let grandchild = records.get(2).expect("grandchild record");
    assert_eq!(
        grandchild.kind().as_task().map(TaskListItem::is_fully_complete),
        Some(true)
    );

    let plain = records.get(3).expect("plain bullet record");
    assert_eq!(plain.kind().as_task().and_then(|t| t.dates().due()), None);
    assert_eq!(plain.kind().as_task().and_then(TaskListItem::priority), None);
    assert_eq!(
        plain.kind().as_task().map(TaskListItem::is_fully_complete),
        None
    );
}

/// Proves redb persistence invariance without a `LISTS` table (ADR 0005):
/// builds an in-memory index, persists to redb (`NOTES` and `FILES` tables
/// only), then loads from a fresh `IndexerService` simulating a cold process
/// restart.
///
/// Asserts identical `QueryBuilder::lists` and `QueryBuilder::tasks` query
/// outcomes and complete list metadata without reparsing Markdown.
#[test]
fn preserves_query_outcomes_and_list_metadata_across_cold_reload() {
    let temp = tempfile::tempdir().expect("create temp dir");
    let project = TestProject::trusted(temp.path().join("project"));

    // Note 1: rich task outline with custom markers, priorities, and dates.
    let tasks_md = r"# Tasks

- [ ] Top task 📅 2026-06-01 🔺
  - [/] Subtask in-progress
  - [x] Subtask completed 📅 2026-06-15
- [-] Cancelled task
- [!] On-hold task [priority:: low]
- [?] Unknown marker task
- Plain bullet item
";

    // Note 2: outlines with frontmatter and hierarchical list items.
    let outlines_md = r"---
project: titan
owner: alice
---
# Outlines

- Root outline bullet
  - Child outline bullet
    - [ ] Nested action item #urgent
  - [x] Done checklist item
";

    project.write_note("tasks.md", tasks_md);
    project.write_note("outlines.md", outlines_md);

    // 1. Build and persist to disk (redb NOTES and FILES tables).
    let (_indexer, built) = project.build_and_persist();
    let built_arc = Arc::new(built);

    // 2. Evaluate queries against the in-memory index.
    let query_service = QueryService::new("class");
    let in_memory_lists =
        query_service.run(&built_arc, QueryBuilder::lists(SourceSelector::All));
    let in_memory_tasks =
        query_service.run(&built_arc, QueryBuilder::tasks(SourceSelector::All));

    // Verify expected baseline row counts:
    // tasks.md: 6 status items + 1 plain bullet = 7 list items.
    // outlines.md: 2 plain bullets + 1 task + 1 checklist = 4 list items.
    // Total lists = 11.
    // tasks.md has 6 tasks (no tag filter, so all status items are tasks).
    // outlines.md has 2 tasks.
    // Total tasks = 8.
    assert_eq!(in_memory_lists.len(), 11);
    assert_eq!(in_memory_tasks.len(), 8);

    // 3. Simulate cold restart: create a fresh IndexerService instance and
    //    load.
    // Also remove the original source Markdown files from disk before loading
    // to prove the reload reconstructs the full list and task domain
    // strictly from redb without reparsing Markdown files (ADR 0005).
    let tasks_path = project.root().join("tasks.md");
    let outlines_path = project.root().join("outlines.md");
    std::fs::remove_file(&tasks_path)
        .expect("remove tasks.md to prove no reparsing");
    std::fs::remove_file(&outlines_path)
        .expect("remove outlines.md to prove no reparsing");

    let fresh_indexer = project.indexer();
    let loaded = Arc::new(fresh_indexer.load().expect("load persisted index"));

    // 4. Evaluate identical queries against the cold-reloaded index.
    let loaded_lists =
        query_service.run(&loaded, QueryBuilder::lists(SourceSelector::All));
    let loaded_tasks =
        query_service.run(&loaded, QueryBuilder::tasks(SourceSelector::All));

    // 5. Assert complete query outcome invariance between in-memory and
    //    reloaded indices.
    assert_rows_match(&loaded_lists, &in_memory_lists, "list");
    assert_rows_match(&loaded_tasks, &in_memory_tasks, "task");

    // 6. Direct zero-allocation inspection of note list items on loaded notes:
    // Dates, priority, and fully_complete survived roundtrip intact.
    let loaded_tasks_note = loaded
        .entries()
        .iter()
        .find(|e| e.file().path().ends_with("tasks.md"))
        .and_then(FileEntry::note)
        .expect("loaded tasks note");
    let loaded_items = loaded_tasks_note.lists();

    let root_task = loaded_items.first().expect("root task");
    let root_t = root_task.kind().as_task().expect("root task kind");
    assert_eq!(
        root_t.dates().due().map(Into::into),
        NaiveDate::from_ymd_opt(2026, 6, 1)
    );
    assert_eq!(root_t.priority(), Some(TaskPriority::Highest));
    assert_eq!(root_t.is_fully_complete(), false);

    let completed_subtask = loaded_items.get(2).expect("completed subtask");
    let completed_t =
        completed_subtask.kind().as_task().expect("completed task kind");
    assert_eq!(
        completed_t.dates().due().map(Into::into),
        NaiveDate::from_ymd_opt(2026, 6, 15)
    );
    assert_eq!(completed_t.is_fully_complete(), true);

    let on_hold_task = loaded_items.get(4).expect("on hold task");
    let on_hold_t = on_hold_task.kind().as_task().expect("on hold task kind");
    assert_eq!(on_hold_t.priority(), Some(TaskPriority::Low));
    assert_eq!(on_hold_t.status().kind(), TaskStatusType::OnHold);
}

/// Builds the expected [`SourceLine`] for a persisted record.
fn line_source(line: u32) -> SourceLine {
    SourceLine::new(line).expect("non-zero")
}

/// Asserts every query row in `loaded` matches the corresponding row in
/// `in_memory`.
fn assert_rows_match(loaded: &QuerySet, in_memory: &QuerySet, label: &str) {
    assert_eq!(loaded.len(), in_memory.len(), "{label} count mismatch");
    for (i, (loaded_row, in_memory_row)) in
        loaded.iter().zip(in_memory.iter()).enumerate()
    {
        assert_eq!(
            (
                loaded_row.file().path(),
                loaded_row.depth(),
                loaded_row.line(),
                loaded_row.parent(),
            ),
            (
                in_memory_row.file().path(),
                in_memory_row.depth(),
                in_memory_row.line(),
                in_memory_row.parent(),
            ),
            "{label} row {i} structural mismatch"
        );
        assert_eq!(
            (
                loaded_row.task_text(),
                loaded_row.task_completed(),
                loaded_row.status_symbol(),
            ),
            (
                in_memory_row.task_text(),
                in_memory_row.task_completed(),
                in_memory_row.status_symbol(),
            ),
            "{label} row {i} task data mismatch"
        );
    }
}

/// Proves `IndexerService::refresh_with_report` recovers from a corrupted
/// `.traces/index.redb` file and reports every project file as upserted,
/// with nothing deleted, through the public API alone.
///
/// Promoted from `src/index/service.rs`'s internal unit test: corrupting a
/// real on-disk redb file and recovering through it is exactly the
/// filesystem/database boundary this module exists to cover, not a private
/// invariant, per `rust-integration-testing`'s boundary strategy.
#[test]
fn refresh_after_corruption_recovery_reports_every_file_upserted_and_nothing_deleted()
 {
    let temp = tempfile::tempdir().expect("create temp dir");
    let project = TestProject::trusted(temp.path().join("project"));
    project.write_note("a.md", "# A\n");
    project.write_note("b.md", "# B\n");
    let (indexer, built) = project.build_and_persist();
    assert_eq!(built.entries().len(), 2);

    let db_path = project.root().join(".traces/index.redb");
    let mut corrupted = std::fs::read(&db_path).expect("read valid db");
    corrupted
        .get_mut(9..)
        .expect("db file longer than the 9-byte magic number")
        .fill(0xFF);
    std::fs::write(&db_path, &corrupted).expect("corrupt the database file");

    let (_, report) = indexer
        .refresh_with_report()
        .expect("refresh recovers from corruption");

    assert_eq!(report.upserted(), 2);
    assert_eq!(report.deleted(), 0);
}
