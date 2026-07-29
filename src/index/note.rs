//! Note Metadata module: [`Note`] aggregate domain model and markdown parser.

mod inline;
mod metadata;
mod parser;
mod structure;
use std::path::{Path, PathBuf};

pub(crate) use metadata::{
    FieldSource, FieldValue, Frontmatter, InlineField, InlineFieldForm,
    MetadataField, RawFrontmatter,
};
pub(crate) use parser::parse_markdown;
use serde::{Deserialize, Serialize};
pub(crate) use structure::{
    CodeRegion, LinkType, List, ListItem, Outlink, Tag, TaskStatus,
};

/// Rich Note Metadata extracted from a markdown file: frontmatter, lists,
/// outlinks, code regions, Inline Fields, and tags. [`Self::tasks`] derives
/// task items from the indexed lists rather than storing them separately.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub(crate) struct Note {
    path: PathBuf,
    frontmatter: Option<Frontmatter>,
    lists: Vec<List>,
    outlinks: Vec<Outlink>,
    code_regions: Vec<CodeRegion>,
    inline_fields: Vec<MetadataField>,
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
        inline_fields: Vec<MetadataField>,
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

    /// Dataview-compatible Inline Fields extracted from paragraph and
    /// heading text and from list items, in document order.
    #[inline]
    #[must_use]
    pub(crate) fn inline_fields(&self) -> &[MetadataField] {
        &self.inline_fields
    }

    /// Combined iterator over all key-value metadata fields on this Note,
    /// yielding frontmatter fields first, followed by body inline fields
    /// in document order.
    pub(crate) fn fields(&self) -> impl Iterator<Item = &MetadataField> {
        let empty: &[MetadataField] = &[];
        let frontmatter_fields =
            self.frontmatter.as_ref().map_or(empty, Frontmatter::fields);
        frontmatter_fields.iter().chain(self.inline_fields.iter())
    }

    /// Markdown tags (e.g. `#book`, `#projects/active`) extracted from
    /// paragraph and heading text and from list items, in document order.
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
/// item to `acc`.
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
    use pretty_assertions::assert_eq;

    use super::{
        metadata::{FieldSource, FieldValue, InlineFieldForm},
        *,
    };
    use crate::index::LinkType;

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
        let note =
            Note::new("notes/a.md", None, Vec::new(), Vec::new(), Vec::new());
        assert_eq!(note.path(), Path::new("notes/a.md"));
        assert_eq!(note.frontmatter(), None);
        assert_eq!(note.lists().len(), 0);
        assert_eq!(note.outlinks().len(), 0);
        assert_eq!(note.code_regions().len(), 0);
    }

    #[test]
    fn builder_methods_attach_inline_fields_and_tags() {
        let field = MetadataField::new(
            "Status",
            FieldValue::String("Draft".to_string()),
            FieldSource::Body(InlineFieldForm::Body),
        );

        let note =
            Note::new("notes/a.md", None, Vec::new(), Vec::new(), Vec::new())
                .with_inline_fields(vec![field.clone()])
                .with_tags(vec![Tag::new("#book")]);

        assert_eq!(note.inline_fields(), [field]);
        assert_eq!(note.tags(), [Tag::new("#book")]);
    }
}
