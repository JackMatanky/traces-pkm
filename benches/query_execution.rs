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
use std::{hint::black_box, sync::Arc};

use criterion::{
    BatchSize, BenchmarkId, Criterion, Throughput, criterion_group,
    criterion_main,
};
use traces_pkm::{
    FileIndex, IndexerService, QueryRecord, QueryRequest, QueryService,
    SourceSelector,
};

// ----------------------------------------------------------- //
//                     Fixtures & Helpers                      //
// ----------------------------------------------------------- //

const WORKSPACE_SIZES: &[usize] = &[100, 1_000, 10_000, 20_000];
const FIELD_COUNTS: &[usize] = &[1, 5, 10, 20];

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
//             Benchmarks: Template Chain Overhead             //
// ----------------------------------------------------------- //

/// Measures the cost of cloning a full `QueryRecordSet` (as a
/// `Vec<QueryRecord>`), swept over workspace size.
///
/// `src/template/engine/query.rs`'s `Object::call_method` for `QueryRecordSet`
/// clones the entire outcome (`self.as_ref().clone()`) on every non-terminal
/// chained call (`.where`/`.filter`/`.sort`/`.limit`/`.group_by`/`.flatten`),
/// so a template chain of `k` steps over `n` records pays `k` full clones of
/// (shrinking) size up to `n` — on top of each step's own transform cost, which
/// `bench_filter_by_metadata_field_count`/`bench_sort_by_metadata` already
/// measure. This isolates just the clone, the cost a "unified lazy query
/// engine" redesign (deferring materialization until a terminal call) would
/// remove. `QueryRecordSet`'s only field is `records: Vec<QueryRecord>` with a
/// derived `Clone`, so cloning the record set is exactly cloning this `Vec`:
/// this benchmark reaches it through the public API (`execute` +
/// `IntoIterator`) without depending on `QueryRecordSet` internals.
///
/// `QueryRecord` itself holds `Arc<FileIndex>` (an atomic refcount bump, not a
/// deep copy) + `RowIndex` (`Copy`) + an overlay `Vec` (empty for fresh page
/// rows) + a `RowKind` tag — so this also measures how close to "zero-copy"
/// cloning already is in practice: no `Note` data is duplicated.
///
/// Expected outcomes:
/// - Cost scales linearly with `n` (a `Vec` clone has no shortcut) and stays
///   small per element (no `Note` data is cloned, only `Arc` bumps).
///
/// Unexpected outcomes:
/// - Cost per element grows with `n` (would indicate allocator contention, not
///   itself expected for a single-threaded bench) or is large in absolute terms
///   (would indicate `QueryRecord` carries more owned data than its fields
///   suggest).
fn bench_clone_query_record_set(c: &mut Criterion) {
    let mut group = c.benchmark_group("QueryService::execute/clone_record_set");
    for &n in WORKSPACE_SIZES {
        let index = create_page_index(n);
        let records: Vec<QueryRecord> = QueryService::new("class")
            .execute(&index, QueryRequest::pages(SourceSelector::All))
            .into_iter()
            .collect();
        group.throughput(Throughput::Elements(
            u64::try_from(n).expect("note count fits u64"),
        ));
        group.bench_with_input(
            BenchmarkId::new("vec_clone", n),
            &records,
            |b, records| {
                b.iter(|| black_box(records.clone()));
            },
        );
    }
    group.finish();
}

criterion_group!(
    benches,
    bench_execute_pages,
    bench_execute_tasks,
    bench_execute_pages_by_metadata,
    bench_filter_by_metadata_field_count,
    bench_clone_query_record_set
);
criterion_main!(benches);
