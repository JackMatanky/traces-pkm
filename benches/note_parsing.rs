//! Performance benchmark suite for markdown/frontmatter parsing.
//!
//! Exposes and monitors the CPU cost of [`parse_markdown`], the crate's
//! markdown/frontmatter/task lexer. Every indexed note passes through this on
//! [`FileIndex::build`] and [`FileIndex::refresh`], so its cost sets a floor
//! under indexing throughput.
//!
//! ### Data Flow Diagram
//!
//! ```text
//! [raw markdown bytes] ──(parse_markdown)──► [Note { frontmatter, body, tasks }]
//! ```
//!
//! ### Profiling Integration
//!
//! To profile parsing CPU bottlenecks:
//! ```bash
//! cargo flamegraph --bench note_parsing -- --bench "parse_markdown/large"
//! ```
//!
//! Run via `mise run bench`, not bare `cargo bench`: this crate's
//! `test-utils`-gated public surface (`parse_markdown` included) is only
//! reachable with `--features test-utils`, which the mise task supplies.

#![expect(
    clippy::expect_used,
    clippy::arithmetic_side_effects,
    reason = "bench fixture/harness code; a failed .expect() here means the \
              fixture itself is broken and should panic immediately"
)]
use std::hint::black_box;

use criterion::{
    BenchmarkId, Criterion, Throughput, criterion_group, criterion_main,
};
use traces_pkm::{MarkdownParserInput, parse_markdown};

#[inline]
fn bench_parse(path: &std::path::Path, src: &str) -> traces_pkm::Note {
    let input = MarkdownParserInput::for_test(path, src);
    parse_markdown(&input)
}

// ----------------------------------------------------------- //
//                     Fixtures & Helpers                      //
// ----------------------------------------------------------- //

const SMALL: &str = "# Title\n\nA short note with one paragraph.\n";

fn medium_source() -> String {
    use std::fmt::Write as _;

    let mut source = String::from(
        "---\ntitle: Medium Note\ndraft: false\n---\n\n# Heading\n\n",
    );
    for i in 0..10 {
        let _ = writeln!(source, "Field{i}:: value {i}");
    }
    source.push_str("\n- [ ] task one\n- [x] task two\n  - nested item\n");
    source
}

fn large_source() -> String {
    use std::fmt::Write as _;

    let mut source =
        String::from("---\ntitle: Large Note\ntags: [a, b, c]\n---\n\n");
    for i in 0..100 {
        let _ = writeln!(source, "Field{i}:: value {i} [[link{i}]]");
    }
    for i in 0..50 {
        let _ = writeln!(source, "- [ ] task {i}");
    }
    source
}

fn prose_source_of_bytes(target_bytes: usize) -> String {
    use std::fmt::Write as _;

    let mut source = String::from("# Pure Prose Title\n\n");
    let mut i = 0;
    while source.len() < target_bytes {
        let _ = writeln!(
            source,
            "Paragraph {i} contains standard prose without frontmatter, \
             wikilinks, or list items."
        );
        i += 1;
    }
    source
}

fn prose_with_code_blocks() -> String {
    use std::fmt::Write as _;

    let mut source = String::from(
        "---\ntitle: Code Heavy Note\n---\n\n# Architecture Guide\n\n",
    );
    for i in 0..20 {
        let _ = writeln!(
            source,
            "Paragraph {i} explains module details and core concepts.\n"
        );
        let _ = writeln!(
            source,
            "```rust\nfn compute_{i}(val: usize) -> usize {{\n    val * 2 + \
             {i}\n}}\n```\n"
        );
    }
    source
}

fn dense_frontmatter() -> String {
    use std::fmt::Write as _;

    let mut source = String::from("---\ntitle: Metadata Dense\n");
    for i in 0..50 {
        let _ = writeln!(source, "field_{i}: \"value_{i}\"");
    }
    source.push_str(
        "tags:\n  - project\n  - active\n  - research\n---\n\n# Note \
         Body\nSimple body.\n",
    );
    source
}

fn dense_wikilinks_only() -> String {
    use std::fmt::Write as _;

    let mut source = String::from("# Document Outlinks\n\n");
    for i in 0..50 {
        let _ = writeln!(
            source,
            "Review [[topic-{i}|Topic {i}]] and verify \
             [[subtopic-{i}#section]]."
        );
    }
    source
}

fn dense_tasks_only() -> String {
    use std::fmt::Write as _;

    let mut source = String::from("# Task List\n\n");
    for i in 0..50 {
        let _ = writeln!(source, "- [ ] Task item {i} to be processed");
    }
    source
}

fn list_items_source(n: usize) -> String {
    use std::fmt::Write as _;

    let mut source = String::from("# List Items\n\n");
    for i in 0..n {
        let _ = writeln!(source, "- [ ] Item {i} for processing");
    }
    source
}

fn nested_items_source(total_items: usize, max_depth: u8) -> String {
    use std::fmt::Write as _;

    let mut source = String::from("# Nested List\n\n");
    let max_depth_usize = usize::from(max_depth);
    for i in 0..total_items {
        let depth = if max_depth_usize == 0 {
            0
        } else {
            i % max_depth_usize
        };
        let indent = "  ".repeat(depth);
        let _ = writeln!(source, "{indent}- [ ] Nested task item {i}");
    }
    source
}

fn line_density_source(target_bytes: usize, line_length: usize) -> String {
    let mut source = String::with_capacity(target_bytes);
    let chunk = "a".repeat(line_length.saturating_sub(1));
    while source.len() < target_bytes {
        source.push_str(&chunk);
        source.push('\n');
    }
    source
}

fn frontmatter_fields_source(field_count: usize) -> String {
    use std::fmt::Write as _;

    let mut source = String::from("---\n");
    for i in 0..field_count {
        let _ = writeln!(source, "field_{i}: \"value_{i}\"");
    }
    source.push_str("---\n\n# Body\nSimple note.\n");
    source
}

/// Builds a task-list note with `count` items whose markers cycle through
/// the default symbol set plus an unknown marker, exercising every marker
/// resolution path (`TaskStatusMap` hits and the incomplete-todo fallback).
fn marker_variety_source(count: usize) -> String {
    use std::fmt::Write as _;

    let symbols = [' ', 'x', 'X', '/', '-', '!', '?'];
    let mut source = String::from("# Marker Variety\n\n");
    for (i, symbol) in symbols.iter().cycle().take(count).enumerate() {
        let _ = writeln!(source, "- [{symbol}] Task {i}");
    }
    source
}

/// Builds a task-list note where every task carries one emoji shorthand date
/// and one inline field, exercising the `has_marker` lexer path.
fn task_metadata_source(count: usize) -> String {
    use std::fmt::Write as _;

    let mut source = String::from("# Task Metadata\n\n");
    for i in 0..count {
        let _ = writeln!(
            source,
            "- [ ] Task {i} 🗓2026-01-{:02} [priority:: high]",
            (i % 28) + 1
        );
    }
    source
}

// ----------------------------------------------------------- //
//                         Benchmarks                          //
// ----------------------------------------------------------- //

/// Measures pure prose parsing cost across byte sizes, serving as a baseline
/// subtracted from composite note parsing costs.
fn bench_parse_markdown_prose_floor(c: &mut Criterion) {
    let mut group = c.benchmark_group("parse_markdown::prose_floor");
    let path = std::path::Path::new("note.md");

    for (label, bytes) in [("1kb", 1_024), ("10kb", 10_240), ("100kb", 102_400)]
    {
        let source = prose_source_of_bytes(bytes);
        group.throughput(Throughput::Bytes(
            u64::try_from(source.len()).expect("byte length fits u64"),
        ));
        group.bench_with_input(
            BenchmarkId::from_parameter(label),
            &source,
            |b, source| {
                b.iter(|| {
                    let note = bench_parse(black_box(path), black_box(source));
                    black_box(note);
                });
            },
        );
    }
    group.finish();
}

/// Parses small, medium, and large synthetic notes through `parse_markdown`.
///
/// Every indexed note passes through this lexer (see module docs); scaling by
/// field/task density, not just byte count, catches a cost regression that a
/// correctness test — which only checks the parsed result — would miss.
///
/// Expected outcomes:
/// - Cost scales with note complexity, not just byte count.
///
/// Unexpected outcomes:
/// - Small notes costing disproportionately more than large, indicating fixed
///   per-call overhead dominating.
fn bench_parse_markdown(c: &mut Criterion) {
    let mut group = c.benchmark_group("parse_markdown::size_scaling");
    let path = std::path::Path::new("note.md");

    for (label, source) in [
        ("small", SMALL.to_owned()),
        ("medium", medium_source()),
        ("large", large_source()),
    ] {
        group.throughput(Throughput::Bytes(
            u64::try_from(source.len()).expect("byte length fits u64"),
        ));
        group.bench_with_input(
            BenchmarkId::from_parameter(label),
            &source,
            |b, source| {
                b.iter(|| {
                    let note = bench_parse(black_box(path), black_box(source));
                    black_box(note);
                });
            },
        );
    }
    group.finish();
}

/// Measures parsing cost across varied real-world PKM document topologies:
/// code blocks, heavy frontmatter, dense wikilinks, and isolated task
/// checklists.
///
/// Expected outcomes:
/// - Code-block-heavy notes parse faster than wikilink-heavy notes, since
///   wikilinks require per-link resolution.
///
/// Unexpected outcomes:
/// - Dense frontmatter parsing exceeding wikilink parsing, indicating
///   frontmatter lexer regression.
fn bench_parse_markdown_workloads(c: &mut Criterion) {
    let mut group = c.benchmark_group("parse_markdown::workloads");
    let path = std::path::Path::new("note.md");

    let workloads = [
        ("prose_code", prose_with_code_blocks()),
        ("dense_frontmatter", dense_frontmatter()),
        ("dense_wikilinks", dense_wikilinks_only()),
        ("dense_tasks", dense_tasks_only()),
    ];

    for (label, source) in workloads {
        group.throughput(Throughput::Bytes(
            u64::try_from(source.len()).expect("byte length fits u64"),
        ));
        group.bench_with_input(
            BenchmarkId::from_parameter(label),
            &source,
            |b, source| {
                b.iter(|| {
                    let note = bench_parse(black_box(path), black_box(source));
                    black_box(note);
                });
            },
        );
    }
    group.finish();
}

/// Measures parsing cost scaled by list item count `[10, 100, 1_000, 5_000]`.
///
/// Isolates per-item position-tracking overhead (`ByteTracker::byte_to_line`,
/// `ListItemPosition` construction) from prose/frontmatter bulk.
fn bench_parse_markdown_list_item_scaling(c: &mut Criterion) {
    let mut group = c.benchmark_group("parse_markdown::list_item_scaling");
    let path = std::path::Path::new("note.md");

    for count in [10_usize, 100, 1_000, 5_000] {
        let source = list_items_source(count);
        group.throughput(Throughput::Elements(
            u64::try_from(count).expect("count fits u64"),
        ));
        group.bench_with_input(
            BenchmarkId::from_parameter(count),
            &source,
            |b, source| {
                b.iter(|| {
                    let note = bench_parse(black_box(path), black_box(source));
                    black_box(note);
                });
            },
        );
    }
    group.finish();
}

/// Measures parsing cost across nesting depths for a fixed 200-item list.
///
/// Confirms that `ListItemPosition.parent` remains O(1) stack-top access
/// regardless of list nesting depth.
fn bench_parse_markdown_nesting_depth(c: &mut Criterion) {
    let mut group = c.benchmark_group("parse_markdown::nesting_depth");
    let path = std::path::Path::new("note.md");
    let total_items = 200_usize;

    for max_depth in [1_u8, 5, 20, 50] {
        let source = nested_items_source(total_items, max_depth);
        group.throughput(Throughput::Elements(
            u64::try_from(total_items).expect("item count fits u64"),
        ));
        group.bench_with_input(
            BenchmarkId::from_parameter(max_depth),
            &source,
            |b, source| {
                b.iter(|| {
                    let note = bench_parse(black_box(path), black_box(source));
                    black_box(note);
                });
            },
        );
    }
    group.finish();
}

/// Measures `ByteTracker`'s `match_indices('\n')` scan and line-start table
/// construction by varying line length at a fixed 50KB total document size.
fn bench_parse_markdown_line_density(c: &mut Criterion) {
    let mut group = c.benchmark_group("parse_markdown::line_density");
    let path = std::path::Path::new("note.md");
    let target_bytes = 51_200_usize;

    for line_length in [10_usize, 50, 200, 1_000] {
        let source = line_density_source(target_bytes, line_length);
        group.throughput(Throughput::Bytes(
            u64::try_from(source.len()).expect("byte length fits u64"),
        ));
        group.bench_with_input(
            BenchmarkId::from_parameter(line_length),
            &source,
            |b, source| {
                b.iter(|| {
                    let note = bench_parse(black_box(path), black_box(source));
                    black_box(note);
                });
            },
        );
    }
    group.finish();
}

/// Measures task-marker overhead against an identical plain-bullet list, and
/// across marker resolutions: plain bullets skip the scanner entirely, `- [
/// ]` hits the todo entry, mixed symbols cycle `TaskStatusMap` hits plus
/// the unknown-symbol todo fallback, and emoji/inline-field tasks add the
/// `has_marker` lexer pass.
///
/// Expected outcomes:
/// - Plain bullets are cheapest; marker overhead per item is small and roughly
///   constant across symbol kinds.
///
/// Unexpected outcomes:
/// - Task-marker items costing multiples of plain bullets, indicating the
///   per-chunk marker re-classification dominating.
fn bench_parse_markdown_task_marker_variants(c: &mut Criterion) {
    let mut group = c.benchmark_group("parse_markdown::task_marker_variants");
    let path = std::path::Path::new("note.md");
    let count = 1_000_usize;

    let mut mixed_markers = String::from("# Mixed Markers\n\n");
    {
        use std::fmt::Write as _;
        let symbols = [' ', 'x', 'X', '/', '-', '!', '?'];
        for (i, symbol) in symbols.iter().cycle().take(count).enumerate() {
            let _ = writeln!(mixed_markers, "- [{symbol}] Task {i}");
        }
    }

    let mut plain_bullets = String::from("# Plain Bullets\n\n");
    {
        use std::fmt::Write as _;
        for i in 0..count {
            let _ = writeln!(plain_bullets, "- Plain item {i}");
        }
    }

    let workloads = [
        ("plain_bullets", plain_bullets),
        ("plain_tasks", dense_tasks_only()),
        ("mixed_markers", marker_variety_source(count)),
        ("task_metadata", task_metadata_source(count)),
    ];

    for (label, source) in workloads {
        group.throughput(Throughput::Elements(
            u64::try_from(count).expect("count fits u64"),
        ));
        group.bench_with_input(
            BenchmarkId::from_parameter(label),
            &source,
            |b, source| {
                b.iter(|| {
                    let note = bench_parse(black_box(path), black_box(source));
                    black_box(note);
                });
            },
        );
    }
    group.finish();
}

/// Measures task-marker parsing cost scaled by marker count `[10, 100, 1_000,
/// 5_000]`.
///
/// Isolates the per-item leading-marker scan (per-chunk classification until
/// the marker decides) from prose/frontmatter bulk.
///
/// Expected outcomes:
/// - Cost scales linearly with marker count; per-marker overhead stays flat.
fn bench_parse_markdown_task_marker_scaling(c: &mut Criterion) {
    let mut group = c.benchmark_group("parse_markdown::task_marker_scaling");
    let path = std::path::Path::new("note.md");

    for count in [10_usize, 100, 1_000, 5_000] {
        let source = marker_variety_source(count);
        group.throughput(Throughput::Elements(
            u64::try_from(count).expect("count fits u64"),
        ));
        group.bench_with_input(
            BenchmarkId::from_parameter(count),
            &source,
            |b, source| {
                b.iter(|| {
                    let note = bench_parse(black_box(path), black_box(source));
                    black_box(note);
                });
            },
        );
    }
    group.finish();
}

/// Measures YAML frontmatter parsing cost scaled by field count `[5, 20, 50,
/// 200]`.
///
/// Isolates YAML field parsing from body text processing.
fn bench_parse_markdown_frontmatter_field_scaling(c: &mut Criterion) {
    let mut group =
        c.benchmark_group("parse_markdown::frontmatter_field_scaling");
    let path = std::path::Path::new("note.md");

    for count in [5_usize, 20, 50, 200] {
        let source = frontmatter_fields_source(count);
        group.throughput(Throughput::Elements(
            u64::try_from(count).expect("count fits u64"),
        ));
        group.bench_with_input(
            BenchmarkId::from_parameter(count),
            &source,
            |b, source| {
                b.iter(|| {
                    let note = bench_parse(black_box(path), black_box(source));
                    black_box(note);
                });
            },
        );
    }
    group.finish();
}

criterion_group!(
    benches,
    bench_parse_markdown_prose_floor,
    bench_parse_markdown,
    bench_parse_markdown_workloads,
    bench_parse_markdown_list_item_scaling,
    bench_parse_markdown_nesting_depth,
    bench_parse_markdown_line_density,
    bench_parse_markdown_frontmatter_field_scaling,
    bench_parse_markdown_task_marker_variants,
    bench_parse_markdown_task_marker_scaling
);
criterion_main!(benches);
