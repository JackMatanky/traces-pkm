//! Markdown Note Metadata domain types.

use std::ops::Range;

use serde::{Deserialize, Serialize};

/// Frontmatter metadata block extracted from a markdown Note.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Frontmatter {
    raw: String,
}

impl Frontmatter {
    /// Creates a new [`Frontmatter`] instance.
    #[inline]
    #[must_use]
    pub(crate) fn new(raw: impl Into<String>) -> Self {
        Self {
            raw: raw.into(),
        }
    }

    /// Raw YAML content of the frontmatter block.
    #[inline]
    #[must_use]
    pub(crate) fn raw(&self) -> &str {
        &self.raw
    }

    /// Returns `true` if the frontmatter block is empty.
    #[inline]
    #[must_use]
    pub(crate) fn is_empty(&self) -> bool {
        self.raw.is_empty()
    }
}

/// Link target classification: standard Markdown link or Obsidian Wikilink.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum LinkType {
    Markdown,
    Wikilink,
}

/// An outgoing link extracted from a markdown Note.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Outlink {
    target: String,
    text: String,
    kind: LinkType,
}

impl Outlink {
    /// Creates a new [`Outlink`].
    #[inline]
    #[must_use]
    pub(crate) fn new(
        target: impl Into<String>,
        text: impl Into<String>,
        kind: LinkType,
    ) -> Self {
        Self {
            target: target.into(),
            text: text.into(),
            kind,
        }
    }

    /// Target URL, relative file path, or wikilink page target.
    #[inline]
    #[must_use]
    pub(crate) fn target(&self) -> &str {
        &self.target
    }

    /// Display text or alias for the link.
    #[inline]
    #[must_use]
    pub(crate) fn text(&self) -> &str {
        &self.text
    }

    /// Link syntax classification ([`LinkType::Markdown`] or
    /// [`LinkType::Wikilink`]).
    #[inline]
    #[must_use]
    pub(crate) fn kind(&self) -> LinkType {
        self.kind
    }

    /// Returns `true` if this link is a Wikilink.
    #[inline]
    #[must_use]
    pub(crate) fn is_wikilink(&self) -> bool {
        matches!(self.kind, LinkType::Wikilink)
    }

    /// Returns `true` if this link is a standard Markdown link.
    #[inline]
    #[must_use]
    pub(crate) fn is_markdown(&self) -> bool {
        matches!(self.kind, LinkType::Markdown)
    }
}

/// Completion state for a task list item.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum TaskStatus {
    Incomplete,
    Complete,
}

/// A single item within a markdown list.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ListItem {
    text: String,
    task_status: Option<TaskStatus>,
    children: Vec<List>,
}

impl ListItem {
    /// Creates a new [`ListItem`].
    #[inline]
    #[must_use]
    pub(crate) fn new(
        text: impl Into<String>,
        task_status: Option<TaskStatus>,
    ) -> Self {
        Self {
            text: text.into(),
            task_status,
            children: Vec::new(),
        }
    }

    /// Creates a new [`ListItem`] with child lists.
    #[inline]
    #[must_use]
    pub(crate) fn with_children(
        text: impl Into<String>,
        task_status: Option<TaskStatus>,
        children: Vec<List>,
    ) -> Self {
        Self {
            text: text.into(),
            task_status,
            children,
        }
    }

    #[inline]
    #[must_use]
    pub(crate) fn text(&self) -> &str {
        &self.text
    }

    /// Task completion state, if this list item is a task.
    #[inline]
    #[must_use]
    pub(crate) fn task_status(&self) -> Option<TaskStatus> {
        self.task_status
    }

    /// Returns `true` if this item is a task item (`- [ ]` or `- [x]`).
    #[inline]
    #[must_use]
    pub(crate) fn is_task(&self) -> bool {
        self.task_status.is_some()
    }

    /// Returns `true` if this task item is completed (`- [x]`).
    #[inline]
    #[must_use]
    pub(crate) fn is_completed(&self) -> bool {
        matches!(self.task_status, Some(TaskStatus::Complete))
    }

    /// Nested child lists under this item.
    #[inline]
    #[must_use]
    pub(crate) fn children(&self) -> &[List] {
        &self.children
    }
}

/// A list extracted from a markdown Note.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct List {
    is_ordered: bool,
    items: Vec<ListItem>,
}

impl List {
    /// Creates a new [`List`].
    #[inline]
    #[must_use]
    pub(crate) fn new(is_ordered: bool, items: Vec<ListItem>) -> Self {
        Self {
            is_ordered,
            items,
        }
    }

    /// Returns `true` if this is an ordered list.
    #[inline]
    #[must_use]
    pub(crate) fn is_ordered(&self) -> bool {
        self.is_ordered
    }

    /// Top-level list items contained in this list.
    #[inline]
    #[must_use]
    pub(crate) fn items(&self) -> &[ListItem] {
        &self.items
    }
}

/// An excludable code region (inline code or code block byte range) in source
/// markdown.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CodeRegion {
    start: usize,
    end: usize,
}

impl CodeRegion {
    /// Creates a new [`CodeRegion`].
    #[inline]
    #[must_use]
    pub(crate) fn new(start: usize, end: usize) -> Self {
        Self {
            start,
            end,
        }
    }

    /// Byte range in the original markdown source.
    #[inline]
    #[must_use]
    pub(crate) fn range(&self) -> Range<usize> {
        self.start..self.end
    }
}

/// Rich Note Metadata extracted from a markdown file.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Note {
    frontmatter: Option<Frontmatter>,
    lists: Vec<List>,
    outlinks: Vec<Outlink>,
    code_regions: Vec<CodeRegion>,
}

impl Note {
    /// Creates a new [`Note`].
    #[inline]
    #[must_use]
    pub(crate) fn new(
        frontmatter: Option<Frontmatter>,
        lists: Vec<List>,
        outlinks: Vec<Outlink>,
        code_regions: Vec<CodeRegion>,
    ) -> Self {
        Self {
            frontmatter,
            lists,
            outlinks,
            code_regions,
        }
    }

    /// Extracted YAML frontmatter block, if present.
    #[inline]
    #[must_use]
    pub(crate) fn frontmatter(&self) -> Option<&Frontmatter> {
        self.frontmatter.as_ref()
    }

    /// Extracted lists.
    #[inline]
    #[must_use]
    pub(crate) fn lists(&self) -> &[List] {
        &self.lists
    }

    /// Extracted outlinks.
    #[inline]
    #[must_use]
    pub(crate) fn outlinks(&self) -> &[Outlink] {
        &self.outlinks
    }

    /// Extracted excludable code regions.
    #[inline]
    #[must_use]
    pub(crate) fn code_regions(&self) -> &[CodeRegion] {
        &self.code_regions
    }

    /// Iterator over all task list items in this Note, including items in
    /// nested sub-lists.
    pub(crate) fn tasks(&self) -> impl Iterator<Item = &ListItem> {
        let mut tasks = Vec::new();
        for list in &self.lists {
            collect_tasks_recursive(list, &mut tasks);
        }
        tasks.into_iter()
    }
}

fn collect_tasks_recursive<'a>(list: &'a List, acc: &mut Vec<&'a ListItem>) {
    for item in &list.items {
        if item.is_task() {
            acc.push(item);
        }
        for child_list in &item.children {
            collect_tasks_recursive(child_list, acc);
        }
    }
}

/// Pair of a project-relative path and its extracted [`Note`] metadata.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct NoteRecord {
    path: std::path::PathBuf,
    note: Note,
}

impl NoteRecord {
    /// Creates a new [`NoteRecord`].
    #[inline]
    #[must_use]
    pub(crate) fn new(path: impl Into<std::path::PathBuf>, note: Note) -> Self {
        Self {
            path: path.into(),
            note,
        }
    }

    /// Project-relative path of the note.
    #[inline]
    #[must_use]
    pub(crate) fn path(&self) -> &std::path::Path {
        &self.path
    }

    /// Extracted Note Metadata.
    #[inline]
    #[must_use]
    pub(crate) fn note(&self) -> &Note {
        &self.note
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    mod constructor {
        use pretty_assertions::assert_eq;

        use super::*;
        #[test]
        fn creates_frontmatter_with_raw_content() {
            let fm = Frontmatter::new("key: value\n");
            assert_eq!(fm.raw(), "key: value\n");
            assert_eq!(fm.is_empty(), false);
        }

        #[test]
        fn creates_empty_frontmatter() {
            let fm = Frontmatter::default();
            assert_eq!(fm.is_empty(), true);
        }

        #[test]
        fn creates_outlink_with_kind() {
            let link = Outlink::new("target", "alias", LinkType::Wikilink);
            assert_eq!(link.target(), "target");
            assert_eq!(link.text(), "alias");
            assert_eq!(link.kind(), LinkType::Wikilink);
            assert_eq!(link.is_wikilink(), true);
            assert_eq!(link.is_markdown(), false);
        }

        #[test]
        fn creates_list_item_and_task_status() {
            let item = ListItem::new("Task 1", Some(TaskStatus::Incomplete));
            assert_eq!(item.text(), "Task 1");
            assert_eq!(item.task_status(), Some(TaskStatus::Incomplete));
            assert_eq!(item.is_task(), true);
            assert_eq!(item.is_completed(), false);
            assert_eq!(item.children().len(), 0);
        }

        #[test]
        fn creates_list_item_with_children() {
            let child_list =
                List::new(false, vec![ListItem::new("Child", None)]);
            let item =
                ListItem::with_children("Parent", None, vec![child_list]);
            assert_eq!(item.children().len(), 1);
            assert_eq!(
                item.children().get(0).map(List::is_ordered),
                Some(false)
            );
        }

        #[test]
        fn creates_code_region_range() {
            let region = CodeRegion::new(10, 25);
            assert_eq!(region.range(), 10..25);
        }

        #[test]
        fn creates_note_record() {
            let note = Note::new(None, Vec::new(), Vec::new(), Vec::new());
            let rec = NoteRecord::new("notes/a.md", note.clone());
            assert_eq!(rec.path(), std::path::Path::new("notes/a.md"));
            assert_eq!(rec.note(), &note);
        }
    }
}
