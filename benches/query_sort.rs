//! Performance benchmark suite for query sorting.
//!
//! Exposes and monitors the CPU cost of sorting in the query engine, including
//! `TopK` optimizations, comparator overhead, and permutation cost.
//!
//! ### Data Flow Diagram
//!
//! `QuerySet::sort` pushes a `QueryTransform::Sort` step onto the pending
//! `QueryPlan`. `QueryPlan::run` rewrites a `Sort` immediately followed by a
//! `Limit` into one `QueryTransform::TopK` step (`O(n)` quickselect instead of
//! `O(n log n)` full sort):
//!
//! ```text
//! [QuerySet] ──(.sort)──► [QueryTransform::Sort]
//!                              │
//!                              └──(.limit)──► [QueryTransform::TopK]
//! ```
//!
//! ### Profiling Integration
//!
//! To profile query sorting CPU bottlenecks:
//! ```bash
//! cargo flamegraph --bench query_sort -- --bench "bench_sort_by_metadata"
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

use std::{cmp::Ordering, hint::black_box, mem::size_of, time::Duration};

use criterion::{
    AxisScale, BatchSize, BenchmarkId, Criterion, PlotConfiguration,
    Throughput, criterion_group, criterion_main,
};
use traces_pkm::{
    NoteFieldValue, QueryBuilder, QueryRow, QueryService, SourceSelector,
};

#[allow(
    dead_code,
    reason = "shared benchmark common helpers are compiled into each bench \
              target; this target uses only query index fixtures"
)]
mod common;

use common::{
    content::{
        ProjectShape, duration_field_note_source, nullable_rating_note_source,
        task_triplet_note_source, title_field_note_source,
    },
    project::{build_index_arc, build_index_arc_from_note_source},
};

// ----------------------------------------------------------- //
//                     Fixtures & Helpers                      //
// ----------------------------------------------------------- //

const SORT_STRESS_FILE_COUNTS: &[usize] = &[5_000, 10_000, 20_000, 40_000];

/// Replica of `SortKey::cmp`'s Number-vs-Number match arm, extracted to keep
/// [`bench_sort_note_field_value_replica`]'s closure nesting within clippy's
/// `excessive_nesting` threshold.
fn replica_cmp(
    lhs: &NoteFieldValue,
    rhs: &NoteFieldValue,
    descending: bool,
) -> Ordering {
    let (NoteFieldValue::Number(x), NoteFieldValue::Number(y)) = (lhs, rhs)
    else {
        return Ordering::Equal;
    };
    if descending {
        y.total_cmp(x)
    } else {
        x.total_cmp(y)
    }
}

/// Deterministic Linear Congruential Generator (LCG): `state = state * a + c`
/// mod 2^64. One multiply and one add per element, with no dependency on
/// `rand`, reproducible across runs so regression detection isn't confounded by
/// different shuffle order.
fn lcg_next(state: &mut u64) -> u64 {
    *state = state
        .wrapping_mul(6_364_136_223_846_793_005)
        .wrapping_add(1_442_695_040_888_963_407);
    *state
}

/// Fisher-Yates shuffles `items` in place in O(n), using the shared
/// deterministic Linear Congruential Generator as the uniform random source.
fn lcg_shuffle<T>(items: &mut [T], state: &mut u64) {
    for i in (1..items.len()).rev() {
        let span = u64::try_from(i)
            .expect("index fits u64")
            .checked_add(1)
            .expect("span fits u64");
        let j =
            (lcg_next(state) >> 33).checked_rem(span).expect("span is nonzero");
        items.swap(i, usize::try_from(j).expect("index fits usize"));
    }
}

/// Returns `n` rating values (0-9, matching `plain_note_source`'s frontmatter)
/// in LCG-shuffled order, so downstream sort benchmarks do not start from
/// nearly-sorted input (timsort is near-linear on sorted input, which would
/// understate comparator cost).
fn shuffled_ratings(n: usize) -> Vec<f64> {
    let mut state = 0x243f_6a88_85a3_08d3_u64;
    let mut keys: Vec<f64> = (0..n)
        .map(|i| f64::from(i32::try_from(i % 10).expect("rating fits i32")))
        .collect();
    lcg_shuffle(&mut keys, &mut state);
    keys
}

// ----------------------------------------------------------- //
//                  Benchmarks: Isolated Sort                  //
// ----------------------------------------------------------- //

/// Measures metadata sort cost over plain in-memory page rows.
///
/// Parameters: varies [`SORT_STRESS_FILE_COUNTS`]; reports input rows.
///
/// Fixture indexes are built outside timing. Timed work builds a
/// `sort("rating")` query and runs source-row construction, metadata key
/// resolution, precomputed-key comparison, and row permutation.
/// `sort_only_desc` exercises reverse ordering.
///
/// The size sweep can distinguish aggregate linear work from comparison-shaped
/// growth, but it does not isolate individual subsystems by itself.
///
/// Expected outcomes:
/// - Cost follows a row-construction/key-resolution plus `n log n` sort shape;
///   descending and ascending stay in the same cost class.
///
/// Unexpected outcomes:
/// - Cost grows faster than `n log n` or descending is much slower than
///   ascending, indicating key materialization, comparison, or permutation work
///   needs inspection.
fn bench_sort_by_metadata(c: &mut Criterion) {
    let mut group = c.benchmark_group("QueryService::run/sort_by_metadata");
    group.plot_config(
        PlotConfiguration::default().summary_scale(AxisScale::Logarithmic),
    );
    for &n in SORT_STRESS_FILE_COUNTS {
        if n >= 20_000 {
            group.sample_size(10);
        }
        let index = build_index_arc(n, ProjectShape::Plain);
        group.throughput(Throughput::Elements(
            u64::try_from(n).expect("note count fits u64"),
        ));
        group.bench_with_input(BenchmarkId::new("sort_only", n), &n, |b, _| {
            b.iter_batched(
                || {
                    QueryBuilder::pages(SourceSelector::All)
                        .sort("rating", false)
                        .expect("valid sort")
                },
                |query| QueryService::new("class").run(&index, query),
                BatchSize::SmallInput,
            );
        });
        group.bench_with_input(
            BenchmarkId::new("sort_only_desc", n),
            &n,
            |b, _| {
                b.iter_batched(
                    || {
                        QueryBuilder::pages(SourceSelector::All)
                            .sort("rating", true)
                            .expect("valid sort")
                    },
                    |query| QueryService::new("class").run(&index, query),
                    BatchSize::SmallInput,
                );
            },
        );
    }
    group.finish();
}

// ----------------------------------------------------------- //
//             Benchmarks: Sort Plan Optimization              //
// ----------------------------------------------------------- //
/// Measures `QueryPlan`'s `Sort`+`Limit(k)` -> `TopK` fusion cost across limit
/// sizes and workspace sizes.
///
/// Parameters: varies limit `k` in `{10, 100, 1000}` and note count in
/// [`SORT_STRESS_FILE_COUNTS`]; reports input rows.
///
/// Fixture: [`ProjectShape::Plain`] index built outside timing; timed work runs
/// `QueryBuilder::pages(...).sort(...).limit(...)` through
/// [`QueryService::run`].
///
/// Compares against [`bench_sort_by_metadata`]'s `sort_only` numbers under the
/// same `n` and fixture.
///
/// `TopK` still pays source-row construction, key extraction, indexed-vector
/// allocation, `O(n)` selection, and an `O(k log k)` selected-slice sort.
///
/// Expected outcomes:
/// - For `n` materially larger than `k`, `topk_limit_10` beats full sort and
///   the gap widens as `n` grows.
/// - Larger `k` trends toward full-sort cost through selected-slice sorting.
///
/// Unexpected outcomes:
/// - Small-`k` cost matches full sort, indicating fixed key/materialization
///   work dominates the saved permutation.
/// - Large-`k` cost exceeds full sort, indicating partition/truncation overhead
///   outweighs the comparison savings.
fn bench_topk_vs_full_sort(c: &mut Criterion) {
    let mut group = c.benchmark_group("QueryService::run/topk_fusion");
    group.plot_config(
        PlotConfiguration::default().summary_scale(AxisScale::Logarithmic),
    );
    for &n in SORT_STRESS_FILE_COUNTS {
        if n >= 20_000 {
            group.sample_size(10);
        }
        let index = build_index_arc(n, ProjectShape::Plain);
        group.throughput(Throughput::Elements(
            u64::try_from(n).expect("note count fits u64"),
        ));
        for limit in [10_i64, 100, 1000] {
            group.bench_with_input(
                BenchmarkId::new(format!("topk_limit_{limit}"), n),
                &n,
                |b, _| {
                    b.iter_batched(
                        || {
                            QueryBuilder::pages(SourceSelector::All)
                                .sort("rating", false)
                                .expect("valid sort")
                                .limit(limit)
                                .expect("valid limit")
                        },
                        |query| QueryService::new("class").run(&index, query),
                        BatchSize::SmallInput,
                    );
                },
            );
        }
    }
    group.finish();
}

// ----------------------------------------------------------- //
//            Benchmarks: Sort Key Type Coverage               //
// ----------------------------------------------------------- //
/// Measures sort cost by a built-in text field (`file.name`) over plain page
/// rows.
///
/// Parameters: varies [`SORT_STRESS_FILE_COUNTS`]; reports input rows.
///
/// Fixture indexes are built outside timing. Timed work sorts by `file.name`,
/// exercising file-field resolution and `SortKey::Text`
/// classification/comparison.
///
/// Expected outcomes:
/// - Cost stays in the broad range of the numeric sort anchor for the same `n`.
///
/// Unexpected outcomes:
/// - Cost significantly exceeds the numeric anchor, indicating text-key field
///   resolution, classification, or string comparison needs inspection.
fn bench_sort_by_text(c: &mut Criterion) {
    let mut group = c.benchmark_group("QueryService::run/sort_by_text");
    group.plot_config(
        PlotConfiguration::default().summary_scale(AxisScale::Logarithmic),
    );
    for &n in SORT_STRESS_FILE_COUNTS {
        if n >= 20_000 {
            group.sample_size(10);
        }
        let index = build_index_arc(n, ProjectShape::Plain);
        group.throughput(Throughput::Elements(
            u64::try_from(n).expect("note count fits u64"),
        ));
        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, _| {
            b.iter_batched(
                || {
                    QueryBuilder::pages(SourceSelector::All)
                        .sort("file.name", false)
                        .expect("valid sort")
                },
                |query| QueryService::new("class").run(&index, query),
                BatchSize::SmallInput,
            );
        });
    }
    group.finish();
}

/// Measures sort cost by a frontmatter text field (`title`) over page rows.
///
/// Parameters: varies [`SORT_STRESS_FILE_COUNTS`]; reports input rows.
///
/// Fixture indexes are built outside timing from `title_field_note_source`,
/// whose title values are pseudorandomized so the sort does not start already
/// ordered.
///
/// Expected outcomes:
/// - Cost stays in the broad range of the numeric sort anchor for the same `n`.
///
/// Unexpected outcomes:
/// - Cost significantly exceeds the numeric anchor, indicating metadata
///   resolution, `SortKey::from_text` classification, borrowing/allocation, or
///   string comparison needs inspection.
fn bench_sort_by_title(c: &mut Criterion) {
    let mut group = c.benchmark_group("QueryService::run/sort_by_title");
    group.plot_config(
        PlotConfiguration::default().summary_scale(AxisScale::Logarithmic),
    );
    for &n in SORT_STRESS_FILE_COUNTS {
        if n >= 20_000 {
            group.sample_size(10);
        }
        let index = build_index_arc_from_note_source(n, |i, _| {
            title_field_note_source(i)
        });
        group.throughput(Throughput::Elements(
            u64::try_from(n).expect("note count fits u64"),
        ));
        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, _| {
            b.iter_batched(
                || {
                    QueryBuilder::pages(SourceSelector::All)
                        .sort("title", false)
                        .expect("valid sort")
                },
                |query| QueryService::new("class").run(&index, query),
                BatchSize::SmallInput,
            );
        });
    }
    group.finish();
}

/// Measures sort cost by a built-in date field (`file.mtime`) over page rows.
///
/// Parameters: varies [`SORT_STRESS_FILE_COUNTS`]; reports input rows.
///
/// Fixture indexes are built in memory outside timing; synthetic [`FileBase`]
/// timestamps are created during fixture setup rather than by writing files.
///
/// This is a DateTime-key resolution and tie-heavy comparison path, not a
/// deterministic varied-timestamp ordering benchmark.
///
/// Expected outcomes:
/// - Cost stays in the broad range of the numeric sort anchor for the same `n`.
///
/// Unexpected outcomes:
/// - Cost significantly exceeds the numeric anchor, indicating `DateTime` key
///   resolution, null/tie handling, or comparison needs inspection.
fn bench_sort_by_date(c: &mut Criterion) {
    let mut group = c.benchmark_group("QueryService::run/sort_by_date");
    group.plot_config(
        PlotConfiguration::default().summary_scale(AxisScale::Logarithmic),
    );
    for &n in SORT_STRESS_FILE_COUNTS {
        if n >= 20_000 {
            group.sample_size(10);
        }
        let index = build_index_arc(n, ProjectShape::Plain);
        group.throughput(Throughput::Elements(
            u64::try_from(n).expect("note count fits u64"),
        ));
        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, _| {
            b.iter_batched(
                || {
                    QueryBuilder::pages(SourceSelector::All)
                        .sort("file.mtime", false)
                        .expect("valid sort")
                },
                |query| QueryService::new("class").run(&index, query),
                BatchSize::SmallInput,
            );
        });
    }
    group.finish();
}

/// Measures composite two-term sort cost (`class, rating`) over classified page
/// rows.
///
/// Parameters: varies [`SORT_STRESS_FILE_COUNTS`]; reports input rows.
///
/// Fixture indexes are built outside timing from [`ProjectShape::Classified`],
/// whose four class values are assigned cyclically.
///
/// The second term is reached only when `class` ties; under uniform random
/// comparison this is roughly one quarter of pairs, though actual sort
/// comparator frequency depends on input order.
///
/// Expected outcomes:
/// - Cost increases over one-term sort but stays consistent with a two-term
///   comparison where most comparisons decide on `class`.
///
/// Unexpected outcomes:
/// - Cost grows disproportionately, indicating term-loop work or unexpectedly
///   frequent second-term comparisons needs inspection.
fn bench_sort_composite(c: &mut Criterion) {
    let mut group = c.benchmark_group("QueryService::run/sort_composite");
    group.plot_config(
        PlotConfiguration::default().summary_scale(AxisScale::Logarithmic),
    );
    for &n in SORT_STRESS_FILE_COUNTS {
        if n >= 20_000 {
            group.sample_size(10);
        }
        let index = build_index_arc(n, ProjectShape::Classified);
        group.throughput(Throughput::Elements(
            u64::try_from(n).expect("note count fits u64"),
        ));
        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, _| {
            b.iter_batched(
                || {
                    QueryBuilder::pages(SourceSelector::All)
                        .sort("class, rating", false)
                        .expect("valid sort")
                },
                |query| QueryService::new("class").run(&index, query),
                BatchSize::SmallInput,
            );
        });
    }
    group.finish();
}

/// Measures sort cost over a numeric field that is `Null` on 30% of rows.
///
/// Parameters: varies [`SORT_STRESS_FILE_COUNTS`]; reports input rows.
///
/// Fixture indexes are built outside timing from
/// [`nullable_rating_note_source`], which omits `rating` on 3 of every 10
/// notes.
///
/// This exercises `SortOrder::compare_keys` null-placement logic for numeric
/// rating sorts.
///
/// Expected outcomes:
/// - Cost stays near the always-present numeric rating sort for the same `n`.
///
/// Unexpected outcomes:
/// - Cost significantly exceeds the numeric anchor, indicating missing-key
///   resolution or null-placement comparison needs inspection.
fn bench_sort_nullable(c: &mut Criterion) {
    let mut group = c.benchmark_group("QueryService::run/sort_nullable");
    group.plot_config(
        PlotConfiguration::default().summary_scale(AxisScale::Logarithmic),
    );
    for &n in SORT_STRESS_FILE_COUNTS {
        if n >= 20_000 {
            group.sample_size(10);
        }
        let index = build_index_arc_from_note_source(n, |i, _| {
            nullable_rating_note_source(i)
        });
        group.throughput(Throughput::Elements(
            u64::try_from(n).expect("note count fits u64"),
        ));
        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, _| {
            b.iter_batched(
                || {
                    QueryBuilder::pages(SourceSelector::All)
                        .sort("rating", false)
                        .expect("valid sort")
                },
                |query| QueryService::new("class").run(&index, query),
                BatchSize::SmallInput,
            );
        });
    }
    group.finish();
}

/// Measures sort cost by a duration-literal metadata field (`estimate`) over
/// page rows.
///
/// Parameters: varies [`SORT_STRESS_FILE_COUNTS`]; reports input rows.
///
/// Fixture indexes are built outside timing from
/// [`duration_field_note_source`]. Source strings are parsed during fixture
/// construction, so timed key extraction calls `DurationValue::to_seconds()`
/// without reparsing strings.
///
/// Expected outcomes:
/// - Cost stays in the broad range of the numeric sort anchor for the same `n`.
///
/// Unexpected outcomes:
/// - Cost significantly exceeds the numeric anchor, indicating duration key
///   extraction or comparison needs inspection.
fn bench_sort_by_duration(c: &mut Criterion) {
    let mut group = c.benchmark_group("QueryService::run/sort_by_duration");
    group.plot_config(
        PlotConfiguration::default().summary_scale(AxisScale::Logarithmic),
    );
    for &n in SORT_STRESS_FILE_COUNTS {
        if n >= 20_000 {
            group.sample_size(10);
        }
        let index = build_index_arc_from_note_source(n, |i, _| {
            duration_field_note_source(i)
        });
        group.throughput(Throughput::Elements(
            u64::try_from(n).expect("note count fits u64"),
        ));
        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, _| {
            b.iter_batched(
                || {
                    QueryBuilder::pages(SourceSelector::All)
                        .sort("estimate", false)
                        .expect("valid sort")
                },
                |query| QueryService::new("class").run(&index, query),
                BatchSize::SmallInput,
            );
        });
    }
    group.finish();
}

/// Measures sort cost on task rows by `task.completed` (`SortKey::Bool`).
///
/// Parameters: varies [`SORT_STRESS_FILE_COUNTS`]; reports task rows (`3 * n`).
///
/// Fixture indexes are built outside timing from [`task_triplet_note_source`],
/// so timed work expands pre-parsed tasks, resolves `task.completed`, and sorts
/// task rows.
///
/// Expected outcomes:
/// - Cost scales with total task count and stays comparable per output row to
///   page-row sort cost.
///
/// Unexpected outcomes:
/// - Cost per task row significantly exceeds page-row sort cost, indicating
///   task-row field resolution or Bool comparison needs inspection.
fn bench_sort_task_rows(c: &mut Criterion) {
    let mut group = c.benchmark_group("QueryService::run/sort_task_rows");
    group.plot_config(
        PlotConfiguration::default().summary_scale(AxisScale::Logarithmic),
    );
    for &n in SORT_STRESS_FILE_COUNTS {
        if n >= 20_000 {
            group.sample_size(10);
        }
        let index = build_index_arc_from_note_source(n, |i, _| {
            task_triplet_note_source(i)
        });
        group.throughput(Throughput::Elements(
            u64::try_from(n).expect("note count fits u64").saturating_mul(3),
        ));
        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, _| {
            b.iter_batched(
                || {
                    QueryBuilder::tasks(SourceSelector::All)
                        .sort("task.completed", false)
                        .expect("valid sort")
                },
                |query| QueryService::new("class").run(&index, query),
                BatchSize::SmallInput,
            );
        });
    }
    group.finish();
}

// ----------------------------------------------------------- //
//               Benchmarks: Sort Decomposition                //
// ----------------------------------------------------------- //
/// Measures synthetic `QueryRow` inline move/permutation cost, isolated from
/// comparisons and field resolution.
///
/// Parameters: varies [`SORT_STRESS_FILE_COUNTS`]; reports inline
/// `size_of::<QueryRow>() * n` bytes moved by the shuffle.
///
/// Fixture rows are produced through the public query API outside timing.
///
/// This does not measure total row memory footprint or the full sort reassembly
/// path; it isolates the Fisher-Yates swap cost for cloned rows.
///
/// Expected outcomes:
/// - Shuffle cost remains a small contextual component relative to sort-only
///   cost at the same `n`.
///
/// Unexpected outcomes:
/// - Shuffle cost approaches sort-only cost, indicating inline row size or row
///   movement deserves inspection before comparator work.
fn bench_permute_query_rows(c: &mut Criterion) {
    let mut group = c.benchmark_group("QueryService::run/permute_records");
    group.plot_config(
        PlotConfiguration::default().summary_scale(AxisScale::Logarithmic),
    );
    group.measurement_time(Duration::from_secs(2));
    for &n in SORT_STRESS_FILE_COUNTS {
        if n >= 20_000 {
            group.sample_size(10);
        }
        let index = build_index_arc(n, ProjectShape::Plain);
        let base: Vec<QueryRow> = {
            let set = QueryService::new("class")
                .run(&index, QueryBuilder::pages(SourceSelector::All));
            (0..set.len())
                .map(|i| set.get(i).expect("row present").clone())
                .collect()
        };
        group.throughput(Throughput::Bytes(
            u64::try_from(n.saturating_mul(size_of::<QueryRow>()))
                .expect("byte length fits u64"),
        ));
        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, _| {
            b.iter_batched_ref(
                || base.clone(),
                |records| {
                    let mut state = 0x853c_49e6_748f_ea9b_u64;
                    lcg_shuffle(records, &mut state);
                    black_box(&*records);
                },
                BatchSize::LargeInput,
            );
        });
    }
    group.finish();
}

/// Measures a synthetic comparator baseline: sorting `n` shuffled bare `f64`
/// keys with `total_cmp`.
///
/// Parameters: varies [`SORT_STRESS_FILE_COUNTS`]; reports key count.
///
/// Fixture keys are generated outside timing with deterministic shuffled
/// ratings.
///
/// This is not a production lower bound: production sort also resolves keys,
/// normalizes values, builds order/reassembly buffers, and permutes rows.
///
/// Expected outcomes:
/// - Cost provides a stable broad anchor for raw numeric comparison work.
///
/// Unexpected outcomes:
/// - Cost converges with production sort, indicating non-comparison overhead
///   has shrunk or this synthetic floor no longer separates the paths.
fn bench_sort_f64_floor(c: &mut Criterion) {
    let mut group = c.benchmark_group("QueryService::run/sort_f64_floor");
    group.plot_config(
        PlotConfiguration::default().summary_scale(AxisScale::Logarithmic),
    );
    group.measurement_time(Duration::from_secs(2));
    for &n in SORT_STRESS_FILE_COUNTS {
        if n >= 20_000 {
            group.sample_size(10);
        }
        group.throughput(Throughput::Elements(
            u64::try_from(n).expect("note count fits u64"),
        ));
        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, &n| {
            b.iter_batched_ref(
                || shuffled_ratings(n),
                |keys| {
                    keys.sort_by(f64::total_cmp);
                    black_box(&*keys);
                },
                BatchSize::SmallInput,
            );
        });
    }
    group.finish();
}

/// Measures a simplified synthetic `NoteFieldValue::Number` comparator shape.
///
/// Parameters: varies [`SORT_STRESS_FILE_COUNTS`]; reports value count. Fixture
/// values are generated outside timing from shuffled ratings.
///
/// The closure matches two `NoteFieldValue::Number` variants and calls
/// `f64::total_cmp`; it is intentionally not the production `SortKey::cmp`
/// implementation, which has normalization and sort-order plumbing.
///
/// Expected outcomes:
/// - Cost stays near the bare-f64 anchor if enum matching adds little overhead
///   in this synthetic path.
///
/// Unexpected outcomes:
/// - Cost significantly exceeds the f64 anchor, indicating this synthetic enum
///   match path deserves inspection before attributing production sort cost.
fn bench_sort_note_field_value_replica(c: &mut Criterion) {
    let mut group = c.benchmark_group("QueryService::run/sort_value_replica");
    group.plot_config(
        PlotConfiguration::default().summary_scale(AxisScale::Logarithmic),
    );
    group.measurement_time(Duration::from_secs(2));
    let descending = false;
    for &n in SORT_STRESS_FILE_COUNTS {
        if n >= 20_000 {
            group.sample_size(10);
        }
        group.throughput(Throughput::Elements(
            u64::try_from(n).expect("note count fits u64"),
        ));
        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, &n| {
            b.iter_batched_ref(
                || {
                    shuffled_ratings(n)
                        .into_iter()
                        .map(NoteFieldValue::Number)
                        .collect::<Vec<_>>()
                },
                |keys| {
                    keys.sort_by(|lhs, rhs| replica_cmp(lhs, rhs, descending));
                    black_box(&*keys);
                },
                BatchSize::SmallInput,
            );
        });
    }
    group.finish();
}

criterion_group!(
    benches,
    bench_sort_by_metadata,
    bench_topk_vs_full_sort,
    bench_sort_by_text,
    bench_sort_by_title,
    bench_sort_by_date,
    bench_sort_composite,
    bench_sort_nullable,
    bench_sort_by_duration,
    bench_sort_task_rows,
    bench_permute_query_rows,
    bench_sort_f64_floor,
    bench_sort_note_field_value_replica
);
criterion_main!(benches);
