//! Performance benchmark suite for template rendering.
//!
//! Exposes and monitors the CPU cost of [`TemplateService::render_to_file`] in
//! [`WriteMode::DryRun`] (excludes disk-write cost) against a pre-built,
//! pre-persisted 1000-note project, exercising `template` + `index` + `note`
//! together the way every `traces template`/`traces -i` render does.
//! Regressions here directly degrade render latency for template-driven
//! workflows.
//!
//! ### Data Flow Diagram
//!
//! ```text
//! [FileIndex] + [Template] + [Note] ──(TemplateService::render_to_file)──► [rendered output]
//!                                         (DryRun — no disk write)
//! ```
//!
//! ### Profiling Integration
//!
//! To profile template rendering CPU bottlenecks:
//! ```bash
//! cargo flamegraph --bench template_render -- --bench "TemplateService::render_to_file"
//! ```
//!
//! Run via `mise run bench`, not bare `cargo bench`: this crate's
//! `test-utils`-gated public surface (`TemplateService`, `Config`, the
//! `test_support` fixtures) is only reachable with `--features test-utils`,
//! which the mise task supplies.

#![expect(
    clippy::expect_used,
    reason = "bench fixture/harness code; a failed .expect() here means the \
              fixture itself is broken and should panic immediately"
)]
use std::{hint::black_box, sync::Arc};

use criterion::{
    BenchmarkId, Criterion, Throughput, criterion_group, criterion_main,
};
use tempfile::TempDir;
use traces_pkm::{
    Config, IndexerService, PresetDialogProvider, TemplatePathInput,
    TemplateService, WriteMode, create_trusted_project, fixture_service,
    write_note, write_template,
};

/// Builds a temporary project fixture populated with `n` synthetic notes,
/// an indexed database, and test templates.
///
/// Returns `(TempDir, PathBuf, Config, PresetDialogProvider)` where `TempDir`
/// owns the lifetime of the project on disk.
fn prepare_fixture(n: usize) -> (TempDir, std::path::PathBuf, Config) {
    let temp = tempfile::tempdir().expect("create temp dir");
    let root = temp.path().join("project");
    let service = fixture_service(temp.path());
    let _ = create_trusted_project(&service, &root);
    for i in 0..n {
        write_note(
            &root,
            &format!("notes/note-{i}.md"),
            &format!(
                "---\nrating: {}\nstatus: {}\n---\n# Note {i}\nBody content \
                 for note {i}.\n",
                i % 10,
                if i % 2 == 0 {
                    "active"
                } else {
                    "archived"
                }
            ),
        );
    }
    write_template(
        &root,
        "list_report.md",
        "{{ query.from() | list(\"file.path\") }}",
    );
    write_template(
        &root,
        "table_report.md",
        "{{ query.from() | where(\"rating\", \">=\", 5) | sort(\"file.name\") \
         | table(\"file.path\", \"rating\", \"status\") }}",
    );
    let indexer = IndexerService::new(&root);
    indexer
        .persist(&indexer.build().expect("build index"))
        .expect("persist index");
    let config = Config::for_test(
        root.clone(),
        Some(root.join("templates")),
        None,
        root.clone(),
    );
    (temp, root, config)
}

/// Measures template rendering cost over a pre-built 1000-note project, in
/// `WriteMode::DryRun`.
///
/// The render path every `traces template`/`-i` invocation pays (see module
/// docs); `DryRun` isolates render cost from disk-write cost so a regression
/// here is unambiguous rather than muddied by I/O variance.
///
/// Expected outcomes:
/// - Render time is dominated by index query + template expansion, not fixture
///   setup or cleanup.
///
/// Unexpected outcomes:
/// - Render cost scales super-linearly with note count, indicating unbounded
///   template expansion or redundant index scans per row.
fn bench_render(c: &mut Criterion) {
    let mut group = c.benchmark_group("TemplateService::render_to_file");

    for n in [100_usize, 1_000] {
        group.throughput(Throughput::Elements(n as u64));
        let (_temp, _root, config) = prepare_fixture(n);
        let dialog = Arc::new(PresetDialogProvider::new());
        let service = TemplateService::new(&config, dialog)
            .expect("valid schema directory");

        let list_input =
            TemplatePathInput::parse(std::path::Path::new("list_report"))
                .expect("valid template input");
        group.bench_with_input(
            BenchmarkId::new("list", n),
            &list_input,
            |b, input| {
                b.iter(|| {
                    let outcome = service
                        .render_to_file(input, None, WriteMode::DryRun)
                        .expect("render list report");
                    black_box(outcome);
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
                b.iter(|| {
                    let outcome = service
                        .render_to_file(input, None, WriteMode::DryRun)
                        .expect("render table report");
                    black_box(outcome);
                });
            },
        );
    }
    group.finish();
}

criterion_group!(benches, bench_render);
criterion_main!(benches);
