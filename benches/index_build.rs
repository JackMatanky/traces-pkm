//! Benches `traces_pkm::FileIndex::build`, the full-scan indexing path run on
//! `traces index` and any first-time `traces list`/`table`/`task`/`template`
//! call. Scaling across note counts shows how indexing cost grows with project
//! size.
//!
//! Run via `mise run bench`, not bare `cargo bench`: this crate's
//! `test-utils`-gated public surface (`FileIndex` included) is only reachable
//! with `--features test-utils`, which the mise task supplies.
#![expect(
    clippy::expect_used,
    reason = "bench fixture/harness code; a failed .expect() here means the \
              fixture itself is broken and should panic immediately"
)]
use criterion::{
    BatchSize, BenchmarkId, Criterion, criterion_group, criterion_main,
};
use traces_pkm::FileIndex;

fn populate(n: usize) -> std::path::PathBuf {
    let temp = tempfile::tempdir().expect("create temp dir");
    for i in 0..n {
        std::fs::write(
            temp.path().join(format!("note-{i}.md")),
            format!(
                "---\nrating: {}\n---\n\nBody text for note {i}.\n",
                i % 10
            ),
        )
        .expect("write fixture note");
    }
    let root = temp.path().to_path_buf();
    // `keep()` intentionally leaks the directory instead of deleting it on
    // drop: `iter_batched`'s `routine` closure below takes ownership of this
    // return value, so an ordinary `TempDir` drop there would count toward the
    // *measured* time and pollute this benchmark with unrelated filesystem
    // cleanup cost.
    let _ = temp.keep();
    root
}

fn bench_file_index_build(c: &mut Criterion) {
    let mut group = c.benchmark_group("FileIndex::build");
    for n in [10_usize, 100, 1000] {
        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, &n| {
            b.iter_batched(
                || populate(n),
                |root| FileIndex::build(&root).expect("build index"),
                BatchSize::LargeInput,
            );
        });
    }
    group.finish();
}

criterion_group!(benches, bench_file_index_build);
criterion_main!(benches);
