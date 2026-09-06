//! Raw Markdown fixture content for benchmark setup.
//!
//! Keep this module pure: functions return project-relative paths or Markdown
//! strings only. Parsing belongs in [`super::notes`]; temporary files and index
//! construction belong in [`super::project`].
//!
//! Add a new [`ProjectShape`] only when multiple benchmark targets need the
//! same vault topology. For a one-off parser or query workload, keep the source
//! generator local to that benchmark so this shared module does not become a
//! dumping ground.
//!
//! Naming convention: use `{shape}_note_source` for full Markdown note content,
//! `*_field_*` for metadata-width fixtures, and `*_link_*` for link topology.
//! Count parameters are named after what they count (`note_count`,
//! `field_count`, `links_per_note`) rather than a generic `n`.

use std::{fmt::Write as _, path::PathBuf};

/// Reusable project topologies for filesystem and index benchmarks.
///
/// Variants describe vault shape, not the benchmark that currently uses them.
/// Add a variant only when the shape is broad enough to reuse.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum ProjectShape {
    /// Minimal frontmatter and body content.
    Plain,
    /// Common, nested, and rare tags in note body text.
    Tagged,
    /// File-class style frontmatter under the default `class` key.
    Classified,
    /// One link per note, forming a ring.
    LinkedSparse,
    /// Twenty links per note, stressing link extraction and backlink grouping.
    LinkedDense,
    /// Twenty task/list items per note.
    ListHeavy,
    /// Tags, classes, links, list items, and nested folders.
    RichRealistic,
    /// Notes that link to real non-Markdown files on disk.
    AttachmentProject,
    /// Rich notes intended for deletion and cleanup refresh benches.
    DeleteHeavy,
}

impl ProjectShape {
    /// Returns the stable benchmark label for this fixture shape.
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::Plain => "plain",
            Self::Tagged => "tagged",
            Self::Classified => "classified",
            Self::LinkedSparse => "linked_sparse",
            Self::LinkedDense => "linked_dense",
            Self::ListHeavy => "list_heavy",
            Self::RichRealistic => "rich_realistic",
            Self::AttachmentProject => "attachments",
            Self::DeleteHeavy => "delete_heavy",
        }
    }
}

/// Returns the project-relative note path for `shape` and `note_index`.
///
/// Most shapes stay at the project root. Rich shapes use nested folders so
/// scanner, path sorting, and proximity-sensitive link resolution see realistic
/// path depth.
pub(crate) fn note_path(shape: ProjectShape, note_index: usize) -> PathBuf {
    match shape {
        ProjectShape::RichRealistic | ProjectShape::DeleteHeavy => {
            PathBuf::from(format!(
                "areas/area-{}/project-{}/note-{note_index}.md",
                note_index % 16,
                note_index % 128
            ))
        }
        _ => PathBuf::from(format!("note-{note_index}.md")),
    }
}

/// Returns minimal note content with one sortable metadata field.
pub(crate) fn plain_note_source(note_index: usize) -> String {
    format!(
        "---\nrating: {}\n---\n\n# Note {note_index}\nBody text for note \
         {note_index}.\n",
        note_index % 10
    )
}

/// Returns note content with common, nested, and rare tags.
pub(crate) fn tagged_note_source(note_index: usize) -> String {
    format!(
        "---\nrating: {}\n---\n\n# Note {note_index}\nCommon #common and \
         nested #topic/{} plus rare #rare_{}.\n",
        note_index % 10,
        note_index % 25,
        note_index,
    )
}

/// Returns note content with a File Class value under the default `class` key.
pub(crate) fn classified_note_source(note_index: usize) -> String {
    let class = match note_index % 4 {
        0 => "Project",
        1 => "Area",
        2 => "Resource",
        _ => "Archive",
    };
    format!(
        "---\nrating: {}\nclass: {class}\n---\n\n# Classified Note \
         {note_index}\n",
        note_index % 10,
    )
}

/// Returns note content with `field_count` synthetic metadata fields.
///
/// Use this when YAML/frontmatter parsing or allocation is the measured unit.
pub(crate) fn frontmatter_fields_source(field_count: usize) -> String {
    let mut source = String::from("---\n");
    for field_index in 0..field_count {
        let _ =
            writeln!(source, "field_{field_index}: \"value_{field_index}\"");
    }
    source.push_str("---\n\n# Body\nSimple note.\n");
    source
}

/// Returns note content with many metadata fields before a sortable `rating`.
///
/// Use this for query-engine field lookup benchmarks: `rating` is deliberately
/// last, making an accidental linear metadata scan obvious.
pub(crate) fn metadata_lookup_note_source(
    note_index: usize,
    field_count: usize,
) -> String {
    let mut source = String::from("---\n");
    for field_index in 0..field_count {
        let _ = writeln!(source, "field_{field_index}: \"value\"");
    }
    let _ = writeln!(source, "rating: {}", note_index % 100);
    source.push_str("---\n");
    source
}

/// Returns note content with `links_per_note` wikilinks into the same
/// workspace.
///
/// `note_count` must be positive because links wrap modulo the fixture size.
pub(crate) fn linked_note_source(
    note_index: usize,
    note_count: usize,
    links_per_note: usize,
) -> String {
    let mut content = format!(
        "---\nrating: {}\n---\n\n# Linked Note {note_index}\n",
        note_index % 10
    );
    for link_offset in 1..=links_per_note {
        let target = (note_index + link_offset) % note_count;
        let _ = writeln!(content, "- Link to [[note-{target}]]");
    }
    content
}

/// Returns note content with repeated links to the same target.
///
/// `note_count` must be positive because the target wraps modulo the fixture
/// size.
pub(crate) fn duplicate_link_note_source(
    note_index: usize,
    note_count: usize,
    repetitions: usize,
) -> String {
    let target = (note_index + 1) % note_count;
    let mut content = format!("# Duplicate Link Note {note_index}\n\n");
    for _ in 0..repetitions {
        let _ = writeln!(content, "Repeated [[note-{target}]]");
    }
    content
}

/// Returns note content with twenty task/list rows.
pub(crate) fn list_heavy_note_source(note_index: usize) -> String {
    let mut content = format!(
        "---\nrating: {}\n---\n\n# Task Note {note_index}\n",
        note_index % 10
    );
    for task_index in 0..20 {
        let status = if task_index % 3 == 0 {
            "x"
        } else {
            " "
        };
        let _ = writeln!(
            content,
            "- [{status}] Task {task_index} for note {note_index} \
             @due(2026-09-{:02}) #task/topic_{}",
            (task_index % 28) + 1,
            task_index % 5,
        );
    }
    content
}

/// Returns a note with exactly three task rows.
pub(crate) fn task_triplet_note_source(note_index: usize) -> String {
    format!(
        "# Task Note {note_index}\n\n- [ ] first\n- [x] second\n- [ ] third\n"
    )
}

/// Returns a task-list note with `item_count` top-level checkbox items.
pub(crate) fn list_items_source(item_count: usize) -> String {
    let mut source = String::from("# List Items\n\n");
    for item_index in 0..item_count {
        let _ = writeln!(source, "- [ ] Item {item_index} for processing");
    }
    source
}

/// Returns realistic mixed-content note text.
///
/// `note_count` must be positive because related links wrap modulo the fixture
/// size.
pub(crate) fn rich_note_source(note_index: usize, note_count: usize) -> String {
    let mut content = format!(
        "---\nrating: {}\nclass: {}\nstatus: {}\n---\n\n# Rich Note \
         {note_index}\n#common #topic/{} #rare_{}\n",
        note_index % 10,
        if note_index.is_multiple_of(2) {
            "Project"
        } else {
            "Area"
        },
        if note_index.is_multiple_of(3) {
            "active"
        } else {
            "archived"
        },
        note_index % 25,
        note_index,
    );
    for link_offset in 1..=5 {
        let target = (note_index + link_offset) % note_count;
        let _ = writeln!(content, "- Related [[note-{target}]]");
    }
    for task_index in 0..10 {
        let _ = writeln!(
            content,
            "- [ ] Task {task_index} for note {note_index} #task/topic_{}",
            task_index % 5
        );
    }
    content
}

/// Returns rich note text with extra secondary-index pressure for deletion
/// benches.
pub(crate) fn delete_heavy_note_source(
    note_index: usize,
    note_count: usize,
) -> String {
    let mut content = rich_note_source(note_index, note_count);
    for tag_index in 0..20 {
        let _ = write!(content, " #delete/topic_{tag_index}");
    }
    content.push('\n');
    content
}

/// Returns attachment-linking note content.
pub(crate) fn attachment_note_source(note_index: usize) -> String {
    let attachment_index = note_index % 20;
    format!(
        "---\nrating: {}\nclass: AttachmentNote\n---\n\n# Attachment Note \
         {note_index}\nEmbedded image: \
         ![[assets/image-{attachment_index}.png]]\nAttachment doc: \
         [[docs/spec-{attachment_index}.pdf]]\n",
        note_index % 10,
    )
}

/// Dispatches to the note-source generator for `shape` and `note_index`.
pub(crate) fn note_source(
    shape: ProjectShape,
    note_index: usize,
    note_count: usize,
) -> String {
    match shape {
        ProjectShape::Plain => plain_note_source(note_index),
        ProjectShape::Tagged => tagged_note_source(note_index),
        ProjectShape::Classified => classified_note_source(note_index),
        ProjectShape::LinkedSparse => {
            linked_note_source(note_index, note_count, 1)
        }
        ProjectShape::LinkedDense => {
            linked_note_source(note_index, note_count, 20)
        }
        ProjectShape::ListHeavy => list_heavy_note_source(note_index),
        ProjectShape::RichRealistic => rich_note_source(note_index, note_count),
        ProjectShape::AttachmentProject => attachment_note_source(note_index),
        ProjectShape::DeleteHeavy => {
            delete_heavy_note_source(note_index, note_count)
        }
    }
}
