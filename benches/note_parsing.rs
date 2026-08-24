//! Benches `traces_pkm::parse_markdown`, the crate's markdown/frontmatter/task
//! lexer. Every indexed base passes through this on `FileIndex::build` and
//! `FileIndex::refresh`, so its cost sets a floor under indexing throughput.
//!
//! Run via `mise run bench`, not bare `cargo bench`: this crate's
//! `test-utils`-gated public surface (`parse_markdown` included) is only
//! reachable with `--features test-utils`, which the mise task supplies.
use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
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
/// Every indexed base passes through this lexer (see module docs); scaling by
/// field/task density, not just byte count, catches a cost regression that a
/// correctness test — which only checks the parsed result — would miss.
fn bench_parse_markdown(c: &mut Criterion) {
    let mut group = c.benchmark_group("parse_markdown");
    for (label, source) in
        [("small", SMALL.to_owned()), ("medium", medium()), ("large", large())]
    {
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
