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

use std::{
    alloc::System, fmt::Write as _, hint::black_box, path::Path, time::Duration,
};

use criterion::{Criterion, criterion_group, criterion_main};
use stats_alloc::{INSTRUMENTED_SYSTEM, Region, StatsAlloc};
use tempfile::TempDir;
use traces_pkm::{IndexerService, MarkdownParserInput, parse_markdown};
#[global_allocator]
static GLOBAL: &StatsAlloc<System> = &INSTRUMENTED_SYSTEM;

const ITEM_SIZES: &[usize] = &[10, 100, 1_000, 5_000];
const FIELD_SIZES: &[usize] = &[5, 20, 50, 200];
const BUILD_SIZES: &[usize] = &[10, 100, 1_000, 20_000];

/// Generates a synthetic Markdown note containing `n` top-level list items.
fn list_items_source(n: usize) -> String {
    let mut source = String::from("# List Items\n\n");
    for i in 0..n {
        let _ = writeln!(source, "- [ ] Item {i} for processing");
    }
    source
}

/// Generates a synthetic Markdown note containing `field_count` frontmatter
/// fields.
fn frontmatter_fields_source(field_count: usize) -> String {
    let mut source = String::from("---\n");
    for i in 0..field_count {
        let _ = writeln!(source, "field_{i}: \"value_{i}\"");
    }
    source.push_str("---\n\n# Body\nSimple note.\n");
    source
}

/// Prepares a temporary project containing `n` synthetic notes on disk.
fn create_temp_notes(n: usize) -> TempDir {
    let temp = tempfile::tempdir().expect("create temp dir");
    for i in 0..n {
        std::fs::write(
            temp.path().join(format!("note-{i}.md")),
            format!(
                "---\nrating: {}\n---\n\nBody text for note {i}.\n",
                i % 10
            ),
        )
        .expect("write fixture note");
    }
    temp
}

fn bench_note_construction_allocation(c: &mut Criterion) {
    let mut group = c.benchmark_group("memory/note_construction");
    group.sample_size(10);
    group.measurement_time(Duration::from_secs(1));
    group.warm_up_time(Duration::from_millis(500));
    for &n in ITEM_SIZES {
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

    for &n in FIELD_SIZES {
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
    for &n in BUILD_SIZES {
        let temp = create_temp_notes(n);
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
