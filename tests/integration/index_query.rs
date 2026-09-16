//! Proves `FileIndex::build` → `QueryBuilder` execution works across real files
//! through the test-utils surface alone. Unit coverage inside `src/query/`
//! exercises crate-internal transforms.

use std::{path::Path, sync::Arc};

use pretty_assertions::assert_eq;
use traces_pkm::{QueryBuilder, QueryService, SourceSelector, TestProject};

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
    let outcome = QueryService::new("class")
        .run(&index, QueryBuilder::pages(SourceSelector::All));

    assert_eq!(outcome.len(), 3);
    let paths: Vec<_> = (&outcome)
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
