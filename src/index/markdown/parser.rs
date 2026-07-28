//! Markdown note event parser yielding [`Note`] records.

use pulldown_cmark::{Event, LinkType, Options, Parser, Tag, TagEnd};

use super::types::{
    CodeRegion, Frontmatter, List, ListItem, Note, Outlink, OutlinkType,
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
    let mut active_link: Option<(String, OutlinkType, String)> = None;

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
                    OutlinkType::Wikilink
                } else {
                    OutlinkType::Markdown
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
    use pretty_assertions::assert_eq;

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
