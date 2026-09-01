//! Performance benchmark suite for query parsing.
//!
//! Exposes and monitors the CPU cost of parsing source selectors and filter
//! expressions via `QueryRequest` and `SourceSelector`.
//!
//! ### Data Flow Diagram
//!
//! ```text
//! [String] ──(QueryRequest::filter)──► [FilterExpression AST]
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
//! Run via `mise run bench`, not bare `cargo bench`: this crate's
//! `test-utils`-gated public surface is only reachable with
//! `--features test-utils`, which the mise task supplies.

#![expect(
    clippy::expect_used,
    reason = "bench fixture/harness code; a failed .expect() here means the \
              fixture itself is broken and should panic immediately"
)]
use std::hint::black_box;

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use traces_pkm::{QueryRequest, SourceSelector};

// ----------------------------------------------------------- //
//                     Benchmarks: Parsing                     //
// ----------------------------------------------------------- //

/// Measures query parsing latency for source selectors and filter expressions.
///
/// Isolates the tokenizer and boolean expression parser from index traversal
/// and row materialization.
///
/// Expected outcomes:
/// - Parsing cost is negligible relative to execution benchmarks.
///
/// Unexpected outcomes:
/// - Parsing cost comparable to execution benchmarks, indicating tokenizer or
///   parser regressions.
fn bench_query_parsing(c: &mut Criterion) {
    let mut group = c.benchmark_group("QueryGrammar::parse");

    let simple_filter = "rating > 2";
    group.throughput(Throughput::Bytes(
        u64::try_from(simple_filter.len()).expect("byte length fits u64"),
    ));
    group.bench_function("simple_filter", |b| {
        b.iter(|| {
            let req = QueryRequest::pages(SourceSelector::All)
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
            let req = QueryRequest::pages(SourceSelector::All)
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

criterion_group!(benches, bench_query_parsing);
criterion_main!(benches);
