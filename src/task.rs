//! Task status, priority, and lifecycle-date domain model shared by note
//! parsing, display, and querying.
//!
//! # Key Types
//!
//! - [`TaskStatus`]: named, typed status keyed by its marker symbol.
//! - [`TaskStatusMap`]: lookup table for resolving marker symbols to configured
//!   statuses.
//! - [`TaskStatusType`]: workflow classification (todo, in-progress, on-hold,
//!   done, cancelled, non-task).
//! - [`TaskStatusSymbol`]: marker character inside `[<char>]`.
//! - [`TaskDateSet`]: set of up to six lifecycle dates.
//! - [`TaskDate`]: single lifecycle date occurrence paired with its slot.
//! - [`TaskDateType`]: enum of the six lifecycle date slots.
//! - [`TaskPriority`]: six-level task priority enum mapped to emoji and text
//!   representations.
//!
//! # Examples
//!
//! ```rust
//! use traces_pkm::{TaskPriority, TaskStatus, TaskStatusType};
//!
//! let status = TaskStatus::default();
//! assert_eq!(status.name(), "Todo");
//! assert_eq!(status.kind(), TaskStatusType::Todo);
//! assert_eq!(TaskPriority::Highest.as_str(), "highest");
//! ```
use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::{DateValue, delimiter::DelimiterType};

/// A named, typed task status keyed by its marker [`TaskStatusSymbol`].
///
/// Encapsulates the marker character, display name, and workflow classification
/// type for task items.
///
/// # Examples
///
/// ```rust
/// use traces_pkm::{TaskStatus, TaskStatusSymbol, TaskStatusType};
///
/// let status = TaskStatus::default();
/// assert_eq!(status.symbol().as_char(), ' ');
/// assert_eq!(status.name(), "Todo");
/// assert_eq!(status.kind(), TaskStatusType::Todo);
/// ```
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct TaskStatus {
    symbol: TaskStatusSymbol,
    name: String,
    kind: TaskStatusType,
}

impl TaskStatus {
    /// Creates a task status from its marker symbol, display name, and workflow
    /// type.
    #[inline]
    #[must_use]
    pub(crate) fn new<S: Into<String>>(
        symbol: TaskStatusSymbol,
        name: S,
        kind: TaskStatusType,
    ) -> Self {
        Self {
            symbol,
            name: name.into(),
            kind,
        }
    }

    /// Returns the marker symbol.
    #[inline]
    #[must_use]
    pub const fn symbol(&self) -> TaskStatusSymbol {
        self.symbol
    }

    /// Returns the display name exactly as configured.
    #[inline]
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the workflow status type.
    #[inline]
    #[must_use]
    pub const fn kind(&self) -> TaskStatusType {
        self.kind
    }
}

impl Default for TaskStatus {
    #[inline]
    fn default() -> Self {
        Self::new(TaskStatusSymbol::new(' '), "Todo", TaskStatusType::Todo)
    }
}

/// A [`TaskStatus`] lookup table, built once during configuration resolution.
///
/// Provides lookup by marker symbol, display name, and workflow type. Default
/// statuses are always present; configuration can add new statuses or override
/// default ones that share a symbol.
///
/// # Examples
///
/// ```rust
/// use traces_pkm::TaskStatusMap;
///
/// let map = TaskStatusMap::default();
/// assert!(format!("{map:?}").contains("Todo"));
/// ```
#[derive(Clone, Debug)]
pub struct TaskStatusMap {
    symbols: HashMap<TaskStatusSymbol, TaskStatus>,
    names: HashMap<String, TaskStatus>,
    kinds: HashMap<TaskStatusType, Vec<TaskStatus>>,
}

impl TaskStatusMap {
    /// Looks up a status by its exact marker symbol.
    #[inline]
    #[must_use]
    pub(crate) fn by_symbol(
        &self,
        symbol: TaskStatusSymbol,
    ) -> Option<&TaskStatus> {
        self.symbols.get(&symbol)
    }

    /// Resolves a scanned marker `symbol` to its configured [`TaskStatus`].
    ///
    /// Falls back to an incomplete todo status when no configured status uses
    /// `symbol`, preserving `symbol` on the fallback for diagnostics. Unknown
    /// markers are never downgraded to plain bullets: this is the custom marker
    /// scanner's only source of truth for marker-to-status resolution.
    #[inline]
    #[must_use]
    pub(crate) fn resolve(&self, symbol: char) -> TaskStatus {
        self.by_symbol(TaskStatusSymbol::new(symbol)).cloned().unwrap_or_else(
            || {
                TaskStatus::new(
                    TaskStatusSymbol::new(symbol),
                    "Todo",
                    TaskStatusType::Todo,
                )
            },
        )
    }

    /// Looks up a status by display name, normalized by case-folding, trimming,
    /// and collapsing internal whitespace to a single space.
    #[inline]
    #[must_use]
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "no current caller outside tests; consumed by \
                      task.status query filtering added in a later \
                      task-system issue"
        )
    )]
    pub(crate) fn by_name(&self, name: &str) -> Option<&TaskStatus> {
        self.names.get(&normalize_name(name))
    }

    /// Returns every status sharing `kind`, e.g. every symbol that resolves to
    /// [`TaskStatusType::Done`].
    #[inline]
    #[must_use]
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "no current caller outside tests; consumed by \
                      task.status query filtering added in a later \
                      task-system issue"
        )
    )]
    pub(crate) fn by_type(&self, kind: TaskStatusType) -> &[TaskStatus] {
        self.kinds.get(&kind).map_or(&[], Vec::as_slice)
    }

    /// Adds a status, or overrides the existing status sharing its symbol.
    ///
    /// Overriding removes the replaced status's stale by-name and by-type
    /// entries before indexing the new one, so all three lookups stay
    /// consistent.
    ///
    /// # Errors
    ///
    /// - [`TaskError::ProhibitedStatusSymbol`] if the status symbol is one of
    ///   `(`, `)`, `[`, `]`, `{`, or `}`.
    #[inline]
    pub(crate) fn insert(
        &mut self,
        status: TaskStatus,
    ) -> Result<(), TaskError> {
        let symbol = status.symbol.as_char();

        if DelimiterType::classify(symbol).is_some() {
            return Err(TaskError::ProhibitedStatusSymbol {
                symbol,
            });
        }

        self.purge_stale_entries(&status);
        self.index_status(status);

        Ok(())
    }

    /// Removes stale entries left by a previous status sharing the same symbol
    /// from the `kinds` and `names` maps. No-op if `status.symbol` has no
    /// predecessor.
    fn purge_stale_entries(&mut self, status: &TaskStatus) {
        let Some(previous) = self.symbols.get(&status.symbol) else {
            return;
        };
        if let Some(bucket) = self.kinds.get_mut(&previous.kind) {
            bucket.retain(|existing| existing.symbol != previous.symbol);
        }
        let previous_key = normalize_name(&previous.name);
        if self
            .names
            .get(&previous_key)
            .is_some_and(|current| current.symbol == previous.symbol)
        {
            self.names.remove(&previous_key);
        }
    }

    /// Indexes `status` into all three lookup maps in one step. Caller must
    /// call [`Self::purge_stale_entries`] first when overriding an existing
    /// symbol.
    fn index_status(&mut self, status: TaskStatus) {
        self.kinds.entry(status.kind).or_default().push(status.clone());
        self.symbols.insert(status.symbol, status.clone());
        self.names.insert(normalize_name(&status.name), status);
    }
}

impl Default for TaskStatusMap {
    /// Builds the map from the always-available default statuses.
    #[inline]
    fn default() -> Self {
        let statuses = default_statuses();
        let mut map = Self {
            symbols: HashMap::with_capacity(statuses.len()),
            names: HashMap::with_capacity(statuses.len()),
            kinds: HashMap::with_capacity(statuses.len()),
        };
        for status in statuses {
            map.index_status(status);
        }
        map
    }
}

/// The workflow classification of a [`TaskStatus`].
///
/// Maps statuses to standard lifecycle states used in task views and completion
/// rollups.
///
/// # Examples
///
/// ```rust
/// use traces_pkm::TaskStatusType;
///
/// assert_ne!(TaskStatusType::Done, TaskStatusType::InProgress);
/// assert_eq!(TaskStatusType::NonTask, TaskStatusType::NonTask);
/// ```
#[derive(Copy, Clone, Debug, Eq, Hash, PartialEq, Deserialize, Serialize)]
pub enum TaskStatusType {
    /// Not yet started.
    Todo,
    /// Actively being worked on.
    InProgress,
    /// Paused, waiting on something external.
    OnHold,
    /// Finished.
    Done,
    /// Abandoned; excluded from both active and completed views.
    Cancelled,
    /// A checkbox status that never becomes a Task (reserved for future
    /// configured statuses; no default status uses it).
    NonTask,
}

impl TaskStatusType {
    /// Derives the tri-state completion value for this status type.
    ///
    /// Returns:
    /// - `Some(true)` for [`Self::Done`]
    /// - `None` for [`Self::Cancelled`]
    /// - `Some(false)` for all other status types
    #[inline]
    #[must_use]
    pub(crate) const fn completed(self) -> Option<bool> {
        match self {
            Self::Done => Some(true),
            Self::Cancelled => None,
            Self::Todo | Self::InProgress | Self::OnHold | Self::NonTask => {
                Some(false)
            }
        }
    }

    /// Derives the boolean completion rollup for this status type.
    ///
    /// Returns `true` for completed or cancelled tasks, and `false` for
    /// incomplete tasks.
    #[inline]
    #[must_use]
    pub(crate) const fn is_complete(self) -> bool {
        !matches!(self.completed(), Some(false))
    }
}

/// The marker character inside `[<char>]`, e.g. `' '`, `'x'`, `'/'`, `'-'`.
///
/// Wraps a single `char` without validation, serving as the lookup key for
/// standard and custom-scanned task markers.
///
/// # Examples
///
/// ```rust
/// use traces_pkm::TaskStatusSymbol;
///
/// let sym = TaskStatusSymbol::new('x');
/// assert_eq!(sym.as_char(), 'x');
/// assert_eq!(sym, 'x');
/// ```
#[derive(Copy, Clone, Debug, Eq, Hash, PartialEq, Deserialize, Serialize)]
pub struct TaskStatusSymbol(char);

impl TaskStatusSymbol {
    /// Wraps `symbol` as a task status marker character.
    #[inline]
    #[must_use]
    pub const fn new(symbol: char) -> Self {
        Self(symbol)
    }

    /// Returns the underlying marker character.
    #[inline]
    #[must_use]
    pub const fn as_char(&self) -> char {
        self.0
    }
}

impl From<char> for TaskStatusSymbol {
    #[inline]
    fn from(symbol: char) -> Self {
        Self::new(symbol)
    }
}

impl PartialEq<char> for TaskStatusSymbol {
    #[inline]
    fn eq(&self, other: &char) -> bool {
        self.0 == *other
    }
}

impl PartialEq<TaskStatusSymbol> for char {
    #[inline]
    fn eq(&self, other: &TaskStatusSymbol) -> bool {
        *self == other.0
    }
}

/// The default statuses always available, before any configured overrides.
///
/// `'x'` and `'X'` both resolve to `Done`, matching the custom marker scanner's
/// case-insensitive acceptance of the done marker.
fn default_statuses() -> [TaskStatus; 6] {
    [
        TaskStatus::new(
            TaskStatusSymbol::new(' '),
            "Todo",
            TaskStatusType::Todo,
        ),
        TaskStatus::new(
            TaskStatusSymbol::new('x'),
            "Done",
            TaskStatusType::Done,
        ),
        TaskStatus::new(
            TaskStatusSymbol::new('X'),
            "Done",
            TaskStatusType::Done,
        ),
        TaskStatus::new(
            TaskStatusSymbol::new('/'),
            "In Progress",
            TaskStatusType::InProgress,
        ),
        TaskStatus::new(
            TaskStatusSymbol::new('-'),
            "Cancelled",
            TaskStatusType::Cancelled,
        ),
        TaskStatus::new(
            TaskStatusSymbol::new('!'),
            "On Hold",
            TaskStatusType::OnHold,
        ),
    ]
}

/// Normalizes a status name for lookup: case-folds, trims, and collapses
/// internal whitespace to a single space.
///
/// Display names ([`TaskStatus::name`]) remain exactly as configured; only the
/// lookup key is normalized.
fn normalize_name(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut words = name.split_whitespace();
    if let Some(first) = words.next() {
        for ch in first.chars().flat_map(char::to_lowercase) {
            out.push(ch);
        }
        for word in words {
            out.push(' ');
            for ch in word.chars().flat_map(char::to_lowercase) {
                out.push(ch);
            }
        }
    }
    out
}

/// Set of up to six task lifecycle dates, at most one per [`TaskDateType`]
/// slot.
///
/// Holds one optional calendar date per lifecycle kind:
///
/// - [`TaskDateType::Created`]
/// - [`TaskDateType::Scheduled`]
/// - [`TaskDateType::Start`]
/// - [`TaskDateType::Due`]
/// - [`TaskDateType::Done`]
/// - [`TaskDateType::Cancelled`]
///
/// Slots without a parsed date stay unset.
///
/// # Examples
///
/// ```rust
/// use chrono::NaiveDate;
/// use traces_pkm::{DateValue, TaskDate, TaskDateSet, TaskDateType};
///
/// let due =
///     NaiveDate::from_ymd_opt(2025, 1, 15).map(DateValue::from).unwrap();
/// let dates = TaskDateSet::from_iter([TaskDate::new(TaskDateType::Due, due)]);
/// assert_eq!(dates.get(TaskDateType::Due), Some(due));
/// assert!(!dates.is_empty());
/// ```
#[derive(
    Copy, Clone, Debug, Default, Eq, Hash, PartialEq, Deserialize, Serialize,
)]
pub struct TaskDateSet {
    created: Option<DateValue>,
    scheduled: Option<DateValue>,
    start: Option<DateValue>,
    due: Option<DateValue>,
    done: Option<DateValue>,
    cancelled: Option<DateValue>,
}

impl TaskDateSet {
    /// Inserts a [`TaskDate`] into its corresponding slot.
    ///
    /// Adheres to first-wins precedence: if the slot is already occupied, the
    /// incoming date is ignored.
    #[inline]
    pub(crate) fn insert(&mut self, date: TaskDate) {
        let slot = match date.kind {
            TaskDateType::Created => &mut self.created,
            TaskDateType::Scheduled => &mut self.scheduled,
            TaskDateType::Start => &mut self.start,
            TaskDateType::Due => &mut self.due,
            TaskDateType::Done => &mut self.done,
            TaskDateType::Cancelled => &mut self.cancelled,
        };
        if slot.is_none() {
            *slot = Some(date.date);
        }
    }

    /// Returns the date value for `kind`, if set.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use chrono::NaiveDate;
    /// use traces_pkm::{DateValue, TaskDate, TaskDateSet, TaskDateType};
    ///
    /// let due =
    ///     NaiveDate::from_ymd_opt(2025, 1, 15).map(DateValue::from).unwrap();
    /// let dates = TaskDateSet::from_iter([TaskDate::new(TaskDateType::Due, due)]);
    /// assert_eq!(dates.get(TaskDateType::Due), Some(due));
    /// assert_eq!(dates.get(TaskDateType::Start), None);
    /// ```
    #[inline]
    #[must_use]
    pub const fn get(&self, kind: TaskDateType) -> Option<DateValue> {
        match kind {
            TaskDateType::Created => self.created,
            TaskDateType::Scheduled => self.scheduled,
            TaskDateType::Start => self.start,
            TaskDateType::Due => self.due,
            TaskDateType::Done => self.done,
            TaskDateType::Cancelled => self.cancelled,
        }
    }

    /// Returns `true` if no lifecycle dates are populated.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use traces_pkm::TaskDateSet;
    ///
    /// let dates = TaskDateSet::default();
    /// assert!(dates.is_empty());
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

    /// Returns an iterator over occupied lifecycle dates in declaration order.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use chrono::NaiveDate;
    /// use traces_pkm::{DateValue, TaskDate, TaskDateSet, TaskDateType};
    ///
    /// let d1 = NaiveDate::from_ymd_opt(2025, 1, 1).map(DateValue::from).unwrap();
    /// let d2 = NaiveDate::from_ymd_opt(2025, 1, 15).map(DateValue::from).unwrap();
    /// let dates = TaskDateSet::from_iter([
    ///     TaskDate::new(TaskDateType::Due, d2),
    ///     TaskDate::new(TaskDateType::Created, d1),
    /// ]);
    /// let kinds: Vec<_> = dates.iter().map(|d| d.kind()).collect();
    /// assert_eq!(kinds, vec![TaskDateType::Created, TaskDateType::Due]);
    /// ```
    #[inline]
    #[must_use]
    pub fn iter(&self) -> TaskDateSetIter {
        self.into_iter()
    }
}

impl IntoIterator for TaskDateSet {
    type IntoIter = TaskDateSetIter;
    type Item = TaskDate;

    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        TaskDateSetIter {
            set: self,
            index: 0,
        }
    }
}

impl IntoIterator for &TaskDateSet {
    type IntoIter = TaskDateSetIter;
    type Item = TaskDate;

    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        (*self).into_iter()
    }
}

impl FromIterator<TaskDate> for TaskDateSet {
    #[inline]
    fn from_iter<T: IntoIterator<Item = TaskDate>>(iter: T) -> Self {
        let mut set = Self::default();
        for date in iter {
            set.insert(date);
        }
        set
    }
}

impl Extend<TaskDate> for TaskDateSet {
    #[inline]
    fn extend<T: IntoIterator<Item = TaskDate>>(&mut self, iter: T) {
        for date in iter {
            self.insert(date);
        }
    }
}

/// An iterator over the occupied lifecycle dates in a [`TaskDateSet`].
///
/// Yields [`TaskDate`] elements in declaration order:
/// 1. [`TaskDateType::Created`]
/// 2. [`TaskDateType::Scheduled`]
/// 3. [`TaskDateType::Start`]
/// 4. [`TaskDateType::Due`]
/// 5. [`TaskDateType::Done`]
/// 6. [`TaskDateType::Cancelled`]
#[derive(Clone, Debug)]
pub struct TaskDateSetIter {
    set: TaskDateSet,
    /// Position within [`TaskDateType::ALL`], bounded by the six-slot array.
    index: u8,
}

impl Iterator for TaskDateSetIter {
    type Item = TaskDate;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        while let Some(&kind) = TaskDateType::ALL.get(usize::from(self.index)) {
            self.index = self.index.saturating_add(1);
            if let Some(date) = self.set.get(kind) {
                return Some(TaskDate::new(kind, date));
            }
        }
        None
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining_slots =
            TaskDateType::ALL.len().saturating_sub(usize::from(self.index));
        (0, Some(remaining_slots))
    }
}

/// A single task lifecycle date occurrence paired with its slot.
///
/// # Examples
///
/// ```rust
/// use chrono::NaiveDate;
/// use traces_pkm::{DateValue, TaskDate, TaskDateType};
///
/// let date_val =
///     NaiveDate::from_ymd_opt(2025, 1, 15).map(DateValue::from).unwrap();
/// let task_date = TaskDate::new(TaskDateType::Due, date_val);
/// assert_eq!(task_date.kind(), TaskDateType::Due);
/// assert_eq!(task_date.date(), date_val);
/// ```
#[derive(Copy, Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct TaskDate {
    kind: TaskDateType,
    date: DateValue,
}

impl TaskDate {
    /// Creates a new [`TaskDate`] pairing a lifecycle slot with its calendar
    /// date.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use chrono::NaiveDate;
    /// use traces_pkm::{DateValue, TaskDate, TaskDateType};
    ///
    /// let date_val =
    ///     NaiveDate::from_ymd_opt(2025, 1, 15).map(DateValue::from).unwrap();
    /// let task_date = TaskDate::new(TaskDateType::Due, date_val);
    /// assert_eq!(task_date.kind(), TaskDateType::Due);
    /// ```
    #[inline]
    #[must_use]
    pub const fn new(kind: TaskDateType, date: DateValue) -> Self {
        Self {
            kind,
            date,
        }
    }

    /// Returns the lifecycle slot for this date.
    #[inline]
    #[must_use]
    pub const fn kind(&self) -> TaskDateType {
        self.kind
    }

    /// Returns the calendar date value.
    #[inline]
    #[must_use]
    pub const fn date(&self) -> DateValue {
        self.date
    }
}

/// Lifecycle slot of a task date.
///
/// Represents the six recognized task lifecycle calendar dates supported by
/// Obsidian Tasks syntax and Dataview inline fields.
///
/// # Examples
///
/// ```rust
/// use traces_pkm::TaskDateType;
///
/// assert_eq!(TaskDateType::from_emoji("📅"), Some(TaskDateType::Due));
/// assert_eq!(TaskDateType::Due.field_keys(), &["due"]);
/// ```
#[derive(Copy, Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum TaskDateType {
    /// Date the task was created (`➕` or `[created::]`).
    Created,
    /// Date the task is scheduled (`⏳` or `[scheduled::]`).
    Scheduled,
    /// Date work on the task begins (`🛫` or `[start::]`).
    Start,
    /// Date the task is due (`📅`, `🗓`, or `[due::]`).
    Due,
    /// Date the task was completed (`✅`, `[done::]`, or `[completion::]`).
    Done,
    /// Date the task was cancelled (`❌` or `[cancelled::]`).
    Cancelled,
}

impl TaskDateType {
    /// Declaration order array defining deterministic iteration order for
    /// [`TaskDateSet::iter`].
    ///
    /// # Examples
    ///
    /// ```rust
    /// use traces_pkm::TaskDateType;
    ///
    /// assert_eq!(TaskDateType::ALL.len(), 6);
    /// assert_eq!(TaskDateType::ALL[0], TaskDateType::Created);
    /// ```
    pub const ALL: [Self; 6] = [
        Self::Created,
        Self::Scheduled,
        Self::Start,
        Self::Due,
        Self::Done,
        Self::Cancelled,
    ];
    /// Base emoji spellings in precedence order.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use traces_pkm::TaskDateType;
    ///
    /// assert_eq!(TaskDateType::EMOJIS[3], ("\u{1F4C5}", TaskDateType::Due));
    /// ```
    pub const EMOJIS: &'static [(&'static str, Self)] = &[
        ("\u{2795}", Self::Created),
        ("\u{23F3}", Self::Scheduled),
        ("\u{1F6EB}", Self::Start),
        ("\u{1F4C5}", Self::Due),
        ("\u{1F5D3}", Self::Due),
        ("\u{2705}", Self::Done),
        ("\u{274C}", Self::Cancelled),
    ];

    /// Resolves a lifecycle slot from an emoji, ignoring trailing variation
    /// selectors.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use traces_pkm::TaskDateType;
    ///
    /// assert_eq!(TaskDateType::from_emoji("📅"), Some(TaskDateType::Due));
    /// assert_eq!(
    ///     TaskDateType::from_emoji("📅\u{FE0F}"),
    ///     Some(TaskDateType::Due)
    /// );
    /// assert_eq!(TaskDateType::from_emoji("invalid"), None);
    /// ```
    #[inline]
    #[must_use]
    pub fn from_emoji(emoji: &str) -> Option<Self> {
        let trimmed = emoji.trim_end_matches('\u{FE0F}');
        Self::EMOJIS.iter().find_map(|&(e, kind)| {
            if e == trimmed {
                Some(kind)
            } else {
                None
            }
        })
    }

    /// Returns canonical inline field aliases for this lifecycle slot.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use traces_pkm::TaskDateType;
    ///
    /// assert_eq!(TaskDateType::Due.field_keys(), &["due"]);
    /// assert_eq!(TaskDateType::Done.field_keys(), &["done", "completion"]);
    /// ```
    #[inline]
    #[must_use]
    pub const fn field_keys(self) -> &'static [&'static str] {
        match self {
            Self::Created => &["created"],
            Self::Scheduled => &["scheduled"],
            Self::Start => &["start"],
            Self::Due => &["due"],
            Self::Done => &["done", "completion"],
            Self::Cancelled => &["cancelled"],
        }
    }

    /// Returns the canonical lowercase string representation of this slot.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use traces_pkm::TaskDateType;
    ///
    /// assert_eq!(TaskDateType::Due.as_str(), "due");
    /// assert_eq!(TaskDateType::Created.as_str(), "created");
    /// ```
    #[inline]
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Created => "created",
            Self::Scheduled => "scheduled",
            Self::Start => "start",
            Self::Due => "due",
            Self::Done => "done",
            Self::Cancelled => "cancelled",
        }
    }

    /// Returns `true` if `key` matches a recognized task metadata field name
    /// (any lifecycle date alias or `"priority"`), ignoring ASCII case.
    ///
    /// Expects `key` to be already trimmed.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use traces_pkm::TaskDateType;
    ///
    /// assert!(TaskDateType::is_field_key("due"));
    /// assert!(TaskDateType::is_field_key("COMPLETION"));
    /// assert!(TaskDateType::is_field_key("priority"));
    /// assert!(!TaskDateType::is_field_key("other"));
    /// ```
    #[inline]
    #[must_use]
    pub fn is_field_key(key: &str) -> bool {
        Self::ALL
            .iter()
            .flat_map(|kind| kind.field_keys().iter().copied())
            .chain(std::iter::once("priority"))
            .any(|candidate| key.eq_ignore_ascii_case(candidate))
    }
}

impl std::fmt::Display for TaskDateType {
    #[inline]
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for TaskDateType {
    type Err = TaskError;

    /// Parses a task date slot from its canonical name (e.g. `"due"`),
    /// case-insensitively.
    ///
    /// # Errors
    ///
    /// - [`TaskError::InvalidDateType`] if `s` is not a recognized date slot
    ///   name.
    #[inline]
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let trimmed = s.trim();
        for kind in Self::ALL {
            if trimmed.eq_ignore_ascii_case(kind.as_str()) {
                return Ok(kind);
            }
        }
        Err(TaskError::InvalidDateType {
            input: s.to_owned(),
        })
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
    /// Normal priority (stored as `None` on
    /// [`TaskListItem`](crate::TaskListItem) when unspecified).
    Normal,
    /// Medium priority (`🔼`).
    Medium,
    /// High priority (`⏫`).
    High,
    /// Highest priority (`🔺`).
    Highest,
}

impl TaskPriority {
    /// Base priority emoji spellings in severity order.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use traces_pkm::TaskPriority;
    ///
    /// assert_eq!(TaskPriority::EMOJIS[0], ("\u{1F53A}", TaskPriority::Highest));
    /// ```
    pub const EMOJIS: &'static [(&'static str, Self)] = &[
        ("\u{1F53A}", Self::Highest),
        ("\u{23EB}", Self::High),
        ("\u{1F53C}", Self::Medium),
        ("\u{1F53D}", Self::Low),
        ("\u{23EC}", Self::Lowest),
    ];

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

    /// Returns the numeric severity rank of this priority, `0` (`Lowest`)
    /// through `5` (`Highest`), in declaration order.
    ///
    /// Sort keys use this rank instead of the [`TaskPriority::as_str`] display
    /// name so `list.priority` orders by severity, not alphabetically. `Normal`
    /// resolves to rank `2` but is unreachable from parsing (no emoji or name
    /// maps to it; items with no priority store `None`).
    #[inline]
    #[must_use]
    pub const fn rank(self) -> u8 {
        match self {
            Self::Lowest => 0,
            Self::Low => 1,
            Self::Normal => 2,
            Self::Medium => 3,
            Self::High => 4,
            Self::Highest => 5,
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
        Self::EMOJIS.iter().find_map(|&(e, p)| {
            if e == trimmed {
                Some(p)
            } else {
                None
            }
        })
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

/// Parses a task priority from a name or emoji.
///
/// # Errors
///
/// - [`TaskError::InvalidPriority`] if `s` does not match a recognized priority
///   name or emoji.
impl std::str::FromStr for TaskPriority {
    type Err = TaskError;

    #[inline]
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "lowest" => Ok(Self::Lowest),
            "low" => Ok(Self::Low),
            "normal" => Ok(Self::Normal),
            "medium" => Ok(Self::Medium),
            "high" => Ok(Self::High),
            "highest" => Ok(Self::Highest),
            _ => {
                Self::from_emoji(s).ok_or_else(|| TaskError::InvalidPriority {
                    input: s.to_owned(),
                })
            }
        }
    }
}

/// Error type for task domain parsing failures.
#[derive(Debug, Clone, Eq, PartialEq, thiserror::Error)]
pub enum TaskError {
    /// Task status symbol is reserved as a delimiter.
    #[error("task status symbol `{symbol}` is reserved as a delimiter")]
    ProhibitedStatusSymbol {
        /// The reserved delimiter symbol.
        symbol: char,
    },
    /// No task priority name or emoji matched `input`.
    #[error("unrecognized task priority: {input:?}")]
    InvalidPriority {
        /// The unrecognized input.
        input: String,
    },
    /// No task lifecycle date slot matched `input`.
    #[error("unrecognized task date slot: {input:?}")]
    InvalidDateType {
        /// The unrecognized input.
        input: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    mod status_type {
        use pretty_assertions::assert_eq;
        use rstest::rstest;

        use super::*;

        #[rstest]
        #[case::done(TaskStatusType::Done, Some(true))]
        #[case::cancelled(TaskStatusType::Cancelled, None)]
        #[case::todo(TaskStatusType::Todo, Some(false))]
        #[case::in_progress(TaskStatusType::InProgress, Some(false))]
        #[case::on_hold(TaskStatusType::OnHold, Some(false))]
        #[case::non_task(TaskStatusType::NonTask, Some(false))]
        fn derives_tri_state_completion_from_status_type(
            #[case] kind: TaskStatusType,
            #[case] expected: Option<bool>,
        ) {
            assert_eq!(kind.completed(), expected);
        }

        #[rstest]
        #[case::done(TaskStatusType::Done, true)]
        #[case::cancelled(TaskStatusType::Cancelled, true)]
        #[case::todo(TaskStatusType::Todo, false)]
        #[case::in_progress(TaskStatusType::InProgress, false)]
        #[case::on_hold(TaskStatusType::OnHold, false)]
        #[case::non_task(TaskStatusType::NonTask, false)]
        fn rolls_completion_up_to_a_boolean_per_status_type(
            #[case] kind: TaskStatusType,
            #[case] expected: bool,
        ) {
            assert_eq!(kind.is_complete(), expected);
        }
    }

    mod status_map {
        use pretty_assertions::assert_eq;
        use rstest::rstest;

        use super::*;

        #[test]
        fn looks_up_default_statuses_by_symbol() {
            let map = TaskStatusMap::default();

            let todo = map.by_symbol(TaskStatusSymbol::new(' ')).expect("todo");
            assert_eq!(todo.name(), "Todo");
            assert_eq!(todo.kind(), TaskStatusType::Todo);

            let done = map.by_symbol(TaskStatusSymbol::new('x')).expect("done");
            assert_eq!(done.kind(), TaskStatusType::Done);

            assert!(map.by_symbol(TaskStatusSymbol::new('?')).is_none());
        }

        #[test]
        fn resolves_a_known_symbol_to_its_configured_status() {
            let map = TaskStatusMap::default();

            let resolved = map.resolve('x');

            assert_eq!(resolved.name(), "Done");
            assert_eq!(resolved.kind(), TaskStatusType::Done);
            assert_eq!(resolved.symbol(), TaskStatusSymbol::new('x'));
        }

        #[test]
        fn resolves_an_unknown_symbol_to_an_incomplete_todo_preserving_it() {
            let map = TaskStatusMap::default();

            let resolved = map.resolve('?');

            assert_eq!(resolved.kind(), TaskStatusType::Todo);
            assert_eq!(resolved.kind().completed(), Some(false));
            assert_eq!(resolved.symbol(), TaskStatusSymbol::new('?'));
        }

        #[test]
        fn maps_both_lowercase_and_uppercase_done_markers() {
            let map = TaskStatusMap::default();

            let lower = map.by_symbol(TaskStatusSymbol::new('x')).expect("x");
            let upper = map.by_symbol(TaskStatusSymbol::new('X')).expect("X");
            assert_eq!(lower.kind(), TaskStatusType::Done);
            assert_eq!(upper.kind(), TaskStatusType::Done);
        }

        #[test]
        fn looks_up_by_name_normalized_case_and_whitespace() {
            let map = TaskStatusMap::default();

            let exact = map.by_name("In Progress").expect("exact name");
            let messy = map.by_name("  in   PROGRESS  ").expect("messy name");
            assert_eq!(exact.symbol(), TaskStatusSymbol::new('/'));
            assert_eq!(messy.symbol(), TaskStatusSymbol::new('/'));
            assert_eq!(exact.name(), "In Progress", "display name unchanged");
        }

        #[test]
        fn returns_none_for_an_unknown_name() {
            let map = TaskStatusMap::default();

            assert!(map.by_name("nonexistent").is_none());
        }

        #[test]
        fn groups_every_symbol_sharing_a_status_type() {
            let map = TaskStatusMap::default();

            let done_symbols: Vec<TaskStatusSymbol> = map
                .by_type(TaskStatusType::Done)
                .iter()
                .map(TaskStatus::symbol)
                .collect();
            assert_eq!(done_symbols.len(), 2, "both x and X resolve to Done");
            assert!(done_symbols.contains(&TaskStatusSymbol::new('x')));
            assert!(done_symbols.contains(&TaskStatusSymbol::new('X')));

            assert_eq!(map.by_type(TaskStatusType::NonTask), []);
        }

        #[test]
        fn insert_adds_a_new_status_reachable_by_every_lookup() {
            let mut map = TaskStatusMap::default();

            map.insert(TaskStatus::new(
                TaskStatusSymbol::new('?'),
                "Question",
                TaskStatusType::Todo,
            ))
            .expect("valid status symbol");

            assert_eq!(
                map.by_symbol(TaskStatusSymbol::new('?')).map(TaskStatus::name),
                Some("Question")
            );
            assert_eq!(
                map.by_name("question").map(TaskStatus::symbol),
                Some(TaskStatusSymbol::new('?'))
            );
            assert!(
                map.by_type(TaskStatusType::Todo)
                    .iter()
                    .any(|status| status.symbol() == TaskStatusSymbol::new('?'))
            );
        }

        #[test]
        fn insert_overrides_a_default_status_sharing_its_symbol() {
            let mut map = TaskStatusMap::default();

            map.insert(TaskStatus::new(
                TaskStatusSymbol::new('/'),
                "Doing",
                TaskStatusType::InProgress,
            ))
            .expect("valid status symbol");

            assert_eq!(
                map.by_symbol(TaskStatusSymbol::new('/')).map(TaskStatus::name),
                Some("Doing")
            );
            assert!(
                map.by_name("in progress").is_none(),
                "stale default name must not resolve to the overridden symbol"
            );
            assert_eq!(
                map.by_name("doing").map(TaskStatus::symbol),
                Some(TaskStatusSymbol::new('/'))
            );
        }

        #[test]
        fn insert_overrides_a_custom_entry_sharing_its_symbol() {
            let mut map = TaskStatusMap::default();

            map.insert(TaskStatus::new(
                TaskStatusSymbol::new('?'),
                "Question",
                TaskStatusType::Todo,
            ))
            .expect("valid status symbol");
            map.insert(TaskStatus::new(
                TaskStatusSymbol::new('?'),
                "Blocked",
                TaskStatusType::OnHold,
            ))
            .expect("valid status symbol");

            assert_eq!(
                map.by_symbol(TaskStatusSymbol::new('?')).map(TaskStatus::name),
                Some("Blocked")
            );
            assert_eq!(
                map.by_symbol(TaskStatusSymbol::new('?')).map(TaskStatus::kind),
                Some(TaskStatusType::OnHold)
            );
            assert!(
                map.by_name("question").is_none(),
                "stale custom name must not resolve to the overridden symbol"
            );
            assert_eq!(
                map.by_name("blocked").map(TaskStatus::symbol),
                Some(TaskStatusSymbol::new('?'))
            );
            assert!(
                map.by_type(TaskStatusType::Todo)
                    .iter()
                    .all(|s| s.symbol() != TaskStatusSymbol::new('?')),
                "stale kind bucket must not contain the overridden symbol"
            );
            assert!(
                map.by_type(TaskStatusType::OnHold)
                    .iter()
                    .any(|s| s.symbol() == TaskStatusSymbol::new('?'))
            );
        }

        #[rstest]
        #[case('(')]
        #[case(')')]
        #[case('[')]
        #[case(']')]
        #[case('{')]
        #[case('}')]
        fn rejects_prohibited_delimiter_status_symbols_without_mutating_indexes(
            #[case] prohibited: char,
        ) {
            let mut map = TaskStatusMap::default();
            map.insert(TaskStatus::new(
                TaskStatusSymbol::new('!'),
                "Original",
                TaskStatusType::OnHold,
            ))
            .expect("seed status insertion succeeds");

            let invalid = TaskStatus::new(
                TaskStatusSymbol::new(prohibited),
                "Original",
                TaskStatusType::OnHold,
            );

            let result = map.insert(invalid);
            assert_eq!(
                result,
                Err(TaskError::ProhibitedStatusSymbol {
                    symbol: prohibited,
                })
            );

            let original_by_symbol = map
                .by_symbol(TaskStatusSymbol::new('!'))
                .expect("symbol lookup");
            assert_eq!(original_by_symbol.name(), "Original");
            assert_eq!(original_by_symbol.kind(), TaskStatusType::OnHold);

            let original_by_name =
                map.by_name("original").expect("name lookup");
            assert_eq!(original_by_name.symbol(), TaskStatusSymbol::new('!'));

            assert!(map.by_type(TaskStatusType::OnHold).iter().any(
                |status| status.symbol() == TaskStatusSymbol::new('!')
            ));

            assert!(map.by_symbol(TaskStatusSymbol::new(prohibited)).is_none());
        }

        #[test]
        fn default_statuses_do_not_use_prohibited_delimiters() {
            for status in default_statuses() {
                let symbol = status.symbol().as_char();
                assert!(
                    DelimiterType::classify(symbol).is_none(),
                    "default status symbol `{symbol}` must not be a \
                     prohibited delimiter"
                );
            }
        }
    }

    mod normalize_name {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn collapses_multiple_whitespace_to_single_space() {
            assert_eq!(normalize_name("  a   b  "), "a b");
        }

        #[test]
        fn folds_case_to_lowercase() {
            assert_eq!(normalize_name("IN PROGRESS"), "in progress");
        }

        #[test]
        fn trims_leading_and_trailing_whitespace() {
            assert_eq!(normalize_name("  todo  "), "todo");
        }

        #[test]
        fn returns_empty_string_for_empty_input() {
            assert_eq!(normalize_name(""), "");
        }

        #[test]
        fn returns_empty_string_for_whitespace_only_input() {
            assert_eq!(normalize_name("   "), "");
        }
    }

    mod priority {
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
        #[case(
            "invalid",
            Err(TaskError::InvalidPriority {
                input: "invalid".to_owned(),
            })
        )]
        fn parses_names_and_emojis_case_insensitively(
            #[case] input: &str,
            #[case] expected: Result<TaskPriority, TaskError>,
        ) {
            assert_eq!(input.parse::<TaskPriority>(), expected);
        }

        #[test]
        fn accepts_priority_emoji_with_repeated_variation_selectors() {
            assert_eq!(
                TaskPriority::from_emoji("🔺\u{FE0F}\u{FE0F}"),
                Some(TaskPriority::Highest)
            );
        }

        #[test]
        fn orders_priorities_from_lowest_to_highest() {
            assert!(TaskPriority::Lowest < TaskPriority::Low);
            assert!(TaskPriority::Low < TaskPriority::Normal);
            assert!(TaskPriority::Normal < TaskPriority::Medium);
            assert!(TaskPriority::Medium < TaskPriority::High);
            assert!(TaskPriority::High < TaskPriority::Highest);
        }

        #[test]
        fn ranks_are_strictly_increasing_by_severity() {
            let all = [
                TaskPriority::Lowest,
                TaskPriority::Low,
                TaskPriority::Normal,
                TaskPriority::Medium,
                TaskPriority::High,
                TaskPriority::Highest,
            ];
            let ranks: Vec<u8> =
                all.iter().copied().map(TaskPriority::rank).collect();
            assert_eq!(ranks, [0, 1, 2, 3, 4, 5]);
            assert!(ranks.windows(2).all(|w| matches!(w, [a, b] if a < b)));
        }
    }

    mod task_date_set {
        use chrono::NaiveDate;
        use pretty_assertions::assert_eq;

        use super::*;

        const _: () = assert!(
            std::mem::size_of::<TaskDateSet>()
                == std::mem::size_of::<[Option<DateValue>; 6]>()
        );

        #[test]
        fn is_empty_when_default() {
            let set = TaskDateSet::default();
            assert_eq!(set.is_empty(), true);
        }

        #[test]
        fn insert_occupies_slot_and_is_not_empty() {
            let mut set = TaskDateSet::default();
            let d = NaiveDate::from_ymd_opt(2025, 1, 15)
                .map(DateValue::from)
                .unwrap();
            set.insert(TaskDate::new(TaskDateType::Due, d));
            assert_eq!(set.is_empty(), false);
            assert_eq!(set.get(TaskDateType::Due), Some(d));
        }

        #[test]
        fn insert_ignores_duplicate_insertion_into_occupied_slot() {
            let mut set = TaskDateSet::default();
            let d1 = NaiveDate::from_ymd_opt(2025, 1, 15)
                .map(DateValue::from)
                .unwrap();
            let d2 = NaiveDate::from_ymd_opt(2025, 2, 20)
                .map(DateValue::from)
                .unwrap();
            set.insert(TaskDate::new(TaskDateType::Due, d1));
            set.insert(TaskDate::new(TaskDateType::Due, d2));
            assert_eq!(set.get(TaskDateType::Due), Some(d1));
        }

        #[test]
        fn iter_yields_occupied_slots_in_declaration_order() {
            let mut set = TaskDateSet::default();
            let d1 = NaiveDate::from_ymd_opt(2025, 1, 1)
                .map(DateValue::from)
                .unwrap();
            let d2 = NaiveDate::from_ymd_opt(2025, 1, 6)
                .map(DateValue::from)
                .unwrap();
            set.insert(TaskDate::new(TaskDateType::Cancelled, d2));
            set.insert(TaskDate::new(TaskDateType::Created, d1));
            let items: Vec<TaskDate> = set.iter().collect();
            assert_eq!(items, vec![
                TaskDate::new(TaskDateType::Created, d1),
                TaskDate::new(TaskDateType::Cancelled, d2),
            ]);
        }

        #[test]
        fn into_iter_yields_occupied_slots_in_declaration_order() {
            let mut set = TaskDateSet::default();
            let d1 = NaiveDate::from_ymd_opt(2025, 1, 1)
                .map(DateValue::from)
                .unwrap();
            let d2 = NaiveDate::from_ymd_opt(2025, 1, 6)
                .map(DateValue::from)
                .unwrap();
            set.insert(TaskDate::new(TaskDateType::Cancelled, d2));
            set.insert(TaskDate::new(TaskDateType::Created, d1));
            let mut count: usize = 0;
            for date in set {
                match count {
                    0 => assert_eq!(
                        date,
                        TaskDate::new(TaskDateType::Created, d1)
                    ),
                    1 => assert_eq!(
                        date,
                        TaskDate::new(TaskDateType::Cancelled, d2)
                    ),
                    _ => break,
                }
                count = count.saturating_add(1);
            }
            assert_eq!(count, 2);
        }

        #[test]
        fn borrowed_into_iter_yields_occupied_slots() {
            let mut set = TaskDateSet::default();
            let d = NaiveDate::from_ymd_opt(2025, 1, 10)
                .map(DateValue::from)
                .unwrap();
            set.insert(TaskDate::new(TaskDateType::Due, d));
            let collected: Vec<_> = (&set).into_iter().collect();
            assert_eq!(collected, vec![TaskDate::new(TaskDateType::Due, d)]);
        }

        #[test]
        fn iter_size_hint_tracks_remaining_slots() {
            let mut set = TaskDateSet::default();
            let d = NaiveDate::from_ymd_opt(2025, 1, 10)
                .map(DateValue::from)
                .unwrap();
            set.insert(TaskDate::new(TaskDateType::Due, d));
            let mut iter = set.iter();
            assert_eq!(iter.size_hint(), (0, Some(6)));
            let _ = iter.next();
            assert!(iter.size_hint().1.unwrap() <= 5);
        }

        #[test]
        fn from_iterator_and_extend_collect_first_wins() {
            let d1 = NaiveDate::from_ymd_opt(2025, 1, 1)
                .map(DateValue::from)
                .unwrap();
            let d2 = NaiveDate::from_ymd_opt(2025, 1, 2)
                .map(DateValue::from)
                .unwrap();
            let set = TaskDateSet::from_iter([
                TaskDate::new(TaskDateType::Due, d1),
                TaskDate::new(TaskDateType::Due, d2),
            ]);
            assert_eq!(set.get(TaskDateType::Due), Some(d1));

            let mut extended = set;
            let d3 = NaiveDate::from_ymd_opt(2025, 1, 3)
                .map(DateValue::from)
                .unwrap();
            extended.extend([
                TaskDate::new(TaskDateType::Due, d3),
                TaskDate::new(TaskDateType::Start, d3),
            ]);
            assert_eq!(extended.get(TaskDateType::Due), Some(d1));
            assert_eq!(extended.get(TaskDateType::Start), Some(d3));
        }

        #[test]
        fn preserves_slots_across_postcard_roundtrip() {
            let mut set = TaskDateSet::default();
            for (kind, day) in [
                (TaskDateType::Created, 1),
                (TaskDateType::Scheduled, 2),
                (TaskDateType::Start, 3),
                (TaskDateType::Due, 4),
                (TaskDateType::Done, 5),
                (TaskDateType::Cancelled, 6),
            ] {
                let day = NaiveDate::from_ymd_opt(2025, 1, day)
                    .map(DateValue::from)
                    .unwrap();
                set.insert(TaskDate::new(kind, day));
            }
            let bytes = postcard::to_allocvec(&set).expect("encode dates");
            let decoded: TaskDateSet =
                postcard::from_bytes(&bytes).expect("decode dates");

            assert_eq!(decoded, set);
            assert!(!decoded.is_empty());
        }

        #[test]
        fn decodes_pre_c1_wire_fixture() {
            const HEX: &str =
                "010a323032352d30312d3031\
                 010a323032352d30312d3032\
                 010a323032352d30312d3033\
                 010a323032352d30312d3034\
                 010a323032352d30312d3035\
                 010a323032352d30312d3036";
            let bytes: Vec<u8> = (0..HEX.len())
                .step_by(2)
                .map(|i| {
                    u8::from_str_radix(&HEX[i..i + 2], 16).expect("valid hex")
                })
                .collect();
            let decoded: TaskDateSet = postcard::from_bytes(&bytes)
                .expect("decode pre-c1 dates wire bytes");
            assert_eq!(
                decoded.get(TaskDateType::Created),
                NaiveDate::from_ymd_opt(2025, 1, 1).map(DateValue::from)
            );
            assert_eq!(
                decoded.get(TaskDateType::Scheduled),
                NaiveDate::from_ymd_opt(2025, 1, 2).map(DateValue::from)
            );
            assert_eq!(
                decoded.get(TaskDateType::Start),
                NaiveDate::from_ymd_opt(2025, 1, 3).map(DateValue::from)
            );
            assert_eq!(
                decoded.get(TaskDateType::Due),
                NaiveDate::from_ymd_opt(2025, 1, 4).map(DateValue::from)
            );
            assert_eq!(
                decoded.get(TaskDateType::Done),
                NaiveDate::from_ymd_opt(2025, 1, 5).map(DateValue::from)
            );
            assert_eq!(
                decoded.get(TaskDateType::Cancelled),
                NaiveDate::from_ymd_opt(2025, 1, 6).map(DateValue::from)
            );
        }
    }
    mod task_date_type {
        use chrono::NaiveDate;
        use pretty_assertions::assert_eq;
        use rstest::rstest;

        use super::*;

        #[rstest]
        #[case::created("➕", TaskDateType::Created)]
        #[case::scheduled("⏳", TaskDateType::Scheduled)]
        #[case::start("🛫", TaskDateType::Start)]
        #[case::due_calendar("📅", TaskDateType::Due)]
        #[case::due_spiral("🗓", TaskDateType::Due)]
        #[case::done("✅", TaskDateType::Done)]
        #[case::cancelled("❌", TaskDateType::Cancelled)]
        fn from_emoji_resolves_all_base_emojis(
            #[case] emoji: &str,
            #[case] expected: TaskDateType,
        ) {
            assert_eq!(TaskDateType::from_emoji(emoji), Some(expected));
        }

        #[rstest]
        #[case::due_calendar_vs("📅\u{FE0F}", TaskDateType::Due)]
        #[case::due_spiral_vs("🗓\u{FE0F}", TaskDateType::Due)]
        fn from_emoji_strips_variation_selector(
            #[case] emoji: &str,
            #[case] expected: TaskDateType,
        ) {
            assert_eq!(TaskDateType::from_emoji(emoji), Some(expected));
        }

        #[test]
        fn from_emoji_rejects_unknown_strings() {
            assert_eq!(TaskDateType::from_emoji("unknown"), None);
            assert_eq!(TaskDateType::from_emoji(""), None);
        }

        #[rstest]
        #[case::created(TaskDateType::Created, &["created"])]
        #[case::scheduled(TaskDateType::Scheduled, &["scheduled"])]
        #[case::start(TaskDateType::Start, &["start"])]
        #[case::due(TaskDateType::Due, &["due"])]
        #[case::done(TaskDateType::Done, &["done", "completion"])]
        #[case::cancelled(TaskDateType::Cancelled, &["cancelled"])]
        fn field_keys_returns_expected_aliases(
            #[case] kind: TaskDateType,
            #[case] expected: &[&str],
        ) {
            assert_eq!(kind.field_keys(), expected);
        }

        #[rstest]
        #[case::created(TaskDateType::Created, "created")]
        #[case::scheduled(TaskDateType::Scheduled, "scheduled")]
        #[case::start(TaskDateType::Start, "start")]
        #[case::due(TaskDateType::Due, "due")]
        #[case::done(TaskDateType::Done, "done")]
        #[case::cancelled(TaskDateType::Cancelled, "cancelled")]
        fn as_str_and_display_match_canonical_name(
            #[case] kind: TaskDateType,
            #[case] expected: &str,
        ) {
            assert_eq!(kind.as_str(), expected);
            assert_eq!(format!("{kind}"), expected);
        }

        #[rstest]
        #[case("created", Ok(TaskDateType::Created))]
        #[case("SCHEDULED", Ok(TaskDateType::Scheduled))]
        #[case("Start", Ok(TaskDateType::Start))]
        #[case("due", Ok(TaskDateType::Due))]
        #[case("Done", Ok(TaskDateType::Done))]
        #[case("cancelled", Ok(TaskDateType::Cancelled))]
        #[case(
            "unknown",
            Err(TaskError::InvalidDateType {
                input: "unknown".to_owned(),
            })
        )]
        fn from_str_parses_case_insensitively(
            #[case] input: &str,
            #[case] expected: Result<TaskDateType, TaskError>,
        ) {
            assert_eq!(input.parse::<TaskDateType>(), expected);
        }

        #[test]
        fn orders_lifecycle_dates_by_declaration_order() {
            assert!(TaskDateType::Created < TaskDateType::Scheduled);
            assert!(TaskDateType::Scheduled < TaskDateType::Start);
            assert!(TaskDateType::Start < TaskDateType::Due);
            assert!(TaskDateType::Due < TaskDateType::Done);
            assert!(TaskDateType::Done < TaskDateType::Cancelled);
        }

        #[test]
        fn task_date_supports_hash_and_ordering() {
            let d1 = NaiveDate::from_ymd_opt(2025, 1, 1)
                .map(DateValue::from)
                .unwrap();
            let d2 = NaiveDate::from_ymd_opt(2025, 1, 2)
                .map(DateValue::from)
                .unwrap();
            let td1 = TaskDate::new(TaskDateType::Created, d1);
            let td2 = TaskDate::new(TaskDateType::Due, d2);
            let td1_dup = TaskDate::new(TaskDateType::Created, d1);

            let mut set = std::collections::HashSet::new();
            set.insert(td1);
            set.insert(td2);
            set.insert(td1_dup);
            assert_eq!(set.len(), 2);
            assert!(td1 < td2);
        }

        #[test]
        fn is_field_key_recognizes_aliases_and_priority() {
            assert!(TaskDateType::is_field_key("due"));
            assert!(TaskDateType::is_field_key("DUE"));
            assert!(TaskDateType::is_field_key("done"));
            assert!(TaskDateType::is_field_key("completion"));
            assert!(TaskDateType::is_field_key("priority"));
            assert!(TaskDateType::is_field_key("PRIORITY"));
            assert!(!TaskDateType::is_field_key("unknown"));
            assert!(!TaskDateType::is_field_key(""));
        }
    }
}
