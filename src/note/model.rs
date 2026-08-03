//! Parsed Markdown note records.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::{
    links::Link,
    lists::{List, ListItem},
    metadata::{Frontmatter, InlineField, MetadataField},
    tag::Tag,
};

/// Parsed metadata and structure for one Markdown note.
///
/// Stores page-level frontmatter, top-level lists, outgoing links, inline
/// fields, and tags. [`Self::tasks`] derives task items from stored lists
/// instead of duplicating them.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub(crate) struct Note {
    path: PathBuf,
    frontmatter: Option<Frontmatter>,
    lists: Vec<List>,
    outlinks: Vec<Link>,
    inline_fields: Vec<InlineField>,
    tags: Vec<Tag>,
}

impl Note {
    /// Creates a note from parser-owned page components.
    ///
    /// The new note starts without inline fields or tags because those are
    /// extracted after block parsing. Attach them with
    /// [`Self::with_inline_fields`] and [`Self::with_tags`].
    #[inline]
    #[must_use]
    pub(crate) fn new(
        path: impl Into<PathBuf>,
        frontmatter: Option<Frontmatter>,
        lists: Vec<List>,
        outlinks: Vec<Link>,
    ) -> Self {
        Self {
            path: path.into(),
            frontmatter,
            lists,
            outlinks,
            inline_fields: Vec::new(),
            tags: Vec::new(),
        }
    }

    /// Attaches `inline_fields` and returns the updated [`Note`].
    #[inline]
    #[must_use]
    pub(crate) fn with_inline_fields(
        mut self,
        inline_fields: Vec<InlineField>,
    ) -> Self {
        self.inline_fields = inline_fields;
        self
    }

    /// Attaches `tags` and returns the updated [`Note`].
    #[inline]
    #[must_use]
    pub(crate) fn with_tags(mut self, tags: Vec<Tag>) -> Self {
        self.tags = tags;
        self
    }

    /// Project-relative path to this note.
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

    /// Top-level body lists.
    ///
    /// Nested lists live under [`ListItem::children`]. Use [`Self::tasks`] for
    /// a flattened view of task items from every list depth.
    #[inline]
    #[must_use]
    pub(crate) fn lists(&self) -> &[List] {
        &self.lists
    }

    /// Outgoing links extracted from Markdown and wikilink syntax.
    #[inline]
    #[must_use]
    pub(crate) fn outlinks(&self) -> &[Link] {
        &self.outlinks
    }

    /// Dataview-compatible inline fields from text blocks and list items, in
    /// document order.
    #[inline]
    #[must_use]
    pub(crate) fn inline_fields(&self) -> &[InlineField] {
        &self.inline_fields
    }

    /// Iterates over frontmatter fields, then body inline fields.
    pub(crate) fn fields(&self) -> impl Iterator<Item = &MetadataField> {
        let empty: &[MetadataField] = &[];
        let frontmatter_fields =
            self.frontmatter.as_ref().map_or(empty, Frontmatter::fields);
        frontmatter_fields
            .iter()
            .chain(self.inline_fields.iter().map(InlineField::metadata))
    }

    /// Markdown tags from paragraphs, headings, and list items, in document
    /// order.
    #[inline]
    #[must_use]
    pub(crate) fn tags(&self) -> &[Tag] {
        &self.tags
    }

    /// Iterates over task list items at every list depth.
    pub(crate) fn tasks(&self) -> TaskIter<'_> {
        TaskIter::new(&self.lists)
    }
}

/// Depth-first iterator over task list items in a [`Note`].
pub(crate) struct TaskIter<'a> {
    stack: Vec<std::slice::Iter<'a, ListItem>>,
}

impl<'a> TaskIter<'a> {
    /// Starts depth-first iteration from the top-level `lists`.
    fn new(lists: &'a [List]) -> Self {
        let mut stack = Vec::with_capacity(lists.len());
        stack.extend(lists.iter().rev().map(|list| list.items().iter()));
        Self {
            stack,
        }
    }
}

impl<'a> Iterator for TaskIter<'a> {
    type Item = &'a ListItem;

    fn next(&mut self) -> Option<Self::Item> {
        while let Some(items) = self.stack.last_mut() {
            let Some(item) = items.next() else {
                self.stack.pop();
                continue;
            };
            self.stack.extend(
                item.children().iter().rev().map(|list| list.items().iter()),
            );
            if item.is_task() {
                return Some(item);
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {

    use super::*;
    use crate::note::{FieldValue, InlineFieldForm, LinkType, TaskStatus};

    mod constructor {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn constructs_note_with_the_given_path_and_parts() {
            let frontmatter = Frontmatter::new(Vec::new());
            let list = List::new(false, vec![ListItem::new("item", None)]);
            let outlink = Link::new("target", "text", LinkType::Wikilink);

            let note = Note::new(
                "notes/a.md",
                Some(frontmatter.clone()),
                vec![list.clone()],
                vec![outlink.clone()],
            );

            assert_eq!(note.path(), Path::new("notes/a.md"));
            assert_eq!(note.frontmatter(), Some(&frontmatter));
            assert_eq!(note.lists(), [list]);
            assert_eq!(note.outlinks(), [outlink]);
        }

        #[test]
        fn constructs_note_with_no_frontmatter_and_empty_collections() {
            let note = Note::new("notes/a.md", None, Vec::new(), Vec::new());

            assert_eq!(note.path(), Path::new("notes/a.md"));
            assert_eq!(note.frontmatter(), None);
            assert_eq!(note.lists().len(), 0);
            assert_eq!(note.outlinks().len(), 0);
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

            let note = Note::new("notes/a.md", None, Vec::new(), Vec::new())
                .with_inline_fields(vec![field.clone()]);

            assert_eq!(note.inline_fields(), [field]);
        }

        #[test]
        fn with_tags_attaches_the_given_tags() {
            let note = Note::new("notes/a.md", None, Vec::new(), Vec::new())
                .with_tags(vec![Tag::new("#book")]);

            assert_eq!(note.tags(), [Tag::new("#book")]);
        }
    }

    mod fields {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn returns_empty_iterator_when_note_has_no_fields() {
            let note = Note::new("notes/a.md", None, Vec::new(), Vec::new());

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
            )
            .with_inline_fields(vec![inline_field]);

            let keys: Vec<&str> =
                note.fields().map(|field| field.key().as_str()).collect();
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
            );

            let task_text: Vec<&str> =
                note.tasks().map(ListItem::text).collect();
            assert_eq!(task_text, ["parent task", "child task"]);
        }
    }
}
