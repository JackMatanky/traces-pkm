//! Parsed Markdown note record.

use std::path::{Path, PathBuf};

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

use super::{
    field::NoteFieldValue,
    links::Link,
    lists::{ListItem, descendants_of},
    metadata::Frontmatter,
};
use crate::{FieldKey, FieldKeyRef, Tag};

/// A parsed Markdown note.
///
/// Stores page-level frontmatter, top-level lists, outgoing links, inline
/// fields, and tags. [`Self::tasks`] derives task items from stored lists
/// instead of duplicating them.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct Note {
    #[serde(with = "crate::index::path")]
    path: PathBuf,
    frontmatter: Option<Frontmatter>,
    lists: Box<[ListItem]>,
    outlinks: Box<[Link]>,
    inline_fields: IndexMap<FieldKey, Box<[NoteFieldValue]>>,
    tags: Box<[Tag]>,
}

impl Note {
    /// Creates a note from parser-owned page components.
    ///
    /// The new note starts without inline fields or tags because those are
    /// extracted after block parsing. Attach them with `with_inline_fields` and
    /// [`Self::with_tags`].
    #[inline]
    #[must_use]
    pub(crate) fn new<
        P: Into<PathBuf>,
        L: Into<Box<[ListItem]>>,
        O: Into<Box<[Link]>>,
    >(
        path: P,
        frontmatter: Option<Frontmatter>,
        lists: L,
        outlinks: O,
    ) -> Self {
        Self {
            path: path.into(),
            frontmatter,
            lists: lists.into(),
            outlinks: outlinks.into(),
            inline_fields: IndexMap::new(),
            tags: Box::default(),
        }
    }

    /// Attaches `inline_fields` and returns the updated [`Note`].
    #[inline]
    #[must_use]
    pub(crate) fn with_inline_fields(
        mut self,
        inline_fields: IndexMap<FieldKey, Vec<NoteFieldValue>>,
    ) -> Self {
        self.inline_fields.clear();
        self.inline_fields.extend(
            inline_fields
                .into_iter()
                .map(|(key, values)| (key, values.into_boxed_slice())),
        );
        self
    }

    /// Attaches `tags` and returns the updated [`Note`].
    #[inline]
    #[must_use]
    pub(crate) fn with_tags<T: Into<Box<[Tag]>>>(mut self, tags: T) -> Self {
        self.tags = tags.into();
        self
    }

    /// Returns the project-relative path to this note.
    #[inline]
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Returns the parsed YAML frontmatter block, if present.
    #[inline]
    #[must_use]
    pub const fn frontmatter(&self) -> Option<&Frontmatter> {
        self.frontmatter.as_ref()
    }

    /// Returns all list items in strict document order.
    #[inline]
    #[must_use]
    pub fn lists(&self) -> &[ListItem] {
        &self.lists
    }

    /// Returns the outgoing links extracted from Markdown and wikilink syntax.
    #[inline]
    #[must_use]
    pub fn outlinks(&self) -> &[Link] {
        &self.outlinks
    }

    /// Returns inline fields parsed from text blocks and list items, in
    /// document order.
    #[inline]
    #[must_use]
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "no current caller outside tests; documented deliberate \
                      API in index-query#03's Note Accessor design, split \
                      from the fields() iterator that is used"
        )
    )]
    pub(crate) fn inline_fields(
        &self,
    ) -> &IndexMap<FieldKey, Box<[NoteFieldValue]>> {
        &self.inline_fields
    }

    /// Iterates over frontmatter fields, then body inline fields.
    ///
    /// Frontmatter keys take precedence: inline fields whose canonical key
    /// matches a frontmatter key are skipped. Returns borrowed keys to avoid
    /// cloning every [`FieldKey`] on each call.
    #[inline]
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "no current caller outside tests; retained for \
                      crate-internal accessor symmetry"
        )
    )]
    pub(crate) fn fields(
        &self,
    ) -> impl Iterator<Item = (&FieldKey, &NoteFieldValue)> {
        let fm_fields =
            self.frontmatter.iter().flat_map(|fm| fm.fields().iter());
        let inline = self
            .inline_fields
            .iter()
            .filter(|(k, _)| {
                !self.frontmatter.as_ref().is_some_and(|frontmatter| {
                    frontmatter.fields().contains_key(*k)
                })
            })
            .flat_map(|(k, values)| values.iter().map(move |v| (k, v)));

        fm_fields.chain(inline)
    }

    /// Returns the first value of a metadata field (frontmatter or inline)
    /// matching `key`, with frontmatter taking precedence.
    #[inline]
    #[must_use]
    pub(crate) fn get(&self, key: &str) -> Option<&NoteFieldValue> {
        if let Some(value) =
            self.frontmatter.as_ref().and_then(|fm| fm.get(key))
        {
            return Some(value);
        }
        self.inline_fields
            .get(&FieldKeyRef::new(key))
            .and_then(|values| values.first())
    }

    /// Returns Markdown tags from paragraphs, headings, and list items, in
    /// document order.
    #[inline]
    #[must_use]
    pub fn tags(&self) -> &[Tag] {
        &self.tags
    }

    /// Iterates over all list items in document order.
    #[inline]
    pub fn list_items(&self) -> impl Iterator<Item = &ListItem> {
        self.lists.iter()
    }

    /// Iterates over task list items in document order.
    #[inline]
    pub fn tasks(&self) -> impl Iterator<Item = &ListItem> {
        self.lists.iter().filter(|item| item.kind().is_task())
    }

    /// Returns an iterator over all descendant items of the list item at
    /// `parent_idx`.
    ///
    /// Scans contiguous items following `parent_idx` in document order whose
    /// depth is strictly greater than the parent's depth, stopping at the first
    /// sibling or ancestor item.
    #[inline]
    pub fn descendants(
        &self,
        parent_idx: usize,
    ) -> impl Iterator<Item = &ListItem> {
        let parent_depth =
            self.lists.get(parent_idx).map_or(u8::MAX, ListItem::depth);
        let rest =
            self.lists.get(parent_idx.saturating_add(1)..).unwrap_or(&[]);
        descendants_of(rest, parent_depth)
    }
}

#[cfg(test)]
mod tests {

    use indexmap::IndexMap;

    use super::*;
    use crate::{
        TaskDates, TaskStatus, TaskStatusSymbol, TaskStatusType,
        note::{
            LinkType, ListItem, ListItemType, NoteFieldValue, TaskListItem,
        },
    };

    fn task(name: &str, symbol: char, kind: TaskStatusType) -> ListItemType {
        ListItemType::Task(TaskListItem::new(
            TaskDates::default(),
            None,
            TaskStatus::new(TaskStatusSymbol::new(symbol), name, kind),
            true,
        ))
    }

    mod constructor {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn constructs_note_with_the_given_path_and_parts() {
            let frontmatter = Frontmatter::new(IndexMap::new());
            let item = ListItem::for_test("item", ListItemType::Plain);
            let outlink = Link::new("target", "text", LinkType::Wikilink);

            let note = Note::new(
                "notes/a.md",
                Some(frontmatter.clone()),
                vec![item.clone()],
                vec![outlink.clone()],
            );

            assert_eq!(note.path(), Path::new("notes/a.md"));
            assert_eq!(note.frontmatter(), Some(&frontmatter));
            assert_eq!(note.lists(), [item]);
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
            let key =
                FieldKey::try_new("Status").expect("valid test field key");
            let mut fields = IndexMap::new();
            fields.insert(key.clone(), vec![NoteFieldValue::String(
                "Draft".to_owned(),
            )]);

            let note = Note::new("notes/a.md", None, Vec::new(), Vec::new())
                .with_inline_fields(fields);

            let mut expected = IndexMap::new();
            expected.insert(
                key,
                vec![NoteFieldValue::String("Draft".to_owned())]
                    .into_boxed_slice(),
            );
            assert_eq!(note.inline_fields(), &expected);
        }
        #[test]
        fn with_tags_attaches_the_given_tags() {
            let note = Note::new("notes/a.md", None, Vec::new(), Vec::new())
                .with_tags(vec![Tag::parse("#book").unwrap()]);

            assert_eq!(note.tags(), [Tag::parse("#book").unwrap()]);
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
            let frontmatter = Frontmatter::new(IndexMap::from_iter([(
                FieldKey::try_new("title").expect("valid test field key"),
                NoteFieldValue::String("Note".to_owned()),
            )]));
            let key =
                FieldKey::try_new("Status").expect("valid test field key");
            let mut inline_fields = IndexMap::new();
            inline_fields
                .insert(key, vec![NoteFieldValue::String("Draft".to_owned())]);

            let note = Note::new(
                "notes/a.md",
                Some(frontmatter),
                Vec::new(),
                Vec::new(),
            )
            .with_inline_fields(inline_fields);

            let keys: Vec<String> =
                note.fields().map(|(k, _)| k.name().to_owned()).collect();
            assert_eq!(keys, ["title", "Status"]);
        }

        #[test]
        fn fields_dedup_frontmatter_over_inline() {
            let frontmatter = Frontmatter::new(IndexMap::from_iter([(
                FieldKey::try_new("status").unwrap(),
                NoteFieldValue::String("published".into()),
            )]));
            let mut inline_fields = IndexMap::new();
            inline_fields.insert(FieldKey::try_new("status").unwrap(), vec![
                NoteFieldValue::String("draft".into()),
            ]);
            let note =
                Note::new("notes/a.md", Some(frontmatter), vec![], vec![])
                    .with_inline_fields(inline_fields);

            let fields: Vec<_> = note.fields().collect();
            assert_eq!(fields.len(), 1);
            let first =
                fields.first().expect("fields has at least one element");
            assert_eq!(first.0, &FieldKey::try_new("status").unwrap());
            assert_eq!(first.1, &NoteFieldValue::String("published".into()));
        }
    }

    mod tasks {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn yields_task_items_from_top_level_and_nested_lists_in_order() {
            let parent = ListItem::for_test(
                "parent task",
                task("Todo", ' ', TaskStatusType::Todo),
            )
            .with_depth(0);
            let child_task = ListItem::for_test(
                "child task",
                task("Done", 'x', TaskStatusType::Done),
            )
            .with_depth(1);
            let plain = ListItem::for_test("plain item", ListItemType::Plain)
                .with_depth(0);
            let note = Note::new(
                "notes/a.md",
                None,
                vec![parent, child_task, plain],
                Vec::new(),
            );

            let task_text: Vec<&str> =
                note.tasks().map(ListItem::clean_text).collect();
            assert_eq!(task_text, ["parent task", "child task"]);
        }

        #[test]
        fn excludes_plain_and_checkbox_items() {
            let plain = ListItem::for_test("plain item", ListItemType::Plain);
            let checkbox =
                ListItem::for_test("checkbox item", ListItemType::Checkbox);
            let note = Note::new(
                "notes/a.md",
                None,
                vec![plain, checkbox],
                Vec::new(),
            );

            assert_eq!(note.tasks().count(), 0);
        }
    }

    mod list_items {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn yields_all_items_including_plain_and_checkbox_and_tasks_in_order() {
            let parent_task = ListItem::for_test(
                "parent task",
                task("Todo", ' ', TaskStatusType::Todo),
            )
            .with_depth(0);
            let child_checkbox =
                ListItem::for_test("child checkbox", ListItemType::Checkbox)
                    .with_depth(1);
            let grandchild_plain =
                ListItem::for_test("grandchild plain", ListItemType::Plain)
                    .with_depth(2);
            let sibling_task = ListItem::for_test(
                "sibling task",
                task("Done", 'x', TaskStatusType::Done),
            )
            .with_depth(0);
            let note = Note::new(
                "notes/a.md",
                None,
                vec![
                    parent_task,
                    child_checkbox,
                    grandchild_plain,
                    sibling_task,
                ],
                Vec::new(),
            );

            let texts: Vec<&str> =
                note.list_items().map(ListItem::clean_text).collect();
            assert_eq!(texts, [
                "parent task",
                "child checkbox",
                "grandchild plain",
                "sibling task"
            ]);
        }
    }

    mod serialization {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn preserves_populated_collections_across_postcard_roundtrip() {
            let key = FieldKey::try_new("status").expect("valid field key");
            let mut fm_fields = IndexMap::new();
            fm_fields.insert(
                key.clone(),
                NoteFieldValue::String("active".to_owned()),
            );
            let frontmatter = Frontmatter::new(fm_fields);

            let item_field_key =
                FieldKey::try_new("priority").expect("valid field key");
            let mut item_fields = IndexMap::new();
            item_fields.insert(item_field_key, vec![NoteFieldValue::String(
                "high".to_owned(),
            )]);
            let child = ListItem::for_test("child item", ListItemType::Plain)
                .with_depth(1);
            let item = ListItem::for_test("item", ListItemType::Plain)
                .with_fields(item_fields)
                .with_tags(vec![crate::parse_tag("#task")]);
            let outlink = Link::new("target", "text", LinkType::Wikilink);

            let mut inline_fields = IndexMap::new();
            inline_fields.insert(key, vec![NoteFieldValue::List(
                vec![NoteFieldValue::Number(1.0), NoteFieldValue::Number(2.0)]
                    .into(),
            )]);

            let note = Note::new(
                "notes/a.md",
                Some(frontmatter),
                vec![item, child],
                vec![outlink],
            )
            .with_inline_fields(inline_fields)
            .with_tags(vec![crate::parse_tag("#book")]);

            let bytes = postcard::to_allocvec(&note).expect("encode note");
            let decoded: Note =
                postcard::from_bytes(&bytes).expect("decode note");

            assert_eq!(decoded, note);
        }
    }
}
