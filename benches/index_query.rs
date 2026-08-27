//! Performance benchmark suite for query execution.
//!
//! Exposes and monitors the CPU cost of [`QueryService::execute`] over
//! pre-built page and task indexes. Queries are the primary user-facing latency
//! path — every `traces query` invocation pays this cost — so regressions here
//! directly degrade CLI responsiveness.
//!
//! ### Data Flow Diagram
//!
//! ```text
//! [FileIndex] ──(QueryService::execute)──► [QueryResponse]
//!                   │
//!                   ├── pages (SourceSelector)
//!                   └── tasks (SourceSelector)
//! ```
//!
//! ### Profiling Integration
//!
//! To profile query execution CPU bottlenecks:
//! ```bash
//! cargo flamegraph --bench index_query -- --bench "QueryService::execute pages"
//! ```
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

/// Measures page-row construction and filtering from a 1000-record index.
///
/// Every `traces query` invocation pays this path (see module docs); isolating
/// page queries catches regressions in filter logic or row materialization that
/// a correctness test would miss.
///
/// Expected outcomes:
/// - Constant-time execution regardless of index size (all notes match).
///
/// Unexpected outcomes:
/// - Linear or worse scaling with note count, indicating unindexed scans or
///   redundant allocation per row.
fn bench_execute_pages(c: &mut Criterion) {
    let index = built_index();
    c.bench_function("QueryService::execute pages", |b| {
        b.iter_batched(
            || index.clone(),
            |index| {
                QueryService::new("class")
                    .execute(&index, QueryRequest::pages(SourceSelector::All))
            },
            BatchSize::SmallInput,
        );
    });
}

/// Measures task-row construction from a 1000-note index with three tasks
/// per note.
///
/// This captures task-row parsing and materialization independently from
/// filesystem indexing, isolating the cost of checkbox extraction and task
/// metadata assembly.
///
/// Expected outcomes:
/// - Task queries scale proportionally to total task count (3000 tasks).
///
/// Unexpected outcomes:
/// - Cost significantly exceeds page-query cost for same note count, indicating
///   task parsing overhead or redundant regex evaluation.
fn bench_execute_tasks(c: &mut Criterion) {
    let index = built_task_index();
    c.bench_function("QueryService::execute tasks", |b| {
        b.iter_batched(
            || index.clone(),
            |index| {
                QueryService::new("class")
                    .execute(&index, QueryRequest::tasks(SourceSelector::All))
            },
            BatchSize::SmallInput,
        );
    });
}

criterion_group!(benches, bench_execute_pages, bench_execute_tasks);
criterion_main!(benches);
