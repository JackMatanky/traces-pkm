//! [`FileIndex`] and its constituent [`FileEntry`] rows.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::{delta::IndexDelta, inlinks::InlinkMap};
use crate::{FileBase, ListItem, Note};

/// Persisted cache of file records, parsed Note metadata, and derived inbound
/// links.
///
/// Every regular file under the project root contributes one [`FileEntry`]: its
/// [`FileBase`] metadata, and for Markdown files, its parsed [`Note`] plus
/// derived inbound links. [`IndexerService`] produces, persists, and loads it;
/// `FileIndex` itself carries no `&Path`.
///
/// Construction always flows through [`IndexerService`]'s [`build`], [`load`],
/// or [`refresh`] methods, never directly.
///
/// [`IndexerService`]: super::service::IndexerService
/// [`build`]: super::service::IndexerService::build
/// [`load`]: super::service::IndexerService::load
/// [`refresh`]: super::service::IndexerService::refresh
#[derive(Clone, Debug)]
pub struct FileIndex {
    entries: Box<[FileEntry]>,
    delta: IndexDelta,
}

impl FileIndex {
    /// Creates an index from its constituent parts.
    ///
    /// Used exclusively by [`IndexBuilder`] and [`IndexerService::load`] after
    /// scanning, parsing, and inlink derivation are complete.
    ///
    /// [`IndexerService::load`]: super::service::IndexerService::load
    /// [`IndexBuilder`]: super::builder::IndexBuilder
    pub(super) fn new(entries: Box<[FileEntry]>, delta: IndexDelta) -> Self {
        Self {
            entries,
            delta,
        }
    }

    /// Returns [`FileEntry`]s, sorted by path.
    #[inline]
    #[must_use]
    pub fn entries(&self) -> &[FileEntry] {
        &self.entries
    }

    /// Returns the [`FileEntry`] at `position`.
    #[expect(
        clippy::expect_used,
        reason = "RowIndex is always in bounds: values are only constructed \
                  from a valid range over entries"
    )]
    #[inline]
    pub(crate) fn entry_at(&self, position: RowIndex) -> &FileEntry {
        self.entries.get(position.get()).expect("RowIndex is always in bounds")
    }

    /// Returns the [`super::delta::IndexDelta`] that
    /// [`super::store::IndexStore::persist_index`] uses to choose between a
    /// full rewrite and a row-level incremental write.
    pub(super) fn delta(&self) -> &super::delta::IndexDelta {
        &self.delta
    }
}

/// A file's metadata, and (if it is a Note) its parsed content and inbound
/// links. A non-Note file structurally cannot carry inlinks (link resolution
/// only ever targets a Note's own path), so inlinks live inside the boxed
/// `NoteEntry`, not as a sibling field every entry carries regardless.
#[derive(Clone, Debug, PartialEq)]
pub struct FileEntry {
    file: FileBase,
    note: Option<Box<NoteEntry>>,
}

impl FileEntry {
    /// Creates a new [`FileEntry`].
    pub(super) fn new(file: FileBase, note: Option<Note>) -> Self {
        Self {
            file,
            note: note.map(|note| Box::new(NoteEntry::new(note))),
        }
    }

    /// Returns this entry's [`FileBase`] metadata.
    #[inline]
    #[must_use]
    pub fn file(&self) -> &FileBase {
        &self.file
    }

    /// Returns the parsed [`Note`], or `None` for a non-Markdown file.
    #[inline]
    #[must_use]
    pub fn note(&self) -> Option<&Note> {
        self.note.as_deref().map(|entry| &entry.note)
    }

    /// Returns inbound link paths for this entry, or an empty slice if
    /// absent.
    #[inline]
    #[must_use]
    pub(crate) fn inlinks(&self) -> &[PathBuf] {
        self.note.as_deref().map_or(&[], |entry| &entry.inlinks)
    }
}

/// A [`Note`] paired with its inbound links, boxed to keep non-Note `FileEntry`
/// small. Inlinks are index-level and cross-file, so they sit beside `Note`,
/// not inside it.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct NoteEntry {
    note: Note,
    inlinks: Box<[PathBuf]>,
}

impl NoteEntry {
    pub(super) fn new(note: Note) -> Self {
        Self {
            note,
            inlinks: Box::default(),
        }
    }

    pub(super) fn set_inlinks(&mut self, inlinks: Box<[PathBuf]>) {
        self.inlinks = inlinks;
    }
}

/// A persisted record of a single list item and its source note path.
///
/// Wraps a project-relative `path` and the parsed [`ListItem`], mirroring
/// how [`NoteEntry`] wraps [`Note`]. Exposes accessor methods that delegate
/// into the [`crate::ListItemType`] discriminant, keeping the persistence
/// shape composable: adding a field to [`crate::TaskListItem`] does not
/// require updating `ListEntry`'s struct layout.
///
/// `item`'s descendant lists are always empty
/// ([`ListItem::without_children`]): a `ListEntry` is one row per list item,
/// not per subtree, and each descendant is persisted as its own,
/// independent `ListEntry`, addressable by its own `(path, line)` key.
/// Nesting a copy of every descendant inside every ancestor's row would
/// duplicate that data once per ancestor, growing storage quadratically with
/// nesting depth for deeply nested lists — unlike [`NoteEntry`], which wraps
/// one [`Note`] once regardless of how deep its lists nest.
///
/// Stored in the `LISTS` table in redb, keyed by `(path, line)`.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct ListEntry {
    path: String,
    item: ListItem,
}

#[cfg_attr(
    not(any(test, feature = "test-utils")),
    expect(dead_code, reason = "consumed by task queries added in issue 08")
)]
impl ListEntry {
    /// Creates a `ListEntry` from a project-relative `path` and `item`,
    /// clearing `item`'s descendant lists first (see the type docs).
    #[inline]
    #[must_use]
    pub fn new(path: impl Into<String>, item: &ListItem) -> Self {
        Self {
            path: path.into(),
            item: item.without_children(),
        }
    }

    /// Returns the project-relative path of the note containing this list
    /// item.
    #[inline]
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Returns the task's status type, or [`None`] if this is not a Task
    /// item.
    #[inline]
    #[must_use]
    pub fn status_type(&self) -> Option<crate::TaskStatusType> {
        self.item.kind().as_task().map(|task| task.status().kind())
    }

    /// Returns the task's priority, or [`None`] if this is not a Task item or
    /// has no priority.
    #[inline]
    #[must_use]
    pub fn priority(&self) -> Option<crate::TaskPriority> {
        self.item.kind().as_task().and_then(crate::TaskListItem::priority)
    }

    /// Returns the task's due date, or [`None`] if this is not a Task item or
    /// has no due date.
    #[inline]
    #[must_use]
    pub fn due_date(&self) -> Option<chrono::NaiveDate> {
        self.item.kind().as_task().and_then(|task| task.dates().due)
    }

    /// Returns `true` if this task item and its entire task subtree are
    /// resolved, or [`None`] if this is not a Task item.
    #[inline]
    #[must_use]
    pub fn is_fully_complete(&self) -> Option<bool> {
        self.item.kind().as_task().map(crate::TaskListItem::is_fully_complete)
    }

    /// Returns the list item's text container.
    #[inline]
    #[must_use]
    pub fn text(&self) -> &crate::ListText {
        self.item.text()
    }

    /// Returns the raw text with only the leading marker prefix stripped.
    #[inline]
    #[must_use]
    pub fn raw_text(&self) -> &str {
        self.item.raw_text()
    }

    /// Returns the normalized clean text with task metadata stripped.
    #[inline]
    #[must_use]
    pub fn clean_text(&self) -> &str {
        self.item.clean_text()
    }

    /// Returns the list item's own tags, scanned from its text.
    #[inline]
    #[must_use]
    pub fn tags(&self) -> &[crate::Tag] {
        self.item.tags()
    }

    /// Returns the list item's 1-indexed source line.
    #[inline]
    #[must_use]
    pub const fn line(&self) -> crate::SourceLine {
        self.item.line()
    }

    /// Returns the list item's 0-indexed nesting depth.
    #[inline]
    #[must_use]
    pub const fn depth(&self) -> u8 {
        self.item.depth()
    }

    /// Returns the immediate parent list item's 1-indexed source line, if
    /// nested.
    #[inline]
    #[must_use]
    pub const fn parent_line(&self) -> Option<crate::SourceLine> {
        self.item.parent()
    }
}

/// Borrowed mirror of [`ListEntry`] used to serialize a `LISTS` row without
/// cloning the source [`Note`]'s path.
///
/// `item` still borrows the live tree node directly — callers MUST pass an
/// item whose descendant lists are already cleared (see
/// [`ListItem::without_children`]), typically a local variable holding that
/// derived value, never the original tree node. Field order and types match
/// [`ListEntry`] exactly, so postcard's positional encoding is byte-for-byte
/// identical between the two; [`super::store::IndexStore::read_lists`] and
/// [`super::store::IndexStore::read_lists_for_path`] deserialize the bytes
/// back as an owned [`ListEntry`].
#[derive(Serialize)]
pub(super) struct ListEntryRef<'a> {
    pub(super) path: &'a str,
    pub(super) item: &'a ListItem,
}

/// Position of a [`FileEntry`] within [`FileIndex::entries`].
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct RowIndex(usize);

impl RowIndex {
    /// Creates a [`RowIndex`] for the given position into
    /// [`FileIndex::entries`].
    #[inline]
    #[must_use]
    pub(crate) const fn new(position: usize) -> Self {
        Self(position)
    }

    /// Returns the row index as a `usize`.
    #[inline]
    #[must_use]
    const fn get(self) -> usize {
        self.0
    }
}

/// Merges sorted `files` with sorted `notes`, redistributes `inlinks` into
/// each entry, and returns boxed [`FileEntry`]s. Used by
/// [`super::IndexerService::load`].
pub(super) fn assemble_entries(
    files: Vec<FileBase>,
    notes: Vec<Note>,
    inlinks: InlinkMap,
) -> Box<[FileEntry]> {
    let mut notes_iter = notes.into_iter().peekable();
    let mut entries = Vec::with_capacity(files.len());
    for file in files {
        while notes_iter.peek().is_some_and(|note| note.path() < file.path()) {
            notes_iter.next();
        }
        let note = notes_iter.next_if(|note| note.path() == file.path());
        entries.push(FileEntry::new(file, note));
    }
    redistribute_inlinks(&mut entries, inlinks);
    entries.into_boxed_slice()
}

/// Distributes inlink sources from `inlinks` map into each matching
/// [`FileEntry`].
pub(super) fn redistribute_inlinks(
    entries: &mut [FileEntry],
    inlinks: InlinkMap,
) {
    for (target, sources) in inlinks {
        if let Ok(index) =
            entries.binary_search_by(|entry| entry.file().path().cmp(&target))
            && let Some(note_entry) =
                entries.get_mut(index).and_then(|entry| entry.note.as_mut())
        {
            note_entry.set_inlinks(sources.into_boxed_slice());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::service::IndexerService;

    mod position_lookup {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn entry_size_stays_under_target() {
            assert!(
                std::mem::size_of::<FileEntry>() <= 128,
                "FileEntry grew past its ~120-byte target — Note must stay \
                 boxed (its own shell is 240 bytes); check for an \
                 accidentally un-boxed field before raising this bound"
            );
        }

        #[test]
        fn entry_at_agrees_with_entries_index() {
            let temp = tempfile::tempdir().expect("create temp dir");
            std::fs::write(temp.path().join("a.md"), "# A").expect("write a");
            std::fs::write(temp.path().join("b.txt"), "plain text")
                .expect("write b.txt");
            std::fs::write(temp.path().join("c.md"), "# C").expect("write c");
            let index =
                IndexerService::new(temp.path()).build().expect("build index");

            for (i, entry) in index.entries().iter().enumerate() {
                let position = RowIndex::new(i);
                assert_eq!(index.entry_at(position), entry);
            }
        }

        #[test]
        fn note_returns_none_for_a_non_markdown_file() {
            let temp = tempfile::tempdir().expect("create temp dir");
            std::fs::write(temp.path().join("plain.txt"), "no frontmatter")
                .expect("write plain.txt");
            let index =
                IndexerService::new(temp.path()).build().expect("build index");

            assert_eq!(index.entries().len(), 1);
            assert_eq!(index.entry_at(RowIndex::new(0)).note(), None);
        }
    }

    mod list_entry {
        use chrono::NaiveDate;
        use pretty_assertions::assert_eq;

        use super::*;
        use crate::{
            List, ListItemType, TaskDates, TaskListItem, TaskPriority,
            TaskStatus, TaskStatusSymbol, TaskStatusType,
        };

        #[test]
        fn stores_path_and_delegates_text_to_item() {
            let item = ListItem::new("plain item", ListItemType::Plain);
            let entry = ListEntry::new("notes/todo.md", &item);

            assert_eq!(entry.path(), "notes/todo.md");
            assert_eq!(entry.text(), item.text());
        }

        #[test]
        fn accessors_delegate_for_task_item() {
            let status = TaskStatus::new(
                TaskStatusSymbol::new(' '),
                "Todo",
                TaskStatusType::Todo,
            );
            let dates = TaskDates::new(
                None,
                None,
                None,
                NaiveDate::from_ymd_opt(2025, 1, 15),
                None,
                None,
            );
            let task_item = TaskListItem::new(
                dates,
                Some(TaskPriority::High),
                status,
                false,
            );
            let item = ListItem::new("my task", ListItemType::Task(task_item));
            let entry = ListEntry::new("notes/task.md", &item);

            assert_eq!(entry.status_type(), Some(TaskStatusType::Todo));
            assert_eq!(entry.priority(), Some(TaskPriority::High));
            assert_eq!(entry.due_date(), NaiveDate::from_ymd_opt(2025, 1, 15));
            assert_eq!(entry.is_fully_complete(), Some(false));
            assert_eq!(entry.clean_text(), "my task");
        }

        #[test]
        fn task_accessors_return_none_for_plain_and_checkbox_items() {
            let plain_item = ListItem::new("bullet", ListItemType::Plain);
            let entry = ListEntry::new("notes/plain.md", &plain_item);

            assert_eq!(entry.status_type(), None);
            assert_eq!(entry.priority(), None);
            assert_eq!(entry.due_date(), None);
            assert_eq!(entry.is_fully_complete(), None);

            let checkbox_item = ListItem::new("check", ListItemType::Checkbox);
            let checkbox_entry =
                ListEntry::new("notes/check.md", &checkbox_item);
            assert_eq!(checkbox_entry.status_type(), None);
            assert_eq!(checkbox_entry.priority(), None);
            assert_eq!(checkbox_entry.due_date(), None);
            assert_eq!(checkbox_entry.is_fully_complete(), None);
        }

        #[test]
        fn clears_descendant_lists_even_when_the_source_item_has_children() {
            let child = ListItem::new("child", ListItemType::Plain);
            let parent =
                ListItem::with_children("parent", ListItemType::Plain, vec![
                    List::new(false, vec![child]),
                ]);
            assert!(!parent.children().is_empty(), "test setup sanity check");

            let entry = ListEntry::new("notes/nested.md", &parent);

            assert!(entry.item.children().is_empty());
        }

        #[test]
        fn postcard_roundtrip() {
            let status = TaskStatus::new(
                TaskStatusSymbol::new('x'),
                "Done",
                TaskStatusType::Done,
            );
            let dates = TaskDates::new(
                None,
                None,
                None,
                NaiveDate::from_ymd_opt(2025, 1, 15),
                Some(NaiveDate::from_ymd_opt(2025, 1, 14).unwrap()),
                None,
            );
            let task_item = TaskListItem::new(
                dates,
                Some(TaskPriority::Medium),
                status,
                true,
            );
            let item =
                ListItem::new("postcard task", ListItemType::Task(task_item));
            let entry = ListEntry::new("path/to/note.md", &item);

            let bytes =
                postcard::to_allocvec(&entry).expect("serialize list entry");
            let decoded: ListEntry =
                postcard::from_bytes(&bytes).expect("deserialize list entry");

            assert_eq!(decoded, entry);
        }
    }
}
