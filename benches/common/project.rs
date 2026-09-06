//! Temporary project and index fixtures for filesystem benchmarks.
//!
//! Use this module only when the measured path needs a real directory tree,
//! scanner output, persisted `index.redb`, or filesystem-backed attachments.
//! For pure parser/link-graph benchmarks, use [`super::content`] and
//! [`super::notes`] so the benchmark setup stays cheaper and clearer.
//!
//! Every public helper that returns a [`TempDir`] expects the caller to keep it
//! alive for the full measured operation. Helpers that return only a
//! [`FileIndex`] intentionally drop the temporary project after indexing; use
//! those when query execution should not include filesystem lifetime concerns.
//!
//! Do not add ad-hoc path-writing helpers. All writes must flow through
//! `write_text_file` or `write_binary_file`, which reject absolute paths and
//! `..` before touching disk.

use std::{
    fs,
    path::{Component, Path, PathBuf},
    sync::Arc,
};

use tempfile::TempDir;
use traces_pkm::{FileIndex, IndexerService};

use super::content::{ProjectShape, note_path, note_source};

/// Resolves a project-relative fixture path beneath `root`.
///
/// Use this before every benchmark fixture file write or delete. It prevents
/// accidental writes outside the [`TempDir`] by rejecting absolute paths and
/// parent-directory traversal.
///
/// # Panics
///
/// Panics if `relative` is absolute or contains a `..` component.
fn fixture_path(root: &Path, relative: &Path) -> PathBuf {
    assert!(
        relative.is_relative()
            && !relative.components().any(|part| part == Component::ParentDir),
        "fixture path must stay inside temporary project: {}",
        relative.display()
    );
    root.join(relative)
}

/// Writes a UTF-8 fixture file beneath a temporary project root.
///
/// `relative` is interpreted as a project-relative path, so callers should pass
/// values such as `note-0.md` or `areas/project/note.md`, never paths from the
/// repository checkout. Parent directories are created inside `root`.
///
/// # Panics
///
/// Panics if `relative` escapes `root`, if a parent directory cannot be
/// created, or if the file cannot be written.
fn write_text_file(root: &Path, relative: &Path, content: &str) {
    let path = fixture_path(root, relative);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create fixture parent dir");
    }
    fs::write(path, content).expect("write fixture file");
}

/// Writes a binary fixture file beneath a temporary project root.
///
/// Used for attachment benchmarks that need real non-Markdown targets on disk.
/// Parent directories are created inside `root`.
///
/// # Panics
///
/// Panics if `relative` escapes `root`, if a parent directory cannot be
/// created, or if the file cannot be written.
fn write_binary_file(root: &Path, relative: &Path, bytes: &[u8]) {
    let path = fixture_path(root, relative);
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
        write_text_file(temp.path(), &path, &content);
    }
    if shape == ProjectShape::AttachmentProject {
        write_attachment_files(temp.path());
    }
    temp
}
/// Creates a temporary project from generated note content.
///
/// `note_source` receives `(note_index, note_count)` and must return full
/// Markdown note content. Notes are written as `note-{i}.md` under the returned
/// [`TempDir`].
///
/// # Panics
///
/// Panics if the temporary directory cannot be created or a fixture note cannot
/// be written.
fn create_project_from_note_source(
    note_count: usize,
    note_source: impl Fn(usize, usize) -> String,
) -> TempDir {
    let temp = tempfile::tempdir().expect("create temp dir");
    for i in 0..note_count {
        let path = PathBuf::from(format!("note-{i}.md"));
        write_text_file(temp.path(), &path, &note_source(i, note_count));
    }
    temp
}

/// Builds an in-memory [`FileIndex`] from a temporary project.
///
/// Use this for query benchmarks that do not need the project directory after
/// the index has been constructed.
///
/// # Panics
///
/// Panics if the temporary project cannot be created or indexed.
pub(crate) fn build_index(note_count: usize, shape: ProjectShape) -> FileIndex {
    let temp = create_project(note_count, shape);
    IndexerService::new(temp.path()).build().expect("build index")
}

/// Builds a shareable [`FileIndex`] from a temporary project.
///
/// The temporary project is deleted after the index is built; clone the
/// returned [`Arc`] inside Criterion iterations.
///
/// # Panics
///
/// Panics if the temporary project cannot be created or indexed.
pub(crate) fn build_index_arc(
    note_count: usize,
    shape: ProjectShape,
) -> Arc<FileIndex> {
    Arc::new(build_index(note_count, shape))
}

/// Builds an in-memory [`FileIndex`] from generated note content.
///
/// `note_source` receives `(note_index, note_count)`. The temporary project is
/// deleted after the index is built.
///
/// # Panics
///
/// Panics if the temporary project cannot be created or indexed.
fn build_index_from_note_source(
    note_count: usize,
    note_source: impl Fn(usize, usize) -> String,
) -> FileIndex {
    let temp = create_project_from_note_source(note_count, note_source);
    IndexerService::new(temp.path()).build().expect("build index")
}

/// Builds a shareable [`FileIndex`] from generated note content.
///
/// Clone the returned [`Arc`] inside Criterion iterations to exclude fixture
/// setup from the measurement.
///
/// # Panics
///
/// Panics if the temporary project cannot be created or indexed.
pub(crate) fn build_index_arc_from_note_source(
    note_count: usize,
    note_source: impl Fn(usize, usize) -> String,
) -> Arc<FileIndex> {
    Arc::new(build_index_from_note_source(note_count, note_source))
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
            &PathBuf::from(format!("assets/image-{i}.png")),
            b"fixture image bytes",
        );
        write_binary_file(
            root,
            &PathBuf::from(format!("docs/spec-{i}.pdf")),
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
/// # Panics
///
/// Panics if the temporary project cannot be created, indexed, or persisted.
pub(crate) fn setup_persisted_project(
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
pub(crate) fn rewrite_note(
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
    write_text_file(root, &path, &content);
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
    let path = fixture_path(root, &note_path(shape, note_index));
    fs::remove_file(path).expect("delete fixture note");
}
