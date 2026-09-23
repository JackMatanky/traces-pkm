//! Task status, priority, and lifecycle-date domain model shared by note
//! parsing, display, and querying.
//!
//! - [`TaskStatus`]: a named, typed status keyed by its marker symbol.
//! - [`TaskStatusMap`]: a lookup table built once at config resolution, indexed
//!   by symbol, name, and type. [`TaskStatusMap::resolve`] is the custom marker
//!   scanner's entry point: known symbols resolve to their configured status,
//!   unknown symbols fall back to an incomplete todo.
//! - [`TaskStatusType`]: the workflow classification of a status (todo,
//!   in-progress, on-hold, done, cancelled, non-task).
//! - [`TaskStatusSymbol`]: the marker character inside `[<char>]`.
//! - [`TaskPriority`]: six-level task priority enum mapped to emoji and text
//!   representations.
//! - [`TaskDates`]: six distinct task-lifecycle calendar dates (created,
//!   scheduled, start, due, done, cancelled).
//! - [`TaskError`]: error type for task domain parsing failures.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::DateValue;

/// A named, typed task status keyed by its marker [`TaskStatusSymbol`].
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

/// A [`TaskStatus`] lookup table, built once at config resolution.
///
/// Provides lookup by marker symbol, display name, and workflow type. Default
/// statuses are always present; configuration can add new statuses or override
/// default ones that share a symbol.
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
    #[inline]
    pub(crate) fn insert(&mut self, status: TaskStatus) {
        self.purge_stale_entries(&status);
        self.index_status(status);
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

    /// Returns every status sorted deterministically by symbol character.
    #[inline]
    #[must_use]
    pub(crate) fn statuses_sorted_by_symbol(&self) -> Vec<TaskStatus> {
        let mut statuses: Vec<TaskStatus> =
            self.symbols.values().cloned().collect();
        statuses.sort_unstable_by_key(|s| s.symbol.as_char());
        statuses
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
            map.insert(status);
        }
        map
    }
}

/// The workflow classification of a [`TaskStatus`].
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
    /// `Some(true)` for [`Self::Done`], `None` for [`Self::Cancelled`] (a
    /// terminal state outside the complete/incomplete binary), and
    /// `Some(false)` for every other status type.
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
}

/// The marker character inside `[<char>]`, e.g. `' '`, `'x'`, `'/'`, `'-'`.
///
/// Wraps a single `char` without validation, serving as the lookup key for
/// standard and custom-scanned task markers. Unknown single-character markers
/// are still valid symbols; this type carries no validation beyond being a
/// `char`.
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
    name.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
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

/// Date metadata associated with a [`TaskListItem`](crate::TaskListItem).
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
/// let due = NaiveDate::from_ymd_opt(2025, 1, 15).map(DateValue::from);
/// let dates = TaskDates::new(None, None, None, due, None, None);
/// assert!(!dates.is_empty());
/// assert_eq!(dates.due(), due);
/// ```
#[derive(
    Copy, Clone, Debug, Default, Eq, Hash, PartialEq, Deserialize, Serialize,
)]
pub struct TaskDates {
    /// Date when the task was created (`➕` or `[created::]`).
    created: Option<DateValue>,
    /// Date when the task is scheduled (`⏳` or `[scheduled::]`).
    scheduled: Option<DateValue>,
    /// Date when work on the task begins (`🛫` or `[start::]`).
    start: Option<DateValue>,
    /// Date when the task is due (`📅` or `[due::]`).
    due: Option<DateValue>,
    /// Date when the task was completed (`✅` or `[done::]`).
    done: Option<DateValue>,
    /// Date when the task was cancelled (`❌` or `[cancelled::]`).
    cancelled: Option<DateValue>,
}

impl TaskDates {
    /// Creates a new [`Self`] instance with all dates specified.
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
    /// let created = NaiveDate::from_ymd_opt(2025, 1, 1).map(DateValue::from);
    /// let dates = TaskDates::new(created, None, None, None, None, None);
    /// assert_eq!(dates.created(), created);
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
    /// let scheduled = NaiveDate::from_ymd_opt(2025, 1, 10).map(DateValue::from);
    /// let dates = TaskDates::new(None, scheduled, None, None, None, None);
    /// assert_eq!(dates.scheduled(), scheduled);
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
    /// let start = NaiveDate::from_ymd_opt(2025, 1, 12).map(DateValue::from);
    /// let dates = TaskDates::new(None, None, start, None, None, None);
    /// assert_eq!(dates.start(), start);
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
    /// let due = NaiveDate::from_ymd_opt(2025, 1, 15).map(DateValue::from);
    /// let dates = TaskDates::new(None, None, None, due, None, None);
    /// assert_eq!(dates.due(), due);
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
    /// let done = NaiveDate::from_ymd_opt(2025, 1, 20).map(DateValue::from);
    /// let dates = TaskDates::new(None, None, None, None, done, None);
    /// assert_eq!(dates.done(), done);
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
    /// let cancelled = NaiveDate::from_ymd_opt(2025, 1, 22).map(DateValue::from);
    /// let dates = TaskDates::new(None, None, None, None, None, cancelled);
    /// assert_eq!(dates.cancelled(), cancelled);
    /// ```
    #[inline]
    #[must_use]
    pub const fn cancelled(&self) -> Option<DateValue> {
        self.cancelled
    }
}

/// Error type for task domain parsing failures.
#[derive(Debug, Clone, Eq, PartialEq, thiserror::Error)]
pub enum TaskError {
    /// No task priority name or emoji matched `input`.
    #[error("unrecognized task priority: {input:?}")]
    InvalidPriority {
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
    }

    mod status_map {
        use pretty_assertions::assert_eq;

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
        fn returns_statuses_in_ascending_symbol_order() {
            let symbols: Vec<char> = TaskStatusMap::default()
                .statuses_sorted_by_symbol()
                .iter()
                .map(|status| status.symbol().as_char())
                .collect();

            assert_eq!(symbols, [' ', '!', '-', '/', 'X', 'x']);
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
            ));

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
            ));

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
            ));
            map.insert(TaskStatus::new(
                TaskStatusSymbol::new('?'),
                "Blocked",
                TaskStatusType::OnHold,
            ));

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

    mod task_dates {
        use chrono::NaiveDate;
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn is_empty_when_all_dates_are_absent() {
            let dates = TaskDates::default();

            assert_eq!(dates.is_empty(), true);
        }

        #[test]
        fn is_not_empty_when_a_lifecycle_date_is_present() {
            let dates = TaskDates::new(
                None,
                None,
                None,
                NaiveDate::from_ymd_opt(2025, 1, 15).map(Into::into),
                None,
                None,
            );

            assert_eq!(dates.is_empty(), false);
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

        #[test]
        fn preserves_all_none_dates_across_postcard_roundtrip() {
            let dates = TaskDates::default();
            let bytes = postcard::to_allocvec(&dates).expect("encode dates");
            let decoded: TaskDates =
                postcard::from_bytes(&bytes).expect("decode dates");

            assert_eq!(decoded, dates);
        }

        #[test]
        fn preserves_partially_populated_dates_across_postcard_roundtrip() {
            let due = NaiveDate::from_ymd_opt(2025, 1, 15).map(DateValue::from);
            let dates = TaskDates::new(None, None, None, due, None, None);
            let bytes = postcard::to_allocvec(&dates).expect("encode dates");
            let decoded: TaskDates =
                postcard::from_bytes(&bytes).expect("decode dates");

            assert_eq!(decoded, dates);
            assert_eq!(decoded.due(), due);
        }
    }
}
