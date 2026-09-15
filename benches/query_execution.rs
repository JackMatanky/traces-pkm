//! Performance benchmark suite for query execution.
//!
//! Exposes and monitors the CPU cost of [`QueryService::run`] over pre-built
//! page and task indexes.
//!
//! ### Data Flow Diagram
//!
//! ```text
//! [FileIndex] ──(QueryService::run)──► [QueryResponse]
//!                   │
//!                   ├── pages (SourceSelector)
//!                   └── tasks (SourceSelector)
//! ```
//!
//! ### Profiling Integration
//!
//! To profile query execution CPU bottlenecks:
//! ```bash
//! cargo flamegraph --bench query_execution -- --bench "QueryService::run pages"
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
use std::hint::black_box;

use criterion::{
    AxisScale, BatchSize, BenchmarkId, Criterion, PlotConfiguration,
    Throughput, criterion_group, criterion_main,
};
use traces_pkm::{QueryBuilder, QueryService, SourceSelector};

#[allow(
    dead_code,
    reason = "shared benchmark common helpers are compiled into each bench \
              target; this target uses only query index fixtures"
)]
mod common;

use common::{
    WORKSPACE_FILE_COUNTS,
    content::{
        ProjectShape, metadata_lookup_note_source, task_triplet_note_source,
    },
    project::{
        build_index_arc, build_index_arc_from_note_source,
        setup_persisted_project,
    },
    quick_file_counts,
};

// ----------------------------------------------------------- //
//                     Fixtures & Helpers                      //
// ----------------------------------------------------------- //

const QUERY_METADATA_FIELD_COUNTS: &[usize] = &[1, 5, 10, 20];

// ----------------------------------------------------------- //
//                Benchmarks: General Execution                //
// ----------------------------------------------------------- //

/// Measures unfiltered page-row selection/materialization over plain in-memory
/// indexes.
///
/// Parameters: varies [`WORKSPACE_FILE_COUNTS`]; reports page-row throughput.
///
/// Fixture: [`ProjectShape::Plain`] indexes are built outside timing through
/// `FileIndex::new_test`. Timed work runs `QueryService::run(All pages)`.
///
/// Expected outcomes:
/// - Cost scales linearly with indexed entries.
///
/// Unexpected outcomes:
/// - Super-linear scaling with note count, indicating row materialization or
///   source iteration does redundant work.
fn bench_run_pages(c: &mut Criterion) {
    let mut group = c.benchmark_group("QueryService::run");
    group.plot_config(
        PlotConfiguration::default().summary_scale(AxisScale::Logarithmic),
    );
    for &n in WORKSPACE_FILE_COUNTS {
        let index = build_index_arc(n, ProjectShape::Plain);
        group.throughput(Throughput::Elements(
            u64::try_from(n).expect("note count fits u64"),
        ));
        group.bench_with_input(BenchmarkId::new("pages", n), &n, |b, _| {
            b.iter_batched(
                || index.clone(),
                |index| {
                    QueryService::new("class")
                        .run(&index, QueryBuilder::pages(SourceSelector::All))
                },
                BatchSize::SmallInput,
            );
        });
    }
    group.finish();
}

/// Measures pre-parsed task-row expansion/materialization over in-memory
/// indexes, with three tasks per note.
///
/// Parameters: varies [`WORKSPACE_FILE_COUNTS`]; reports task-row throughput
/// (`3 * n` rows). Fixture parsing happens outside timing through
/// `task_triplet_note_source` and `FileIndex::new_test`.
///
/// Expected outcomes:
/// - Task queries scale proportionally to output task rows and stay comparable
///   per output row to [`bench_run_pages`].
///
/// Unexpected outcomes:
/// - Per-task-row cost significantly exceeds page-row cost, indicating task-row
///   field resolution or row assembly does extra per-task work.
fn bench_run_tasks(c: &mut Criterion) {
    let mut group = c.benchmark_group("QueryService::run");
    group.plot_config(
        PlotConfiguration::default().summary_scale(AxisScale::Logarithmic),
    );
    for &n in WORKSPACE_FILE_COUNTS {
        let index = build_index_arc_from_note_source(n, |i, _| {
            task_triplet_note_source(i)
        });
        group.throughput(Throughput::Elements(
            u64::try_from(n).expect("note count fits u64").saturating_mul(3),
        ));
        group.bench_with_input(BenchmarkId::new("tasks", n), &n, |b, _| {
            b.iter_batched(
                || index.clone(),
                |index| {
                    QueryService::new("class")
                        .run(&index, QueryBuilder::tasks(SourceSelector::All))
                },
                BatchSize::SmallInput,
            );
        });
    }
    group.finish();
}

// ----------------------------------------------------------- //
//              Benchmarks: Combined Filter+Sort               //
// ----------------------------------------------------------- //
/// Measures page-row filtering and sorting by frontmatter metadata combined,
/// swept over workspace size.
///
/// Parameters: varies [`WORKSPACE_FILE_COUNTS`]; reports input note throughput.
///
/// Fixture: [`ProjectShape::Plain`] in-memory indexes are built outside timing.
/// Timed work builds and runs `All pages -> filter("rating > 2") ->
/// sort("rating")`.
///
/// This group covers the combined end-to-end path only. The filter-width and
/// sort-only benchmarks are diagnostic context, but they are not a controlled
/// subtraction because their fixtures and setup boundaries differ.
///
/// Expected outcomes:
/// - Cost scales with row construction, filtered-row count, and full-sort work
///   for the plain fixture.
///
/// Unexpected outcomes:
/// - Cost grows beyond the filter-plus-sort shape for this fixture, indicating
///   combined-path overhead in query construction, filtering, key extraction,
///   or sorting.
fn bench_run_pages_by_metadata(c: &mut Criterion) {
    let mut group = c.benchmark_group("QueryService::run");
    group.plot_config(
        PlotConfiguration::default().summary_scale(AxisScale::Logarithmic),
    );
    for &n in WORKSPACE_FILE_COUNTS {
        if n >= 10_000 {
            group.sample_size(10);
        }
        let index = build_index_arc(n, ProjectShape::Plain);
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
                        QueryService::new("class").run(
                            &index,
                            QueryBuilder::pages(SourceSelector::All)
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

/// Measures [`QueryService::sync_and_run`] latency across source-selector
/// selectivity over a persisted redb store.
///
/// Parameters: varies selector (`single_tag_point_lookup` vs.
/// `full_vault_scan`) and note count; throughput is indexed entries traversed
/// during sync, not output rows for the one-tag query.
///
/// Fixture: persisted tagged project is built outside timing. Each timed call
/// runs the real persisted command path: sync/scan first, then store-backed
/// query reads and row materialization.
///
/// Expected outcomes:
/// - Both shapes include the shared O(n) sync prelude; after that,
///   `single_tag_point_lookup` reads/materializes one matching note and
///   `full_vault_scan` reads/materializes all notes.
///
/// Unexpected outcomes:
/// - The one-tag path widens beyond the shared sync baseline as `n` grows,
///   indicating `PATHS_BY_TAG` candidate lookup, store reads, or row
///   materialization has regressed toward full-vault behavior.
fn bench_sync_and_run_selectors(c: &mut Criterion) {
    let mut group = c.benchmark_group("QueryService::sync_and_run_selectors");
    group.plot_config(
        PlotConfiguration::default().summary_scale(AxisScale::Logarithmic),
    );
    let service = QueryService::new("class");
    let single_tag =
        || SourceSelector::parse("#rare_0").expect("valid tag selector");
    let all_pages = || SourceSelector::All;

    for n in quick_file_counts() {
        let (_temp, indexer) = setup_persisted_project(n, ProjectShape::Tagged);
        group.throughput(Throughput::Elements(
            u64::try_from(n).expect("note count fits u64"),
        ));
        group.bench_with_input(
            BenchmarkId::new("single_tag_point_lookup", n),
            &n,
            |b, _| {
                b.iter_batched(
                    || QueryBuilder::pages(single_tag()),
                    |query| {
                        black_box(
                            service
                                .sync_and_run(&indexer, query)
                                .expect("sync_and_run succeeds"),
                        )
                    },
                    BatchSize::SmallInput,
                );
            },
        );
        group.bench_with_input(
            BenchmarkId::new("full_vault_scan", n),
            &n,
            |b, _| {
                b.iter_batched(
                    || QueryBuilder::pages(all_pages()),
                    |query| {
                        black_box(
                            service
                                .sync_and_run(&indexer, query)
                                .expect("sync_and_run succeeds"),
                        )
                    },
                    BatchSize::SmallInput,
                );
            },
        );
    }
    group.finish();
}

// ----------------------------------------------------------- //
//                 Benchmarks: Isolated Filter                 //
// ----------------------------------------------------------- //
/// Measures end-to-end filter cost by frontmatter field count at a fixed
/// 20,000-note workspace size.
///
/// Parameters: varies metadata field count over `{1, 5, 10, 20}`; reports
/// 20,000 input rows per iteration. Fixture indexes are built outside timing by
/// `metadata_lookup_note_source`, which places `rating` after the synthetic
/// fields so accidental linear metadata lookup is visible.
///
/// Timed work parses and runs the fixed `rating > 2` filter; it also constructs
/// page rows, so this is not a pure lookup microbenchmark.
///
/// Expected outcomes:
/// - Cost remains broadly flat across field counts because query field paths
///   use canonical hash-keyed lookup.
///
/// Unexpected outcomes:
/// - Cost grows with field count, signaling metadata lookup/key-resolution or
///   field-width-sensitive row work needs inspection.
fn bench_filter_by_metadata_field_count(c: &mut Criterion) {
    let mut group =
        c.benchmark_group("QueryService::run/filter_by_field_count");
    let n = 20_000_usize;
    for &fields in QUERY_METADATA_FIELD_COUNTS {
        let index = build_index_arc_from_note_source(n, |i, _| {
            metadata_lookup_note_source(i, fields)
        });
        group.throughput(Throughput::Elements(
            u64::try_from(n).expect("note count fits u64"),
        ));
        group.bench_with_input(
            BenchmarkId::new("fields", fields),
            &fields,
            |b, _| {
                b.iter_batched(
                    || index.clone(),
                    |index| {
                        QueryService::new("class").run(
                            &index,
                            QueryBuilder::pages(SourceSelector::All)
                                .filter("rating > 2")
                                .expect("valid filter"),
                        )
                    },
                    BatchSize::SmallInput,
                );
            },
        );
    }
    group.finish();
}

// ----------------------------------------------------------- //
//             Benchmarks: Template Chain Overhead             //
// ----------------------------------------------------------- //
/// Measures one `QuerySet::clone()` over result sets of varying source size.
///
/// Parameters: varies [`WORKSPACE_FILE_COUNTS`]; `n` is a size-sensitivity
/// probe, not processed work inside the timed clone.
///
/// Fixture: a plain in-memory index and unmaterialized no-transform
/// [`QuerySet`] are built outside timing.
///
/// `QuerySet::base` is `Arc<Vec<QueryRow>>`, so cloning should copy the `Arc`
/// and empty plan state, not row data.
///
/// Expected outcomes:
/// - Clone time is small and roughly constant across workspace sizes.
///
/// Unexpected outcomes:
/// - Clone time scales with `n`, indicating `base` is no longer `Arc`-backed or
///   clone materializes/deep-copies rows.
fn bench_clone_query_set(c: &mut Criterion) {
    let mut group = c.benchmark_group("QueryService::run/clone_query_set");
    group.plot_config(
        PlotConfiguration::default().summary_scale(AxisScale::Logarithmic),
    );
    for &n in WORKSPACE_FILE_COUNTS {
        let index = build_index_arc(n, ProjectShape::Plain);
        let outcome = QueryService::new("class")
            .run(&index, QueryBuilder::pages(SourceSelector::All));
        group.throughput(Throughput::Elements(
            u64::try_from(n).expect("note count fits u64"),
        ));
        group.bench_with_input(
            BenchmarkId::from_parameter(n),
            &outcome,
            |b, outcome| {
                b.iter(|| black_box(outcome.clone()));
            },
        );
    }
    group.finish();
}

/// Measures `QuerySet`'s owned `IntoIterator::into_iter()` sole-owner path,
/// swept over workspace size and row shape.
///
/// Parameters: varies [`WORKSPACE_FILE_COUNTS`] and shape (`pages`, `tasks`);
/// reports output rows (`n` or `3 * n`). Query construction happens in
/// Criterion setup; timed work is only `outcome.into_iter().count()`.
///
/// The fresh, never-cloned setup lets `Arc::try_unwrap` reclaim cached rows
/// rather than cloning them.
///
/// Expected outcomes:
/// - Cost stays close to iterator consumption and does not deep-copy row data.
///
/// Unexpected outcomes:
/// - Cost scales beyond simple row consumption, indicating shared-cache
///   fallback, row cloning, or iterator materialization overhead needs
///   inspection.
fn bench_into_iter_owned(c: &mut Criterion) {
    let mut group = c.benchmark_group("QueryService::run/into_iter_owned");
    group.plot_config(
        PlotConfiguration::default().summary_scale(AxisScale::Logarithmic),
    );
    for &n in WORKSPACE_FILE_COUNTS {
        let page_index = build_index_arc(n, ProjectShape::Plain);
        group.throughput(Throughput::Elements(
            u64::try_from(n).expect("note count fits u64"),
        ));
        group.bench_with_input(
            BenchmarkId::new("pages", n),
            &page_index,
            |b, index| {
                b.iter_batched(
                    || {
                        QueryService::new("class").run(
                            index,
                            QueryBuilder::pages(SourceSelector::All),
                        )
                    },
                    |outcome| black_box(outcome.into_iter().count()),
                    BatchSize::SmallInput,
                );
            },
        );

        let task_index = build_index_arc_from_note_source(n, |i, _| {
            task_triplet_note_source(i)
        });
        group.throughput(Throughput::Elements(
            u64::try_from(n).expect("note count fits u64").saturating_mul(3),
        ));
        group.bench_with_input(
            BenchmarkId::new("tasks", n),
            &task_index,
            |b, index| {
                b.iter_batched(
                    || {
                        QueryService::new("class").run(
                            index,
                            QueryBuilder::tasks(SourceSelector::All),
                        )
                    },
                    |outcome| black_box(outcome.into_iter().count()),
                    BatchSize::SmallInput,
                );
            },
        );
    }
    group.finish();
}

criterion_group!(
    benches,
    bench_run_pages,
    bench_run_tasks,
    bench_run_pages_by_metadata,
    bench_sync_and_run_selectors,
    bench_filter_by_metadata_field_count,
    bench_clone_query_set,
    bench_into_iter_owned
);
criterion_main!(benches);
