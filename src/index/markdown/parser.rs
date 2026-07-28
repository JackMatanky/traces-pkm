//! Markdown note event parser yielding [`Note`] records.

use pulldown_cmark::{
    Event, LinkType as CmarkLinkType, Options, Parser, Tag, TagEnd,
};

use super::types::{
    CodeRegion, Frontmatter, LinkType, List, ListItem, Note, Outlink,
    TaskStatus,
};

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
    let mut active_link: Option<(String, LinkType, String)> = None;

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
                let kind =
                    if matches!(link_type, CmarkLinkType::WikiLink { .. }) {
                        LinkType::Wikilink
                    } else {
                        LinkType::Markdown
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
                    let list_item = ListItem::with_children(
                        item_frame.text_buffer.trim(),
                        item_frame.task_status,
                        item_frame.children,
                    );
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
    use super::*;

    mod parse {
        use pretty_assertions::assert_eq;
        use rstest::rstest;

        use super::*;

        #[test]
        fn returns_empty_note_when_source_is_empty() {
            let input = "";
            let note = parse_markdown(input);

            assert_eq!(note.frontmatter(), None);
            assert_eq!(note.lists().len(), 0);
            assert_eq!(note.outlinks().len(), 0);
            assert_eq!(note.code_regions().len(), 0);
        }

        #[test]
        fn returns_none_for_frontmatter_when_absent() {
            let input = "# Header\nNo YAML block.";
            let note = parse_markdown(input);

            assert_eq!(note.frontmatter(), None);
        }

        #[test]
        fn extracts_yaml_frontmatter_block_raw_content() {
            let input = "---\ntitle: My Note\ntags: [rust, pkm]\n---\n# Header";
            let note = parse_markdown(input);

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
            let note = parse_markdown(input);

            let link = note.outlinks().first().expect("outlink present");
            assert_eq!(link.target(), expected_target);
            assert_eq!(link.text(), expected_text);
            assert_eq!(link.kind(), expected_kind);
        }

        #[test]
        fn extracts_task_item_completion_status() {
            let input = "- [ ] Incomplete task\n- [x] Completed task";
            let note = parse_markdown(input);

            let list = note.lists().first().expect("list present");
            let item0 = list.items().get(0).expect("item 0");
            let item1 = list.items().get(1).expect("item 1");

            assert_eq!(item0.text(), "Incomplete task");
            assert_eq!(item0.is_task(), true);
            assert_eq!(item0.is_completed(), false);

            assert_eq!(item1.text(), "Completed task");
            assert_eq!(item1.is_task(), true);
            assert_eq!(item1.is_completed(), true);
        }

        #[test]
        fn extracts_nested_child_lists() {
            let input = "- Parent item\n  - Child item";
            let note = parse_markdown(input);

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
            let note = parse_markdown(input);

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
            let note = parse_markdown(input);

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
            let note = parse_markdown(input);

            let tasks: Vec<&ListItem> = note.tasks().collect();
            assert_eq!(tasks.len(), 2);
            assert_eq!(tasks.get(0).map(|t| t.text()), Some("Task 1"));
            assert_eq!(tasks.get(1).map(|t| t.text()), Some("Task 2"));
        }

        #[test]
        fn iterates_nested_sub_list_task_items() {
            let input = "- Plain parent\n  - [x] Subtask 1";
            let note = parse_markdown(input);

            let tasks: Vec<&ListItem> = note.tasks().collect();
            assert_eq!(tasks.len(), 1);
            assert_eq!(tasks.get(0).map(|t| t.text()), Some("Subtask 1"));
            assert_eq!(tasks.get(0).map(|t| t.is_completed()), Some(true));
        }
    }
}
