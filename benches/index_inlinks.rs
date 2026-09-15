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
//! [Parsed Notes + Files] ──(InlinkMap::new)──► [InlinkMap]
//! [InlinkMap] ──(.inlinks_of)──► [Box<[PathBuf]>]
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

#![expect(
    clippy::expect_used,
    reason = "bench fixture/harness code; a failed .expect() here means the \
              fixture itself is broken and should panic immediately"
)]

use std::{hint::black_box, path::Path};

use criterion::{
    AxisScale, BenchmarkId, Criterion, PlotConfiguration, Throughput,
    criterion_group, criterion_main,
};
use traces_pkm::InlinkMap;

#[expect(
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

const SAME_STEM_CANDIDATE_COUNTS: &[usize] = &[2, 10, 100];

// ----------------------------------------------------------- //
//               Benchmarks: InlinkMap Compilation             //
// ----------------------------------------------------------- //

/// Measures in-memory link graph compilation across workspace sizes and link
/// topologies.
///
/// Parameters: varies [`WORKSPACE_FILE_COUNTS`] and topology (`sparse`,
/// `dense`, `single_hub`, `duplicate_links`, `deep_paths`, `ambiguous`,
/// `attachments`); reports note throughput.
///
/// Fixture: parsed notes and matching file records are built outside timing;
/// timed work is only [`InlinkMap::new`] resolution, global edge sort/dedup,
/// and grouping.
///
/// Expected outcomes:
/// - Cost tracks edge count for each topology; the global edge sort may add `E
///   log E` shape without candidate-resolution blowups.
/// - Ambiguous same-stem resolution stays bounded by candidate count, not by a
///   full workspace scan per link.
///
/// Unexpected outcomes:
/// - Dense, duplicate, deep-path, or ambiguous graphs grow beyond their edge
///   count plus sort cost, indicating redundant allocation, repeated candidate
///   scans, or pathological tie-breaking loops.
fn bench_inlink_map_new(c: &mut Criterion) {
    let mut group = c.benchmark_group("InlinkMap::new");
    group.plot_config(
        PlotConfiguration::default().summary_scale(AxisScale::Logarithmic),
    );
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

/// Measures same-stem resolution cost across candidate target counts while full
/// map construction cost is held fixed.
///
/// Parameters: holds note count at 10,000; varies same-stem candidates in
/// [`SAME_STEM_CANDIDATE_COUNTS`]; reports wall-clock time and note throughput.
///
/// Fixture: ambiguous-target notes and matching file records are built outside
/// timing; timed work constructs the whole [`InlinkMap`], including resolver
/// creation, all source resolution, global edge sort/dedup, and grouping.
///
/// Expected outcomes:
/// - The incremental delta from 2 to 10 to 100 candidates stays proportional to
///   candidate count on top of the fixed full-map construction cost.
///
/// Unexpected outcomes:
/// - Cost jumps disproportionately from 10 to 100 candidates, indicating
///   repeated candidate scans or workspace-wide lookup inside tie-breaking.
fn bench_inlink_map_collision_candidates(c: &mut Criterion) {
    let mut group = c.benchmark_group("InlinkMap::new/collisions");
    let n = 10_000_usize;
    group.sample_size(25);
    group.throughput(Throughput::Elements(
        u64::try_from(n).expect("note count fits u64"),
    ));

    for &candidates in SAME_STEM_CANDIDATE_COUNTS {
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

/// Measures point-lookup and iteration performance over one fixed assembled
/// inlink graph.
///
/// Parameters: holds a 1,000-note dense-link graph fixed (20 outgoing links per
/// note); varies hit/miss `inlinks_of`, hit/miss `has_target`, target-row
/// iteration, and source-edge counting.
///
/// Fixture: parsed notes, file records, and [`InlinkMap`] are built outside
/// timing; hit target is `note-500.md`, miss target is `nonexistent.md`.
///
/// Expected outcomes:
/// - Point lookups stay much cheaper than full traversal because they are one
///   `HashMap` lookup returning a borrowed slice or boolean.
/// - Target-row iteration visits map rows only; source-edge counting
///   additionally sums each row's borrowed source slice.
///
/// Unexpected outcomes:
/// - Point-lookup cost diverges between hits and misses or approaches traversal
///   cost, indicating unexpected hashing, path comparison, or allocation work.
/// - Target-row iteration approaches source-edge traversal, indicating row
///   iteration is cloning or flattening source slices.
/// - Source-edge counting grows expensive for this fixed map, indicating nested
///   re-walks or per-edge allocation.
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
            let found = inlinks.has_target(black_box(hub_target));
            black_box(found);
        });
    });

    group.bench_function("contains_target_miss", |b| {
        b.iter(|| {
            let found = inlinks.has_target(black_box(missing_target));
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
