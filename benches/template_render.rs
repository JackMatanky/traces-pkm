//! Performance benchmark suite for template rendering.
//!
//! Exposes and monitors the CPU cost of [`TemplateService::render_to_file`] in
//! [`WriteMode::DryRun`] (excludes disk-write cost) against pre-built,
//! pre-persisted projects swept up to 1000 notes, exercising `template` +
//! `index` + `note` together the way every `traces template`/`traces -i` render
//! does. Regressions here directly degrade render latency for template-driven
//! workflows.
//!
//! ### Data Flow Diagram
//!
//! ```text
//! [FileIndex] + [Template] + [Note]
//!   └──(TemplateService::render_to_file)──► [rendered output]
//!       (DryRun — no disk write)
//! ```
//!
//! ### Profiling Integration
//!
//! To profile template rendering CPU bottlenecks:
//! ```bash
//! cargo flamegraph --bench template_render -- --bench "TemplateService::render_to_file"
//! ```
//!
//! Run via `mise run bench -f template_render` (or `mise run bench -m
//! template`): this crate's `test-utils`-gated public surface
//! (`TemplateService`, `Config`, the `test_support` fixtures) is only reachable
//! with `--features test-utils`, which the mise task supplies.

#![expect(
    clippy::expect_used,
    reason = "bench fixture/harness code; a failed .expect() here means the \
              fixture itself is broken and should panic immediately"
)]
use std::{hint::black_box, sync::Arc};

use criterion::{
    AxisScale, BenchmarkId, Criterion, PlotConfiguration, Throughput,
    criterion_group, criterion_main,
};
use tempfile::TempDir;
use traces_pkm::{
    Config, IndexerService, PresetDialogProvider, TemplatePathInput,
    TemplateService, TestProject, WriteMode,
};

#[expect(
    dead_code,
    reason = "shared benchmark common helpers are compiled into each bench \
              target; this target uses only the quick size sweep"
)]
mod common;

use common::quick_file_counts;

// ----------------------------------------------------------- //
//                     Fixtures & Helpers                      //
// ----------------------------------------------------------- //

/// Builds a temporary project fixture populated with `n` synthetic notes, an
/// indexed database, and test templates.
///
/// Returns `(TempDir, PathBuf, Config)` where `TempDir` owns the lifetime of
/// the project on disk.
fn prepare_project(n: usize) -> (TempDir, std::path::PathBuf, Config) {
    let temp = tempfile::tempdir().expect("create temp dir");
    let root = temp.path().join("project");
    let project = TestProject::trusted(&root);
    for i in 0..n {
        let status = if i % 2 == 0 {
            "active"
        } else {
            "archived"
        };
        project.write_note(
            format!("notes/note-{i}.md"),
            &format!(
                "---\nrating: {}\nstatus: {status}\n---\n# Note {i}\n",
                i % 10
            ),
        );
    }
    project.write_template(
        "list_report.md",
        "{{ query.from() | list(\"file.path\") }}",
    );
    project.write_template(
        "table_report.md",
        r#"{{ query.from().where("rating >= 5").sort("file.name", false).table(["Path", "Rating", "Status"], ["file.path", "rating", "status"]) }}"#,
    );
    let _ = project.persist_index();
    let config = Config::test_default(&root).with_templates();
    (temp, root, config)
}

// ----------------------------------------------------------- //
//                         Benchmarks                          //
// ----------------------------------------------------------- //

/// Measures end-to-end template render latency in [`WriteMode::DryRun`].
///
/// Parameters: varies note count and benchmark shape (`refresh_floor`, `list`,
/// `table_filtered`); reports note throughput.
///
/// Fixture projects, config, dialog provider, service, and template path inputs
/// are built outside timing.
///
/// Runs `refresh_floor` before template rendering to isolate the persisted
/// project refresh prelude.
///
/// Subtraction formula:
/// - `list - refresh_floor`: Isolates template parsing, AST execution, and
///   output formatting from the ~12.5 ms project refresh prelude.
///
/// Expected outcomes:
/// - `refresh_floor` accounts for the common filesystem scan and database
///   reconciliation.
/// - `list - refresh_floor` scales with rendering `n` paths.
/// - `table_filtered` adds filter predicate evaluation, sort permutation, and
///   Markdown table generation.
///
/// Unexpected outcomes:
/// - Template rendering overhead dominating the refresh prelude unexpectedly,
///   or `table_filtered` growing super-linearly relative to `list`.
fn bench_render(c: &mut Criterion) {
    let mut group = c.benchmark_group("TemplateService::render_to_file");
    group.plot_config(
        PlotConfiguration::default().summary_scale(AxisScale::Logarithmic),
    );
    group.sample_size(10);

    for n in quick_file_counts() {
        group.throughput(Throughput::Elements(
            u64::try_from(n).expect("note count fits u64"),
        ));
        let (_temp, _root, config) = prepare_project(n);
        let dialog = Arc::new(PresetDialogProvider::new());
        let service = TemplateService::new(&config, dialog)
            .expect("valid schema directory");

        let indexer = IndexerService::new(config.root());
        group.bench_with_input(
            BenchmarkId::new("refresh_floor", n),
            &n,
            |b, _| {
                b.iter_with_large_drop(|| {
                    let (index, report) = indexer
                        .refresh_with_report()
                        .expect("refresh succeeded");
                    black_box((index, report))
                });
            },
        );

        let list_input =
            TemplatePathInput::parse(std::path::Path::new("list_report"))
                .expect("valid template input");
        group.bench_with_input(
            BenchmarkId::new("list", n),
            &list_input,
            |b, input| {
                b.iter_with_large_drop(|| {
                    let outcome = service
                        .render_to_file(input, None, WriteMode::DryRun)
                        .expect("render list report");
                    black_box(outcome)
                });
            },
        );
        let table_input =
            TemplatePathInput::parse(std::path::Path::new("table_report"))
                .expect("valid template input");
        group.bench_with_input(
            BenchmarkId::new("table_filtered", n),
            &table_input,
            |b, input| {
                b.iter_with_large_drop(|| {
                    let outcome = service
                        .render_to_file(input, None, WriteMode::DryRun)
                        .expect("render table report");
                    black_box(outcome)
                });
            },
        );
    }
    group.finish();
}

criterion_group!(benches, bench_render);
criterion_main!(benches);
