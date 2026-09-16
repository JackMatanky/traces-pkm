//! Temporary project fixtures for filesystem benchmarks, plus in-memory index
//! fixtures for query benchmarks.
//!
//! Two fixture families live here:
//!
//! - **Filesystem fixtures** (`create_project`, `setup_persisted_project`,
//!   `setup_unpersisted_project`): a real directory tree, scanner output, and
//!   persisted `index.redb`. Use these only when the measured path needs disk.
//!   Every helper returning a [`TempDir`] expects the caller to keep it alive
//!   for the full measured operation.
//! - **In-memory fixtures** (`build_index`, `build_index_arc`,
//!   `build_index_arc_from_note_source`): a [`FileIndex`] assembled entirely on
//!   the heap via `FileIndex::new_test`, with zero disk I/O. Use these for
//!   query, sort, and filter benchmarks.
//!
//! Do not add ad-hoc path-writing helpers. All writes must flow through
//! `write_text_file` or `write_binary_file`, which reject absolute paths and
//! `..` before touching disk.

use std::{fs, path::Path, sync::Arc};

use tempfile::TempDir;
use traces_pkm::{
    FileIndex, IndexerService, build_test_index, resolve_safe_path, write_note,
};

use super::content::{ProjectShape, note_path, note_source};

/// Writes a binary fixture file beneath a temporary project root.
///
/// Used for attachment benchmarks that need real non-Markdown targets on disk.
/// Parent directories are created inside `root`.
///
/// # Panics
///
/// Panics if `relative` escapes `root`, if a parent directory cannot be
/// created, or if the file cannot be written.
fn write_binary_file<P: AsRef<Path>>(root: &Path, relative: P, bytes: &[u8]) {
    let path = resolve_safe_path(root, relative);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create fixture parent dir");
    }
    fs::write(path, bytes).expect("write binary fixture file");
}

/// Creates a temporary project containing `note_count` notes of `shape`.
///
/// The returned [`TempDir`] owns every fixture file. Keep it in the benchmark
/// input tuple until the measured operation is complete; dropping it removes
/// the project directory and all generated notes, attachments, and `index.redb`
/// files.
///
/// # Panics
///
/// Panics if the temporary directory cannot be created or a fixture file cannot
/// be written.
pub(crate) fn create_project(
    note_count: usize,
    shape: ProjectShape,
) -> TempDir {
    let temp = tempfile::tempdir().expect("create temp dir");
    for i in 0..note_count {
        let path = note_path(shape, i);
        let content = note_source(shape, i, note_count);
        write_note(temp.path(), &path, &content);
    }
    if shape == ProjectShape::AttachmentProject {
        write_attachment_files(temp.path());
    }
    temp
}
/// Builds an in-memory [`FileIndex`] for a `ProjectShape` without touching the
/// filesystem.
///
/// - Notes are parsed on the heap via `FileIndex::new_test`.
/// - File records carry each source's exact byte length.
/// - Inbound links are compiled in-process.
///
/// Use this for query, sort, and filter benchmarks. Fixture setup is excluded
/// from measurements: call this once before the benchmark loop and clone the
/// returned `Arc` inside iterations.
pub(crate) fn build_index(note_count: usize, shape: ProjectShape) -> FileIndex {
    let pairs: Vec<(String, String)> = (0..note_count)
        .map(|i| {
            (
                note_path(shape, i).display().to_string(),
                note_source(shape, i, note_count),
            )
        })
        .collect();
    let refs: Vec<(&str, &str)> =
        pairs.iter().map(|(p, c)| (p.as_str(), c.as_str())).collect();
    FileIndex::new_test(&refs)
}
/// Builds a shareable, in-memory [`FileIndex`] for a `ProjectShape`.
///
/// Clone the returned [`Arc`] inside Criterion iterations to exclude fixture
/// setup from the measurement.
///
/// # Panics
///
/// Panics if a generated note path or source is malformed.
pub(crate) fn build_index_arc(
    note_count: usize,
    shape: ProjectShape,
) -> Arc<FileIndex> {
    Arc::new(build_index(note_count, shape))
}

/// Builds an in-memory [`FileIndex`] from generated note content.
///
/// `note_source` receives `(note_index, note_count)` and must return full
/// Markdown content. Notes are written as `note-{i}.md` in memory only.
/// Handles errors by panicking internally.
pub(crate) fn build_index_arc_from_note_source(
    note_count: usize,
    note_source: impl Fn(usize, usize) -> String,
) -> Arc<FileIndex> {
    let pairs: Vec<(String, String)> = (0..note_count)
        .map(|i| (format!("note-{i}.md"), note_source(i, note_count)))
        .collect();
    let refs: Vec<(&str, &str)> =
        pairs.iter().map(|(p, c)| (p.as_str(), c.as_str())).collect();
    build_test_index(&refs)
}

/// Creates attachment files referenced by [`ProjectShape::AttachmentProject`].
///
/// Files are written under `root` using project-relative `assets/` and `docs/`
/// paths. Call this only with a [`TempDir`] project root.
///
/// # Panics
///
/// Panics if an attachment fixture cannot be written under `root`.
fn write_attachment_files(root: &Path) {
    for i in 0..20 {
        write_binary_file(
            root,
            format!("assets/image-{i}.png"),
            b"fixture image bytes",
        );
        write_binary_file(
            root,
            format!("docs/spec-{i}.pdf"),
            b"fixture pdf bytes",
        );
    }
}

/// Prepares a temporary project with a populated and persisted index.
///
/// Use this for refresh, load, list-read, and incremental persistence
/// benchmarks. The [`IndexerService`] points at the returned [`TempDir`], so
/// the guard must remain alive for every measured operation.
///
/// **Criterion trap**: never consume this tuple by value inside a
/// `b.iter_batched` routine without returning it. If `routine` destructures
/// `(TempDir, IndexerService)` and drops the `TempDir` internally, that drop
/// (recursively deleting every fixture file) runs *inside* the timed call. Use
/// `b.iter_batched_ref` so the routine only ever borrows `&mut (TempDir,
/// IndexerService)`, deferring the drop to after the batch is timed.
///
/// # Panics
///
/// Panics if the temporary project cannot be created, indexed, or persisted.
pub fn setup_persisted_project(
    note_count: usize,
    shape: ProjectShape,
) -> (TempDir, IndexerService) {
    let temp = create_project(note_count, shape);
    let indexer = IndexerService::new(temp.path());
    let index = indexer.build().expect("build index");
    indexer.persist(&index).expect("persist index");
    (temp, indexer)
}

/// Prepares a temporary project with a populated but unpersisted index.
///
/// Use this when the measured operation is the first persistence write. Keep
/// the [`TempDir`] alive with the returned [`IndexerService`] and
/// [`FileIndex`].
///
/// **Criterion trap**: same as [`setup_persisted_project`] — use
/// `b.iter_batched_ref`, never consume this tuple by value inside `routine`.
///
/// # Panics
///
/// Panics if the temporary project cannot be created or indexed.
pub(crate) fn setup_unpersisted_project(
    note_count: usize,
    shape: ProjectShape,
) -> (TempDir, IndexerService, FileIndex) {
    let temp = create_project(note_count, shape);
    let indexer = IndexerService::new(temp.path());
    let index = indexer.build().expect("build index");
    (temp, indexer, index)
}

/// Rewrites `note_index` inside a temporary project.
///
/// Pass the `temp.path()` returned by [`create_project`] or
/// [`setup_persisted_project`] as `root`. An empty `content` string regenerates
/// the shape's default note content for the same note index.
///
/// # Panics
///
/// Panics if the rewritten note path escapes `root` or cannot be written.
pub fn rewrite_note(
    root: &Path,
    shape: ProjectShape,
    note_index: usize,
    note_count: usize,
    content: &str,
) {
    let path = note_path(shape, note_index);
    let content = if content.is_empty() {
        note_source(shape, note_index, note_count)
    } else {
        content.to_owned()
    };
    write_note(root, &path, &content);
}

/// Removes `note_index` from a temporary project.
///
/// Pass the `temp.path()` returned by [`create_project`] or
/// [`setup_persisted_project`] as `root`.
///
/// # Panics
///
/// Panics if the note path escapes `root` or cannot be deleted.
pub(crate) fn remove_note(root: &Path, shape: ProjectShape, note_index: usize) {
    let path = resolve_safe_path(root, note_path(shape, note_index));
    fs::remove_file(path).expect("delete fixture note");
}
