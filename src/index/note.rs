//! Parsed markdown note metadata.

mod inline;
mod metadata;
mod parser;
mod structure;

use std::path::{Path, PathBuf};

pub(crate) use metadata::{
    FieldValue, Frontmatter, InlineField, InlineFieldForm, MetadataField,
    RawFrontmatter,
};
pub(crate) use parser::parse_markdown;
use serde::{Deserialize, Serialize};
pub(crate) use structure::{
    CodeRegion, LinkType, List, ListItem, Outlink, Tag, TaskStatus,
};

/// Metadata extracted from one markdown note.
///
/// A [`Note`] stores frontmatter, lists, outlinks, code regions, inline fields,
/// and tags. [`Self::tasks`] derives task items from stored lists instead of
/// duplicating them.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
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
    /// Creates a [`Note`] without inline fields or tags.
    ///
    /// The constructor stores parsed path, frontmatter, list, link, and code
    /// region data. Attach inline fields and tags with
    /// [`Self::with_inline_fields`] and [`Self::with_tags`] after parser-owned
    /// extraction has finished.
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

    /// Parsed YAML frontmatter block, if present.
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

    /// Outgoing links extracted from markdown and wikilink syntax.
    #[inline]
    #[must_use]
    pub(crate) fn outlinks(&self) -> &[Outlink] {
        &self.outlinks
    }

    /// Source byte ranges excluded from inline metadata scanning.
    #[inline]
    #[must_use]
    pub(crate) fn code_regions(&self) -> &[CodeRegion] {
        &self.code_regions
    }

    /// Dataview-compatible inline fields from text blocks and list items, in
    /// document order.
    #[inline]
    #[must_use]
    pub(crate) fn inline_fields(&self) -> &[InlineField] {
        &self.inline_fields
    }

    /// Iterates over all metadata fields on this note.
    ///
    /// Frontmatter fields are yielded first, followed by body inline fields in
    /// document order.
    pub(crate) fn fields(&self) -> impl Iterator<Item = &MetadataField> {
        let empty: &[MetadataField] = &[];
        let frontmatter_fields =
            self.frontmatter.as_ref().map_or(empty, Frontmatter::fields);
        frontmatter_fields
            .iter()
            .chain(self.inline_fields.iter().map(InlineField::metadata))
    }

    /// Markdown tags (e.g. `#book`, `#projects/active`) extracted from
    /// paragraph and heading text and from list items, in document order.
    #[inline]
    #[must_use]
    pub(crate) fn tags(&self) -> &[Tag] {
        &self.tags
    }

    /// Iterates over all task list items, including nested sub-list items.
    pub(crate) fn tasks(&self) -> impl Iterator<Item = &ListItem> {
        let mut tasks = Vec::new();
        for list in &self.lists {
            collect_tasks_recursive(list, &mut tasks);
        }
        tasks.into_iter()
    }
}

/// Appends task items from `list` and its nested sub-lists to `acc`.
fn collect_tasks_recursive<'a>(list: &'a List, acc: &mut Vec<&'a ListItem>) {
    for item in list.items() {
        if item.is_task() {
            acc.push(item);
        }
        for child_list in item.children() {
            collect_tasks_recursive(child_list, acc);
        }
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    mod constructor {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn constructs_note_with_the_given_path_and_parts() {
            let frontmatter = Frontmatter::new(Vec::new());
            let list = List::new(false, vec![ListItem::new("item", None)]);
            let outlink = Outlink::new("target", "text", LinkType::Wikilink);
            let code_region = CodeRegion::new(3, 7);

            let note = Note::new(
                "notes/a.md",
                Some(frontmatter.clone()),
                vec![list.clone()],
                vec![outlink.clone()],
                vec![code_region],
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

    mod builder {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn with_inline_fields_attaches_the_given_fields() {
            let field = InlineField::new(
                "Status",
                FieldValue::String("Draft".to_owned()),
                InlineFieldForm::Body,
            );

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
    }

    mod fields {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn returns_empty_iterator_when_note_has_no_fields() {
            let note = Note::new(
                "notes/a.md",
                None,
                Vec::new(),
                Vec::new(),
                Vec::new(),
            );

            assert_eq!(note.fields().count(), 0);
        }

        #[test]
        fn yields_frontmatter_fields_before_inline_fields() {
            let frontmatter = Frontmatter::new(vec![MetadataField::new(
                "title",
                FieldValue::String("Note".to_owned()),
            )]);
            let inline_field = InlineField::new(
                "Status",
                FieldValue::String("Draft".to_owned()),
                InlineFieldForm::Body,
            );

            let note = Note::new(
                "notes/a.md",
                Some(frontmatter),
                Vec::new(),
                Vec::new(),
                Vec::new(),
            )
            .with_inline_fields(vec![inline_field]);

            let keys: Vec<&str> =
                note.fields().map(MetadataField::key).collect();
            assert_eq!(keys, ["title", "Status"]);
        }
    }

    mod tasks {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn yields_task_items_from_top_level_and_nested_lists_in_order() {
            let child_task =
                ListItem::new("child task", Some(TaskStatus::Complete));
            let parent = ListItem::with_children(
                "parent task",
                Some(TaskStatus::Incomplete),
                vec![List::new(false, vec![child_task])],
            );
            let plain = ListItem::new("plain item", None);
            let note = Note::new(
                "notes/a.md",
                None,
                vec![List::new(false, vec![parent, plain])],
                Vec::new(),
                Vec::new(),
            );

            let task_text: Vec<&str> =
                note.tasks().map(ListItem::text).collect();
            assert_eq!(task_text, ["parent task", "child task"]);
        }
    }
}
