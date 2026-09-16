//! Performance benchmarks for clean index construction
//! (`IndexerService::build`).
//!
//! Exposes and monitors the execution cost of full filesystem scanning, note
//! parsing, file path sorting, and in-memory [`FileIndex`] compilation from
//! disk state.
//!
//! ### Data Flow Diagram
//!
//! ```text
//! [Files on Disk] ──(WalkDir Scan)──► [FileBase / Notes]
//!                                             │
//!                                             └──(Compile)──► [FileIndex]
//! ```
//!
//! ### Profiling Integration
//!
//! To profile index build CPU bottlenecks:
//! ```bash
//! cargo flamegraph --bench index_build -- --bench "FileIndex::build/1000"
//! ```
//!
//! Run via `mise run bench -f index_build` (or `mise run bench -m index`): this
//! crate's `test-utils`-gated public surface is only reachable with `--features
//! test-utils`, which the mise task supplies.
#![expect(
    clippy::expect_used,
    reason = "bench fixture/harness code; a failed .expect() here means the \
              fixture itself is broken and should panic immediately"
)]

use std::hint::black_box;

use criterion::{
    AxisScale, BenchmarkId, Criterion, PlotConfiguration, Throughput,
    criterion_group, criterion_main,
};
use traces_pkm::{FileIndex, IndexerService};
#[expect(
    dead_code,
    reason = "shared benchmark common helpers are compiled into each bench \
              target; this target uses filesystem lifecycle helpers and sweeps"
)]
mod common;

use common::{
    PROFILE_CONTRAST_COUNTS, WORKSPACE_FILE_COUNTS, content::ProjectShape,
    project::create_project,
};

// ----------------------------------------------------------- //
//                     Fixtures & Helpers                      //
// ----------------------------------------------------------- //

const BUILD_CONTRAST_SHAPES: &[ProjectShape] = &[
    ProjectShape::LinkedDense,
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

// ----------------------------------------------------------- //
//                   Benchmarks: Index Build                   //
// ----------------------------------------------------------- //

/// Measures wall-clock index build time over the baseline plain-note fixture.
///
/// Parameters: varies note count across [`WORKSPACE_FILE_COUNTS`]; reports note
/// throughput.
///
/// Fixture: temporary plain-note project created once per tier outside timing.
/// Timed work scans the filesystem, parses notes, extracts tags, resolves
/// links, and compiles inlinks into a complete [`FileIndex`]. Path sorting adds
/// an $n \cdot \ln(n)$ component to the linear scan and parse baseline.
///
/// Expected outcomes:
/// - Near-linear $O(n \log n)$ scaling governed primarily by single-pass note
///   parsing with minor logarithmic growth from deterministic path sorting.
///
/// Unexpected outcomes:
/// - Quadratic or super-linear scaling, indicating repeated full-vault scans,
///   accidental duplicate walks, or avoidable intermediate collections in link
///   graph construction.
fn bench_file_index_build(c: &mut Criterion) {
    let mut group = c.benchmark_group("FileIndex::build");
    group.plot_config(
        PlotConfiguration::default().summary_scale(AxisScale::Logarithmic),
    );
    for &n in WORKSPACE_FILE_COUNTS {
        group.throughput(Throughput::Elements(
            u64::try_from(n).expect("note count fits u64"),
        ));
        let temp = create_project(n, ProjectShape::Plain);
        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, _| {
            b.iter(|| {
                let index = IndexerService::new(temp.path())
                    .build()
                    .expect("build index");
                observe_index(&index);
                black_box(temp.path());
                index
            });
        });
    }
    group.finish();
}

/// Measures build cost across contrasting fixture profiles at two orders of
/// magnitude.
///
/// Parameters: varies shape across [`BUILD_CONTRAST_SHAPES`] and size across
/// [`PROFILE_CONTRAST_COUNTS`]; reports note throughput.
///
/// Fixture: temporary directory created once per shape and size outside timing.
/// Compares dense link graphs, rich realistic note shapes, and binary
/// attachments against the plain baseline. Redundant shapes (`tagged`,
/// `classified`, `list_heavy`) whose build scaling matches plain notes are
/// omitted.
///
/// Expected outcomes:
/// - Rich realistic and dense link shapes exhibit higher constant factors due
///   to link extraction and frontmatter parsing, but retain identical $O(n \log
///   n)$ scaling.
///
/// Unexpected outcomes:
/// - Dense link or attachment profiles growing super-linearly, indicating graph
///   allocation churn or quadratic link resolution overhead.
fn bench_file_index_build_profiles(c: &mut Criterion) {
    let mut group = c.benchmark_group("FileIndex::build/profiles");
    group.plot_config(
        PlotConfiguration::default().summary_scale(AxisScale::Logarithmic),
    );
    for &shape in BUILD_CONTRAST_SHAPES {
        for &n in PROFILE_CONTRAST_COUNTS {
            group.throughput(Throughput::Elements(
                u64::try_from(n).expect("note count fits u64"),
            ));
            let temp = create_project(n, shape);
            group.bench_with_input(
                BenchmarkId::new(shape.name(), n),
                &n,
                |b, _| {
                    b.iter(|| {
                        let index = IndexerService::new(temp.path())
                            .build()
                            .expect("build index");
                        observe_index(&index);
                        black_box(temp.path());
                        index
                    });
                },
            );
        }
    }
    group.finish();
}

criterion_group!(
    benches,
    bench_file_index_build,
    bench_file_index_build_profiles
);
criterion_main!(benches);
