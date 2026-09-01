//! Performance benchmark suite for the index lifecycle pipeline.
//!
//! Exposes and monitors the execution cost of indexing operations driven by
//! `IndexerService` (build, refresh, and persist) and the underlying link graph
//! compiler (`derive_inlinks`).
//!
//! This suite serves as a key guardian against performance regressions in the
//! write path of the personal knowledge base index. Because PKM queries must
//! refresh transparently on command execution, any overhead in these lifecycle
//! functions directly limits the responsiveness of the CLI.
//!
//! ### Data Flow Diagram
//!
//! ```text
//! [Files on Disk] ──(Scan)──► [FileBase / Notes] ──(derive_inlinks)──► [InlinkMap]
//!                                                                          │
//! [redb database] ◄──(Persist)─────────────────────────────────────────────┘
//! ```
//!
//! ### Profiling Integration
//!
//! To profile index lifecycle CPU bottlenecks:
//! ```bash
//! cargo flamegraph --bench index_lifecycle -- --bench
//! "FileIndex::refresh/no-op/1000"
//! ```
//!
//! Run via `mise run bench`, not bare `cargo bench`: this crate's
//! `test-utils`-gated public surface is only reachable with `--features
//! test-utils`.

#![expect(
    clippy::expect_used,
    clippy::arithmetic_side_effects,
    reason = "bench fixture/harness code; a failed .expect() here means the \
              fixture itself is broken and should panic immediately"
)]
use std::{fmt::Write as _, hint::black_box};

use criterion::{
    BatchSize, BenchmarkId, Criterion, Throughput, criterion_group,
    criterion_main,
};
use tempfile::TempDir;
use traces_pkm::{
    FileIndex, IndexerService, Note, derive_inlinks, parse_markdown,
};

// ----------------------------------------------------------- //
//                     Fixtures & Helpers                      //
// ----------------------------------------------------------- //

const BUILD_SIZES: &[usize] = &[10, 100, 1_000, 20_000];
const WORKSPACE_SIZES: &[usize] = &[100, 1_000, 20_000];

/// Creates a temporary project containing `n` synthetic notes.
///
/// The returned [`TempDir`] owns cleanup. Benchmark routines return the fixture
/// with their result, so Criterion drops it after timing.
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

/// Prepares a project directory with a populated and persisted index of `n`
/// notes.
fn setup_refresh(n: usize) -> (TempDir, IndexerService) {
    let temp = create_temp_notes(n);
    let indexer = IndexerService::new(temp.path());
    let index = indexer.build().expect("build index");
    indexer.persist(&index).expect("persist index");
    (temp, indexer)
}

/// Creates a temporary project containing `n` synthetic notes, each linking to
/// the next, mirroring `generate_notes_sparse`'s link shape but written to disk
/// so `IndexerService::build`/`refresh` parse and persist real outlinks — none
/// of this file's other fixtures produce any `LINKS` rows.
fn create_linked_notes(n: usize) -> TempDir {
    let temp = tempfile::tempdir().expect("create temp dir");
    for i in 0..n {
        std::fs::write(
            temp.path().join(format!("note-{i}.md")),
            format!(
                "---\nrating: {}\n---\n\nLink to [[note-{}]]\n",
                i % 10,
                (i + 1) % n
            ),
        )
        .expect("write fixture note");
    }
    temp
}

/// Prepares a project directory with a populated and persisted index of `n`
/// linked notes.
fn setup_refresh_linked(n: usize) -> (TempDir, IndexerService) {
    let temp = create_linked_notes(n);
    let indexer = IndexerService::new(temp.path());
    let index = indexer.build().expect("build index");
    indexer.persist(&index).expect("persist index");
    (temp, indexer)
}

/// Prepares a project directory with a populated but unpersisted index of `n`
/// notes.
fn setup_unpersisted_project(n: usize) -> (TempDir, IndexerService, FileIndex) {
    let temp = create_temp_notes(n);
    let indexer = IndexerService::new(temp.path());
    let index = indexer.build().expect("build index");
    (temp, indexer, index)
}

/// Generates a [`Vec`] of `n` notes in-memory where each note links to the next
/// note, creating a sparse link graph.
fn generate_notes_sparse(n: usize) -> Vec<Note> {
    let mut notes = Vec::with_capacity(n);
    for i in 0..n {
        let path = format!("note-{i}.md");
        let content =
            format!("# Note {i}\n\nLink to [[note-{}]]\n", (i + 1) % n);
        notes.push(parse_markdown(&path, &content));
    }
    notes.sort_by(|a, b| a.path().cmp(b.path()));
    notes
}

/// Generates a [`Vec`] of `n` notes in-memory where each note links to 20 other
/// notes, creating a highly dense link graph.
fn generate_notes_dense(n: usize) -> Vec<Note> {
    let mut notes = Vec::with_capacity(n);
    for i in 0..n {
        let path = format!("note-{i}.md");
        let mut content = format!("# Note {i}\n\n");
        for j in 0..20 {
            let target = (i + j) % n;
            let _ = writeln!(content, "- Link to [[note-{target}]]");
        }
        notes.push(parse_markdown(&path, &content));
    }
    notes.sort_by(|a, b| a.path().cmp(b.path()));
    notes
}

/// Generates a [`Vec`] of `n` notes in-memory containing same-stem collisions
/// across multiple directories, where every other note links to the ambiguous
/// stem.
fn generate_notes_ambiguous(n: usize) -> Vec<Note> {
    let mut notes = Vec::with_capacity(n);
    for i in 0..n {
        let path = if i % 10 == 0 {
            format!("folder_{}/target.md", i / 10)
        } else {
            format!("note-{i}.md")
        };
        let content = if i % 10 != 0 {
            String::from("# Note\n\nLink to [[target]]\n")
        } else {
            String::from("# Target\n")
        };
        notes.push(parse_markdown(&path, &content));
    }
    notes.sort_by(|a, b| a.path().cmp(b.path()));
    notes
}

/// Loads multiple project indexes concurrently, one thread per project.
///
/// Used by [`bench_concurrent_operations`] to simulate multi-vault tooling
/// (batch importers, multi-project LSP workspaces). redb enforces at most one
/// open [`redb::Database`] handle per file *per process*, so this benchmark
/// spawns one thread per project rather than sharing across threads.
fn load_concurrently(projects: &[(TempDir, IndexerService)]) {
    std::thread::scope(|scope| {
        let handles: Vec<_> = projects
            .iter()
            .map(|(_temp, indexer)| {
                scope.spawn(|| indexer.load().expect("load index"))
            })
            .collect();
        for handle in handles {
            black_box(handle.join().expect("thread joined"));
        }
    });
}

// ----------------------------------------------------------- //
//                   Benchmarks: Index Build                   //
// ----------------------------------------------------------- //

/// Measures compile time for building the index.
///
/// Runs the raw build operation on a temporary directory.
///
/// Expected outcomes:
/// - Linear O(n) scaling where doubling the note count roughly doubles
///   compilation time.
///
/// Unexpected outcomes:
/// - Superlinear (e.g., O(n^2)) scaling, indicating memory leaks, nested
///   iterations, or poor algorithms in link graph construction or file path
///   sorting.
fn bench_file_index_build(c: &mut Criterion) {
    let mut group = c.benchmark_group("FileIndex::build");
    for &n in BUILD_SIZES {
        group.throughput(Throughput::Elements(
            u64::try_from(n).expect("note count fits u64"),
        ));
        if n >= 20_000 {
            // 20,000-file builds do real per-iteration disk I/O; bound total
            // suite runtime with criterion's minimum valid sample size (10)
            // instead of the default 100.
            group.sample_size(10);
        }
        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, &n| {
            b.iter_batched_ref(
                || create_temp_notes(n),
                |temp: &mut TempDir| {
                    let index = IndexerService::new(temp.path())
                        .build()
                        .expect("build index");
                    black_box(index);
                },
                BatchSize::LargeInput,
            );
        });
    }
    group.finish();
}

// ----------------------------------------------------------- //
//                  Benchmarks: Index Refresh                  //
// ----------------------------------------------------------- //

/// Measures the refresh lifecycle path across different filesystem states.
///
/// Refresh is run implicitly on every query command. A regression in the no-op
/// path directly degrades general CLI responsiveness, while regressions in
/// upserts or deletions increase edit-to-view latency.
///
/// Expected outcomes:
/// - No-op refresh avoids reparsing and unnecessary point lookups.
/// - Single-file changes are proportional to parsing one note and committing
///   its delta.
///
/// Unexpected outcomes:
/// - High execution times in the "no-op" scenario, indicating cache
///   invalidation leaks or broken comparison logic.
fn bench_file_index_refresh(c: &mut Criterion) {
    let mut group = c.benchmark_group("FileIndex::refresh");
    for &n in WORKSPACE_SIZES {
        group.throughput(Throughput::Elements(
            u64::try_from(n).expect("note count fits u64"),
        ));
        if n >= 20_000 {
            // 20,000-file builds do real per-iteration disk I/O; bound total
            // suite runtime with criterion's minimum valid sample size (10)
            // instead of the default 100.
            group.sample_size(10);
        }

        group.bench_with_input(BenchmarkId::new("no-op", n), &n, |b, &n| {
            b.iter_batched_ref(
                || setup_refresh(n),
                |(_temp, indexer): &mut (TempDir, IndexerService)| {
                    let index = indexer.refresh().expect("refresh index");
                    black_box(index);
                },
                BatchSize::LargeInput,
            );
        });

        group.bench_with_input(
            BenchmarkId::new("single-upsert", n),
            &n,
            |b, &n| {
                b.iter_batched_ref(
                    || {
                        let (temp, indexer) = setup_refresh(n);
                        std::fs::write(
                            temp.path().join("note-0.md"),
                            "---\nrating: 99\n---\n\nBody change.\n",
                        )
                        .expect("write update");
                        (temp, indexer)
                    },
                    |(_temp, indexer): &mut (TempDir, IndexerService)| {
                        let index = indexer.refresh().expect("refresh index");
                        black_box(index);
                    },
                    BatchSize::LargeInput,
                );
            },
        );

        group.bench_with_input(
            BenchmarkId::new("single-delete", n),
            &n,
            |b, &n| {
                b.iter_batched_ref(
                    || {
                        let (temp, indexer) = setup_refresh(n);
                        std::fs::remove_file(temp.path().join("note-0.md"))
                            .expect("delete note");
                        (temp, indexer)
                    },
                    |(_temp, indexer): &mut (TempDir, IndexerService)| {
                        let index = indexer.refresh().expect("refresh index");
                        black_box(index);
                    },
                    BatchSize::LargeInput,
                );
            },
        );

        group.bench_with_input(
            BenchmarkId::new("linked-single-upsert", n),
            &n,
            |b, &n| {
                b.iter_batched_ref(
                    || {
                        let (temp, indexer) = setup_refresh_linked(n);
                        std::fs::write(
                            temp.path().join("note-0.md"),
                            "---\nrating: 99\n---\n\nLink to [[note-1]]\nLink \
                             to [[note-2]]\n",
                        )
                        .expect("write update");
                        (temp, indexer)
                    },
                    |(_temp, indexer): &mut (TempDir, IndexerService)| {
                        let index = indexer.refresh().expect("refresh index");
                        black_box(index);
                    },
                    BatchSize::LargeInput,
                );
            },
        );
    }
    group.finish();
}

// ----------------------------------------------------------- //
//                  Benchmarks: Index Persist                  //
// ----------------------------------------------------------- //

/// Measures full database persistence transaction overhead.
///
/// Isolates the serialization and disk-write cost of a full index rewrite.
///
/// Expected outcomes:
/// - Cost scales linearly with note count.
///
/// Unexpected outcomes:
/// - Cost scaling super-linearly, indicating redundant serialization or
///   unbounded transaction size.
fn bench_index_persist(c: &mut Criterion) {
    let mut group = c.benchmark_group("FileIndex::persist");
    for &n in BUILD_SIZES {
        group.throughput(Throughput::Elements(
            u64::try_from(n).expect("note count fits u64"),
        ));
        if n >= 20_000 {
            // 20,000-file builds do real per-iteration disk I/O; bound total
            // suite runtime with criterion's minimum valid sample size (10)
            // instead of the default 100.
            group.sample_size(10);
        }
        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, &n| {
            b.iter_batched_ref(
                || setup_unpersisted_project(n),
                |(_temp, indexer, index): &mut (
                    TempDir,
                    IndexerService,
                    FileIndex,
                )| {
                    indexer.persist(index).expect("persist index");
                },
                BatchSize::LargeInput,
            );
        });
    }
    group.finish();
}

// ----------------------------------------------------------- //
//                   Benchmarks: Link Graph                    //
// ----------------------------------------------------------- //

/// Measures isolated link graph resolution and compilation in-memory.
///
/// Isolates the CPU complexity of parsing and building the inlink map from disk
/// and database overhead. Evaluates path resolution, stem index lookups, and
/// proximity-based tie-breaking.
///
/// Expected outcomes:
/// - Linear or near-linear scaling for sparse and dense graphs.
/// - High ambiguity graphs show minor degradation due to directory crawling but
///   remain within stable limits.
///
/// Unexpected outcomes:
/// - O(n^2) scaling under tie-breaking search, indicating folder distance
///   calculation is too hot or allocating redundant paths.
fn bench_derive_inlinks(c: &mut Criterion) {
    let mut group = c.benchmark_group("derive_inlinks");
    for &n in WORKSPACE_SIZES {
        group.throughput(Throughput::Elements(
            u64::try_from(n).expect("note count fits u64"),
        ));

        group.bench_with_input(BenchmarkId::new("sparse", n), &n, |b, &n| {
            let notes = generate_notes_sparse(n);
            let refs: Vec<&Note> = notes.iter().collect();
            b.iter(|| {
                let inlinks = derive_inlinks(black_box(&refs));
                black_box(inlinks);
            });
        });

        group.bench_with_input(BenchmarkId::new("dense", n), &n, |b, &n| {
            let notes = generate_notes_dense(n);
            let refs: Vec<&Note> = notes.iter().collect();
            b.iter(|| {
                let inlinks = derive_inlinks(black_box(&refs));
                black_box(inlinks);
            });
        });

        group.bench_with_input(
            BenchmarkId::new("ambiguous", n),
            &n,
            |b, &n| {
                let notes = generate_notes_ambiguous(n);
                let refs: Vec<&Note> = notes.iter().collect();
                b.iter(|| {
                    let inlinks = derive_inlinks(black_box(&refs));
                    black_box(inlinks);
                });
            },
        );
    }
    group.finish();
}

// ----------------------------------------------------------- //
//              Benchmarks: Concurrent Operations              //
// ----------------------------------------------------------- //

/// Benchmarks concurrent loading of independent project indexes.
///
/// Expected outcomes:
/// - Parallel loads complete faster than serial loads, dominated by
///   thread-spawn and per-project I/O cost.
///
/// Unexpected outcomes:
/// - Parallel loads slower than serial, indicating OS-level resource contention
///   (disk I/O, page cache) rather than application-level lock.
fn bench_concurrent_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("FileIndex::concurrent");
    let n = 250_usize;
    group.throughput(Throughput::Elements(1000));
    // Thread-spawn and per-project I/O are inherently noisy; a larger sample
    // size and longer measurement window smooth out scheduler jitter.
    group.sample_size(50);
    group.measurement_time(std::time::Duration::from_secs(10));
    group.sampling_mode(criterion::SamplingMode::Flat);

    group.bench_function("concurrent-load-4-independent-projects", |b| {
        b.iter_batched_ref(
            || {
                let mut projects = Vec::with_capacity(4);
                for _ in 0..4 {
                    projects.push(setup_refresh(n));
                }
                projects
            },
            |projects: &mut Vec<(TempDir, IndexerService)>| {
                load_concurrently(projects);
            },
            BatchSize::LargeInput,
        );
    });
    group.finish();
}

criterion_group!(
    benches,
    bench_file_index_build,
    bench_file_index_refresh,
    bench_index_persist,
    bench_derive_inlinks,
    bench_concurrent_operations
);
criterion_main!(benches);
