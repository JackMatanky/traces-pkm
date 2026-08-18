//! Benches `traces_pkm::TemplateService::render_to_file` in `WriteMode::DryRun`
//! (excludes disk-write cost) against a pre-built, pre-persisted 1000-note
//! project, exercising `template` + `index` + `note` together the way every
//! `traces template`/`traces -i` render does.
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
use std::sync::Arc;

use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use traces_pkm::{
    Config, PresetDialogProvider, TemplatePathInput, TemplateService,
    WriteMode, create_trusted_project, fixture_service, write_note,
    write_template,
};

fn prepared_root() -> std::path::PathBuf {
    let temp = tempfile::tempdir().expect("create temp dir");
    let root = temp.path().join("project");
    let service = fixture_service(temp.path());
    let _ = create_trusted_project(&service, &root);
    for i in 0..1000 {
        write_note(
            &root,
            &format!("notes/note-{i}.md"),
            &format!("# Note {i}\n"),
        );
    }
    write_template(
        &root,
        "report.md",
        "{{ query.from() | list(\"file.path\") }}",
    );
    // `keep()` intentionally leaks the directory: the setup closure's directory
    // must outlive the measured `render_to_file` call below, so an ordinary
    // `TempDir` drop inside `routine` would count toward the *measured* time
    // and pollute this benchmark with filesystem cleanup cost.
    let _ = temp.keep();
    root
}

/// Renders a template listing every path over a pre-built 1000-note project, in
/// `WriteMode::DryRun`.
///
/// The render path every `traces template`/`-i` invocation pays (see module
/// docs); `DryRun` isolates render cost from disk-write cost so a regression
/// here is unambiguous rather than muddied by I/O variance.
fn bench_render(c: &mut Criterion) {
    c.bench_function("TemplateService::render_to_file", |b| {
        b.iter_batched(
            prepared_root,
            |root| {
                let config = Config::for_test(
                    root.clone(),
                    Some(root.join("templates")),
                    None,
                    root.clone(),
                );
                let service = TemplateService::new(
                    &config,
                    Arc::new(PresetDialogProvider::new()),
                )
                .expect("valid schema directory");
                let input =
                    TemplatePathInput::parse(std::path::Path::new("report"))
                        .expect("valid template input");
                service
                    .render_to_file(&input, None, WriteMode::DryRun)
                    .expect("render report")
            },
            BatchSize::LargeInput,
        );
    });
}

criterion_group!(benches, bench_render);
criterion_main!(benches);
