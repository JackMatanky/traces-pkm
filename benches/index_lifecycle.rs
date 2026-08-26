//! Performance benchmark suite for the index lifecycle pipeline.
//!
//! Exposes and monitors the execution cost (CPU latency and allocations) of
//! indexing operations driven by `IndexerService` (build, refresh, and persist)
//! and the underlying link graph compiler (`derive_inlinks`).
//!
//! This suite serves as a key guardian against performance regressions in the
//! write path of the personal knowledge base index. Because PKM queries must
//! refresh transparently on command execution, any overhead in these lifecycle
//! functions directly limits the responsiveness of the CLI.
//!
//! ### Data Flow Diagram
//! ```text
//! [Files on Disk] ──(Scan)──► [FileBase / Notes] ──(derive_inlinks)──► [InlinkMap]
//!                                                                          │
//! [redb database] ◄──(Persist)─────────────────────────────────────────────┘
//! ```
//!
//! ### Expected Baselines
//! - **Build**: ~40ms for 1000 notes.
//! - **Refresh (No-op)**: ~1ms.
//! - **Refresh (Upsert/Delete)**: ~5ms.
//! - **Inlink Graph**: ~5ms for 1000 notes.
//!
//! ### Profiling Integration
//! To profile index lifecycle CPU bottlenecks:
//! ```bash
//! cargo flamegraph --bench index_lifecycle -- --bench "FileIndex::refresh/no-op/1000"
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
use std::{alloc::System, fmt::Write as _, hint::black_box, path::PathBuf};

use criterion::{
    BatchSize, BenchmarkId, Criterion, criterion_group, criterion_main,
};
use stats_alloc::{INSTRUMENTED_SYSTEM, Region, StatsAlloc};
use traces_pkm::{
    FileIndex, IndexerService, Note, derive_inlinks, parse_markdown,
};

// `StatsAlloc` implements `GlobalAlloc` internally (the crate's own audited
// code owns the only `unsafe impl`); this benchmark never writes `unsafe`
// itself while still measuring per-call allocation counts and byte totals.
#[global_allocator]
static GLOBAL: &StatsAlloc<System> = &INSTRUMENTED_SYSTEM;

/// Creates a temporary directory populated with `n` synthetic notes, returning
/// the project path.
fn populate(n: usize) -> PathBuf {
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
    let root = temp.path().to_path_buf();
    // Leaks the temp directory to prevent directory cleanup costs from
    // polluting the measured timing run of Criterion closures.
    let _ = temp.keep();
    root
}

/// Prepares a project directory with a populated and persisted index of `n`
/// notes, and returns the root [`PathBuf`] and the [`IndexerService`] instance.
fn setup_refresh(n: usize) -> (PathBuf, IndexerService) {
    let root = populate(n);
    let indexer = IndexerService::new(&root);
    let index = indexer.build().expect("build index");
    indexer.persist(&index).expect("persist index");
    (root, indexer)
}

/// Prepares a project directory with a populated but unpersisted index of `n`
/// notes, and returns the [`IndexerService`] instance and the built
/// [`FileIndex`].
fn setup_persist_full(n: usize) -> (IndexerService, FileIndex) {
    let root = populate(n);
    let indexer = IndexerService::new(&root);
    let index = indexer.build().expect("build index");
    (indexer, index)
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
    // Report allocation stats once before timing.
    eprintln!("\n[FileIndex::build Allocation Stats]");
    {
        let root = populate(1000);
        let region = Region::new(GLOBAL);
        let index = IndexerService::new(&root).build().expect("build index");
        eprintln!("  - build (1000 notes): {:?}", region.change());
        black_box(index);
    }

    let mut group = c.benchmark_group("FileIndex::build");
    for n in [10_usize, 100, 1000] {
        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, &n| {
            b.iter_batched(
                || populate(n),
                |root| IndexerService::new(&root).build().expect("build index"),
                BatchSize::LargeInput,
            );
        });
    }
    group.finish();
}

/// Measures the refresh lifecycle path across different filesystem states.
///
/// Refresh is run implicitly on every query command. A regression in the no-op
/// path directly degrades general CLI responsiveness, while regressions in
/// upserts or deletions increase edit-to-view latency.
///
/// Expected outcomes:
/// - "no-op" runs in sub-millisecond times, confirming the engine avoids
///   unnecessary reparses and point-lookups.
/// - "single-upsert" is fast and proportional to the cost of parsing a single
///   note and committing the delta.
/// - "single-delete" handles removal quickly, executing only necessary
///   link-graph updates.
///
/// Unexpected outcomes:
/// - High execution times in the "no-op" scenario, indicating cache
///   invalidation leaks or broken comparison logic.
fn bench_file_index_refresh(c: &mut Criterion) {
    // Report allocation stats once before timing.
    eprintln!("\n[FileIndex::refresh Allocation Stats]");
    {
        let (_root, indexer) = setup_refresh(1000);
        let region = Region::new(GLOBAL);
        let index = indexer.refresh().expect("refresh index");
        eprintln!("  - refresh no-op (1000 notes): {:?}", region.change());
        black_box(index);
    }
    {
        let (root, indexer) = setup_refresh(1000);
        let note_path = root.join("note-0.md");
        std::fs::write(&note_path, "---\nrating: 99\n---\n\nBody change.\n")
            .expect("write update");
        let region = Region::new(GLOBAL);
        let index = indexer.refresh().expect("refresh index");
        eprintln!(
            "  - refresh single-upsert (1000 notes): {:?}",
            region.change()
        );
        black_box(index);
    }

    let mut group = c.benchmark_group("FileIndex::refresh");
    for n in [100_usize, 1000] {
        // No-op refresh
        group.bench_with_input(BenchmarkId::new("no-op", n), &n, |b, &n| {
            b.iter_batched(
                || setup_refresh(n),
                |(_root, indexer)| indexer.refresh().expect("refresh index"),
                BatchSize::LargeInput,
            );
        });

        // Single upsert
        group.bench_with_input(
            BenchmarkId::new("single-upsert", n),
            &n,
            |b, &n| {
                b.iter_batched(
                    || {
                        let (root, indexer) = setup_refresh(n);
                        let note_path = root.join("note-0.md");
                        std::fs::write(
                            &note_path,
                            "---\nrating: 99\n---\n\nBody change.\n",
                        )
                        .expect("write update");
                        (root, indexer)
                    },
                    |(_root, indexer)| {
                        indexer.refresh().expect("refresh index")
                    },
                    BatchSize::LargeInput,
                );
            },
        );

        // Single delete
        group.bench_with_input(
            BenchmarkId::new("single-delete", n),
            &n,
            |b, &n| {
                b.iter_batched(
                    || {
                        let (root, indexer) = setup_refresh(n);
                        let note_path = root.join("note-0.md");
                        std::fs::remove_file(&note_path).expect("delete note");
                        (root, indexer)
                    },
                    |(_root, indexer)| {
                        indexer.refresh().expect("refresh index")
                    },
                    BatchSize::LargeInput,
                );
            },
        );
    }
    group.finish();
}

/// Measures database persistence transaction overhead.
///
/// Isolates serialization and disk-write transaction costs, helping determine
/// the efficiency of full rewrites compared to incremental commits.
///
/// Expected outcomes:
/// - Persistence time scales with the number of written records, with
///   incremental updates being significantly faster than full rewrites.
///
/// Unexpected outcomes:
/// - Write times that grow exponentially or present high variance, indicating
///   transaction lock contention or redb table allocation bottlenecks.
fn bench_index_persist(c: &mut Criterion) {
    // Report allocation stats once before timing.
    eprintln!("\n[FileIndex::persist Allocation Stats]");
    {
        let (indexer, index) = setup_persist_full(1000);
        let region = Region::new(GLOBAL);
        indexer.persist(&index).expect("persist index");
        eprintln!("  - persist full (1000 notes): {:?}", region.change());
    }

    let mut group = c.benchmark_group("FileIndex::persist");
    for n in [10_usize, 100, 1000] {
        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, &n| {
            b.iter_batched(
                || setup_persist_full(n),
                |(indexer, index)| {
                    indexer.persist(&index).expect("persist index");
                },
                BatchSize::LargeInput,
            );
        });
    }
    group.finish();
}

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
    // Report allocation stats once before timing.
    eprintln!("\n[derive_inlinks Allocation Stats]");
    {
        let notes = generate_notes_sparse(1000);
        let region = Region::new(GLOBAL);
        let res = derive_inlinks(&notes);
        eprintln!(
            "  - derive_inlinks sparse (1000 notes): {:?}",
            region.change()
        );
        black_box(res);
    }
    {
        let notes = generate_notes_dense(1000);
        let region = Region::new(GLOBAL);
        let res = derive_inlinks(&notes);
        eprintln!(
            "  - derive_inlinks dense (1000 notes): {:?}",
            region.change()
        );
        black_box(res);
    }
    {
        let notes = generate_notes_ambiguous(1000);
        let region = Region::new(GLOBAL);
        let res = derive_inlinks(&notes);
        eprintln!(
            "  - derive_inlinks ambiguous (1000 notes): {:?}",
            region.change()
        );
        black_box(res);
    }

    let mut group = c.benchmark_group("derive_inlinks");
    for n in [100_usize, 1000] {
        // Sparse in-memory link graph
        group.bench_with_input(BenchmarkId::new("sparse", n), &n, |b, &n| {
            let notes = generate_notes_sparse(n);
            b.iter(|| {
                let res = derive_inlinks(black_box(&notes));
                black_box(res);
            });
        });

        // Dense in-memory link graph
        group.bench_with_input(BenchmarkId::new("dense", n), &n, |b, &n| {
            let notes = generate_notes_dense(n);
            b.iter(|| {
                let res = derive_inlinks(black_box(&notes));
                black_box(res);
            });
        });

        // Ambiguous link graph with stem collisions triggering proximity
        // resolver
        group.bench_with_input(
            BenchmarkId::new("ambiguous", n),
            &n,
            |b, &n| {
                let notes = generate_notes_ambiguous(n);
                b.iter(|| {
                    let res = derive_inlinks(black_box(&notes));
                    black_box(res);
                });
            },
        );
    }
    group.finish();
}

/// Measures concurrent reading performance across multiple independently
/// indexed projects.
///
/// Multi-vault tooling (batch importers, multi-project LSP workspaces) loads
/// several project indexes concurrently. This benchmark spawns one thread per
/// project rather than sharing one project across threads: redb enforces at
/// most one open [`redb::Database`] handle per file *per process*, so two
/// [`IndexerService::load`] calls against the *same* database file from
/// concurrent threads panic with `DatabaseAlreadyOpen` (discovered while
/// writing this benchmark) rather than contending on a read lock. Genuine
/// same-file concurrent reads require sharing one `Database` handle across
/// threads, which `IndexerService`'s per-call `open`/`close` design does not
/// expose.
///
/// Expected outcomes:
/// - Smooth scaling under parallel loads, dominated by thread-spawn and
///   per-project I/O cost rather than lock contention (each project has its own
///   database file).
///
/// Unexpected outcomes:
/// - Scaling bottlenecks, indicating shared OS-level resource contention (disk
///   I/O, page cache) rather than an application-level lock.
fn bench_concurrent_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("FileIndex::concurrent");
    let n = 250_usize;

    group.bench_function("concurrent-load-4-independent-projects", |b| {
        b.iter_batched(
            || {
                let mut projects = Vec::new();
                for _ in 0..4 {
                    projects.push(setup_refresh(n));
                }
                projects
            },
            |projects| {
                let mut handles = Vec::new();
                for (_root, indexer) in projects {
                    handles.push(std::thread::spawn(move || {
                        let index = indexer.load().expect("load index");
                        black_box(index);
                    }));
                }
                for handle in handles {
                    handle.join().expect("thread joined");
                }
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
