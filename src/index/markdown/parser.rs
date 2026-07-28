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

        #[test]
        fn extracts_wikilinks_with_display_alias() {
            let input = "See [[target_page|Display Alias]] for context.";
            let note = parse_markdown(input);

            let link = note.outlinks().first().expect("wikilink present");
            assert_eq!(link.target(), "target_page");
            assert_eq!(link.text(), "Display Alias");
            assert_eq!(link.kind(), LinkType::Wikilink);
            assert_eq!(link.is_wikilink(), true);
        }

        #[test]
        fn extracts_wikilinks_without_display_alias() {
            let input = "See [[simple_target]] for details.";
            let note = parse_markdown(input);

            let link = note.outlinks().first().expect("wikilink present");
            assert_eq!(link.target(), "simple_target");
            assert_eq!(link.text(), "simple_target");
            assert_eq!(link.kind(), LinkType::Wikilink);
            assert_eq!(link.is_wikilink(), true);
        }

        #[test]
        fn extracts_standard_markdown_links() {
            let input = "Check out [Markdown Link](https://example.com).";
            let note = parse_markdown(input);

            let link = note.outlinks().first().expect("markdown link present");
            assert_eq!(link.target(), "https://example.com");
            assert_eq!(link.text(), "Markdown Link");
            assert_eq!(link.kind(), LinkType::Markdown);
            assert_eq!(link.is_markdown(), true);
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

        #[test]
        fn extracts_unordered_lists() {
            let input = "- First\n- Second";
            let note = parse_markdown(input);

            let list = note.lists().first().expect("list present");
            assert_eq!(list.is_ordered(), false);
            assert_eq!(list.items().len(), 2);
        }

        #[test]
        fn extracts_ordered_lists() {
            let input = "1. First step\n2. Second step";
            let note = parse_markdown(input);

            let list = note.lists().first().expect("list present");
            assert_eq!(list.is_ordered(), true);
            assert_eq!(list.items().len(), 2);
        }

        #[test]
        fn tracks_inline_code_span_byte_regions() {
            let input = "Text with `inline code` span.";
            let note = parse_markdown(input);

            let region = note.code_regions().first().expect("code region");
            assert_eq!(&input[region.range()], "`inline code`");
        }

        #[test]
        fn tracks_fenced_code_block_byte_regions() {
            let input = "```rust\nfn main() {}\n```";
            let note = parse_markdown(input);

            let region = note.code_regions().first().expect("code region");
            assert_eq!(&input[region.range()], "```rust\nfn main() {}\n```");
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
