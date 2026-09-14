//! Performance benchmark suite for query sorting.
//!
//! Exposes and monitors the CPU cost of sorting in the query engine, including
//! `TopK` optimizations, comparator overhead, and permutation cost.
//!
//! ### Data Flow Diagram
//!
//! `QuerySet::sort` pushes a `QueryTransform::Sort` step onto the pending
//! `QueryPlan`. `QueryPlan::run` rewrites a `Sort` immediately followed by a
//! `Limit` into one `QueryTransform::TopK` step (`O(n)` quickselect instead
//! of `O(n log n)` full sort):
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
        task_triplet_note_source,
    },
    project::{build_index_arc, build_index_arc_from_note_source},
};

// ----------------------------------------------------------- //
//                     Fixtures & Helpers                      //
// ----------------------------------------------------------- //

const SORT_STRESS_FILE_COUNTS: &[usize] = &[5_000, 10_000, 20_000, 40_000];

/// Replica of `SortKey::total_cmp`'s Number-vs-Number match arm, extracted to
/// keep
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
/// `rand`, reproducible across runs so regression detection isn't confounded
/// by different shuffle order.
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

/// Returns `n` rating values (0-9, matching `plain_note_source`'s
/// frontmatter) in LCG-shuffled order, so downstream sort benchmarks do not
/// start from nearly-sorted input (timsort is near-linear on sorted input,
/// which would understate comparator cost).
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

/// Measures sort-only cost by frontmatter metadata, swept over workspace size.
///
/// Isolated from the `TopK` fusion benchmarks below (which measure a
/// `Sort`+`Limit` pipeline, not a bare sort) so a regression in
/// `SortOrder::sort_rows`'s comparison or permutation cost is distinguishable
/// from a regression in filter evaluation or field resolution. The size sweep
/// (not a single point) exists so the result can be fit as `A·n + B·n·log₂(n)`:
/// the linear term isolates per-row key resolution/materialization, the `n·log
/// n` term isolates comparator + permutation cost. Single-point measurements
/// cannot separate the two.
///
/// Also runs `sort_only_desc` (identical query, `descending: true`) to
/// exercise `compare_keys`'s `Ordering::reverse()` branch, otherwise
/// unmeasured.
///
/// Expected outcomes:
/// - Cost dominated by the `n·log n` term if comparator dispatch dominates; by
///   the linear term if per-row key materialization dominates.
///
/// Unexpected outcomes:
/// - Cost dominated by the linear term at all sizes, indicating key
///   materialization dominates and comparator dispatch is not the bottleneck.
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

/// Measures `QueryPlan`'s `Sort`+`Limit(n)` -> `TopK` fusion cost across three
/// limit sizes, swept over workspace size.
///
/// `QueryBuilder::sort(...).limit(...)` executed through `QueryService::run`
/// always passes through `QueryPlan::run` (`src/query/service.rs`:
/// `plan.run(records)`), which fuses an adjacent `Sort`+`Limit` into one `TopK`
/// step: `select_nth_unstable_by` partitions in `O(n)`, then the resulting
/// `k`-sized slice is `truncate`d and `sort_unstable_by`'d (`O(k log k)`),
/// total `O(n + k log k)` — not pure `O(n)`. For `k` in `{10, 100, 1000}`
/// against `n` in `[5000, 40000]` here, the tail term is negligible relative
/// to the partition, but the claim below is stated precisely for clarity.
/// The fused `TopK` step overall is compared against a full permutation
/// sort via `SortOrder::sort_rows` (`O(n log n)`). Since the
/// `QuerySet` CTE redesign, `.sort(...).limit(...)` chained directly on a
/// `QuerySet` (the shape the template `tasks`/`query` namespaces use) reaches
/// the same fusion (deferred into the same `QueryPlan`, flushed once on read),
/// so this gap is no longer template-specific; it's the general cost of `TopK`
/// fusion vs. a full sort, still worth guarding against regression. The
/// chained-`QuerySet` path itself isn't benchmarked here:
/// `QuerySet::sort`/`limit` are `pub(crate)`, unreachable from this external
/// bench crate even under `test-utils`; its correctness (not performance) is
/// proven by `src/query/results.rs`'s
/// `cte_chaining::chained_sort_then_limit_matches_full_sort_order_for_tied_keys`
/// unit test. Swept over the same sizes as [`bench_sort_by_metadata`] (not a
/// single point) so the fusion's advantage can be checked against its `O(n)`
/// vs. `O(n log n)` prediction: the ratio between `topk_limit_10` and the full
/// sort should widen as `n` grows, not stay flat.
///
/// Does not re-measure the unfused full sort directly: it is identical to
/// [`bench_sort_by_metadata`]'s `sort_only` (same query, same
/// [`ProjectShape::Plain`] fixture, same sizes), so compare against that
/// benchmark's numbers externally (HTML report or `critcmp`) rather than
/// duplicating the measurement here. Three limit sizes (10, 100, 1000) check
/// whether `select_nth_unstable_by`'s `O(n)` selection advantage holds as `k`
/// grows relative to `n`.
///
/// Expected outcomes:
/// - `topk_limit_10` costs meaningfully less than [`bench_sort_by_metadata`]'s
///   `sort_only` at every size, and the gap widens as `n` grows.
/// - `topk_limit_100` and `topk_limit_1000` cost more than `topk_limit_10` but
///   still less than the full sort, tracking `k`'s share of `n`.
///
/// Unexpected outcomes:
/// - `topk_limit_10`'s cost is comparable to the full sort, indicating `TopK`'s
///   key-materialization pass (paid regardless of `k`) dominates over the
///   selection it avoids.
/// - `topk_limit_1000`'s cost is comparable to `topk_limit_10`'s, indicating
///   `select_nth_unstable_by`'s cost is not growing with `k` as expected.
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
//               Benchmarks: Sort Decomposition                //
// ----------------------------------------------------------- //

/// Measures bare `QueryRow` move/permutation cost, isolated from all
/// comparison and field resolution, swept over workspace size.
///
/// Decomposition of [`bench_sort_by_metadata`]: if a full Fisher-Yates shuffle
/// (n moves of `QueryRow`, each carrying its `Arc<FileIndex>` + `RowIndex` +
/// overlay fields) costs a small fraction of the real sort at every size,
/// element-move cost is ruled out as the dominant component and the cost must
/// live in the comparator or key materialization. Swept over the same sizes as
/// [`bench_sort_by_metadata`] (not a single point) so the permutation share of
/// sort cost can be checked at each `n`, not projected from one measurement:
/// a linear-cost operation's *share* of an `n log n` operation shrinks as `n`
/// grows, so a single point cannot confirm the share stays small at scale.
///
/// Records are produced through the public query API (`run` then
/// `QuerySet::get` + clone); no internals are reached.
///
/// Reports `Throughput::Bytes` (not `Elements`): this benchmark measures
/// data-movement cost, and bytes/second is the meaningful unit for `QueryRow`'s
/// actual in-memory size, unlike an elements/second count that hides
/// per-element cost.
///
/// Expected outcomes:
/// - Shuffle cost is a small fraction of sort-only cost at every size, ruling
///   out element-move as the dominant sort component.
///
/// Unexpected outcomes:
/// - Shuffle cost comparable to sort-only cost, indicating element-move
///   dominates and `QueryRow` size should be reduced.
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

/// Measures the floor: sorting `n` bare `f64` keys with `total_cmp`, swept over
/// workspace size.
///
/// Reference lower bound for `n·log n` comparisons with no enum dispatch, no
/// `SortKey` wrapping, and no `QueryRow` permutation. The gap between this
/// floor and [`bench_sort_by_metadata`] is what the replica and permutation
/// benchmarks attribute. Swept over the same sizes as
/// [`bench_sort_by_metadata`] (not a single point) so the floor's `n log n`
/// scaling can be checked directly against the real sort's fitted curve at each
/// `n`, not assumed from one measurement.
///
/// Expected outcomes:
/// - Cost is lower than sort-only and replica benchmarks at every size,
///   confirming enum dispatch and `QueryRow` permutation add measurable
///   overhead.
///
/// Unexpected outcomes:
/// - Cost matching sort-only or replica benchmarks, indicating overhead beyond
///   raw comparison dominates and the floor is not the bottleneck.
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

/// Measures comparator dispatch cost on `NoteFieldValue` values with a replica
/// of `SortKey::total_cmp`'s shape, swept over workspace size.
///
/// The production comparator, `SortKey::total_cmp`, is `pub(crate)` in
/// `src/query/sort.rs` and unreachable from an external bench crate, so this
/// replicates the exact arm structure the Number-vs-Number path exercises (enum
/// `match` on both operands, then `f64::total_cmp`, with the `descending`
/// branch) against real `NoteFieldValue` values. It measures what a comparator
/// of this shape costs, not the production function itself; conclusions must
/// treat it as a shape-equivalent upper bound on dispatch cost. Swept over the
/// same sizes as [`bench_sort_by_metadata`] (not a single point) so dispatch
/// overhead can be checked against the f64 floor at each `n`.
///
/// Expected outcomes:
/// - Cost is close to the f64 floor at every size, confirming enum dispatch
///   overhead is small relative to comparator + permutation cost.
///
/// Unexpected outcomes:
/// - Cost significantly exceeding the f64 floor, indicating enum dispatch in
///   the real comparator is a meaningful cost contributor.
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

// ----------------------------------------------------------- //
//            Benchmarks: Sort Key Type Coverage               //
// ----------------------------------------------------------- //

/// Measures sort cost by a built-in text field (`file.name`), swept over
/// workspace size.
///
/// [`bench_sort_by_metadata`] only exercises `SortKey::Number`; every other
/// `SortKey` variant (`Text`, `Date`, `Duration`, `Bool`, `Null`) goes
/// unmeasured elsewhere in this file. `file.name` resolves through
/// `FileField::Name`, a different code path than frontmatter metadata
/// resolution, and lands on `SortKey::Text` after `SortKey::from_value_ref`'s
/// date/duration parsing fallback fails to match a filename.
///
/// Expected outcomes:
/// - Cost is comparable to [`bench_sort_by_metadata`]'s `sort_only` at the same
///   `n`; `str::cmp` is not meaningfully more expensive than `f64::total_cmp`.
///
/// Unexpected outcomes:
/// - Cost significantly exceeds `sort_only`, indicating the date/duration
///   parsing fallback in `SortKey::from_value_ref` is not short-circuiting
///   cheaply for non-matching text.
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

/// Measures sort cost by a built-in date field (`file.mtime`), swept over
/// workspace size.
///
/// Exercises `SortKey::DateTime`'s `DateTimeValue` comparator, unmeasured
/// elsewhere in this file. Fixture notes are all written within the same
/// benchmark setup call, so their modification timestamps cluster within the
/// same second or two; this benchmark measures resolution + comparator cost,
/// not a meaningfully discriminating sort order.
///
/// Expected outcomes:
/// - Cost is comparable to [`bench_sort_by_metadata`]'s `sort_only` at the same
///   `n`.
///
/// Unexpected outcomes:
/// - Cost significantly exceeds `sort_only`, indicating `DateTimeValue`
///   resolution or comparison is more expensive than `f64::total_cmp`.
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

/// Measures composite two-term sort cost (`class, rating`) against
/// [`ProjectShape::Classified`]'s four-value `class` field, swept over
/// workspace size.
///
/// [`ProjectShape::Classified`] assigns one of four `class` values
/// (`Project`/`Area`/`Resource`/`Archive`) cyclically, so roughly a quarter of
/// rows tie on the first sort term at every `n`. Ties force
/// `SortOrder::compare_keys`'s tie-break loop into the second term (`rating`),
/// exercising multi-term composite sorting - unmeasured by any other benchmark
/// in this file, which sorts by exactly one field.
///
/// Expected outcomes:
/// - Cost scales similarly to a single-term sort at the same `n`; the tie-break
///   loop adds a second comparison only for the roughly 75% of adjacent pairs
///   that tie on `class`.
///
/// Unexpected outcomes:
/// - Cost meaningfully exceeds twice [`bench_sort_by_metadata`]'s `sort_only`,
///   indicating `compare_keys`'s per-term loop has more than the expected
///   linear-in-terms overhead.
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

/// Measures sort cost over a field that is `Null` on 30% of rows, swept over
/// workspace size.
///
/// [`SortKey::total_cmp`] sorts `Null` below every other value; every other
/// benchmark in this file resolves `rating` from frontmatter that always sets
/// it, so the `Null`-sorts-below branch never fires at scale. This uses
/// [`nullable_rating_note_source`], which omits the `rating` key entirely on 3
/// of every 10 notes, forcing that branch on a meaningful fraction of
/// comparisons.
///
/// Expected outcomes:
/// - Cost is comparable to [`bench_sort_by_metadata`]'s `sort_only` at the same
///   `n`; the `Null` branch is a cheap `Ordering::Less`/`Greater`
///   short-circuit.
///
/// Unexpected outcomes:
/// - Cost significantly exceeds `sort_only`, indicating null resolution
///   (missing-key lookup) is more expensive than a present-key lookup.
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

/// Measures sort cost by a duration-literal metadata field (`estimate`), swept
/// over workspace size.
///
/// Exercises `SortKey::Duration`'s `DurationSeconds` comparator, reached only
/// after `SortKey::from_value_ref`/`from_owned` fail an ISO-date parse and
/// succeed a `DurationValue::parse` on the field text - a different resolution
/// path than the numeric `rating` field every other sort benchmark in this file
/// uses.
///
/// Expected outcomes:
/// - Cost is comparable to [`bench_sort_by_metadata`]'s `sort_only` at the same
///   `n`.
///
/// Unexpected outcomes:
/// - Cost significantly exceeds `sort_only`, indicating `DurationValue::parse`
///   is a meaningful per-row cost at scale.
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

/// Measures sort cost on task rows by `task.completed` (`SortKey::Bool`), swept
/// over workspace size.
///
/// Every other benchmark in this file sorts page rows; task rows resolve fields
/// through `QueryRow::resolve_ref`'s `RowKind::Task` branch
/// (`src/query/results.rs`), a different path than page-row
/// frontmatter/file-field resolution. This also exercises `SortKey::Bool`, the
/// only `SortKey` variant no other benchmark in this file reaches.
///
/// Expected outcomes:
/// - Cost scales with total task count (`n * 3`, from
///   [`task_triplet_note_source`]'s three tasks per note), similar in shape to
///   [`bench_sort_by_metadata`]'s per-row cost.
///
/// Unexpected outcomes:
/// - Cost per task row significantly exceeds [`bench_sort_by_metadata`]'s cost
///   per page row, indicating task-row field resolution is more expensive than
///   page-row resolution.
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

criterion_group!(
    benches,
    bench_sort_by_metadata,
    bench_topk_vs_full_sort,
    bench_permute_query_rows,
    bench_sort_f64_floor,
    bench_sort_note_field_value_replica,
    bench_sort_by_text,
    bench_sort_by_date,
    bench_sort_composite,
    bench_sort_nullable,
    bench_sort_by_duration,
    bench_sort_task_rows
);
criterion_main!(benches);
