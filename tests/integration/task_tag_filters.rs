//! Integration tests for task tag filter configuration and classification.
//!
//! Proves that configured task tag filters correctly classify status-marked
//! Markdown list items as Tasks vs Checkboxes across real files, config
//! resolution, indexing, and query execution through the public service
//! surface.

use std::{fs, sync::Arc};

use pretty_assertions::assert_eq;
use traces_pkm::{
    Config, IndexerService, ListItemType, QueryBuilder, QueryService,
    SourceSelector, Tag, TaskConfig,
};

#[test]
fn config_with_tag_filters_classifies_tasks_and_checkboxes_correctly() {
    let temp = tempfile::tempdir().expect("create temp dir");
    let root = temp.path().join("project");
    fs::create_dir_all(&root).expect("create project dir");

    let markdown = r"# Tasks and Checklists

- [ ] Buy groceries #task
- [x] Write report #todo
- [ ] Read book #personal
- [ ] Plain checkbox without tag
- [ ] Nested tag item #task/urgent
- Plain bullet with #task
";
    fs::write(root.join("notes.md"), markdown).expect("write notes.md");

    let tag_filters = vec![
        Tag::parse("#task").expect("valid tag"),
        Tag::parse("#todo").expect("valid tag"),
    ];
    let tasks_config = TaskConfig::for_test(tag_filters);
    let config = Config::for_test(root.clone(), None, None, root.clone())
        .with_tasks(tasks_config);

    let index = Arc::new(
        IndexerService::new(&root)
            .with_config(&config)
            .build()
            .expect("build index"),
    );

    let query_service = QueryService::new("class");
    let task_rows =
        query_service.run(&index, QueryBuilder::tasks(SourceSelector::All));

    assert_eq!(task_rows.len(), 2);
    let task_texts: Vec<&str> = (&task_rows)
        .into_iter()
        .map(|row| row.task_text().unwrap_or_default())
        .collect();
    assert_eq!(task_texts, ["Buy groceries #task", "Write report #todo"]);

    let note_entry = index.entries().first().expect("entry present");
    let note = note_entry.note().expect("note present");
    let list = note.lists().first().expect("list present");
    let items = list.items();
    assert_eq!(items.len(), 6);
    assert_eq!(items.first().expect("item 0").text(), "Buy groceries #task");
    assert!(items.first().expect("item 0").kind().is_task());
    assert_eq!(items.get(1).expect("item 1").text(), "Write report #todo");
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
    let root = temp.path().join("project");
    fs::create_dir_all(&root).expect("create project dir");

    let markdown = r"# Simple Tasks

- [ ] First item
- [x] Second item #other
- [?] Unknown marker item
- Plain bullet
";
    fs::write(root.join("notes.md"), markdown).expect("write notes.md");

    let index =
        Arc::new(IndexerService::new(&root).build().expect("build index"));

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
