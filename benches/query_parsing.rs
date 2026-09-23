//! Performance benchmark suite for query parsing.
//!
//! Exposes and monitors the CPU cost of parsing source selectors and filter
//! expressions via `QueryBuilder` and `SourceSelector`.
//!
//! ### Data Flow Diagram
//!
//! ```text
//! [String] ──(QueryBuilder::filter)──► [FilterExpression AST]
//! [String] ──(SourceSelector::parse)─► [SourceSelector AST]
//! ```
//!
//! ### Profiling Integration
//!
//! To profile query parsing CPU bottlenecks:
//! ```bash
//! cargo flamegraph --bench query_parsing -- --bench "QueryGrammar::parse"
//! ```
//!
//! Run via `mise run bench -f query_parsing` (or `mise run bench -m query`):
//! this crate's `test-utils`-gated public surface is only reachable with
//! `--features test-utils`, which the mise task supplies.

#![expect(
    clippy::expect_used,
    reason = "bench fixture/harness code; a failed .expect() here means the \
              fixture itself is broken and should panic immediately"
)]
use std::hint::black_box;

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use traces_pkm::{QueryBuilder, SourceSelector};
#[expect(
    dead_code,
    reason = "shared benchmark common helpers are compiled into each bench \
              target; this target uses the shared Criterion timing config"
)]
mod common;

// ----------------------------------------------------------- //
//                     Benchmarks: Parsing                     //
// ----------------------------------------------------------- //

/// Measures parsing latency for source selectors and filter expressions.
///
/// Parameters: varies `simple_filter`, `complex_boolean_filter`, and
/// `source_selector`; reports DSL input-byte throughput.
///
/// Fixture: static query strings are built outside timing. Timed work parses
/// the filter/source DSL and constructs the corresponding builder or selector
/// value; index traversal and row materialization are excluded.
///
/// Expected outcomes:
/// - Parsing remains small in absolute terms and stable for these fixed
///   strings.
///
/// Unexpected outcomes:
/// - Complex expressions costing disproportionately more than their byte
///   length, indicating lexer allocation or boolean-parser traversal needs
///   inspection.
fn bench_query_parsing(c: &mut Criterion) {
    let mut group = c.benchmark_group("QueryGrammar::parse");

    let simple_filter = "rating > 2";
    group.throughput(Throughput::Bytes(
        u64::try_from(simple_filter.len()).expect("byte length fits u64"),
    ));
    group.bench_function("simple_filter", |b| {
        b.iter(|| {
            let req = QueryBuilder::pages(SourceSelector::All)
                .filter(black_box(simple_filter))
                .expect("parse filter");
            black_box(req);
        });
    });

    let complex_filter = "(rating >= 4 and status == \"active\") or not \
                          contains(tags, \"archived\")";
    group.throughput(Throughput::Bytes(
        u64::try_from(complex_filter.len()).expect("byte length fits u64"),
    ));
    group.bench_function("complex_boolean_filter", |b| {
        b.iter(|| {
            let req = QueryBuilder::pages(SourceSelector::All)
                .filter(black_box(complex_filter))
                .expect("parse filter");
            black_box(req);
        });
    });

    let selector = "class(Book) or class(Article)";
    group.throughput(Throughput::Bytes(
        u64::try_from(selector.len()).expect("byte length fits u64"),
    ));
    group.bench_function("source_selector", |b| {
        b.iter(|| {
            let sel = SourceSelector::parse(black_box(selector))
                .expect("parse selector");
            black_box(sel);
        });
    });

    group.finish();
}

criterion_group! {
    name = benches;
    config = common::criterion_config();
    targets = bench_query_parsing
}
criterion_main!(benches);
