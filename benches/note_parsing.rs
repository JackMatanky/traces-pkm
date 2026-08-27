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

/// Parses small, medium, and large synthetic notes through `parse_markdown`.
///
/// Every indexed note passes through this lexer (see module docs); scaling by
/// field/task density, not just byte count, catches a cost regression that a
/// correctness test — which only checks the parsed result — would miss.
///
/// Expected outcomes:
/// - Scaling is dominated by frontmatter field count and task count, not raw
///   byte length.
/// - Small notes remain sub-microsecond (trivial frontmatter, one paragraph).
///
/// Unexpected outcomes:
/// - Large-note parsing cost exceeds 10x medium-note cost, indicating quadratic
///   field scanning or unbounded allocation per task line.
fn bench_parse_markdown(c: &mut Criterion) {
    let mut group = c.benchmark_group("parse_markdown");
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
                b.iter(|| parse_markdown("note.md", source));
            },
        );
    }
    group.finish();
}

criterion_group!(benches, bench_parse_markdown);
criterion_main!(benches);
