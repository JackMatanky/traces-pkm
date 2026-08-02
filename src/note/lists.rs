//! Markdown list, list item, and task-list structures.

use serde::{Deserialize, Serialize};

use super::metadata::InlineField;

/// Ordered or unordered Markdown list.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub(crate) struct List {
    is_ordered: bool,
    items: Vec<ListItem>,
}

impl List {
    /// Creates a list from its ordering flag and direct child items.
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

    /// Direct child items in this list.
    #[inline]
    #[must_use]
    pub(crate) fn items(&self) -> &[ListItem] {
        &self.items
    }
}

/// Completion state of a Markdown task list item.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub(crate) enum TaskStatus {
    Incomplete,
    Complete,
}

/// Markdown list item with optional task state, child lists, and item fields.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub(crate) struct ListItem {
    text: String,
    task_status: Option<TaskStatus>,
    children: Vec<List>,
    fields: Vec<InlineField>,
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
            fields: Vec::new(),
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
        task_status: Option<TaskStatus>,
        children: Vec<List>,
    ) -> Self {
        Self {
            text: text.into(),
            task_status,
            children,
            fields: Vec::new(),
        }
    }

    /// Plain text content.
    #[inline]
    #[must_use]
    pub(crate) fn text(&self) -> &str {
        &self.text
    }

    /// Task completion state, if this item is a task.
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

    /// Attaches inline fields parsed from this item's own text.
    ///
    /// [`Note::inline_fields`](crate::note::Note::inline_fields) also includes
    /// these fields for page-level queries. This per-item list preserves the
    /// field-to-item relationship for task and list queries.
    #[inline]
    #[must_use]
    pub(crate) fn with_fields(mut self, fields: Vec<InlineField>) -> Self {
        self.fields = fields;
        self
    }

    /// Inline fields parsed from this item's own text.
    ///
    /// Task items also recognize Dataview date shorthand emoji such as `🗓️`,
    /// `➕`, `🛫`, `⏳`, and `✅`.
    #[inline]
    #[must_use]
    pub(crate) fn fields(&self) -> &[InlineField] {
        &self.fields
    }
}

#[cfg(test)]
mod tests {
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
        let item = ListItem::with_children("parent", None, vec![child.clone()]);

        assert_eq!(item.children(), [child]);
    }

    #[test]
    fn stores_ordering_and_items() {
        let item = ListItem::new("task item", Some(TaskStatus::Incomplete));
        let list = List::new(true, vec![item.clone()]);

        assert_eq!(list.is_ordered(), true);
        assert_eq!(list.items(), [item]);
    }

    #[test]
    fn stores_fields_when_attached_with_with_fields() {
        use crate::note::{FieldValue, InlineFieldForm};

        let field = InlineField::new(
            "priority",
            FieldValue::String("high".to_owned()),
            InlineFieldForm::VisibleKey,
        );
        let item = ListItem::new("task item", Some(TaskStatus::Incomplete))
            .with_fields(vec![field.clone()]);

        assert_eq!(item.fields(), [field]);
    }

    #[test]
    fn has_no_fields_by_default() {
        let item = ListItem::new("plain item", None);

        assert!(item.fields().is_empty());
    }
}
