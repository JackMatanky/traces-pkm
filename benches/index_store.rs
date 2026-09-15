//! Performance benchmarks for redb database storage and table operations
//! (`IndexStore`).
//!
//! Exposes and monitors the execution cost of database transaction commits,
//! full table deserialization into in-memory [`FileIndex`], selective table
//! scans (`read_lists`), and multi-project concurrent database access.
//!
//! ### Data Flow Diagram
//!
//! ```text
//! [FileIndex] ──(Persist Txn)──► [redb Tables: files, notes, inlinks, lists]
//!                                      │
//! [FileIndex] ◄──(Load Tables)─────────┘
//! ```
//!
//! ### Profiling Integration
//!
//! To profile index store CPU bottlenecks:
//! ```bash
//! cargo flamegraph --bench index_store -- --bench "FileIndex::load/1000"
//! ```

#![expect(
    clippy::expect_used,
    reason = "bench fixture/harness code; a failed .expect() here means the \
              fixture itself is broken and should panic immediately"
)]

use std::{hint::black_box, time::Duration};

use criterion::{
    AxisScale, BatchSize, BenchmarkId, Criterion, PlotConfiguration,
    Throughput, criterion_group, criterion_main,
};
use tempfile::TempDir;
use traces_pkm::{FileIndex, IndexerService};

#[expect(
    dead_code,
    reason = "shared benchmark common helpers are compiled into each bench \
              target; this target uses filesystem lifecycle helpers and sweeps"
)]
mod common;

use common::{
    PROFILE_CONTRAST_COUNTS, WORKSPACE_FILE_COUNTS,
    content::ProjectShape,
    project::{setup_persisted_project, setup_unpersisted_project},
};

// ----------------------------------------------------------- //
//                     Fixtures & Helpers                      //
// ----------------------------------------------------------- //

const PERSIST_CONTRAST_SHAPES: &[ProjectShape] = &[
    ProjectShape::LinkedDense,
    ProjectShape::RichRealistic,
    ProjectShape::ListHeavy,
];

const LOAD_CONTRAST_SHAPES: &[ProjectShape] =
    &[ProjectShape::RichRealistic, ProjectShape::AttachmentProject];

fn observe_index(index: &FileIndex) {
    let entries = index.entries();
    let note_count =
        entries.iter().filter(|entry| entry.note().is_some()).count();
    let inlink_count: usize =
        entries.iter().map(|entry| entry.inlinks().len()).sum();
    black_box((entries.len(), note_count, inlink_count));
}

fn observe_load(indexer: &IndexerService) -> FileIndex {
    let index = indexer.load().expect("load index");
    observe_index(&index);
    index
}

/// Loads multiple project indexes concurrently, one thread per project.
///
/// redb enforces at most one open [`redb::Database`] handle per file per
/// process, so multi-project workflows (such as batch importers) spawn one
/// thread per project.
fn load_concurrently(projects: &[(TempDir, IndexerService)]) -> Vec<FileIndex> {
    std::thread::scope(|scope| {
        let mut handles = Vec::with_capacity(projects.len());
        for (_, indexer) in projects {
            handles.push(scope.spawn(move || observe_load(indexer)));
        }
        let mut results = Vec::with_capacity(handles.len());
        for handle in handles {
            results.push(handle.join().expect("thread panicked"));
        }
        results
    })
}

// ----------------------------------------------------------- //
//                  Benchmarks: Index Persist                  //
// ----------------------------------------------------------- //

/// Measures full database persistence transaction overhead for plain notes.
///
/// Timed work persists a pre-built in-memory [`FileIndex`] into a fresh redb
/// database. Disk sync and fsync incur a fixed transaction commit floor (~34
/// ms) for small vaults (< 1,000 notes) before scaling linearly with table row
/// volume.
///
/// Expected outcomes:
/// - Fixed ~34 ms commit transaction floor dominates below 1,000 notes.
/// - Linear scaling above 1,000 notes as payload size and B-tree page writes
///   grow.
///
/// Unexpected outcomes:
/// - Super-linear scaling at 5K or 20K notes, indicating write amplification,
///   redundant table scans during commit, or unindexed B-tree splits.
fn bench_index_persist(c: &mut Criterion) {
    let mut group = c.benchmark_group("FileIndex::persist");
    group.plot_config(
        PlotConfiguration::default().summary_scale(AxisScale::Logarithmic),
    );
    for &n in WORKSPACE_FILE_COUNTS {
        group.throughput(Throughput::Elements(
            u64::try_from(n).expect("note count fits u64"),
        ));
        if n >= 5_000 {
            group.measurement_time(Duration::from_secs(2));
            group.sample_size(15);
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

/// Measures full persistence cost across contrasting note shapes.
///
/// Evaluates row encoding and secondary table write costs for dense links, rich
/// frontmatter, and list items across [`PROFILE_CONTRAST_COUNTS`].
///
/// Expected outcomes:
/// - Persistence time scales with the total number of table rows (lists, links,
///   notes) inserted into redb.
///
/// Unexpected outcomes:
/// - List-heavy or dense link profiles growing super-linearly, indicating table
///   lock contention or disproportionate index serialization overhead.
fn bench_index_persist_profiles(c: &mut Criterion) {
    let mut group = c.benchmark_group("FileIndex::persist/profiles");
    group.plot_config(
        PlotConfiguration::default().summary_scale(AxisScale::Logarithmic),
    );
    for &shape in PERSIST_CONTRAST_SHAPES {
        for &n in PROFILE_CONTRAST_COUNTS {
            group.throughput(Throughput::Elements(
                u64::try_from(n).expect("note count fits u64"),
            ));
            if n >= 5_000 {
                group.measurement_time(Duration::from_secs(2));
                group.sample_size(15);
            }
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

/// Measures public persisted index load and full in-memory materialization
/// cost.
///
/// Opening the redb store and reading all table keys introduces a fixed
/// open/iteration floor of ~14.5 ms. Above this floor, load time scales
/// linearly with total persisted note, inlink, and file row counts.
///
/// Expected outcomes:
/// - Constant ~14.5 ms floor below 1,000 notes.
/// - Linear scaling above 1,000 notes governed by row deserialization
///   throughput.
///
/// Unexpected outcomes:
/// - Load time growing super-linearly, indicating redundant inlink graph
///   recomputation or secondary table lookups during initial load.
fn bench_index_load(c: &mut Criterion) {
    let mut group = c.benchmark_group("FileIndex::load");
    group.plot_config(
        PlotConfiguration::default().summary_scale(AxisScale::Logarithmic),
    );

    // 1. Baseline sweep across all workspace sizes
    for &n in WORKSPACE_FILE_COUNTS {
        let (_temp, indexer) = setup_persisted_project(n, ProjectShape::Plain);
        group.throughput(Throughput::Elements(
            u64::try_from(n).expect("note count fits u64"),
        ));
        if n >= 5_000 {
            group.measurement_time(Duration::from_secs(2));
            group.sample_size(15);
        }
        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, _| {
            b.iter_with_large_drop(|| observe_load(black_box(&indexer)));
        });
    }

    // 2. Profile contrast sweep across two orders of magnitude
    for &shape in LOAD_CONTRAST_SHAPES {
        for &n in PROFILE_CONTRAST_COUNTS {
            let (_temp, indexer) = setup_persisted_project(n, shape);
            group.throughput(Throughput::Elements(
                u64::try_from(n).expect("note count fits u64"),
            ));
            if n >= 5_000 {
                group.measurement_time(Duration::from_secs(2));
                group.sample_size(15);
            }
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
/// Sweeps [`PROFILE_CONTRAST_COUNTS`]. Each [`ProjectShape::ListHeavy`] note
/// contributes 20 list rows, reporting throughput as `20 * n` rows.
///
/// Expected outcomes:
/// - Read cost scales linearly with persisted list row count.
///
/// Unexpected outcomes:
/// - Disproportionate latency per row or non-linear scaling across sizes.
fn bench_read_lists(c: &mut Criterion) {
    let mut group = c.benchmark_group("IndexerService::read_lists");
    group.plot_config(
        PlotConfiguration::default().summary_scale(AxisScale::Logarithmic),
    );
    for &n in PROFILE_CONTRAST_COUNTS {
        let (_temp, indexer) =
            setup_persisted_project(n, ProjectShape::ListHeavy);
        let list_rows = n.saturating_mul(20);
        group.throughput(Throughput::Elements(
            u64::try_from(list_rows).expect("list row count fits u64"),
        ));
        if n >= 5_000 {
            group.measurement_time(Duration::from_secs(2));
            group.sample_size(15);
        }
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
/// Total loaded notes held fixed at 1,000; tests concurrent database handle
/// initialization and table read throughput across scoped worker threads.
///
/// Expected outcomes:
/// - Stable execution time without thread starvation or file lock contention.
///
/// Unexpected outcomes:
/// - Latency spikes indicating process-wide mutex contention in table reads.
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
    bench_index_persist,
    bench_index_persist_profiles,
    bench_index_load,
    bench_read_lists,
    bench_concurrent_operations
);
criterion_main!(benches);
