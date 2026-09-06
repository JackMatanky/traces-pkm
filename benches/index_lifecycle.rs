//! Performance benchmark suite for the index lifecycle pipeline.
//!
//! Exposes and monitors the execution cost of indexing operations driven by
//! `IndexerService` (build, refresh, persist, load, and list reads).
//!
//! This suite serves as a key guardian against performance regressions in the
//! write path of the personal knowledge base index. Because PKM queries must
//! refresh transparently on command execution, any overhead in these lifecycle
//! functions directly limits the responsiveness of the CLI.
//!
//! ### Data Flow Diagram
//!
//! ```text
//! [Files on Disk] ──(Scan)──► [FileBase / Notes] ──(Index)──► [FileIndex]
//!                                                                  │
//! [redb database] ◄──(Persist)─────────────────────────────────────┘
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

#![allow(
    clippy::expect_used,
    clippy::arithmetic_side_effects,
    reason = "bench fixture/harness code uses deterministic arithmetic and \
              should panic immediately on broken fixtures"
)]

use std::hint::black_box;

use criterion::{
    BatchSize, BenchmarkId, Criterion, Throughput, criterion_group,
    criterion_main,
};
use tempfile::TempDir;
use traces_pkm::{FileIndex, IndexerService};

#[allow(
    dead_code,
    reason = "shared benchmark common helpers are compiled into each bench \
              target; this target uses only the filesystem lifecycle helpers"
)]
mod common;

use common::{
    WORKSPACE_FILE_COUNTS,
    content::{
        ProjectShape, linked_note_source, plain_note_source, rich_note_source,
        tagged_note_source,
    },
    project::{
        create_project, remove_note, rewrite_note, setup_persisted_project,
        setup_unpersisted_project,
    },
    quick_file_counts,
};

// ----------------------------------------------------------- //
//                     Fixtures & Helpers                      //
// ----------------------------------------------------------- //

const BUILD_PROFILE_SHAPES: &[ProjectShape] = &[
    ProjectShape::Tagged,
    ProjectShape::Classified,
    ProjectShape::LinkedDense,
    ProjectShape::ListHeavy,
    ProjectShape::RichRealistic,
    ProjectShape::AttachmentProject,
];

fn observe_index(index: &FileIndex) {
    let entries = index.entries();
    let note_count =
        entries.iter().filter(|entry| entry.note().is_some()).count();
    let inlink_count: usize =
        entries.iter().map(|entry| entry.inlinks().len()).sum();
    black_box((entries.len(), note_count, inlink_count));
}

fn observe_refresh(indexer: &IndexerService) {
    let (index, report) = indexer.refresh_with_report().expect("refresh index");
    let entries = index.entries();
    let inlink_count: usize =
        entries.iter().map(|entry| entry.inlinks().len()).sum();
    black_box((
        entries.len(),
        inlink_count,
        report.upserted(),
        report.deleted(),
        report.links_modified(),
    ));
}

fn observe_load(indexer: &IndexerService) {
    let index = indexer.load().expect("load index");
    observe_index(&index);
}

/// Loads multiple project indexes concurrently, one thread per project.
///
/// Used by [`bench_concurrent_operations`] to simulate multi-vault tooling
/// (batch importers, multi-project LSP workspaces). redb enforces at most one
/// open [`redb::Database`] handle per file *per process*, so this benchmark
/// spawns one thread per project rather than sharing across threads.
fn load_concurrently(projects: &[(TempDir, IndexerService)]) {
    std::thread::scope(|scope| {
        for (_, indexer) in projects {
            scope.spawn(move || observe_load(indexer));
        }
    });
}

// ----------------------------------------------------------- //
//                   Benchmarks: Index Build                   //
// ----------------------------------------------------------- //

/// Measures compile time for building the index over the baseline plain-note
/// fixture.
///
/// Runs the raw build operation on a temporary directory, excluding fixture
/// creation from the measured loop via Criterion batched setup.
///
/// Expected outcomes:
/// - Linear O(n) scaling where doubling the note count roughly doubles
///   compilation time.
///
/// Unexpected outcomes:
/// - Superlinear scaling, indicating memory leaks, nested iterations, or poor
///   algorithms in link graph construction or file path sorting.
fn bench_file_index_build(c: &mut Criterion) {
    let mut group = c.benchmark_group("FileIndex::build");
    for &n in WORKSPACE_FILE_COUNTS {
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
            b.iter_batched(
                || create_project(n, ProjectShape::Plain),
                |temp| {
                    let index = IndexerService::new(temp.path())
                        .build()
                        .expect("build index");
                    observe_index(&index);
                    black_box(temp.path());
                },
                BatchSize::LargeInput,
            );
        });
    }
    group.finish();
}

/// Measures build cost across richer fixture profiles at suite-friendly sizes.
///
/// These profiles cover tag extraction, File Class frontmatter, dense link
/// graphs, list persistence, nested folders, and real non-Markdown attachments
/// through the public [`IndexerService`] API.
///
/// Expected outcomes:
/// - Rich profiles are slower than plain notes but remain roughly linear in
///   note count.
///
/// Unexpected outcomes:
/// - A single rich profile dominates disproportionately, identifying the next
///   index subsystem to profile directly.
fn bench_file_index_build_profiles(c: &mut Criterion) {
    let mut group = c.benchmark_group("FileIndex::build/profiles");
    for &shape in BUILD_PROFILE_SHAPES {
        for n in quick_file_counts() {
            group.throughput(Throughput::Elements(
                u64::try_from(n).expect("note count fits u64"),
            ));
            group.bench_with_input(
                BenchmarkId::new(shape.name(), n),
                &n,
                |b, &n| {
                    b.iter_batched(
                        || create_project(n, shape),
                        |temp| {
                            let index = IndexerService::new(temp.path())
                                .build()
                                .expect("build index");
                            observe_index(&index);
                            black_box(temp.path());
                        },
                        BatchSize::LargeInput,
                    );
                },
            );
        }
    }
    group.finish();
}

// ----------------------------------------------------------- //
//                  Benchmarks: Index Refresh                  //
// ----------------------------------------------------------- //

/// Measures the refresh lifecycle path across baseline filesystem states.
///
/// Refresh is run implicitly on every query command. A regression in the no-op
/// path directly degrades general CLI responsiveness, while regressions in
/// upserts or deletions increase edit-to-view latency.
///
/// Expected outcomes:
/// - No-op refresh avoids reparsing and unnecessary writes.
/// - Single-file changes are proportional to parsing one note and committing
///   its observable delta.
///
/// Unexpected outcomes:
/// - High execution times in the "no-op" scenario, indicating cache
///   invalidation leaks or broken comparison logic.
fn bench_file_index_refresh(c: &mut Criterion) {
    let mut group = c.benchmark_group("FileIndex::refresh");
    for &n in WORKSPACE_FILE_COUNTS {
        group.throughput(Throughput::Elements(
            u64::try_from(n).expect("note count fits u64"),
        ));
        if n >= 10_000 {
            // 10,000+ note refreshes do real per-iteration disk I/O; bound
            // total suite runtime with criterion's minimum valid
            // sample size (10) instead of the default 100.
            group.sample_size(10);
        }

        group.bench_with_input(BenchmarkId::new("no-op", n), &n, |b, &n| {
            b.iter_batched(
                || setup_persisted_project(n, ProjectShape::Plain),
                |(_temp, indexer)| observe_refresh(&indexer),
                BatchSize::LargeInput,
            );
        });

        group.bench_with_input(
            BenchmarkId::new("single-upsert", n),
            &n,
            |b, &n| {
                b.iter_batched(
                    || {
                        let (temp, indexer) =
                            setup_persisted_project(n, ProjectShape::Plain);
                        let mut content = plain_note_source(0);
                        content
                            .push_str("\nBody change for refresh benchmark.\n");
                        rewrite_note(
                            temp.path(),
                            ProjectShape::Plain,
                            0,
                            n,
                            &content,
                        );
                        (temp, indexer)
                    },
                    |(_temp, indexer)| observe_refresh(&indexer),
                    BatchSize::LargeInput,
                );
            },
        );

        group.bench_with_input(
            BenchmarkId::new("single-delete", n),
            &n,
            |b, &n| {
                b.iter_batched(
                    || {
                        let (temp, indexer) =
                            setup_persisted_project(n, ProjectShape::Plain);
                        remove_note(temp.path(), ProjectShape::Plain, 0);
                        (temp, indexer)
                    },
                    |(_temp, indexer)| observe_refresh(&indexer),
                    BatchSize::LargeInput,
                );
            },
        );

        group.bench_with_input(
            BenchmarkId::new("linked-single-upsert", n),
            &n,
            |b, &n| {
                b.iter_batched(
                    || {
                        let (temp, indexer) = setup_persisted_project(
                            n,
                            ProjectShape::LinkedSparse,
                        );
                        let content = linked_note_source(0, n, 2);
                        rewrite_note(
                            temp.path(),
                            ProjectShape::LinkedSparse,
                            0,
                            n,
                            &content,
                        );
                        (temp, indexer)
                    },
                    |(_temp, indexer)| observe_refresh(&indexer),
                    BatchSize::LargeInput,
                );
            },
        );
    }
    group.finish();
}

fn setup_single_tag_upsert(n: usize) -> (TempDir, IndexerService) {
    let (temp, indexer) = setup_persisted_project(n, ProjectShape::Tagged);
    let mut content = tagged_note_source(0);
    content.push_str("\nAdded #common #topic/updated #new_tag.\n");
    rewrite_note(temp.path(), ProjectShape::Tagged, 0, n, &content);
    (temp, indexer)
}

fn setup_many_rich_upserts(
    n: usize,
    changed: usize,
) -> (TempDir, IndexerService) {
    let (temp, indexer) =
        setup_persisted_project(n, ProjectShape::RichRealistic);
    for i in 0..changed.min(n) {
        let mut content = rich_note_source(i, n);
        content.push_str("\nChanged in many-upsert benchmark.\n");
        rewrite_note(temp.path(), ProjectShape::RichRealistic, i, n, &content);
    }
    (temp, indexer)
}

fn setup_single_rich_delete(n: usize) -> (TempDir, IndexerService) {
    let (temp, indexer) = setup_persisted_project(n, ProjectShape::DeleteHeavy);
    remove_note(temp.path(), ProjectShape::DeleteHeavy, 0);
    (temp, indexer)
}

/// Measures refresh cost for richer mutation shapes using only public lifecycle
/// APIs.
///
/// This group is a public proxy for internal store cleanup, secondary index
/// updates, inlink recomputation, and many-note merge behavior without exposing
/// `IndexStore`, redb tables, or delta structs.
///
/// Expected outcomes:
/// - Rich no-op refresh remains near the plain no-op path.
/// - Many-note updates scale with changed notes plus one full inlink recompute,
///   not quadratically with total persisted notes.
///
/// Unexpected outcomes:
/// - Rich delete or tag updates dominate, suggesting secondary-index cleanup is
///   scanning too much persisted state.
fn bench_file_index_refresh_profiles(c: &mut Criterion) {
    let mut group = c.benchmark_group("FileIndex::refresh/profiles");
    for n in quick_file_counts() {
        group.throughput(Throughput::Elements(
            u64::try_from(n).expect("note count fits u64"),
        ));

        group.bench_with_input(
            BenchmarkId::new("no-op-rich", n),
            &n,
            |b, &n| {
                b.iter_batched(
                    || setup_persisted_project(n, ProjectShape::RichRealistic),
                    |(_temp, indexer)| observe_refresh(&indexer),
                    BatchSize::LargeInput,
                );
            },
        );

        group.bench_with_input(
            BenchmarkId::new("single-tag-upsert", n),
            &n,
            |b, &n| {
                b.iter_batched(
                    || setup_single_tag_upsert(n),
                    |(_temp, indexer)| observe_refresh(&indexer),
                    BatchSize::LargeInput,
                );
            },
        );

        for changed in [10_usize, 100] {
            group.bench_with_input(
                BenchmarkId::new(format!("many-upsert-{changed}"), n),
                &n,
                |b, &n| {
                    b.iter_batched(
                        || setup_many_rich_upserts(n, changed),
                        |(_temp, indexer)| observe_refresh(&indexer),
                        BatchSize::LargeInput,
                    );
                },
            );
        }

        group.bench_with_input(
            BenchmarkId::new("single-rich-delete", n),
            &n,
            |b, &n| {
                b.iter_batched(
                    || setup_single_rich_delete(n),
                    |(_temp, indexer)| observe_refresh(&indexer),
                    BatchSize::LargeInput,
                );
            },
        );

        group.bench_with_input(
            BenchmarkId::new("attachment-target-present", n),
            &n,
            |b, &n| {
                b.iter_batched(
                    || {
                        setup_persisted_project(
                            n,
                            ProjectShape::AttachmentProject,
                        )
                    },
                    |(_temp, indexer)| observe_refresh(&indexer),
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

/// Measures full database persistence transaction overhead for plain notes.
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
    for &n in WORKSPACE_FILE_COUNTS {
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
            b.iter_batched(
                || setup_unpersisted_project(n, ProjectShape::Plain),
                |(_temp, indexer, index)| {
                    indexer.persist(&index).expect("persist index");
                    black_box(index.entries().len());
                },
                BatchSize::LargeInput,
            );
        });
    }
    group.finish();
}

/// Measures full persistence cost for richer note shapes at suite-friendly
/// sizes.
///
/// Expected outcomes:
/// - Rich persistence is slower than plain but scales with rows written.
///
/// Unexpected outcomes:
/// - List-heavy or rich persistence grows disproportionately, indicating row
///   encoding or secondary-index write amplification.
fn bench_index_persist_profiles(c: &mut Criterion) {
    let mut group = c.benchmark_group("FileIndex::persist/profiles");
    for &shape in BUILD_PROFILE_SHAPES {
        for n in quick_file_counts() {
            group.throughput(Throughput::Elements(
                u64::try_from(n).expect("note count fits u64"),
            ));
            group.bench_with_input(
                BenchmarkId::new(shape.name(), n),
                &n,
                |b, &n| {
                    b.iter_batched(
                        || setup_unpersisted_project(n, shape),
                        |(_temp, indexer, index)| {
                            indexer.persist(&index).expect("persist index");
                            black_box(index.entries().len());
                        },
                        BatchSize::LargeInput,
                    );
                },
            );
        }
    }
    group.finish();
}

// ----------------------------------------------------------- //
//                    Benchmarks: Index Load                    //
// ----------------------------------------------------------- //

/// Measures public persisted index load/materialization cost.
///
/// This is the public visibility-safe proxy for the internal `IndexStore` full
/// read path. Fixture setup is outside the measured loop; each iteration opens
/// the persisted index and materializes a [`FileIndex`].
///
/// Expected outcomes:
/// - Load cost scales linearly with stored files, notes, and inlink rows.
///
/// Unexpected outcomes:
/// - Rich or attachment loads dominate plain loads, indicating decode or
///   assembly overhead rather than raw row count.
fn bench_index_load(c: &mut Criterion) {
    let mut group = c.benchmark_group("FileIndex::load");
    for &n in WORKSPACE_FILE_COUNTS {
        let (_temp, indexer) = setup_persisted_project(n, ProjectShape::Plain);
        group.throughput(Throughput::Elements(
            u64::try_from(n).expect("note count fits u64"),
        ));
        if n >= 10_000 {
            group.sample_size(10);
        }
        group.bench_with_input(BenchmarkId::new("plain", n), &n, |b, _| {
            b.iter(|| observe_load(black_box(&indexer)));
        });
    }

    for &shape in
        &[ProjectShape::RichRealistic, ProjectShape::AttachmentProject]
    {
        for n in quick_file_counts() {
            let (_temp, indexer) = setup_persisted_project(n, shape);
            group.throughput(Throughput::Elements(
                u64::try_from(n).expect("note count fits u64"),
            ));
            group.bench_with_input(
                BenchmarkId::new(shape.name(), n),
                &n,
                |b, _| {
                    b.iter(|| observe_load(black_box(&indexer)));
                },
            );
        }
    }
    group.finish();
}

/// Measures public list-table reads over persisted list-heavy projects.
///
/// This keeps list persistence visible without exposing
/// `IndexStore::read_all_lists`.
///
/// Expected outcomes:
/// - Read cost scales linearly with persisted list rows.
///
/// Unexpected outcomes:
/// - Cost grows faster than list count, indicating decode or key traversal
///   overhead in the persisted list table.
fn bench_read_lists(c: &mut Criterion) {
    let mut group = c.benchmark_group("IndexerService::read_lists");
    for n in quick_file_counts() {
        let (_temp, indexer) =
            setup_persisted_project(n, ProjectShape::ListHeavy);
        let list_rows = n.saturating_mul(20);
        group.throughput(Throughput::Elements(
            u64::try_from(list_rows).expect("list row count fits u64"),
        ));
        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, _| {
            b.iter(|| {
                let lists = indexer.read_lists().expect("read lists");
                black_box(lists.len());
            });
        });
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
    group.throughput(Throughput::Elements(1_000));
    group.bench_function("concurrent-load-4-independent-projects", |b| {
        b.iter_batched_ref(
            || {
                let mut projects = Vec::with_capacity(4);
                for _ in 0..4 {
                    projects
                        .push(setup_persisted_project(n, ProjectShape::Plain));
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
    bench_file_index_build_profiles,
    bench_file_index_refresh,
    bench_file_index_refresh_profiles,
    bench_index_persist,
    bench_index_persist_profiles,
    bench_index_load,
    bench_read_lists,
    bench_concurrent_operations
);
criterion_main!(benches);
