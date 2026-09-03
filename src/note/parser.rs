//! Markdown event parser for [`Note`] records.
//!
//! [`parse_markdown`] walks a `pulldown-cmark` event stream once, building a
//! [`Note`] from frontmatter, lists, outlinks, inline fields, and tags.
//!
//! # Architecture
//!
//! The parser is organized into six specialized submodules:
//!
//! - [`inline`]: [`inline::parse_inline_value`] parses raw inline field value
//!   text into strongly typed [`NoteFieldValue`] records (comma lists, quoted
//!   strings, durations, wikilinks, booleans, dates, numbers, tags).
//! - [`input`]: [`MarkdownParserInput`] encapsulates borrowed path, source
//!   text, and configuration references for parsing.
//! - [`lexer`]: [`InlineTokenLexer`] extracts `Key:: Value`, `[Key:: Value]`,
//!   and `(Key:: Value)` inline fields, task emoji shorthands, and `#tag`
//!   tokens from plain-text scan buffers using [`logos`].
//! - [`mod@line`]: [`ByteTracker`] precomputes line-start byte offsets for
//!   $O(\log n)$ byte-to-line translation without scanning the source string
//!   multiple times.
//! - [`list`]: [`ListTracker`] manages explicit list and list-item stacks so
//!   nested Markdown never recurses through the call stack, driving the
//!   item-leading marker state machine, tag filter classification, and flushing
//!   item metadata.
//! - [`marker`]: custom task marker scanner that recognizes `[<symbol>]`
//!   markers at item-leading positions with pulldown-cmark-compatible
//!   whitespace rules.
//!
//! Parser state lives in [`ParserContext`], which dispatches events to
//! specialized handlers and assembles the final [`Note`].
//!
//! # Metadata Extraction
//!
//! Inline fields and tags are lexed from parser-built plain-text buffers: one
//! per top-level paragraph or heading, and one per list item. The buffers
//! exclude fenced code blocks, indented code blocks, and inline code.
//!
//! Standard Markdown link text is copied into the surrounding scan buffer
//! wrapped in literal `[` and `]` delimiters, so `[Key:: Value](url)` becomes a
//! visible-key inline field while [`ListItem::text`](super::ListItem::text)
//! retains the plain display text.
use std::{mem, path::PathBuf};

use indexmap::IndexMap;
use pulldown_cmark::{
    CowStr, Event, LinkType as CmarkLinkType, Options, Parser, Tag as CmarkTag,
    TagEnd,
};

use super::{
    Frontmatter, Link, LinkType, Note, NoteFieldValue, RawFrontmatter,
};
use crate::{ByteOffset, FieldKey, Tag, TaskStatusMap};

mod inline;
mod input;
mod lexer;
mod line;
mod list;
mod marker;

pub use input::MarkdownParserInput;
use lexer::InlineTokenLexer;
use line::ByteTracker;
use list::ListTracker;

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
/// use std::path::Path;
///
/// use traces_pkm::{MarkdownParserInput, parse_markdown};
///
/// let input = MarkdownParserInput::for_test(
///     Path::new("note.md"),
///     "# Hello\nStatus:: Draft",
/// );
/// let note = parse_markdown(&input);
/// assert!(note.outlinks().is_empty());
/// assert_eq!(note.tags().len(), 0);
/// # }
/// ```
#[inline]
#[must_use]
pub fn parse_markdown(input: &MarkdownParserInput<'_>) -> Note {
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_YAML_STYLE_METADATA_BLOCKS);
    opts.insert(Options::ENABLE_WIKILINKS);

    let mut ctx = ParserContext::new(
        input.src(),
        input.tasks().statuses(),
        input.tasks().tag_filters(),
    );
    for (event, range) in Parser::new_ext(input.src(), opts).into_offset_iter()
    {
        ctx.handle_event(event, ByteOffset::from(range.start));
    }
    ctx.into_note(input.path())
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
    Option<(IndexMap<FieldKey, Vec<NoteFieldValue>>, Vec<Tag>)>;

/// State accumulated while walking Markdown events for one note.
struct ParserContext<'a> {
    frontmatter: Option<Frontmatter>,
    block: BlockContext,
    metadata_buffer: String,
    outlinks: Vec<Link>,
    /// The link currently being walked, if any.
    active_link: Option<ActiveLink>,
    list_nesting: ListTracker,
    body_buffer: String,
    inline_fields: IndexMap<FieldKey, Vec<NoteFieldValue>>,
    tags: Vec<Tag>,
    /// Precomputed line-start offsets for the source being parsed, used to
    /// populate [`ListItem`](super::ListItem)'s `line`/`parent` position
    /// fields.
    line_tracker: ByteTracker,
    /// Resolves scanned marker symbols to their [`TaskStatus`], used to
    /// classify status-marked list items in
    /// [`list::ListTracker::end_item`](self::list::ListTracker::end_item).
    ///
    /// [`TaskStatus`]: crate::TaskStatus
    task_statuses: &'a TaskStatusMap,
    /// Tag filters that classify status-marked items as Tasks vs Checkboxes.
    tag_filters: &'a [Tag],
}

impl<'a> ParserContext<'a> {
    /// Starts a new context for `source`, precomputing its line-start
    /// offsets.
    #[inline]
    #[must_use]
    fn new(
        source: &str,
        task_statuses: &'a TaskStatusMap,
        tag_filters: &'a [Tag],
    ) -> Self {
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
            task_statuses,
            tag_filters,
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
                self.list_nesting.reject_marker();
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
                self.list_nesting.reject_marker();
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
            // marker is not at the content start, mirroring pulldown-cmark,
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
                self.list_nesting.reject_marker();
            }
            // Any other event ends the item's first line structurally (a nested
            // list, a loose-item paragraph, the item's end), which counts as
            // the marker's trailing whitespace.
            _ => self.list_nesting.resolve_pending_marker(),
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
    /// [`ListItem::children`](super::ListItem::children). Otherwise, it becomes
    /// a top-level [`Note::lists`] entry.
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
        let flushed =
            self.list_nesting.end_item(self.tag_filters, self.task_statuses);
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

#[cfg(test)]
mod tests {
    use super::{
        super::{ListItem, ListItemType, NoteFieldValue},
        *,
    };
    use crate::SourceLine;

    fn parse(src: &str) -> Note {
        let input =
            MarkdownParserInput::for_test(std::path::Path::new("note.md"), src);
        parse_markdown(&input)
    }

    fn parse_with_tasks(src: &str, tasks: &crate::TaskConfig) -> Note {
        let frontmatter = crate::config::FrontmatterConfig::default();
        let input = MarkdownParserInput::new(
            std::path::Path::new("note.md"),
            src,
            tasks,
            &frontmatter,
        );
        parse_markdown(&input)
    }
    mod parse {
        use pretty_assertions::assert_eq;
        use rstest::rstest;

        use super::*;

        #[test]
        fn returns_empty_note_when_source_is_empty() {
            let input = "";
            let note = parse(input);

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
            let note = parse(input);

            assert_eq!(note.frontmatter(), None);
        }

        #[test]
        fn extracts_yaml_frontmatter_block_fields() {
            let input = "---\ntitle: My Note\ntags: [rust, pkm]\n---\n# Header";
            let note = parse(input);

            assert_eq!(note.frontmatter().map(|fm| fm.fields().len()), Some(2));
            assert_eq!(
                note.frontmatter().map(Frontmatter::is_empty),
                Some(false)
            );
        }

        #[test]
        fn returns_none_for_frontmatter_when_yaml_block_is_empty() {
            let input = "---\n---\n# Header";
            let note = parse(input);

            assert_eq!(note.frontmatter(), None);
        }

        #[test]
        fn returns_empty_frontmatter_when_yaml_block_is_malformed() {
            let input = "---\ninvalid: [yaml: :\n---\n# Header";
            let note = parse(input);

            assert_eq!(
                note.frontmatter().map(Frontmatter::is_empty),
                Some(true)
            );
        }

        #[test]
        fn extracts_structured_fields_from_yaml_frontmatter() {
            let input = "---\ntitle: Note Title\nauthor: Alice\ndraft: \
                         true\nrating: 5.0\ndate: 2026-07-29\n---\nBody text.";
            let note = parse(input);

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
            let note = parse(
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
            let note = parse(input);

            let link = note.outlinks().first().expect("outlink present");
            assert_eq!(link.target(), expected_target);
            assert_eq!(link.text(), expected_text);
            assert_eq!(link.kind(), expected_kind);
        }

        #[test]
        #[expect(clippy::panic, reason = "test assertion on enum variant")]
        fn extracts_task_item_completion_status() {
            let input = "- [ ] Incomplete task\n- [x] Completed task";
            let note = parse(input);

            let list = note.lists().first().expect("list present");
            let item0 = list.items().first().expect("item 0");
            let item1 = list.items().get(1).expect("item 1");

            assert_eq!(item0.text(), "Incomplete task");
            let ListItemType::Task(task0) = item0.kind() else {
                panic!("item0 must be a Task, got {:?}", item0.kind());
            };
            assert_eq!(task0.status().kind().completed(), Some(false));

            assert_eq!(item1.text(), "Completed task");
            let ListItemType::Task(task1) = item1.kind() else {
                panic!("item1 must be a Task, got {:?}", item1.kind());
            };
            assert_eq!(task1.status().kind().completed(), Some(true));
        }

        #[test]
        fn includes_link_display_text_in_the_containing_item_text() {
            let input = "- [ ] Check [link text](https://example.com) here";
            let note = parse(input);

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
            let note = parse(input);

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
            let note = parse(input);

            let grandchild = note
                .lists()
                .first()
                .and_then(|list| list.items().first())
                .and_then(|item| item.children().first())
                .and_then(|list| list.items().first())
                .and_then(|item| item.children().first())
                .and_then(|list| list.items().first())
                .map(ListItem::raw_text);
            assert_eq!(grandchild, Some("Grandchild"));
        }

        #[test]
        fn populates_depth_line_and_parent_down_the_nesting_chain() {
            let input = "- Parent\n  - Child\n    - Grandchild";
            let note = parse(input);

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
            let note = parse(input);

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
            let note = parse(input);

            let list = note.lists().first().expect("list present");
            assert_eq!(list.is_ordered(), expected_ordered);
            assert_eq!(list.items().len(), 2);
        }

        #[test]
        fn preserves_soft_breaks_inside_list_item_text() {
            let note = parse("- Wrapped\n  line");

            let text = note
                .lists()
                .first()
                .and_then(|list| list.items().first())
                .map(ListItem::raw_text);
            assert_eq!(text, Some("Wrapped\nline"));
        }

        #[test]
        fn parses_code_block_without_leaking_content_as_metadata() {
            // Arrange — fenced code block contains YAML-like content that could
            // be mistaken for frontmatter if block context doesn't switch.
            let input = "---\ntitle: Real Frontmatter\n---\n\nSome \
                         text.\n\n```\n---\nfake: value\n```\n\nMore text.";
            let note = parse(input);

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
            let note = parse(input);

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
            let note = parse(input);

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
            let note = parse(input);

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
            let note = parse(input);

            // Act
            let lists = note.lists();
            let item_text = lists
                .first()
                .and_then(|l| l.items().first())
                .map(ListItem::raw_text)
                .unwrap_or_default();

            // Assert
            assert!(
                item_text.contains("inline code"),
                "inline code must appear in list item text, got: {item_text:?}"
            );
        }
    }

    mod inline_metadata {
        use pretty_assertions::assert_eq;
        use rstest::rstest;

        use super::*;
        use crate::Tag;

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
            let note = parse(input);

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
            let note = parse("Status:: Draft\n\nAuthor:: Jane");
            let keys: Vec<&str> = note
                .inline_fields()
                .keys()
                .map(crate::FieldKey::name)
                .collect();
            assert_eq!(keys, ["Status", "Author"]);
        }

        #[test]
        fn extracts_a_bare_field_from_a_list_item_and_keeps_it_in_item_text() {
            let note = parse("- Status:: Draft");
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
            let note = parse(
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
            let note = parse(
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
            let note = parse("- Plain item with no fields");

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
        #[case::done("- [x] testTask ✅2022-07-26", "done", "2022-07-26")]
        #[case::cancelled(
            "- [x] testTask ❌2022-07-27",
            "cancelled",
            "2022-07-27"
        )]
        fn extracts_task_emoji_shorthand_fields_from_task_items_only(
            #[case] input: &str,
            #[case] expected_key: &str,
            #[case] expected_date: &str,
        ) {
            let note = parse(input);

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
            let note = parse("- [ ] testTask 🗓2022-07-14 ⏳2022-07-24");

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
            let note = parse("- Plain item 🗓2022-07-14");

            assert_eq!(note.inline_fields().len(), 0);
        }
        #[test]
        fn ignores_task_emoji_shorthand_fields_outside_task_items() {
            let note = parse("testTask 🗓2022-07-14");

            assert_eq!(note.inline_fields().len(), 0);
        }

        #[rstest]
        #[case::fenced_code_block("```\nKey:: Value\n```")]
        #[case::indented_code_block("Paragraph text.\n\n    Key:: Value\n")]
        #[case::inline_code_span("Text with `Key:: Value` inline.")]
        fn ignores_fields_inside_excluded_code_regions(#[case] input: &str) {
            let note = parse(input);

            assert_eq!(note.inline_fields().len(), 0);
        }

        #[test]
        fn extracts_a_tag_from_body_text() {
            let note = parse("Filed under #book today.");

            assert_eq!(note.tags(), [Tag::parse("#book").unwrap()]);
        }

        #[test]
        fn extracts_a_tag_from_a_list_item_and_keeps_it_in_item_text() {
            let note = parse("- Reading #book now");

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
            let note = parse(input);

            assert_eq!(note.tags().len(), 0);
        }

        #[test]
        fn extracts_a_bare_field_from_a_second_paragraph_within_a_loose_list_item()
         {
            let note = parse("- Task line\n\n  Status:: Draft\n");
            assert_eq!(note.inline_fields().len(), 1);
            let (key, values) =
                note.inline_fields().iter().next().expect("field present");
            assert!(key.is_canonical_match("status"));
            assert_eq!(values.first().and_then(|v| v.as_str()), Some("Draft"));
        }

        #[test]
        fn orders_parent_item_fields_before_nested_child_item_fields() {
            let note = parse("- Status:: Draft\n  - Priority:: High\n");

            let keys: Vec<&str> =
                note.inline_fields().iter().map(|(k, _)| k.name()).collect();
            assert_eq!(keys, ["Status", "Priority"]);
        }

        #[test]
        fn orders_parent_item_fields_before_and_after_nested_child_fields() {
            let note = parse(
                "- Status:: Draft\n  - Priority:: High\n\n  Reviewer:: Jane\n",
            );

            let keys: Vec<&str> =
                note.inline_fields().iter().map(|(k, _)| k.name()).collect();
            assert_eq!(keys, ["Status", "Priority", "Reviewer"]);
        }

        #[test]
        fn isolates_parent_and_child_item_tags_without_leaking_between_levels()
        {
            let note = parse("- Parent #alpha\n  - Child #beta\n");
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
            let note = parse(input);

            assert_eq!(note.inline_fields().len(), 0);
        }

        #[rstest]
        #[case::fenced_code_block("- Item text\n\n  ```\n  #book\n  ```\n")]
        #[case::indented_code_block("- Item text\n\n      #book\n")]
        #[case::inline_code_span("- Text with `#book` inline")]
        fn ignores_tags_inside_excluded_code_regions_within_a_list_item(
            #[case] input: &str,
        ) {
            let note = parse(input);

            assert_eq!(note.tags().len(), 0);
        }

        #[test]
        fn extracts_both_a_field_and_a_tag_from_the_same_list_item_text() {
            let note = parse("- Status:: Draft #urgent");

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
            let note = parse("Status:: Draft\n\n- Reviewer:: Jane");

            let keys: Vec<&str> =
                note.inline_fields().iter().map(|(k, _)| k.name()).collect();
            assert_eq!(keys, ["Status", "Reviewer"]);
        }

        #[test]
        fn preserves_document_order_between_a_list_item_field_and_a_body_field()
        {
            let note = parse("- Reviewer:: Jane\n\nStatus:: Draft");

            let keys: Vec<&str> =
                note.inline_fields().iter().map(|(k, _)| k.name()).collect();
            assert_eq!(keys, ["Reviewer", "Status"]);
        }

        #[test]
        fn keeps_a_field_value_intact_when_it_directly_abuts_excluded_inline_code()
         {
            let note = parse("Status:: Draft`note` more text");

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
            let note = parse("# Chapter #book\n\nBody.");

            assert_eq!(note.tags(), [Tag::parse("#book").unwrap()]);
        }

        #[test]
        fn extracts_a_bare_field_from_heading_text() {
            let note = parse("# Status:: Draft");

            assert_eq!(note.inline_fields().len(), 1);
            let (key, values) =
                note.inline_fields().iter().next().expect("field present");
            assert!(key.is_canonical_match("status"));
            assert_eq!(values.first().and_then(|v| v.as_str()), Some("Draft"));
        }

        #[test]
        fn extracts_a_visible_key_field_from_a_markdown_links_display_text() {
            let note = parse("[Status:: Draft](http://example.com)");

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
            let note = parse("See [Status:: Draft](http://example.com) here.");

            assert_eq!(note.inline_fields().len(), 1);
            let (key, values) =
                note.inline_fields().iter().next().expect("field present");
            assert!(key.is_canonical_match("status"));
            assert_eq!(values.first().and_then(|v| v.as_str()), Some("Draft"));
        }
    }

    mod tag_filters {
        use pretty_assertions::assert_eq;

        use super::*;
        use crate::TaskConfig;

        #[test]
        fn classifies_matching_items_as_tasks_and_non_matching_as_checkboxes() {
            let tasks =
                TaskConfig::for_test(vec![Tag::parse("#task").unwrap()]);
            let input = "- [ ] Marked matching #task\n- [x] Marked \
                         non-matching #other\n- [ ] Marked without tags\n- \
                         Plain with #task";
            let note = parse_with_tasks(input, &tasks);

            let tasks_collected: Vec<&ListItem> = note.tasks().collect();
            assert_eq!(tasks_collected.len(), 1);
            assert_eq!(
                tasks_collected.first().copied().map(ListItem::raw_text),
                Some("Marked matching #task")
            );

            let list = note.lists().first().expect("list present");
            let items = list.items();
            assert_eq!(items.len(), 4);
            assert!(matches!(
                items.first().expect("item 0").kind(),
                ListItemType::Task(_)
            ));
            assert_eq!(
                items.get(1).expect("item 1").kind(),
                &ListItemType::Checkbox
            );
            assert_eq!(
                items.get(2).expect("item 2").kind(),
                &ListItemType::Checkbox
            );
            assert_eq!(
                items.get(3).expect("item 3").kind(),
                &ListItemType::Plain
            );
        }

        #[test]
        fn classifies_all_marked_items_as_tasks_when_filters_empty() {
            let tasks = TaskConfig::default();
            let input =
                "- [ ] Todo without tags\n- [x] Done with #other\n- Plain item";
            let note = parse_with_tasks(input, &tasks);

            assert_eq!(note.tasks().count(), 2);

            let list = note.lists().first().expect("list present");
            let items = list.items();
            assert_eq!(items.len(), 3);
            assert!(matches!(
                items.first().expect("item 0").kind(),
                ListItemType::Task(_)
            ));
            assert!(matches!(
                items.get(1).expect("item 1").kind(),
                ListItemType::Task(_)
            ));
            assert_eq!(
                items.get(2).expect("item 2").kind(),
                &ListItemType::Plain
            );
        }

        #[test]
        fn enforces_exact_tag_matching_for_nested_tags() {
            let tasks =
                TaskConfig::for_test(vec![Tag::parse("#task").unwrap()]);
            let input = "- [ ] Nested tag #task/project\n- [ ] Exact tag #task";
            let note = parse_with_tasks(input, &tasks);

            let tasks_collected: Vec<&ListItem> = note.tasks().collect();
            assert_eq!(tasks_collected.len(), 1);
            assert_eq!(
                tasks_collected.first().copied().map(ListItem::raw_text),
                Some("Exact tag #task")
            );

            let list = note.lists().first().expect("list present");
            let items = list.items();
            assert_eq!(
                items.first().expect("item 0").kind(),
                &ListItemType::Checkbox
            );
            assert!(matches!(
                items.get(1).expect("item 1").kind(),
                ListItemType::Task(_)
            ));
        }
    }
}
