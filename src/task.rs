//! Task status domain model: symbols, statuses, and the resolved lookup map
//! shared by note parsing, display, and querying.
//!
//! - [`TaskStatus`]: a named, typed status keyed by its marker symbol.
//! - [`TaskStatusMap`]: a lookup table built once at config resolution, indexed
//!   by symbol, name, and type. [`TaskStatusMap::resolve`] is the custom marker
//!   scanner's entry point: known symbols resolve to their configured status,
//!   unknown symbols fall back to an incomplete todo.
//! - [`TaskStatusType`]: the workflow classification of a status (todo,
//!   in-progress, on-hold, done, cancelled, non-task).
//! - [`TaskStatusSymbol`]: the marker character inside `[<char>]`.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// A named, typed task status keyed by its marker [`TaskStatusSymbol`].
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct TaskStatus {
    symbol: TaskStatusSymbol,
    name: String,
    kind: TaskStatusType,
}

impl TaskStatus {
    /// Creates a task status from its marker symbol, display name, and
    /// workflow type.
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
/// display name, and by workflow type. Default statuses are always present;
/// [`Self::insert`] lets configuration add new statuses or override a default
/// one that shares its symbol.
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
    /// markers are never downgraded to plain bullets: this is the custom
    /// marker scanner's only source of truth for marker-to-status
    /// resolution.
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

    /// Returns every status sharing `kind`, e.g. every symbol that resolves
    /// to [`TaskStatusType::Done`].
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
        // Clean up stale entries from a previous status sharing this symbol
        // before indexing the new one, so all three lookups stay consistent.
        if let Some(previous) = self.symbols.get(&status.symbol) {
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
        // `status` is moved into `names` last; `kinds` gets one clone.
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
    /// `Some(true)` for [`Self::Done`], `None` for [`Self::Cancelled`]
    /// (a terminal state outside the complete/incomplete binary), and
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

            assert!(map.by_type(TaskStatusType::NonTask).is_empty());
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
}
