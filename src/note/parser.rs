//! Markdown event parser for [`Note`] records.
//!
//! [`parse_markdown`] walks a `pulldown-cmark` event stream in a single pass,
//! assembling a [`Note`] from frontmatter, lists, outlinks, inline fields, and
//! tags.
//!
//! # Architecture
//!
//! The parser is organized into six specialized submodules:
//!
//! - [`inline`]: parses inline field value strings into typed
//!   [`NoteFieldValue`] records.
//! - [`input`]: provides borrowed input and configuration containers via
//!   [`MarkdownParserInput`].
//! - [`lexer`]: tokenizes inline fields, task emoji shorthands, priority
//!   symbols, and tags.
//! - [`list`]: tracks list hierarchy and manages item classification via
//!   [`ListTracker`].
//! - [`marker`]: scans and classifies item-leading task markers.
//! - [`task`]: extracts task dates, priorities, and normalized text from item
//!   tokens.
//! # Metadata Extraction
//!
//! Inline fields and tags are lexed from text buffers for paragraphs, headings,
//! and list items. Code blocks and inline code spans are excluded from metadata
//! scanning. Standard Markdown links preserve bracketed text in scan buffers so
//! visible-key inline fields can be detected within link text.
//!
//! # Examples
//!
//! ```rust
//! # #[cfg(feature = "test-utils")]
//! # {
//! use std::path::Path;
//!
//! use traces_pkm::{MarkdownParserInput, parse_markdown};
//!
//! let path = Path::new("daily.md");
//! let input = MarkdownParserInput::for_test(path, "# Notes\n- [ ] Task item");
//! let note = parse_markdown(&input);
//! assert_eq!(note.path(), path);
//! # }
//! ```
use std::mem;

use indexmap::IndexMap;
use pulldown_cmark::{
    CowStr, Event, LinkType as CmarkLinkType, Options, Parser, Tag as CmarkTag,
    TagEnd,
};

use super::{
    Frontmatter, Link, LinkType, Note, NoteFieldValue, RawFrontmatter,
};
use crate::{BytePos, ByteSpan, FieldKey, LineIndex, Tag};

mod inline;
mod input;
mod lexer;
mod list;
mod marker;
mod task;

pub use input::MarkdownParserInput;
use list::ListTracker;

/// Options configuring the Markdown parser: YAML frontmatter metadata blocks
/// and Obsidian-style wikilinks.
const MARKDOWN_OPTIONS: Options =
    Options::ENABLE_YAML_STYLE_METADATA_BLOCKS.union(Options::ENABLE_WIKILINKS);

/// Parses Markdown source into a [`Note`].
///
/// Traverses the source text in a single pass, extracting:
/// - YAML frontmatter metadata blocks
/// - Ordered and unordered list items, including task statuses
/// - Outlinks (wikilinks and standard Markdown links)
/// - Dataview-style inline fields (`Key:: Value`, `[Key:: Value]`, `(Key::
///   Value)`)
/// - Hashtags (`#tag`, `#nested/tag`)
///
/// # Examples
///
///
/// ```rust
/// # #[cfg(feature = "test-utils")]
/// # {
/// use std::path::Path;
///
/// use traces_pkm::{MarkdownParserInput, parse_markdown};
///
/// let path = Path::new("example.md");
/// let input = MarkdownParserInput::for_test(path, "# Title\n- [x] Done");
/// let note = parse_markdown(&input);
/// assert_eq!(note.path(), path);
/// # }
/// ```
#[inline]
#[must_use]
pub fn parse_markdown(input: &MarkdownParserInput<'_>) -> Note {
    let mut ctx = ParserContext::new(input);
    for (event, range) in
        Parser::new_ext(input.src(), MARKDOWN_OPTIONS).into_offset_iter()
    {
        handle_event(&mut ctx, event, ByteSpan::from(range));
    }
    ctx.into_note()
}

/// Top-level syntactic block currently being traversed.
///
/// Metadata, code, and text blocks represent mutually exclusive parsing states.
#[derive(Default, Eq, PartialEq)]
enum BlockContext {
    #[default]
    None,
    MetadataBlock,
    CodeBlock,
    Text,
}

/// Flushed inline fields collected from an item buffer.
type FlushedFields = Vec<(FieldKey, NoteFieldValue)>;

/// Intermediate metadata collected from a list item's scan buffer.
///
/// Holds the inline fields and tags extracted from an item before they are
/// folded into the document-level collections.
#[derive(Debug)]
struct FlushedMetadata {
    fields: FlushedFields,
    tags: Vec<Tag>,
}

impl FlushedMetadata {
    /// Creates a flushed metadata record with the given fields and tags.
    #[inline]
    #[must_use]
    const fn new(
        fields: Vec<(FieldKey, NoteFieldValue)>,
        tags: Vec<Tag>,
    ) -> Self {
        Self {
            fields,
            tags,
        }
    }

    /// Returns a slice of the flushed inline fields.
    #[inline]
    #[must_use]
    fn fields(&self) -> &[(FieldKey, NoteFieldValue)] {
        &self.fields
    }

    /// Returns a slice of the flushed tags.
    #[inline]
    #[must_use]
    fn tags(&self) -> &[Tag] {
        &self.tags
    }

    /// Decomposes the record into its inner field and tag collections.
    #[inline]
    #[must_use]
    fn into_parts(self) -> (FlushedFields, Vec<Tag>) {
        (self.fields, self.tags)
    }
}

/// Returns `true` if `event` is an inline markup element that occupies the
/// item's leading slot, preventing a task marker from being recognized.
fn is_inline_marker_barrier(event: &Event<'_>) -> bool {
    matches!(
        event,
        Event::Start(
            CmarkTag::Emphasis
                | CmarkTag::Strong
                | CmarkTag::Strikethrough
                | CmarkTag::Image { .. }
        ) | Event::InlineHtml(_)
            | Event::InlineMath(_)
            | Event::DisplayMath(_)
            | Event::Html(_)
            | Event::FootnoteReference(_)
    )
}

/// Dispatches one Markdown event to the matching handler.
fn handle_event(ctx: &mut ParserContext<'_>, event: Event<'_>, span: ByteSpan) {
    if is_inline_marker_barrier(&event) {
        ctx.list_nesting.reject_marker();
        return;
    }
    match event {
        Event::Start(tag) => handle_start_tag(ctx, tag, span.start()),
        Event::End(tag) => handle_end_tag(ctx, tag),
        Event::Code(text) => ctx.handle_code(&text),
        Event::Text(text) => ctx.push_text(&text),
        Event::SoftBreak | Event::HardBreak => ctx.push_break(),
        _ => ctx.list_nesting.resolve_pending_marker(),
    }
}

/// Dispatches a `Start` tag event to its block handler.
///
/// Any tag without a dedicated arm still ends the item's first line
/// structurally, counting as the marker's trailing whitespace.
fn handle_start_tag(
    ctx: &mut ParserContext<'_>,
    tag: CmarkTag<'_>,
    start: BytePos,
) {
    match tag {
        CmarkTag::MetadataBlock(_) => ctx.start_metadata_block(),
        CmarkTag::Link {
            link_type,
            dest_url,
            ..
        } => {
            ctx.list_nesting.reject_marker();
            ctx.start_link(link_type, dest_url);
        }
        CmarkTag::CodeBlock(_) => ctx.start_code_block(),
        CmarkTag::Paragraph
        | CmarkTag::Heading {
            ..
        } => ctx.start_text_block(),
        CmarkTag::List(start_number) => {
            ctx.start_list(start_number.is_some());
        }
        CmarkTag::Item => ctx.start_item(start),
        CmarkTag::BlockQuote(_) => {
            ctx.list_nesting.resolve_pending_marker();
            ctx.list_nesting.start_nested_block();
        }
        _ => ctx.list_nesting.resolve_pending_marker(),
    }
}

/// Dispatches an `End` tag event to its block handler.
///
/// Any tag without a dedicated arm still ends the item's first line
/// structurally, counting as the marker's trailing whitespace.
fn handle_end_tag(ctx: &mut ParserContext<'_>, tag: TagEnd) {
    match tag {
        TagEnd::MetadataBlock(_) => ctx.end_metadata_block(),
        TagEnd::Link => ctx.end_link(),
        TagEnd::CodeBlock => ctx.end_code_block(),
        TagEnd::Paragraph | TagEnd::Heading(_) => ctx.end_text_block(),
        TagEnd::List(_) => ctx.end_list(),
        TagEnd::Item => ctx.end_item(),
        _ => ctx.list_nesting.resolve_pending_marker(),
    }
}
/// State accumulated while traversing Markdown events for a single note.
struct ParserContext<'a> {
    /// Borrowed parse input providing the source text, path, and task and
    /// frontmatter configuration.
    input: &'a MarkdownParserInput<'a>,
    /// Parsed YAML frontmatter, populated by the document's metadata block.
    frontmatter: Option<Frontmatter>,
    /// Syntactic block currently being traversed.
    block: BlockContext,
    /// Buffered frontmatter YAML text.
    metadata_buffer: String,
    /// Outlinks recorded in document order.
    outlinks: Vec<Link>,
    /// Link currently being traversed and accumulating display text, if any.
    active_link: Option<ActiveLink>,
    /// Nested list and list-item state for the document.
    list_nesting: ListTracker,
    /// Buffer for top-level paragraph and heading text.
    body_buffer: String,
    /// Inline fields keyed by canonical key in first-seen order.
    inline_fields: IndexMap<FieldKey, Vec<NoteFieldValue>>,
    /// Tags scanned from body text in document order.
    tags: Vec<Tag>,
    /// Precomputed line-start positions for the source being parsed, used to
    /// populate the position fields of [`ListItem`](super::ListItem).
    line_index: LineIndex,
}

impl<'a> ParserContext<'a> {
    /// Starts a new context for `input`, precomputing its line-start offsets.
    #[inline]
    #[must_use]
    fn new(input: &'a MarkdownParserInput<'a>) -> Self {
        // Pre-allocate body buffer proportionally to source length, with small
        // initial capacities for sparser metadata collections to reduce
        // reallocations.
        let body_capacity = input.src().len().saturating_mul(3) / 4;
        Self {
            input,
            frontmatter: None,
            block: BlockContext::default(),
            metadata_buffer: String::with_capacity(256),
            outlinks: Vec::with_capacity(8),
            active_link: None,
            list_nesting: ListTracker::default(),
            body_buffer: String::with_capacity(body_capacity),
            inline_fields: IndexMap::with_capacity(8),
            tags: Vec::with_capacity(8),
            line_index: LineIndex::new(input.src()),
        }
    }

    /// Rejects any pending marker, then records code in the active link's
    /// display text and the active item's buffers.
    fn handle_code(&mut self, text: &str) {
        self.list_nesting.reject_marker();
        if let Some(link) = self.active_link.as_mut() {
            link.text.push_str(text);
        }
        self.inline_code(text);
    }

    /// Consumes the accumulated context into a [`Note`].
    ///
    /// Merges frontmatter-sourced tags (keyed by the configured tags name
    /// from [`MarkdownParserInput::frontmatter`]) after body-sourced tags.
    fn into_note(self) -> Note {
        let mut tags = self.tags;
        if let Some(frontmatter) = self.frontmatter.as_ref() {
            tags.extend(frontmatter.tags(self.input.frontmatter().tags_name()));
        }
        Note::new(
            self.input.path(),
            self.frontmatter,
            self.list_nesting.into_lists(),
            self.outlinks,
            self.inline_fields,
            tags,
        )
    }

    /// Enters a frontmatter metadata block and clears the metadata buffer.
    fn start_metadata_block(&mut self) {
        self.block = BlockContext::MetadataBlock;
        self.metadata_buffer.clear();
    }

    /// Leaves a frontmatter metadata block and parses any buffered YAML
    /// frontmatter.
    fn end_metadata_block(&mut self) {
        self.block = BlockContext::None;
        let raw_text = mem::take(&mut self.metadata_buffer);
        let raw = RawFrontmatter::new(raw_text);
        if !raw.is_empty() {
            self.frontmatter = Some(Frontmatter::from(&raw));
        }
    }

    /// Starts tracking an outgoing link.
    ///
    /// Standard Markdown links push `[` into the scan buffer (and `]` in
    /// [`Self::end_link`]) so visible-key inline fields can be detected in link
    /// text.
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

    /// Enters a code block and separates nested blocks in active item buffers.
    fn start_code_block(&mut self) {
        self.list_nesting.start_nested_block();
        self.block = BlockContext::CodeBlock;
    }

    /// Leaves a code block and restores default block context.
    const fn end_code_block(&mut self) {
        self.block = BlockContext::None;
    }

    /// Starts a paragraph or heading text block.
    ///
    /// Top-level text fills `body_buffer`. Text within list items is separated
    /// by newlines in the active item's buffers.
    fn start_text_block(&mut self) {
        self.block = BlockContext::Text;
        if !self.list_nesting.start_nested_block() {
            self.body_buffer.clear();
        }
    }

    /// Lexes a completed top-level text block.
    ///
    /// Nested text blocks are handled through the active list item.
    fn end_text_block(&mut self) {
        self.block = BlockContext::None;
        self.list_nesting.resolve_pending_marker();
        if !self.list_nesting.is_item_active() {
            let flushed = lexer::scan_metadata(
                &self.body_buffer,
                lexer::TaskFieldEmojis::Exclude,
            );
            self.extend_from_flush(Some(flushed));
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
    fn extend_from_flush(&mut self, flushed: Option<FlushedMetadata>) {
        if let Some(metadata) = flushed {
            let (fields, tags) = metadata.into_parts();
            for (key, value) in fields {
                self.inline_fields.entry(key).or_default().push(value);
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
    /// Items of a list nested inside an active item carry that item's line as
    /// their `parent` and a deeper `depth`; a top-level list's items become
    /// [`Note::lists`] entries in document order.
    fn end_list(&mut self) {
        self.list_nesting.end_list();
    }

    /// Computes the item's source line from `pos` and starts tracking it.
    fn start_item(&mut self, pos: BytePos) {
        let line = self.line_index.line_at(pos);
        self.list_nesting.start_item(line);
    }

    /// Flushes and records the innermost list item.
    fn end_item(&mut self) {
        let flushed = self.list_nesting.end_item(
            self.input.tasks().tag_filters(),
            self.input.tasks().statuses(),
        );
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
        if let Some(link) = self.active_link.as_mut() {
            link.text.push('\n');
        }
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
    /// Creates an active link with an empty text buffer.
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
    use crate::{DateValue, SourceLine, parse_note_str as parse};

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

    fn parse_with_frontmatter(
        src: &str,
        frontmatter: &crate::config::FrontmatterConfig,
    ) -> Note {
        let tasks = crate::TaskConfig::default();
        let input = MarkdownParserInput::new(
            std::path::Path::new("note.md"),
            src,
            &tasks,
            frontmatter,
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
                note.frontmatter().map(|fm| fm.fields().is_empty()),
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
                note.frontmatter().map(|fm| fm.fields().is_empty()),
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
                Some(&NoteFieldValue::Date(
                    DateValue::parse_iso("2026-07-29").expect("valid date")
                ))
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
        fn extracts_task_item_completion_status() {
            let input = "- [ ] Incomplete task\n- [x] Completed task";
            let note = parse(input);

            let items = note.lists();
            let item0 = items.first().expect("item 0");
            assert_eq!(item0.text(), "Incomplete task");
            let task0 = item0.kind().as_task().expect("item 0 is a task");
            assert_eq!(task0.status().kind().completed(), Some(false));

            let item1 = items.get(1).expect("item 1");
            assert_eq!(item1.text(), "Completed task");
            let task1 = item1.kind().as_task().expect("item 1 is a task");
            assert_eq!(task1.status().kind().completed(), Some(true));
        }

        #[test]
        fn includes_link_display_text_in_the_containing_item_text() {
            let input = "- [ ] Check [link text](https://example.com) here";
            let note = parse(input);

            let item = note.lists().first().expect("item present");
            assert_eq!(item.text(), "Check link text here");

            let link = note.outlinks().first().expect("outlink present");
            assert_eq!(link.text(), "link text");
        }

        #[test]
        fn extracts_multiline_link_display_text() {
            let note = parse("[multi\nline](https://example.com)");
            let link = note.outlinks().first().expect("outlink present");
            assert_eq!(link.target(), "https://example.com");
            assert_eq!(link.text(), "multi\nline");
        }

        #[test]
        fn extracts_code_span_within_link_display_text() {
            let note = parse("[`my_func()`](https://example.com)");
            let link = note.outlinks().first().expect("outlink present");
            assert_eq!(link.target(), "https://example.com");
            assert_eq!(link.text(), "my_func()");
        }

        #[test]
        fn extracts_code_span_within_wikilink_display_text() {
            let note = parse("[[target|`alias`]]");
            let link = note.outlinks().first().expect("outlink present");
            assert_eq!(link.target(), "target");
            assert_eq!(link.text(), "alias");
        }

        #[test]
        fn extracts_nested_child_lists() {
            let input = "- Parent item\n  - Child item";
            let note = parse(input);

            let items = note.lists();
            assert_eq!(items.len(), 2);
            let parent_item = items.first().expect("parent item");
            assert_eq!(parent_item.clean_text(), "Parent item");
            assert_eq!(parent_item.depth(), 0);

            let child_item = items.get(1).expect("child item");
            assert_eq!(child_item.clean_text(), "Child item");
            assert_eq!(child_item.depth(), 1);
            assert_eq!(child_item.parent(), Some(parent_item.line()));
        }

        #[test]
        fn extracts_grandchild_lists_beyond_two_levels() {
            let input = "- Parent\n  - Child\n    - Grandchild";
            let note = parse(input);

            let items = note.lists();
            assert_eq!(items.len(), 3);
            assert_eq!(items.first().expect("item 0").raw_text(), "Parent");
            assert_eq!(items.get(1).expect("item 1").raw_text(), "Child");
            assert_eq!(items.get(2).expect("item 2").raw_text(), "Grandchild");
        }

        #[test]
        fn populates_depth_line_and_parent_down_the_nesting_chain() {
            let input = "- Parent\n  - Child\n    - Grandchild";
            let note = parse(input);

            let items = note.lists();
            assert_eq!(items.len(), 3);

            let parent = items.first().expect("parent item");
            assert_eq!(parent.line(), SourceLine::new(1).expect("non-zero"));
            assert_eq!(parent.depth(), 0);
            assert_eq!(parent.parent(), None);

            let child = items.get(1).expect("child item");
            assert_eq!(child.line(), SourceLine::new(2).expect("non-zero"));
            assert_eq!(child.depth(), 1);
            assert_eq!(child.parent(), Some(parent.line()));

            let grandchild = items.get(2).expect("grandchild item");
            assert_eq!(
                grandchild.line(),
                SourceLine::new(3).expect("non-zero")
            );
            assert_eq!(grandchild.depth(), 2);
            assert_eq!(grandchild.parent(), Some(child.line()));
        }

        #[test]
        fn gives_top_level_siblings_distinct_lines_and_no_parent() {
            let input = "- Parent\n  - Child\n- Sibling";
            let note = parse(input);

            let items = note.lists();
            assert_eq!(items.len(), 3);
            let sibling = items.get(2).expect("sibling item");
            assert_eq!(sibling.line(), SourceLine::new(3).expect("non-zero"));
            assert_eq!(sibling.depth(), 0);
            assert_eq!(sibling.parent(), None);
        }
        #[test]
        fn outputs_list_items_in_strict_pre_order_document_order() {
            let input = "\
- Parent 1
  - Child 1.1
    - Grandchild 1.1.1
    - Grandchild 1.1.2
  - Child 1.2
- Parent 2
  1. Ordered child 2.1
  2. Ordered child 2.2
- Sibling 3
";
            let note = parse(input);
            let items = note.lists();
            let actual: Vec<(&str, u8, bool)> = items
                .iter()
                .map(|item| {
                    (item.clean_text(), item.depth(), item.is_ordered())
                })
                .collect();
            let expected = [
                ("Parent 1", 0, false),
                ("Child 1.1", 1, false),
                ("Grandchild 1.1.1", 2, false),
                ("Grandchild 1.1.2", 2, false),
                ("Child 1.2", 1, false),
                ("Parent 2", 0, false),
                ("Ordered child 2.1", 1, true),
                ("Ordered child 2.2", 1, true),
                ("Sibling 3", 0, false),
            ];
            assert_eq!(actual.as_slice(), expected.as_slice());

            let p1_line = items.first().map(ListItem::line);
            let c1_line = items.get(1).map(ListItem::line);
            let p2_line = items.get(5).map(ListItem::line);

            assert_eq!(items.get(1).and_then(ListItem::parent), p1_line);
            assert_eq!(items.get(2).and_then(ListItem::parent), c1_line);
            assert_eq!(items.get(3).and_then(ListItem::parent), c1_line);
            assert_eq!(items.get(4).and_then(ListItem::parent), p1_line);
            assert_eq!(items.get(6).and_then(ListItem::parent), p2_line);
            assert_eq!(items.get(7).and_then(ListItem::parent), p2_line);
        }

        #[rstest]
        #[case::unordered_list("- First\n- Second", false)]
        #[case::ordered_list("1. First step\n2. Second step", true)]
        fn extracts_list_ordering(
            #[case] input: &str,
            #[case] expected_ordered: bool,
        ) {
            let note = parse(input);

            let items = note.lists();
            assert_eq!(items.len(), 2);
            assert_eq!(
                items.first().expect("item 0").is_ordered(),
                expected_ordered
            );
            assert_eq!(
                items.get(1).expect("item 1").is_ordered(),
                expected_ordered
            );
        }

        #[test]
        fn preserves_soft_breaks_inside_list_item_text() {
            let note = parse("- Wrapped\n  line");

            let text = note.lists().first().map(ListItem::raw_text);
            assert_eq!(text, Some("Wrapped\nline"));
        }

        #[test]
        fn parses_code_block_without_leaking_content_as_metadata() {
            // Arrange: fenced code block contains YAML-like content that could
            // be mistaken for frontmatter if block context doesn't switch.
            let input = "---\ntitle: Real Frontmatter\n---\n\nSome \
                         text.\n\n```\n---\nfake: value\n```\n\nMore text.";
            let note = parse(input);

            // Act: the real frontmatter has 1 field; the fenced content must
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
            // Arrange: text after a fenced code block must be treated as body
            // text, not as code block content (end_code_block
            // resets BlockContext). Inline fields are extracted from body text,
            // so verifying they appear after a code block proves the context
            // reset worked.
            let input = "```\ncode here\n```\n\nStatus:: Draft";
            let note = parse(input);

            // Act: the parser extracts inline fields from body text
            let field_count = note.inline_fields().len();

            // Assert
            assert_eq!(
                field_count, 1,
                "inline field after closing fence must be extracted"
            );
        }

        #[test]
        fn preserves_body_through_nested_list_text_blocks() {
            // Arrange: a paragraph followed by a nested list with text,
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

            // Assert: both paragraph fields must be extracted
            assert!(
                keys.contains(&"Status"),
                "first paragraph field must be extracted, got: {keys:?}"
            );
            assert!(
                keys.contains(&"Author"),
                "second paragraph field must be extracted, got: {keys:?}"
            );
        }

        #[rstest]
        #[case::loose_paragraph(
            "- Task line\n\n  Status:: Draft",
            "Task line\nStatus:: Draft"
        )]
        #[case::after_nested_list(
            "- Task line\n  - nested\n\n  after para",
            "Task line\nafter para"
        )]
        #[case::blockquote("- alpha\n\n  > quoted", "alpha\nquoted")]
        #[case::code_fence("- alpha\n\n  ```\n  code\n  ```", "alpha\ncode\n")]
        #[case::heading("- alpha\n\n  ## sub", "alpha\nsub")]
        fn separates_block_children_in_item_text(
            #[case] input: &str,
            #[case] expected: &str,
        ) {
            let note = parse(input);

            let text = note.lists().first().map(ListItem::raw_text);

            assert_eq!(text, Some(expected));
        }

        #[test]
        fn captures_field_after_blockquote_inside_item() {
            // Bare fields are line-start-gated in the scan buffer; a
            // block-level child before them must end the prior line or the
            // field is invisible to the lexer.
            let note = parse("- alpha\n\n  > Status:: Draft");

            let keys: Vec<&str> =
                note.inline_fields().iter().map(|(k, _)| k.name()).collect();

            assert!(
                keys.contains(&"Status"),
                "field after blockquote must be captured, got: {keys:?}"
            );
        }

        #[test]
        fn keeps_code_fence_content_out_of_field_scan() {
            // The scan buffer excludes code text: a bare field inside a fence
            // must never be extracted, even after block separation.
            let note = parse("- alpha\n\n  ```\n  Status:: Draft\n  ```");

            let keys: Vec<&str> =
                note.inline_fields().iter().map(|(k, _)| k.name()).collect();

            assert!(
                !keys.contains(&"Status"),
                "field inside fence must stay hidden from scan, got: {keys:?}"
            );
        }

        #[test]
        fn emits_breaks_in_body_text() {
            // Arrange: hard breaks (two trailing spaces) in body text must
            // produce newlines in the body buffer (L317 mutant removes the
            // push). We verify indirectly: a field value must be
            // truncated at the newline, not span across the break.
            let input = "Key:: value1  \nmore text";
            let note = parse(input);

            // Act
            let (_key, values) =
                note.inline_fields().iter().next().expect("field present");

            // Assert: field value must stop at the hard break
            assert_eq!(
                values.first().and_then(|v| v.as_str()),
                Some("value1"),
                "hard breaks must appear as newlines in body, got: {:?}",
                values.first()
            );
        }

        #[test]
        fn preserves_inline_code_in_list_item_text() {
            // Arrange: inline code inside a list item must appear in the
            // item's display text (buffers.text) but NOT in the scan
            // buffer (for field/tag scanning). The push_code method
            // writes only to buffers.text.
            let input = "- Item with `inline code` here\n";
            let note = parse(input);

            let item_text = note
                .lists()
                .first()
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

        /// Asserts the note carries exactly one inline field whose key
        /// canonically matches `key`, returning its values.
        fn single_inline_field<'a>(
            note: &'a Note,
            key: &str,
        ) -> &'a [NoteFieldValue] {
            assert_eq!(note.inline_fields().len(), 1);
            let (field_key, values) =
                note.inline_fields().iter().next().expect("field present");
            assert!(field_key.is_canonical_match(key));
            values
        }
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

            let values = single_inline_field(&note, expected_key);
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
            let values = single_inline_field(&note, "status");
            assert_eq!(values.first().and_then(|v| v.as_str()), Some("Draft"));

            let item = note.lists().first().expect("item present");
            assert_eq!(item.text(), "Status:: Draft");
        }

        #[test]
        fn scopes_a_list_item_field_to_that_item_and_not_its_siblings() {
            let note = parse(
                "- [ ] First task [priority:: high]\n- [ ] Second task \
                 [priority:: low]",
            );

            let items = note.lists();
            let first = items.first().expect("first item present");
            let second = items.get(1).expect("second item present");

            let first_fields =
                first.fields().expect("first item fields present");
            let first_priority = first_fields
                .iter()
                .find(|(k, _)| k.is_canonical_match("priority"))
                .expect("first item field present");
            assert_eq!(
                first_priority.1.first().and_then(|v| v.as_str()),
                Some("high")
            );

            let second_fields =
                second.fields().expect("second item fields present");
            let second_priority = second_fields
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
            let items = note.lists();
            let first = items.first().expect("first item present");
            let second = items.get(1).expect("second item present");

            let first_fields =
                first.fields().expect("first item fields present");
            let (first_key, first_vals) =
                first_fields.iter().next().expect("first due field");
            assert!(first_key.is_canonical_match("due"));
            assert_eq!(
                first_vals.first(),
                Some(&NoteFieldValue::Date(
                    DateValue::parse_iso("2026-01-01").expect("valid date")
                ))
            );

            let second_fields =
                second.fields().expect("second item fields present");
            let (_second_key, second_vals) =
                second_fields.iter().next().expect("second due field");
            assert_eq!(
                second_vals.first(),
                Some(&NoteFieldValue::Date(
                    DateValue::parse_iso("2026-02-02").expect("valid date")
                ))
            );
        }

        #[test]
        fn plain_list_items_without_fields_have_no_scoped_fields() {
            let note = parse("- Plain item with no fields");

            let item = note.lists().first().expect("item present");
            assert!(item.fields().is_none());
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

            let values = single_inline_field(&note, expected_key);
            assert_eq!(
                values.first(),
                Some(&NoteFieldValue::Date(
                    DateValue::parse_iso(expected_date).expect("valid date")
                ))
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
                Some(&NoteFieldValue::Date(
                    DateValue::parse_iso("2022-07-14").expect("valid date")
                ))
            );
            let (sched_key, sched_vals) =
                fields.iter().nth(1).expect("scheduled field");
            assert_eq!(sched_key.name(), "scheduled");
            assert_eq!(
                sched_vals.first(),
                Some(&NoteFieldValue::Date(
                    DateValue::parse_iso("2022-07-24").expect("valid date")
                ))
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

            let item = note.lists().first().expect("item present");
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
            let values = single_inline_field(&note, "status");
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

            let item = note.lists().first().expect("item present");
            assert_eq!(item.text(), "Status:: Draft #urgent");

            let values = single_inline_field(&note, "status");
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

            let values = single_inline_field(&note, "status");
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

            let values = single_inline_field(&note, "status");
            assert_eq!(values.first().and_then(|v| v.as_str()), Some("Draft"));
        }

        #[test]
        fn extracts_a_visible_key_field_from_a_markdown_links_display_text() {
            let note = parse("[Status:: Draft](http://example.com)");

            assert_eq!(note.outlinks().len(), 1);
            let values = single_inline_field(&note, "status");
            assert_eq!(values.first().and_then(|v| v.as_str()), Some("Draft"));

            let link = note.outlinks().first().expect("outlink present");
            assert_eq!(link.target(), "http://example.com");
            assert_eq!(link.text(), "Status:: Draft");
        }

        #[test]
        fn extracts_a_visible_key_field_from_link_text_amid_other_prose() {
            let note = parse("See [Status:: Draft](http://example.com) here.");

            let values = single_inline_field(&note, "status");
            assert_eq!(values.first().and_then(|v| v.as_str()), Some("Draft"));
        }
    }

    mod frontmatter_tag_merge {
        use pretty_assertions::assert_eq;
        use rstest::rstest;

        use super::*;

        #[rstest]
        #[case::yaml_flow_list("---\ntags: [a, b]\n---\nBody", &["#a", "#b"])]
        #[case::yaml_block_list(
            "---\ntags:\n  - a\n  - b\n---\nBody",
            &["#a", "#b"]
        )]
        #[case::bare_scalar("---\ntags: a\n---\nBody", &["#a"])]
        #[case::comma_separated_scalar(
            "---\ntags: a, b\n---\nBody",
            &["#a", "#b"]
        )]
        #[case::leading_hash_normalizes_instead_of_doubling(
            "---\ntags: [\"#a\"]\n---\nBody",
            &["#a"]
        )]
        #[case::invalid_candidates_dropped(
            "---\ntags: [a, \"\", \"1bad\", \"bad!\"]\n---\nBody",
            &["#a"]
        )]
        #[case::comma_inside_a_list_element_splits_like_a_scalar(
            "---\ntags:\n  - \"a, b\"\n  - c\n---\nBody",
            &["#a", "#b", "#c"]
        )]
        fn extracts_tags_from_a_frontmatter_value_form(
            #[case] input: &str,
            #[case] expected: &[&str],
        ) {
            let note = parse(input);

            let expected: Vec<Tag> =
                expected.iter().map(|tag| Tag::parse(tag).unwrap()).collect();
            assert_eq!(note.tags(), expected.as_slice());
        }

        #[test]
        fn reads_from_a_configured_non_default_tags_key_without_tags_fallback()
        {
            let frontmatter = crate::config::FrontmatterConfig::default()
                .with_tags_name("categories");
            let note = parse_with_frontmatter(
                "---\ncategories: [a]\ntags: [b]\n---\nBody",
                &frontmatter,
            );

            assert_eq!(note.tags(), [Tag::parse("#a").unwrap()]);
        }

        #[test]
        fn combines_frontmatter_tags_with_body_tags() {
            let note = parse("---\ntags: [a]\n---\nBody with #b tag");

            assert_eq!(note.tags(), [
                Tag::parse("#b").unwrap(),
                Tag::parse("#a").unwrap()
            ]);
        }
    }

    mod tag_filters {
        use pretty_assertions::assert_eq;

        use super::*;
        use crate::TaskConfig;

        #[test]
        fn classifies_matching_items_as_tasks_and_non_matching_as_checkboxes() {
            let tasks = TaskConfig::from_tags(&["#task"]);
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

            let items = note.lists();
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
    }
}
