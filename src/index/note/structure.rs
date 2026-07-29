//! Markdown structure extracted from notes.
//!
//! This module stores links, lists, task state, tags, and code ranges produced
//! by the markdown parser.

use std::ops::Range;

use serde::{Deserialize, Serialize};

/// Link syntax used for an extracted [`Outlink`].
#[derive(Copy, Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub(crate) enum LinkType {
    /// Standard Markdown `[text](target)` link.
    Markdown,
    /// Obsidian `[[target|alias]]` wikilink.
    Wikilink,
}

/// Outgoing link extracted from markdown link syntax.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub(crate) struct Outlink {
    target: String,
    text: String,
    kind: LinkType,
}

impl Outlink {
    /// Creates an outlink from a target, display text, and syntax kind.
    #[inline]
    #[must_use]
    pub(crate) fn new(
        target: impl Into<String>,
        text: impl Into<String>,
        kind: LinkType,
    ) -> Self {
        Self {
            target: target.into(),
            text: text.into(),
            kind,
        }
    }

    /// Target URL, relative path, or wikilink page target.
    #[inline]
    #[must_use]
    pub(crate) fn target(&self) -> &str {
        &self.target
    }

    /// Display text, or alias text for wikilinks.
    #[inline]
    #[must_use]
    pub(crate) fn text(&self) -> &str {
        &self.text
    }

    /// Link syntax kind.
    #[inline]
    #[must_use]
    pub(crate) fn kind(&self) -> LinkType {
        self.kind
    }

    /// Returns `true` if this link is a Wikilink.
    #[inline]
    #[must_use]
    pub(crate) fn is_wikilink(&self) -> bool {
        matches!(self.kind, LinkType::Wikilink)
    }

    /// Returns `true` if this link is a standard Markdown link.
    #[inline]
    #[must_use]
    pub(crate) fn is_markdown(&self) -> bool {
        matches!(self.kind, LinkType::Markdown)
    }
}

/// Completion state of a markdown task list item.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub(crate) enum TaskStatus {
    /// Unchecked task item.
    Incomplete,
    /// Checked task item.
    Complete,
}

/// List item text, optional task state, and nested child lists.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub(crate) struct ListItem {
    text: String,
    task_status: Option<TaskStatus>,
    children: Vec<List>,
}

impl ListItem {
    /// Creates a list item without child lists.
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

    /// Creates a list item with nested child lists.
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

    /// Plain text content.
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

    /// Nested lists under this item.
    #[inline]
    #[must_use]
    pub(crate) fn children(&self) -> &[List] {
        &self.children
    }
}

/// Ordered or unordered markdown list.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub(crate) struct List {
    is_ordered: bool,
    items: Vec<ListItem>,
}

impl List {
    /// Creates a list from its ordering flag and items.
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

    /// Items contained directly in this list.
    #[inline]
    #[must_use]
    pub(crate) fn items(&self) -> &[ListItem] {
        &self.items
    }
}

/// Source byte range of inline code or a code block excluded from metadata
/// scanning.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub(crate) struct CodeRegion {
    start: usize,
    end: usize,
}

impl CodeRegion {
    /// Creates a code region from start and end byte offsets.
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

/// Markdown tag including its leading `#`, such as `#book`.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub(crate) struct Tag(String);

impl Tag {
    /// Creates a tag from text that includes the leading `#`.
    #[inline]
    #[must_use]
    pub(crate) fn new(text: impl Into<String>) -> Self {
        Self(text.into())
    }

    /// Full tag text, including the leading `#`.
    #[inline]
    #[must_use]
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    mod outlink {
        use pretty_assertions::assert_eq;
        use rstest::rstest;

        use super::*;

        #[rstest]
        #[case::wikilink("target", "alias", LinkType::Wikilink, true, false)]
        #[case::markdown(
            "https://example.com",
            "text",
            LinkType::Markdown,
            false,
            true
        )]
        fn evaluates_outlink_kind_predicates(
            #[case] target: &str,
            #[case] text: &str,
            #[case] kind: LinkType,
            #[case] expected_wikilink: bool,
            #[case] expected_markdown: bool,
        ) {
            let link = Outlink::new(target, text, kind);
            assert_eq!(link.target(), target);
            assert_eq!(link.text(), text);
            assert_eq!(link.kind(), kind);
            assert_eq!(link.is_wikilink(), expected_wikilink);
            assert_eq!(link.is_markdown(), expected_markdown);
        }
    }

    mod list_item {
        use pretty_assertions::assert_eq;
        use rstest::rstest;

        use super::*;

        #[rstest]
        #[case::incomplete_task(Some(TaskStatus::Incomplete), true, false)]
        #[case::completed_task(Some(TaskStatus::Complete), true, true)]
        #[case::plain_bullet(None, false, false)]
        fn evaluates_list_item_task_predicates(
            #[case] task_status: Option<TaskStatus>,
            #[case] expected_is_task: bool,
            #[case] expected_is_completed: bool,
        ) {
            let item = ListItem::new("task item", task_status);
            assert_eq!(item.text(), "task item");
            assert_eq!(item.task_status(), task_status);
            assert_eq!(item.is_task(), expected_is_task);
            assert_eq!(item.is_completed(), expected_is_completed);
        }

        #[test]
        fn stores_child_lists_when_constructed_with_children() {
            let child = List::new(false, vec![ListItem::new("child", None)]);
            let item =
                ListItem::with_children("parent", None, vec![child.clone()]);

            assert_eq!(item.children(), [child]);
        }
    }

    mod list {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn stores_ordering_and_items() {
            let item = ListItem::new("task item", Some(TaskStatus::Incomplete));
            let list = List::new(true, vec![item.clone()]);

            assert_eq!(list.is_ordered(), true);
            assert_eq!(list.items(), [item]);
        }
    }

    mod code_region {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn returns_the_original_source_range() {
            let region = CodeRegion::new(3, 7);

            assert_eq!(region.range(), 3..7);
        }
    }

    mod tag {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn stores_the_given_text() {
            let tag = Tag::new("#book");

            assert_eq!(tag.as_str(), "#book");
        }
    }
}
