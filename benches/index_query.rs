//! Benches `traces_pkm::QueryRecordSet::{filter, sort}` over a pre-built
//! 1000-record index, the transformation chain every `traces
//! list`/`table`/`task` command and every template `query`/`tasks` call runs.
//!
//! Run via `mise run bench`, not bare `cargo bench`: this crate's
//! `test-utils`-gated public surface (`FileIndex`, `QueryOutcome`) is only
//! reachable with `--features test-utils`, which the mise task supplies.
#![expect(
    clippy::expect_used,
    reason = "bench fixture/harness code; a failed .expect() here means the \
              fixture itself is broken and should panic immediately"
)]
use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use traces_pkm::{FileIndex, IndexerService, QueryService, QuerySource};

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

/// Filters a pre-built 1000-record index on `rating >= 50`.
///
/// The transformation every `--where` and template `.filter()` call runs (see
/// module docs); a correctness test would pass regardless of a silent slowdown
/// here.
fn bench_filter(c: &mut Criterion) {
    c.bench_function("QueryRecordSet::filter", |b| {
        b.iter_batched(
            built_index,
            |index| {
                let (records, notes, inlinks) = index.into_parts();
                QueryService::new("class")
                    .query(records, notes, inlinks, &QuerySource::All)
                    .filter("rating >= 50")
                    .expect("valid filter expression")
            },
            BatchSize::LargeInput,
        );
    });
}

/// Expands a pre-built 1000-note index with three tasks per Note.
///
/// This captures task-row construction independently from filesystem indexing.
fn bench_query_tasks(c: &mut Criterion) {
    c.bench_function("QueryService::query_tasks", |b| {
        b.iter_batched(
            built_task_index,
            |index| {
                let (records, notes, inlinks) = index.into_parts();
                QueryService::new("class").query_tasks(
                    records,
                    notes,
                    inlinks,
                    &QuerySource::All,
                )
            },
            BatchSize::LargeInput,
        );
    });
}

/// Sorts a pre-built 1000-record index on `rating` descending.
///
/// Same reasoning as `bench_filter` above, for `--sort`/template `.sort()`.
fn bench_sort(c: &mut Criterion) {
    c.bench_function("QueryRecordSet::sort", |b| {
        b.iter_batched(
            built_index,
            |index| {
                let (records, notes, inlinks) = index.into_parts();
                QueryService::new("class")
                    .query(records, notes, inlinks, &QuerySource::All)
                    .sort("rating", false)
                    .expect("valid sort field")
            },
            BatchSize::LargeInput,
        );
    });
}

criterion_group!(benches, bench_filter, bench_query_tasks, bench_sort);
criterion_main!(benches);
