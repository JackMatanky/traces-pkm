//! Benches `traces_pkm::QueryOutcome::{filter, sort}` over a pre-built
//! 1000-record index, the transformation chain every `traces list`/`table`/
//! `task` command and every template `query`/`tasks` call runs.
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
use traces_pkm::{FileIndex, QuerySource};

fn built_index() -> FileIndex {
    let temp = tempfile::tempdir().expect("create temp dir");
    for i in 0..1000 {
        std::fs::write(
            temp.path().join(format!("note-{i}.md")),
            format!("---\nrating: {}\n---\n", i % 100),
        )
        .expect("write fixture note");
    }
    FileIndex::build(temp.path()).expect("build index")
}

fn bench_filter(c: &mut Criterion) {
    c.bench_function("QueryOutcome::filter", |b| {
        b.iter_batched(
            built_index,
            |index| {
                index
                    .query(&QuerySource::All)
                    .filter("rating >= 50")
                    .expect("valid filter expression")
            },
            BatchSize::LargeInput,
        );
    });
}

fn bench_sort(c: &mut Criterion) {
    c.bench_function("QueryOutcome::sort", |b| {
        b.iter_batched(
            built_index,
            |index| {
                index
                    .query(&QuerySource::All)
                    .sort("rating", false)
                    .expect("valid sort field")
            },
            BatchSize::LargeInput,
        );
    });
}

criterion_group!(benches, bench_filter, bench_sort);
criterion_main!(benches);
