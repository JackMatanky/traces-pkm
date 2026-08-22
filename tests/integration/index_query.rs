//! Proves `FileIndex::build` → `QueryRequest` execution works across real
//! files through the test-utils surface alone. Unit coverage inside
//! `src/query/` exercises crate-internal transforms.

use std::{fs, path::Path};

use pretty_assertions::assert_eq;
use traces_pkm::{IndexerService, QueryRequest, QueryService, QuerySource};

/// Checks a page request returns every indexed note without consuming the
/// borrowed index.
#[test]
fn page_query_returns_real_indexed_notes() {
    let temp = tempfile::tempdir().expect("create temp dir");
    fs::write(temp.path().join("a.md"), "---\nrating: 3\n---\n")
        .expect("write a.md");
    fs::write(temp.path().join("b.md"), "---\nrating: 9\n---\n")
        .expect("write b.md");
    fs::write(temp.path().join("c.md"), "---\nrating: 5\n---\n")
        .expect("write c.md");
    let index = IndexerService::new(temp.path()).build().expect("build index");
    let outcome = QueryService::new("class")
        .execute(&index, QueryRequest::pages(QuerySource::All));

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

/// Checks task queries flatten two tasks in one note into two rows, each with
/// the correct completion state.
///
/// Proves `QueryRecord::task_completed` works from outside the crate.
#[test]
fn query_tasks_returns_task_level_rows_distinct_from_page_level_query() {
    let temp = tempfile::tempdir().expect("create temp dir");
    fs::write(temp.path().join("todo.md"), "- [ ] one\n- [x] two\n")
        .expect("write todo.md");
    let index = IndexerService::new(temp.path()).build().expect("build index");
    let tasks = QueryService::new("class")
        .execute(&index, QueryRequest::tasks(QuerySource::All));
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
fn query_request_reuses_one_index_for_page_and_task_queries() {
    let temp = tempfile::tempdir().expect("create temp dir");
    fs::write(
        temp.path().join("book.md"),
        "---\nrating: 9\n---\n#book [[todo]]\n- [ ] read chapter\n",
    )
    .expect("write book.md");
    fs::write(temp.path().join("todo.md"), "---\nrating: 1\n---\n")
        .expect("write todo.md");
    let index = IndexerService::new(temp.path()).build().expect("build index");
    let service = QueryService::new("class");

    let pages = service.execute(&index, QueryRequest::pages(QuerySource::All));
    let tasks = service.execute(&index, QueryRequest::tasks(QuerySource::All));
    let pages_again =
        service.execute(&index, QueryRequest::pages(QuerySource::All));

    let page_paths: Vec<_> = (&pages)
        .into_iter()
        .map(|row| row.file().path().to_path_buf())
        .collect();
    assert_eq!(page_paths, [Path::new("book.md"), Path::new("todo.md")]);
    assert_eq!(tasks.len(), 1);
    assert_eq!(pages_again.len(), 2);
}
