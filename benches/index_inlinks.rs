//! Performance benchmark suite for the inbound link (backlink) graph compiler.
//!
//! Exposes and monitors the execution cost of building and querying
//! [`InlinkMap`], isolating in-memory graph resolution from disk I/O and
//! database transactions.
//!
//! Backlinks are derived dynamically during index compilation and refresh.
//! Because every internal wikilink and Markdown link traverses resolution,
//! regressions in this compiler directly degrade vault indexing speed.
//!
//! ### Data Flow Diagram
//!
//! ```text
//! [Parsed Notes + Files] ──(InlinkMap::new)──► [InlinkMap] ──(.inlinks_of)──► [Box<[PathBuf]>]
//! ```
//!
//! ### Profiling Integration
//!
//! To profile inlink graph CPU bottlenecks:
//! ```bash
//! cargo flamegraph --bench index_inlinks -- --bench "InlinkMap::new/dense/1000"
//! ```
//!
//! Run via `mise run bench`, not bare `cargo bench`: this crate's
//! `test-utils`-gated public surface is only reachable with `--features
//! test-utils`.

#![allow(
    clippy::expect_used,
    clippy::arithmetic_side_effects,
    reason = "bench fixture/harness code uses deterministic arithmetic and \
              should panic immediately on broken fixtures"
)]

use std::{hint::black_box, path::Path};

use criterion::{
    BenchmarkId, Criterion, Throughput, criterion_group, criterion_main,
};
use traces_pkm::InlinkMap;

#[allow(
    dead_code,
    reason = "shared benchmark common helpers are compiled into each bench \
              target; this target uses only the in-memory inlink helpers"
)]
mod common;

use common::{
    WORKSPACE_FILE_COUNTS,
    notes::{
        file_records_for_notes, generate_ambiguous_target_link_notes,
        generate_attachment_link_notes, generate_deep_path_link_notes,
        generate_dense_link_notes, generate_duplicate_link_notes,
        generate_hub_link_notes, generate_sparse_link_notes,
    },
};

// ----------------------------------------------------------- //
//               Benchmarks: InlinkMap Compilation             //
// ----------------------------------------------------------- //

/// Measures in-memory link graph compilation across varying graph densities and
/// topologies.
///
/// Evaluates path resolution, stem index lookups, proximity tie-breaking,
/// duplicate-edge deduplication, hub source grouping, and attachment path
/// resolution in isolation from disk and database serialization overhead.
///
/// Expected outcomes:
/// - Linear or near-linear scaling for sparse, dense, hub, duplicate,
///   deep-path, and attachment graphs.
/// - Stable scaling under stem ambiguity without combinatorial degradation.
///
/// Unexpected outcomes:
/// - Super-linear scaling under dense, duplicate, deep-path, or ambiguous
///   graphs, indicating redundant allocations or pathological tie-breaking
///   loops.
fn bench_inlink_map_new(c: &mut Criterion) {
    let mut group = c.benchmark_group("InlinkMap::new");
    for &n in WORKSPACE_FILE_COUNTS {
        group.throughput(Throughput::Elements(
            u64::try_from(n).expect("note count fits u64"),
        ));
        if n >= 10_000 {
            // 10,000+ note dense graphs have 200,000+ links; bound total suite
            // runtime with criterion's minimum sample size (10).
            group.sample_size(10);
        }

        group.bench_with_input(BenchmarkId::new("sparse", n), &n, |b, &n| {
            let notes = generate_sparse_link_notes(n);
            let files = file_records_for_notes(&notes);
            b.iter_with_large_drop(|| {
                black_box(InlinkMap::new(black_box(&notes), black_box(&files)))
            });
        });

        group.bench_with_input(BenchmarkId::new("dense", n), &n, |b, &n| {
            let notes = generate_dense_link_notes(n);
            let files = file_records_for_notes(&notes);
            b.iter_with_large_drop(|| {
                black_box(InlinkMap::new(black_box(&notes), black_box(&files)))
            });
        });

        group.bench_with_input(
            BenchmarkId::new("single_hub", n),
            &n,
            |b, &n| {
                let notes = generate_hub_link_notes(n);
                let files = file_records_for_notes(&notes);
                b.iter_with_large_drop(|| {
                    black_box(InlinkMap::new(
                        black_box(&notes),
                        black_box(&files),
                    ))
                });
            },
        );

        group.bench_with_input(
            BenchmarkId::new("duplicate_links", n),
            &n,
            |b, &n| {
                let notes = generate_duplicate_link_notes(n, 20);
                let files = file_records_for_notes(&notes);
                b.iter_with_large_drop(|| {
                    black_box(InlinkMap::new(
                        black_box(&notes),
                        black_box(&files),
                    ))
                });
            },
        );

        group.bench_with_input(
            BenchmarkId::new("deep_paths", n),
            &n,
            |b, &n| {
                let notes = generate_deep_path_link_notes(n);
                let files = file_records_for_notes(&notes);
                b.iter_with_large_drop(|| {
                    black_box(InlinkMap::new(
                        black_box(&notes),
                        black_box(&files),
                    ))
                });
            },
        );

        group.bench_with_input(
            BenchmarkId::new("ambiguous", n),
            &n,
            |b, &n| {
                let notes = generate_ambiguous_target_link_notes(n, 10);
                let files = file_records_for_notes(&notes);
                b.iter_with_large_drop(|| {
                    black_box(InlinkMap::new(
                        black_box(&notes),
                        black_box(&files),
                    ))
                });
            },
        );

        group.bench_with_input(
            BenchmarkId::new("attachments", n),
            &n,
            |b, &n| {
                let (notes, files) = generate_attachment_link_notes(n);
                b.iter_with_large_drop(|| {
                    black_box(InlinkMap::new(
                        black_box(&notes),
                        black_box(&files),
                    ))
                });
            },
        );
    }
    group.finish();
}

// ----------------------------------------------------------- //
//             Benchmarks: Collision Candidate Count            //
// ----------------------------------------------------------- //

/// Measures same-stem resolution as the number of candidate targets grows.
///
/// Keeps total note count fixed and varies only the number of same-stem
/// candidates, isolating folder-proximity tie-breaking from workspace-size
/// scaling.
///
/// Expected outcomes:
/// - Cost grows with candidate count but stays bounded by the number of
///   same-stem candidates, not total workspace size.
///
/// Unexpected outcomes:
/// - Cost jumps disproportionately from 10 to 100 candidates, indicating
///   repeated full-workspace scans instead of stem-indexed candidate lookup.
fn bench_inlink_map_collision_candidates(c: &mut Criterion) {
    let mut group = c.benchmark_group("InlinkMap::new/collisions");
    let n = 10_000_usize;
    group.sample_size(10);
    group.throughput(Throughput::Elements(
        u64::try_from(n).expect("note count fits u64"),
    ));

    for candidates in [2_usize, 10, 100] {
        group.bench_with_input(
            BenchmarkId::new("same_stem_candidates", candidates),
            &candidates,
            |b, &candidates| {
                let notes = generate_ambiguous_target_link_notes(n, candidates);
                let files = file_records_for_notes(&notes);
                b.iter_with_large_drop(|| {
                    black_box(InlinkMap::new(
                        black_box(&notes),
                        black_box(&files),
                    ))
                });
            },
        );
    }
    group.finish();
}

// ----------------------------------------------------------- //
//               Benchmarks: InlinkMap Accessors               //
// ----------------------------------------------------------- //

/// Measures point-lookup and iteration performance over an assembled inlink
/// graph. Evaluates direct slice returns via [`InlinkMap::inlinks_of`],
/// presence tests via [`InlinkMap::contains_target`], target-row traversal via
/// [`InlinkMap::iter`], and full source-edge traversal over every inlink slice.
///
/// Expected outcomes:
/// - Sub-microsecond latency for point lookups with zero heap allocation.
/// - Target-row iteration scales with unique targets.
/// - Source-edge iteration scales with total inbound edges.
///
/// Unexpected outcomes: Microsecond or higher point-lookup latencies,
/// indicating unexpected hashing overhead or heap allocations.
fn bench_inlink_map_accessors(c: &mut Criterion) {
    let mut group = c.benchmark_group("InlinkMap::accessors");
    let n = 1_000;
    let notes = generate_dense_link_notes(n);
    let files = file_records_for_notes(&notes);
    let inlinks = InlinkMap::new(&notes, &files);

    let hub_target = Path::new("note-500.md");
    let missing_target = Path::new("nonexistent.md");

    group.bench_function("inlinks_of_hit", |b| {
        b.iter(|| {
            let sources = inlinks.inlinks_of(black_box(hub_target));
            black_box(sources);
        });
    });

    group.bench_function("inlinks_of_miss", |b| {
        b.iter(|| {
            let sources = inlinks.inlinks_of(black_box(missing_target));
            black_box(sources);
        });
    });

    group.bench_function("contains_target_hit", |b| {
        b.iter(|| {
            let found = inlinks.contains_target(black_box(hub_target));
            black_box(found);
        });
    });

    group.bench_function("contains_target_miss", |b| {
        b.iter(|| {
            let found = inlinks.contains_target(black_box(missing_target));
            black_box(found);
        });
    });

    group.bench_function("iter_target_rows", |b| {
        b.iter(|| {
            let count = inlinks.iter().count();
            black_box(count);
        });
    });

    group.bench_function("iter_source_edges", |b| {
        b.iter(|| {
            let count: usize =
                inlinks.iter().map(|(_, sources)| sources.len()).sum();
            black_box(count);
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_inlink_map_new,
    bench_inlink_map_collision_candidates,
    bench_inlink_map_accessors,
);
criterion_main!(benches);
