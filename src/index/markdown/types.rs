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

/// Outlink target classification: standard Markdown link or Obsidian Wikilink.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum OutlinkType {
    Markdown,
    Wikilink,
}

/// An outgoing link extracted from a markdown Note.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Outlink {
    target: String,
    text: String,
    outlink_type: OutlinkType,
}

impl Outlink {
    /// Creates a new [`Outlink`].
    #[inline]
    #[must_use]
    pub(crate) fn new(
        target: impl Into<String>,
        text: impl Into<String>,
        outlink_type: OutlinkType,
    ) -> Self {
        Self {
            target: target.into(),
            text: text.into(),
            outlink_type,
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

    /// Link syntax classification ([`OutlinkType::Markdown`] or
    /// [`OutlinkType::Wikilink`]).
    #[inline]
    #[must_use]
    pub(crate) fn outlink_type(&self) -> OutlinkType {
        self.outlink_type
    }

    /// Returns `true` if this link is a Wikilink.
    #[inline]
    #[must_use]
    pub(crate) fn is_wikilink(&self) -> bool {
        matches!(self.outlink_type, OutlinkType::Wikilink)
    }

    /// Returns `true` if this link is a standard Markdown link.
    #[inline]
    #[must_use]
    pub(crate) fn is_markdown(&self) -> bool {
        matches!(self.outlink_type, OutlinkType::Markdown)
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
