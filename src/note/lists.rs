//! Markdown list, list item, and task-list structures.
//!
//! This module defines the core data model for ordered and unordered Markdown
//! lists, individual list items, task-specific metadata, and recursive task
//! iterators.
//!
//! # Key Types
//!
//! - [`List`]: An ordered or unordered Markdown list holding direct child
//!   items.
//! - [`ListItem`]: A list item with a classified [`ListItemType`], child lists,
//!   inline fields, and source positioning.
//! - [`ListItemType`]: Classification of an item as a plain bullet, a checkbox,
//!   or a task carrying a [`TaskListItem`].
//! - [`TaskListItem`]: Task-specific metadata (resolved status, priority,
//!   dates, and precomputed subtree completion state) carried by
//!   [`ListItemType::Task`].
//! - [`TaskPriority`]: Six-level task priority enum mapped to emoji and text
//!   representations.
//! - [`TaskDates`]: Six distinct task-lifecycle calendar dates (created,
//!   scheduled, start, due, done, cancelled).
//! - [`ListText`]: Dual-representation text container maintaining both raw
//!   source and clean display text.
//! - [`ListItemIter`]: A depth-first iterator yielding all list items across
//!   top-level and nested child lists in document order, optionally filtered to
//!   [`ListItemType::Task`] items.
//!
//! # Examples
//!
//! ```rust
//! use traces_pkm::{
//!     ListText, TaskDates, TaskListItem, TaskPriority, TaskStatus,
//! };
//!
//! let dates = TaskDates::default();
//! let task = TaskListItem::new(
//!     dates,
//!     Some(TaskPriority::High),
//!     TaskStatus::default(),
//!     true,
//! );
//! assert_eq!(task.priority(), Some(TaskPriority::High));
//! assert!(task.is_fully_complete());
//! ```
use chrono::NaiveDate;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

use super::field::NoteFieldValue;
use crate::{FieldKey, SourceLine, TaskStatus, TaskStatusType};
/// An ordered or unordered Markdown list.
///
/// Holds direct child [`ListItem`] elements and a flag indicating whether the
/// list is numbered (ordered) or bulleted (unordered).
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
///     "1. First\n2. Second",
/// );
/// let note = parse_markdown(&input);
/// assert_eq!(note.lists().len(), 1);
/// # }
/// ```
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
    ///
    /// Does not include descendant items nested inside child lists.
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
    /// let list = &note.lists()[0];
    /// assert_eq!(list.items().len(), 2);
    /// # }
    /// ```
    #[inline]
    #[must_use]
    pub fn items(&self) -> &[ListItem] {
        &self.items
    }
}
/// A Markdown list item with a classified [`ListItemType`], child lists, and
/// inline fields.
///
/// Stores both raw and normalized text representations via [`ListText`], nested
/// child [`List`] structures, extracted Dataview-style inline fields, and
/// source line positioning information.
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
///     "- [ ] Action item",
/// );
/// let note = parse_markdown(&input);
/// let item = &note.lists()[0].items()[0];
/// assert!(item.kind().is_task());
/// assert_eq!(item.clean_text(), "Action item");
/// # }
/// ```
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct ListItem {
    text: ListText,
    kind: ListItemType,
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
    pub(crate) fn new(text: impl Into<ListText>, kind: ListItemType) -> Self {
        Self {
            text: text.into(),
            kind,
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
        text: impl Into<ListText>,
        kind: ListItemType,
        children: Vec<List>,
    ) -> Self {
        Self {
            text: text.into(),
            kind,
            children,
            fields: IndexMap::new(),
            position: ListItemPosition::default(),
        }
    }

    /// Attaches inline fields parsed from this item's own text.
    ///
    /// [`Note::inline_fields`] also includes these fields for page-level
    /// queries. This per-item list preserves the field-to-item relationship for
    /// task and list queries.
    ///
    /// [`Note::inline_fields`]: crate::Note::inline_fields
    #[inline]
    #[must_use]
    pub(crate) fn with_fields(
        mut self,
        fields: IndexMap<FieldKey, Vec<NoteFieldValue>>,
    ) -> Self {
        self.fields = fields;
        self
    }

    /// Returns the plain or normalized text representation holding both raw
    /// and clean variants.
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
    ///     "- [ ] Task 📅 2025-01-15",
    /// );
    /// let note = parse_markdown(&input);
    /// let item = &note.lists()[0].items()[0];
    /// assert_eq!(item.text().raw(), "Task 📅 2025-01-15");
    /// assert_eq!(item.text().clean(), "Task");
    /// # }
    /// ```
    #[inline]
    #[must_use]
    pub fn text(&self) -> &ListText {
        &self.text
    }

    /// Returns the raw text with only the leading marker prefix stripped.
    ///
    /// Retains tags, dates, priority emojis, and inline fields.
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
    ///     "- [ ] Task 📅 2025-01-15",
    /// );
    /// let note = parse_markdown(&input);
    /// let item = &note.lists()[0].items()[0];
    /// assert_eq!(item.raw_text(), "Task 📅 2025-01-15");
    /// # }
    /// ```
    #[inline]
    #[must_use]
    pub fn raw_text(&self) -> &str {
        self.text.raw()
    }

    /// Returns the normalized clean text with task metadata stripped.
    ///
    /// Strips configured task tag filters, date shorthand syntax, priority
    /// emojis, and inline task fields.
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
    ///     "- [ ] Task 📅 2025-01-15",
    /// );
    /// let note = parse_markdown(&input);
    /// let item = &note.lists()[0].items()[0];
    /// assert_eq!(item.clean_text(), "Task");
    /// # }
    /// ```
    #[inline]
    #[must_use]
    pub fn clean_text(&self) -> &str {
        self.text.clean()
    }

    /// Returns this item's classification: plain bullet, checkbox, or Task.
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
    ///     MarkdownParserInput::for_test(Path::new("note.md"), "- [ ] Task");
    /// let note = parse_markdown(&input);
    /// let item = &note.lists()[0].items()[0];
    /// assert!(item.kind().is_task());
    /// # }
    /// ```
    #[inline]
    #[must_use]
    pub const fn kind(&self) -> &ListItemType {
        &self.kind
    }

    /// Returns the nested lists under this item.
    #[inline]
    #[must_use]
    pub(crate) fn children(&self) -> &[List] {
        &self.children
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
    pub(crate) const fn depth(&self) -> u8 {
        self.position.depth()
    }

    /// Returns the item's 1-indexed source line.
    #[inline]
    #[must_use]
    pub(crate) const fn line(&self) -> SourceLine {
        self.position.line()
    }

    /// Returns the immediate parent list item's 1-indexed source line, if
    /// this item is nested inside another list item.
    #[inline]
    #[must_use]
    pub(crate) const fn parent(&self) -> Option<SourceLine> {
        self.position.parent()
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
///     "- Plain\n- [ ] Task",
/// );
/// let note = parse_markdown(&input);
/// assert!(note.lists()[0].items()[0].kind().is_plain());
/// assert!(note.lists()[0].items()[1].kind().is_task());
/// # }
/// ```
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
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
    /// Creates a task list item with its dates, priority, resolved status,
    /// and precomputed fully-complete state.
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

    /// Returns `true` if all descendant tasks in this item's subtree are
    /// resolved (done or cancelled), or if this item has no descendant tasks.
    ///
    /// Alias for [`Self::is_fully_complete`].
    #[inline]
    #[must_use]
    pub const fn fully_complete(&self) -> bool {
        self.is_fully_complete()
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

/// Task priority level.
///
/// Supports six priority levels ordered from lowest to highest:
/// [`Self::Lowest`] < [`Self::Low`] < [`Self::Normal`] < [`Self::Medium`] <
/// [`Self::High`] < [`Self::Highest`].
///
/// # Examples
///
/// ```rust
/// use traces_pkm::TaskPriority;
///
/// assert!(TaskPriority::Highest > TaskPriority::High);
/// assert!(TaskPriority::High > TaskPriority::Medium);
/// assert_eq!(TaskPriority::from_emoji("🔺"), Some(TaskPriority::Highest));
/// ```
#[derive(
    Copy,
    Clone,
    Debug,
    Eq,
    PartialEq,
    Ord,
    PartialOrd,
    Hash,
    Deserialize,
    Serialize,
)]
#[serde(rename_all = "lowercase")]
pub enum TaskPriority {
    /// Lowest priority (`⏬`).
    Lowest,
    /// Low priority (`🔽`).
    Low,
    /// Normal priority (stored as `None` on [`TaskListItem`] when unspecified).
    Normal,
    /// Medium priority (`🔼`).
    Medium,
    /// High priority (`⏫`).
    High,
    /// Highest priority (`🔺`).
    Highest,
}
impl TaskPriority {
    /// Returns the canonical lowercase string name of the priority.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use traces_pkm::TaskPriority;
    ///
    /// assert_eq!(TaskPriority::Highest.as_str(), "highest");
    /// assert_eq!(TaskPriority::Normal.as_str(), "normal");
    /// ```
    #[inline]
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Lowest => "lowest",
            Self::Low => "low",
            Self::Normal => "normal",
            Self::Medium => "medium",
            Self::High => "high",
            Self::Highest => "highest",
        }
    }

    /// Parses a priority from an emoji, with or without variation selector 16
    /// (`\u{FE0F}`).
    ///
    /// | Emoji | Priority |
    /// | ----- | -------- |
    /// | 🔺    | highest  |
    /// | ⏫    | high     |
    /// | 🔼    | medium   |
    /// | 🔽    | low      |
    /// | ⏬    | lowest   |
    ///
    /// # Examples
    ///
    /// ```rust
    /// use traces_pkm::TaskPriority;
    ///
    /// assert_eq!(TaskPriority::from_emoji("🔺"), Some(TaskPriority::Highest));
    /// assert_eq!(TaskPriority::from_emoji("⏬"), Some(TaskPriority::Lowest));
    /// assert_eq!(TaskPriority::from_emoji("invalid"), None);
    /// ```
    #[inline]
    #[must_use]
    pub fn from_emoji(emoji: &str) -> Option<Self> {
        let trimmed = emoji.trim_end_matches('\u{FE0F}');
        match trimmed {
            "\u{1F53A}" => Some(Self::Highest),
            "\u{23EB}" => Some(Self::High),
            "\u{1F53C}" => Some(Self::Medium),
            "\u{1F53D}" => Some(Self::Low),
            "\u{23EC}" => Some(Self::Lowest),
            _ => None,
        }
    }

    /// Returns the canonical emoji representation for this priority, or
    /// [`None`] for [`Self::Normal`].
    ///
    /// # Examples
    ///
    /// ```rust
    /// use traces_pkm::TaskPriority;
    ///
    /// assert_eq!(TaskPriority::Highest.emoji(), Some("🔺"));
    /// assert_eq!(TaskPriority::Normal.emoji(), None);
    /// ```
    #[inline]
    #[must_use]
    pub const fn emoji(&self) -> Option<&'static str> {
        match self {
            Self::Highest => Some("🔺"),
            Self::High => Some("⏫"),
            Self::Medium => Some("🔼"),
            Self::Low => Some("🔽"),
            Self::Lowest => Some("⏬"),
            Self::Normal => None,
        }
    }
}

impl std::fmt::Display for TaskPriority {
    #[inline]
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for TaskPriority {
    type Err = ();

    #[inline]
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "lowest" => Ok(Self::Lowest),
            "low" => Ok(Self::Low),
            "normal" => Ok(Self::Normal),
            "medium" => Ok(Self::Medium),
            "high" => Ok(Self::High),
            "highest" => Ok(Self::Highest),
            _ => Self::from_emoji(s).ok_or(()),
        }
    }
}

/// Date metadata associated with a [`TaskListItem`].
///
/// Stores six distinct task-lifecycle dates parsed from emoji shorthand or
/// Dataview inline field syntax. Missing dates are represented as [`None`].
///
/// # Examples
///
/// ```rust
/// use chrono::NaiveDate;
/// use traces_pkm::TaskDates;
///
/// let mut dates = TaskDates::default();
/// dates.due = NaiveDate::from_ymd_opt(2025, 1, 15);
/// assert!(!dates.is_empty());
/// assert_eq!(dates.due(), NaiveDate::from_ymd_opt(2025, 1, 15));
/// ```
#[derive(
    Copy, Clone, Debug, Default, Eq, PartialEq, Hash, Deserialize, Serialize,
)]
pub struct TaskDates {
    /// Date when the task was created (`➕` or `[created::]`).
    pub created: Option<NaiveDate>,
    /// Date when the task is scheduled (`⏳` or `[scheduled::]`).
    pub scheduled: Option<NaiveDate>,
    /// Date when work on the task begins (`🛫` or `[start::]`).
    pub start: Option<NaiveDate>,
    /// Date when the task is due (`📅` or `[due::]`).
    pub due: Option<NaiveDate>,
    /// Date when the task was completed (`✅` or `[done::]`).
    pub done: Option<NaiveDate>,
    /// Date when the task was cancelled (`❌` or `[cancelled::]`).
    pub cancelled: Option<NaiveDate>,
}
impl TaskDates {
    /// Creates a new `TaskDates` instance with all dates specified.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use chrono::NaiveDate;
    /// use traces_pkm::TaskDates;
    ///
    /// let due = NaiveDate::from_ymd_opt(2025, 1, 15);
    /// let dates = TaskDates::new(None, None, None, due, None, None);
    /// assert_eq!(dates.due(), due);
    /// ```
    #[inline]
    #[must_use]
    #[expect(
        clippy::too_many_arguments,
        reason = "constructor accepts all 6 task dates"
    )]
    pub const fn new(
        created: Option<NaiveDate>,
        scheduled: Option<NaiveDate>,
        start: Option<NaiveDate>,
        due: Option<NaiveDate>,
        done: Option<NaiveDate>,
        cancelled: Option<NaiveDate>,
    ) -> Self {
        Self {
            created,
            scheduled,
            start,
            due,
            done,
            cancelled,
        }
    }

    /// Returns `true` if no dates are set.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use traces_pkm::TaskDates;
    ///
    /// assert!(TaskDates::default().is_empty());
    /// ```
    #[inline]
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.created.is_none()
            && self.scheduled.is_none()
            && self.start.is_none()
            && self.due.is_none()
            && self.done.is_none()
            && self.cancelled.is_none()
    }

    /// Returns the task's creation date, if set.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use chrono::NaiveDate;
    /// use traces_pkm::TaskDates;
    ///
    /// let mut dates = TaskDates::default();
    /// dates.created = NaiveDate::from_ymd_opt(2025, 1, 1);
    /// assert_eq!(dates.created(), NaiveDate::from_ymd_opt(2025, 1, 1));
    /// ```
    #[inline]
    #[must_use]
    pub const fn created(&self) -> Option<NaiveDate> {
        self.created
    }

    /// Returns the task's scheduled date, if set.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use chrono::NaiveDate;
    /// use traces_pkm::TaskDates;
    ///
    /// let mut dates = TaskDates::default();
    /// dates.scheduled = NaiveDate::from_ymd_opt(2025, 1, 10);
    /// assert_eq!(dates.scheduled(), NaiveDate::from_ymd_opt(2025, 1, 10));
    /// ```
    #[inline]
    #[must_use]
    pub const fn scheduled(&self) -> Option<NaiveDate> {
        self.scheduled
    }

    /// Returns the task's start date, if set.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use chrono::NaiveDate;
    /// use traces_pkm::TaskDates;
    ///
    /// let mut dates = TaskDates::default();
    /// dates.start = NaiveDate::from_ymd_opt(2025, 1, 12);
    /// assert_eq!(dates.start(), NaiveDate::from_ymd_opt(2025, 1, 12));
    /// ```
    #[inline]
    #[must_use]
    pub const fn start(&self) -> Option<NaiveDate> {
        self.start
    }

    /// Returns the task's due date, if set.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use chrono::NaiveDate;
    /// use traces_pkm::TaskDates;
    ///
    /// let mut dates = TaskDates::default();
    /// dates.due = NaiveDate::from_ymd_opt(2025, 1, 15);
    /// assert_eq!(dates.due(), NaiveDate::from_ymd_opt(2025, 1, 15));
    /// ```
    #[inline]
    #[must_use]
    pub const fn due(&self) -> Option<NaiveDate> {
        self.due
    }

    /// Returns the task's completion date, if set.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use chrono::NaiveDate;
    /// use traces_pkm::TaskDates;
    ///
    /// let mut dates = TaskDates::default();
    /// dates.done = NaiveDate::from_ymd_opt(2025, 1, 20);
    /// assert_eq!(dates.done(), NaiveDate::from_ymd_opt(2025, 1, 20));
    /// ```
    #[inline]
    #[must_use]
    pub const fn done(&self) -> Option<NaiveDate> {
        self.done
    }

    /// Returns the task's cancellation date, if set.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use chrono::NaiveDate;
    /// use traces_pkm::TaskDates;
    ///
    /// let mut dates = TaskDates::default();
    /// dates.cancelled = NaiveDate::from_ymd_opt(2025, 1, 22);
    /// assert_eq!(dates.cancelled(), NaiveDate::from_ymd_opt(2025, 1, 22));
    /// ```
    #[inline]
    #[must_use]
    pub const fn cancelled(&self) -> Option<NaiveDate> {
        self.cancelled
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
    Clone, Debug, Default, Eq, PartialEq, Hash, Deserialize, Serialize,
)]
pub struct ListText {
    /// Source text minus the leading `[<char>] ` marker prefix only.
    pub raw: String,
    /// Normalized display text with task metadata stripped.
    pub clean: String,
}

impl ListText {
    /// Creates a new `ListText` from raw and clean text representations.
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
        Self {
            raw: raw.into(),
            clean: clean.into(),
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
        &self.clean
    }
}
impl std::fmt::Display for ListText {
    #[inline]
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.clean)
    }
}

impl From<&str> for ListText {
    #[inline]
    fn from(s: &str) -> Self {
        Self {
            raw: s.to_owned(),
            clean: s.to_owned(),
        }
    }
}

impl From<String> for ListText {
    #[inline]
    fn from(s: String) -> Self {
        Self {
            raw: s.clone(),
            clean: s,
        }
    }
}

impl From<(&str, &str)> for ListText {
    #[inline]
    fn from((raw, clean): (&str, &str)) -> Self {
        Self {
            raw: raw.to_owned(),
            clean: clean.to_owned(),
        }
    }
}

impl From<(String, String)> for ListText {
    #[inline]
    fn from((raw, clean): (String, String)) -> Self {
        Self {
            raw,
            clean,
        }
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

/// A depth-first iterator over top-level and nested child lists in document
/// order, yielding either every item ([`Note::list_items`]) or only items
/// classified as [`ListItemType::Task`] ([`Note::tasks`]).
///
/// [`Note::list_items`]: super::Note::list_items
/// [`Note::tasks`]: super::Note::tasks
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
///     "- Item 1\n  - Item 1.1\n- Item 2",
/// );
/// let note = parse_markdown(&input);
/// assert_eq!(note.list_items().count(), 3);
/// # }
/// ```
#[derive(Clone, Debug)]
pub struct ListItemIter<'a> {
    stack: Vec<std::slice::Iter<'a, ListItem>>,
    tasks_only: bool,
}

impl<'a> ListItemIter<'a> {
    /// Starts depth-first iteration over every item in top-level `lists`.
    #[inline]
    #[must_use]
    pub(crate) fn new(lists: &'a [List]) -> Self {
        Self::with_stack(lists, false)
    }

    /// Starts depth-first iteration over top-level `lists`, yielding only
    /// items classified as [`ListItemType::Task`].
    ///
    /// Filters at yield time rather than traversal time: descending into a
    /// non-task item's children is unaffected, so nested tasks under a plain
    /// bullet or checkbox are still reached.
    #[inline]
    #[must_use]
    pub(crate) fn tasks(lists: &'a [List]) -> Self {
        Self::with_stack(lists, true)
    }

    /// Builds the shared traversal stack for [`Self::new`] and
    /// [`Self::tasks`].
    #[inline]
    #[must_use]
    fn with_stack(lists: &'a [List], tasks_only: bool) -> Self {
        let mut stack = Vec::with_capacity(lists.len());
        stack.extend(lists.iter().rev().map(|list| list.items().iter()));
        Self {
            stack,
            tasks_only,
        }
    }
}

impl<'a> Iterator for ListItemIter<'a> {
    type Item = &'a ListItem;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        while let Some(items) = self.stack.last_mut() {
            let Some(item) = items.next() else {
                self.stack.pop();
                continue;
            };
            self.stack.extend(
                item.children().iter().rev().map(|list| list.items().iter()),
            );
            if !self.tasks_only || matches!(item.kind(), ListItemType::Task(_))
            {
                return Some(item);
            }
        }
        None
    }
}

impl std::iter::FusedIterator for ListItemIter<'_> {}

/// A persisted record of a single list item and its source note path.
///
/// Wraps a project-relative `path` and the parsed [`ListItem`]. Exposes
/// accessor methods that delegate into the [`ListItemType`] discriminant,
/// keeping the persistence shape composable while providing flat field access.
///
/// Serializes via postcard as `path` + `ListItem`. Stored in the `LISTS`
/// table in redb keyed by `(path, line)`.
///
/// # Examples
///
/// ```rust
/// # #[cfg(feature = "test-utils")]
/// # {
/// use std::path::Path;
///
/// use traces_pkm::{ListRecord, MarkdownParserInput, parse_markdown};
///
/// let input = MarkdownParserInput::for_test(Path::new("note.md"), "- bullet");
/// let note = parse_markdown(&input);
/// let item = note.list_items().next().unwrap().clone();
/// let record = ListRecord::new("notes/a.md".to_string(), item);
/// assert_eq!(record.path(), "notes/a.md");
/// assert_eq!(record.status_type(), None);
/// # }
/// ```
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct ListRecord {
    path: String,
    item: ListItem,
}

impl ListRecord {
    /// Creates a new `ListRecord` wrapping a project-relative `path` and
    /// `item`.
    #[inline]
    #[must_use]
    pub fn new<P: Into<String>>(path: P, item: ListItem) -> Self {
        Self {
            path: path.into(),
            item,
        }
    }

    /// Returns the project-relative path of the note containing this list item.
    #[inline]
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Returns the task's status type, or [`None`] if this is not a Task item.
    #[inline]
    #[must_use]
    pub const fn status_type(&self) -> Option<TaskStatusType> {
        match self.item.kind() {
            ListItemType::Task(task) => Some(task.status().kind()),
            ListItemType::Plain | ListItemType::Checkbox => None,
        }
    }

    /// Returns the task's priority, or [`None`] if this is not a Task item or
    /// has no priority.
    #[inline]
    #[must_use]
    pub const fn priority(&self) -> Option<TaskPriority> {
        match self.item.kind() {
            ListItemType::Task(task) => task.priority(),
            ListItemType::Plain | ListItemType::Checkbox => None,
        }
    }

    /// Returns the task's due date, or [`None`] if this is not a Task item or
    /// has no due date.
    #[inline]
    #[must_use]
    pub const fn due_date(&self) -> Option<NaiveDate> {
        match self.item.kind() {
            ListItemType::Task(task) => task.dates().due,
            ListItemType::Plain | ListItemType::Checkbox => None,
        }
    }

    /// Returns `true` if this task item and its entire task subtree are
    /// resolved, or [`None`] if this is not a Task item.
    #[inline]
    #[must_use]
    pub const fn is_fully_complete(&self) -> Option<bool> {
        match self.item.kind() {
            ListItemType::Task(task) => Some(task.is_fully_complete()),
            ListItemType::Plain | ListItemType::Checkbox => None,
        }
    }

    /// Returns the list item's text container.
    #[inline]
    #[must_use]
    pub fn text(&self) -> &ListText {
        self.item.text()
    }

    /// Returns the raw text with only the leading marker prefix stripped.
    #[inline]
    #[must_use]
    pub fn raw_text(&self) -> &str {
        self.item.raw_text()
    }

    /// Returns the normalized clean text with task metadata stripped.
    #[inline]
    #[must_use]
    pub fn clean_text(&self) -> &str {
        self.item.clean_text()
    }

    /// Returns the list item's 1-indexed source line.
    #[inline]
    #[must_use]
    pub const fn line(&self) -> SourceLine {
        self.item.line()
    }

    /// Returns the list item's 0-indexed nesting depth.
    #[inline]
    #[must_use]
    pub const fn depth(&self) -> u8 {
        self.item.depth()
    }

    /// Returns the immediate parent list item's 1-indexed source line, if
    /// nested.
    #[inline]
    #[must_use]
    pub const fn parent_line(&self) -> Option<SourceLine> {
        self.item.parent()
    }
}

/// A list item's position: its 0-indexed nesting depth, 1-indexed source line,
/// and its immediate parent's 1-indexed line, if nested.
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

    fn todo_task() -> ListItemType {
        ListItemType::Task(TaskListItem::new(
            TaskDates::default(),
            None,
            TaskStatus::new(
                TaskStatusSymbol::new(' '),
                "Todo",
                TaskStatusType::Todo,
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
            fn stores_the_given_kind(#[case] kind: ListItemType) {
                let item = ListItem::new("task item", kind.clone());

                assert_eq!(item.text().raw(), "task item");
                assert_eq!(item.text().clean(), "task item");
                assert_eq!(item.raw_text(), "task item");
                assert_eq!(item.clean_text(), "task item");
                assert_eq!(item.kind(), &kind);
            }

            #[test]
            fn stores_child_lists_when_constructed_with_children() {
                let child = List::new(false, vec![ListItem::new(
                    "child",
                    ListItemType::Plain,
                )]);
                let item = ListItem::with_children(
                    "parent",
                    ListItemType::Plain,
                    vec![child.clone()],
                );

                assert_eq!(item.children(), [child]);
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
                fields.insert(key, vec![NoteFieldValue::String(
                    "high".to_owned(),
                )]);
                let item = ListItem::new("task item", done_task())
                    .with_fields(fields.clone());

                assert_eq!(item.fields(), &fields);
            }

            #[test]
            fn has_no_fields_by_default() {
                let item = ListItem::new("plain item", ListItemType::Plain);

                assert!(item.fields().is_empty());
            }
        }

        mod position {
            use pretty_assertions::assert_eq;

            use super::*;
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
                let item = ListItem::new("item", ListItemType::Plain)
                    .with_position(position);

                assert_eq!(item.line(), SourceLine::new(3));
                assert_eq!(item.depth(), 2);
                assert_eq!(item.parent(), Some(SourceLine::new(1)));
            }
        }
    }

    mod list {
        use super::*;

        mod constructor {
            use pretty_assertions::assert_eq;

            use super::*;
            #[test]
            fn stores_ordering_and_items() {
                let item = ListItem::new("task item", done_task());
                let list = List::new(true, vec![item.clone()]);

                assert_eq!(list.is_ordered(), true);
                assert_eq!(list.items(), [item]);
            }
        }
    }

    mod list_item_iter {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn yields_all_items_depth_first_across_nested_lists() {
            let grandchild_plain =
                ListItem::new("grandchild plain", ListItemType::Plain);
            let child_checkbox = ListItem::with_children(
                "child checkbox",
                ListItemType::Checkbox,
                vec![List::new(false, vec![grandchild_plain])],
            );
            let parent_task =
                ListItem::with_children("parent task", todo_task(), vec![
                    List::new(false, vec![child_checkbox]),
                ]);
            let sibling = ListItem::new("sibling item", done_task());
            let lists = vec![
                List::new(false, vec![parent_task]),
                List::new(false, vec![sibling]),
            ];

            let iter = ListItemIter::new(&lists);
            let texts: Vec<&str> = iter.map(ListItem::clean_text).collect();

            assert_eq!(texts, [
                "parent task",
                "child checkbox",
                "grandchild plain",
                "sibling item"
            ]);
        }

        #[test]
        fn tasks_yields_only_task_items_depth_first_across_nested_lists() {
            let subchild_task = ListItem::new("subchild task", done_task());
            let child_checkbox = ListItem::with_children(
                "child checkbox",
                ListItemType::Checkbox,
                vec![List::new(false, vec![subchild_task])],
            );
            let parent_task =
                ListItem::with_children("parent task", todo_task(), vec![
                    List::new(false, vec![child_checkbox]),
                ]);
            let plain = ListItem::new("plain item", ListItemType::Plain);
            let lists = vec![List::new(false, vec![parent_task, plain])];

            let iter = ListItemIter::tasks(&lists);
            let texts: Vec<&str> = iter.map(ListItem::clean_text).collect();

            assert_eq!(texts, ["parent task", "subchild task"]);
        }

        #[test]
        fn returns_none_for_empty_lists() {
            let lists: Vec<List> = Vec::new();
            let mut iter = ListItemIter::new(&lists);

            assert_eq!(iter.next(), None);
        }
    }

    mod list_record {
        use chrono::NaiveDate;
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn stores_path_and_delegates_text_to_item() {
            let item = ListItem::new("plain item", ListItemType::Plain);
            let record =
                ListRecord::new("notes/todo.md".to_owned(), item.clone());

            assert_eq!(record.path(), "notes/todo.md");
            assert_eq!(record.text(), item.text());
        }

        #[test]
        fn accessors_delegate_for_task_item() {
            let status = TaskStatus::new(
                TaskStatusSymbol::new(' '),
                "Todo",
                TaskStatusType::Todo,
            );
            let dates = TaskDates::new(
                None,
                None,
                None,
                NaiveDate::from_ymd_opt(2025, 1, 15),
                None,
                None,
            );
            let task_item = TaskListItem::new(
                dates,
                Some(TaskPriority::High),
                status,
                false,
            );
            let position = ListItemPosition::new(
                SourceLine::new(5),
                1,
                Some(SourceLine::new(2)),
            );
            let item = ListItem::new("my task", ListItemType::Task(task_item))
                .with_position(position);
            let record =
                ListRecord::new("notes/task.md".to_owned(), item.clone());

            assert_eq!(record.status_type(), Some(TaskStatusType::Todo));
            assert_eq!(record.priority(), Some(TaskPriority::High));
            assert_eq!(record.due_date(), NaiveDate::from_ymd_opt(2025, 1, 15));
            assert_eq!(record.is_fully_complete(), Some(false));
            assert_eq!(record.text(), item.text());
            assert_eq!(record.line(), SourceLine::new(5));
            assert_eq!(record.depth(), 1);
            assert_eq!(record.parent_line(), Some(SourceLine::new(2)));
        }

        #[test]
        fn task_accessors_return_none_for_plain_and_checkbox_items() {
            let position = ListItemPosition::new(SourceLine::new(10), 0, None);
            let plain_item = ListItem::new("bullet", ListItemType::Plain)
                .with_position(position);
            let record = ListRecord::new(
                "notes/plain.md".to_owned(),
                plain_item.clone(),
            );

            assert_eq!(record.status_type(), None);
            assert_eq!(record.priority(), None);
            assert_eq!(record.due_date(), None);
            assert_eq!(record.is_fully_complete(), None);
            assert_eq!(record.text(), plain_item.text());
            assert_eq!(record.line(), SourceLine::new(10));
            assert_eq!(record.depth(), 0);
            assert_eq!(record.parent_line(), None);

            let checkbox_item = ListItem::new("check", ListItemType::Checkbox);
            let record_check =
                ListRecord::new("notes/check.md".to_owned(), checkbox_item);
            assert_eq!(record_check.status_type(), None);
            assert_eq!(record_check.priority(), None);
            assert_eq!(record_check.due_date(), None);
            assert_eq!(record_check.is_fully_complete(), None);
        }

        #[test]
        fn postcard_roundtrip() {
            let status = TaskStatus::new(
                TaskStatusSymbol::new('x'),
                "Done",
                TaskStatusType::Done,
            );
            let dates = TaskDates::new(
                None,
                None,
                None,
                NaiveDate::from_ymd_opt(2025, 1, 15),
                Some(NaiveDate::from_ymd_opt(2025, 1, 14).unwrap()),
                None,
            );
            let task_item = TaskListItem::new(
                dates,
                Some(TaskPriority::Medium),
                status,
                true,
            );
            let position = ListItemPosition::new(
                SourceLine::new(42),
                2,
                Some(SourceLine::new(10)),
            );
            let item =
                ListItem::new("postcard task", ListItemType::Task(task_item))
                    .with_position(position);
            let record = ListRecord::new("path/to/note.md".to_owned(), item);

            let bytes =
                postcard::to_allocvec(&record).expect("serialize list record");
            let decoded: ListRecord =
                postcard::from_bytes(&bytes).expect("deserialize list record");

            assert_eq!(decoded, record);
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
                    NaiveDate::from_ymd_opt(2025, 1, 1),
                    None,
                    None,
                    NaiveDate::from_ymd_opt(2025, 1, 15),
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
            fn returns_fully_complete_boolean() {
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
                assert_eq!(item.fully_complete(), false);
            }

            #[test]
            fn returns_priority_when_present_or_absent() {
                let status = TaskStatus::new(
                    TaskStatusSymbol::new(' '),
                    "Todo",
                    TaskStatusType::Todo,
                );
                let item_without = TaskListItem::new(
                    TaskDates::default(),
                    None,
                    status.clone(),
                    false,
                );
                let item_with = TaskListItem::new(
                    TaskDates::default(),
                    Some(TaskPriority::Highest),
                    status,
                    false,
                );
                assert_eq!(item_without.priority(), None);
                assert_eq!(item_with.priority(), Some(TaskPriority::Highest));
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
                    NaiveDate::from_ymd_opt(2025, 2, 1),
                    None,
                    None,
                );
                let item = TaskListItem::new(dates, None, status, false);

                assert_eq!(item.dates(), dates);
                assert_eq!(
                    item.dates().due,
                    NaiveDate::from_ymd_opt(2025, 2, 1)
                );
            }
        }
    }

    mod task_priority {
        use pretty_assertions::assert_eq;
        use rstest::rstest;

        use super::*;

        #[rstest]
        #[case(TaskPriority::Lowest, "lowest")]
        #[case(TaskPriority::Low, "low")]
        #[case(TaskPriority::Normal, "normal")]
        #[case(TaskPriority::Medium, "medium")]
        #[case(TaskPriority::High, "high")]
        #[case(TaskPriority::Highest, "highest")]
        fn returns_canonical_name_for_each_level(
            #[case] priority: TaskPriority,
            #[case] expected: &str,
        ) {
            assert_eq!(priority.as_str(), expected);
            assert_eq!(format!("{priority}"), expected);
        }

        #[rstest]
        #[case("🔺", Some(TaskPriority::Highest))]
        #[case("🔺\u{FE0F}", Some(TaskPriority::Highest))]
        #[case("⏫", Some(TaskPriority::High))]
        #[case("⏫\u{FE0F}", Some(TaskPriority::High))]
        #[case("🔼", Some(TaskPriority::Medium))]
        #[case("🔼\u{FE0F}", Some(TaskPriority::Medium))]
        #[case("🔽", Some(TaskPriority::Low))]
        #[case("🔽\u{FE0F}", Some(TaskPriority::Low))]
        #[case("⏬", Some(TaskPriority::Lowest))]
        #[case("⏬\u{FE0F}", Some(TaskPriority::Lowest))]
        #[case("⭐", None)]
        #[case("", None)]
        fn parses_priority_emojis_with_and_without_variation_selector(
            #[case] emoji: &str,
            #[case] expected: Option<TaskPriority>,
        ) {
            assert_eq!(TaskPriority::from_emoji(emoji), expected);
        }

        #[rstest]
        #[case("lowest", Ok(TaskPriority::Lowest))]
        #[case("LOW", Ok(TaskPriority::Low))]
        #[case("Normal", Ok(TaskPriority::Normal))]
        #[case("medium", Ok(TaskPriority::Medium))]
        #[case("HIGH", Ok(TaskPriority::High))]
        #[case("highest", Ok(TaskPriority::Highest))]
        #[case("🔺", Ok(TaskPriority::Highest))]
        #[case("invalid", Err(()))]
        fn parses_names_and_emojis_case_insensitively(
            #[case] input: &str,
            #[case] expected: Result<TaskPriority, ()>,
        ) {
            assert_eq!(input.parse::<TaskPriority>(), expected);
        }

        #[test]
        fn orders_priorities_from_lowest_to_highest() {
            assert!(TaskPriority::Lowest < TaskPriority::Low);
            assert!(TaskPriority::Low < TaskPriority::Normal);
            assert!(TaskPriority::Normal < TaskPriority::Medium);
            assert!(TaskPriority::Medium < TaskPriority::High);
            assert!(TaskPriority::High < TaskPriority::Highest);
        }
    }

    mod task_dates {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn returns_true_when_no_dates_are_set() {
            let dates = TaskDates::default();

            assert_eq!(dates.is_empty(), true);
            assert_eq!(dates.created, None);
            assert_eq!(dates.scheduled, None);
            assert_eq!(dates.start, None);
            assert_eq!(dates.due, None);
            assert_eq!(dates.done, None);
            assert_eq!(dates.cancelled, None);
        }

        #[test]
        fn returns_false_when_any_date_is_set() {
            let dates = TaskDates::new(
                None,
                None,
                None,
                NaiveDate::from_ymd_opt(2025, 1, 15),
                None,
                None,
            );

            assert_eq!(dates.is_empty(), false);
            assert_eq!(dates.due(), NaiveDate::from_ymd_opt(2025, 1, 15));
        }

        #[test]
        fn returns_configured_date_values() {
            let created = NaiveDate::from_ymd_opt(2025, 1, 1);
            let scheduled = NaiveDate::from_ymd_opt(2025, 1, 2);
            let start = NaiveDate::from_ymd_opt(2025, 1, 3);
            let due = NaiveDate::from_ymd_opt(2025, 1, 4);
            let done = NaiveDate::from_ymd_opt(2025, 1, 5);
            let cancelled = NaiveDate::from_ymd_opt(2025, 1, 6);
            let dates =
                TaskDates::new(created, scheduled, start, due, done, cancelled);

            assert_eq!(dates.created(), created);
            assert_eq!(dates.scheduled(), scheduled);
            assert_eq!(dates.start(), start);
            assert_eq!(dates.due(), due);
            assert_eq!(dates.done(), done);
            assert_eq!(dates.cancelled(), cancelled);
        }
    }

    mod list_text {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn stores_raw_and_clean_text() {
            let text = ListText::new("raw text", "clean text");

            assert_eq!(text.raw(), "raw text");
            assert_eq!(text.clean(), "clean text");
            assert_eq!(format!("{text}"), "clean text");
        }

        #[test]
        fn converts_from_str_and_tuples() {
            let from_str: ListText = "plain".into();
            assert_eq!(from_str.raw(), "plain");
            assert_eq!(from_str.clean(), "plain");

            let from_tuple: ListText = ("raw", "clean").into();
            assert_eq!(from_tuple.raw(), "raw");
            assert_eq!(from_tuple.clean(), "clean");
        }
    }
}
