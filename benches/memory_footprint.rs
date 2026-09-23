//! Memory allocation benchmark suite.
//!
//! Reports gross allocated bytes and allocation counts for `parse_markdown`,
//! `IndexerService::build`, and persisted query/refresh paths.
//!
//! ### Data Flow Diagram
//!
//! ```text
//! [Markdown / Files] ──► [Instrumented System Allocator] ──► [Region Stats]
//! ```
//!
//! ### Profiling Integration
//!
//! To profile allocation CPU/memory bottlenecks:
//! ```bash
//! cargo flamegraph --bench memory_footprint -- --bench "memory/file_index_build/build_1000"
//! ```
//!
//! Run via `mise run bench -f memory_footprint` (or `mise run bench -m
//! memory_footprint`): this crate's `test-utils`-gated public surface is only
//! reachable with `--features test-utils`, which the mise task supplies.
#![expect(
    clippy::expect_used,
    reason = "bench fixture/harness code; reporting allocation statistics"
)]

use std::{alloc::System, hint::black_box, path::Path, time::Duration};

use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use stats_alloc::{INSTRUMENTED_SYSTEM, Region, StatsAlloc};
use traces_pkm::{
    IndexerService, MarkdownParserInput, QueryBuilder, QueryService,
    SourceSelector, parse_markdown,
};

#[expect(
    dead_code,
    reason = "shared benchmark common helpers are compiled into each bench \
              target; this target uses only allocation fixtures"
)]
mod common;

use common::{
    FRONTMATTER_FIELD_COUNTS, LIST_ITEM_COUNTS, WORKSPACE_FILE_COUNTS,
    content::{ProjectShape, frontmatter_fields_source, list_items_source},
    project::{
        build_index_arc, build_index_arc_from_note_source, create_project,
        setup_persisted_project,
    },
};
#[global_allocator]
static GLOBAL: &StatsAlloc<System> = &INSTRUMENTED_SYSTEM;

// ----------------------------------------------------------- //
//          Benchmarks: Note Construction Allocation           //
// ----------------------------------------------------------- //

/// Measures gross allocated bytes and allocation calls for `parse_markdown`,
/// printed to stderr for each list-item and frontmatter-field size.
///
/// Parameters: varies `LIST_ITEM_COUNTS` through `list_items_source` and
/// `FRONTMATTER_FIELD_COUNTS` through `frontmatter_fields_source`.
///
/// Fixture strings and [`MarkdownParserInput`] values are built outside both
/// the allocation region and Criterion timing loops.
///
/// Expected outcomes:
/// - Gross allocated bytes and allocation calls grow roughly proportionally
///   within each series.
///
/// Unexpected outcomes:
/// - A tier jumping disproportionately in bytes or allocation count, indicating
///   buffer duplication or per-item/per-field allocation in the parser.
fn bench_note_construction_allocation(c: &mut Criterion) {
    let mut group = c.benchmark_group("memory/note_construction");
    group.sample_size(10);
    group.measurement_time(Duration::from_secs(1));
    group.warm_up_time(Duration::from_millis(500));
    for &n in LIST_ITEM_COUNTS {
        let src = list_items_source(n);
        let path = Path::new("test.md");
        let input = MarkdownParserInput::for_test(path, &src);

        let region = Region::new(GLOBAL);
        let note = black_box(parse_markdown(&input));
        let stats = region.change();
        drop(note);

        eprintln!(
            "[memory] parse_markdown list_items({n}): gross {} bytes, {} \
             allocs",
            stats.bytes_allocated, stats.allocations
        );

        group.bench_function(format!("list_items_{n}"), |b| {
            b.iter(|| black_box(parse_markdown(&input)));
        });
    }

    for &n in FRONTMATTER_FIELD_COUNTS {
        let src = frontmatter_fields_source(n);
        let path = Path::new("test.md");
        let input = MarkdownParserInput::for_test(path, &src);

        let region = Region::new(GLOBAL);
        let note = black_box(parse_markdown(&input));
        let stats = region.change();
        drop(note);

        eprintln!(
            "[memory] parse_markdown frontmatter_fields({n}): gross {} bytes, \
             {} allocs",
            stats.bytes_allocated, stats.allocations
        );

        group.bench_function(format!("frontmatter_fields_{n}"), |b| {
            b.iter(|| black_box(parse_markdown(&input)));
        });
    }

    group.finish();
}

// ----------------------------------------------------------- //
//              Benchmarks: Index Build Footprint              //
// ----------------------------------------------------------- //

/// Measures gross allocated bytes and allocation calls for one
/// `IndexerService::build` over plain-project workspace sizes.
///
/// Parameters: varies note count; reports allocation bytes, allocation calls,
/// and sampled build latency for tiers up to 1,000 notes.
///
/// Fixture: temporary [`ProjectShape::Plain`] projects are created before the
/// allocation probe. The measured build includes filesystem scan/read, parsing,
/// inlink construction, and [`WorkspaceIndex`] assembly.
///
/// Expected outcomes:
/// - Allocation bytes and calls grow roughly linearly with note count.
///
/// Unexpected outcomes:
/// - Allocations growing super-linearly with note count, indicating duplicated
///   note storage or unbounded intermediate collections during indexing.
fn bench_file_index_footprint(c: &mut Criterion) {
    let mut group = c.benchmark_group("memory/file_index_build");
    group.sample_size(10);
    group.measurement_time(Duration::from_secs(1));
    group.warm_up_time(Duration::from_millis(500));
    for &n in WORKSPACE_FILE_COUNTS {
        let temp = create_project(n, ProjectShape::Plain);
        let indexer = IndexerService::new(temp.path());

        let region = Region::new(GLOBAL);
        let index = black_box(indexer.build().expect("build index"));
        let stats = region.change();
        drop(index);

        eprintln!(
            "[memory] WorkspaceIndex::build({n}): gross {} bytes, {} allocs",
            stats.bytes_allocated, stats.allocations
        );

        // For Criterion timing, only run for smaller sizes to keep bench run
        // reasonable
        if n <= 1_000 {
            group.bench_function(format!("build_{n}"), |b| {
                b.iter_with_large_drop(|| {
                    black_box(indexer.build().expect("build index"))
                });
            });
        }
    }

    group.finish();
}

// ----------------------------------------------------------- //
//              Benchmarks: Sync and Run Footprint             //
// ----------------------------------------------------------- //

/// Measures gross allocation cost across vault sizes for a narrow
/// [`QueryService::sync_and_run`] query vs. a full
/// [`WorkspaceIndex`]-materializing refresh.
///
/// Parameters: varies note count; holds one matching `#rare_0` tag selector
/// fixed; reports gross allocated bytes and allocation calls.
///
/// Fixture: each persisted tagged project contains exactly one `#rare_0` note;
/// project creation and persistence occur outside the measured regions.
///
/// Compares `sync_and_run` against [`IndexerService::refresh_with_report`] on a
/// no-op filesystem state: both include scan/sync work, while `sync_and_run`
/// resolves and decodes matching stored records and `refresh_with_report`
/// materializes every persisted file, note, and inlink.
///
/// Expected outcomes:
/// - One-match `sync_and_run` allocation stays below full-refresh allocation
///   and does not grow like every persisted note is decoded.
///
/// Unexpected outcomes:
/// - `sync_and_run`'s allocation count converging toward
///   `refresh_with_report`'s as `n` grows, indicating a full [`WorkspaceIndex`]
///   or full-note read snuck back into the store-query path.
fn bench_sync_and_run_footprint(c: &mut Criterion) {
    let mut group = c.benchmark_group("memory/sync_and_run");
    group.sample_size(10);
    group.measurement_time(Duration::from_secs(1));
    group.warm_up_time(Duration::from_millis(500));
    let service = QueryService::new("class");
    let one_match =
        || SourceSelector::parse("#rare_0").expect("valid tag selector");

    for &n in WORKSPACE_FILE_COUNTS {
        let (_temp, indexer) = setup_persisted_project(n, ProjectShape::Tagged);

        let query = QueryBuilder::pages(one_match());
        let region = Region::new(GLOBAL);
        let outcome = black_box(
            service
                .sync_and_run(&indexer, query)
                .expect("sync_and_run succeeds"),
        );
        let sync_stats = region.change();
        drop(outcome);

        let refresh_region = Region::new(GLOBAL);
        let (index, _report) =
            black_box(indexer.refresh_with_report().expect("refresh index"));
        let refresh_stats = refresh_region.change();
        drop(index);

        eprintln!(
            "[memory] sync_and_run/one-match({n}): gross {} bytes, {} allocs; \
             refresh_with_report({n}): gross {} bytes, {} allocs",
            sync_stats.bytes_allocated,
            sync_stats.allocations,
            refresh_stats.bytes_allocated,
            refresh_stats.allocations,
        );

        if n <= 1_000 {
            group.bench_function(format!("one_match_{n}"), |b| {
                b.iter_batched(
                    || QueryBuilder::pages(one_match()),
                    |page_query| {
                        black_box(
                            service
                                .sync_and_run(&indexer, page_query)
                                .expect("sync_and_run succeeds"),
                        )
                    },
                    BatchSize::SmallInput,
                );
            });
        }
    }
    group.finish();
}

// ----------------------------------------------------------- //
//            Benchmarks: Query Execution Footprint            //
// ----------------------------------------------------------- //

const QUERY_FOOTPRINT_COUNTS: &[usize] = &[100, 1_000, 10_000];

/// Measures gross allocated bytes and allocation calls for query execution
/// paths.
///
/// Parameters: sweeps `{100, 1_000, 10_000}` notes for unfiltered page
/// selection and metadata-sorted selection (`sort("rating")`).
///
/// Fixture: [`ProjectShape::Plain`] in-memory indexes are built outside both
/// the allocation region and timing loops.
///
/// Expected outcomes:
/// - `pages` allocation scales linearly with selected row count ($O(n)$).
/// - `sort` adds indexed-permutation vector allocation and comparator buffers
///   proportional to $n$.
///
/// Unexpected outcomes:
/// - Sorting allocating super-linearly or cloning row bodies during comparator
///   evaluation.
fn bench_query_execution_footprint(c: &mut Criterion) {
    let mut group = c.benchmark_group("memory/query_execution");
    group.sample_size(10);
    group.measurement_time(Duration::from_secs(1));
    group.warm_up_time(Duration::from_millis(500));
    let service = QueryService::new("class");

    for &n in QUERY_FOOTPRINT_COUNTS {
        let index = build_index_arc(n, ProjectShape::Plain);

        let region = Region::new(GLOBAL);
        let outcome = black_box(
            service.run(&index, QueryBuilder::pages(SourceSelector::All)),
        );
        let pages_stats = region.change();
        drop(outcome);

        let sort_query = QueryBuilder::pages(SourceSelector::All)
            .sort("rating", false)
            .expect("valid sort");
        let sort_region = Region::new(GLOBAL);
        let sorted_outcome = black_box(service.run(&index, sort_query));
        let sort_stats = sort_region.change();
        drop(sorted_outcome);

        eprintln!(
            "[memory] query_pages({n}): gross {} bytes, {} allocs; \
             query_pages_sorted({n}): gross {} bytes, {} allocs",
            pages_stats.bytes_allocated,
            pages_stats.allocations,
            sort_stats.bytes_allocated,
            sort_stats.allocations,
        );

        if n <= 1_000 {
            group.bench_function(format!("pages_{n}"), |b| {
                b.iter_batched(
                    || QueryBuilder::pages(SourceSelector::All),
                    |q| black_box(service.run(&index, q)),
                    BatchSize::SmallInput,
                );
            });
            group.bench_function(format!("pages_sorted_{n}"), |b| {
                b.iter_batched(
                    || {
                        QueryBuilder::pages(SourceSelector::All)
                            .sort("rating", false)
                            .expect("valid sort")
                    },
                    |q| black_box(service.run(&index, q)),
                    BatchSize::SmallInput,
                );
            });
        }
    }

    group.finish();
}

// ----------------------------------------------------------- //
//           Benchmarks: List Item Memory Footprint             //
// ----------------------------------------------------------- //

/// Total flat `ListItem` count asserted against the post-compaction memory
/// budget: 50,000 items in one in-memory `WorkspaceIndex`.
const LIST_ITEM_FOOTPRINT_COUNT: usize = 50_000;

/// Maximum allowed gross bytes allocated per `ListItem`. Measured baseline:
/// 152-byte `size_of::<ListItem>()` struct plus ~29 bytes of heap across two
/// allocations (~180 gross). Pre-compaction items cost over 450 bytes each;
/// this bound proves the required >50% reduction (ticket 12 triage notes).
const MAX_LIST_ITEM_FOOTPRINT_BYTES: usize = 220;

/// Measures gross bytes allocated per `ListItem` for 50,000 flat list items
/// in `WorkspaceIndex` via `Region::new(GLOBAL)`, asserting the required >50%
/// reduction over the pre-compaction (`>450` bytes/item) layout.
///
/// Parameters: fixed at [`LIST_ITEM_FOOTPRINT_COUNT`] items built through
/// [`list_items_source`], exercising the shared raw/clean text and sparse
/// inline fields path where `clean` is `None` and `fields` is `None`.
///
/// Fixture: [`build_index_arc_from_note_source`] parses the note in-memory
/// with zero disk I/O. The [`Region`] probe measures `index.as_ref().clone()`,
/// capturing the exact resident heap bytes requested by the `WorkspaceIndex`'s
/// entries and list items (struct layout + heap strings) while excluding
/// transient Markdown parser scratch buffers.
///
/// Expected outcomes:
/// - Gross memory per item stays under [`MAX_LIST_ITEM_FOOTPRINT_BYTES`] (~180
///   bytes/item: 152-byte struct plus ~29 heap bytes across two allocations),
///   validating the compacted layout and achieving a >55% reduction over the
///   pre-compaction >450 bytes/item layout.
///
/// Unexpected outcomes:
/// - Memory per item exceeding the budget, indicating a compaction regression:
///   empty inline field maps allocating a real `IndexMap`, duplicate raw/clean
///   text allocations, or a `ListItem` layout regression widening
///   `size_of::<ListItem>()`.
fn bench_list_items_memory_footprint(c: &mut Criterion) {
    let mut group = c.benchmark_group("memory/list_item_footprint");
    group.sample_size(10);
    group.measurement_time(Duration::from_secs(1));
    group.warm_up_time(Duration::from_millis(500));

    let index = build_index_arc_from_note_source(1, |_, _| {
        list_items_source(LIST_ITEM_FOOTPRINT_COUNT)
    });
    let region = Region::new(GLOBAL);
    let cloned = black_box(index.as_ref().clone());
    let stats = region.change();
    drop(cloned);

    let bytes_per_item = stats.bytes_allocated / LIST_ITEM_FOOTPRINT_COUNT;
    eprintln!(
        "[memory] list_item_footprint({LIST_ITEM_FOOTPRINT_COUNT}): gross {} \
         bytes, {} allocs, {bytes_per_item} bytes/item, size_of::<ListItem>() \
         = {}",
        stats.bytes_allocated,
        stats.allocations,
        std::mem::size_of::<traces_pkm::ListItem>(),
    );
    assert!(
        bytes_per_item < MAX_LIST_ITEM_FOOTPRINT_BYTES,
        "gross memory per ListItem ({bytes_per_item} bytes) exceeds the \
         {MAX_LIST_ITEM_FOOTPRINT_BYTES}-byte budget",
    );

    group.bench_function("clone_50000", |b| {
        b.iter_with_large_drop(|| black_box(index.as_ref().clone()));
    });

    group.finish();
}

criterion_group! {
    name = benches;
    config = common::criterion_config();
    targets = bench_note_construction_allocation, bench_file_index_footprint,
        bench_sync_and_run_footprint, bench_query_execution_footprint,
        bench_list_items_memory_footprint
}
criterion_main!(benches);
