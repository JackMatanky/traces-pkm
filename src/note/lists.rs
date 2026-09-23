//! Markdown list items and task-list structures.
//!
//! This module defines the flat list-item data model. Items are stored in
//! strict document order inside a [`Note`](crate::Note); hierarchy is
//! reconstructed from each item's `depth` and `parent` source line rather
//! than from child containers.
//!
//! # Key Types
//!
//! - [`ListItem`]: A list item with a classified [`ListItemType`], inline
//!   fields, tags, and source positioning.
//! - [`ListItemType`]: Classification of an item as a plain bullet, a checkbox,
//!   or a task carrying a [`TaskListItem`].
//! - [`TaskListItem`]: Task-specific metadata (resolved status, priority,
//!   dates, and precomputed subtree completion state) carried by
//!   [`ListItemType::Task`].
//! - [`ListText`]: Dual-representation text container maintaining both raw
//!   source and clean display text.
//! - [`descendants_of`]: Contiguous slice scan yielding the items following a
//!   parent within a document-order slice.

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

use super::field::NoteFieldValue;
use crate::{FieldKey, SourceLine, Tag, TaskDates, TaskPriority, TaskStatus};
/// Compact inline field map for a list item.
pub(crate) type ListFieldMap = IndexMap<FieldKey, Box<[NoteFieldValue]>>;

/// A Markdown list item with a classified [`ListItemType`], inline fields,
/// tags, and source line positioning information.
//
// NOTE: `Eq` is deliberately not derived. `ListFieldMap` contains
// `NoteFieldValue`, whose `Number` variant holds `f64`, so `Eq` is
// unrepresentable on this struct and the clippy nursery lint
// `derive_partial_eq_without_eq` (visible only under `-W clippy::nursery`)
// flags it in error — its suggestion does not compile.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct ListItem {
    text: ListText,
    kind: ListItemType,
    depth: u8,
    line: SourceLine,
    parent: Option<SourceLine>,
    is_ordered: bool,
    fields: Option<Box<ListFieldMap>>,
    tags: Box<[Tag]>,
}

impl ListItem {
    /// Creates a list item with its source line and classification.
    #[inline]
    #[must_use]
    pub fn new<T: Into<ListText>>(
        line: SourceLine,
        text: T,
        kind: ListItemType,
    ) -> Self {
        Self {
            text: text.into(),
            kind,
            depth: 0,
            line,
            parent: None,
            is_ordered: false,
            fields: None,
            tags: Box::default(),
        }
    }

    /// Creates a test list item with a default source line
    /// ([`SourceLine::MIN`]).
    #[cfg(any(test, feature = "test-utils"))]
    #[inline]
    #[must_use]
    pub fn for_test<T: Into<ListText>>(text: T, kind: ListItemType) -> Self {
        Self::new(SourceLine::MIN, text, kind)
    }

    /// Attaches the item's 0-indexed nesting depth.
    #[inline]
    #[must_use]
    pub fn with_depth(mut self, depth: u8) -> Self {
        self.depth = depth;
        self
    }

    /// Attaches the item's immediate parent's 1-indexed source line.
    #[inline]
    #[must_use]
    pub fn with_parent(mut self, parent: Option<SourceLine>) -> Self {
        self.parent = parent;
        self
    }

    /// Attaches whether the item belongs to an ordered list.
    #[inline]
    #[must_use]
    pub fn with_is_ordered(mut self, is_ordered: bool) -> Self {
        self.is_ordered = is_ordered;
        self
    }

    /// Attaches inline fields parsed from this item's own text.
    ///
    /// Stores `None` when `fields` is empty to avoid heap-allocating an empty
    /// map.
    #[inline]
    #[must_use]
    pub(crate) fn with_fields(
        mut self,
        fields: IndexMap<FieldKey, Vec<NoteFieldValue>>,
    ) -> Self {
        self.fields = if fields.is_empty() {
            None
        } else {
            let boxed: IndexMap<FieldKey, Box<[NoteFieldValue]>> = fields
                .into_iter()
                .map(|(key, values)| (key, values.into_boxed_slice()))
                .collect();
            Some(Box::new(boxed))
        };
        self
    }

    /// Attaches tags scanned from this item's own text.
    ///
    /// Uses the same tag-token lexer that scans note body text; these are the
    /// same tags already consulted for task tag filter classification,
    /// resurfaced here as queryable data rather than discarded after
    /// classification decides.
    #[inline]
    #[must_use]
    pub(crate) fn with_tags<T: Into<Box<[Tag]>>>(mut self, tags: T) -> Self {
        self.tags = tags.into();
        self
    }

    /// Returns this item's own tags, scanned from its text.
    ///
    /// Does not include tags from child items or inherited Note-level tags.
    #[inline]
    #[must_use]
    pub fn tags(&self) -> &[Tag] {
        &self.tags
    }

    /// Returns the plain or normalized text representation holding both raw and
    /// clean variants.
    #[inline]
    #[must_use]
    pub fn text(&self) -> &ListText {
        &self.text
    }

    /// Returns the raw text with only the leading marker prefix stripped.
    ///
    /// Retains tags, dates, priority emojis, and inline fields.
    #[inline]
    #[must_use]
    pub fn raw_text(&self) -> &str {
        self.text.raw()
    }

    /// Returns the normalized clean text with task metadata stripped.
    ///
    /// Strips configured task tag filters, date shorthand syntax, priority
    /// emojis, and inline task fields.
    #[inline]
    #[must_use]
    pub fn clean_text(&self) -> &str {
        self.text.clean()
    }

    /// Returns this item's classification: plain bullet, checkbox, or Task.
    #[inline]
    #[must_use]
    pub const fn kind(&self) -> &ListItemType {
        &self.kind
    }

    /// Returns the inline fields parsed from this item's own text, or `None` if
    /// the item carries no inline fields.
    #[inline]
    #[must_use]
    pub(crate) fn fields(&self) -> Option<&ListFieldMap> {
        self.fields.as_deref()
    }

    /// Returns the item's 0-indexed nesting level.
    #[inline]
    #[must_use]
    pub const fn depth(&self) -> u8 {
        self.depth
    }

    /// Returns the item's 1-indexed source line.
    #[inline]
    #[must_use]
    pub const fn line(&self) -> SourceLine {
        self.line
    }

    /// Returns the immediate parent list item's 1-indexed source line, if this
    /// item is nested inside another list item.
    #[inline]
    #[must_use]
    pub const fn parent(&self) -> Option<SourceLine> {
        self.parent
    }

    /// Returns `true` if this item is part of an ordered list.
    #[inline]
    #[must_use]
    pub const fn is_ordered(&self) -> bool {
        self.is_ordered
    }
}

/// Classification of a Markdown list item.
///
/// List items are classified during parsing based on leading marker syntax and
/// configured task tag filters:
///
/// - [`Self::Plain`]: Standard bullet or numbered item with no checkbox marker.
/// - [`Self::Checkbox`]: Status-marked item that did not match configured task
///   tag filters. Checkboxes carry no task-specific metadata and are excluded
///   from [`super::Note::tasks`].
/// - [`Self::Task`]: Status-marked item classified as an active task, carrying
///   an encapsulated [`TaskListItem`].
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub enum ListItemType {
    /// A plain bullet with no marker.
    Plain,
    /// A status-marked item that did not match a configured task tag filter.
    Checkbox,
    /// A status-marked item classified as a Task, carrying its task data.
    Task(TaskListItem),
}

impl ListItemType {
    /// Returns `true` if this list item is classified as a Task.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use traces_pkm::{ListItemType, TaskDates, TaskListItem, TaskStatus};
    ///
    /// let kind = ListItemType::Task(TaskListItem::new(
    ///     TaskDates::default(),
    ///     None,
    ///     TaskStatus::default(),
    ///     false,
    /// ));
    /// assert!(kind.is_task());
    /// assert!(!kind.is_plain());
    /// ```
    #[inline]
    #[must_use]
    pub const fn is_task(&self) -> bool {
        matches!(self, Self::Task(_))
    }

    /// Returns this item's [`TaskListItem`] data, or [`None`] if this list item
    /// is not classified as a Task.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use traces_pkm::{ListItemType, TaskDates, TaskListItem, TaskStatus};
    ///
    /// let kind = ListItemType::Task(TaskListItem::new(
    ///     TaskDates::default(),
    ///     None,
    ///     TaskStatus::default(),
    ///     false,
    /// ));
    /// assert!(kind.as_task().is_some());
    /// assert!(ListItemType::Plain.as_task().is_none());
    /// ```
    #[inline]
    #[must_use]
    pub const fn as_task(&self) -> Option<&TaskListItem> {
        match self {
            Self::Task(task) => Some(task),
            Self::Plain | Self::Checkbox => None,
        }
    }

    /// Returns `true` if this list item is classified as a Checkbox.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use traces_pkm::ListItemType;
    ///
    /// let kind = ListItemType::Checkbox;
    /// assert!(kind.is_checkbox());
    /// assert!(!kind.is_task());
    /// ```
    #[inline]
    #[must_use]
    pub const fn is_checkbox(&self) -> bool {
        matches!(self, Self::Checkbox)
    }

    /// Returns `true` if this list item is classified as a plain bullet.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use traces_pkm::ListItemType;
    ///
    /// let kind = ListItemType::Plain;
    /// assert!(kind.is_plain());
    /// assert!(!kind.is_task());
    /// ```
    #[inline]
    #[must_use]
    pub const fn is_plain(&self) -> bool {
        matches!(self, Self::Plain)
    }
}

/// Task-specific data carried by a [`ListItemType::Task`] item.
///
/// Encapsulates extracted task lifecycle dates ([`TaskDates`]), an optional
/// priority ([`TaskPriority`]), the resolved [`TaskStatus`], and a precomputed
/// boolean flag indicating whether the entire task subtree is complete.
///
/// # Examples
///
/// ```rust
/// use traces_pkm::{TaskDates, TaskListItem, TaskPriority, TaskStatus};
///
/// let task = TaskListItem::new(
///     TaskDates::default(),
///     Some(TaskPriority::Highest),
///     TaskStatus::default(),
///     true,
/// );
/// assert_eq!(task.priority(), Some(TaskPriority::Highest));
/// assert!(task.is_fully_complete());
/// ```
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct TaskListItem {
    dates: TaskDates,
    priority: Option<TaskPriority>,
    status: TaskStatus,
    fully_complete: bool,
}

impl TaskListItem {
    /// Creates a task list item with its dates, priority, resolved status, and
    /// precomputed fully-complete state.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use traces_pkm::{TaskDates, TaskListItem, TaskPriority, TaskStatus};
    ///
    /// let task = TaskListItem::new(
    ///     TaskDates::default(),
    ///     Some(TaskPriority::Low),
    ///     TaskStatus::default(),
    ///     false,
    /// );
    /// assert_eq!(task.priority(), Some(TaskPriority::Low));
    /// ```
    #[inline]
    #[must_use]
    pub const fn new(
        dates: TaskDates,
        priority: Option<TaskPriority>,
        status: TaskStatus,
        fully_complete: bool,
    ) -> Self {
        Self {
            dates,
            priority,
            status,
            fully_complete,
        }
    }

    /// Returns the task's resolved status (marker symbol, display name, and
    /// workflow type).
    ///
    /// # Examples
    ///
    /// ```rust
    /// use traces_pkm::{TaskDates, TaskListItem, TaskStatus};
    ///
    /// let task = TaskListItem::new(
    ///     TaskDates::default(),
    ///     None,
    ///     TaskStatus::default(),
    ///     true,
    /// );
    /// assert_eq!(task.status().symbol(), ' ');
    /// ```
    #[inline]
    #[must_use]
    pub const fn status(&self) -> &TaskStatus {
        &self.status
    }

    /// Returns `true` if all descendant tasks in this item's subtree are
    /// resolved (done or cancelled), or if this item has no descendant tasks.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use traces_pkm::{TaskDates, TaskListItem, TaskStatus};
    ///
    /// let task = TaskListItem::new(
    ///     TaskDates::default(),
    ///     None,
    ///     TaskStatus::default(),
    ///     true,
    /// );
    /// assert!(task.is_fully_complete());
    /// ```
    #[inline]
    #[must_use]
    pub const fn is_fully_complete(&self) -> bool {
        self.fully_complete
    }

    /// Returns the task's priority, or [`None`] if no priority was specified.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use traces_pkm::{TaskDates, TaskListItem, TaskPriority, TaskStatus};
    ///
    /// let task = TaskListItem::new(
    ///     TaskDates::default(),
    ///     Some(TaskPriority::Medium),
    ///     TaskStatus::default(),
    ///     false,
    /// );
    /// assert_eq!(task.priority(), Some(TaskPriority::Medium));
    /// ```
    #[inline]
    #[must_use]
    pub const fn priority(&self) -> Option<TaskPriority> {
        self.priority
    }

    /// Returns the task's dates.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use traces_pkm::{TaskDates, TaskListItem, TaskStatus};
    ///
    /// let task = TaskListItem::new(
    ///     TaskDates::default(),
    ///     None,
    ///     TaskStatus::default(),
    ///     false,
    /// );
    /// assert!(task.dates().is_empty());
    /// ```
    #[inline]
    #[must_use]
    pub const fn dates(&self) -> TaskDates {
        self.dates
    }
}

/// Text representation of a list item holding both raw source-like text and
/// cleaned display text.
///
/// - `raw`: Source text minus the leading `[<char>] ` marker prefix only. All
///   other inline syntax (tags, date syntax, priority emojis, inline fields) is
///   preserved.
/// - `clean`: Normalized text with task marker, configured task tag filters,
///   date syntax, priority emojis, and inline task fields stripped.
///
/// # Examples
///
/// ```rust
/// use traces_pkm::ListText;
///
/// let text = ListText::new("Buy milk 📅 2025-01-15", "Buy milk");
/// assert_eq!(text.raw(), "Buy milk 📅 2025-01-15");
/// assert_eq!(text.clean(), "Buy milk");
/// ```
#[derive(
    Clone, Debug, Default, Eq, Hash, PartialEq, Deserialize, Serialize,
)]
pub struct ListText {
    /// Source text minus the leading `[<char>] ` marker prefix only.
    raw: String,
    /// Normalized display text with task metadata stripped, or `None` when
    /// identical to `raw`.
    clean: Option<String>,
}

impl ListText {
    /// Creates a new [`Self`] from raw and clean text representations.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use traces_pkm::ListText;
    ///
    /// let text = ListText::new("Task 📅 2025-01-15", "Task");
    /// assert_eq!(text.clean(), "Task");
    /// ```
    #[inline]
    #[must_use]
    pub fn new<R: Into<String>, C: Into<String>>(raw: R, clean: C) -> Self {
        let raw = raw.into();
        let clean = clean.into();
        let clean = if clean == raw {
            None
        } else {
            Some(clean)
        };
        Self {
            raw,
            clean,
        }
    }

    /// Returns the raw text with only the leading marker prefix stripped.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use traces_pkm::ListText;
    ///
    /// let text = ListText::new("Task 📅 2025-01-15", "Task");
    /// assert_eq!(text.raw(), "Task 📅 2025-01-15");
    /// ```
    #[inline]
    #[must_use]
    pub fn raw(&self) -> &str {
        &self.raw
    }

    /// Returns the normalized clean text suitable for display and queries.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use traces_pkm::ListText;
    ///
    /// let text = ListText::new("Task 📅 2025-01-15", "Task");
    /// assert_eq!(text.clean(), "Task");
    /// ```
    #[inline]
    #[must_use]
    pub fn clean(&self) -> &str {
        self.clean.as_deref().unwrap_or(&self.raw)
    }
}
impl std::fmt::Display for ListText {
    #[inline]
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.clean())
    }
}

impl From<&str> for ListText {
    #[inline]
    fn from(s: &str) -> Self {
        Self {
            raw: s.to_owned(),
            clean: None,
        }
    }
}

impl From<String> for ListText {
    #[inline]
    fn from(s: String) -> Self {
        Self {
            raw: s,
            clean: None,
        }
    }
}

impl From<(&str, &str)> for ListText {
    #[inline]
    fn from((raw, clean): (&str, &str)) -> Self {
        Self::new(raw, clean)
    }
}

impl From<(String, String)> for ListText {
    #[inline]
    fn from((raw, clean): (String, String)) -> Self {
        Self::new(raw, clean)
    }
}
impl AsRef<str> for ListText {
    #[inline]
    fn as_ref(&self) -> &str {
        &self.raw
    }
}

impl PartialEq<str> for ListText {
    #[inline]
    fn eq(&self, other: &str) -> bool {
        self.raw == other
    }
}

impl PartialEq<&str> for ListText {
    #[inline]
    fn eq(&self, other: &&str) -> bool {
        self.raw == *other
    }
}

impl PartialEq<ListText> for str {
    #[inline]
    fn eq(&self, other: &ListText) -> bool {
        self == other.raw
    }
}

impl PartialEq<ListText> for &str {
    #[inline]
    fn eq(&self, other: &ListText) -> bool {
        *self == other.raw
    }
}

/// Returns an iterator over all descendant items of a list item with
/// `parent_depth` from the following items in `slice`.
///
/// Scans contiguous items in document order whose depth is strictly greater
/// than `parent_depth`, stopping at the first sibling or ancestor item.
#[inline]
pub(crate) fn descendants_of(
    slice: &[ListItem],
    parent_depth: u8,
) -> impl Iterator<Item = &ListItem> {
    slice.iter().take_while(move |child| child.depth() > parent_depth)
}

#[cfg(test)]
mod tests {
    use chrono::NaiveDate;
    use rstest::rstest;

    use super::*;
    use crate::{TaskStatusSymbol, TaskStatusType};

    fn done_task() -> ListItemType {
        ListItemType::Task(TaskListItem::new(
            TaskDates::default(),
            None,
            TaskStatus::new(
                TaskStatusSymbol::new('x'),
                "Done",
                TaskStatusType::Done,
            ),
            true,
        ))
    }

    mod list_item {
        use super::*;

        mod constructor {
            use pretty_assertions::assert_eq;

            use super::*;
            #[rstest]
            #[case::plain(ListItemType::Plain)]
            #[case::checkbox(ListItemType::Checkbox)]
            #[case::task(done_task())]
            fn preserves_the_constructed_item_kind(#[case] kind: ListItemType) {
                let item = ListItem::for_test("task item", kind.clone());

                assert_eq!(item.kind(), &kind);
            }
        }

        mod fields {
            use pretty_assertions::assert_eq;

            use super::*;
            use crate::NoteFieldValue;

            #[test]
            fn stores_fields_when_attached_with_with_fields() {
                let key = FieldKey::try_new("priority")
                    .expect("valid test field key");
                let mut fields = IndexMap::new();
                fields.insert(key.clone(), vec![NoteFieldValue::String(
                    "high".to_owned(),
                )]);
                let item = ListItem::for_test("task item", done_task())
                    .with_fields(fields);
                let mut expected = IndexMap::new();
                expected.insert(
                    key,
                    vec![NoteFieldValue::String("high".to_owned())]
                        .into_boxed_slice(),
                );
                assert_eq!(item.fields(), Some(&expected));
            }
            #[test]
            fn has_no_fields_by_default() {
                let item =
                    ListItem::for_test("plain item", ListItemType::Plain);

                assert_eq!(item.fields(), None);
            }

            #[test]
            fn drops_an_explicit_empty_field_map() {
                let item =
                    ListItem::for_test("plain item", ListItemType::Plain)
                        .with_fields(IndexMap::new());

                assert_eq!(item.fields(), None);
            }
        }

        mod tags {
            use pretty_assertions::assert_eq;

            use super::*;

            #[test]
            fn stores_tags_when_attached_with_with_tags() {
                let tags = vec![Tag::parse("#project").expect("valid tag")];
                let item = ListItem::for_test("task item", done_task())
                    .with_tags(tags.clone());

                assert_eq!(item.tags(), tags.as_slice());
            }

            #[test]
            fn has_no_tags_by_default() {
                let item =
                    ListItem::for_test("plain item", ListItemType::Plain);

                assert_eq!(item.tags(), []);
            }
        }

        mod position {
            use pretty_assertions::assert_eq;

            use super::*;
            #[test]
            fn defaults_position_to_zero_and_no_parent() {
                let item = ListItem::new(
                    SourceLine::new(5).expect("non-zero"),
                    "item",
                    ListItemType::Plain,
                );

                assert_eq!(item.depth(), 0);
                assert_eq!(item.line(), SourceLine::new(5).expect("non-zero"));
                assert_eq!(item.parent(), None);
                assert!(!item.is_ordered());
            }

            #[test]
            fn builders_set_depth_parent_and_ordering() {
                let item = ListItem::new(
                    SourceLine::new(3).expect("non-zero"),
                    "item",
                    ListItemType::Plain,
                )
                .with_depth(2)
                .with_parent(Some(SourceLine::new(1).expect("non-zero")))
                .with_is_ordered(true);

                assert_eq!(item.line(), SourceLine::new(3).expect("non-zero"));
                assert_eq!(item.depth(), 2);
                assert_eq!(
                    item.parent(),
                    Some(SourceLine::new(1).expect("non-zero"))
                );
                assert!(item.is_ordered());
            }
        }
    }

    mod descendants {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn yields_all_descendant_items_whose_depth_is_greater() {
            let parent = ListItem::new(
                SourceLine::new(1).expect("non-zero"),
                "parent",
                ListItemType::Plain,
            )
            .with_depth(0);
            let child1 = ListItem::new(
                SourceLine::new(2).expect("non-zero"),
                "child 1",
                ListItemType::Plain,
            )
            .with_depth(1)
            .with_parent(Some(SourceLine::new(1).expect("non-zero")));
            let grandchild = ListItem::new(
                SourceLine::new(3).expect("non-zero"),
                "grandchild",
                ListItemType::Plain,
            )
            .with_depth(2)
            .with_parent(Some(SourceLine::new(2).expect("non-zero")));
            let child2 = ListItem::new(
                SourceLine::new(4).expect("non-zero"),
                "child 2",
                ListItemType::Plain,
            )
            .with_depth(1)
            .with_parent(Some(SourceLine::new(1).expect("non-zero")));
            let sibling = ListItem::for_test("sibling", ListItemType::Plain)
                .with_depth(0);

            let slice = [parent, child1, grandchild, child2, sibling];
            let desc: Vec<&str> = descendants_of(&slice[1..], 0)
                .map(ListItem::clean_text)
                .collect();
            assert_eq!(desc, ["child 1", "grandchild", "child 2"]);

            let child1_desc: Vec<&str> = descendants_of(&slice[2..], 1)
                .map(ListItem::clean_text)
                .collect();
            assert_eq!(child1_desc, ["grandchild"]);
        }

        #[test]
        fn returns_empty_iterator_when_no_descendants_exist() {
            let parent =
                ListItem::for_test("parent", ListItemType::Plain).with_depth(0);
            let sibling = ListItem::for_test("sibling", ListItemType::Plain)
                .with_depth(0);
            let slice = [parent, sibling];
            assert_eq!(descendants_of(&slice[1..], 0).count(), 0);
        }

        #[test]
        fn returns_no_items_for_an_empty_slice() {
            assert_eq!(descendants_of(&[], 0).count(), 0);
        }

        #[test]
        fn includes_items_when_depth_skips_a_level() {
            let parent =
                ListItem::for_test("parent", ListItemType::Plain).with_depth(0);
            let grandchild =
                ListItem::for_test("grandchild", ListItemType::Plain)
                    .with_depth(2)
                    .with_parent(Some(SourceLine::new(1).expect("non-zero")));
            let sibling = ListItem::for_test("sibling", ListItemType::Plain)
                .with_depth(0);
            let slice = [parent, grandchild, sibling];

            let desc: Vec<&str> = descendants_of(&slice[1..], 0)
                .map(ListItem::clean_text)
                .collect();
            assert_eq!(desc, ["grandchild"]);
        }
    }
    mod task_list_item {
        use super::*;

        mod constructor {
            use pretty_assertions::assert_eq;

            use super::*;
            #[test]
            fn stores_status_and_fully_complete_flag_and_priority_and_dates() {
                let status = TaskStatus::new(
                    TaskStatusSymbol::new('x'),
                    "Done",
                    TaskStatusType::Done,
                );
                let dates = TaskDates::new(
                    NaiveDate::from_ymd_opt(2025, 1, 1).map(Into::into),
                    None,
                    None,
                    NaiveDate::from_ymd_opt(2025, 1, 15).map(Into::into),
                    None,
                    None,
                );
                let item = TaskListItem::new(
                    dates,
                    Some(TaskPriority::High),
                    status.clone(),
                    true,
                );
                assert_eq!(item.status(), &status);
                assert_eq!(item.is_fully_complete(), true);
                assert_eq!(item.priority(), Some(TaskPriority::High));
                assert_eq!(item.dates(), dates);
            }
        }

        mod accessors {
            use pretty_assertions::assert_eq;

            use super::*;
            #[test]
            fn returns_status_reference() {
                let status = TaskStatus::new(
                    TaskStatusSymbol::new('/'),
                    "In Progress",
                    TaskStatusType::InProgress,
                );
                let item = TaskListItem::new(
                    TaskDates::default(),
                    None,
                    status.clone(),
                    false,
                );

                assert_eq!(item.status(), &status);
            }

            #[test]
            fn reports_whether_the_task_subtree_is_fully_complete() {
                let status = TaskStatus::new(
                    TaskStatusSymbol::new(' '),
                    "Todo",
                    TaskStatusType::Todo,
                );
                let item = TaskListItem::new(
                    TaskDates::default(),
                    None,
                    status,
                    false,
                );

                assert_eq!(item.is_fully_complete(), false);
            }

            #[test]
            fn returns_priority_when_configured() {
                let status = TaskStatus::new(
                    TaskStatusSymbol::new(' '),
                    "Todo",
                    TaskStatusType::Todo,
                );
                let item = TaskListItem::new(
                    TaskDates::default(),
                    Some(TaskPriority::Highest),
                    status,
                    false,
                );
                let priority = item.priority();

                assert_eq!(priority, Some(TaskPriority::Highest));
            }

            #[test]
            fn returns_none_when_priority_is_not_configured() {
                let status = TaskStatus::new(
                    TaskStatusSymbol::new(' '),
                    "Todo",
                    TaskStatusType::Todo,
                );
                let item = TaskListItem::new(
                    TaskDates::default(),
                    None,
                    status,
                    false,
                );
                let priority = item.priority();

                assert_eq!(priority, None);
            }

            #[test]
            fn returns_dates() {
                let status = TaskStatus::new(
                    TaskStatusSymbol::new(' '),
                    "Todo",
                    TaskStatusType::Todo,
                );
                let dates = TaskDates::new(
                    None,
                    None,
                    None,
                    NaiveDate::from_ymd_opt(2025, 2, 1).map(Into::into),
                    None,
                    None,
                );
                let item = TaskListItem::new(dates, None, status, false);

                assert_eq!(item.dates(), dates);
                assert_eq!(
                    item.dates().due(),
                    NaiveDate::from_ymd_opt(2025, 2, 1).map(Into::into)
                );
            }
        }
    }

    mod list_text {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn returns_distinct_raw_and_clean_text() {
            let text = ListText::new("raw text", "clean text");

            assert_eq!(text.raw(), "raw text");
            assert_eq!(text.clean(), "clean text");
        }

        #[test]
        fn displays_clean_text() {
            let text = ListText::new("raw text", "clean text");

            assert_eq!(format!("{text}"), "clean text");
        }

        #[test]
        fn uses_raw_text_when_clean_text_matches_raw_text() {
            let text = ListText::new("same", "same");

            assert_eq!(text.clean(), "same");
        }

        #[test]
        fn converts_a_text_pair_to_raw_and_clean_text() {
            let from_str: ListText = "plain".into();
            assert_eq!(from_str.raw(), "plain");
            assert_eq!(from_str.clean(), "plain");

            let from_tuple: ListText = ("raw", "clean").into();
            assert_eq!(from_tuple.raw(), "raw");
            assert_eq!(from_tuple.clean(), "clean");
        }
    }
}
