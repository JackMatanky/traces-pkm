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
    AxisScale, BatchSize, BenchmarkId, Criterion, PlotConfiguration,
    Throughput, criterion_group, criterion_main,
};
use tempfile::TempDir;
use traces_pkm::{
    FileIndex, IndexerService, QueryBuilder, QueryService, QuerySet,
    SourceSelector, SyncReport,
};

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

fn observe_refresh(indexer: &IndexerService) -> (FileIndex, SyncReport) {
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
    (index, report)
}

fn observe_sync_and_run(
    service: &QueryService,
    indexer: &IndexerService,
    source: SourceSelector,
) -> QuerySet {
    let outcome = service
        .sync_and_run(indexer, QueryBuilder::pages(source))
        .expect("sync_and_run succeeds");
    black_box(outcome.len());
    outcome
}

fn observe_load(indexer: &IndexerService) -> FileIndex {
    let index = indexer.load().expect("load index");
    observe_index(&index);
    index
}

/// Loads multiple project indexes concurrently, one thread per project.
///
/// Used by [`bench_concurrent_operations`] to simulate multi-vault tooling
/// (batch importers, multi-project LSP workspaces). redb enforces at most one
/// open [`redb::Database`] handle per file *per process*, so this benchmark
/// spawns one thread per project rather than sharing across threads.
fn load_concurrently(projects: &[(TempDir, IndexerService)]) -> Vec<FileIndex> {
    std::thread::scope(|scope| {
        projects
            .iter()
            .map(|(_, indexer)| scope.spawn(move || observe_load(indexer)))
            .map(|handle| handle.join().expect("thread panicked"))
            .collect()
    })
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

// ----------------------------------------------------------- //
//                   Benchmarks: Index Build                   //
// ----------------------------------------------------------- //

/// Measures wall-clock index build time over the baseline plain-note fixture.
///
/// Runs the raw build operation on a temporary directory, excluding fixture
/// creation from the measured loop via Criterion batched setup.
///
/// Expected outcomes:
/// - Linear O(n) scaling where doubling the note count roughly doubles scan,
///   parse, sort, and inlink compilation time.
///
/// Unexpected outcomes:
/// - Superlinear scaling, indicating nested iterations, repeated full-vault
///   scans, or avoidable intermediate collections in link graph construction or
///   file path sorting.
fn bench_file_index_build(c: &mut Criterion) {
    let mut group = c.benchmark_group("FileIndex::build");
    group.plot_config(
        PlotConfiguration::default().summary_scale(AxisScale::Logarithmic),
    );
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
            b.iter_batched_ref(
                || create_project(n, ProjectShape::Plain),
                |temp| {
                    let index = IndexerService::new(temp.path())
                        .build()
                        .expect("build index");
                    observe_index(&index);
                    black_box(temp.path());
                    index
                },
                BatchSize::LargeInput,
            );
        });
    }
    group.finish();
}

/// Measures build cost across richer fixture profiles at bounded suite sizes.
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
/// - One profile grows much faster than the others as `n` increases, indicating
///   shape-specific repeated scans or allocation churn in extraction, link
///   resolution, list capture, or attachment indexing.
fn bench_file_index_build_profiles(c: &mut Criterion) {
    let mut group = c.benchmark_group("FileIndex::build/profiles");
    group.plot_config(
        PlotConfiguration::default().summary_scale(AxisScale::Logarithmic),
    );
    for &shape in BUILD_PROFILE_SHAPES {
        for n in quick_file_counts() {
            group.throughput(Throughput::Elements(
                u64::try_from(n).expect("note count fits u64"),
            ));
            group.bench_with_input(
                BenchmarkId::new(shape.name(), n),
                &n,
                |b, &n| {
                    b.iter_batched_ref(
                        || create_project(n, shape),
                        |temp| {
                            let index = IndexerService::new(temp.path())
                                .build()
                                .expect("build index");
                            observe_index(&index);
                            black_box(temp.path());
                            index
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

/// Measures [`IndexerService::refresh_with_report`] across baseline filesystem
/// states.
///
/// Parameters: varies note count and scenario (`no-op`, `single-upsert`,
/// `single-delete`, `linked-single-upsert`); reports note throughput.
///
/// Fixture: persisted projects and mutations are created outside timing. Every
/// timed refresh includes store open, full tree scan/diff, reconciliation, and
/// full [`FileIndex`] materialization.
///
/// Expected outcomes:
/// - No-op refresh avoids note reparsing and persisted writes after the common
///   scan/diff.
/// - Content-only updates add one-note parsing and patching; deletes may
///   trigger path-set/inlink rebuild work.
///
/// Unexpected outcomes:
/// - No-op cost grows beyond the shared scan/diff/materialization floor, or
///   content-only updates approach delete cost, indicating broken delta
///   detection, unnecessary writes, or avoidable full-vault recomputation.
fn bench_file_index_refresh(c: &mut Criterion) {
    let mut group = c.benchmark_group("FileIndex::refresh");
    group.plot_config(
        PlotConfiguration::default().summary_scale(AxisScale::Logarithmic),
    );
    for &n in WORKSPACE_FILE_COUNTS {
        group.throughput(Throughput::Elements(
            u64::try_from(n).expect("note count fits u64"),
        ));
        if n >= 10_000 {
            // 10,000+ note refreshes do real per-iteration disk I/O; bound
            // total suite runtime with criterion's minimum valid sample size
            // (10) instead of the default 100.
            group.sample_size(10);
        }

        group.bench_with_input(BenchmarkId::new("no-op", n), &n, |b, &n| {
            b.iter_batched_ref(
                || setup_persisted_project(n, ProjectShape::Plain),
                |(_temp, indexer)| observe_refresh(indexer),
                BatchSize::LargeInput,
            );
        });

        group.bench_with_input(
            BenchmarkId::new("single-upsert", n),
            &n,
            |b, &n| {
                b.iter_batched_ref(
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
                    |(_temp, indexer)| observe_refresh(indexer),
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
                        let (temp, indexer) =
                            setup_persisted_project(n, ProjectShape::Plain);
                        remove_note(temp.path(), ProjectShape::Plain, 0);
                        (temp, indexer)
                    },
                    |(_temp, indexer)| observe_refresh(indexer),
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
                    |(_temp, indexer)| observe_refresh(indexer),
                    BatchSize::LargeInput,
                );
            },
        );
    }
    group.finish();
}

/// Measures persisted-store [`QueryService::sync_and_run`] cost across vault
/// sizes and no-op vs. single-edit states.
///
/// Parameters: varies note count and state (`no-op`, `single-edit`); holds one
/// matching `#rare_0` tag selector fixed and reports note throughput.
///
/// Fixture: persisted tagged project and optional edit are created outside
/// timing. The timed call runs [`IndexerService::sync`] and then a store-backed
/// page query through `PATHS_BY_TAG`; it does not materialize a full
/// [`FileIndex`] unless the query path regresses.
///
/// Every call still performs the full filesystem scan/diff prelude needed to
/// detect changes. [`bench_file_index_refresh`] is only an external contrast
/// for the full-index materialization path, not a decomposed floor.
///
/// Expected outcomes:
/// - A content-only single edit stays close to the no-op state after the shared
///   scan/diff work, without adding full-table reads or full-index assembly.
///
/// Unexpected outcomes:
/// - Single-edit cost grows measurably faster than the no-op state as `n`
///   grows, indicating a full recompute, full-table scan, or full [`FileIndex`]
///   materialization snuck back into the sync or query path.
fn bench_sync_and_run(c: &mut Criterion) {
    let mut group = c.benchmark_group("QueryService::sync_and_run");
    group.plot_config(
        PlotConfiguration::default().summary_scale(AxisScale::Logarithmic),
    );
    let service = QueryService::new("class");
    let one_match =
        || SourceSelector::parse("#rare_0").expect("valid tag selector");

    for &n in WORKSPACE_FILE_COUNTS {
        group.throughput(Throughput::Elements(
            u64::try_from(n).expect("note count fits u64"),
        ));
        if n >= 10_000 {
            group.sample_size(10);
        }

        group.bench_with_input(BenchmarkId::new("no-op", n), &n, |b, &n| {
            b.iter_batched_ref(
                || {
                    (
                        setup_persisted_project(n, ProjectShape::Tagged),
                        Some(one_match()),
                    )
                },
                |((_temp, indexer), source)| {
                    observe_sync_and_run(
                        &service,
                        indexer,
                        source.take().expect("source set by setup"),
                    )
                },
                BatchSize::LargeInput,
            );
        });

        group.bench_with_input(
            BenchmarkId::new("single-edit", n),
            &n,
            |b, &n| {
                b.iter_batched_ref(
                    || {
                        let (temp, indexer) =
                            setup_persisted_project(n, ProjectShape::Tagged);
                        let mut content = tagged_note_source(1.min(n - 1));
                        content.push_str(
                            "\nBody change for sync_and_run benchmark.\n",
                        );
                        rewrite_note(
                            temp.path(),
                            ProjectShape::Tagged,
                            1.min(n - 1),
                            n,
                            &content,
                        );
                        ((temp, indexer), Some(one_match()))
                    },
                    |((_temp, indexer), source)| {
                        observe_sync_and_run(
                            &service,
                            indexer,
                            source.take().expect("source set by setup"),
                        )
                    },
                    BatchSize::LargeInput,
                );
            },
        );
    }
    group.finish();
}

/// Measures refresh cost for richer mutation shapes using only public lifecycle
/// APIs.
///
/// Parameters: varies quick note counts and scenario (`no-op-rich`,
/// `single-tag-upsert`, requested `many-upsert-10`/`many-upsert-100` capped at
/// `n`, `single-rich-delete`, `attachment-target-present`); reports note
/// throughput. Fixture setup and mutations are outside timing.
///
/// Expected outcomes:
/// - Rich no-op refresh remains near the plain no-op path from
///   [`bench_file_index_refresh`].
/// - Content-only updates scale with modified-note parsing and patching on top
///   of the shared scan/diff; the delete case is the full inlink-rebuild shape.
///
/// Unexpected outcomes:
/// - Tag/content updates approach delete cost or attachment no-op dominates,
///   suggesting secondary-index cleanup, patching, or store reads are scanning
///   too much persisted state.
fn bench_file_index_refresh_profiles(c: &mut Criterion) {
    let mut group = c.benchmark_group("FileIndex::refresh/profiles");
    group.plot_config(
        PlotConfiguration::default().summary_scale(AxisScale::Logarithmic),
    );
    for n in quick_file_counts() {
        group.throughput(Throughput::Elements(
            u64::try_from(n).expect("note count fits u64"),
        ));

        group.bench_with_input(
            BenchmarkId::new("no-op-rich", n),
            &n,
            |b, &n| {
                b.iter_batched_ref(
                    || setup_persisted_project(n, ProjectShape::RichRealistic),
                    |(_temp, indexer)| observe_refresh(indexer),
                    BatchSize::LargeInput,
                );
            },
        );

        group.bench_with_input(
            BenchmarkId::new("single-tag-upsert", n),
            &n,
            |b, &n| {
                b.iter_batched_ref(
                    || setup_single_tag_upsert(n),
                    |(_temp, indexer)| observe_refresh(indexer),
                    BatchSize::LargeInput,
                );
            },
        );

        for changed in [10_usize, 100] {
            group.bench_with_input(
                BenchmarkId::new(format!("many-upsert-{changed}"), n),
                &n,
                |b, &n| {
                    b.iter_batched_ref(
                        || setup_many_rich_upserts(n, changed),
                        |(_temp, indexer)| observe_refresh(indexer),
                        BatchSize::LargeInput,
                    );
                },
            );
        }

        group.bench_with_input(
            BenchmarkId::new("single-rich-delete", n),
            &n,
            |b, &n| {
                b.iter_batched_ref(
                    || setup_single_rich_delete(n),
                    |(_temp, indexer)| observe_refresh(indexer),
                    BatchSize::LargeInput,
                );
            },
        );

        group.bench_with_input(
            BenchmarkId::new("attachment-target-present", n),
            &n,
            |b, &n| {
                b.iter_batched_ref(
                    || {
                        setup_persisted_project(
                            n,
                            ProjectShape::AttachmentProject,
                        )
                    },
                    |(_temp, indexer)| observe_refresh(indexer),
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
/// Parameters: varies [`WORKSPACE_FILE_COUNTS`]; reports note throughput.
///
/// Fixture: `setup_unpersisted_project` creates files and builds the complete
/// in-memory [`FileIndex`] outside timing. Timed work persists that existing
/// index.
///
/// Expected outcomes:
/// - Cost scales linearly with note count.
///
/// Unexpected outcomes:
/// - Cost scaling super-linearly, indicating redundant row serialization,
///   excessive secondary-index writes, or unbounded transaction growth.
fn bench_index_persist(c: &mut Criterion) {
    let mut group = c.benchmark_group("FileIndex::persist");
    group.plot_config(
        PlotConfiguration::default().summary_scale(AxisScale::Logarithmic),
    );
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
            b.iter_batched_ref(
                || setup_unpersisted_project(n, ProjectShape::Plain),
                |(_temp, indexer, index)| {
                    indexer.persist(index).expect("persist index");
                    black_box(index.entries().len());
                },
                BatchSize::LargeInput,
            );
        });
    }
    group.finish();
}

/// Measures full persistence cost for richer note shapes at bounded suite
/// sizes.
///
/// Parameters: varies `BUILD_PROFILE_SHAPES` and `quick_file_counts`; reports
/// note throughput. Fixture creation and full in-memory index build happen
/// outside timing.
/// Compares externally with [`bench_index_persist`] for the plain-note anchor.
///
/// Expected outcomes:
/// - Persistence cost tracks each profile's stored payload: links, lists,
///   nested paths, and fixed attachment file rows.
///
/// Unexpected outcomes:
/// - List-heavy or rich persistence grows beyond its row/payload shape,
///   indicating row encoding or secondary-index write amplification.
fn bench_index_persist_profiles(c: &mut Criterion) {
    let mut group = c.benchmark_group("FileIndex::persist/profiles");
    group.plot_config(
        PlotConfiguration::default().summary_scale(AxisScale::Logarithmic),
    );
    for &shape in BUILD_PROFILE_SHAPES {
        for n in quick_file_counts() {
            group.throughput(Throughput::Elements(
                u64::try_from(n).expect("note count fits u64"),
            ));
            group.bench_with_input(
                BenchmarkId::new(shape.name(), n),
                &n,
                |b, &n| {
                    b.iter_batched_ref(
                        || setup_unpersisted_project(n, shape),
                        |(_temp, indexer, index)| {
                            indexer.persist(index).expect("persist index");
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
/// Parameters: sweeps plain projects over [`WORKSPACE_FILE_COUNTS`] and
/// rich/attachment projects over `quick_file_counts`; reports note throughput.
///
/// Fixture persistence is outside timing. Each timed iteration opens the store,
/// reads all persisted files/notes/inlinks, assembles a [`FileIndex`], and
/// observes it.
///
/// Expected outcomes:
/// - Load cost scales with stored files, notes, inlink rows, and fixture
///   payload size.
///
/// Unexpected outcomes:
/// - Rich or attachment loads exceed what their additional rows/payload
///   explain, indicating decode or assembly overhead.
fn bench_index_load(c: &mut Criterion) {
    let mut group = c.benchmark_group("FileIndex::load");
    group.plot_config(
        PlotConfiguration::default().summary_scale(AxisScale::Logarithmic),
    );
    for &n in WORKSPACE_FILE_COUNTS {
        let (_temp, indexer) = setup_persisted_project(n, ProjectShape::Plain);
        group.throughput(Throughput::Elements(
            u64::try_from(n).expect("note count fits u64"),
        ));
        if n >= 10_000 {
            group.sample_size(10);
        }
        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, _| {
            b.iter_with_large_drop(|| observe_load(black_box(&indexer)));
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
                    b.iter_with_large_drop(|| {
                        observe_load(black_box(&indexer))
                    });
                },
            );
        }
    }
    group.finish();
}

/// Measures public list-table reads over persisted list-heavy projects.
///
/// Parameters: varies `quick_file_counts`; each [`ProjectShape::ListHeavy`]
/// note contributes 20 list rows, so throughput is reported as `20 * n` rows.
/// Fixture persistence is outside timing. Timed work opens the store and reads
/// all list rows through [`IndexerService::read_lists`].
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
            b.iter_with_large_drop(|| {
                let lists = indexer.read_lists().expect("read lists");
                black_box(lists.len());
                black_box(lists)
            });
        });
    }
    group.finish();
}

// ----------------------------------------------------------- //
//              Benchmarks: Concurrent Operations              //
// ----------------------------------------------------------- //

/// Measures concurrent persisted-index load cost for four independent 250-note
/// projects.
///
/// Parameters: holds total loaded notes at 1,000; reports wall-clock time.
///
/// Fixture: four persisted plain projects are built outside timing; timed work
/// loads each project on one scoped thread.
///
/// Expected outcomes:
/// - Concurrent load remains stable across revisions for this fixed
///   four-project workload.
///
/// Unexpected outcomes:
/// - A regression relative to this benchmark's own history indicates possible
///   global locking, redb open contention, page-cache contention, or thread
///   scheduling overhead. Compare with an input-matched serial baseline before
///   claiming one-vs-four-project speedup.
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
                load_concurrently(projects)
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
    bench_sync_and_run,
    bench_file_index_refresh_profiles,
    bench_index_persist,
    bench_index_persist_profiles,
    bench_index_load,
    bench_read_lists,
    bench_concurrent_operations
);
criterion_main!(benches);
