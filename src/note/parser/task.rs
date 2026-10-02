//! Task text processing, date and priority extraction, and text normalization.
//!
//! [`TaskScan`] tokenizes list item text in a single pass to extract task
//! lifecycle dates, priority levels, and cleaned display text without repeated
//! parsing passes.

use indexmap::IndexMap;

use super::lexer::{FieldForm, ItemToken, TaskFieldEmojis, tokenize_item_text};
use crate::{
    DateValue, FieldKey, FieldKeyRef, Spanned, Tag, TaskDate, TaskDateSet,
    TaskDateType, TaskPriority, TaskStatusMap,
    note::{ListItemType, NoteFieldValue, TaskListItem},
};

/// Single-pass tokenization view of list item text.
///
/// Encapsulates raw display text paired with its parsed token stream, ensuring
/// date, priority, and clean text extraction remain synchronized with the
/// underlying text.
pub(super) struct TaskScan<'a> {
    raw: &'a str,
    tokens: Vec<Spanned<ItemToken>>,
}

impl<'a> TaskScan<'a> {
    /// Tokenizes `raw` in a single pass with task shorthands enabled.
    pub(super) fn scan(
        raw: &'a str,
        code_spans: &[std::ops::Range<usize>],
    ) -> Self {
        let mut tokens = tokenize_item_text(raw, TaskFieldEmojis::Include);
        if !code_spans.is_empty() {
            tokens.retain(|token| {
                let range = token.span_usize();
                !code_spans.iter().any(|code| {
                    range.start < code.end && code.start < range.end
                })
            });
        }
        Self {
            raw,
            tokens,
        }
    }

    /// Extracts task lifecycle dates from emoji shorthands and inline task
    /// fields.
    ///
    /// Precedence and resolution rules:
    /// - Emoji date shorthands take precedence over inline key-value fields.
    /// - When multiple dates appear for the same lifecycle kind, the first
    ///   occurrence wins.
    /// - Unmatched lifecycle dates fall back to searching `fields` by canonical
    ///   key names.
    pub(super) fn dates(
        &self,
        fields: &IndexMap<FieldKey, Vec<NoteFieldValue>>,
    ) -> TaskDateSet {
        let mut set = TaskDateSet::default();

        // 1. Emoji dates (first-wins per slot)
        for token in &self.tokens {
            if let ItemToken::Date(task_date) = token.value()
                && set.get(task_date.kind()).is_none()
            {
                set.insert(*task_date);
            }
        }

        // 2. Inline fields fallback
        for kind in TaskDateType::ALL {
            if set.get(kind).is_some() {
                continue;
            }
            for key_name in kind.field_keys() {
                if let Some(date) = Self::first_date_in_field(fields, key_name)
                {
                    set.insert(TaskDate::new(kind, date));
                    break;
                }
            }
        }

        set
    }

    /// Extracts task priority from text emojis or an inline `[priority::
    /// <level>]` field.
    ///
    /// Precedence and resolution rules:
    /// - Priority emojis take precedence over inline fields.
    /// - When multiple priority emojis are present, the first in document order
    ///   wins.
    /// - Returns [`None`] if no priority is specified or if priority resolves
    ///   to [`TaskPriority::Normal`].
    pub(super) fn priority(
        &self,
        fields: &IndexMap<FieldKey, Vec<NoteFieldValue>>,
    ) -> Option<TaskPriority> {
        for token in &self.tokens {
            if let ItemToken::Priority(priority) = token.value() {
                return Some(*priority);
            }
        }

        let value =
            fields.get(&FieldKeyRef::new("priority"))?.iter().find_map(
                |val| val.as_str().and_then(|s| s.parse::<TaskPriority>().ok()),
            )?;
        if matches!(value, TaskPriority::Normal) {
            return None;
        }
        Some(value)
    }

    /// Computes normalized list text by stripping task metadata and collapsing
    /// whitespace.
    ///
    /// Strips:
    /// - Priority emojis
    /// - Task date shorthands and wrapped date fields
    /// - Tags matching configured `tag_filters`
    ///
    /// Expects `raw` to have any leading task marker prefix already removed.
    pub(super) fn clean_text(&self, tag_filters: &[Tag]) -> String {
        // Token spans from a single logos pass are disjoint and ordered, so
        // unremoved text slices can be accumulated directly without an
        // intermediate allocation of removal spans.
        let mut cleaned = String::with_capacity(self.raw.len());
        let mut current_idx = 0;
        for token in &self.tokens {
            let should_remove = match token.value() {
                ItemToken::Priority(_) | ItemToken::Date(_) => true,
                ItemToken::Tag(tag) => tag_filters.contains(tag),
                ItemToken::Field((key, _, form)) => {
                    matches!(form, FieldForm::Wrapped)
                        && TaskDateType::is_field_key(key.canonical())
                }
            };

            if should_remove {
                let start = token.start_usize();
                let end = token.end_usize();
                if start > current_idx
                    && let Some(slice) = self.raw.get(current_idx..start)
                {
                    cleaned.push_str(slice);
                }
                current_idx = current_idx.max(end);
            }
        }

        if current_idx < self.raw.len()
            && let Some(slice) = self.raw.get(current_idx..)
        {
            cleaned.push_str(slice);
        }

        Self::normalize_whitespace(&cleaned)
    }

    /// Returns the first date value matching `key_name` across `fields`.
    ///
    /// [`FieldKeyRef`] resolves the canonical entry in O(1); keys in `fields`
    /// are unique under canonical equality, so at most one entry matches.
    fn first_date_in_field(
        fields: &IndexMap<FieldKey, Vec<NoteFieldValue>>,
        key_name: &str,
    ) -> Option<DateValue> {
        fields
            .get(&FieldKeyRef::new(key_name))?
            .iter()
            .find_map(|val| val.as_date().map(Into::into))
    }

    /// Collapses in-line whitespace runs to single spaces, drops blank lines,
    /// and separates the remaining lines with single newlines.
    fn normalize_whitespace(text: &str) -> String {
        let mut result = String::with_capacity(text.len());
        let mut first_line = true;
        for line in text.split('\n') {
            let mut words = line.split_whitespace();
            let Some(first_word) = words.next() else {
                continue;
            };
            if !first_line {
                result.push('\n');
            }
            result.push_str(first_word);
            for word in words {
                result.push(' ');
                result.push_str(word);
            }
            first_line = false;
        }
        result
    }
}
/// Tracks whether any descendant task within a list item's subtree is
/// incomplete.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub(super) enum SubTaskCompletion {
    AllComplete,
    /// Conservative fallback: unknown or default completion state is
    /// fail-closed.
    #[default]
    HasIncomplete,
}

impl SubTaskCompletion {
    /// Returns the initial completion state for a newly opened item before any
    /// child tasks exist.
    ///
    /// A newly opened item contains zero child tasks, so its descendant tree
    /// contains no incomplete tasks until a child task is observed.
    #[inline]
    #[must_use]
    pub(super) const fn initial() -> Self {
        Self::AllComplete
    }

    /// Updates completion state after observing a child item.
    ///
    /// Transitions to [`Self::HasIncomplete`] when:
    /// - The child item is an incomplete task
    /// - The child item's subtask completion is [`Self::HasIncomplete`]
    #[inline]
    pub(super) fn observe_child(
        &mut self,
        is_task: bool,
        is_complete: bool,
        child: Self,
    ) {
        if (is_task && !is_complete) || child == Self::HasIncomplete {
            *self = Self::HasIncomplete;
        }
    }

    /// Returns `true` if all descendant tasks in the subtree are complete.
    #[must_use]
    pub(super) const fn is_fully_complete(self) -> bool {
        matches!(self, Self::AllComplete)
    }
}

/// Classification inputs for one list item.
#[derive(Copy, Clone)]
pub(super) struct TaskClassificationParams<'a> {
    /// Item-leading marker symbol, or `None` for plain bullets.
    pub(super) marker: Option<char>,
    /// Inline fields scanned from the item's own text.
    pub(super) fields: &'a IndexMap<FieldKey, Vec<NoteFieldValue>>,
    /// Tags scanned from the item's own text.
    pub(super) tags: &'a [Tag],
    /// Configured filters deciding Task vs Checkbox classification.
    pub(super) tag_filters: &'a [Tag],
    /// Resolves marker symbols to task statuses.
    pub(super) statuses: &'a TaskStatusMap,
    /// Whether every descendant task in the item's subtree is complete.
    pub(super) fully_complete: bool,
}

/// Classifies a list item by its marker symbol and associated metadata.
///
/// Status-marked items resolve against configured tag filters to determine
/// whether they form a [`ListItemType::Task`] or [`ListItemType::Checkbox`].
/// Returns the resolved item type alongside cleaned display text.
pub(super) fn classify_item(
    scan: &TaskScan<'_>,
    params: TaskClassificationParams<'_>,
) -> (ListItemType, String) {
    let clean = scan.clean_text(params.tag_filters);
    let item_type = match params.marker {
        Some(symbol) => {
            let status = params.statuses.resolve(symbol);
            if params.tag_filters.is_empty()
                || params
                    .tags
                    .iter()
                    .any(|tag| params.tag_filters.contains(tag))
            {
                let priority = scan.priority(params.fields);
                let dates = scan.dates(params.fields);
                ListItemType::Task(TaskListItem::new(
                    dates,
                    priority,
                    status,
                    params.fully_complete,
                ))
            } else {
                ListItemType::Checkbox
            }
        }
        None => ListItemType::Plain,
    };
    (item_type, clean)
}

#[cfg(test)]
mod tests {
    use chrono::NaiveDate;
    use rstest::rstest;

    use super::{super::list::ListTracker, *};
    use crate::{
        BytePos, DateValue, Note, SourceLine, SpanStart, TaskStatusType,
        note::{ListItem, MarkdownParserInput, parse_markdown},
        parse_note_str as parse,
    };

    mod dates {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn gives_precedence_to_emoji_over_inline_field() {
            let text = "Task 📅 2025-01-01";
            let mut fields = IndexMap::new();
            fields.insert(FieldKey::try_from("due").unwrap(), vec![
                NoteFieldValue::Date(
                    DateValue::parse_iso("2025-02-02").unwrap(),
                ),
            ]);
            let set = TaskScan::scan(text, &[]).dates(&fields);
            assert_eq!(
                set.get(TaskDateType::Due),
                Some(DateValue::parse_iso("2025-01-01").unwrap())
            );
        }

        #[test]
        fn prefers_done_over_completion_alias() {
            let text = "Task";
            let mut fields = IndexMap::new();
            fields.insert(FieldKey::try_from("done").unwrap(), vec![
                NoteFieldValue::Date(
                    DateValue::parse_iso("2025-01-01").unwrap(),
                ),
            ]);
            fields.insert(FieldKey::try_from("completion").unwrap(), vec![
                NoteFieldValue::Date(
                    DateValue::parse_iso("2025-02-02").unwrap(),
                ),
            ]);
            let set = TaskScan::scan(text, &[]).dates(&fields);
            assert_eq!(
                set.get(TaskDateType::Done),
                Some(DateValue::parse_iso("2025-01-01").unwrap())
            );
        }

        #[test]
        fn keeps_the_first_emoji_when_a_slot_repeats() {
            let text = "Task 📅 2025-01-01 📅 2025-02-02";
            let fields = IndexMap::new();

            let set = TaskScan::scan(text, &[]).dates(&fields);

            assert_eq!(
                set.get(TaskDateType::Due),
                Some(DateValue::parse_iso("2025-01-01").unwrap())
            );
        }

        #[test]
        fn fills_emoji_free_slots_from_inline_fields() {
            let text = "Task 📅 2025-01-01";
            let mut fields = IndexMap::new();
            fields.insert(FieldKey::try_from("start").unwrap(), vec![
                NoteFieldValue::Date(
                    DateValue::parse_iso("2025-03-03").unwrap(),
                ),
            ]);

            let set = TaskScan::scan(text, &[]).dates(&fields);

            assert_eq!(
                set.get(TaskDateType::Due),
                Some(DateValue::parse_iso("2025-01-01").unwrap())
            );
            assert_eq!(
                set.get(TaskDateType::Start),
                Some(DateValue::parse_iso("2025-03-03").unwrap())
            );
        }
    }

    mod priority {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn selects_first_in_document_order() {
            let text = "Task 🔺 and 🔽";
            let fields = IndexMap::new();
            let priority = TaskScan::scan(text, &[]).priority(&fields);
            assert_eq!(priority, Some(TaskPriority::Highest));
        }

        #[test]
        fn gives_precedence_to_emoji_over_inline_field() {
            let text = "Task 🔺";
            let mut fields = IndexMap::new();
            fields.insert(FieldKey::try_from("priority").unwrap(), vec![
                NoteFieldValue::String("low".to_owned()),
            ]);

            let priority = TaskScan::scan(text, &[]).priority(&fields);

            assert_eq!(priority, Some(TaskPriority::Highest));
        }

        #[test]
        fn collapses_normal_field_to_none() {
            let text = "Task";
            let mut fields = IndexMap::new();
            fields.insert(FieldKey::try_from("priority").unwrap(), vec![
                NoteFieldValue::String("normal".to_owned()),
            ]);
            let priority = TaskScan::scan(text, &[]).priority(&fields);
            assert_eq!(priority, None);
        }
    }

    mod clean_text {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn strips_dates_priorities_tags_and_inline_fields() {
            let filters = [Tag::parse("#task").unwrap()];
            let raw = "Task #task 🔺 📅 2025-01-01 [custom:: value] \
                       [created:: 2024-12-01]";
            let cleaned = TaskScan::scan(raw, &[]).clean_text(&filters);
            assert_eq!(cleaned, "Task [custom:: value]");
        }

        #[test]
        fn preserves_tags_outside_the_filter_set() {
            let filters = [Tag::parse("#task").unwrap()];
            let raw = "Task #other 🔺";
            let cleaned = TaskScan::scan(raw, &[]).clean_text(&filters);
            assert_eq!(cleaned, "Task #other");
        }

        #[test]
        fn preserves_unparsed_invalid_dates() {
            let raw = "Task 📅 2026-13-45 keep";
            let cleaned = TaskScan::scan(raw, &[]).clean_text(&[]);
            assert_eq!(cleaned, "Task 📅 2026-13-45 keep");
        }

        #[test]
        fn strips_priority_emoji_with_variation_selector() {
            let raw = "Task 🔺\u{FE0F} remaining text";
            let cleaned = TaskScan::scan(raw, &[]).clean_text(&[]);
            assert_eq!(cleaned, "Task remaining text");
        }
    }

    mod scan {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn ignores_emoji_date_inside_code_span() {
            let raw = "task with 🗓2022-07-14";
            let start = raw.find('🗓').expect("emoji present");
            let code_span = start..start + "🗓2022-07-14".len();

            let scan = TaskScan::scan(raw, &[code_span]);
            let fields = IndexMap::new();

            assert_eq!(scan.dates(&fields).get(TaskDateType::Due), None);
            assert_eq!(scan.clean_text(&[]), "task with 🗓2022-07-14");
        }
    }

    mod is_task_field_key {
        use super::*;

        #[test]
        fn recognizes_task_field_keys_case_insensitively() {
            assert!(TaskDateType::is_field_key("Due"));
            assert!(TaskDateType::is_field_key("COMPLETION"));
            assert!(TaskDateType::is_field_key("priority"));
        }

        #[test]
        fn rejects_keys_outside_the_task_field_allowlist() {
            assert!(!TaskDateType::is_field_key("store"));
            assert!(!TaskDateType::is_field_key(""));
        }
    }
    mod classification {
        use pretty_assertions::assert_eq;

        use super::*;
        #[test]
        fn starts_fully_complete_until_an_incomplete_child_is_observed() {
            let mut completion = SubTaskCompletion::initial();
            assert!(completion.is_fully_complete());
            completion.observe_child(
                true,
                false,
                SubTaskCompletion::AllComplete,
            );
            assert!(!completion.is_fully_complete());
        }

        #[rstest]
        #[case::space_todo(' ', TaskStatusType::Todo)]
        #[case::checked_lowercase('x', TaskStatusType::Done)]
        #[case::checked_uppercase('X', TaskStatusType::Done)]
        #[case::in_progress('/', TaskStatusType::InProgress)]
        #[case::cancelled('-', TaskStatusType::Cancelled)]
        #[case::on_hold('!', TaskStatusType::OnHold)]
        fn classifies_every_default_marker_as_a_task(
            #[case] symbol: char,
            #[case] expected_kind: TaskStatusType,
        ) {
            let input = format!("- [{symbol}] Task text");
            let note = parse(&input);

            let tasks: Vec<&ListItem> = note.tasks().collect();
            assert_eq!(tasks.len(), 1, "marker {symbol:?} must become a Task");
            assert_eq!(
                tasks.first().copied().map(ListItem::raw_text),
                Some("Task text")
            );

            let item = tasks.first().expect("task present");
            assert!(
                matches!(item.kind(), ListItemType::Task(task) if
                    task.status().kind() == expected_kind),
                "marker {symbol:?} must resolve to {expected_kind:?}, got {:?}",
                item.kind()
            );
        }

        #[test]
        #[expect(clippy::panic, reason = "test assertion on enum variant")]
        fn preserves_and_classifies_an_unknown_marker_as_an_incomplete_task() {
            let note = parse("- [?] Mystery task");

            let item = note.lists().first().expect("item present");
            assert_eq!(item.text(), "Mystery task");
            let ListItemType::Task(task) = item.kind() else {
                panic!(
                    "unknown marker must never be downgraded to a plain \
                     bullet, got {:?}",
                    item.kind()
                );
            };
            assert_eq!(
                task.status().kind().completed(),
                Some(false),
                "unknown markers resolve as incomplete todos"
            );
        }

        #[test]
        fn does_not_treat_bracket_text_in_the_item_body_as_a_marker() {
            let note = parse("- Check [x] later");

            let item = note.lists().first().expect("item present");
            assert_eq!(item.text(), "Check [x] later");
            assert_eq!(item.kind(), &ListItemType::Plain);
            assert_eq!(note.tasks().count(), 0);
        }

        #[test]
        fn classifies_a_bare_marker_with_no_trailing_text_as_a_task() {
            // `- [x]` as an entire item: the line terminator supplies the
            // marker's trailing whitespace, matching pulldown-cmark's
            // ENABLE_TASKLISTS behavior.
            let note = parse("- [x]");

            let item = note.lists().first().expect("item present");
            assert_eq!(item.text(), "");
            assert_eq!(note.tasks().count(), 1);
        }

        #[test]
        fn resolves_a_pending_marker_before_a_nested_list_flush() {
            // `- [x]` + nested list: no whitespace text chunk arrives before
            // the child list starts, but the parent still carries a marker.
            let note = parse("- [x]\n  - sub");

            let item = note.lists().first().expect("item present");
            assert_eq!(item.text(), "");
            assert_eq!(note.tasks().count(), 1);
        }

        #[test]
        fn classifies_a_marker_before_a_soft_break_as_a_task() {
            let note = parse("- [x]\n  continued");

            let tasks: Vec<&ListItem> = note.tasks().collect();
            assert_eq!(tasks.len(), 1);
            assert_eq!(
                tasks.first().copied().map(ListItem::raw_text),
                Some("\ncontinued")
            );
        }

        #[test]
        fn keeps_an_item_starting_with_inline_markup_plain() {
            // The emphasis opens the item's content, so `[x]` is not at the
            // item-leading position and must not become a marker.
            let note = parse("- **[x] Task**");

            let item = note.lists().first().expect("item present");
            assert_eq!(item.text(), "[x] Task");
            assert_eq!(item.kind(), &ListItemType::Plain);
            assert_eq!(note.tasks().count(), 0);
        }

        #[test]
        fn keeps_an_item_starting_with_inline_code_plain() {
            let note = parse("- `[x]` Task");

            let item = note.lists().first().expect("item present");
            assert_eq!(item.text(), "[x] Task");
            assert_eq!(item.kind(), &ListItemType::Plain);
            assert_eq!(note.tasks().count(), 0);
        }

        #[test]
        fn keeps_a_link_lookalike_plain() {
            // `- [x](y)` is a link whose text abuts the closing bracket;
            // no whitespace after `]`, so no marker.
            let note = parse("- [x](y) z");

            let item = note.lists().first().expect("item present");
            assert_eq!(item.text(), "x z");
            assert_eq!(item.kind(), &ListItemType::Plain);
            assert_eq!(note.tasks().count(), 0);
        }

        #[test]
        fn rejects_unicode_whitespace_after_the_marker() {
            // NBSP is ordinary text in Markdown, not the marker's trailing
            // whitespace (ASCII whitespace only, mirroring pulldown-cmark).
            let note = parse("- [x]\u{00A0}Task");

            let item = note.lists().first().expect("item present");
            assert_eq!(item.text(), "[x]\u{00A0}Task");
            assert_eq!(item.kind(), &ListItemType::Plain);
            assert_eq!(note.tasks().count(), 0);
        }

        #[test]
        fn classifies_a_multibyte_symbol_marker_as_an_incomplete_task() {
            let note = parse("- [β] Task");

            let tasks: Vec<&ListItem> = note.tasks().collect();
            assert_eq!(tasks.len(), 1);
            assert_eq!(
                tasks.first().copied().map(ListItem::raw_text),
                Some("Task")
            );
            let item = tasks.first().expect("task present");
            assert!(matches!(
                item.kind(),
                ListItemType::Task(task) if task.status().kind().completed() == Some(false)
            ));
        }
        #[rstest]
        #[case::inline_strong_formatting("- [x] buy **milk**", 'x')]
        #[case::inline_link("- [ ] check [link](https://example.com)", ' ')]
        #[case::inline_code("- [x] see `code`", 'x')]
        fn classifies_task_despite_inline_content_in_the_body(
            #[case] input: &str,
            #[case] symbol: char,
        ) {
            let note = parse(input);
            let item = note.lists().first().expect("item present");
            assert_eq!(
                item.kind(),
                &ListItemType::Task(TaskListItem::new(
                    TaskDateSet::default(),
                    None,
                    TaskStatusMap::default().resolve(symbol),
                    true,
                ))
            );
        }

        #[test]
        fn classifies_task_with_loose_block_child() {
            let note = parse("- [x]\n\n  loose para");
            let item = note.lists().first().expect("item present");
            assert_eq!(item.raw_text(), "loose para");
            assert!(matches!(item.kind(), ListItemType::Task(_)));
        }

        #[test]
        fn classifies_item_with_leading_bold_marker_as_plain() {
            let note = parse("- **[x] bold**");
            let item = note.lists().first().expect("item present");
            assert_eq!(item.kind(), &ListItemType::Plain);
        }
    }
    mod tag_filters {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn classifies_marked_item_as_task_when_tag_filters_are_empty() {
            let mut tracker = ListTracker::default();
            tracker.start_list(false);
            tracker.start_item(
                SourceLine::new(1).expect("non-zero"),
                SpanStart::default(),
            );
            tracker.push_text("[x] Task without tag", false);
            tracker.end_item(&[], &TaskStatusMap::default(), BytePos::new(0));

            let item = tracker.lists.first().expect("item present");
            assert!(matches!(item.kind(), ListItemType::Task(_)));
        }

        #[test]
        fn classifies_marked_item_as_task_when_tag_matches_filter() {
            let mut tracker = ListTracker::default();
            tracker.start_list(false);
            tracker.start_item(
                SourceLine::new(1).expect("non-zero"),
                SpanStart::default(),
            );
            tracker.push_text("[x] Task with tag #task", false);
            let tag_filters = [Tag::parse("#task").unwrap()];
            tracker.end_item(
                &tag_filters,
                &TaskStatusMap::default(),
                BytePos::new(0),
            );
            tracker.end_list();

            let item = tracker.lists.first().expect("item present");
            assert!(matches!(item.kind(), ListItemType::Task(_)));
        }

        #[test]
        fn item_tags_survive_classification_as_queryable_data() {
            let mut tracker = ListTracker::default();
            tracker.start_list(false);
            tracker.start_item(
                SourceLine::new(1).expect("non-zero"),
                SpanStart::default(),
            );
            tracker.push_text("[x] Task #task #project", false);
            let tag_filters = [Tag::parse("#task").unwrap()];
            tracker.end_item(
                &tag_filters,
                &TaskStatusMap::default(),
                BytePos::new(0),
            );
            tracker.end_list();

            let item = tracker.lists.first().expect("item present");
            // Classification used only #task to decide Task vs Checkbox, but
            // both tags the item actually carries remain queryable.
            assert_eq!(item.tags(), [
                Tag::parse("#task").unwrap(),
                Tag::parse("#project").unwrap(),
            ]);
        }

        #[test]
        fn classifies_marked_item_as_checkbox_when_tag_does_not_match_filter() {
            let mut tracker = ListTracker::default();
            tracker.start_list(false);
            tracker.start_item(
                SourceLine::new(1).expect("non-zero"),
                SpanStart::default(),
            );
            tracker.push_text("[x] Checkbox with different tag #other", false);
            let tag_filters = [Tag::parse("#task").unwrap()];
            tracker.end_item(
                &tag_filters,
                &TaskStatusMap::default(),
                BytePos::new(0),
            );
            tracker.end_list();

            let item = tracker.lists.first().expect("item present");
            assert_eq!(item.kind(), &ListItemType::Checkbox);
        }

        #[test]
        fn classifies_marked_item_as_checkbox_when_item_has_no_tags_and_filter_is_non_empty()
         {
            let mut tracker = ListTracker::default();
            tracker.start_list(false);
            tracker.start_item(
                SourceLine::new(1).expect("non-zero"),
                SpanStart::default(),
            );
            tracker.push_text("[x] Checkbox without tags", false);
            let tag_filters = [Tag::parse("#task").unwrap()];
            tracker.end_item(
                &tag_filters,
                &TaskStatusMap::default(),
                BytePos::new(0),
            );
            tracker.end_list();

            let item = tracker.lists.first().expect("item present");
            assert_eq!(item.kind(), &ListItemType::Checkbox);
        }

        #[test]
        fn classifies_marked_item_as_task_when_one_of_multiple_tags_matches_filter()
         {
            let mut tracker = ListTracker::default();
            tracker.start_list(false);
            tracker.start_item(
                SourceLine::new(1).expect("non-zero"),
                SpanStart::default(),
            );
            tracker.push_text(
                "[x] Task with multiple tags #other #task #work",
                false,
            );
            let tag_filters = [Tag::parse("#task").unwrap()];
            tracker.end_item(
                &tag_filters,
                &TaskStatusMap::default(),
                BytePos::new(0),
            );
            tracker.end_list();

            let item = tracker.lists.first().expect("item present");
            assert!(matches!(item.kind(), ListItemType::Task(_)));
        }

        #[test]
        fn classifies_marked_item_as_task_when_tag_matches_one_of_multiple_filters()
         {
            let mut tracker = ListTracker::default();
            tracker.start_list(false);
            tracker.start_item(
                SourceLine::new(1).expect("non-zero"),
                SpanStart::default(),
            );
            tracker.push_text("[x] Task matching second filter #todo", false);
            let tag_filters =
                [Tag::parse("#task").unwrap(), Tag::parse("#todo").unwrap()];
            tracker.end_item(
                &tag_filters,
                &TaskStatusMap::default(),
                BytePos::new(0),
            );
            tracker.end_list();

            let item = tracker.lists.first().expect("item present");
            assert!(matches!(item.kind(), ListItemType::Task(_)));
        }

        #[test]
        fn rejects_prefix_match_for_exact_nested_tag() {
            let mut tracker = ListTracker::default();
            tracker.start_list(false);
            tracker.start_item(
                SourceLine::new(1).expect("non-zero"),
                SpanStart::default(),
            );
            tracker
                .push_text("[x] Checkbox with nested tag #task/project", false);
            let tag_filters = [Tag::parse("#task").unwrap()];
            tracker.end_item(
                &tag_filters,
                &TaskStatusMap::default(),
                BytePos::new(0),
            );
            tracker.end_list();

            let item = tracker.lists.first().expect("item present");
            assert_eq!(item.kind(), &ListItemType::Checkbox);
        }

        #[test]
        fn accepts_exact_nested_tag_match() {
            let mut tracker = ListTracker::default();
            tracker.start_list(false);
            tracker.start_item(
                SourceLine::new(1).expect("non-zero"),
                SpanStart::default(),
            );
            tracker.push_text("[x] Task with nested tag #task/project", false);
            let tag_filters = [Tag::parse("#task/project").unwrap()];
            tracker.end_item(
                &tag_filters,
                &TaskStatusMap::default(),
                BytePos::new(0),
            );
            tracker.end_list();

            let item = tracker.lists.first().expect("item present");
            assert!(matches!(item.kind(), ListItemType::Task(_)));
        }

        #[test]
        fn keeps_unmarked_item_plain_even_with_matching_tag() {
            let mut tracker = ListTracker::default();
            tracker.start_list(false);
            tracker.start_item(
                SourceLine::new(1).expect("non-zero"),
                SpanStart::default(),
            );
            tracker.push_text("Plain item with tag #task", false);
            let tag_filters = [Tag::parse("#task").unwrap()];
            tracker.end_item(
                &tag_filters,
                &TaskStatusMap::default(),
                BytePos::new(0),
            );
            tracker.end_list();

            let item = tracker.lists.first().expect("item present");
            assert_eq!(item.kind(), &ListItemType::Plain);
        }
    }

    #[expect(clippy::panic, reason = "test assertions on enum variants")]
    mod fully_complete {
        use pretty_assertions::assert_eq;

        use super::*;
        use crate::TaskConfig;

        #[test]
        fn returns_true_for_leaf_task_with_no_children() {
            let note = parse("- [ ] Lone task");

            let item = note.lists().first().expect("item present");
            let ListItemType::Task(task) = item.kind() else {
                panic!("must be task");
            };
            assert_eq!(task.is_fully_complete(), true);
        }

        #[test]
        fn returns_true_when_all_child_tasks_are_done() {
            let note = parse("- [ ] Parent\n  - [x] Child 1\n  - [x] Child 2");

            let parent = note.lists().first().expect("parent present");
            let ListItemType::Task(parent_task) = parent.kind() else {
                panic!("must be task");
            };
            assert_eq!(parent_task.is_fully_complete(), true);
        }

        #[test]
        fn returns_true_when_child_task_is_cancelled() {
            let note = parse("- [ ] Parent\n  - [-] Cancelled child");

            let parent = note.lists().first().expect("parent present");
            let ListItemType::Task(parent_task) = parent.kind() else {
                panic!("must be task");
            };
            assert_eq!(parent_task.is_fully_complete(), true);
        }

        #[test]
        fn returns_true_when_child_tasks_are_mixed_done_and_cancelled() {
            let note = parse(
                "- [x] Parent\n  - [x] Done child\n  - [-] Cancelled child",
            );

            let parent = note.lists().first().expect("parent present");
            let ListItemType::Task(parent_task) = parent.kind() else {
                panic!("must be task");
            };
            assert_eq!(parent_task.is_fully_complete(), true);
        }

        #[test]
        fn returns_false_when_any_child_task_is_incomplete() {
            let note = parse(
                "- [x] Parent\n  - [x] Done child\n  - [ ] Incomplete child",
            );

            let parent = note.lists().first().expect("parent present");
            let ListItemType::Task(parent_task) = parent.kind() else {
                panic!("must be task");
            };
            assert_eq!(parent_task.is_fully_complete(), false);
        }

        #[test]
        fn returns_false_when_child_task_is_in_progress() {
            let note = parse("- [x] Parent\n  - [/] In progress child");

            let parent = note.lists().first().expect("parent present");
            let ListItemType::Task(parent_task) = parent.kind() else {
                panic!("must be task");
            };
            assert_eq!(parent_task.is_fully_complete(), false);
        }

        #[test]
        fn returns_true_when_only_plain_bullet_children_exist() {
            let note =
                parse("- [ ] Parent\n  - Plain child 1\n  - Plain child 2");

            let parent = note.lists().first().expect("parent present");
            let ListItemType::Task(parent_task) = parent.kind() else {
                panic!("must be task");
            };
            assert_eq!(parent_task.is_fully_complete(), true);
        }

        #[test]
        fn returns_true_when_only_non_task_checkbox_children_exist() {
            let tasks = TaskConfig::from_tags(&["#task"]);
            let frontmatter = crate::config::FrontmatterConfig::default();
            let input = MarkdownParserInput::new(
                std::path::Path::new("note.md"),
                "- [ ] Parent #task\n  - [ ] Checkbox without tag",
                &tasks,
                &frontmatter,
            );
            let note = parse_markdown(&input);

            let parent = note.lists().first().expect("parent present");
            let ListItemType::Task(parent_task) = parent.kind() else {
                panic!("must be task");
            };
            assert_eq!(parent_task.is_fully_complete(), true);
        }

        #[test]
        fn returns_true_when_three_levels_of_nested_tasks_are_all_done() {
            let note =
                parse("- [ ] Level 1\n  - [x] Level 2\n    - [x] Level 3");

            let items = note.lists();
            let l1 = items.first().expect("l1 present");
            let ListItemType::Task(t1) = l1.kind() else {
                panic!("must be task");
            };
            assert_eq!(t1.is_fully_complete(), true);

            let l2 = items.get(1).expect("l2 item present");
            let ListItemType::Task(t2) = l2.kind() else {
                panic!("must be task");
            };
            assert_eq!(t2.is_fully_complete(), true);

            let l3 = items.get(2).expect("l3 item present");
            let ListItemType::Task(t3) = l3.kind() else {
                panic!("must be task");
            };
            assert_eq!(t3.is_fully_complete(), true);
        }

        #[test]
        fn returns_false_when_grandchild_task_is_incomplete() {
            let note =
                parse("- [x] Level 1\n  - [x] Level 2\n    - [ ] Level 3");

            let items = note.lists();
            let l1 = items.first().expect("l1 present");
            let ListItemType::Task(t1) = l1.kind() else {
                panic!("must be task");
            };
            assert_eq!(t1.is_fully_complete(), false);

            let l2 = items.get(1).expect("l2 item present");
            let ListItemType::Task(t2) = l2.kind() else {
                panic!("must be task");
            };
            assert_eq!(t2.is_fully_complete(), false);

            let l3 = items.get(2).expect("l3 item present");
            let ListItemType::Task(t3) = l3.kind() else {
                panic!("must be task");
            };
            assert_eq!(t3.is_fully_complete(), true);
        }

        #[test]
        fn returns_false_when_incomplete_task_is_nested_under_plain_child() {
            let note = parse(
                "- [x] Parent\n  - Plain bullet\n    - [ ] Incomplete subtask",
            );

            let parent = note.lists().first().expect("parent present");
            let ListItemType::Task(parent_task) = parent.kind() else {
                panic!("must be task");
            };
            assert_eq!(parent_task.is_fully_complete(), false);
        }

        #[test]
        fn returns_true_when_complete_task_is_nested_under_plain_child() {
            let note =
                parse("- [ ] Parent\n  - Plain bullet\n    - [x] Done subtask");

            let parent = note.lists().first().expect("parent present");
            let ListItemType::Task(parent_task) = parent.kind() else {
                panic!("must be task");
            };
            assert_eq!(parent_task.is_fully_complete(), true);
        }

        #[test]
        fn returns_false_when_child_task_has_unknown_marker() {
            let note = parse("- [x] Parent\n  - [?] Unknown marker child");

            let parent = note.lists().first().expect("parent present");
            let ListItemType::Task(parent_task) = parent.kind() else {
                panic!("must be task");
            };
            assert_eq!(parent_task.is_fully_complete(), false);
        }
    }
    /// Returns the first task item of `note`, or `None` if it has none.
    fn first_task(note: &Note) -> Option<&TaskListItem> {
        note.tasks().find_map(|item| item.kind().as_task())
    }

    mod priority_extraction {
        use pretty_assertions::assert_eq;
        use rstest::rstest;

        use super::*;

        #[rstest]
        #[case("- [ ] Task 🔺", TaskPriority::Highest)]
        #[case("- [ ] Task 🔺\u{FE0F}", TaskPriority::Highest)]
        #[case("- [ ] Task ⏫", TaskPriority::High)]
        #[case("- [ ] Task ⏫\u{FE0F}", TaskPriority::High)]
        #[case("- [ ] Task 🔼", TaskPriority::Medium)]
        #[case("- [ ] Task 🔼\u{FE0F}", TaskPriority::Medium)]
        #[case("- [ ] Task 🔽", TaskPriority::Low)]
        #[case("- [ ] Task 🔽\u{FE0F}", TaskPriority::Low)]
        #[case("- [ ] Task ⏬", TaskPriority::Lowest)]
        #[case("- [ ] Task ⏬\u{FE0F}", TaskPriority::Lowest)]
        fn extracts_priority_from_emojis(
            #[case] input: &str,
            #[case] expected: TaskPriority,
        ) {
            let note = parse(input);
            let item = note.tasks().next().expect("task present");
            let task = item.kind().as_task().expect("task kind");

            assert_eq!(task.priority(), Some(expected));
            assert_eq!(item.text().clean(), "Task");
        }

        #[test]
        fn returns_none_when_priority_emoji_is_missing() {
            let note = parse("- [ ] Plain task without priority");
            let task = first_task(&note).expect("task present");

            assert_eq!(task.priority(), None);
        }

        #[test]
        fn extracts_priority_from_inline_field_when_emoji_absent() {
            let note = parse("- [ ] Task [priority:: high]");
            let item = note.tasks().next().expect("task present");
            let task = item.kind().as_task().expect("task kind");

            assert_eq!(task.priority(), Some(TaskPriority::High));
            assert_eq!(item.text().clean(), "Task");
        }

        #[test]
        fn stores_none_for_an_explicit_normal_priority_field() {
            let note = parse("- [ ] Task [priority:: normal]");
            let task = first_task(&note).expect("task present");

            assert_eq!(task.priority(), None);
        }

        #[test]
        fn prefers_earliest_priority_emoji_when_multiple_present() {
            let note = parse("- [ ] Task ⏫ then 🔽 later");
            let item = note.tasks().next().expect("task present");
            let task = item.kind().as_task().expect("task kind");

            assert_eq!(task.priority(), Some(TaskPriority::High));
            assert_eq!(item.text().clean(), "Task then later");
        }

        #[test]
        fn prefers_priority_emoji_over_inline_priority_field() {
            let note = parse("- [ ] Task 🔼 [priority:: high]");
            let task = first_task(&note).expect("task present");

            assert_eq!(task.priority(), Some(TaskPriority::Medium));
        }

        #[test]
        fn extracts_first_valid_inline_priority_after_invalid_value() {
            let note =
                parse("- [ ] Task [priority:: urgent] [priority:: high]");
            let task = first_task(&note).expect("task present");

            assert_eq!(task.priority(), Some(TaskPriority::High));
        }
    }

    mod date_extraction {
        use pretty_assertions::assert_eq;

        use super::*;

        fn date(year: i32, month: u32, day: u32) -> Option<DateValue> {
            NaiveDate::from_ymd_opt(year, month, day).map(Into::into)
        }
        #[rstest]
        #[case::emoji(
            "- [ ] Task ➕ 2025-01-01 🛫 2025-01-05 ⏳ 2025-01-10 📅 \
             2025-01-15 ✅ 2025-01-20 ❌ 2025-01-25"
        )]
        #[case::emoji_with_variation_selectors(
            "- [ ] Task ➕\u{FE0F} 2025-01-01 🛫\u{FE0F} 2025-01-05 \
             ⏳\u{FE0F} 2025-01-10 📅\u{FE0F} 2025-01-15 ✅\u{FE0F} \
             2025-01-20 ❌\u{FE0F} 2025-01-25"
        )]
        #[case::inline_fields(
            "- [ ] Task [created:: 2025-01-01] [start:: 2025-01-05] \
             [scheduled:: 2025-01-10] [due:: 2025-01-15] [done:: 2025-01-20] \
             [cancelled:: 2025-01-25]"
        )]
        fn extracts_all_lifecycle_dates_from_supported_syntax(
            #[case] input: &str,
        ) {
            let note = parse(input);
            let task = first_task(&note).expect("task present");
            let dates = task.dates();

            assert_eq!(dates.get(TaskDateType::Created), date(2025, 1, 1));
            assert_eq!(dates.get(TaskDateType::Start), date(2025, 1, 5));
            assert_eq!(dates.get(TaskDateType::Scheduled), date(2025, 1, 10));
            assert_eq!(dates.get(TaskDateType::Due), date(2025, 1, 15));
            assert_eq!(dates.get(TaskDateType::Done), date(2025, 1, 20));
            assert_eq!(dates.get(TaskDateType::Cancelled), date(2025, 1, 25));
        }

        #[rstest]
        #[case::without_variation_selector("🗓")]
        #[case::with_variation_selector("🗓\u{FE0F}")]
        fn extracts_due_date_from_spiral_calendar_emojis(#[case] emoji: &str) {
            let note = parse(&format!("- [ ] Task {emoji} 2025-01-15"));
            let task = first_task(&note).expect("task present");

            assert_eq!(task.dates().get(TaskDateType::Due), date(2025, 1, 15));
        }

        #[test]
        fn prefers_emoji_date_over_inline_field_when_both_present() {
            let input = "- [ ] Task 📅 2025-03-01 [due:: 2025-03-15]";
            let note = parse(input);
            let task = first_task(&note).expect("task present");

            assert_eq!(task.dates().get(TaskDateType::Due), date(2025, 3, 1));
        }

        #[test]
        fn returns_none_for_an_invalid_calendar_date_in_emoji_syntax() {
            let input = "- [ ] Task 📅 2025-02-30";
            let note = parse(input);
            let task = first_task(&note).expect("task present");

            assert_eq!(task.dates().get(TaskDateType::Due), None);
        }

        #[test]
        fn rejects_datetime_shorthand_with_alphanumeric_terminator() {
            // `📅 2025-01-15T12:00` is not a valid shorthand date: both the
            // lexer and the date scanner must reject it so no due date is
            // extracted from plain text.
            let input = "- [ ] Task 📅 2025-01-15T12:00";
            let note = parse(input);
            let task = first_task(&note).expect("task present");

            assert_eq!(task.dates().get(TaskDateType::Due), None);
        }

        #[test]
        fn skips_multibyte_text_after_date_emoji_without_panicking() {
            // Byte 10 of the multibyte candidate is not a char boundary; the
            // scanner must skip it and still find the valid date that follows.
            let input = "- [ ] Task 📅 你好你好 📅 2025-01-15";
            let note = parse(input);
            let task = first_task(&note).expect("task present");

            assert_eq!(task.dates().get(TaskDateType::Due), date(2025, 1, 15));
            assert!(
                note.lists()
                    .first()
                    .expect("item present")
                    .clean_text()
                    .contains("你好你好")
            );
        }

        #[test]
        fn returns_none_for_an_invalid_date_in_inline_field_syntax() {
            let input = "- [ ] Task [start:: not-a-date]";
            let note = parse(input);
            let task = first_task(&note).expect("task present");

            assert_eq!(task.dates().get(TaskDateType::Start), None);
        }

        #[test]
        fn ignores_emoji_date_inside_inline_code() {
            let input = "- [ ] task with `🗓2022-07-14`";
            let note = parse(input);
            let item = note.lists().first().expect("item present");
            let task = item.kind().as_task().expect("task kind");
            assert_eq!(task.dates().get(TaskDateType::Due), None);
            assert_eq!(item.clean_text(), "task with 🗓2022-07-14");
        }

        #[test]
        fn extracts_emoji_date_when_outside_inline_code() {
            let input = "- [ ] task 🗓2022-07-14 with `code`";
            let note = parse(input);
            let item = note.lists().first().expect("item present");
            let task = item.kind().as_task().expect("task kind");
            assert_eq!(task.dates().get(TaskDateType::Due), date(2022, 7, 14));
            assert_eq!(item.clean_text(), "task with code");
        }

        #[test]
        fn ignores_emoji_date_inside_fenced_code_block() {
            let input = "- [ ] task\n  ```\n  🗓2022-07-14\n  ```";
            let note = parse(input);
            let item = note.lists().first().expect("item present");
            let task = item.kind().as_task().expect("task kind");
            assert_eq!(task.dates().get(TaskDateType::Due), None);
        }
    }
}
