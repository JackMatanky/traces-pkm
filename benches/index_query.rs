//! Benches `QueryService::execute` over pre-built page and task indexes.
//!
//! Run via `mise run bench`, not bare `cargo bench`: this crate's
//! `test-utils`-gated public surface is only reachable with
//! `--features test-utils`, which the mise task supplies.
#![expect(
    clippy::expect_used,
    reason = "bench fixture/harness code; a failed .expect() here means the \
              fixture itself is broken and should panic immediately"
)]
use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use traces_pkm::{
    FileIndex, IndexerService, QueryRequest, QueryService, SourceSelector,
};

fn built_index() -> FileIndex {
    let temp = tempfile::tempdir().expect("create temp dir");
    for i in 0..1000 {
        std::fs::write(
            temp.path().join(format!("note-{i}.md")),
            format!("---\nrating: {}\n---\n", i % 100),
        )
        .expect("write fixture note");
    }
    IndexerService::new(temp.path()).build().expect("build index")
}

fn built_task_index() -> FileIndex {
    let temp = tempfile::tempdir().expect("create temp dir");
    for i in 0..1000 {
        std::fs::write(
            temp.path().join(format!("note-{i}.md")),
            "- [ ] first\n- [x] second\n- [ ] third\n",
        )
        .expect("write fixture note");
    }
    IndexerService::new(temp.path()).build().expect("build index")
}

/// Produces page rows from a pre-built 1000-record index.
fn bench_execute_pages(c: &mut Criterion) {
    c.bench_function("QueryService::execute pages", |b| {
        b.iter_batched(
            built_index,
            |index| {
                QueryService::new("class")
                    .execute(&index, QueryRequest::pages(SourceSelector::All))
            },
            BatchSize::LargeInput,
        );
    });
}

/// Expands a pre-built 1000-note index with three tasks per Note.
///
/// This captures task-row construction independently from filesystem indexing.
fn bench_execute_tasks(c: &mut Criterion) {
    c.bench_function("QueryService::execute tasks", |b| {
        b.iter_batched(
            built_task_index,
            |index| {
                QueryService::new("class")
                    .execute(&index, QueryRequest::tasks(SourceSelector::All))
            },
            BatchSize::LargeInput,
        );
    });
}

criterion_group!(benches, bench_execute_pages, bench_execute_tasks);
criterion_main!(benches);
