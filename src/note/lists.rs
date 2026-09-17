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

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

use super::field::NoteFieldValue;
use crate::{DateValue, FieldKey, SourceLine, Tag, TaskStatus};
/// Compact inline field map for a list item.
pub(crate) type ListFieldMap = IndexMap<FieldKey, Box<[NoteFieldValue]>>;

/// A Markdown list item with a classified [`ListItemType`], inline fields,
/// tags, and source line positioning information.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct ListItem {
    text: ListText,
    kind: ListItemType,
    depth: u8,
    line: Option<SourceLine>,
    parent: Option<SourceLine>,
    is_ordered: bool,
    fields: Option<Box<ListFieldMap>>,
    tags: Box<[Tag]>,
}

impl ListItem {
    /// Creates a list item with default positioning.
    #[inline]
    #[must_use]
    pub fn new<T: Into<ListText>>(text: T, kind: ListItemType) -> Self {
        Self {
            text: text.into(),
            kind,
            depth: 0,
            line: None,
            parent: None,
            is_ordered: false,
            fields: None,
            tags: Box::default(),
        }
    }

    /// Attaches the item's 1-indexed source line.
    #[inline]
    #[must_use]
    pub fn with_line(mut self, line: Option<SourceLine>) -> Self {
        self.line = line;
        self
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
    /// re-surfaced here as queryable data rather than discarded after
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

    /// Returns the plain or normalized text representation holding both raw
    /// and clean variants.
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
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "part of ListItem API; consumed by query resolution in \
                      issue 08"
        )
    )]
    pub(crate) fn fields(&self) -> Option<&ListFieldMap> {
        self.fields.as_deref()
    }

    /// Returns the item's 0-indexed nesting level.
    #[inline]
    #[must_use]
    pub const fn depth(&self) -> u8 {
        self.depth
    }

    /// Returns the item's 1-indexed source line, or `None` if the position
    /// has not been assigned yet.
    #[inline]
    #[must_use]
    pub const fn line(&self) -> Option<SourceLine> {
        self.line
    }

    /// Returns the immediate parent list item's 1-indexed source line, if
    /// this item is nested inside another list item.
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

    /// Returns this item's [`TaskListItem`] data, or [`None`] if this list
    /// item is not classified as a Task.
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
    Hash,
    Ord,
    PartialEq,
    PartialOrd,
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
/// use traces_pkm::{DateValue, TaskDates};
///
/// let mut dates = TaskDates::default();
/// dates.due = NaiveDate::from_ymd_opt(2025, 1, 15).map(DateValue::from);
/// assert!(!dates.is_empty());
/// assert_eq!(
///     dates.due(),
///     NaiveDate::from_ymd_opt(2025, 1, 15).map(DateValue::from)
/// );
/// ```
#[derive(
    Copy, Clone, Debug, Default, Eq, Hash, PartialEq, Deserialize, Serialize,
)]
pub struct TaskDates {
    /// Date when the task was created (`➕` or `[created::]`).
    pub created: Option<DateValue>,
    /// Date when the task is scheduled (`⏳` or `[scheduled::]`).
    pub scheduled: Option<DateValue>,
    /// Date when work on the task begins (`🛫` or `[start::]`).
    pub start: Option<DateValue>,
    /// Date when the task is due (`📅` or `[due::]`).
    pub due: Option<DateValue>,
    /// Date when the task was completed (`✅` or `[done::]`).
    pub done: Option<DateValue>,
    /// Date when the task was cancelled (`❌` or `[cancelled::]`).
    pub cancelled: Option<DateValue>,
}
impl TaskDates {
    /// Creates a new `TaskDates` instance with all dates specified.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use chrono::NaiveDate;
    /// use traces_pkm::{DateValue, TaskDates};
    ///
    /// let due = NaiveDate::from_ymd_opt(2025, 1, 15).map(DateValue::from);
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
        created: Option<DateValue>,
        scheduled: Option<DateValue>,
        start: Option<DateValue>,
        due: Option<DateValue>,
        done: Option<DateValue>,
        cancelled: Option<DateValue>,
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
    /// use traces_pkm::{DateValue, TaskDates};
    ///
    /// let mut dates = TaskDates::default();
    /// dates.created = NaiveDate::from_ymd_opt(2025, 1, 1).map(DateValue::from);
    /// assert_eq!(
    ///     dates.created(),
    ///     NaiveDate::from_ymd_opt(2025, 1, 1).map(DateValue::from)
    /// );
    /// ```
    #[inline]
    #[must_use]
    pub const fn created(&self) -> Option<DateValue> {
        self.created
    }

    /// Returns the task's scheduled date, if set.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use chrono::NaiveDate;
    /// use traces_pkm::{DateValue, TaskDates};
    ///
    /// let mut dates = TaskDates::default();
    /// dates.scheduled = NaiveDate::from_ymd_opt(2025, 1, 10).map(DateValue::from);
    /// assert_eq!(
    ///     dates.scheduled(),
    ///     NaiveDate::from_ymd_opt(2025, 1, 10).map(DateValue::from)
    /// );
    /// ```
    #[inline]
    #[must_use]
    pub const fn scheduled(&self) -> Option<DateValue> {
        self.scheduled
    }

    /// Returns the task's start date, if set.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use chrono::NaiveDate;
    /// use traces_pkm::{DateValue, TaskDates};
    ///
    /// let mut dates = TaskDates::default();
    /// dates.start = NaiveDate::from_ymd_opt(2025, 1, 12).map(DateValue::from);
    /// assert_eq!(
    ///     dates.start(),
    ///     NaiveDate::from_ymd_opt(2025, 1, 12).map(DateValue::from)
    /// );
    /// ```
    #[inline]
    #[must_use]
    pub const fn start(&self) -> Option<DateValue> {
        self.start
    }

    /// Returns the task's due date, if set.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use chrono::NaiveDate;
    /// use traces_pkm::{DateValue, TaskDates};
    ///
    /// let mut dates = TaskDates::default();
    /// dates.due = NaiveDate::from_ymd_opt(2025, 1, 15).map(DateValue::from);
    /// assert_eq!(
    ///     dates.due(),
    ///     NaiveDate::from_ymd_opt(2025, 1, 15).map(DateValue::from)
    /// );
    /// ```
    #[inline]
    #[must_use]
    pub const fn due(&self) -> Option<DateValue> {
        self.due
    }

    /// Returns the task's completion date, if set.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use chrono::NaiveDate;
    /// use traces_pkm::{DateValue, TaskDates};
    ///
    /// let mut dates = TaskDates::default();
    /// dates.done = NaiveDate::from_ymd_opt(2025, 1, 20).map(DateValue::from);
    /// assert_eq!(
    ///     dates.done(),
    ///     NaiveDate::from_ymd_opt(2025, 1, 20).map(DateValue::from)
    /// );
    /// ```
    #[inline]
    #[must_use]
    pub const fn done(&self) -> Option<DateValue> {
        self.done
    }

    /// Returns the task's cancellation date, if set.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use chrono::NaiveDate;
    /// use traces_pkm::{DateValue, TaskDates};
    ///
    /// let mut dates = TaskDates::default();
    /// dates.cancelled = NaiveDate::from_ymd_opt(2025, 1, 22).map(DateValue::from);
    /// assert_eq!(
    ///     dates.cancelled(),
    ///     NaiveDate::from_ymd_opt(2025, 1, 22).map(DateValue::from)
    /// );
    /// ```
    #[inline]
    #[must_use]
    pub const fn cancelled(&self) -> Option<DateValue> {
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
    Clone, Debug, Default, Eq, Hash, PartialEq, Deserialize, Serialize,
)]
pub struct ListText {
    /// Source text minus the leading `[<char>] ` marker prefix only.
    pub raw: String,
    /// Normalized display text with task metadata stripped, or `None` when
    /// identical to `raw`.
    pub clean: Option<String>,
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
pub fn descendants_of(
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
            fn stores_the_given_kind(#[case] kind: ListItemType) {
                let item = ListItem::new("task item", kind.clone());

                assert_eq!(item.text().raw(), "task item");
                assert_eq!(item.text().clean(), "task item");
                assert_eq!(item.raw_text(), "task item");
                assert_eq!(item.clean_text(), "task item");
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
                let item =
                    ListItem::new("task item", done_task()).with_fields(fields);

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
                let item = ListItem::new("plain item", ListItemType::Plain);

                assert_eq!(item.fields(), None);
            }
        }

        mod tags {
            use pretty_assertions::assert_eq;

            use super::*;

            #[test]
            fn stores_tags_when_attached_with_with_tags() {
                let tags = vec![Tag::parse("#project").expect("valid tag")];
                let item = ListItem::new("task item", done_task())
                    .with_tags(tags.clone());

                assert_eq!(item.tags(), tags.as_slice());
            }

            #[test]
            fn has_no_tags_by_default() {
                let item = ListItem::new("plain item", ListItemType::Plain);

                assert_eq!(item.tags(), []);
            }
        }

        mod position {
            use pretty_assertions::assert_eq;

            use super::*;
            #[test]
            fn defaults_position_to_zero_and_no_parent() {
                let item = ListItem::new("item", ListItemType::Plain);

                assert_eq!(item.depth(), 0);
                assert_eq!(item.line(), None);
                assert_eq!(item.parent(), None);
                assert!(!item.is_ordered());
            }

            #[test]
            fn builders_set_line_depth_parent_and_ordering() {
                let item = ListItem::new("item", ListItemType::Plain)
                    .with_line(Some(SourceLine::new(3).expect("non-zero")))
                    .with_depth(2)
                    .with_parent(Some(SourceLine::new(1).expect("non-zero")))
                    .with_is_ordered(true);

                assert_eq!(
                    item.line(),
                    Some(SourceLine::new(3).expect("non-zero"))
                );
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
            let parent = ListItem::new("parent", ListItemType::Plain)
                .with_depth(0)
                .with_line(Some(SourceLine::new(1).expect("non-zero")));
            let child1 = ListItem::new("child 1", ListItemType::Plain)
                .with_depth(1)
                .with_parent(Some(SourceLine::new(1).expect("non-zero")));
            let grandchild = ListItem::new("grandchild", ListItemType::Plain)
                .with_depth(2)
                .with_parent(Some(SourceLine::new(2).expect("non-zero")));
            let child2 = ListItem::new("child 2", ListItemType::Plain)
                .with_depth(1)
                .with_parent(Some(SourceLine::new(1).expect("non-zero")));
            let sibling =
                ListItem::new("sibling", ListItemType::Plain).with_depth(0);

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
                ListItem::new("parent", ListItemType::Plain).with_depth(0);
            let sibling =
                ListItem::new("sibling", ListItemType::Plain).with_depth(0);
            let slice = [parent, sibling];
            assert_eq!(descendants_of(&slice[1..], 0).count(), 0);
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
                    NaiveDate::from_ymd_opt(2025, 2, 1).map(Into::into),
                    None,
                    None,
                );
                let item = TaskListItem::new(dates, None, status, false);

                assert_eq!(item.dates(), dates);
                assert_eq!(
                    item.dates().due,
                    NaiveDate::from_ymd_opt(2025, 2, 1).map(Into::into)
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
                NaiveDate::from_ymd_opt(2025, 1, 15).map(Into::into),
                None,
                None,
            );

            assert_eq!(dates.is_empty(), false);
            assert_eq!(
                dates.due(),
                NaiveDate::from_ymd_opt(2025, 1, 15).map(Into::into)
            );
        }

        #[test]
        fn returns_configured_date_values() {
            let created =
                NaiveDate::from_ymd_opt(2025, 1, 1).map(DateValue::from);
            let scheduled =
                NaiveDate::from_ymd_opt(2025, 1, 2).map(DateValue::from);
            let start =
                NaiveDate::from_ymd_opt(2025, 1, 3).map(DateValue::from);
            let due = NaiveDate::from_ymd_opt(2025, 1, 4).map(DateValue::from);
            let done = NaiveDate::from_ymd_opt(2025, 1, 5).map(DateValue::from);
            let cancelled =
                NaiveDate::from_ymd_opt(2025, 1, 6).map(DateValue::from);
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
        fn sets_clean_to_none_when_equal_to_raw() {
            let text = ListText::new("same", "same");
            assert_eq!(text.clean, None);
            assert_eq!(text.clean(), "same");

            let from_str: ListText = "plain".into();
            assert_eq!(from_str.clean, None);
            assert_eq!(from_str.clean(), "plain");

            let diff = ListText::new("raw", "clean");
            assert_eq!(diff.clean, Some("clean".to_owned()));
            assert_eq!(diff.clean(), "clean");
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
