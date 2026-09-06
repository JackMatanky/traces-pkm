//! Parsed-note fixtures for in-memory benchmarks.
//!
//! Use this module when the benchmark needs [`Note`] values or matching
//! [`FileBase`] records but not a real directory tree. Every generator returns
//! notes sorted by path so [`InlinkMap`](traces_pkm::InlinkMap) callers get
//! deterministic input order.
//!
//! Do not add filesystem setup here. If a benchmark needs actual files,
//! attachments, or `index.redb`, use [`super::project`] instead. Keep topology
//! names short but explicit: `sparse_link`, `dense_link`, `hub_link`,
//! `duplicate_link`, `deep_path_link`, `ambiguous_target`, and
//! `attachment_link`.

use std::path::{Path, PathBuf};

use traces_pkm::{FileBase, MarkdownParserInput, Note, parse_markdown};

use super::content::{
    attachment_note_source, duplicate_link_note_source, linked_note_source,
};

/// Parses a synthetic note at `path` from `content`.
pub(crate) fn parse_note(path: &Path, content: &str) -> Note {
    let input = MarkdownParserInput::for_test(path, content);
    parse_markdown(&input)
}

/// Returns sorted [`FileBase`] note records matching `notes`.
pub(crate) fn file_records_for_notes(notes: &[Note]) -> Vec<FileBase> {
    let mut files: Vec<FileBase> = notes
        .iter()
        .map(|note| {
            FileBase::new_note_test(
                note.path().to_path_buf(),
                note.path()
                    .parent()
                    .map_or_else(PathBuf::new, Path::to_path_buf),
            )
        })
        .collect();
    files.sort_by(|a, b| a.path().cmp(b.path()));
    files
}

/// Generates parsed notes in a one-link ring topology.
pub(crate) fn generate_sparse_link_notes(note_count: usize) -> Vec<Note> {
    generate_link_notes(note_count, 1)
}

/// Generates parsed notes where each note links to twenty later targets.
pub(crate) fn generate_dense_link_notes(note_count: usize) -> Vec<Note> {
    generate_link_notes(note_count, 20)
}

/// Generates parsed notes with `links_per_note` links per note.
fn generate_link_notes(note_count: usize, links_per_note: usize) -> Vec<Note> {
    let mut notes = Vec::with_capacity(note_count);
    for i in 0..note_count {
        let path = PathBuf::from(format!("note-{i}.md"));
        let content = linked_note_source(i, note_count, links_per_note);
        notes.push(parse_note(&path, &content));
    }
    notes.sort_by(|a, b| a.path().cmp(b.path()));
    notes
}

/// Generates parsed notes where every source links to one hub note.
pub(crate) fn generate_hub_link_notes(note_count: usize) -> Vec<Note> {
    let mut notes = Vec::with_capacity(note_count);
    notes.push(parse_note(Path::new("hub.md"), "# Hub\n"));
    for i in 1..note_count {
        let path = PathBuf::from(format!("note-{i}.md"));
        let content = format!("# Source {i}\n\nLink to [[hub]]\n");
        notes.push(parse_note(&path, &content));
    }
    notes.sort_by(|a, b| a.path().cmp(b.path()));
    notes
}

/// Generates parsed notes with repeated links to the same target.
pub(crate) fn generate_duplicate_link_notes(
    note_count: usize,
    repetitions: usize,
) -> Vec<Note> {
    let mut notes = Vec::with_capacity(note_count);
    for i in 0..note_count {
        let path = PathBuf::from(format!("note-{i}.md"));
        let content = duplicate_link_note_source(i, note_count, repetitions);
        notes.push(parse_note(&path, &content));
    }
    notes.sort_by(|a, b| a.path().cmp(b.path()));
    notes
}

/// Generates parsed notes in deep folders with same-stem local targets.
pub(crate) fn generate_deep_path_link_notes(note_count: usize) -> Vec<Note> {
    let mut notes = Vec::with_capacity(note_count);
    for i in 0..note_count {
        let cluster = i / 10;
        let folder = format!(
            "areas/area-{cluster}/projects/project-{cluster}/phase/current"
        );
        let path = if i % 10 == 0 {
            PathBuf::from(format!("{folder}/target.md"))
        } else {
            PathBuf::from(format!("{folder}/note-{i}.md"))
        };
        let content = if i % 10 == 0 {
            String::from("# Target\n")
        } else {
            format!("# Deep Note {i}\n\nLink to [[target]]\n")
        };
        notes.push(parse_note(&path, &content));
    }
    notes.sort_by(|a, b| a.path().cmp(b.path()));
    notes
}

/// Generates parsed notes with `candidate_count` same-stem target candidates.
pub(crate) fn generate_ambiguous_target_link_notes(
    note_count: usize,
    candidate_count: usize,
) -> Vec<Note> {
    let candidate_count = candidate_count.clamp(1, note_count.max(1));
    let mut notes = Vec::with_capacity(note_count);
    for candidate in 0..candidate_count {
        let path = PathBuf::from(format!("folder-{candidate}/target.md"));
        notes.push(parse_note(&path, "# Target\n"));
    }
    for i in candidate_count..note_count {
        let folder = i % candidate_count;
        let path = PathBuf::from(format!("folder-{folder}/source-{i}.md"));
        let content = format!("# Source {i}\n\nLink to [[target]]\n");
        notes.push(parse_note(&path, &content));
    }
    notes.sort_by(|a, b| a.path().cmp(b.path()));
    notes
}

/// Generates parsed notes that link to attachment-looking paths plus matching
/// [`FileBase`] records.
///
/// Use this for in-memory [`InlinkMap`](traces_pkm::InlinkMap) benchmarks. It
/// does not create files on disk; use [`super::project::create_project`] with
/// [`ProjectShape::AttachmentProject`](super::content::ProjectShape::AttachmentProject)
/// when a benchmark needs real temporary attachment files.
pub(crate) fn generate_attachment_link_notes(
    note_count: usize,
) -> (Vec<Note>, Vec<FileBase>) {
    let mut notes = Vec::with_capacity(note_count);
    let mut files = Vec::with_capacity(note_count.saturating_add(40));
    for i in 0..note_count {
        let path = PathBuf::from(format!("note-{i}.md"));
        let content = attachment_note_source(i);
        notes.push(parse_note(&path, &content));
        files.push(FileBase::new_note_test(path, PathBuf::new()));
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
