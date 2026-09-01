//! Performance benchmark suite for query execution.
//!
//! Exposes and monitors the CPU cost of [`QueryService::execute`] over
//! pre-built page and task indexes.
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
use std::{cmp::Ordering, hint::black_box, sync::Arc};

use criterion::{
    BatchSize, BenchmarkId, Criterion, Throughput, criterion_group,
    criterion_main,
};
use traces_pkm::{
    FileIndex, IndexerService, NoteFieldValue, QueryRecord, QueryRequest,
    QueryService, SourceSelector,
};

// ----------------------------------------------------------- //
//                     Fixtures & Helpers                      //
// ----------------------------------------------------------- //

const WORKSPACE_SIZES: &[usize] = &[100, 1_000, 10_000, 20_000];
const FIELD_COUNTS: &[usize] = &[1, 5, 10, 20];
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

fn create_task_index(n: usize) -> Arc<FileIndex> {
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

/// Builds an index where each note has `fields` distinct frontmatter keys
/// before a trailing `rating` key, isolating whether metadata field lookup
/// scales with the number of fields per note (an O(K) linear scan) or stays
/// flat (an O(1) hash lookup). `rating` is always written last, the worst
/// case for a linear scan and irrelevant to a hash lookup, so this benchmark
/// is maximally sensitive to a regression back toward scanning.
fn create_index_with_field_count(n: usize, fields: usize) -> Arc<FileIndex> {
    use std::fmt::Write as _;

    let temp = tempfile::tempdir().expect("create temp dir");
    for i in 0..n {
        let mut frontmatter = String::from("---\n");
        for f in 0..fields {
            let _ = writeln!(frontmatter, "field_{f}: \"value\"");
        }
        let _ = writeln!(frontmatter, "rating: {}", i % 100);
        frontmatter.push_str("---\n");
        std::fs::write(temp.path().join(format!("note-{i}.md")), frontmatter)
            .expect("write fixture note");
    }
    Arc::new(IndexerService::new(temp.path()).build().expect("build index"))
}

/// Deterministic Linear Congruential Generator (LCG): `state = state * a + c`
/// mod 2^64. One multiply and one add per element — no dependency on `rand`,
/// reproducible across runs so regression detection isn't confounded by
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
//                Benchmarks: General Execution                //
// ----------------------------------------------------------- //

/// Measures page-row construction and filtering, swept over workspace size.
///
/// Every `traces query` invocation pays this path — regressions here directly
/// degrade CLI responsiveness — so isolating page queries catches regressions
/// in filter logic or row materialization that a correctness test would miss.
///
/// Expected outcomes:
/// - Constant-time execution regardless of index size (all notes match).
///
/// Unexpected outcomes:
/// - Linear or worse scaling with note count, indicating unindexed scans or
///   redundant allocation per row.
fn bench_execute_pages(c: &mut Criterion) {
    let mut group = c.benchmark_group("QueryService::execute");
    for &n in WORKSPACE_SIZES {
        let index = create_page_index(n);
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

/// Measures task-row construction, swept over workspace size, with three tasks
/// per note.
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
    for &n in WORKSPACE_SIZES {
        let index = create_task_index(n);
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

// ----------------------------------------------------------- //
//              Benchmarks: Combined Filter+Sort               //
// ----------------------------------------------------------- //

/// Measures page-row filtering and sorting by frontmatter metadata combined,
/// swept over workspace size.
///
/// Distinct from [`bench_execute_pages`], which never touches a
/// `FieldPath::Metadata`/`Tags` field and would not catch regressions in
/// per-record Note metadata resolution. Combining filter and sort here cannot
/// attribute a regression to either one; see
/// [`bench_filter_by_metadata_field_count`] and [`bench_sort_by_metadata`] for
/// the isolated halves.
///
/// Measured finding (20,000 rows): sort accounts for the large majority of this
/// benchmark's cost (~4.7 ms of ~5.2 ms combined), not field lookup (~1.8 ms
/// filter-only, ~141 µs unfiltered baseline) — `sort_by_cached_key` over 20,000
/// `QueryRecord`s dominates, not metadata resolution.
///
/// Expected outcomes:
/// - Cost tracks [`bench_sort_by_metadata`]'s sort-only cost plus
///   [`bench_filter_by_metadata_field_count`]'s filter-only cost, not a
///   disproportionate combination of the two.
///
/// Unexpected outcomes:
/// - Cost exceeding the sum of the isolated halves, indicating the combined
///   path introduces overhead not present in either half alone.
fn bench_execute_pages_by_metadata(c: &mut Criterion) {
    let mut group = c.benchmark_group("QueryService::execute");
    for &n in WORKSPACE_SIZES {
        let index = create_page_index(n);
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

// ----------------------------------------------------------- //
//                 Benchmarks: Isolated Filter                 //
// ----------------------------------------------------------- //

/// Measures filter-only cost by frontmatter field count per note, isolated
/// from sorting, at a fixed 20,000-note workspace size.
///
/// `FieldPath::parse` (the crate-internal query field-path parser)
/// canonicalizes a query's metadata field name once at parse time, not per row:
/// every row a query touches calls `Frontmatter::get`/`Note::get` with an
/// already-canonical candidate, so the allocating canonicalize-on-mismatch
/// fallback inside metadata field lookup is unreachable from the query engine —
/// it only matters for direct `Frontmatter`/`Note` callers outside a query,
/// already covered by `src/field.rs`'s own unit tests. Do not add a
/// "case-mismatched query candidate" benchmark here expecting it to exercise
/// that fallback: it cannot, by construction.
///
/// Distinct from [`bench_execute_pages_by_metadata`], which combines filter
/// and sort and never varies field count, so it cannot distinguish a filter
/// regression from a sort regression, or an O(K)-scan regression from a flat
/// O(1) lookup at any field count.
///
/// Expected outcomes:
/// - Flat cost across field counts: metadata lookup is O(1) (hash-keyed), not
///   O(K) (linear-scanned).
///
/// Unexpected outcomes:
/// - Cost growing with field count indicates a regression back to an O(K)
///   linear scan per lookup.
fn bench_filter_by_metadata_field_count(c: &mut Criterion) {
    let mut group =
        c.benchmark_group("QueryService::execute/filter_by_field_count");
    let n = 20_000_usize;
    for &fields in FIELD_COUNTS {
        let index = create_index_with_field_count(n, fields);
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
                        QueryService::new("class").execute(
                            &index,
                            QueryRequest::pages(SourceSelector::All)
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
//                  Benchmarks: Isolated Sort                  //
// ----------------------------------------------------------- //

/// Measures sort-only cost by frontmatter metadata, swept over workspace size.
///
/// Isolated from [`bench_execute_pages_by_metadata`]'s combined measurement so
/// a regression in `sort_by_cached_key`'s comparison or permutation cost is
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
    let mut group = c.benchmark_group("QueryService::execute/sort_by_metadata");
    for &n in SORT_SWEEP_SIZES {
        let index = create_page_index(n);
        group.throughput(Throughput::Elements(
            u64::try_from(n).expect("note count fits u64"),
        ));
        group.bench_with_input(BenchmarkId::new("sort_only", n), &n, |b, _| {
            b.iter_batched(
                || index.clone(),
                |index| {
                    QueryService::new("class").execute(
                        &index,
                        QueryRequest::pages(SourceSelector::All)
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
//               Benchmarks: Sort Decomposition                //
// ----------------------------------------------------------- //

/// Measures bare `QueryRecord` move/permutation cost, isolated from all
/// comparison and field resolution.
///
/// Decomposition of [`bench_sort_by_metadata`]'s 4.7 ms: if a full
/// Fisher-Yates shuffle (n moves of `QueryRecord`, each carrying its
/// `Arc<FileIndex>` + `RowIndex` + overlay fields) costs a small fraction of
/// the real sort, element-move cost is ruled out as the dominant component and
/// the cost must live in the comparator or key materialization.
///
/// Records are produced through the public query API (`execute` then
/// `QueryRecordSet::get` + clone); no internals are reached.
///
/// Expected outcomes:
/// - Shuffle cost is a small fraction of sort-only cost, ruling out
///   element-move as the dominant sort component.
///
/// Unexpected outcomes:
/// - Shuffle cost comparable to sort-only cost, indicating element-move
///   dominates and `QueryRecord` size should be reduced.
fn bench_permute_query_records(c: &mut Criterion) {
    let mut group = c.benchmark_group("QueryService::execute/permute_records");
    let n = 20_000_usize;
    let index = create_page_index(n);
    let base: Vec<QueryRecord> = {
        let set = QueryService::new("class")
            .execute(&index, QueryRequest::pages(SourceSelector::All));
        (0..set.len())
            .map(|i| set.get(i).expect("row present").clone())
            .collect()
    };
    group.throughput(Throughput::Elements(
        u64::try_from(n).expect("note count fits u64"),
    ));
    group.bench_function("fisher_yates_shuffle", |b| {
        b.iter_batched(
            || base.clone(),
            |mut records| {
                let mut state = 0x853c_49e6_748f_ea9b_u64;
                lcg_shuffle(&mut records, &mut state);
                black_box(records);
            },
            BatchSize::LargeInput,
        );
    });
    group.finish();
}

/// Measures the floor: sorting `n` bare `f64` keys with `total_cmp`.
///
/// Reference lower bound for `n·log n` comparisons with no enum dispatch, no
/// `SortKey` wrapping, and no `QueryRecord` permutation. The gap between this
/// floor and [`bench_sort_by_metadata`] is what the replica and permutation
/// benchmarks attribute.
///
/// Expected outcomes:
/// - Cost is lower than sort-only and replica benchmarks, confirming enum
///   dispatch and QueryRecord permutation add measurable overhead.
///
/// Unexpected outcomes:
/// - Cost matching sort-only or replica benchmarks, indicating overhead beyond
///   raw comparison dominates and the floor is not the bottleneck.
fn bench_sort_f64_floor(c: &mut Criterion) {
    let mut group = c.benchmark_group("QueryService::execute/sort_f64_floor");
    let n = 20_000_usize;
    group.throughput(Throughput::Elements(
        u64::try_from(n).expect("note count fits u64"),
    ));
    group.bench_function("total_cmp_sort", |b| {
        b.iter_batched(
            || shuffled_ratings(n),
            |mut keys| {
                keys.sort_by(f64::total_cmp);
                black_box(keys);
            },
            BatchSize::SmallInput,
        );
    });
    group.finish();
}

/// Measures comparator dispatch cost on `NoteFieldValue` values with a replica
/// of `sort_key_cmp`'s shape.
///
/// The production comparator is `pub(super)` in `src/query/sort.rs` and
/// unreachable from an external bench, so this replicates the exact arm
/// structure the Number-vs-Number path exercises (enum `match` on both
/// operands, then `f64::total_cmp`, with the `descending` branch) against real
/// `NoteFieldValue` values. It measures what a comparator of this shape costs —
/// not the production function itself; conclusions must treat it as a
/// shape-equivalent upper bound on dispatch cost.
///
/// Expected outcomes:
/// - Cost is close to the f64 floor, confirming enum dispatch overhead is small
///   relative to comparator + permutation cost.
///
/// Unexpected outcomes:
/// - Cost significantly exceeding the f64 floor, indicating enum dispatch in
///   the real comparator is a meaningful cost contributor.
fn bench_sort_note_field_value_replica(c: &mut Criterion) {
    let mut group =
        c.benchmark_group("QueryService::execute/sort_value_replica");
    let n = 20_000_usize;
    let descending = false;
    group.throughput(Throughput::Elements(
        u64::try_from(n).expect("note count fits u64"),
    ));
    group.bench_function("enum_dispatch_replica", |b| {
        b.iter_batched(
            || {
                shuffled_ratings(n)
                    .into_iter()
                    .map(NoteFieldValue::Number)
                    .collect::<Vec<_>>()
            },
            |mut keys| {
                keys.sort_by(|lhs, rhs| match (lhs, rhs) {
                    (NoteFieldValue::Number(x), NoteFieldValue::Number(y)) => {
                        if descending {
                            y.total_cmp(x)
                        } else {
                            x.total_cmp(y)
                        }
                    }
                    _ => Ordering::Equal,
                });
                black_box(keys);
            },
            BatchSize::SmallInput,
        );
    });
    group.finish();
}

// ----------------------------------------------------------- //
//                     Benchmarks: Parsing                     //
// ----------------------------------------------------------- //

/// Measures query parsing latency for source selectors and filter expressions.
///
/// Isolates the tokenizer and boolean expression parser from index traversal
/// and row materialization.
///
/// Expected outcomes:
/// - Parsing cost is negligible relative to execution benchmarks.
///
/// Unexpected outcomes:
/// - Parsing cost comparable to execution benchmarks, indicating tokenizer or
///   parser regressions.
fn bench_query_parsing(c: &mut Criterion) {
    let mut group = c.benchmark_group("QueryGrammar::parse");

    let simple_filter = "rating > 2";
    group.throughput(Throughput::Bytes(
        u64::try_from(simple_filter.len()).expect("byte length fits u64"),
    ));
    group.bench_function("simple_filter", |b| {
        b.iter(|| {
            let req = QueryRequest::pages(SourceSelector::All)
                .filter(black_box(simple_filter))
                .expect("parse filter");
            black_box(req);
        });
    });

    let complex_filter =
        "(rating >= 4 and status == 'active') or not tags.contains('archived')";
    group.throughput(Throughput::Bytes(
        u64::try_from(complex_filter.len()).expect("byte length fits u64"),
    ));
    group.bench_function("complex_boolean_filter", |b| {
        b.iter(|| {
            let req = QueryRequest::pages(SourceSelector::All)
                .filter(black_box(complex_filter))
                .expect("parse filter");
            black_box(req);
        });
    });

    let selector = "file_class(\"book\") or file_class(\"article\")";
    group.throughput(Throughput::Bytes(
        u64::try_from(selector.len()).expect("byte length fits u64"),
    ));
    group.bench_function("source_selector", |b| {
        b.iter(|| {
            let sel = SourceSelector::parse(black_box(selector))
                .expect("parse selector");
            black_box(sel);
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_execute_pages,
    bench_execute_tasks,
    bench_execute_pages_by_metadata,
    bench_filter_by_metadata_field_count,
    bench_sort_by_metadata,
    bench_permute_query_records,
    bench_sort_f64_floor,
    bench_sort_note_field_value_replica,
    bench_query_parsing
);
criterion_main!(benches);
