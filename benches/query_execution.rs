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

#[expect(
    dead_code,
    reason = "shared benchmark common helpers are compiled into each bench \
              target; this target uses only query index fixtures"
)]
mod common;

use common::{
    WORKSPACE_FILE_COUNTS,
    content::{
        ProjectShape, metadata_lookup_note_source, task_note_source,
        task_triplet_note_source,
    },
    project::{build_index_arc, build_index_arc_from_note_source},
};

// ----------------------------------------------------------- //
//                     Fixtures & Helpers                      //
// ----------------------------------------------------------- //

const QUERY_METADATA_FIELD_COUNTS: &[usize] = &[1, 5, 10, 20];
const TASK_DENSITY_COUNTS: &[usize] = &[1, 3, 10, 20];
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
    let service = QueryService::new("class");
    for &n in WORKSPACE_FILE_COUNTS {
        let index = build_index_arc(n, ProjectShape::Plain);
        group.throughput(Throughput::Elements(
            u64::try_from(n).expect("note count fits u64"),
        ));
        group.bench_with_input(BenchmarkId::new("pages", n), &n, |b, _| {
            b.iter_batched(
                || index.clone(),
                |index| {
                    service
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
    let service = QueryService::new("class");
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
                    service
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
    let service = QueryService::new("class");
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
                        service.run(
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
// ----------------------------------------------------------- //
/// Measures end-to-end filter cost by frontmatter field count at a fixed
/// 20,000-note workspace size.
///
/// Parameters: varies metadata field count over `{1, 5, 10, 20}`; reports
/// 20,000 input rows per iteration. Fixture indexes are built outside timing by
/// `metadata_lookup_note_source`, which places `rating` after the synthetic
/// fields so accidental linear metadata lookup is visible.
///
/// Runs `rows_floor` (unfiltered page selection across varied field width)
/// alongside `filter` (`rating > 2`).
///
/// Subtraction formula:
/// - `filter - rows_floor`: Isolates filter predicate evaluation from
///   field-width note row construction.
///
/// Expected outcomes:
/// - `rows_floor` reflects field-width row extraction cost.
/// - `filter - rows_floor` remains broadly flat across field counts because
///   query field paths use canonical hash-keyed lookup.
///
/// Unexpected outcomes:
/// - Predicate evaluation cost growing with field count, signaling metadata
///   lookup or key-resolution regressions.
fn bench_filter_by_metadata_field_count(c: &mut Criterion) {
    let mut group =
        c.benchmark_group("QueryService::run/filter_by_field_count");
    let service = QueryService::new("class");
    let n = 20_000_usize;
    for &fields in QUERY_METADATA_FIELD_COUNTS {
        let index = build_index_arc_from_note_source(n, |i, _| {
            metadata_lookup_note_source(i, fields)
        });
        group.throughput(Throughput::Elements(
            u64::try_from(n).expect("note count fits u64"),
        ));
        group.bench_with_input(
            BenchmarkId::new("rows_floor", fields),
            &fields,
            |b, _| {
                b.iter_batched(
                    || index.clone(),
                    |index| {
                        service.run(
                            &index,
                            QueryBuilder::pages(SourceSelector::All),
                        )
                    },
                    BatchSize::SmallInput,
                );
            },
        );
        group.bench_with_input(
            BenchmarkId::new("filter", fields),
            &fields,
            |b, _| {
                b.iter_batched(
                    || index.clone(),
                    |index| {
                        service.run(
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

/// Measures task expansion throughput across varied tasks-per-note density.
///
/// Holds notes fixed at 1,000 and sweeps tasks per note over `{1, 3, 10, 20}`.
/// Isolates per-task expansion cost from fixed per-note iteration.
///
/// Expected outcomes:
/// - Execution time scales linearly with total expanded task rows ($n \times
///   \text{tasks\_per\_note}$).
///
/// Unexpected outcomes:
/// - Super-linear growth with task density, indicating per-note allocation
///   churn or unbounded vector resizing during task row extraction.
fn bench_run_tasks_density(c: &mut Criterion) {
    let mut group = c.benchmark_group("QueryService::run/tasks_density");
    group.plot_config(
        PlotConfiguration::default().summary_scale(AxisScale::Logarithmic),
    );
    let service = QueryService::new("class");
    let note_count = 1_000_usize;
    for &tasks_per_note in TASK_DENSITY_COUNTS {
        let index = build_index_arc_from_note_source(note_count, |i, _| {
            task_note_source(i, tasks_per_note)
        });
        let total_tasks = note_count.saturating_mul(tasks_per_note);
        group.throughput(Throughput::Elements(
            u64::try_from(total_tasks).expect("task count fits u64"),
        ));
        group.bench_with_input(
            BenchmarkId::new("tasks", tasks_per_note),
            &tasks_per_note,
            |b, _| {
                b.iter_batched(
                    || index.clone(),
                    |index| {
                        service.run(
                            &index,
                            QueryBuilder::tasks(SourceSelector::All),
                        )
                    },
                    BatchSize::SmallInput,
                );
            },
        );
    }
    group.finish();
}
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
    let service = QueryService::new("class");
    for &n in WORKSPACE_FILE_COUNTS {
        let index = build_index_arc(n, ProjectShape::Plain);
        let outcome =
            service.run(&index, QueryBuilder::pages(SourceSelector::All));
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
    let service = QueryService::new("class");
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
                        service.run(
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
                        service.run(
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
    bench_run_tasks_density,
    bench_run_pages_by_metadata,
    bench_filter_by_metadata_field_count,
    bench_clone_query_set,
    bench_into_iter_owned
);
criterion_main!(benches);
