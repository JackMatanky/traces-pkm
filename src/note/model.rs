//! Parsed Markdown note record.

use std::path::{Path, PathBuf};

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

use super::{
    field::NoteFieldValue,
    links::Link,
    lists::{List, ListItemIter},
    metadata::Frontmatter,
};
use crate::{FieldKey, FieldKeyRef, Tag};

/// A parsed Markdown note.
///
/// Stores page-level frontmatter, top-level lists, outgoing links, inline
/// fields, and tags. [`Self::tasks`] derives task items from stored lists
/// instead of duplicating them.
///
/// # Examples
///
/// ```rust
/// # #[cfg(feature = "test-utils")]
/// # {
/// use std::path::Path;
///
/// use traces_pkm::{MarkdownParserInput, parse_markdown};
///
/// let input = MarkdownParserInput::for_test(
///     Path::new("note.md"),
///     "# Title\n- [ ] Task",
/// );
/// let note = parse_markdown(&input);
/// assert_eq!(note.path(), Path::new("note.md"));
/// assert_eq!(note.tasks().count(), 1);
/// # }
/// ```
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct Note {
    #[serde(with = "crate::index::path")]
    path: PathBuf,
    frontmatter: Option<Frontmatter>,
    lists: Vec<List>,
    outlinks: Vec<Link>,
    inline_fields: IndexMap<FieldKey, Vec<NoteFieldValue>>,
    tags: Vec<Tag>,
}

impl Note {
    /// Creates a note from parser-owned page components.
    ///
    /// The new note starts without inline fields or tags because those are
    /// extracted after block parsing. Attach them with `with_inline_fields` and
    /// [`Self::with_tags`].
    #[inline]
    #[must_use]
    pub fn new<P: Into<PathBuf>>(
        path: P,
        frontmatter: Option<Frontmatter>,
        lists: Vec<List>,
        outlinks: Vec<Link>,
    ) -> Self {
        Self {
            path: path.into(),
            frontmatter,
            lists,
            outlinks,
            inline_fields: IndexMap::new(),
            tags: Vec::new(),
        }
    }

    /// Attaches `inline_fields` and returns the updated [`Note`].
    #[inline]
    #[must_use]
    pub(crate) fn with_inline_fields(
        mut self,
        inline_fields: IndexMap<FieldKey, Vec<NoteFieldValue>>,
    ) -> Self {
        self.inline_fields = inline_fields;
        self
    }

    /// Attaches `tags` and returns the updated [`Note`].
    #[inline]
    #[must_use]
    pub fn with_tags(mut self, tags: Vec<Tag>) -> Self {
        self.tags = tags;
        self
    }

    /// Returns the project-relative path to this note.
    ///
    /// # Examples
    ///
    /// ```rust
    /// # #[cfg(feature = "test-utils")]
    /// # {
    /// use std::path::Path;
    ///
    /// use traces_pkm::{MarkdownParserInput, parse_markdown};
    ///
    /// let input =
    ///     MarkdownParserInput::for_test(Path::new("docs/spec.md"), "# Spec");
    /// let note = parse_markdown(&input);
    /// assert_eq!(note.path(), Path::new("docs/spec.md"));
    /// # }
    /// ```
    #[inline]
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Returns the parsed YAML frontmatter block, if present.
    ///
    /// # Examples
    ///
    /// ```rust
    /// # #[cfg(feature = "test-utils")]
    /// # {
    /// use std::path::Path;
    ///
    /// use traces_pkm::{MarkdownParserInput, parse_markdown};
    ///
    /// let input = MarkdownParserInput::for_test(
    ///     Path::new("note.md"),
    ///     "---\ntitle: Hello\n---\nBody",
    /// );
    /// let note = parse_markdown(&input);
    /// assert!(note.frontmatter().is_some());
    /// # }
    /// ```
    #[inline]
    #[must_use]
    pub const fn frontmatter(&self) -> Option<&Frontmatter> {
        self.frontmatter.as_ref()
    }

    /// Returns the top-level body lists.
    ///
    /// Nested `ListItem` values hold child lists. Use [`Self::tasks`] for a
    /// flattened view of task items from every list depth.
    ///
    /// # Examples
    ///
    /// ```rust
    /// # #[cfg(feature = "test-utils")]
    /// # {
    /// use std::path::Path;
    ///
    /// use traces_pkm::{MarkdownParserInput, parse_markdown};
    ///
    /// let input = MarkdownParserInput::for_test(
    ///     Path::new("note.md"),
    ///     "- Item 1\n- Item 2",
    /// );
    /// let note = parse_markdown(&input);
    /// assert_eq!(note.lists().len(), 1);
    /// # }
    /// ```
    #[inline]
    #[must_use]
    #[cfg_attr(
        not(any(test, feature = "test-utils")),
        expect(
            dead_code,
            reason = "no current caller outside tests; kept for Note accessor \
                      symmetry with its fields"
        )
    )]
    pub fn lists(&self) -> &[List] {
        &self.lists
    }

    /// Returns the outgoing links extracted from Markdown and wikilink syntax.
    ///
    /// # Examples
    ///
    /// ```rust
    /// # #[cfg(feature = "test-utils")]
    /// # {
    /// use std::path::Path;
    ///
    /// use traces_pkm::{MarkdownParserInput, parse_markdown};
    ///
    /// let input = MarkdownParserInput::for_test(
    ///     Path::new("note.md"),
    ///     "Link to [[Other Note]]",
    /// );
    /// let note = parse_markdown(&input);
    /// assert_eq!(note.outlinks().len(), 1);
    /// # }
    /// ```
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
    ) -> &IndexMap<FieldKey, Vec<NoteFieldValue>> {
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
    ///
    /// # Examples
    ///
    /// ```rust
    /// # #[cfg(feature = "test-utils")]
    /// # {
    /// use std::path::Path;
    ///
    /// use traces_pkm::{MarkdownParserInput, parse_markdown};
    ///
    /// let input = MarkdownParserInput::for_test(
    ///     Path::new("note.md"),
    ///     "Tag #rust in text",
    /// );
    /// let note = parse_markdown(&input);
    /// assert_eq!(note.tags().len(), 1);
    /// # }
    /// ```
    #[inline]
    #[must_use]
    pub fn tags(&self) -> &[Tag] {
        &self.tags
    }

    /// Iterates over all list items across all nesting depths in document
    /// order.
    ///
    /// # Examples
    ///
    /// ```rust
    /// # #[cfg(feature = "test-utils")]
    /// # {
    /// use std::path::Path;
    ///
    /// use traces_pkm::{MarkdownParserInput, parse_markdown};
    ///
    /// let input = MarkdownParserInput::for_test(
    ///     Path::new("tasks.md"),
    ///     "- [ ] Top level task\n  - [x] Subtask\n- Plain bullet",
    /// );
    /// let note = parse_markdown(&input);
    /// assert_eq!(note.list_items().count(), 3);
    /// # }
    /// ```
    #[inline]
    #[must_use]
    pub fn list_items(&self) -> ListItemIter<'_> {
        ListItemIter::new(&self.lists)
    }

    /// Iterates over task list items across all nesting depths.
    ///
    /// # Examples
    ///
    /// ```rust
    /// # #[cfg(feature = "test-utils")]
    /// # {
    /// use std::path::Path;
    ///
    /// use traces_pkm::{MarkdownParserInput, parse_markdown};
    ///
    /// let input = MarkdownParserInput::for_test(
    ///     Path::new("tasks.md"),
    ///     "- [ ] Top level task\n  - [x] Subtask\n- Plain bullet",
    /// );
    /// let note = parse_markdown(&input);
    /// assert_eq!(note.tasks().count(), 2);
    /// # }
    /// ```
    #[inline]
    #[must_use]
    pub fn tasks(&self) -> ListItemIter<'_> {
        ListItemIter::tasks(&self.lists)
    }
}

#[cfg(test)]
mod tests {

    use indexmap::IndexMap;

    use super::*;
    use crate::{
        TaskStatus, TaskStatusSymbol, TaskStatusType,
        note::{
            LinkType, ListItem, ListItemType, NoteFieldValue, TaskDates,
            TaskListItem,
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
            let list = List::new(false, vec![ListItem::new(
                "item",
                ListItemType::Plain,
            )]);
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
            let key =
                FieldKey::try_new("Status").expect("valid test field key");
            let mut fields = IndexMap::new();
            fields
                .insert(key, vec![NoteFieldValue::String("Draft".to_owned())]);

            let note = Note::new("notes/a.md", None, Vec::new(), Vec::new())
                .with_inline_fields(fields.clone());

            assert_eq!(note.inline_fields(), &fields);
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
            let child_task = ListItem::new(
                "child task",
                task("Done", 'x', TaskStatusType::Done),
            );
            let parent = ListItem::with_children(
                "parent task",
                task("Todo", ' ', TaskStatusType::Todo),
                vec![List::new(false, vec![child_task])],
            );
            let plain = ListItem::new("plain item", ListItemType::Plain);
            let note = Note::new(
                "notes/a.md",
                None,
                vec![List::new(false, vec![parent, plain])],
                Vec::new(),
            );

            let task_text: Vec<&str> =
                note.tasks().map(ListItem::clean_text).collect();
            assert_eq!(task_text, ["parent task", "child task"]);
        }

        #[test]
        fn excludes_plain_and_checkbox_items() {
            let plain = ListItem::new("plain item", ListItemType::Plain);
            let checkbox =
                ListItem::new("checkbox item", ListItemType::Checkbox);
            let note = Note::new(
                "notes/a.md",
                None,
                vec![List::new(false, vec![plain, checkbox])],
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
            let grandchild_plain =
                ListItem::new("grandchild plain", ListItemType::Plain);
            let child_checkbox = ListItem::with_children(
                "child checkbox",
                ListItemType::Checkbox,
                vec![List::new(false, vec![grandchild_plain])],
            );
            let parent_task = ListItem::with_children(
                "parent task",
                task("Todo", ' ', TaskStatusType::Todo),
                vec![List::new(false, vec![child_checkbox])],
            );
            let sibling_task = ListItem::new(
                "sibling task",
                task("Done", 'x', TaskStatusType::Done),
            );
            let note = Note::new(
                "notes/a.md",
                None,
                vec![
                    List::new(false, vec![parent_task]),
                    List::new(false, vec![sibling_task]),
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
}
