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
//! cargo flamegraph --bench query_execution -- --bench "QueryService::execute pages"
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
use std::sync::Arc;

use criterion::{
    BatchSize, BenchmarkId, Criterion, Throughput, criterion_group,
    criterion_main,
};
use traces_pkm::{
    FileIndex, IndexerService, QueryRequest, QueryService, SourceSelector,
};

fn built_index(n: usize) -> Arc<FileIndex> {
    let temp = tempfile::tempdir().expect("create temp dir");
    for i in 0..n {
        std::fs::write(
            temp.path().join(format!("note-{i}.md")),
            format!("---\nrating: {}\n---\n", i % 100),
        )
        .expect("write fixture note");
    }
    Arc::new(IndexerService::new(temp.path()).build().expect("build index"))
}

fn built_task_index(n: usize) -> Arc<FileIndex> {
    let temp = tempfile::tempdir().expect("create temp dir");
    for i in 0..n {
        std::fs::write(
            temp.path().join(format!("note-{i}.md")),
            "- [ ] first\n- [x] second\n- [ ] third\n",
        )
        .expect("write fixture note");
    }
    Arc::new(IndexerService::new(temp.path()).build().expect("build index"))
}

/// Measures page-row construction and filtering, swept over workspace size.
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
    let mut group = c.benchmark_group("QueryService::execute");
    for n in [100_usize, 1_000, 20_000] {
        let index = built_index(n);
        group.throughput(Throughput::Elements(
            u64::try_from(n).expect("note count fits u64"),
        ));
        group.bench_with_input(BenchmarkId::new("pages", n), &n, |b, _| {
            b.iter_batched(
                || index.clone(),
                |index| {
                    QueryService::new("class").execute(
                        &index,
                        QueryRequest::pages(SourceSelector::All),
                    )
                },
                BatchSize::SmallInput,
            );
        });
    }
    group.finish();
}

/// Measures task-row construction, swept over workspace size, with three
/// tasks per note.
///
/// This captures task-row parsing and materialization independently from
/// filesystem indexing, isolating the cost of checkbox extraction and task
/// metadata assembly.
///
/// Expected outcomes:
/// - Task queries scale proportionally to total task count.
///
/// Unexpected outcomes:
/// - Cost significantly exceeds page-query cost for same note count, indicating
///   task parsing overhead or redundant regex evaluation.
fn bench_execute_tasks(c: &mut Criterion) {
    let mut group = c.benchmark_group("QueryService::execute");
    for n in [100_usize, 1_000, 20_000] {
        let index = built_task_index(n);
        group.throughput(Throughput::Elements(
            u64::try_from(n).expect("note count fits u64").saturating_mul(3),
        ));
        group.bench_with_input(BenchmarkId::new("tasks", n), &n, |b, _| {
            b.iter_batched(
                || index.clone(),
                |index| {
                    QueryService::new("class").execute(
                        &index,
                        QueryRequest::tasks(SourceSelector::All),
                    )
                },
                BatchSize::SmallInput,
            );
        });
    }
    group.finish();
}

/// Measures page-row filtering and sorting by frontmatter metadata, swept over
/// workspace size — the path `FileIndexRow::note()` resolution sits on.
///
/// Distinct from [`bench_execute_pages`], which never touches a
/// `FieldPath::Metadata`/`Tags` field and would not catch regressions in
/// per-record Note metadata resolution.
///
/// Expected outcomes:
/// - Fast O(1) field lookup per record without repeated binary searches.
fn bench_execute_pages_by_metadata(c: &mut Criterion) {
    let mut group = c.benchmark_group("QueryService::execute");
    for n in [100_usize, 1_000, 20_000] {
        let index = built_index(n);
        group.throughput(Throughput::Elements(
            u64::try_from(n).expect("note count fits u64"),
        ));
        group.bench_with_input(
            BenchmarkId::new("pages filter+sort by metadata", n),
            &n,
            |b, _| {
                b.iter_batched(
                    || index.clone(),
                    |index| {
                        QueryService::new("class").execute(
                            &index,
                            QueryRequest::pages(SourceSelector::All)
                                .filter("rating > 2")
                                .expect("valid filter")
                                .sort("rating", false)
                                .expect("valid sort"),
                        )
                    },
                    BatchSize::SmallInput,
                );
            },
        );
    }
    group.finish();
}

criterion_group!(
    benches,
    bench_execute_pages,
    bench_execute_tasks,
    bench_execute_pages_by_metadata
);
criterion_main!(benches);
