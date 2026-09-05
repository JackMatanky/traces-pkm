//! Performance benchmark suite for the inbound link (backlink) graph compiler.
//!
//! Exposes and monitors the execution cost of building and querying
//! [`InlinkMap`], isolating in-memory graph resolution from disk I/O and
//! database transactions.
//!
//! ### Evaluated Operations
//! - Graph compilation across graph topologies:
//!   - `sparse`: Chain topology (each note links to its successor).
//!   - `dense`: Hub topology (each note links to 20 distinct targets).
//!   - `ambiguous`: Same-stem file collisions across directories testing folder
//!     proximity.
//!   - `attachments`: Links targeting non-note media files (images, PDFs).
//! - Direct slice lookups via [`InlinkMap::inlinks_of`] and
//!   [`InlinkMap::contains_target`].
//!
//! ### Profiling Integration
//! ```bash
//! cargo flamegraph --bench index_inlinks -- --bench "InlinkMap::new/dense/1000"
//! ```

#![expect(
    clippy::expect_used,
    clippy::arithmetic_side_effects,
    reason = "bench fixture/harness code; a failed .expect() here means the \
              fixture itself is broken and should panic immediately"
)]

use std::{fmt::Write as _, hint::black_box, path::PathBuf};

use criterion::{
    BenchmarkId, Criterion, Throughput, criterion_group, criterion_main,
};
use traces_pkm::{
    FileBase, InlinkMap, MarkdownParserInput, Note, parse_markdown,
};

const WORKSPACE_SIZES: &[usize] = &[100, 1_000, 10_000, 20_000];

// ----------------------------------------------------------- //
//                     Fixtures & Generators                   //
// ----------------------------------------------------------- //

fn files_for_notes(notes: &[Note]) -> Vec<FileBase> {
    let mut files: Vec<FileBase> = notes
        .iter()
        .map(|n| {
            FileBase::new_note_test(
                n.path().to_path_buf(),
                n.path()
                    .parent()
                    .map_or_else(PathBuf::new, std::path::Path::to_path_buf),
            )
        })
        .collect();
    files.sort_by(|a, b| a.path().cmp(b.path()));
    files
}

/// Generates `n` notes in a chain topology where each note links to the next
/// note.
fn generate_notes_sparse(n: usize) -> Vec<Note> {
    let mut notes = Vec::with_capacity(n);
    for i in 0..n {
        let path = format!("note-{i}.md");
        let content =
            format!("# Note {i}\n\nLink to [[note-{}]]\n", (i + 1) % n);
        let input = MarkdownParserInput::for_test(
            std::path::Path::new(&path),
            &content,
        );
        notes.push(parse_markdown(&input));
    }
    notes.sort_by(|a, b| a.path().cmp(b.path()));
    notes
}

/// Generates `n` notes where each note links to 20 other notes, creating a
/// dense hub graph.
fn generate_notes_dense(n: usize) -> Vec<Note> {
    let mut notes = Vec::with_capacity(n);
    for i in 0..n {
        let path = format!("note-{i}.md");
        let mut content = format!("# Note {i}\n\n");
        for j in 0..20 {
            let target = (i + j) % n;
            let _ = writeln!(content, "- Link to [[note-{target}]]");
        }
        let input = MarkdownParserInput::for_test(
            std::path::Path::new(&path),
            &content,
        );
        notes.push(parse_markdown(&input));
    }
    notes.sort_by(|a, b| a.path().cmp(b.path()));
    notes
}

/// Generates `n` notes with same-stem collisions across multiple folders,
/// forcing proximity-based tie-breaking calculations.
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
        let input = MarkdownParserInput::for_test(
            std::path::Path::new(&path),
            &content,
        );
        notes.push(parse_markdown(&input));
    }
    notes.sort_by(|a, b| a.path().cmp(b.path()));
    notes
}

/// Generates `n` notes that link to attachment files (images and PDFs).
fn generate_notes_with_attachments(n: usize) -> (Vec<Note>, Vec<FileBase>) {
    let mut notes = Vec::with_capacity(n);
    let mut files = Vec::with_capacity(n + 20);

    for i in 0..n {
        let path = format!("note-{i}.md");
        let attachment_id = i % 20;
        let content = format!(
            "# Note {i}\n\nEmbedded image: \
             ![[image-{attachment_id}.png]]\nAttachment doc: \
             [[spec-{attachment_id}.pdf]]\n"
        );
        let input = MarkdownParserInput::for_test(
            std::path::Path::new(&path),
            &content,
        );
        notes.push(parse_markdown(&input));
        files.push(FileBase::new_note_test(
            PathBuf::from(&path),
            PathBuf::new(),
        ));
    }

    for i in 0..20 {
        files.push(FileBase::new_note_test(
            PathBuf::from(format!("assets/image-{i}.png")),
            PathBuf::from("assets"),
        ));
        files.push(FileBase::new_note_test(
            PathBuf::from(format!("docs/spec-{i}.pdf")),
            PathBuf::from("docs"),
        ));
    }

    notes.sort_by(|a, b| a.path().cmp(b.path()));
    files.sort_by(|a, b| a.path().cmp(b.path()));
    (notes, files)
}

// ----------------------------------------------------------- //
//               Benchmarks: InlinkMap Compilation             //
// ----------------------------------------------------------- //

fn bench_inlink_map_new(c: &mut Criterion) {
    let mut group = c.benchmark_group("InlinkMap::new");
    for &n in WORKSPACE_SIZES {
        group.throughput(Throughput::Elements(
            u64::try_from(n).expect("note count fits u64"),
        ));
        if n >= 10_000 {
            // 10,000+ note dense graphs have 200,000+ links; bound total
            // suite runtime with criterion's minimum sample size (10).
            group.sample_size(10);
        }

        group.bench_with_input(BenchmarkId::new("sparse", n), &n, |b, &n| {
            let notes = generate_notes_sparse(n);
            let files = files_for_notes(&notes);
            b.iter(|| {
                let inlinks =
                    InlinkMap::new(black_box(&notes), black_box(&files));
                black_box(inlinks);
            });
        });

        group.bench_with_input(BenchmarkId::new("dense", n), &n, |b, &n| {
            let notes = generate_notes_dense(n);
            let files = files_for_notes(&notes);
            b.iter(|| {
                let inlinks =
                    InlinkMap::new(black_box(&notes), black_box(&files));
                black_box(inlinks);
            });
        });

        group.bench_with_input(
            BenchmarkId::new("ambiguous", n),
            &n,
            |b, &n| {
                let notes = generate_notes_ambiguous(n);
                let files = files_for_notes(&notes);
                b.iter(|| {
                    let inlinks =
                        InlinkMap::new(black_box(&notes), black_box(&files));
                    black_box(inlinks);
                });
            },
        );

        group.bench_with_input(
            BenchmarkId::new("attachments", n),
            &n,
            |b, &n| {
                let (notes, files) = generate_notes_with_attachments(n);
                b.iter(|| {
                    let inlinks =
                        InlinkMap::new(black_box(&notes), black_box(&files));
                    black_box(inlinks);
                });
            },
        );
    }
    group.finish();
}

// ----------------------------------------------------------- //
//               Benchmarks: InlinkMap Accessors               //
// ----------------------------------------------------------- //

fn bench_inlink_map_accessors(c: &mut Criterion) {
    let mut group = c.benchmark_group("InlinkMap::accessors");
    let n = 1_000;
    let notes = generate_notes_dense(n);
    let files = files_for_notes(&notes);
    let inlinks = InlinkMap::new(&notes, &files);

    let hub_target = std::path::Path::new("note-500.md");
    let missing_target = std::path::Path::new("nonexistent.md");

    group.bench_function("inlinks_of_hit", |b| {
        b.iter(|| {
            let sources = inlinks.inlinks_of(black_box(hub_target));
            black_box(sources);
        });
    });

    group.bench_function("inlinks_of_miss", |b| {
        b.iter(|| {
            let sources = inlinks.inlinks_of(black_box(missing_target));
            black_box(sources);
        });
    });

    group.bench_function("contains_target_hit", |b| {
        b.iter(|| {
            let found = inlinks.contains_target(black_box(hub_target));
            black_box(found);
        });
    });

    group.bench_function("contains_target_miss", |b| {
        b.iter(|| {
            let found = inlinks.contains_target(black_box(missing_target));
            black_box(found);
        });
    });

    group.bench_function("iter_all_edges", |b| {
        b.iter(|| {
            let count = inlinks.iter().count();
            black_box(count);
        });
    });

    group.finish();
}

criterion_group!(benches, bench_inlink_map_new, bench_inlink_map_accessors,);
criterion_main!(benches);
