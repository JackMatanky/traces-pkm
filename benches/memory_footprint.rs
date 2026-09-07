//! Memory footprint and allocation benchmark suite.
//!
//! Measures net heap bytes and allocation counts for `parse_markdown` and
//! `IndexerService::build` across representative corpus sizes.
//!
//! ### Data Flow Diagram
//!
//! ```text
//! [Markdown / Files] ──► [Instrumented System Allocator] ──► [Region Stats]
//! ```
//!
//! Run via `cargo bench --bench memory_footprint --features test-utils`.

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

#[allow(
    dead_code,
    reason = "shared benchmark common helpers are compiled into each bench \
              target; this target uses only allocation fixtures"
)]
mod common;

use common::{
    FRONTMATTER_FIELD_COUNTS, LIST_ITEM_COUNTS, WORKSPACE_FILE_COUNTS,
    content::{ProjectShape, frontmatter_fields_source, list_items_source},
    project::{create_project, setup_persisted_project},
};
#[global_allocator]
static GLOBAL: &StatsAlloc<System> = &INSTRUMENTED_SYSTEM;

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
            "[memory] parse_markdown list_items({n}): net {} bytes, {} allocs",
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
            "[memory] parse_markdown frontmatter_fields({n}): net {} bytes, \
             {} allocs",
            stats.bytes_allocated, stats.allocations
        );

        group.bench_function(format!("frontmatter_fields_{n}"), |b| {
            b.iter(|| black_box(parse_markdown(&input)));
        });
    }

    group.finish();
}

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
            "[memory] FileIndex::build({n}): net {} bytes, {} allocs",
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

/// Compares net allocation cost of a cold, narrow store-scoped query
/// ([`QueryService::sync_and_run`]) against a full [`FileIndex`]
/// -materializing refresh ([`IndexerService::refresh_with_report`]) at
/// matching vault sizes.
///
/// `sync_and_run` queries a tag unique to note `0` (`#rare_0`, see
/// `tagged_note_source`), so exactly one note matches regardless of `n`.
/// `refresh_with_report` must decode and materialize every persisted note
/// into a `FileEntry`/`FileIndex` row; nothing changes on disk between the
/// two calls, so both take their respective no-op path.
///
/// Both allocation counts still grow with `n`: `scan()` and the
/// `FileBase`-diff step are O(vault size) by construction (there is no
/// filesystem-watcher layer here), and both paths pay that cost. What
/// differs is the decode step layered on top of it.
///
/// Expected outcomes:
/// - `sync_and_run` allocates roughly a third of `refresh_with_report`'s count
///   at every scale (observed: ~142k vs ~443k allocations at 20,000 files) - it
///   decodes and materializes the one matching `Note`, not all `n`.
///
/// Unexpected outcomes:
/// - `sync_and_run`'s allocation count converging toward
///   `refresh_with_report`'s as `n` grows, indicating a full `FileIndex` snuck
///   back into the cold-read path.
fn bench_sync_and_run_footprint(c: &mut Criterion) {
    let mut group = c.benchmark_group("memory/sync_and_run");
    group.sample_size(10);
    group.measurement_time(Duration::from_secs(1));
    group.warm_up_time(Duration::from_millis(500));
    let service = QueryService::new("class");
    let one_match =
        || SourceSelector::parse("#rare_0").expect("valid tag selector");

    for &n in WORKSPACE_FILE_COUNTS {
        let (temp, indexer) = setup_persisted_project(n, ProjectShape::Tagged);

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
        drop(temp);

        eprintln!(
            "[memory] sync_and_run/one-match({n}): net {} bytes, {} allocs; \
             refresh_with_report({n}): net {} bytes, {} allocs",
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

criterion_group!(
    benches,
    bench_note_construction_allocation,
    bench_file_index_footprint,
    bench_sync_and_run_footprint
);
criterion_main!(benches);
