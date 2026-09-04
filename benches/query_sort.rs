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

use std::{cmp::Ordering, hint::black_box, sync::Arc};

use criterion::{
    BatchSize, BenchmarkId, Criterion, Throughput, criterion_group,
    criterion_main,
};
use traces_pkm::{
    FileIndex, IndexerService, NoteFieldValue, QueryBuilder, QueryRow,
    QueryService, SourceSelector,
};

// ----------------------------------------------------------- //
//                     Fixtures & Helpers                      //
// ----------------------------------------------------------- //

const SORT_SWEEP_SIZES: &[usize] = &[5_000, 10_000, 20_000, 40_000];

fn create_page_index(n: usize) -> Arc<FileIndex> {
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

/// Returns `n` rating values (0–99, matching `built_index`'s frontmatter) in
/// LCG-shuffled order, so downstream sort benchmarks do not start from
/// nearly-sorted input (timsort is near-linear on sorted input, which would
/// understate comparator cost).
fn shuffled_ratings(n: usize) -> Vec<f64> {
    let mut state = 0x243f_6a88_85a3_08d3_u64;
    let mut keys: Vec<f64> = (0..n)
        .map(|i| f64::from(i32::try_from(i % 100).expect("rating fits i32")))
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
/// `Sort`+`Limit` pipeline, not a bare sort) so
/// a regression in `SortOrder::sort_rows`'s comparison or permutation cost is
/// distinguishable from a regression in filter evaluation or field resolution.
/// The size sweep (not a single point) exists so the result can be fit as
/// `A·n + B·n·log₂(n)`: the linear term isolates per-row key
/// resolution/materialization, the `n·log n` term isolates comparator +
/// permutation cost. Single-point measurements cannot separate the two.
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
    for &n in SORT_SWEEP_SIZES {
        let index = create_page_index(n);
        group.throughput(Throughput::Elements(
            u64::try_from(n).expect("note count fits u64"),
        ));
        group.bench_with_input(BenchmarkId::new("sort_only", n), &n, |b, _| {
            b.iter_batched(
                || index.clone(),
                |index| {
                    QueryService::new("class").run(
                        &index,
                        QueryBuilder::pages(SourceSelector::All)
                            .sort("rating", false)
                            .expect("valid sort"),
                    )
                },
                BatchSize::SmallInput,
            );
        });
    }
    group.finish();
}

// ----------------------------------------------------------- //
//             Benchmarks: Sort Plan Optimization              //
// ----------------------------------------------------------- //

/// Measures `QueryPlan`'s `Sort`+`Limit(n)` → `TopK` fusion against an
/// unfused full sort, swept over workspace size.
///
/// `QueryBuilder::sort(...).limit(...)` executed through
/// `QueryService::run` always passes through `QueryPlan::run`
/// (`src/query/service.rs`: `plan.run(records)`), which fuses an
/// adjacent `Sort`+`Limit` into one `TopK` step using
/// `select_nth_unstable_by` (`O(n)` selection) instead of a full
/// permutation sort via `SortOrder::sort_rows` (`O(n log n)`). Since the
/// `QuerySet` CTE redesign, `.sort(...).limit(...)` chained directly on a
/// `QuerySet` (the shape the template `tasks`/`query` namespaces use) reaches
/// the same fusion (deferred into the same `QueryPlan`, flushed once on read),
/// so this gap is no longer template-specific; it's the general cost of `TopK`
/// fusion vs. a full sort, still worth guarding against regression. The
/// chained-`QuerySet` path itself isn't benchmarked here:
/// `QuerySet::sort`/`limit` are `pub(crate)`, unreachable from this
/// external bench crate even under `test-utils`; its correctness (not
/// performance) is proven by
/// `src/query/results.rs`'s
/// `cte_chaining::chained_sort_then_limit_matches_full_sort_order_for_tied_keys`
/// unit test. Swept over the same sizes as [`bench_sort_by_metadata`] (not a
/// single point) so the fusion's advantage can be checked against its `O(n)`
/// vs. `O(n log n)` prediction: the ratio between the two sub-benchmarks
/// should widen as `n` grows, not stay flat.
///
/// Expected outcomes:
/// - `topk_limit_10` costs meaningfully less than `full_sort_no_limit` at every
///   size, and the gap widens as `n` grows.
///
/// Unexpected outcomes:
/// - Costs are comparable, indicating `TopK`'s key-materialization pass (paid
///   regardless of `n < len(keyed)`) dominates over the selection it avoids,
///   and the fusion buys little for this workload. A gap that does not widen
///   with `n` would contradict the `O(n)` vs. `O(n log n)` prediction.
fn bench_topk_vs_full_sort(c: &mut Criterion) {
    let mut group = c.benchmark_group("QueryService::run/topk_fusion");
    for &n in SORT_SWEEP_SIZES {
        let index = create_page_index(n);
        group.throughput(Throughput::Elements(
            u64::try_from(n).expect("note count fits u64"),
        ));
        group.bench_with_input(
            BenchmarkId::new("full_sort_no_limit", n),
            &n,
            |b, _| {
                b.iter_batched(
                    || index.clone(),
                    |index| {
                        QueryService::new("class").run(
                            &index,
                            QueryBuilder::pages(SourceSelector::All)
                                .sort("rating", false)
                                .expect("valid sort"),
                        )
                    },
                    BatchSize::SmallInput,
                );
            },
        );
        group.bench_with_input(
            BenchmarkId::new("topk_limit_10", n),
            &n,
            |b, _| {
                b.iter_batched(
                    || index.clone(),
                    |index| {
                        QueryService::new("class").run(
                            &index,
                            QueryBuilder::pages(SourceSelector::All)
                                .sort("rating", false)
                                .expect("valid sort")
                                .limit(10)
                                .expect("valid limit"),
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
/// Expected outcomes:
/// - Shuffle cost is a small fraction of sort-only cost at every size, ruling
///   out element-move as the dominant sort component.
///
/// Unexpected outcomes:
/// - Shuffle cost comparable to sort-only cost, indicating element-move
///   dominates and `QueryRow` size should be reduced.
fn bench_permute_query_rows(c: &mut Criterion) {
    let mut group = c.benchmark_group("QueryService::run/permute_records");
    for &n in SORT_SWEEP_SIZES {
        let index = create_page_index(n);
        let base: Vec<QueryRow> = {
            let set = QueryService::new("class")
                .run(&index, QueryBuilder::pages(SourceSelector::All));
            (0..set.len())
                .map(|i| set.get(i).expect("row present").clone())
                .collect()
        };
        group.throughput(Throughput::Elements(
            u64::try_from(n).expect("note count fits u64"),
        ));
        group.bench_with_input(
            BenchmarkId::new("fisher_yates_shuffle", n),
            &n,
            |b, _| {
                b.iter_batched(
                    || base.clone(),
                    |mut records| {
                        let mut state = 0x853c_49e6_748f_ea9b_u64;
                        lcg_shuffle(&mut records, &mut state);
                        black_box(records);
                    },
                    BatchSize::LargeInput,
                );
            },
        );
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
    for &n in SORT_SWEEP_SIZES {
        group.throughput(Throughput::Elements(
            u64::try_from(n).expect("note count fits u64"),
        ));
        group.bench_with_input(
            BenchmarkId::new("total_cmp_sort", n),
            &n,
            |b, &n| {
                b.iter_batched(
                    || shuffled_ratings(n),
                    |mut keys| {
                        keys.sort_by(f64::total_cmp);
                        black_box(keys);
                    },
                    BatchSize::SmallInput,
                );
            },
        );
    }
    group.finish();
}

/// Measures comparator dispatch cost on `NoteFieldValue` values with a replica
/// of `SortKey::total_cmp`'s shape, swept over workspace size.
///
/// The production comparator, `SortKey::total_cmp`, is `pub(crate)` in
/// `src/query/sort.rs` and unreachable from an external bench crate, so this
/// replicates the exact arm structure the Number-vs-Number path exercises
/// (enum `match` on both operands, then `f64::total_cmp`, with the
/// `descending` branch) against real
/// `NoteFieldValue` values. It measures what a comparator of this shape costs,
/// not the production function itself; conclusions must treat it as a
/// shape-equivalent upper bound on dispatch cost. Swept over the same sizes as
/// [`bench_sort_by_metadata`] (not a single point) so dispatch overhead can be
/// checked against the f64 floor at each `n`.
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
    let descending = false;
    for &n in SORT_SWEEP_SIZES {
        group.throughput(Throughput::Elements(
            u64::try_from(n).expect("note count fits u64"),
        ));
        group.bench_with_input(
            BenchmarkId::new("enum_dispatch_replica", n),
            &n,
            |b, &n| {
                b.iter_batched(
                    || {
                        shuffled_ratings(n)
                            .into_iter()
                            .map(NoteFieldValue::Number)
                            .collect::<Vec<_>>()
                    },
                    |mut keys| {
                        keys.sort_by(|lhs, rhs| {
                            replica_cmp(lhs, rhs, descending)
                        });
                        black_box(keys);
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
    bench_sort_by_metadata,
    bench_topk_vs_full_sort,
    bench_permute_query_rows,
    bench_sort_f64_floor,
    bench_sort_note_field_value_replica
);
criterion_main!(benches);
