//! Markdown note event parser yielding [`Note`] records.

use std::{mem, ops::Range, path::PathBuf};

use pulldown_cmark::{
    CowStr, Event, LinkType as CmarkLinkType, Options, Parser, Tag, TagEnd,
};

use super::types::{
    CodeRegion, Frontmatter, LinkType, List, ListItem, Note, Outlink,
    TaskStatus,
};

/// Parses a markdown string into a [`Note`] record using `pulldown-cmark`.
#[must_use]
pub(crate) fn parse_markdown(path: impl Into<PathBuf>, src: &str) -> Note {
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_TASKLISTS);
    opts.insert(Options::ENABLE_YAML_STYLE_METADATA_BLOCKS);
    opts.insert(Options::ENABLE_WIKILINKS);

    let mut context = ParserContext::default();
    for (event, range) in Parser::new_ext(src, opts).into_offset_iter() {
        context.handle_event(event, range);
    }
    context.into_note(path)
}

/// Accumulated context while walking `pulldown-cmark` events for one Note.
#[derive(Default)]
struct ParserContext {
    frontmatter: Option<Frontmatter>,
    in_metadata_block: bool,
    metadata_buffer: String,
    outlinks: Vec<Outlink>,
    active_link: Option<(String, LinkType, String)>,
    code_regions: Vec<CodeRegion>,
    active_code_block_start: Option<usize>,
    lists: Vec<List>,
    list_stack: Vec<ListFrame>,
    item_stack: Vec<ItemFrame>,
}

impl ParserContext {
    /// Consumes the accumulated context into a [`Note`] at `path`.
    fn into_note(self, path: impl Into<PathBuf>) -> Note {
        Note::new(
            path,
            self.frontmatter,
            self.lists,
            self.outlinks,
            self.code_regions,
        )
    }

    /// Dispatches one `pulldown-cmark` event to the handler for its kind.
    fn handle_event(&mut self, event: Event<'_>, range: Range<usize>) {
        match event {
            Event::Start(Tag::MetadataBlock(_)) => self.start_metadata_block(),
            Event::End(TagEnd::MetadataBlock(_)) => self.end_metadata_block(),
            Event::Start(Tag::Link {
                link_type,
                dest_url,
                ..
            }) => self.start_link(link_type, dest_url),
            Event::End(TagEnd::Link) => self.end_link(),
            Event::Start(Tag::CodeBlock(_)) => self.start_code_block(range),
            Event::End(TagEnd::CodeBlock) => self.end_code_block(range),
            Event::Code(text) => self.inline_code(&text, range),
            Event::Start(Tag::List(start_number)) => {
                self.start_list(start_number.is_some());
            }
            Event::End(TagEnd::List(_)) => self.end_list(),
            Event::Start(Tag::Item) => self.start_item(),
            Event::End(TagEnd::Item) => self.end_item(),
            Event::TaskListMarker(checked) => self.set_task_status(checked),
            Event::Text(text) => self.push_text(&text),
            Event::SoftBreak | Event::HardBreak => self.push_break(),
            _ => {}
        }
    }

    fn start_metadata_block(&mut self) {
        self.in_metadata_block = true;
        self.metadata_buffer.clear();
    }

    fn end_metadata_block(&mut self) {
        self.in_metadata_block = false;
        self.frontmatter =
            Some(Frontmatter::new(mem::take(&mut self.metadata_buffer)));
    }

    fn start_link(&mut self, link_type: CmarkLinkType, dest_url: CowStr<'_>) {
        let kind = if matches!(link_type, CmarkLinkType::WikiLink { .. }) {
            LinkType::Wikilink
        } else {
            LinkType::Markdown
        };
        self.active_link = Some((dest_url.into_string(), kind, String::new()));
    }

    fn end_link(&mut self) {
        if let Some((target, kind, text)) = self.active_link.take() {
            self.outlinks.push(Outlink::new(target, text, kind));
        }
    }

    fn start_code_block(&mut self, range: Range<usize>) {
        self.active_code_block_start = Some(range.start);
    }

    fn end_code_block(&mut self, range: Range<usize>) {
        if let Some(start) = self.active_code_block_start.take() {
            self.code_regions.push(CodeRegion::new(start, range.end));
        }
    }

    fn inline_code(&mut self, text: &str, range: Range<usize>) {
        self.code_regions.push(CodeRegion::new(range.start, range.end));
        if let Some(item) = self.item_stack.last_mut() {
            item.text_buffer.push_str(text);
        }
    }

    fn start_list(&mut self, is_ordered: bool) {
        self.list_stack.push(ListFrame {
            is_ordered,
            items: Vec::new(),
        });
    }

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
            children: Vec::new(),
        });
    }

    fn end_item(&mut self) {
        if let Some(item_frame) = self.item_stack.pop() {
            let item = ListItem::with_children(
                item_frame.text_buffer,
                item_frame.task_status,
                item_frame.children,
            );
            if let Some(list_frame) = self.list_stack.last_mut() {
                list_frame.items.push(item);
            }
        }
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

    /// Appends `text` to the active metadata buffer and/or link display
    /// text; independently, also appends to the enclosing list item's text
    /// if one is active, so a link's display text is part of both the
    /// [`Outlink`] and the plain text of the item containing it.
    fn push_text(&mut self, text: &str) {
        if self.in_metadata_block {
            self.metadata_buffer.push_str(text);
            return;
        }
        if let Some((_, _, link_text)) = self.active_link.as_mut() {
            link_text.push_str(text);
        }
        if let Some(item) = self.item_stack.last_mut() {
            item.text_buffer.push_str(text);
        }
    }

    /// Appends a newline to the active metadata buffer or list item text.
    fn push_break(&mut self) {
        if self.in_metadata_block {
            self.metadata_buffer.push('\n');
            return;
        }
        if let Some(item) = self.item_stack.last_mut() {
            item.text_buffer.push('\n');
        }
    }
}

/// Active list context on the parser stack.
struct ListFrame {
    is_ordered: bool,
    items: Vec<ListItem>,
}

/// Active list item context on the parser stack.
struct ItemFrame {
    task_status: Option<TaskStatus>,
    text_buffer: String,
    children: Vec<List>,
}

#[cfg(test)]
mod tests {
    use super::*;

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
            assert_eq!(note.code_regions().len(), 0);
        }

        #[test]
        fn returns_none_for_frontmatter_when_absent() {
            let input = "# Header\nNo YAML block.";
            let note = parse_markdown("note.md", input);

            assert_eq!(note.frontmatter(), None);
        }

        #[test]
        fn extracts_yaml_frontmatter_block_raw_content() {
            let input = "---\ntitle: My Note\ntags: [rust, pkm]\n---\n# Header";
            let note = parse_markdown("note.md", input);

            let fm = note.frontmatter().expect("frontmatter present");
            assert_eq!(fm.raw(), "title: My Note\ntags: [rust, pkm]\n");
            assert_eq!(fm.is_empty(), false);
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
        #[case::inline_code_span(
            "Text with `inline code` span.",
            "`inline code`"
        )]
        #[case::fenced_code_block(
            "```rust\nfn main() {}\n```",
            "```rust\nfn main() {}\n```"
        )]
        fn tracks_code_regions(
            #[case] input: &str,
            #[case] expected_snippet: &str,
        ) {
            let note = parse_markdown("note.md", input);

            let region = note.code_regions().first().expect("code region");
            assert_eq!(&input[region.range()], expected_snippet);
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
}
