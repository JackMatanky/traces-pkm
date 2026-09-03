//! Markdown list, list item, and task-list structures.
//!
//! - [`List`]: an ordered or unordered Markdown list.
//! - [`ListItem`]: a list item with a classified [`ListItemType`], inline
//!   fields, and child lists.
//! - [`ListItemType`]: whether a list item is a plain bullet, a checkbox, or a
//!   Task carrying a resolved [`TaskStatus`].
//! - [`ListItemPosition`]: a list item's nesting depth, source line, and parent
//!   line.

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

use super::metadata::NoteFieldValue;
use crate::{FieldKey, SourceLine, task::TaskStatus};

/// An ordered or unordered Markdown list.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct List {
    is_ordered: bool,
    items: Vec<ListItem>,
}

impl List {
    /// Creates a list from its ordering flag and direct child items.
    #[inline]
    #[must_use]
    pub(crate) const fn new(is_ordered: bool, items: Vec<ListItem>) -> Self {
        Self {
            is_ordered,
            items,
        }
    }

    /// Returns `true` if this is an ordered list.
    #[inline]
    #[must_use]
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "no current caller outside tests; kept for List accessor \
                      symmetry with its fields"
        )
    )]
    pub(crate) const fn is_ordered(&self) -> bool {
        self.is_ordered
    }

    /// Returns the direct child items in this list.
    #[inline]
    #[must_use]
    pub fn items(&self) -> &[ListItem] {
        &self.items
    }
}
/// A Markdown list item with a classified [`ListItemType`], child lists, and
/// inline fields.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct ListItem {
    text: String,
    item_type: ListItemType,
    children: Vec<List>,
    fields: IndexMap<FieldKey, Vec<NoteFieldValue>>,
    position: ListItemPosition,
}

impl ListItem {
    /// Creates a list item without child lists.
    #[inline]
    #[must_use]
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "no current caller outside tests; kept for ListItem \
                      constructor symmetry with with_children"
        )
    )]
    pub(crate) fn new(
        text: impl Into<String>,
        item_type: ListItemType,
    ) -> Self {
        Self {
            text: text.into(),
            item_type,
            children: Vec::new(),
            fields: IndexMap::new(),
            position: ListItemPosition::default(),
        }
    }

    /// Creates a list item with nested child lists.
    ///
    /// The item starts with no inline fields. Attach fields parsed from the
    /// item's own text with [`Self::with_fields`].
    #[inline]
    #[must_use]
    pub(crate) fn with_children(
        text: impl Into<String>,
        item_type: ListItemType,
        children: Vec<List>,
    ) -> Self {
        Self {
            text: text.into(),
            item_type,
            children,
            fields: IndexMap::new(),
            position: ListItemPosition::default(),
        }
    }

    /// Returns the plain text content.
    #[inline]
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Returns this item's classification: plain bullet, checkbox, or Task.
    #[inline]
    #[must_use]
    pub const fn item_type(&self) -> &ListItemType {
        &self.item_type
    }

    /// Returns the nested lists under this item.
    #[inline]
    #[must_use]
    pub(crate) fn children(&self) -> &[List] {
        &self.children
    }

    /// Attaches inline fields parsed from this item's own text.
    ///
    /// [`Note::inline_fields`] also includes these fields for page-level
    /// queries. This per-item list preserves the field-to-item relationship for
    /// task and list queries.
    ///
    /// [`Note::inline_fields`]: crate::note::Note::inline_fields
    #[inline]
    #[must_use]
    pub(crate) fn with_fields(
        mut self,
        fields: IndexMap<FieldKey, Vec<NoteFieldValue>>,
    ) -> Self {
        self.fields = fields;
        self
    }

    /// Returns the inline fields parsed from this item's own text.
    ///
    /// Task items also recognize date shorthand emoji such as `🗓️`, `➕`, `🛫`,
    /// `⏳`, and `✅`.
    #[inline]
    #[must_use]
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "no current caller outside tests; kept for ListItem \
                      accessor symmetry with its fields"
        )
    )]
    pub(crate) fn fields(&self) -> &IndexMap<FieldKey, Vec<NoteFieldValue>> {
        &self.fields
    }

    /// Attaches the source position (depth, line, parent line) computed by
    /// the parser from Markdown byte offsets.
    ///
    /// Items built via [`Self::new`] or [`Self::with_children`] default to
    /// [`ListItemPosition::default`] until this is called.
    #[inline]
    #[must_use]
    pub(super) const fn with_position(
        mut self,
        position: ListItemPosition,
    ) -> Self {
        self.position = position;
        self
    }

    /// Returns the item's 0-indexed nesting level.
    #[inline]
    #[must_use]
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "no current caller outside tests; consumed by LISTS \
                      persistence and task queries added in a later \
                      task-system issue"
        )
    )]
    pub(crate) const fn depth(&self) -> u8 {
        self.position.depth()
    }

    /// Returns the item's 1-indexed source line.
    #[inline]
    #[must_use]
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "no current caller outside tests; consumed by LISTS \
                      persistence and task queries added in a later \
                      task-system issue"
        )
    )]
    pub(crate) const fn line(&self) -> SourceLine {
        self.position.line()
    }

    /// Returns the immediate parent list item's 1-indexed source line, if
    /// this item is nested inside another list item.
    #[inline]
    #[must_use]
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "no current caller outside tests; consumed by LISTS \
                      persistence and task queries added in a later \
                      task-system issue"
        )
    )]
    pub(crate) const fn parent(&self) -> Option<SourceLine> {
        self.position.parent()
    }
}

/// How the custom marker scanner classified a Markdown list item.
///
/// [`Self::Plain`] items carry no task data. [`Self::Checkbox`] items are
/// status-marked items that did not match a configured task tag filter. They
/// carry only derived completion state and are excluded from
/// [`super::Note::tasks`]. [`Self::Task`] items carry a resolved task status
/// (symbol, name, and workflow type).
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub enum ListItemType {
    /// A plain bullet with no marker.
    Plain,
    /// A status-marked item that did not match a configured task tag filter.
    Checkbox,
    /// A status-marked item classified as a Task, carrying its resolved
    /// status.
    Task(TaskStatus),
}

impl ListItemType {
    /// Returns `true` if this list item is classified as a Task.
    #[inline]
    #[must_use]
    pub const fn is_task(&self) -> bool {
        matches!(self, Self::Task(_))
    }

    /// Returns `true` if this list item is classified as a Checkbox.
    #[inline]
    #[must_use]
    pub const fn is_checkbox(&self) -> bool {
        matches!(self, Self::Checkbox)
    }

    /// Returns `true` if this list item is classified as a plain bullet.
    #[inline]
    #[must_use]
    pub const fn is_plain(&self) -> bool {
        matches!(self, Self::Plain)
    }
}

/// A list item's position: its 0-indexed nesting depth, 1-indexed source
/// line, and its immediate parent's 1-indexed line, if nested.
///
/// `depth` is a `u8`: nesting hundreds of levels deep in a Markdown list is
/// degenerate input, not a real document, so a `usize` counter would spend
/// seven unreachable bytes per item. Saturates at 255 rather than wrapping.
#[derive(
    Copy, Clone, Debug, Default, Eq, PartialEq, Deserialize, Serialize,
)]
pub(super) struct ListItemPosition {
    depth: u8,
    line: SourceLine,
    parent: Option<SourceLine>,
}

impl ListItemPosition {
    /// Creates a position from its source line, 0-indexed nesting depth, and
    /// optional parent line.
    #[inline]
    #[must_use]
    pub(super) const fn new(
        line: SourceLine,
        depth: u8,
        parent: Option<SourceLine>,
    ) -> Self {
        Self {
            depth,
            line,
            parent,
        }
    }

    /// Returns the 0-indexed nesting level.
    #[inline]
    #[must_use]
    pub(super) const fn depth(&self) -> u8 {
        self.depth
    }

    /// Returns the 1-indexed source line.
    #[inline]
    #[must_use]
    pub(super) const fn line(&self) -> SourceLine {
        self.line
    }

    /// Returns the immediate parent item's 1-indexed source line, if this
    /// item is nested inside another item's child list.
    #[inline]
    #[must_use]
    pub(super) const fn parent(&self) -> Option<SourceLine> {
        self.parent
    }
}
#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use rstest::rstest;

    use super::*;
    use crate::task::{TaskStatusSymbol, TaskStatusType};

    fn done_task() -> ListItemType {
        ListItemType::Task(TaskStatus::new(
            TaskStatusSymbol::new('x'),
            "Done",
            TaskStatusType::Done,
        ))
    }

    #[rstest]
    #[case::plain(ListItemType::Plain)]
    #[case::checkbox(ListItemType::Checkbox)]
    #[case::task(done_task())]
    fn stores_the_given_item_type(#[case] item_type: ListItemType) {
        let item = ListItem::new("task item", item_type.clone());

        assert_eq!(item.text(), "task item");
        assert_eq!(item.item_type(), &item_type);
    }

    #[test]
    fn stores_child_lists_when_constructed_with_children() {
        let child =
            List::new(false, vec![ListItem::new("child", ListItemType::Plain)]);
        let item =
            ListItem::with_children("parent", ListItemType::Plain, vec![
                child.clone(),
            ]);

        assert_eq!(item.children(), [child]);
    }

    #[test]
    fn stores_ordering_and_items() {
        let item = ListItem::new("task item", done_task());
        let list = List::new(true, vec![item.clone()]);

        assert_eq!(list.is_ordered(), true);
        assert_eq!(list.items(), [item]);
    }

    #[test]
    fn stores_fields_when_attached_with_with_fields() {
        use crate::note::NoteFieldValue;

        let key = FieldKey::try_new("priority").expect("valid test field key");
        let mut fields = IndexMap::new();
        fields.insert(key, vec![NoteFieldValue::String("high".to_owned())]);
        let item =
            ListItem::new("task item", done_task()).with_fields(fields.clone());

        assert_eq!(item.fields(), &fields);
    }

    #[test]
    fn has_no_fields_by_default() {
        let item = ListItem::new("plain item", ListItemType::Plain);

        assert!(item.fields().is_empty());
    }

    #[test]
    fn defaults_position_to_zero_and_no_parent() {
        let item = ListItem::new("item", ListItemType::Plain);

        assert_eq!(item.line(), SourceLine::new(0));
        assert_eq!(item.depth(), 0);
        assert_eq!(item.parent(), None);
    }

    #[test]
    fn with_position_sets_line_depth_and_parent() {
        let position = ListItemPosition::new(
            SourceLine::new(3),
            2,
            Some(SourceLine::new(1)),
        );
        let item =
            ListItem::new("item", ListItemType::Plain).with_position(position);

        assert_eq!(item.line(), SourceLine::new(3));
        assert_eq!(item.depth(), 2);
        assert_eq!(item.parent(), Some(SourceLine::new(1)));
    }
}
