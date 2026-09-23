//! Performance benchmark suite for markdown/frontmatter parsing.
//!
//! Exposes and monitors the CPU cost of [`parse_markdown`], the crate's
//! markdown/frontmatter/task lexer. Every indexed note passes through this on
//! [`WorkspaceIndex::build`] and [`WorkspaceIndex::refresh`], so its cost sets
//! a floor under indexing throughput.
//!
//! ### Data Flow Diagram
//!
//! ```text
//! [raw markdown bytes]
//!   └──(parse_markdown)──► [Note]
//!                           ├── frontmatter
//!                           ├── body
//!                           └── tasks
//! ```
//!
//! ### Profiling Integration
//!
//! To profile parsing CPU bottlenecks:
//! ```bash
//! cargo flamegraph --bench note_parsing -- --bench "parse_markdown/large"
//! ```
//!
//! Run via `mise run bench -f note_parsing` (or `mise run bench -m note`): this
//! crate's `test-utils`-gated public surface (`parse_markdown` included) is
//! only reachable with `--features test-utils`, which the mise task supplies.

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
use traces_pkm::Note;

#[expect(
    dead_code,
    reason = "shared benchmark common helpers are compiled into each bench \
              target; this target uses only parser input fixtures"
)]
mod common;

use common::{
    FRONTMATTER_FIELD_COUNTS, LIST_ITEM_COUNTS,
    content::{frontmatter_fields_source, list_items_source},
    notes::parse_note,
};

// ----------------------------------------------------------- //
//                     Fixtures & Helpers                      //
// ----------------------------------------------------------- //

const NESTING_DEPTHS: &[u8] = &[1, 5, 20, 50];
const LINE_LENGTHS: &[usize] = &[10, 50, 200, 1_000, 2_000, 4_000];
#[inline]
fn parse_fixture(path: &std::path::Path, src: &str) -> Note {
    parse_note(path, src)
}

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

fn dense_frontmatter_dates() -> String {
    use std::fmt::Write as _;

    let mut source = String::from("---\ntitle: Date Dense\n");
    for i in 0..50 {
        let day = (i % 28) + 1;
        let month = (i % 12) + 1;
        let _ = writeln!(source, "date_{i}: 2026-{month:02}-{day:02}");
    }
    source.push_str("---\n\n# Dates Body\n");
    source
}

fn dense_frontmatter_durations() -> String {
    use std::fmt::Write as _;

    let mut source = String::from("---\ntitle: Duration Dense\n");
    for i in 0..50 {
        let _ =
            writeln!(source, "duration_{i}: {}h {}m", (i % 8) + 1, (i % 60));
    }
    source.push_str("---\n\n# Durations Body\n");
    source
}

fn dense_frontmatter_numbers() -> String {
    use std::fmt::Write as _;

    let mut source = String::from("---\ntitle: Number Dense\n");
    for i in 0..50 {
        let _ = writeln!(source, "num_{i}: {}", i * 42);
    }
    source.push_str("---\n\n# Numbers Body\n");
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

/// Builds a task-list note with `count` items whose markers cycle through the
/// default symbol set plus an unknown marker, exercising every marker
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

/// Measures prose-only parsing cost across byte sizes, serving as the parser's
/// byte-scanning floor for comparison with structured-note workloads.
///
/// Expected outcomes:
/// - Cost scales linearly with byte count (throughput roughly flat in bytes/s).
///
/// Unexpected outcomes:
/// - Super-linear scaling with byte count, indicating quadratic text scanning
///   or per-line allocation growth in the lexer.
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
                b.iter_with_large_drop(|| {
                    black_box(parse_fixture(black_box(path), black_box(source)))
                });
            },
        );
    }
    group.finish();
}

/// Measures `parse_markdown` cost across small, medium, and large synthetic
/// note shapes.
///
/// Parameters: varies note size and metadata/task density; reports byte
/// throughput.
///
/// Fixture: synthetic Markdown strings built outside timing; timed work parses
/// one note with [`parse_fixture`].
///
/// Compares against [`bench_parse_markdown_prose_floor`], isolating structured
/// field and task parsing from plain prose scanning.
///
/// Expected outcomes:
/// - Cost scales with note complexity and byte count, not fixed per-call
///   overhead.
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
                b.iter_with_large_drop(|| {
                    black_box(parse_fixture(black_box(path), black_box(source)))
                });
            },
        );
    }
    group.finish();
}

// ----------------------------------------------------------- //
//                Benchmarks: Parser Workloads                 //
// ----------------------------------------------------------- //

/// Measures parsing cost across varied real-world PKM document topologies: code
/// blocks, heavy frontmatter across value types (strings, ISO dates, duration
/// literals, integers), dense wikilinks, and isolated task checklists.
///
/// Expected outcomes:
/// - Code-block and dense-wikilink cases stay in the same cost class for
///   similar byte sizes; this parser extracts link syntax but does not resolve
///   targets.
/// - Frontmatter conversion costs remain comparable across dates, durations,
///   and numbers.
/// - Dense task lists cost more than prose but scale linearly with task count.
///
/// Unexpected outcomes:
/// - Specific frontmatter value types (such as dates or durations) dominating
///   parse time, indicating date/duration regex parsing or chrono/duration
///   allocation overhead.
/// - Dense task parsing dominating similarly sized workloads, indicating
///   repeated marker/status resolution or per-task allocation.
fn bench_parse_markdown_workloads(c: &mut Criterion) {
    let mut group = c.benchmark_group("parse_markdown::workloads");
    let path = std::path::Path::new("note.md");

    let workloads = [
        ("prose_code", prose_with_code_blocks()),
        ("dense_frontmatter", dense_frontmatter()),
        ("dense_frontmatter_dates", dense_frontmatter_dates()),
        ("dense_frontmatter_durations", dense_frontmatter_durations()),
        ("dense_frontmatter_numbers", dense_frontmatter_numbers()),
        ("dense_wikilinks", dense_wikilinks_only()),
        ("dense_tasks", list_items_source(50)),
    ];
    for (label, source) in workloads {
        group.throughput(Throughput::Bytes(
            u64::try_from(source.len()).expect("byte length fits u64"),
        ));
        group.bench_with_input(
            BenchmarkId::from_parameter(label),
            &source,
            |b, source| {
                b.iter_with_large_drop(|| {
                    black_box(parse_fixture(black_box(path), black_box(source)))
                });
            },
        );
    }
    group.finish();
}

// ----------------------------------------------------------- //
//                 Benchmarks: Item Scaling                    //
// ----------------------------------------------------------- //

/// Measures full-parse cost for top-level checkbox lists scaled by
/// [`LIST_ITEM_COUNTS`].
///
/// Parameters: varies item count; reports item throughput.
///
/// Fixture: `list_items_source(count)` is built outside timing and parsed at
/// fixed path `note.md`. Timed work is one full [`parse_fixture`] call.
///
/// Expected outcomes:
/// - Elapsed time grows with item/source size without a disproportionate
///   time-per-item jump across the sweep.
///
/// Unexpected outcomes:
/// - Time per item rises sharply with count, indicating repeated document
///   scans, list-position bookkeeping overhead, or avoidable per-item
///   allocation.
fn bench_parse_markdown_list_item_scaling(c: &mut Criterion) {
    let mut group = c.benchmark_group("parse_markdown::list_item_scaling");
    let path = std::path::Path::new("note.md");

    for &count in LIST_ITEM_COUNTS {
        let source = list_items_source(count);
        group.throughput(Throughput::Elements(
            u64::try_from(count).expect("count fits u64"),
        ));
        group.bench_with_input(
            BenchmarkId::from_parameter(count),
            &source,
            |b, source| {
                b.iter_with_large_drop(|| {
                    black_box(parse_fixture(black_box(path), black_box(source)))
                });
            },
        );
    }
    group.finish();
}

/// Measures full-parse sensitivity to list nesting depth at a fixed 200-item
/// count.
///
/// Parameters: varies [`NESTING_DEPTHS`]; reports item
/// throughput. Deeper fixtures also contain more indentation bytes.
///
/// Fixture: nested-list source is built outside timing and parsed at fixed path
/// `note.md`; timed work is one full [`parse_fixture`] call.
///
/// Expected outcomes:
/// - Depth-associated cost grows no faster than the added indentation and stack
///   bookkeeping should explain.
///
/// Unexpected outcomes:
/// - Cost rises disproportionately with depth, indicating nested-list stack,
///   position bookkeeping, or event handling is walking ancestor chains.
fn bench_parse_markdown_nesting_depth(c: &mut Criterion) {
    let mut group = c.benchmark_group("parse_markdown::nesting_depth");
    let path = std::path::Path::new("note.md");
    let total_items = 200_usize;

    for &max_depth in NESTING_DEPTHS {
        let source = nested_items_source(total_items, max_depth);
        group.throughput(Throughput::Elements(
            u64::try_from(total_items).expect("item count fits u64"),
        ));
        group.bench_with_input(
            BenchmarkId::from_parameter(max_depth),
            &source,
            |b, source| {
                b.iter_with_large_drop(|| {
                    black_box(parse_fixture(black_box(path), black_box(source)))
                });
            },
        );
    }
    group.finish();
}

/// Measures full-parser sensitivity to newline density by varying line length
/// while targeting at least `50 KiB` of source text.
///
/// Parameters: varies [`LINE_LENGTHS`]; reports actual
/// source-byte throughput. Fixture generation is outside timing.
///
/// Expected outcomes:
/// - Throughput stays in the same range after accounting for actual byte
///   length.
///
/// Unexpected outcomes:
/// - A material throughput drop correlated with line length, indicating line
///   tracking, Markdown event emission, or text buffering needs inspection.
fn bench_parse_markdown_line_density(c: &mut Criterion) {
    let mut group = c.benchmark_group("parse_markdown::line_density");
    let path = std::path::Path::new("note.md");
    let target_bytes = 51_200_usize;

    for &line_length in LINE_LENGTHS {
        let source = line_density_source(target_bytes, line_length);
        group.throughput(Throughput::Bytes(
            u64::try_from(source.len()).expect("byte length fits u64"),
        ));
        group.bench_with_input(
            BenchmarkId::from_parameter(line_length),
            &source,
            |b, source| {
                b.iter_with_large_drop(|| {
                    black_box(parse_fixture(black_box(path), black_box(source)))
                });
            },
        );
    }
    group.finish();
}

/// Measures task-marker overhead across four representative 1,000-item list
/// sources.
///
/// Parameters: compares `plain_bullets`, `plain_tasks`, `mixed_markers`, and
/// `task_metadata`; reports item throughput.
///
/// Fixture: sources are built outside timing and parsed at fixed path
/// `note.md`. Plain bullets take the early marker-rejection path; status-marked
/// tasks use marker-aware inline lexing; `task_metadata` also exercises emoji
/// shorthand dates and one inline field per item.
///
/// Expected outcomes:
/// - Status-marked workloads are slower than plain bullets, but the gap stays
///   bounded for these fixed 1,000-item fixtures.
///
/// Unexpected outcomes:
/// - Task-marker workloads costing multiples of plain bullets, indicating
///   marker classification, metadata lexing, or per-task allocation dominates.
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
        ("plain_tasks", list_items_source(count)),
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
                b.iter_with_large_drop(|| {
                    black_box(parse_fixture(black_box(path), black_box(source)))
                });
            },
        );
    }
    group.finish();
}

/// Measures full-parse cost for mixed task markers scaled by
/// [`LIST_ITEM_COUNTS`].
///
/// Parameters: varies marker/item count; reports element throughput. The marker
/// cycle covers the default symbols plus an unknown-symbol fallback.
///
/// Fixture: `marker_variety_source(count)` is built outside timing and parsed
/// at fixed path `note.md`.
///
/// Expected outcomes:
/// - Elapsed time grows with marker/source size without a disproportionate
///   time-per-marker jump across the sweep.
///
/// Unexpected outcomes:
/// - Time per marker rises sharply with count, indicating repeated
///   full-document marker scans, repeated lexer passes, or avoidable per-item
///   allocation.
fn bench_parse_markdown_task_marker_scaling(c: &mut Criterion) {
    let mut group = c.benchmark_group("parse_markdown::task_marker_scaling");
    let path = std::path::Path::new("note.md");

    for &count in LIST_ITEM_COUNTS {
        let source = marker_variety_source(count);
        group.throughput(Throughput::Elements(
            u64::try_from(count).expect("count fits u64"),
        ));
        group.bench_with_input(
            BenchmarkId::from_parameter(count),
            &source,
            |b, source| {
                b.iter_with_large_drop(|| {
                    black_box(parse_fixture(black_box(path), black_box(source)))
                });
            },
        );
    }
    group.finish();
}

// ----------------------------------------------------------- //
//              Benchmarks: Frontmatter Scaling                //
// ----------------------------------------------------------- //

/// Measures frontmatter field-width parsing cost scaled by
/// [`FRONTMATTER_FIELD_COUNTS`].
///
/// Parameters: varies YAML field count; reports field throughput. Source bytes
/// rise with field count.
///
/// Fixture: `frontmatter_fields_source(count)` is built outside timing and
/// parsed at fixed path `note.md`; the body is intentionally small but the full
/// Markdown parser still runs.
///
/// Expected outcomes:
/// - Time per field stays broadly stable across the field-count sweep.
///
/// Unexpected outcomes:
/// - Time per field rises sharply, indicating YAML deserialization,
///   frontmatter-field conversion, or map growth needs inspection.
fn bench_parse_markdown_frontmatter_field_scaling(c: &mut Criterion) {
    let mut group =
        c.benchmark_group("parse_markdown::frontmatter_field_scaling");
    let path = std::path::Path::new("note.md");

    for &count in FRONTMATTER_FIELD_COUNTS {
        let source = frontmatter_fields_source(count);
        group.throughput(Throughput::Elements(
            u64::try_from(count).expect("count fits u64"),
        ));
        group.bench_with_input(
            BenchmarkId::from_parameter(count),
            &source,
            |b, source| {
                b.iter_with_large_drop(|| {
                    black_box(parse_fixture(black_box(path), black_box(source)))
                });
            },
        );
    }
    group.finish();
}

criterion_group! {
    name = benches;
    config = common::criterion_config();
    targets = bench_parse_markdown_prose_floor, bench_parse_markdown,
        bench_parse_markdown_workloads, bench_parse_markdown_list_item_scaling,
        bench_parse_markdown_nesting_depth, bench_parse_markdown_line_density,
        bench_parse_markdown_frontmatter_field_scaling,
        bench_parse_markdown_task_marker_variants,
        bench_parse_markdown_task_marker_scaling
}
criterion_main!(benches);
