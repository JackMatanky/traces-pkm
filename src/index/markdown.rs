//! Markdown note parser yielding [`Note`] records.

use std::ops::Range;

use pulldown_cmark::{Event, LinkType, Options, Parser, Tag, TagEnd};
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
pub(crate) enum OutlinkKind {
    Markdown,
    Wikilink,
}

/// An outgoing link extracted from a markdown Note.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Outlink {
    target: String,
    text: String,
    kind: OutlinkKind,
}

impl Outlink {
    /// Creates a new [`Outlink`].
    #[inline]
    #[must_use]
    pub(crate) fn new(
        target: impl Into<String>,
        text: impl Into<String>,
        kind: OutlinkKind,
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

    /// Link syntax classification ([`OutlinkKind::Markdown`] or
    /// [`OutlinkKind::Wikilink`]).
    #[inline]
    #[must_use]
    pub(crate) fn kind(&self) -> OutlinkKind {
        self.kind
    }

    /// Returns `true` if this link is a Wikilink.
    #[inline]
    #[must_use]
    pub(crate) fn is_wikilink(&self) -> bool {
        matches!(self.kind, OutlinkKind::Wikilink)
    }

    /// Returns `true` if this link is a standard Markdown link.
    #[inline]
    #[must_use]
    pub(crate) fn is_markdown(&self) -> bool {
        matches!(self.kind, OutlinkKind::Markdown)
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

    /// Plain text content of the list item.
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

struct ListFrame {
    is_ordered: bool,
    items: Vec<ListItem>,
}

struct ItemFrame {
    task_status: Option<TaskStatus>,
    text_buffer: String,
    children: Vec<List>,
}

/// Parses a markdown string into a [`Note`] record using `pulldown-cmark`.
#[must_use]
pub(crate) fn parse_markdown(src: &str) -> Note {
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_TASKLISTS);
    opts.insert(Options::ENABLE_YAML_STYLE_METADATA_BLOCKS);
    opts.insert(Options::ENABLE_WIKILINKS);

    let parser = Parser::new_ext(src, opts).into_offset_iter();

    let mut frontmatter: Option<Frontmatter> = None;
    let mut in_metadata_block = false;
    let mut metadata_buffer = String::new();

    let mut outlinks: Vec<Outlink> = Vec::new();
    let mut active_link: Option<(String, OutlinkKind, String)> = None;

    let mut code_regions: Vec<CodeRegion> = Vec::new();
    let mut active_code_block_start: Option<usize> = None;

    let mut lists: Vec<List> = Vec::new();
    let mut list_stack: Vec<ListFrame> = Vec::new();
    let mut item_stack: Vec<ItemFrame> = Vec::new();

    for (event, range) in parser {
        match event {
            Event::Start(Tag::MetadataBlock(_)) => {
                in_metadata_block = true;
                metadata_buffer.clear();
            }
            Event::End(TagEnd::MetadataBlock(_)) => {
                in_metadata_block = false;
                frontmatter = Some(Frontmatter::new(std::mem::take(
                    &mut metadata_buffer,
                )));
            }
            Event::Start(Tag::Link {
                link_type,
                dest_url,
                ..
            }) => {
                let kind = if matches!(link_type, LinkType::WikiLink { .. }) {
                    OutlinkKind::Wikilink
                } else {
                    OutlinkKind::Markdown
                };
                active_link =
                    Some((dest_url.into_string(), kind, String::new()));
            }
            Event::End(TagEnd::Link) => {
                if let Some((target, kind, text)) = active_link.take() {
                    outlinks.push(Outlink::new(target, text.trim(), kind));
                }
            }
            Event::Start(Tag::CodeBlock(_)) => {
                active_code_block_start = Some(range.start);
            }
            Event::End(TagEnd::CodeBlock) => {
                if let Some(start) = active_code_block_start.take() {
                    code_regions.push(CodeRegion::new(start, range.end));
                }
            }
            Event::Code(text) => {
                code_regions.push(CodeRegion::new(range.start, range.end));
                if let Some(item) = item_stack.last_mut() {
                    item.text_buffer.push_str(&text);
                }
            }
            Event::Start(Tag::List(first_item_number)) => {
                list_stack.push(ListFrame {
                    is_ordered: first_item_number.is_some(),
                    items: Vec::new(),
                });
            }
            Event::End(TagEnd::List(_)) => {
                if let Some(list_frame) = list_stack.pop() {
                    let list =
                        List::new(list_frame.is_ordered, list_frame.items);
                    if let Some(parent_item) = item_stack.last_mut() {
                        parent_item.children.push(list);
                    } else {
                        lists.push(list);
                    }
                }
            }
            Event::Start(Tag::Item) => {
                item_stack.push(ItemFrame {
                    task_status: None,
                    text_buffer: String::new(),
                    children: Vec::new(),
                });
            }
            Event::End(TagEnd::Item) => {
                if let Some(item_frame) = item_stack.pop() {
                    let list_item = ListItem {
                        text: item_frame.text_buffer.trim().to_string(),
                        task_status: item_frame.task_status,
                        children: item_frame.children,
                    };
                    if let Some(parent_list) = list_stack.last_mut() {
                        parent_list.items.push(list_item);
                    }
                }
            }
            Event::TaskListMarker(checked) => {
                if let Some(item) = item_stack.last_mut() {
                    item.task_status = Some(if checked {
                        TaskStatus::Complete
                    } else {
                        TaskStatus::Incomplete
                    });
                }
            }
            Event::Text(text) => {
                if in_metadata_block {
                    metadata_buffer.push_str(&text);
                } else if let Some((_, _, link_text)) = &mut active_link {
                    link_text.push_str(&text);
                } else if let Some(item) = item_stack.last_mut() {
                    item.text_buffer.push_str(&text);
                }
            }
            Event::SoftBreak | Event::HardBreak => {
                if let Some(item) = item_stack.last_mut() {
                    item.text_buffer.push(' ');
                }
            }
            _ => {}
        }
    }

    Note::new(frontmatter, lists, outlinks, code_regions)
}

#[cfg(test)]
mod tests {
    use pretty_assertions::{assert_eq, assert_ne};

    use super::*;

    #[test]
    fn extracts_yaml_frontmatter() {
        let input =
            "---\ntitle: My Note\ntags: [rust, pkm]\n---\n# Header\nBody text";
        let note = parse_markdown(input);
        let fm = note.frontmatter().expect("frontmatter present");
        assert_eq!(fm.raw(), "title: My Note\ntags: [rust, pkm]\n");
        assert_eq!(fm.is_empty(), false);
    }

    #[test]
    fn extracts_markdown_and_wikilinks() {
        let input = "Here is a [[target_page|Display Alias]] and [[simple_target]] as well as [Markdown Link](https://example.com).";
        let note = parse_markdown(input);
        let links = note.outlinks();
        assert_eq!(links.len(), 3);

        assert_eq!(links.get(0).map(Outlink::target), Some("target_page"));
        assert_eq!(links.get(0).map(Outlink::text), Some("Display Alias"));
        assert_eq!(links.get(0).map(Outlink::is_wikilink), Some(true));

        assert_eq!(links.get(1).map(Outlink::target), Some("simple_target"));
        assert_eq!(links.get(1).map(Outlink::is_wikilink), Some(true));

        assert_eq!(
            links.get(2).map(Outlink::target),
            Some("https://example.com")
        );
        assert_eq!(links.get(2).map(Outlink::text), Some("Markdown Link"));
        assert_eq!(links.get(2).map(Outlink::is_markdown), Some(true));
    }

    #[test]
    fn extracts_lists_and_tasks() {
        let input = "- [ ] Buy milk\n- [x] Read book\n  - [ ] Subtask 1\n- \
                     Plain bullet";
        let note = parse_markdown(input);
        assert_eq!(note.lists().len(), 1);

        let list = note.lists().get(0).expect("list present");
        assert_eq!(list.is_ordered(), false);
        assert_eq!(list.items().len(), 3);

        let item0 = list.items().get(0).expect("item 0");
        assert_eq!(item0.text(), "Buy milk");
        assert_eq!(item0.is_task(), true);
        assert_eq!(item0.is_completed(), false);

        let item1 = list.items().get(1).expect("item 1");
        assert_eq!(item1.text(), "Read book");
        assert_eq!(item1.is_completed(), true);
        assert_eq!(item1.children().len(), 1);

        let sub_list = item1.children().get(0).expect("sub list");
        assert_eq!(sub_list.items().len(), 1);
        let sub_item = sub_list.items().get(0).expect("sub item");
        assert_eq!(sub_item.text(), "Subtask 1");
        assert_eq!(sub_item.is_task(), true);
        assert_eq!(sub_item.is_completed(), false);

        let item2 = list.items().get(2).expect("item 2");
        assert_eq!(item2.text(), "Plain bullet");
        assert_eq!(item2.is_task(), false);
    }

    #[test]
    fn note_tasks_accessor_returns_all_task_items() {
        let input = "- [ ] Top task 1\n- Plain bullet\n  - [x] Nested task \
                     1\n- [ ] Top task 2";
        let note = parse_markdown(input);
        let tasks: Vec<&ListItem> = note.tasks().collect();
        assert_eq!(tasks.len(), 3);

        assert_eq!(tasks.get(0).map(|t| t.text()), Some("Top task 1"));
        assert_eq!(tasks.get(0).map(|t| t.is_completed()), Some(false));

        assert_eq!(tasks.get(1).map(|t| t.text()), Some("Nested task 1"));
        assert_eq!(tasks.get(1).map(|t| t.is_completed()), Some(true));

        assert_eq!(tasks.get(2).map(|t| t.text()), Some("Top task 2"));
        assert_eq!(tasks.get(2).map(|t| t.is_completed()), Some(false));
    }

    #[test]
    fn tracks_code_regions_for_inline_and_fenced_code() {
        let input =
            "Text with `inline code` span.\n\n```rust\nfn main() {}\n```\n";
        let note = parse_markdown(input);
        let regions = note.code_regions();
        assert_eq!(regions.len(), 2);

        let r0 = regions.get(0).expect("region 0");
        assert_eq!(&input[r0.range()], "`inline code`");

        let r1 = regions.get(1).expect("region 1");
        assert_eq!(&input[r1.range()], "```rust\nfn main() {}\n```");
    }
}
