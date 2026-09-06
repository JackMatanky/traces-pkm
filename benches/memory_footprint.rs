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

use criterion::{Criterion, criterion_group, criterion_main};
use stats_alloc::{INSTRUMENTED_SYSTEM, Region, StatsAlloc};
use traces_pkm::{IndexerService, MarkdownParserInput, parse_markdown};

#[allow(
    dead_code,
    reason = "shared benchmark common helpers are compiled into each bench \
              target; this target uses only allocation fixtures"
)]
mod common;

use common::{
    FRONTMATTER_FIELD_COUNTS, LIST_ITEM_COUNTS, WORKSPACE_FILE_COUNTS,
    content::{ProjectShape, frontmatter_fields_source, list_items_source},
    project::create_project,
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
                b.iter(|| {
                    black_box(indexer.build().expect("build index"));
                });
            });
        }
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_note_construction_allocation,
    bench_file_index_footprint
);
criterion_main!(benches);
