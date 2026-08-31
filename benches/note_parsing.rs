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
use std::hint::black_box;

use criterion::{
    BenchmarkId, Criterion, Throughput, criterion_group, criterion_main,
};
use traces_pkm::parse_markdown;

const SMALL: &str = "# Title\n\nA short note with one paragraph.\n";

fn medium() -> String {
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

fn large() -> String {
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

fn dense_wikilinks_and_tasks() -> String {
    use std::fmt::Write as _;

    let mut source = String::from("# Daily Log\n\n");
    for i in 0..50 {
        let _ = writeln!(
            source,
            "- [ ] Review [[topic-{i}|Topic {i}]] and verify \
             [[subtopic-{i}#section]]"
        );
    }
    source
}

/// Parses small, medium, and large synthetic notes through `parse_markdown`.
///
/// Every indexed note passes through this lexer (see module docs); scaling by
/// field/task density, not just byte count, catches a cost regression that a
/// correctness test — which only checks the parsed result — would miss.
fn bench_parse_markdown(c: &mut Criterion) {
    let mut group = c.benchmark_group("parse_markdown::size_scaling");
    let path = std::path::Path::new("note.md");

    for (label, source) in
        [("small", SMALL.to_owned()), ("medium", medium()), ("large", large())]
    {
        #[allow(
            clippy::as_conversions,
            reason = "usize→u64 is a lossless widening cast"
        )]
        group.throughput(Throughput::Bytes(source.len() as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(label),
            &source,
            |b, source| {
                b.iter(|| {
                    let note =
                        parse_markdown(black_box(path), black_box(source));
                    black_box(note);
                });
            },
        );
    }
    group.finish();
}

/// Measures parsing cost across varied real-world PKM document topologies:
/// code blocks, heavy frontmatter, and dense wikilink/task checklists.
fn bench_parse_markdown_workloads(c: &mut Criterion) {
    let mut group = c.benchmark_group("parse_markdown::workloads");
    let path = std::path::Path::new("note.md");

    let workloads = [
        ("prose_code", prose_with_code_blocks()),
        ("dense_frontmatter", dense_frontmatter()),
        ("dense_wikilinks_tasks", dense_wikilinks_and_tasks()),
    ];

    for (label, source) in workloads {
        #[allow(
            clippy::as_conversions,
            reason = "usize→u64 is a lossless widening cast"
        )]
        group.throughput(Throughput::Bytes(source.len() as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(label),
            &source,
            |b, source| {
                b.iter(|| {
                    let note =
                        parse_markdown(black_box(path), black_box(source));
                    black_box(note);
                });
            },
        );
    }
    group.finish();
}

criterion_group!(benches, bench_parse_markdown, bench_parse_markdown_workloads);
criterion_main!(benches);
