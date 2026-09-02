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
    Frontmatter, Link, LinkType, List, ListItem, ListItemType, Note,
    RawFrontmatter,
    lexer::InlineTokenLexer,
    lists::ListItemPosition,
    marker::{MarkerPrefix, scan_marker_at_line_end, scan_marker_prefix},
};
use crate::{
    ByteOffset, FieldKey, SourceLine,
    tag::Tag,
    task::{DEFAULT_TASK_STATUSES, TaskStatusMap},
};

/// Parses Markdown source into a [`Note`].
///
/// Recognizes custom task markers, YAML frontmatter blocks, and Obsidian
/// wikilinks.
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
    opts.insert(Options::ENABLE_YAML_STYLE_METADATA_BLOCKS);
    opts.insert(Options::ENABLE_WIKILINKS);

    let mut ctx = ParserContext::new(src);
    for (event, range) in Parser::new_ext(src, opts).into_offset_iter() {
        ctx.handle_event(event, ByteOffset::from(range.start));
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
    /// Precomputed line-start offsets for the source being parsed, used to
    /// populate [`ListItem`]'s `line`/`parent` position fields.
    line_tracker: ByteTracker,
    /// Resolves scanned marker symbols to their [`TaskStatus`], used to
    /// classify status-marked list items in [`ListTracker::end_item`].
    ///
    /// [`TaskStatus`]: crate::task::TaskStatus
    task_statuses: &'static TaskStatusMap,
}

impl ParserContext {
    /// Starts a new context for `source`, precomputing its line-start
    /// offsets.
    #[inline]
    #[must_use]
    fn new(source: &str) -> Self {
        Self {
            frontmatter: None,
            block: BlockContext::default(),
            metadata_buffer: String::new(),
            outlinks: Vec::new(),
            active_link: None,
            list_nesting: ListTracker::default(),
            body_buffer: String::new(),
            inline_fields: IndexMap::new(),
            tags: Vec::new(),
            line_tracker: ByteTracker::new(source),
            task_statuses: &DEFAULT_TASK_STATUSES,
        }
    }

    /// Dispatches one Markdown event to the matching handler.
    ///
    /// `offset` is the event's starting byte offset, used only by
    /// [`Self::start_item`] to resolve the item's source line.
    fn handle_event(&mut self, event: Event<'_>, offset: ByteOffset) {
        match event {
            Event::Start(CmarkTag::MetadataBlock(_)) => {
                self.start_metadata_block();
            }
            Event::End(TagEnd::MetadataBlock(_)) => self.end_metadata_block(),
            Event::Start(CmarkTag::Link {
                link_type,
                dest_url,
                ..
            }) => {
                self.list_nesting.close_leading();
                self.start_link(link_type, dest_url);
            }
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
            Event::Code(text) => {
                self.list_nesting.close_leading();
                self.inline_code(&text);
            }
            Event::Start(CmarkTag::List(start_number)) => {
                self.start_list(start_number.is_some());
            }
            Event::End(TagEnd::List(_)) => self.end_list(),
            Event::Start(CmarkTag::Item) => self.start_item(offset),
            Event::End(TagEnd::Item) => self.end_item(),
            Event::Text(text) => self.push_text(&text),
            Event::SoftBreak | Event::HardBreak => self.push_break(),
            // Inline markup occupying an item's leading slot means the task
            // marker is not at the content start — mirroring pulldown-cmark,
            // which scans for the marker before parsing any inline content.
            // `- **[x] Task**` and `` - `[x]` Task `` stay plain.
            Event::Start(
                CmarkTag::Emphasis
                | CmarkTag::Strong
                | CmarkTag::Strikethrough
                | CmarkTag::Image {
                    ..
                },
            )
            | Event::InlineHtml(_)
            | Event::InlineMath(_)
            | Event::DisplayMath(_)
            | Event::Html(_)
            | Event::FootnoteReference(_) => {
                self.list_nesting.close_leading();
            }
            // Any other event ends the item's first line structurally (a nested
            // list, a loose-item paragraph, the item's end), which counts as
            // the marker's trailing whitespace.
            _ => self.list_nesting.resolve_leading(),
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
            let lexer = InlineTokenLexer::new(false);
            for (key, value) in lexer.extract_fields(&self.body_buffer) {
                self.inline_fields.entry(key).or_default().push(value);
            }
            self.tags.extend(lexer.extract_tags(&self.body_buffer));
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

    /// Computes the item's source line from `offset` and starts tracking it.
    fn start_item(&mut self, offset: ByteOffset) {
        let line = self.line_tracker.byte_to_line(offset);
        self.list_nesting.start_item(line);
    }

    /// Flushes and records the innermost list item.
    fn end_item(&mut self) {
        let flushed = self.list_nesting.end_item(&self.task_statuses);
        self.extend_from_flush(flushed);
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

/// Converts UTF-8 byte offsets into 1-indexed source line numbers.
///
/// Precomputes line-start byte offsets once per document; each conversion is an
/// O(log n) binary search over them via [`slice::partition_point`].
struct ByteTracker {
    /// Byte offset of the first character of each line, ascending; always
    /// starts with `0` for line 1.
    line_starts: Box<[usize]>,
}

impl ByteTracker {
    /// Precomputes line-start offsets for `source`.
    #[inline]
    #[must_use]
    #[expect(
        clippy::naive_bytecount,
        reason = "avoid extra bytecount dependency for simple newline counting"
    )]
    fn new(source: &str) -> Self {
        let count = source.as_bytes().iter().filter(|&&b| b == b'\n').count();
        let mut line_starts = Vec::with_capacity(count.saturating_add(1));
        line_starts.push(0);
        line_starts.extend(
            source
                .match_indices('\n')
                .map(|(offset, _)| offset.saturating_add(1)),
        );
        Self {
            line_starts: line_starts.into_boxed_slice(),
        }
    }

    /// Converts a byte offset into its 1-indexed source line.
    ///
    /// An offset beyond the source length resolves to the last line.
    #[inline]
    #[must_use]
    fn byte_to_line(&self, offset: ByteOffset) -> SourceLine {
        let offset = usize::from(offset);
        let line = self.line_starts.partition_point(|&start| start <= offset);
        SourceLine::new(u32::try_from(line).unwrap_or(u32::MAX))
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

    /// Rejects a pending item-leading marker: inline content occupies the
    /// item's leading slot, so no marker can be recognized there.
    fn close_leading(&mut self) {
        if let Some(item) = self.item_stack.last_mut() {
            item.reject_leading();
        }
    }

    /// Force-decides a pending item-leading marker as if the item's first line
    /// ended: a complete `[<char>]` shape becomes a marker.
    ///
    /// Called before the marker state is read (scan-buffer flushes, item end)
    /// and on block-structure events that terminate the first line.
    fn resolve_leading(&mut self) {
        if let Some(item) = self.item_stack.last_mut() {
            item.resolve_leading();
        }
    }

    /// Lexes and clears the active list item's scan buffer.
    ///
    /// Returns the inline fields and tags yielded by that buffer, or `None` if
    /// no item is active or the buffer is empty. Called before nested lists
    /// start and when an item closes, both to preserve document-order metadata.
    fn flush_active_item_scan_buffer(&mut self) -> FlushedFields {
        // The marker state must be decided before `has_marker` is read: a
        // pending `- [x]` item flushes when a nested list starts, with no
        // trailing-whitespace text chunk ever arriving.
        self.resolve_leading();
        let item = self.item_stack.last_mut()?;
        if item.scan_buffer.is_empty() {
            return None;
        }
        let text = mem::take(&mut item.scan_buffer);
        let lexer = InlineTokenLexer::new(matches!(
            item.leading,
            LeadingMarker::Decided(Some(_))
        ));
        let raw_fields = lexer.extract_fields(&text);
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
            // ponytail: clone needed for two-out pattern (item + page fields)
            item_fields.entry(key.clone()).or_default().push(value.clone());
            page_fields.entry(key).or_default().push(value);
        }
        item.fields = item_fields;
        let tags = lexer.extract_tags(&text);
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

    /// Starts tracking a new list item at `line`.
    ///
    /// `depth` is the number of currently open lists (0-indexed); `parent` is
    /// the innermost active item's line, if this item is nested inside another
    /// item's child list.
    fn start_item(&mut self, line: SourceLine) {
        let depth = u8::try_from(self.list_stack.len().saturating_sub(1))
            .unwrap_or(u8::MAX);
        let parent = self.item_stack.last().map(|item| item.position.line());
        self.item_stack.push(ItemFrame {
            leading: LeadingMarker::Pending,
            text_buffer: String::new(),
            scan_buffer: String::new(),
            fields: IndexMap::new(),
            children: Vec::new(),
            position: ListItemPosition::new(line, depth, parent),
        });
    }

    /// Flushes and records the innermost list item.
    ///
    /// The flush decides any pending leading marker (see
    /// [`Self::resolve_leading`]); a decided marker always resolves to
    /// [`ListItemType::Task`] — tag-filter reclassification into
    /// [`ListItemType::Checkbox`] is a later task-system issue. Returns the
    /// flushed inline fields and tags, if any.
    fn end_item(&mut self, statuses: &TaskStatusMap) -> FlushedFields {
        let flushed = self.flush_active_item_scan_buffer();
        if let Some(item_frame) = self.item_stack.pop() {
            let item_type = match item_frame.leading {
                LeadingMarker::Decided(Some(symbol)) => {
                    ListItemType::Task(statuses.resolve(symbol))
                }
                _ => ListItemType::Plain,
            };
            let item = ListItem::with_children(
                item_frame.text_buffer,
                item_type,
                item_frame.children,
            )
            .with_fields(item_frame.fields)
            .with_position(item_frame.position);
            if let Some(list_frame) = self.list_stack.last_mut() {
                list_frame.items.push(item);
            }
        }
        flushed
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
    /// Decision state for the item-leading task marker. Mirrors
    /// pulldown-cmark's first-pass gating: the marker is only valid at the
    /// item's content start, so the decision is finalized before any inline
    /// content event or block boundary.
    leading: LeadingMarker,
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
    /// This item's source position: line, nesting depth, and parent line.
    position: ListItemPosition,
}

/// Whether an item-leading task marker has been decided, and how.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum LeadingMarker {
    /// The item's first chunks may still assemble into a marker; `text_buffer`
    /// holds the candidate bytes so far.
    Pending,
    /// The leading position is decided: `Some` carries the recognized marker's
    /// symbol (already trimmed from both buffers), `None` means the item does
    /// not start with a marker.
    Decided(Option<char>),
}

impl ItemFrame {
    /// Appends text to display text and, outside code blocks, the scan buffer.
    /// While the leading marker is [`LeadingMarker::Pending`], the chunk first
    /// feeds the incremental marker scan: Markdown splits a leading `[<char>]`
    /// marker across several `Event::Text` runs (observed: `"["`, `"x"`, `"]"`,
    /// `" Task"`), so each chunk extends the candidate and re-classifies it. A
    /// recognized marker is trimmed from both buffers.
    fn push_text(&mut self, text: &str, in_code_block: bool) {
        let pending = matches!(self.leading, LeadingMarker::Pending);
        self.text_buffer.push_str(text);
        if !in_code_block {
            self.scan_buffer.push_str(text);
        }
        if pending {
            match scan_marker_prefix(&self.text_buffer) {
                MarkerPrefix::Incomplete => {}
                MarkerPrefix::Rejected => {
                    self.leading = LeadingMarker::Decided(None);
                }
                MarkerPrefix::Complete(scan) => {
                    let symbol = scan.symbol();
                    let prefix_len = self
                        .text_buffer
                        .len()
                        .saturating_sub(scan.remainder().len());
                    self.trim_marker_prefix(prefix_len);
                    self.leading = LeadingMarker::Decided(Some(symbol));
                }
            }
        }
    }

    /// Appends a line break to both the display text and scan buffer.
    ///
    /// A pending marker is decided first: the break terminates the marker's
    /// trailing-whitespace slot (`- [x]` wrapped over two lines still carries
    /// a marker).
    fn push_break(&mut self) {
        self.decide_pending_at_line_end();
        self.text_buffer.push('\n');
        self.scan_buffer.push('\n');
    }

    /// Rejects a pending marker: inline content (emphasis, code, links, images,
    /// inline HTML) occupies the item's leading slot, so the item does not
    /// start with a marker.
    fn reject_leading(&mut self) {
        if let LeadingMarker::Pending = self.leading {
            self.leading = LeadingMarker::Decided(None);
        }
    }

    /// Force-decides a pending marker as if the item's first line ended: a
    /// complete `[<char>]` shape becomes a marker with empty text.
    fn resolve_leading(&mut self) {
        self.decide_pending_at_line_end();
    }

    /// Decides a pending marker using end-of-line semantics. Always leaves the
    /// marker [`LeadingMarker::Decided`].
    fn decide_pending_at_line_end(&mut self) {
        if let LeadingMarker::Pending = self.leading {
            let decision = match scan_marker_at_line_end(&self.text_buffer) {
                Some(scan) => {
                    let symbol = scan.symbol();
                    let prefix_len = self.text_buffer.len();
                    self.trim_marker_prefix(prefix_len);
                    LeadingMarker::Decided(Some(symbol))
                }
                None => LeadingMarker::Decided(None),
            };
            self.leading = decision;
        }
    }

    /// Drains the first `prefix_len` bytes from `text_buffer`, and from
    /// `scan_buffer` only when it mirrors those bytes (inline code and link
    /// brackets make the buffers diverge, in which case the scan buffer keeps
    /// its own content).
    fn trim_marker_prefix(&mut self, prefix_len: usize) {
        if self.scan_buffer.as_bytes().get(..prefix_len)
            == self.text_buffer.as_bytes().get(..prefix_len)
        {
            self.scan_buffer.drain(..prefix_len);
        }
        self.text_buffer.drain(..prefix_len);
    }

    /// Pushes a literal character into the scan buffer only.
    ///
    /// Used to reconstruct Markdown link brackets for visible-key inline field
    /// scanning.
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
        #[expect(clippy::panic, reason = "test assertion on enum variant")]
        fn extracts_task_item_completion_status() {
            let input = "- [ ] Incomplete task\n- [x] Completed task";
            let note = parse_markdown("note.md", input);

            let list = note.lists().first().expect("list present");
            let item0 = list.items().first().expect("item 0");
            let item1 = list.items().get(1).expect("item 1");

            assert_eq!(item0.text(), "Incomplete task");
            let ListItemType::Task(status0) = item0.item_type() else {
                panic!("item0 must be a Task, got {:?}", item0.item_type());
            };
            assert_eq!(status0.kind().completed(), Some(false));

            assert_eq!(item1.text(), "Completed task");
            let ListItemType::Task(status1) = item1.item_type() else {
                panic!("item1 must be a Task, got {:?}", item1.item_type());
            };
            assert_eq!(status1.kind().completed(), Some(true));
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

        #[test]
        fn populates_depth_line_and_parent_down_the_nesting_chain() {
            let input = "- Parent\n  - Child\n    - Grandchild";
            let note = parse_markdown("note.md", input);

            let parent = note
                .lists()
                .first()
                .and_then(|list| list.items().first())
                .expect("parent item");
            assert_eq!(parent.line(), SourceLine::new(1));
            assert_eq!(parent.depth(), 0);
            assert_eq!(parent.parent(), None);

            let child = parent
                .children()
                .first()
                .and_then(|list| list.items().first())
                .expect("child item");
            assert_eq!(child.line(), SourceLine::new(2));
            assert_eq!(child.depth(), 1);
            assert_eq!(child.parent(), Some(SourceLine::new(1)));

            let grandchild = child
                .children()
                .first()
                .and_then(|list| list.items().first())
                .expect("grandchild item");
            assert_eq!(grandchild.line(), SourceLine::new(3));
            assert_eq!(grandchild.depth(), 2);
            assert_eq!(grandchild.parent(), Some(SourceLine::new(2)));
        }

        #[test]
        fn gives_top_level_siblings_distinct_lines_and_no_parent() {
            let input = "- Parent\n  - Child\n- Sibling";
            let note = parse_markdown("note.md", input);

            let list = note.lists().first().expect("list present");
            let sibling = list.items().get(1).expect("sibling item");
            assert_eq!(sibling.line(), SourceLine::new(3));
            assert_eq!(sibling.depth(), 0);
            assert_eq!(sibling.parent(), None);
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

        #[test]
        fn parses_code_block_without_leaking_content_as_metadata() {
            // Arrange — fenced code block contains YAML-like content that could
            // be mistaken for frontmatter if block context doesn't switch.
            let input = "---\ntitle: Real Frontmatter\n---\n\nSome \
                         text.\n\n```\n---\nfake: value\n```\n\nMore text.";
            let note = parse_markdown("note.md", input);

            // Act — the real frontmatter has 1 field; the fenced content must
            // not appear as additional fields.
            let field_count =
                note.frontmatter().map_or(0, |fm| fm.fields().len());

            // Assert
            assert_eq!(
                field_count, 1,
                "code block content must not leak into frontmatter"
            );
        }

        #[test]
        fn treats_text_after_closing_fence_as_body() {
            // Arrange — text after a fenced code block must be treated as body
            // text, not as code block content (end_code_block
            // resets BlockContext). Inline fields are extracted from body text,
            // so verifying they appear after a code block proves the context
            // reset worked.
            let input = "```\ncode here\n```\n\nStatus:: Draft";
            let note = parse_markdown("note.md", input);

            // Act — the parser extracts inline fields from body text
            let field_count = note.inline_fields().len();

            // Assert
            assert_eq!(
                field_count, 1,
                "inline field after closing fence must be extracted"
            );
        }

        #[test]
        fn preserves_body_through_nested_list_text_blocks() {
            // Arrange — a paragraph followed by a nested list with text,
            // followed by another paragraph. The
            // body_buffer.clear() in start_text_block must NOT fire
            // for nested text blocks (L215 mutant inverts the guard).
            // We verify indirectly: inline fields from both paragraphs must be
            // extracted, proving both paragraphs were processed.
            let input = "Status:: Draft\n\n- Item one.\n- Nested \
                         item.\n\nAuthor:: Jane";
            let note = parse_markdown("note.md", input);

            // Act
            let keys: Vec<&str> =
                note.inline_fields().iter().map(|(k, _)| k.name()).collect();

            // Assert — both paragraph fields must be extracted
            assert!(
                keys.contains(&"Status"),
                "first paragraph field must be extracted, got: {keys:?}"
            );
            assert!(
                keys.contains(&"Author"),
                "second paragraph field must be extracted, got: {keys:?}"
            );
        }

        #[test]
        fn emits_breaks_in_body_text() {
            // Arrange — hard breaks (two trailing spaces) in body text must
            // produce newlines in the body buffer (L317 mutant removes the
            // push). We verify indirectly: a field value must be
            // truncated at the newline, not span across the break.
            let input = "Key:: value1  \nmore text";
            let note = parse_markdown("note.md", input);

            // Act
            let (_key, values) =
                note.inline_fields().iter().next().expect("field present");

            // Assert — field value must stop at the hard break
            assert_eq!(
                values.first().and_then(|v| v.as_str()),
                Some("value1"),
                "hard breaks must appear as newlines in body, got: {:?}",
                values.first()
            );
        }

        #[test]
        fn preserves_inline_code_in_list_item_text() {
            // Arrange — inline code inside a list item must appear in the
            // item's display text (text_buffer) but NOT in the scan
            // buffer (for field/tag scanning). The push_code method
            // writes only to text_buffer.
            let input = "- Item with `inline code` here\n";
            let note = parse_markdown("note.md", input);

            // Act
            let lists = note.lists();
            let item_text = lists
                .first()
                .and_then(|l| l.items().first())
                .map(crate::note::lists::ListItem::text)
                .unwrap_or_default();

            // Assert
            assert!(
                item_text.contains("inline code"),
                "inline code must appear in list item text, got: {item_text:?}"
            );
        }
    }

    mod list_tracker {
        use super::*;

        #[test]
        fn is_item_active_returns_true_when_stack_nonempty() {
            let mut tracker = ListTracker::default();
            assert!(!tracker.is_item_active());

            tracker.start_list(false);
            tracker.start_item(SourceLine::new(1));

            assert!(
                tracker.is_item_active(),
                "is_item_active must return true after start_item"
            );
        }

        #[test]
        fn inline_code_pushes_to_last_item_not_first() {
            let mut tracker = ListTracker::default();
            tracker.start_list(false);
            tracker.start_item(SourceLine::new(1));
            tracker.push_text("before ", false);
            tracker.start_item(SourceLine::new(2));

            tracker.inline_code("code");

            tracker.end_item(&TaskStatusMap::default());
            tracker.end_item(&TaskStatusMap::default());
            tracker.end_list();

            // end_item pops LIFO: item2 is index 0, item1 is index 1
            let list = tracker.lists.first().expect("list must exist");
            let items = list.items();
            let item1_text = items.get(1).expect("item 1 must exist").text();
            let item2_text = items.first().expect("item 2 must exist").text();
            assert!(
                !item1_text.contains("code"),
                "inline code must not leak to item 1, got: {item1_text:?}"
            );
            assert!(
                item2_text.contains("code"),
                "inline code must be in item 2, got: {item2_text:?}"
            );
        }

        #[test]
        fn push_scan_char_returns_false_when_no_item_active() {
            let mut tracker = ListTracker::default();
            let result = tracker.push_scan_char('a');
            assert!(
                !result,
                "push_scan_char must return false when no item is active"
            );
        }

        #[test]
        fn start_nested_text_block_returns_false_when_no_item() {
            let mut tracker = ListTracker::default();
            assert!(
                !tracker.start_nested_text_block(),
                "start_nested_text_block must return false with no item"
            );
        }

        #[test]
        fn start_nested_text_block_returns_true_with_active_item() {
            let mut tracker = ListTracker::default();
            tracker.start_list(false);
            tracker.start_item(SourceLine::new(1));
            assert!(
                tracker.start_nested_text_block(),
                "start_nested_text_block must return true with active item"
            );
        }

        #[test]
        fn start_list_flushes_active_item_scan_buffer() {
            let mut tracker = ListTracker::default();
            tracker.start_list(false);
            tracker.start_item(SourceLine::new(1));
            tracker.push_text("Status:: Draft", false);

            let flushed = tracker.start_list(false);

            assert!(
                flushed.is_some(),
                "start_list must flush active item scan buffer"
            );
            let (fields, _) = flushed.unwrap();
            let has_status =
                fields.keys().any(|k| k.is_canonical_match("status"));
            assert!(has_status, "flushed fields must contain Status");
        }

        #[test]
        fn end_item_flushes_scan_buffer() {
            let mut tracker = ListTracker::default();
            tracker.start_list(false);
            tracker.start_item(SourceLine::new(1));
            tracker.push_text("Author:: Jane", false);

            let flushed = tracker.end_item(&TaskStatusMap::default());

            assert!(flushed.is_some(), "end_item must flush scan buffer");
            let (fields, _) = flushed.unwrap();
            let has_author =
                fields.keys().any(|k| k.is_canonical_match("author"));
            assert!(has_author, "flushed fields must contain Author");
        }

        #[test]
        fn push_text_and_push_break_return_false_when_no_item_active() {
            let mut tracker = ListTracker::default();
            assert!(
                !tracker.push_text("hello", false),
                "push_text must return false when no item active"
            );
            assert!(
                !tracker.push_break(),
                "push_break must return false when no item active"
            );
        }
    }

    mod byte_tracker {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn resolves_any_offset_in_single_line_source_to_line_one() {
            let tracker = ByteTracker::new("no newlines here");

            assert_eq!(
                tracker.byte_to_line(ByteOffset::new(0)),
                SourceLine::new(1)
            );
            assert_eq!(
                tracker.byte_to_line(ByteOffset::new(10)),
                SourceLine::new(1)
            );
        }

        #[test]
        fn resolves_offsets_within_each_line_of_multi_line_source() {
            let tracker = ByteTracker::new("one\ntwo\nthree");

            assert_eq!(
                tracker.byte_to_line(ByteOffset::new(0)),
                SourceLine::new(1),
                "start of line 1"
            );
            assert_eq!(
                tracker.byte_to_line(ByteOffset::new(2)),
                SourceLine::new(1),
                "mid line 1"
            );
            assert_eq!(
                tracker.byte_to_line(ByteOffset::new(4)),
                SourceLine::new(2),
                "start of line 2"
            );
            assert_eq!(
                tracker.byte_to_line(ByteOffset::new(8)),
                SourceLine::new(3),
                "start of line 3"
            );
            assert_eq!(
                tracker.byte_to_line(ByteOffset::new(12)),
                SourceLine::new(3),
                "last byte of line 3"
            );
        }

        #[test]
        fn counts_empty_lines_as_separate_lines() {
            let tracker = ByteTracker::new("one\n\nthree");

            assert_eq!(
                tracker.byte_to_line(ByteOffset::new(4)),
                SourceLine::new(2),
                "the empty line"
            );
            assert_eq!(
                tracker.byte_to_line(ByteOffset::new(5)),
                SourceLine::new(3)
            );
        }

        #[test]
        fn resolves_empty_source_to_line_one() {
            let tracker = ByteTracker::new("");

            assert_eq!(
                tracker.byte_to_line(ByteOffset::new(0)),
                SourceLine::new(1)
            );
        }

        #[test]
        fn resolves_an_offset_beyond_source_length_to_the_last_line() {
            let tracker = ByteTracker::new("one\ntwo\nthree");

            assert_eq!(
                tracker.byte_to_line(ByteOffset::new(1000)),
                SourceLine::new(3)
            );
        }
    }

    mod tasks {
        use pretty_assertions::assert_eq;
        use rstest::rstest;

        use super::*;
        use crate::task::TaskStatusType;

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
        #[expect(clippy::panic, reason = "test assertion on enum variant")]
        fn iterates_nested_sub_list_task_items() {
            let input = "- Plain parent\n  - [x] Subtask 1";
            let note = parse_markdown("note.md", input);

            let tasks: Vec<&ListItem> = note.tasks().collect();
            assert_eq!(tasks.len(), 1);
            assert_eq!(tasks.first().map(|t| t.text()), Some("Subtask 1"));
            let ListItemType::Task(status) = tasks.first().unwrap().item_type()
            else {
                panic!("subtask must be a Task");
            };
            assert_eq!(status.kind().completed(), Some(true));
        }

        #[rstest]
        #[case::space_todo(' ', TaskStatusType::Todo)]
        #[case::checked_lowercase('x', TaskStatusType::Done)]
        #[case::checked_uppercase('X', TaskStatusType::Done)]
        #[case::in_progress('/', TaskStatusType::InProgress)]
        #[case::cancelled('-', TaskStatusType::Cancelled)]
        #[case::on_hold('!', TaskStatusType::OnHold)]
        fn classifies_every_default_marker_as_a_task(
            #[case] symbol: char,
            #[case] expected_kind: TaskStatusType,
        ) {
            let input = format!("- [{symbol}] Task text");
            let note = parse_markdown("note.md", &input);

            let tasks: Vec<&ListItem> = note.tasks().collect();
            assert_eq!(tasks.len(), 1, "marker {symbol:?} must become a Task");
            assert_eq!(tasks.first().map(|t| t.text()), Some("Task text"));

            let item = tasks.first().expect("task present");
            assert!(
                matches!(item.item_type(), ListItemType::Task(status) if
                    status.kind() == expected_kind),
                "marker {symbol:?} must resolve to {expected_kind:?}, got {:?}",
                item.item_type()
            );
        }

        #[test]
        #[expect(clippy::panic, reason = "test assertion on enum variant")]
        fn preserves_and_classifies_an_unknown_marker_as_an_incomplete_task() {
            let note = parse_markdown("note.md", "- [?] Mystery task");

            let list = note.lists().first().expect("list present");
            let item = list.items().first().expect("item present");
            assert_eq!(item.text(), "Mystery task");
            let ListItemType::Task(status) = item.item_type() else {
                panic!(
                    "unknown marker must never be downgraded to a plain \
                     bullet, got {:?}",
                    item.item_type()
                );
            };
            assert_eq!(
                status.kind().completed(),
                Some(false),
                "unknown markers resolve as incomplete todos"
            );
        }

        #[test]
        fn does_not_treat_bracket_text_in_the_item_body_as_a_marker() {
            let note = parse_markdown("note.md", "- Check [x] later");

            let list = note.lists().first().expect("list present");
            let item = list.items().first().expect("item present");
            assert_eq!(item.text(), "Check [x] later");
            assert_eq!(item.item_type(), &ListItemType::Plain);
            assert_eq!(note.tasks().count(), 0);
        }

        #[test]
        fn classifies_a_bare_marker_with_no_trailing_text_as_a_task() {
            // `- [x]` as an entire item: the line terminator supplies the
            // marker's trailing whitespace, matching pulldown-cmark's
            // ENABLE_TASKLISTS behavior.
            let note = parse_markdown("note.md", "- [x]");

            let list = note.lists().first().expect("list present");
            let item = list.items().first().expect("item present");
            assert_eq!(item.text(), "");
            assert_eq!(note.tasks().count(), 1);
        }

        #[test]
        fn resolves_a_pending_marker_before_a_nested_list_flush() {
            // `- [x]` + nested list: no whitespace text chunk arrives before
            // the child list starts, but the parent still carries a marker.
            let note = parse_markdown("note.md", "- [x]\n  - sub");

            let list = note.lists().first().expect("list present");
            let item = list.items().first().expect("item present");
            assert_eq!(item.text(), "");
            assert_eq!(note.tasks().count(), 1);
        }

        #[test]
        fn classifies_a_marker_before_a_soft_break_as_a_task() {
            let note = parse_markdown("note.md", "- [x]\n  continued");

            let tasks: Vec<&ListItem> = note.tasks().collect();
            assert_eq!(tasks.len(), 1);
            assert_eq!(tasks.first().map(|t| t.text()), Some("\ncontinued"));
        }

        #[test]
        fn keeps_an_item_starting_with_inline_markup_plain() {
            // The emphasis opens the item's content, so `[x]` is not at the
            // item-leading position and must not become a marker.
            let note = parse_markdown("note.md", "- **[x] Task**");

            let list = note.lists().first().expect("list present");
            let item = list.items().first().expect("item present");
            assert_eq!(item.text(), "[x] Task");
            assert_eq!(item.item_type(), &ListItemType::Plain);
            assert_eq!(note.tasks().count(), 0);
        }

        #[test]
        fn keeps_an_item_starting_with_inline_code_plain() {
            let note = parse_markdown("note.md", "- `[x]` Task");

            let list = note.lists().first().expect("list present");
            let item = list.items().first().expect("item present");
            assert_eq!(item.text(), "[x] Task");
            assert_eq!(item.item_type(), &ListItemType::Plain);
            assert_eq!(note.tasks().count(), 0);
        }

        #[test]
        fn keeps_a_link_lookalike_plain() {
            // `- [x](y)` is a link whose text abuts the closing bracket —
            // no whitespace after `]`, so no marker.
            let note = parse_markdown("note.md", "- [x](y) z");

            let list = note.lists().first().expect("list present");
            let item = list.items().first().expect("item present");
            assert_eq!(item.text(), "x z");
            assert_eq!(item.item_type(), &ListItemType::Plain);
            assert_eq!(note.tasks().count(), 0);
        }

        #[test]
        fn rejects_unicode_whitespace_after_the_marker() {
            // NBSP is ordinary text in Markdown, not the marker's trailing
            // whitespace (ASCII whitespace only, mirroring pulldown-cmark).
            let note = parse_markdown("note.md", "- [x]\u{00A0}Task");

            let list = note.lists().first().expect("list present");
            let item = list.items().first().expect("item present");
            assert_eq!(item.text(), "[x]\u{00A0}Task");
            assert_eq!(item.item_type(), &ListItemType::Plain);
            assert_eq!(note.tasks().count(), 0);
        }

        #[test]
        fn classifies_a_multibyte_symbol_marker_as_an_incomplete_task() {
            let note = parse_markdown("note.md", "- [β] Task");

            let tasks: Vec<&ListItem> = note.tasks().collect();
            assert_eq!(tasks.len(), 1);
            assert_eq!(tasks.first().map(|t| t.text()), Some("Task"));
            let item = tasks.first().expect("task present");
            assert!(matches!(
                item.item_type(),
                ListItemType::Task(status) if status.kind().completed() == Some(false)
            ));
        }
    }

    mod inline_metadata {
        use pretty_assertions::assert_eq;
        use rstest::rstest;

        use super::*;
        use crate::tag::Tag;

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
                .map(crate::FieldKey::name)
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

            assert_eq!(note.tags(), [Tag::parse("#book").unwrap()]);
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
            assert_eq!(note.tags(), [Tag::parse("#book").unwrap()]);
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

            assert_eq!(note.tags(), [
                Tag::parse("#alpha").unwrap(),
                Tag::parse("#beta").unwrap()
            ]);
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

            assert_eq!(note.tags(), [Tag::parse("#urgent").unwrap()]);
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

            assert_eq!(note.tags(), [Tag::parse("#book").unwrap()]);
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
