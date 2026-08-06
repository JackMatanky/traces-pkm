//! Proves `FileIndex::build` → `query`/`query_tasks` → `QueryOutcome::{filter,
//! sort, limit}` compose correctly across real files, through the public
//! surface alone. The existing `#[cfg(test)]` coverage inside
//! `src/index/query.rs` tests the same semantics through crate-internal
//! helpers, so it can't catch a regression in the *external* contract these
//! methods expose now that they're `pub`.

use std::{fs, path::Path};

use pretty_assertions::assert_eq;
use traces_pkm::{FileIndex, QuerySource};

#[test]
fn query_then_filter_then_sort_then_limit_composes_across_the_public_surface() {
    let temp = tempfile::tempdir().expect("create temp dir");
    fs::write(temp.path().join("a.md"), "---\nrating: 3\n---\n")
        .expect("write a.md");
    fs::write(temp.path().join("b.md"), "---\nrating: 9\n---\n")
        .expect("write b.md");
    fs::write(temp.path().join("c.md"), "---\nrating: 5\n---\n")
        .expect("write c.md");
    let index = FileIndex::build(temp.path()).expect("build index");

    let outcome = index
        .query(&QuerySource::All)
        .filter("rating >= 5")
        .expect("valid filter expression")
        .sort("rating", true)
        .expect("valid sort field")
        .limit(1)
        .expect("non-negative limit");

    assert_eq!(outcome.len(), 1);
    let top = outcome.get(0).expect("one record");
    assert_eq!(top.file().path(), Path::new("b.md"));
}

#[test]
fn query_tasks_returns_task_level_rows_distinct_from_page_level_query() {
    let temp = tempfile::tempdir().expect("create temp dir");
    fs::write(temp.path().join("todo.md"), "- [ ] one\n- [x] two\n")
        .expect("write todo.md");
    let index = FileIndex::build(temp.path()).expect("build index");

    let tasks = index.query_tasks(&QuerySource::All);
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
