//! Markdown event parser for [`Note`] records.
//!
//! [`parse_markdown`] walks a `pulldown-cmark` event stream once, building a
//! [`Note`] from frontmatter, lists, outlinks, inline fields, and tags.
//!
//! Parser state lives in [`ParserContext`], while [`ListTracker`] handles
//! explicit list and list-item stacks so nested Markdown never recurses
//! through the call stack.
//!
//! Inline fields and tags are lexed from parser-built plain-text buffers: one
//! per top-level paragraph or heading, and one per list item. The buffers
//! exclude fenced code blocks, indented code blocks, and inline code.
//!
//! Standard Markdown link text is copied into the surrounding scan buffer
//! wrapped in literal `[` and `]` delimiters, so `[Key:: Value](url)` becomes a
//! visible-key inline field while [`ListItem::text`] retains the plain display
//! text.

use std::{mem, path::PathBuf};

use indexmap::IndexMap;
use pulldown_cmark::{
    CowStr, Event, LinkType as CmarkLinkType, Options, Parser, Tag as CmarkTag,
    TagEnd,
};

use super::{
    Frontmatter, Link, LinkType, List, ListItem, Note, RawFrontmatter, Tag,
    TaskStatus, lexer,
};
use crate::field::FieldKey;

/// Parses Markdown source into a [`Note`].
///
/// Enables task lists, YAML frontmatter blocks, and Obsidian wikilinks.
///
/// The parser walks the `pulldown-cmark` event stream once, collecting
/// frontmatter, lists, outlinks, inline fields, and tags in document order.
/// Inline fields and tags are excluded from fenced code blocks, indented code
/// blocks, and inline code spans.
///
/// # Examples
///
/// ```rust
/// # #[cfg(feature = "test-utils")]
/// # {
/// use traces_pkm::parse_markdown;
///
/// let note = parse_markdown("note.md", "# Hello\nStatus:: Draft");
/// assert!(note.outlinks().is_empty());
/// assert_eq!(note.tags().len(), 0);
/// # }
/// ```
#[inline]
#[must_use]
pub fn parse_markdown<P: Into<PathBuf>>(path: P, src: &str) -> Note {
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_TASKLISTS);
    opts.insert(Options::ENABLE_YAML_STYLE_METADATA_BLOCKS);
    opts.insert(Options::ENABLE_WIKILINKS);

    let mut ctx = ParserContext::default();
    for event in Parser::new_ext(src, opts) {
        ctx.handle_event(event);
    }
    ctx.into_note(path)
}

/// The top-level block currently being parsed.
///
/// Metadata, code, and text blocks are mutually exclusive.
#[derive(Default, Eq, PartialEq)]
enum BlockContext {
    #[default]
    None,
    MetadataBlock,
    CodeBlock,
    Text,
}

/// Inline fields and tags flushed from a closed list item's scan buffer.
type FlushedFields =
    Option<(IndexMap<FieldKey, Vec<super::NoteFieldValue>>, Vec<Tag>)>;

/// State accumulated while walking Markdown events for one note.
#[derive(Default)]
struct ParserContext {
    frontmatter: Option<Frontmatter>,
    block: BlockContext,
    metadata_buffer: String,
    outlinks: Vec<Link>,
    /// The link currently being walked, if any.
    active_link: Option<ActiveLink>,
    list_nesting: ListTracker,
    body_buffer: String,
    inline_fields: IndexMap<FieldKey, Vec<super::NoteFieldValue>>,
    tags: Vec<Tag>,
}

impl ParserContext {
    /// Dispatches one Markdown event to the matching handler.
    fn handle_event(&mut self, event: Event<'_>) {
        match event {
            Event::Start(CmarkTag::MetadataBlock(_)) => {
                self.start_metadata_block();
            }
            Event::End(TagEnd::MetadataBlock(_)) => self.end_metadata_block(),
            Event::Start(CmarkTag::Link {
                link_type,
                dest_url,
                ..
            }) => self.start_link(link_type, dest_url),
            Event::End(TagEnd::Link) => self.end_link(),
            Event::Start(CmarkTag::CodeBlock(_)) => {
                self.start_code_block();
            }
            Event::End(TagEnd::CodeBlock) => self.end_code_block(),
            Event::Start(
                CmarkTag::Paragraph
                | CmarkTag::Heading {
                    ..
                },
            ) => {
                self.start_text_block();
            }
            Event::End(TagEnd::Paragraph | TagEnd::Heading(_)) => {
                self.end_text_block();
            }
            Event::Code(text) => self.inline_code(&text),
            Event::Start(CmarkTag::List(start_number)) => {
                self.start_list(start_number.is_some());
            }
            Event::End(TagEnd::List(_)) => self.end_list(),
            Event::Start(CmarkTag::Item) => self.start_item(),
            Event::End(TagEnd::Item) => self.end_item(),
            Event::TaskListMarker(checked) => self.set_task_status(checked),
            Event::Text(text) => self.push_text(&text),
            Event::SoftBreak | Event::HardBreak => self.push_break(),
            _ => {}
        }
    }

    /// Consumes the accumulated context into a [`Note`] at `path`.
    fn into_note(self, path: impl Into<PathBuf>) -> Note {
        Note::new(
            path,
            self.frontmatter,
            self.list_nesting.lists,
            self.outlinks,
        )
        .with_inline_fields(self.inline_fields)
        .with_tags(self.tags)
    }

    fn start_metadata_block(&mut self) {
        self.block = BlockContext::MetadataBlock;
        self.metadata_buffer.clear();
    }

    fn end_metadata_block(&mut self) {
        self.block = BlockContext::None;
        let raw_text = mem::take(&mut self.metadata_buffer);
        let raw = RawFrontmatter::new(raw_text);
        if !raw.is_empty() {
            self.frontmatter = Some(Frontmatter::from(&raw));
        }
    }

    /// Starts tracking a Markdown or wikilink outlink.
    ///
    /// Standard Markdown links push `[` into the scan buffer (and `]` in
    /// [`Self::end_link`]) so visible-key inline fields can be detected in
    /// link text.
    fn start_link(&mut self, link_type: CmarkLinkType, dest_url: CowStr<'_>) {
        let kind = if matches!(link_type, CmarkLinkType::WikiLink { .. }) {
            LinkType::Wikilink
        } else {
            LinkType::Markdown
        };
        if kind == LinkType::Markdown {
            self.push_scan_char('[');
        }
        self.active_link = Some(ActiveLink::new(kind, dest_url.into_string()));
    }

    /// Records the active [`Link`] and closes any scan-buffer bracket.
    fn end_link(&mut self) {
        if let Some(ActiveLink {
            target,
            kind,
            text,
        }) = self.active_link.take()
        {
            if kind == LinkType::Markdown {
                self.push_scan_char(']');
            }
            self.outlinks.push(Link::new(target, text, kind));
        }
    }

    const fn start_code_block(&mut self) {
        self.block = BlockContext::CodeBlock;
    }

    const fn end_code_block(&mut self) {
        self.block = BlockContext::None;
    }

    /// Starts a paragraph or heading text block.
    ///
    /// Top-level text fills `body_buffer`. Text within list items is separated
    /// by newlines in the active item's scan buffer.
    fn start_text_block(&mut self) {
        self.block = BlockContext::Text;
        if !self.list_nesting.start_nested_text_block() {
            self.body_buffer.clear();
        }
    }

    /// Lexes a completed top-level text block.
    ///
    /// Nested text blocks are handled through the active list item.
    fn end_text_block(&mut self) {
        self.block = BlockContext::None;
        if !self.list_nesting.is_item_active() {
            for (key, value) in lexer::extract_inline_fields(&self.body_buffer)
            {
                self.inline_fields.entry(key).or_default().push(value);
            }
            self.tags.extend(lexer::extract_tags(&self.body_buffer));
            self.body_buffer.clear();
        }
    }

    /// Records an inline code span and keeps it out of metadata scanning.
    ///
    /// Inline code remains in list item display text.
    fn inline_code(&mut self, text: &str) {
        self.list_nesting.inline_code(text);
    }

    /// Folds a flushed item's inline fields and tags into this context's
    /// document-order streams, if any were flushed.
    fn extend_from_flush(&mut self, flushed: FlushedFields) {
        if let Some((fields, tags)) = flushed {
            for (key, values) in fields {
                self.inline_fields.entry(key).or_default().extend(values);
            }
            self.tags.extend(tags);
        }
    }

    /// Pushes a list frame and flushes any active parent item scan buffer.
    ///
    /// Flushing before nested lists keeps parent metadata before child
    /// metadata.
    fn start_list(&mut self, is_ordered: bool) {
        let flushed = self.list_nesting.start_list(is_ordered);
        self.extend_from_flush(flushed);
    }

    /// Closes the innermost list.
    ///
    /// A list nested inside an active item is stored under
    /// [`ListItem::children`]. Otherwise, it becomes a top-level
    /// [`Note::lists`] entry.
    fn end_list(&mut self) {
        self.list_nesting.end_list();
    }

    fn start_item(&mut self) {
        self.list_nesting.start_item();
    }

    /// Flushes and records the innermost list item.
    fn end_item(&mut self) {
        let flushed = self.list_nesting.end_item();
        self.extend_from_flush(flushed);
    }

    fn set_task_status(&mut self, checked: bool) {
        self.list_nesting.set_task_status(checked);
    }

    /// Appends text to every active output buffer.
    ///
    /// The same text can update frontmatter, link display text, list item text,
    /// and metadata scan buffers. Scan buffers skip code block content.
    fn push_text(&mut self, text: &str) {
        if self.block == BlockContext::MetadataBlock {
            self.metadata_buffer.push_str(text);
            return;
        }
        if let Some(link) = self.active_link.as_mut() {
            link.text.push_str(text);
        }
        if self
            .list_nesting
            .push_text(text, self.block == BlockContext::CodeBlock)
        {
            return;
        }
        if self.block == BlockContext::Text {
            self.body_buffer.push_str(text);
        }
    }

    /// Appends a Markdown line break to the active text buffer.
    fn push_break(&mut self) {
        if self.block == BlockContext::MetadataBlock {
            self.metadata_buffer.push('\n');
            return;
        }
        if self.list_nesting.push_break() {
            return;
        }
        if self.block == BlockContext::Text {
            self.body_buffer.push('\n');
        }
    }

    /// Pushes a literal character into the active scan buffer.
    ///
    /// Used to reconstruct Markdown link brackets for visible-key inline field
    /// scanning.
    fn push_scan_char(&mut self, ch: char) {
        if self.list_nesting.push_scan_char(ch) {
            return;
        }
        if self.block == BlockContext::Text {
            self.body_buffer.push(ch);
        }
    }
}

/// A link currently being walked, accumulating its display text.
struct ActiveLink {
    target: String,
    kind: LinkType,
    text: String,
}

impl ActiveLink {
    const fn new(kind: LinkType, target: String) -> Self {
        Self {
            target,
            kind,
            text: String::new(),
        }
    }
}

/// Nested list and list-item state for one Markdown event stream.
///
/// Completed top-level lists live in `lists`; still-open lists and items live
/// on explicit stacks.
#[derive(Default)]
struct ListTracker {
    lists: Vec<List>,
    list_stack: Vec<ListFrame>,
    item_stack: Vec<ItemFrame>,
}

impl ListTracker {
    /// Starts a nested text block inside the active item.
    ///
    /// Separates it from prior scan-buffer content with a newline. Returns
    /// `false` if no list item is active, allowing the caller to treat the
    /// block as top-level text.
    fn start_nested_text_block(&mut self) -> bool {
        let Some(item) = self.item_stack.last_mut() else {
            return false;
        };
        if !item.scan_buffer.is_empty() {
            item.scan_buffer.push('\n');
        }
        true
    }

    /// Returns `true` if there is an active list item.
    const fn is_item_active(&self) -> bool {
        !self.item_stack.is_empty()
    }

    /// Records an inline code span on the active item's display text only.
    /// Inline code is excluded from inline field/tag scanning.
    fn inline_code(&mut self, text: &str) {
        if let Some(item) = self.item_stack.last_mut() {
            item.push_code(text);
        }
    }

    /// Lexes and clears the active list item's scan buffer.
    ///
    /// Returns the inline fields and tags yielded by that buffer, or `None` if
    /// no item is active or the buffer is empty. Called before nested lists
    /// start and when an item closes, both to preserve document-order metadata.
    fn flush_active_item_scan_buffer(&mut self) -> FlushedFields {
        let item = self.item_stack.last_mut()?;
        if item.scan_buffer.is_empty() {
            return None;
        }
        let text = mem::take(&mut item.scan_buffer);
        let raw_fields = if item.task_status.is_some() {
            lexer::extract_task_inline_fields(&text)
        } else {
            lexer::extract_inline_fields(&text)
        };
        // Two independently owned copies, not a borrow-checker workaround:
        // `item.fields` lets a task/list item resolve its own metadata
        // (`ListItem::fields`), while the returned copy feeds the caller's
        // document-order stream every page-level query already relies on. Both
        // outlive this function inside different serialized structs, so neither
        // can borrow from the other.
        let mut item_fields: IndexMap<FieldKey, Vec<super::NoteFieldValue>> =
            IndexMap::new();
        let mut page_fields: IndexMap<FieldKey, Vec<super::NoteFieldValue>> =
            IndexMap::new();
        for (key, value) in raw_fields {
            item_fields.entry(key.clone()).or_default().push(value.clone());
            page_fields.entry(key).or_default().push(value);
        }
        item.fields = item_fields;
        let tags = lexer::extract_tags(&text);
        Some((page_fields, tags))
    }

    /// Pushes a list frame and flushes any active parent item's scan buffer.
    ///
    /// Returns the flushed inline fields and tags, if any.
    fn start_list(&mut self, is_ordered: bool) -> FlushedFields {
        let flushed = self.flush_active_item_scan_buffer();
        self.list_stack.push(ListFrame {
            is_ordered,
            items: Vec::new(),
        });
        flushed
    }

    /// Closes the innermost list.
    ///
    /// A list nested inside an active item is stored under
    /// [`ListItem::children`]. Otherwise, it becomes a top-level completed
    /// list.
    fn end_list(&mut self) {
        if let Some(frame) = self.list_stack.pop() {
            let list = List::new(frame.is_ordered, frame.items);
            if let Some(item) = self.item_stack.last_mut() {
                item.children.push(list);
            } else {
                self.lists.push(list);
            }
        }
    }

    fn start_item(&mut self) {
        self.item_stack.push(ItemFrame {
            task_status: None,
            text_buffer: String::new(),
            scan_buffer: String::new(),
            fields: IndexMap::new(),
            children: Vec::new(),
        });
    }

    /// Flushes and records the innermost list item.
    ///
    /// Returns the flushed inline fields and tags, if any.
    fn end_item(&mut self) -> FlushedFields {
        let flushed = self.flush_active_item_scan_buffer();
        if let Some(item_frame) = self.item_stack.pop() {
            let item = ListItem::with_children(
                item_frame.text_buffer,
                item_frame.task_status,
                item_frame.children,
            )
            .with_fields(item_frame.fields);
            if let Some(list_frame) = self.list_stack.last_mut() {
                list_frame.items.push(item);
            }
        }
        flushed
    }

    fn set_task_status(&mut self, checked: bool) {
        if let Some(item) = self.item_stack.last_mut() {
            item.task_status = Some(if checked {
                TaskStatus::Complete
            } else {
                TaskStatus::Incomplete
            });
        }
    }

    /// Appends text to the active item's display text and scan buffer.
    ///
    /// Returns `false` if there is no active item.
    fn push_text(&mut self, text: &str, in_code_block: bool) -> bool {
        let Some(item) = self.item_stack.last_mut() else {
            return false;
        };
        item.push_text(text, in_code_block);
        true
    }

    /// Appends a line break to the active item's buffers.
    ///
    /// Returns `false` if there is no active item.
    fn push_break(&mut self) -> bool {
        let Some(item) = self.item_stack.last_mut() else {
            return false;
        };
        item.push_break();
        true
    }

    /// Pushes a literal character into the active item's scan buffer.
    ///
    /// Returns `false` if there is no active item.
    fn push_scan_char(&mut self, ch: char) -> bool {
        let Some(item) = self.item_stack.last_mut() else {
            return false;
        };
        item.push_scan_char(ch);
        true
    }
}

/// An active list frame on the parser stack.
struct ListFrame {
    is_ordered: bool,
    items: Vec<ListItem>,
}

/// An active list item frame on the parser stack.
struct ItemFrame {
    task_status: Option<TaskStatus>,
    text_buffer: String,
    /// Mirrors `text_buffer` but excludes code text.
    ///
    /// [`ListTracker::flush_active_item_scan_buffer`] lexes this buffer for
    /// inline fields and tags.
    scan_buffer: String,
    /// Inline fields lexed from this item's own text.
    ///
    /// Kept separate from child items' fields so [`ListItem::fields`] resolves
    /// per-item, not per-list.
    fields: IndexMap<FieldKey, Vec<super::NoteFieldValue>>,
    children: Vec<List>,
}

impl ItemFrame {
    /// Appends text to display text and, outside code blocks, the scan buffer.
    fn push_text(&mut self, text: &str, in_code_block: bool) {
        self.text_buffer.push_str(text);
        if !in_code_block {
            self.scan_buffer.push_str(text);
        }
    }

    /// Appends a line break to both the display text and scan buffer.
    fn push_break(&mut self) {
        self.text_buffer.push('\n');
        self.scan_buffer.push('\n');
    }

    /// Pushes a literal character into the scan buffer only.
    ///
    /// Used to reconstruct Markdown link brackets for visible-key inline
    /// field scanning.
    fn push_scan_char(&mut self, ch: char) {
        self.scan_buffer.push(ch);
    }

    /// Appends inline code text to display text only.
    ///
    /// Inline code is excluded from inline field and tag scanning.
    fn push_code(&mut self, text: &str) {
        self.text_buffer.push_str(text);
    }
}

#[cfg(test)]
mod tests {
    use super::{super::NoteFieldValue, *};
    mod parse {
        use pretty_assertions::assert_eq;
        use rstest::rstest;

        use super::*;

        #[test]
        fn returns_empty_note_when_source_is_empty() {
            let input = "";
            let note = parse_markdown("note.md", input);

            assert_eq!(note.path(), std::path::Path::new("note.md"));
            assert_eq!(note.frontmatter(), None);
            assert_eq!(note.lists().len(), 0);
            assert_eq!(note.outlinks().len(), 0);
            assert_eq!(note.inline_fields().len(), 0);
            assert_eq!(note.tags().len(), 0);
        }

        #[test]
        fn returns_none_for_frontmatter_when_absent() {
            let input = "# Header\nNo YAML block.";
            let note = parse_markdown("note.md", input);

            assert_eq!(note.frontmatter(), None);
        }

        #[test]
        fn extracts_yaml_frontmatter_block_fields() {
            let input = "---\ntitle: My Note\ntags: [rust, pkm]\n---\n# Header";
            let note = parse_markdown("note.md", input);

            assert_eq!(note.frontmatter().map(|fm| fm.fields().len()), Some(2));
            assert_eq!(
                note.frontmatter().map(Frontmatter::is_empty),
                Some(false)
            );
        }

        #[test]
        fn returns_none_for_frontmatter_when_yaml_block_is_empty() {
            let input = "---\n---\n# Header";
            let note = parse_markdown("note.md", input);

            assert_eq!(note.frontmatter(), None);
        }

        #[test]
        fn returns_empty_frontmatter_when_yaml_block_is_malformed() {
            let input = "---\ninvalid: [yaml: :\n---\n# Header";
            let note = parse_markdown("note.md", input);

            assert_eq!(
                note.frontmatter().map(Frontmatter::is_empty),
                Some(true)
            );
        }

        #[test]
        fn extracts_structured_fields_from_yaml_frontmatter() {
            let input = "---\ntitle: Note Title\nauthor: Alice\ndraft: \
                         true\nrating: 5.0\ndate: 2026-07-29\n---\nBody text.";
            let note = parse_markdown("note.md", input);

            let fields: std::collections::BTreeMap<&str, &NoteFieldValue> =
                note.frontmatter()
                    .into_iter()
                    .flat_map(Frontmatter::fields)
                    .map(|(k, v)| (k.name(), v))
                    .collect();
            assert_eq!(fields.len(), 5);
            assert_eq!(
                fields.get("title").copied(),
                Some(&NoteFieldValue::String("Note Title".to_owned()))
            );
            assert_eq!(
                fields.get("draft").copied(),
                Some(&NoteFieldValue::Bool(true))
            );
            assert_eq!(
                fields.get("rating").copied(),
                Some(&NoteFieldValue::Number(5.0))
            );
            assert_eq!(
                fields.get("date").copied(),
                Some(&NoteFieldValue::Date("2026-07-29".to_owned()))
            );
        }

        #[test]
        fn extracts_wikilink_values_from_yaml_frontmatter() {
            let note = parse_markdown(
                "note.md",
                "---\nrelated: \"[[Project Alpha|Alpha]]\"\n---\nBody text.",
            );

            let field = note
                .frontmatter()
                .into_iter()
                .flat_map(Frontmatter::fields)
                .find(|(k, _)| k.is_canonical_match("related"))
                .expect("related field");
            assert_eq!(
                field.1,
                &NoteFieldValue::Link(Link::new(
                    "Project Alpha",
                    "Alpha",
                    LinkType::Wikilink
                ))
            );
        }

        #[rstest]
        #[case::wikilink_with_alias(
            "See [[target_page|Display Alias]] for context.",
            "target_page",
            "Display Alias",
            LinkType::Wikilink
        )]
        #[case::wikilink_without_alias(
            "See [[simple_target]] for details.",
            "simple_target",
            "simple_target",
            LinkType::Wikilink
        )]
        #[case::markdown_link(
            "Check out [Markdown Link](https://example.com).",
            "https://example.com",
            "Markdown Link",
            LinkType::Markdown
        )]
        fn extracts_outlinks(
            #[case] input: &str,
            #[case] expected_target: &str,
            #[case] expected_text: &str,
            #[case] expected_kind: LinkType,
        ) {
            let note = parse_markdown("note.md", input);

            let link = note.outlinks().first().expect("outlink present");
            assert_eq!(link.target(), expected_target);
            assert_eq!(link.text(), expected_text);
            assert_eq!(link.kind(), expected_kind);
        }

        #[test]
        fn extracts_task_item_completion_status() {
            let input = "- [ ] Incomplete task\n- [x] Completed task";
            let note = parse_markdown("note.md", input);

            let list = note.lists().first().expect("list present");
            let item0 = list.items().first().expect("item 0");
            let item1 = list.items().get(1).expect("item 1");

            assert_eq!(item0.text(), "Incomplete task");
            assert_eq!(item0.is_task(), true);
            assert_eq!(item0.is_completed(), false);

            assert_eq!(item1.text(), "Completed task");
            assert_eq!(item1.is_task(), true);
            assert_eq!(item1.is_completed(), true);
        }

        #[test]
        fn includes_link_display_text_in_the_containing_item_text() {
            let input = "- [ ] Check [link text](https://example.com) here";
            let note = parse_markdown("note.md", input);

            let item = note
                .lists()
                .first()
                .and_then(|list| list.items().first())
                .expect("item present");
            assert_eq!(item.text(), "Check link text here");

            let link = note.outlinks().first().expect("outlink present");
            assert_eq!(link.text(), "link text");
        }

        #[test]
        fn extracts_nested_child_lists() {
            let input = "- Parent item\n  - Child item";
            let note = parse_markdown("note.md", input);

            let parent_list = note.lists().first().expect("parent list");
            let parent_item = parent_list.items().first().expect("parent item");
            assert_eq!(parent_item.children().len(), 1);

            let child_list =
                parent_item.children().first().expect("child list");
            let child_item = child_list.items().first().expect("child item");
            assert_eq!(child_item.text(), "Child item");
        }

        #[test]
        fn extracts_grandchild_lists_beyond_two_levels() {
            let input = "- Parent\n  - Child\n    - Grandchild";
            let note = parse_markdown("note.md", input);

            let grandchild = note
                .lists()
                .first()
                .and_then(|list| list.items().first())
                .and_then(|item| item.children().first())
                .and_then(|list| list.items().first())
                .and_then(|item| item.children().first())
                .and_then(|list| list.items().first())
                .map(ListItem::text);
            assert_eq!(grandchild, Some("Grandchild"));
        }

        #[rstest]
        #[case::unordered_list("- First\n- Second", false)]
        #[case::ordered_list("1. First step\n2. Second step", true)]
        fn extracts_list_ordering(
            #[case] input: &str,
            #[case] expected_ordered: bool,
        ) {
            let note = parse_markdown("note.md", input);

            let list = note.lists().first().expect("list present");
            assert_eq!(list.is_ordered(), expected_ordered);
            assert_eq!(list.items().len(), 2);
        }

        #[rstest]
        #[test]
        fn preserves_soft_breaks_inside_list_item_text() {
            let note = parse_markdown("note.md", "- Wrapped\n  line");

            let text = note
                .lists()
                .first()
                .and_then(|list| list.items().first())
                .map(ListItem::text);
            assert_eq!(text, Some("Wrapped\nline"));
        }
    }

    mod tasks {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn iterates_top_level_task_items() {
            let input = "- [ ] Task 1\n- Plain item\n- [x] Task 2";
            let note = parse_markdown("note.md", input);

            let tasks: Vec<&ListItem> = note.tasks().collect();
            assert_eq!(tasks.len(), 2);
            assert_eq!(tasks.first().map(|t| t.text()), Some("Task 1"));
            assert_eq!(tasks.get(1).map(|t| t.text()), Some("Task 2"));
        }

        #[test]
        fn iterates_nested_sub_list_task_items() {
            let input = "- Plain parent\n  - [x] Subtask 1";
            let note = parse_markdown("note.md", input);

            let tasks: Vec<&ListItem> = note.tasks().collect();
            assert_eq!(tasks.len(), 1);
            assert_eq!(tasks.first().map(|t| t.text()), Some("Subtask 1"));
            assert_eq!(tasks.first().map(|t| t.is_completed()), Some(true));
        }
    }

    mod inline_metadata {
        use pretty_assertions::assert_eq;
        use rstest::rstest;

        use super::*;
        use crate::note::Tag;

        #[rstest]
        #[case::body("Author:: Jane Doe", "author", "Jane Doe")]
        #[case::visible_key(
            "See the [Status:: Draft] note.",
            "status",
            "Draft"
        )]
        #[case::hidden_key("See the (Status:: Draft) note.", "status", "Draft")]
        fn extracts_a_field_in_its_declared_form_from_body_text(
            #[case] input: &str,
            #[case] expected_key: &str,
            #[case] expected_value: &str,
        ) {
            let note = parse_markdown("note.md", input);

            assert_eq!(note.inline_fields().len(), 1);
            let (key, values) =
                note.inline_fields().iter().next().expect("field present");
            assert!(key.is_canonical_match(expected_key));
            assert_eq!(
                values.first().and_then(|v| v.as_str()),
                Some(expected_value)
            );
        }

        #[test]
        fn extracts_a_field_from_each_of_two_separate_paragraphs() {
            let note =
                parse_markdown("note.md", "Status:: Draft\n\nAuthor:: Jane");

            let keys: Vec<&str> = note
                .inline_fields()
                .keys()
                .map(crate::field::FieldKey::name)
                .collect();
            assert_eq!(keys, ["Status", "Author"]);
        }

        #[test]
        fn extracts_a_bare_field_from_a_list_item_and_keeps_it_in_item_text() {
            let note = parse_markdown("note.md", "- Status:: Draft");
            assert_eq!(note.inline_fields().len(), 1);

            let item = note
                .lists()
                .first()
                .and_then(|list| list.items().first())
                .expect("item present");
            assert_eq!(item.text(), "Status:: Draft");

            let (key, values) =
                note.inline_fields().iter().next().expect("field present");
            assert!(key.is_canonical_match("status"));
            assert_eq!(values.first().and_then(|v| v.as_str()), Some("Draft"));
        }

        #[test]
        fn scopes_a_list_item_field_to_that_item_and_not_its_siblings() {
            let note = parse_markdown(
                "note.md",
                "- [ ] First task [priority:: high]\n- [ ] Second task \
                 [priority:: low]",
            );

            let list = note.lists().first().expect("list present");
            let mut items = list.items().iter();
            let first = items.next().expect("first item present");
            let second = items.next().expect("second item present");

            let first_priority = first
                .fields()
                .iter()
                .find(|(k, _)| k.is_canonical_match("priority"))
                .expect("first item field present");
            assert_eq!(
                first_priority.1.first().and_then(|v| v.as_str()),
                Some("high")
            );

            let second_priority = second
                .fields()
                .iter()
                .find(|(k, _)| k.is_canonical_match("priority"))
                .expect("second item field present");
            assert_eq!(
                second_priority.1.first().and_then(|v| v.as_str()),
                Some("low")
            );

            // Both fields still surface on the page-level bag, unscoped,
            // grouped under the same key.
            assert_eq!(note.inline_fields().len(), 1);
            assert_eq!(
                note.inline_fields()
                    .values()
                    .next()
                    .expect("values present")
                    .len(),
                2
            );
        }

        #[test]
        fn scopes_a_task_emoji_shorthand_field_to_its_own_item() {
            let note = parse_markdown(
                "note.md",
                "- [ ] First task 🗓️2026-01-01\n- [ ] Second task 🗓️2026-02-02",
            );

            let list = note.lists().first().expect("list present");
            let mut items = list.items().iter();
            let first = items.next().expect("first item present");
            let second = items.next().expect("second item present");

            let (first_key, first_vals) =
                first.fields().iter().next().expect("first due field");
            assert!(first_key.is_canonical_match("due"));
            assert_eq!(
                first_vals.first().and_then(|v| v.as_str()),
                Some("2026-01-01")
            );

            let (_second_key, second_vals) =
                second.fields().iter().next().expect("second due field");
            assert_eq!(
                second_vals.first().and_then(|v| v.as_str()),
                Some("2026-02-02")
            );
        }

        #[test]
        fn plain_list_items_without_fields_have_no_scoped_fields() {
            let note = parse_markdown("note.md", "- Plain item with no fields");

            let item = note
                .lists()
                .first()
                .and_then(|list| list.items().first())
                .expect("item present");
            assert!(item.fields().is_empty());
        }

        #[rstest]
        #[case::due_variant_selector(
            "- [ ] testTask 🗓️2022-07-14",
            "due",
            "2022-07-14"
        )]
        #[case::due_text_selector(
            "- [ ] testTask 🗓2022-07-14",
            "due",
            "2022-07-14"
        )]
        #[case::created("- [ ] testTask ➕2022-07-25", "created", "2022-07-25")]
        #[case::start("- [ ] testTask 🛫2022-07-21", "start", "2022-07-21")]
        #[case::scheduled(
            "- [ ] testTask ⏳2022-07-24",
            "scheduled",
            "2022-07-24"
        )]
        #[case::completion(
            "- [x] testTask ✅2022-07-26",
            "completion",
            "2022-07-26"
        )]
        fn extracts_task_emoji_shorthand_fields_from_task_items_only(
            #[case] input: &str,
            #[case] expected_key: &str,
            #[case] expected_date: &str,
        ) {
            let note = parse_markdown("note.md", input);

            assert_eq!(note.inline_fields().len(), 1);
            let (key, values) =
                note.inline_fields().iter().next().expect("field present");
            assert!(key.is_canonical_match(expected_key));
            assert_eq!(
                values.first(),
                Some(&NoteFieldValue::Date(expected_date.to_owned()))
            );
        }

        #[test]
        fn extracts_multiple_task_emoji_shorthand_fields_from_one_task_item() {
            let note = parse_markdown(
                "note.md",
                "- [ ] testTask 🗓2022-07-14 ⏳2022-07-24",
            );

            let fields = note.inline_fields();
            assert_eq!(fields.len(), 2);
            let (due_key, due_vals) = fields.iter().next().expect("due field");
            assert_eq!(due_key.name(), "due");
            assert_eq!(
                due_vals.first(),
                Some(&NoteFieldValue::Date("2022-07-14".to_owned()))
            );
            let (sched_key, sched_vals) =
                fields.iter().nth(1).expect("scheduled field");
            assert_eq!(sched_key.name(), "scheduled");
            assert_eq!(
                sched_vals.first(),
                Some(&NoteFieldValue::Date("2022-07-24".to_owned()))
            );
        }

        #[test]
        fn ignores_task_emoji_shorthand_fields_in_plain_list_items() {
            let note = parse_markdown("note.md", "- Plain item 🗓2022-07-14");

            assert_eq!(note.inline_fields().len(), 0);
        }
        #[test]
        fn ignores_task_emoji_shorthand_fields_outside_task_items() {
            let note = parse_markdown("note.md", "testTask 🗓2022-07-14");

            assert_eq!(note.inline_fields().len(), 0);
        }

        #[rstest]
        #[case::fenced_code_block("```\nKey:: Value\n```")]
        #[case::indented_code_block("Paragraph text.\n\n    Key:: Value\n")]
        #[case::inline_code_span("Text with `Key:: Value` inline.")]
        fn ignores_fields_inside_excluded_code_regions(#[case] input: &str) {
            let note = parse_markdown("note.md", input);

            assert_eq!(note.inline_fields().len(), 0);
        }

        #[test]
        fn extracts_a_tag_from_body_text() {
            let note = parse_markdown("note.md", "Filed under #book today.");

            assert_eq!(note.tags(), [Tag::new("#book")]);
        }

        #[test]
        fn extracts_a_tag_from_a_list_item_and_keeps_it_in_item_text() {
            let note = parse_markdown("note.md", "- Reading #book now");

            let item = note
                .lists()
                .first()
                .and_then(|list| list.items().first())
                .expect("item present");
            assert_eq!(item.text(), "Reading #book now");
            assert_eq!(note.tags(), [Tag::new("#book")]);
        }

        #[rstest]
        #[case::fenced_code_block("```\n#book\n```")]
        #[case::indented_code_block("Paragraph text.\n\n    #book\n")]
        #[case::inline_code_span("Text with `#book` inline.")]
        fn ignores_tags_inside_excluded_code_regions(#[case] input: &str) {
            let note = parse_markdown("note.md", input);

            assert_eq!(note.tags().len(), 0);
        }

        #[test]
        fn extracts_a_bare_field_from_a_second_paragraph_within_a_loose_list_item()
         {
            let note =
                parse_markdown("note.md", "- Task line\n\n  Status:: Draft\n");

            assert_eq!(note.inline_fields().len(), 1);
            let (key, values) =
                note.inline_fields().iter().next().expect("field present");
            assert!(key.is_canonical_match("status"));
            assert_eq!(values.first().and_then(|v| v.as_str()), Some("Draft"));
        }

        #[test]
        fn orders_parent_item_fields_before_nested_child_item_fields() {
            let note = parse_markdown(
                "note.md",
                "- Status:: Draft\n  - Priority:: High\n",
            );

            let keys: Vec<&str> =
                note.inline_fields().iter().map(|(k, _)| k.name()).collect();
            assert_eq!(keys, ["Status", "Priority"]);
        }

        #[test]
        fn orders_parent_item_fields_before_and_after_nested_child_fields() {
            let note = parse_markdown(
                "note.md",
                "- Status:: Draft\n  - Priority:: High\n\n  Reviewer:: Jane\n",
            );

            let keys: Vec<&str> =
                note.inline_fields().iter().map(|(k, _)| k.name()).collect();
            assert_eq!(keys, ["Status", "Priority", "Reviewer"]);
        }

        #[test]
        fn isolates_parent_and_child_item_tags_without_leaking_between_levels()
        {
            let note =
                parse_markdown("note.md", "- Parent #alpha\n  - Child #beta\n");

            assert_eq!(note.tags(), [Tag::new("#alpha"), Tag::new("#beta")]);
        }

        #[rstest]
        #[case::fenced_code_block(
            "- Item text\n\n  ```\n  Key:: Value\n  ```\n"
        )]
        #[case::indented_code_block("- Item text\n\n      Key:: Value\n")]
        #[case::inline_code_span("- Text with `Key:: Value` inline")]
        fn ignores_fields_inside_excluded_code_regions_within_a_list_item(
            #[case] input: &str,
        ) {
            let note = parse_markdown("note.md", input);

            assert_eq!(note.inline_fields().len(), 0);
        }

        #[rstest]
        #[case::fenced_code_block("- Item text\n\n  ```\n  #book\n  ```\n")]
        #[case::indented_code_block("- Item text\n\n      #book\n")]
        #[case::inline_code_span("- Text with `#book` inline")]
        fn ignores_tags_inside_excluded_code_regions_within_a_list_item(
            #[case] input: &str,
        ) {
            let note = parse_markdown("note.md", input);

            assert_eq!(note.tags().len(), 0);
        }

        #[test]
        fn extracts_both_a_field_and_a_tag_from_the_same_list_item_text() {
            let note = parse_markdown("note.md", "- Status:: Draft #urgent");

            let item = note
                .lists()
                .first()
                .and_then(|list| list.items().first())
                .expect("item present");
            assert_eq!(item.text(), "Status:: Draft #urgent");

            assert_eq!(note.inline_fields().len(), 1);
            let (key, values) =
                note.inline_fields().iter().next().expect("field present");
            assert!(key.is_canonical_match("status"));
            assert_eq!(
                values.first().and_then(|v| v.as_str()),
                Some("Draft #urgent")
            );

            assert_eq!(note.tags(), [Tag::new("#urgent")]);
        }

        #[test]
        fn preserves_document_order_between_a_body_field_and_a_list_item_field()
        {
            let note = parse_markdown(
                "note.md",
                "Status:: Draft\n\n- Reviewer:: Jane",
            );

            let keys: Vec<&str> =
                note.inline_fields().iter().map(|(k, _)| k.name()).collect();
            assert_eq!(keys, ["Status", "Reviewer"]);
        }

        #[test]
        fn preserves_document_order_between_a_list_item_field_and_a_body_field()
        {
            let note = parse_markdown(
                "note.md",
                "- Reviewer:: Jane\n\nStatus:: Draft",
            );

            let keys: Vec<&str> =
                note.inline_fields().iter().map(|(k, _)| k.name()).collect();
            assert_eq!(keys, ["Reviewer", "Status"]);
        }

        #[test]
        fn keeps_a_field_value_intact_when_it_directly_abuts_excluded_inline_code()
         {
            let note =
                parse_markdown("note.md", "Status:: Draft`note` more text");

            assert_eq!(note.inline_fields().len(), 1);
            let (key, values) =
                note.inline_fields().iter().next().expect("field present");
            assert!(key.is_canonical_match("status"));
            assert_eq!(
                values.first().and_then(|v| v.as_str()),
                Some("Draft more text")
            );
        }

        #[test]
        fn extracts_a_tag_from_heading_text() {
            let note = parse_markdown("note.md", "# Chapter #book\n\nBody.");

            assert_eq!(note.tags(), [Tag::new("#book")]);
        }

        #[test]
        fn extracts_a_bare_field_from_heading_text() {
            let note = parse_markdown("note.md", "# Status:: Draft");

            assert_eq!(note.inline_fields().len(), 1);
            let (key, values) =
                note.inline_fields().iter().next().expect("field present");
            assert!(key.is_canonical_match("status"));
            assert_eq!(values.first().and_then(|v| v.as_str()), Some("Draft"));
        }

        #[test]
        fn extracts_a_visible_key_field_from_a_markdown_links_display_text() {
            let note = parse_markdown(
                "note.md",
                "[Status:: Draft](http://example.com)",
            );

            assert_eq!(note.inline_fields().len(), 1);
            assert_eq!(note.outlinks().len(), 1);
            let (key, values) =
                note.inline_fields().iter().next().expect("field present");
            assert!(key.is_canonical_match("status"));
            assert_eq!(values.first().and_then(|v| v.as_str()), Some("Draft"));

            let link = note.outlinks().first().expect("outlink present");
            assert_eq!(link.target(), "http://example.com");
            assert_eq!(link.text(), "Status:: Draft");
        }

        #[test]
        fn extracts_a_visible_key_field_from_link_text_amid_other_prose() {
            let note = parse_markdown(
                "note.md",
                "See [Status:: Draft](http://example.com) here.",
            );

            assert_eq!(note.inline_fields().len(), 1);
            let (key, values) =
                note.inline_fields().iter().next().expect("field present");
            assert!(key.is_canonical_match("status"));
            assert_eq!(values.first().and_then(|v| v.as_str()), Some("Draft"));
        }
    }
}
