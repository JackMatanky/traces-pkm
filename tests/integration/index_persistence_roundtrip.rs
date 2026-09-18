//! Promotes `src/index/mod.rs`'s internal
//! `persist_then_load_recovers_the_same_records_and_notes` test to prove the
//! public round-trip contract: `FileIndex::build` → `persist` → `load` in a
//! fresh `FileIndex` value, simulating a new process.

use chrono::NaiveDate;
use pretty_assertions::assert_eq;
use traces_pkm::{
    FileEntry, SourceLine, TaskListItem, TaskPriority, TaskStatusType,
    TestProject,
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
    let (_, _) = project.persist_index();
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

/// Builds the expected [`SourceLine`] for a persisted record.
fn line_source(line: u32) -> SourceLine {
    SourceLine::new(line).expect("non-zero")
}
