//! Performance benchmarks for differential synchronization and update
//! reconciliation (`IndexerService::refresh`).
//!
//! Exposes and monitors the execution cost of filesystem scan/diffing, store
//! updates, and incremental [`FileIndex`] reconciliation across clean and
//! mutated states.
//!
//! ### Data Flow Diagram
//!
//! ```text
//! [Disk Files] ──(mtime / diff)──► [Store Txn / Updates] ──(Reconcile)──► [FileIndex]
//! ```
//!
//! ### Profiling Integration
//!
//! To profile index refresh CPU bottlenecks:
//! ```bash
//! cargo flamegraph --bench index_refresh -- --bench "FileIndex::refresh/no-op/1000"
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
use traces_pkm::{
    FileIndex, IndexerService, QueryBuilder, QueryService, QuerySet,
    SourceSelector, SyncReport,
};

#[expect(
    dead_code,
    reason = "shared benchmark common helpers are compiled into each bench \
              target; this target uses filesystem lifecycle helpers and sweeps"
)]
mod common;

use common::{
    WORKSPACE_FILE_COUNTS,
    content::{
        ProjectShape, linked_note_source, plain_note_source, rich_note_source,
        tagged_note_source,
    },
    project::{remove_note, rewrite_note, setup_persisted_project},
};

// ----------------------------------------------------------- //
//                     Fixtures & Helpers                      //
// ----------------------------------------------------------- //

const MUTATION_ANCHOR_COUNTS: &[usize] = &[1_000, 5_000];

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

fn setup_single_edit(n: usize) -> (TempDir, IndexerService) {
    let (temp, indexer) = setup_persisted_project(n, ProjectShape::Tagged);
    let note_idx = 1.min(n.saturating_sub(1));
    let mut content = tagged_note_source(note_idx);
    content.push_str("\nBody change for sync_and_run benchmark.\n");
    rewrite_note(temp.path(), ProjectShape::Tagged, note_idx, n, &content);
    (temp, indexer)
}

fn bench_sync_and_run_scenario<F>(
    group: &mut criterion::BenchmarkGroup<'_, criterion::measurement::WallTime>,
    scenario: &'static str,
    n: usize,
    service: &QueryService,
    mut setup_source: F,
) where
    F: FnMut() -> (TempDir, IndexerService, SourceSelector),
{
    group.bench_with_input(BenchmarkId::new(scenario, n), &n, |b, _| {
        b.iter_batched_ref(
            || {
                let (temp, indexer, source) = setup_source();
                ((temp, indexer), Some(source))
            },
            |((_temp, indexer), source)| {
                observe_sync_and_run(
                    service,
                    indexer,
                    source.take().expect("source set by setup"),
                )
            },
            BatchSize::LargeInput,
        );
    });
}
/// Measures [`IndexerService::refresh_with_report`] across baseline filesystem
/// states.
///
/// Sweeps baseline no-op refresh across all [`WORKSPACE_FILE_COUNTS`]. Mutation
/// scenarios (`single-upsert`, `single-delete`, `linked-single-upsert`) are
/// anchored at 1,000 and 5,000 notes to verify that incremental patching does
/// not trigger full-vault recomputation.
///
/// Expected outcomes:
/// - No-op refresh scales with directory scan/diff without note parsing or
///   database writes.
/// - Single-upsert and delete times remain within constant factor of no-op at
///   anchor sizes.
///
/// Unexpected outcomes:
/// - No-op refresh scaling with full vault parse time, or single-note mutations
///   taking time proportional to full index rebuilds.
fn bench_file_index_refresh(c: &mut Criterion) {
    let mut group = c.benchmark_group("FileIndex::refresh");
    group.plot_config(
        PlotConfiguration::default().summary_scale(AxisScale::Logarithmic),
    );

    // 1. Baseline no-op sweep across all workspace sizes
    for &n in WORKSPACE_FILE_COUNTS {
        group.throughput(Throughput::Elements(
            u64::try_from(n).expect("note count fits u64"),
        ));
        let (_temp, indexer) = setup_persisted_project(n, ProjectShape::Plain);
        group.bench_with_input(BenchmarkId::new("no-op", n), &n, |b, _| {
            b.iter(|| observe_refresh(&indexer));
        });
    }

    // 2. Mutation scenarios at anchor sizes (1K, 5K)
    for &n in MUTATION_ANCHOR_COUNTS {
        group.throughput(Throughput::Elements(
            u64::try_from(n).expect("note count fits u64"),
        ));
        group.measurement_time(Duration::from_secs(2));
        group.sample_size(15);

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

/// Measures persisted-store [`QueryService::sync_and_run`] cost across
/// workspace sizes.
///
/// Sweeps four key operational points:
/// - `zero-row`: Tag selector `#no_such_tag_benchmark` (measures store open +
///   scan/diff prelude).
/// - `no-op`: Tag selector `#rare_0` (single-tag point lookup).
/// - `single-edit`: Tag selector `#rare_0` after content modification.
/// - `full_vault_scan`: Selector [`SourceSelector::All`] (all notes read and
///   decoded).
///
/// Subtraction formulas:
/// - `no-op - zero-row`: Isolates tag index lookup cost.
/// - `full_vault_scan - zero-row`: Isolates full-table row decode cost.
/// - `refresh no-op - zero-row`: Isolates full [`FileIndex`] materialization
///   cost.
///
/// Expected outcomes:
/// - `zero-row` tracks filesystem scan/diff time without query work.
/// - `no-op` adds negligible tag point-lookup overhead over `zero-row`.
/// - `single-edit` updates only dirty notes without triggering full-vault scan.
///
/// Unexpected outcomes:
/// - Single-tag query scaling with total vault size, or single-edit triggering
///   full-vault reload.
fn bench_sync_and_run(c: &mut Criterion) {
    let mut group = c.benchmark_group("QueryService::sync_and_run");
    group.plot_config(
        PlotConfiguration::default().summary_scale(AxisScale::Logarithmic),
    );
    let service = QueryService::new("class");
    let zero_match = || {
        SourceSelector::parse("#no_such_tag_benchmark")
            .expect("valid tag selector")
    };
    let one_match =
        || SourceSelector::parse("#rare_0").expect("valid tag selector");

    for &n in WORKSPACE_FILE_COUNTS {
        group.throughput(Throughput::Elements(
            u64::try_from(n).expect("note count fits u64"),
        ));
        let (_temp, indexer) = setup_persisted_project(n, ProjectShape::Tagged);

        group.bench_with_input(BenchmarkId::new("zero-row", n), &n, |b, _| {
            b.iter(|| observe_sync_and_run(&service, &indexer, zero_match()));
        });

        group.bench_with_input(BenchmarkId::new("no-op", n), &n, |b, _| {
            b.iter(|| observe_sync_and_run(&service, &indexer, one_match()));
        });

        bench_sync_and_run_scenario(
            &mut group,
            "single-edit",
            n,
            &service,
            || {
                let (temp, edit_indexer) = setup_single_edit(n);
                (temp, edit_indexer, one_match())
            },
        );

        group.bench_with_input(
            BenchmarkId::new("full_vault_scan", n),
            &n,
            |b, _| {
                b.iter(|| {
                    observe_sync_and_run(
                        &service,
                        &indexer,
                        SourceSelector::All,
                    )
                });
            },
        );
    }
    group.finish();
}

/// Measures refresh cost for richer mutation shapes at anchor contrast sizes.
///
/// Sweeps rich realistic note mutations across [`MUTATION_ANCHOR_COUNTS`]
/// (`[1_000, 5_000]`) covering no-op, single-tag upsert, multi-note upsert, and
/// single note deletion.
///
/// Expected outcomes:
/// - Rich no-op refresh scales with scan/diff without reparsing unchanged
///   notes.
/// - Single tag and multi-upsert costs scale with the number of mutated files
///   only.
///
/// Unexpected outcomes:
/// - Multi-upsert or delete triggering secondary index reconstruction beyond
///   the modified delta.
fn bench_file_index_refresh_profiles(c: &mut Criterion) {
    let mut group = c.benchmark_group("FileIndex::refresh/profiles");
    group.plot_config(
        PlotConfiguration::default().summary_scale(AxisScale::Logarithmic),
    );
    group.measurement_time(Duration::from_secs(2));
    group.sample_size(15);

    for &n in MUTATION_ANCHOR_COUNTS {
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

        group.bench_with_input(
            BenchmarkId::new("many-upsert-10", n),
            &n,
            |b, &n| {
                b.iter_batched_ref(
                    || setup_many_rich_upserts(n, 10),
                    |(_temp, indexer)| observe_refresh(indexer),
                    BatchSize::LargeInput,
                );
            },
        );

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
    }
    group.finish();
}

criterion_group!(
    benches,
    bench_file_index_refresh,
    bench_sync_and_run,
    bench_file_index_refresh_profiles
);
criterion_main!(benches);
