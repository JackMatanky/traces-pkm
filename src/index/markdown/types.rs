//! Markdown Note Metadata domain types.

use std::{
    ops::Range,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

/// Rich Note Metadata extracted from a markdown file: frontmatter, lists,
/// outlinks, code regions, Inline Fields, and tags. [`Self::tasks`] derives
/// task items from the indexed lists rather than storing them separately.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub(crate) struct Note {
    path: PathBuf,
    frontmatter: Option<Frontmatter>,
    lists: Vec<List>,
    outlinks: Vec<Outlink>,
    code_regions: Vec<CodeRegion>,
    inline_fields: Vec<InlineField>,
    tags: Vec<Tag>,
}

impl Note {
    /// Creates a new [`Note`] with no Inline Fields or tags. Chain
    /// [`Self::with_inline_fields`] and/or [`Self::with_tags`] to attach
    /// them.
    #[inline]
    #[must_use]
    pub(crate) fn new(
        path: impl Into<PathBuf>,
        frontmatter: Option<Frontmatter>,
        lists: Vec<List>,
        outlinks: Vec<Outlink>,
        code_regions: Vec<CodeRegion>,
    ) -> Self {
        Self {
            path: path.into(),
            frontmatter,
            lists,
            outlinks,
            code_regions,
            inline_fields: Vec::new(),
            tags: Vec::new(),
        }
    }

    /// Returns this [`Note`] with `inline_fields` attached.
    #[inline]
    #[must_use]
    pub(crate) fn with_inline_fields(
        mut self,
        inline_fields: Vec<InlineField>,
    ) -> Self {
        self.inline_fields = inline_fields;
        self
    }

    /// Returns this [`Note`] with `tags` attached.
    #[inline]
    #[must_use]
    pub(crate) fn with_tags(mut self, tags: Vec<Tag>) -> Self {
        self.tags = tags;
        self
    }

    /// Project-relative path of this note.
    #[inline]
    #[must_use]
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    /// Extracted YAML frontmatter block, if present.
    #[inline]
    #[must_use]
    pub(crate) fn frontmatter(&self) -> Option<&Frontmatter> {
        self.frontmatter.as_ref()
    }

    /// Top-level lists extracted from the note's body. Nested lists live under
    /// each [`ListItem::children`], not here — see [`Self::tasks`] for a
    /// flattened view that does walk into nested lists.
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

    /// Dataview-compatible Inline Fields extracted from body text and list
    /// items, in document order.
    #[inline]
    #[must_use]
    pub(crate) fn inline_fields(&self) -> &[InlineField] {
        &self.inline_fields
    }

    /// Markdown tags (e.g. `#book`, `#projects/active`) extracted from body
    /// text and list items, in document order.
    #[inline]
    #[must_use]
    pub(crate) fn tags(&self) -> &[Tag] {
        &self.tags
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

/// Depth-first walk of `list` and its nested sub-lists, appending every task
/// item to `acc`. Recursion depth tracks list nesting depth, which markdown
/// limits in practice (indentation-driven).
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

/// Frontmatter metadata block extracted from a markdown Note.
#[derive(Clone, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
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

/// Link target classification: standard Markdown link or Obsidian Wikilink.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub(crate) enum LinkType {
    Markdown,
    Wikilink,
}

/// An outgoing link extracted from a markdown Note.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub(crate) struct Outlink {
    target: String,
    text: String,
    kind: LinkType,
}

impl Outlink {
    /// Creates a new [`Outlink`].
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

    /// Link syntax classification ([`LinkType::Markdown`] or
    /// [`LinkType::Wikilink`]).
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

/// Completion state for a task list item.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub(crate) enum TaskStatus {
    Incomplete,
    Complete,
}

/// A single item within a markdown list.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
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

    /// Creates a new [`ListItem`] with child lists.
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
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
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
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
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

/// Dataview-compatible Inline Field syntax form.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub(crate) enum InlineFieldForm {
    /// `Key:: Value` filling an entire line.
    Body,
    /// `[Key:: Value]` — the key stays visible in rendered text.
    VisibleKey,
    /// `(Key:: Value)` — the key is hidden in rendered text.
    HiddenKey,
}

/// A Dataview-compatible Inline Field extracted from a Note's body text or
/// list items.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub(crate) struct InlineField {
    key: String,
    value: String,
    form: InlineFieldForm,
}

impl InlineField {
    /// Creates a new [`InlineField`].
    #[inline]
    #[must_use]
    pub(crate) fn new(
        key: impl Into<String>,
        value: impl Into<String>,
        form: InlineFieldForm,
    ) -> Self {
        Self {
            key: key.into(),
            value: value.into(),
            form,
        }
    }

    /// The field's key.
    #[inline]
    #[must_use]
    pub(crate) fn key(&self) -> &str {
        &self.key
    }

    /// The field's value.
    #[inline]
    #[must_use]
    pub(crate) fn value(&self) -> &str {
        &self.value
    }

    /// Syntax form the field was written in.
    #[inline]
    #[must_use]
    pub(crate) fn form(&self) -> InlineFieldForm {
        self.form
    }
}

/// A markdown tag (e.g. `#book`, `#projects/active`), including its
/// leading `#`.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub(crate) struct Tag(String);

impl Tag {
    /// Creates a new [`Tag`] from its full text, including the leading `#`.
    #[inline]
    #[must_use]
    pub(crate) fn new(text: impl Into<String>) -> Self {
        Self(text.into())
    }

    /// The tag's full text, including the leading `#`.
    #[inline]
    #[must_use]
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    mod note {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn constructs_note_with_the_given_path_and_parts() {
            let frontmatter = Frontmatter::new("title: A\n");
            let list = List::new(false, vec![ListItem::new("item", None)]);
            let outlink = Outlink::new("target", "text", LinkType::Wikilink);
            let code_region = CodeRegion::new(3, 7);

            let note = Note::new(
                "notes/a.md",
                Some(frontmatter.clone()),
                vec![list.clone()],
                vec![outlink.clone()],
                vec![code_region.clone()],
            );

            assert_eq!(note.path(), Path::new("notes/a.md"));
            assert_eq!(note.frontmatter(), Some(&frontmatter));
            assert_eq!(note.lists(), [list]);
            assert_eq!(note.outlinks(), [outlink]);
            assert_eq!(note.code_regions(), [code_region]);
        }

        #[test]
        fn constructs_note_with_no_frontmatter_and_empty_collections() {
            let note = Note::new(
                "notes/a.md",
                None,
                Vec::new(),
                Vec::new(),
                Vec::new(),
            );
            assert_eq!(note.path(), Path::new("notes/a.md"));
            assert_eq!(note.frontmatter(), None);
            assert_eq!(note.lists().len(), 0);
            assert_eq!(note.outlinks().len(), 0);
            assert_eq!(note.code_regions().len(), 0);
        }
    }

    mod frontmatter {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn creates_frontmatter_with_raw_content() {
            let fm = Frontmatter::new("key: value\n");
            assert_eq!(fm.raw(), "key: value\n");
            assert_eq!(fm.is_empty(), false);
        }

        #[test]
        fn creates_empty_frontmatter() {
            let fm = Frontmatter::default();
            assert_eq!(fm.raw(), "");
            assert_eq!(fm.is_empty(), true);
        }
    }

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
            assert_eq!(item.children().len(), 0);
        }

        #[test]
        fn creates_item_with_children() {
            let child_item = ListItem::new("child item", None);
            let child_list = List::new(false, vec![child_item]);
            let parent_item =
                ListItem::with_children("parent item", None, vec![child_list]);

            assert_eq!(parent_item.children().len(), 1);
            let parent_children = parent_item.children();
            let first_child_list = parent_children.first().expect("child list");
            assert_eq!(first_child_list.items().len(), 1);
            let child_items = first_child_list.items();
            let first_child_item = child_items.first().expect("child item");
            assert_eq!(first_child_item.text(), "child item");
        }
    }

    mod list {
        use pretty_assertions::assert_eq;
        use rstest::rstest;

        use super::*;

        #[rstest]
        #[case::ordered(true)]
        #[case::unordered(false)]
        fn stores_the_given_ordering_and_items(#[case] is_ordered: bool) {
            let item1 = ListItem::new("First step", None);
            let item2 = ListItem::new("Second step", None);
            let list = List::new(is_ordered, vec![item1, item2]);

            assert_eq!(list.is_ordered(), is_ordered);
            assert_eq!(list.items().len(), 2);
        }
    }

    mod code_region {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn returns_range() {
            let region = CodeRegion::new(10, 25);
            assert_eq!(region.range(), 10..25);
        }
    }

    mod inline_field {
        use pretty_assertions::assert_eq;
        use rstest::rstest;

        use super::*;

        #[rstest]
        #[case::body(InlineFieldForm::Body)]
        #[case::visible_key(InlineFieldForm::VisibleKey)]
        #[case::hidden_key(InlineFieldForm::HiddenKey)]
        fn stores_key_value_and_form(#[case] form: InlineFieldForm) {
            let field = InlineField::new("Author", "Jane Doe", form);

            assert_eq!(field.key(), "Author");
            assert_eq!(field.value(), "Jane Doe");
            assert_eq!(field.form(), form);
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

    mod builder {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn with_inline_fields_attaches_the_given_fields() {
            let field =
                InlineField::new("Status", "Draft", InlineFieldForm::Body);

            let note = Note::new(
                "notes/a.md",
                None,
                Vec::new(),
                Vec::new(),
                Vec::new(),
            )
            .with_inline_fields(vec![field.clone()]);

            assert_eq!(note.inline_fields(), [field]);
        }

        #[test]
        fn with_tags_attaches_the_given_tags() {
            let note = Note::new(
                "notes/a.md",
                None,
                Vec::new(),
                Vec::new(),
                Vec::new(),
            )
            .with_tags(vec![Tag::new("#book")]);

            assert_eq!(note.tags(), [Tag::new("#book")]);
        }

        #[test]
        fn defaults_to_no_inline_fields_or_tags() {
            let note = Note::new(
                "notes/a.md",
                None,
                Vec::new(),
                Vec::new(),
                Vec::new(),
            );

            assert_eq!(note.inline_fields().len(), 0);
            assert_eq!(note.tags().len(), 0);
        }
    }
}
