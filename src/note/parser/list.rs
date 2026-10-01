//! Nested list and list-item tracking, marker classification, and task metadata
//! extraction.
//!
//! [`ListTracker`] maintains explicit list and list-item stacks so nested
//! Markdown structures never recurse through the call stack. [`ItemFrame`]
//! drives the incremental [`MarkerAccumulator`] state machine that detects
//! leading task markers.
//!
//! Status-marked items are evaluated against configured tag filters to classify
//! them as [`ListItemType::Task`] or [`ListItemType::Checkbox`], extracting
//! dates, priorities, and normalized clean text.
use indexmap::IndexMap;

use super::{FlushedMetadata, marker::MarkerAccumulator};
use crate::{
    FieldKey, SourceLine, Tag, TaskStatusMap,
    note::{ListItem, ListItemType, ListText, NoteFieldValue, TaskListItem},
};

/// Nested list and list-item state for one Markdown event stream.
///
/// Completed top-level lists live in `lists`; still-open lists and items live
/// on explicit stacks.
#[derive(Default)]
pub(super) struct ListTracker {
    pub(super) lists: Vec<ListItem>,
    list_stack: Vec<ListFrame>,
    item_stack: Vec<ItemFrame>,
}

impl ListTracker {
    /// Starts a block-level child inside the active item.
    ///
    /// Separates it from prior buffer content with a newline (never doubled).
    /// Returns `false` if no list item is active, allowing the caller to treat
    /// the block as top-level text.
    pub(super) fn start_nested_block(&mut self) -> bool {
        let Some(item) = self.item_stack.last_mut() else {
            return false;
        };
        item.buffers.separate_block();
        true
    }

    /// Returns `true` if there is an active list item.
    pub(super) const fn is_item_active(&self) -> bool {
        !self.item_stack.is_empty()
    }

    /// Records an inline code span on the active item's display text only.
    /// Inline code is excluded from inline field/tag scanning.
    pub(super) fn inline_code(&mut self, text: &str) {
        if let Some(item) = self.item_stack.last_mut() {
            item.push_code(text);
        }
    }

    /// Rejects a pending item-leading marker: inline content occupies the
    /// item's leading slot, so no marker can be recognized there.
    pub(super) fn reject_marker(&mut self) {
        if let Some(item) = self.item_stack.last_mut() {
            item.reject_marker();
        }
    }

    /// Force-decides a pending item-leading marker as if the item's first line
    /// ended: a complete `[<char>]` shape becomes a marker.
    ///
    /// Called before the classification state is read (scan-buffer flushes,
    /// item end) and on block-structure events that terminate the first line.
    pub(super) fn resolve_pending_marker(&mut self) {
        if let Some(item) = self.item_stack.last_mut() {
            item.resolve_pending_marker();
        }
    }

    /// Lexes and clears the active list item's scan buffer.
    ///
    /// Returns the inline fields and tags yielded by that buffer, or `None` if
    /// no item is active or the buffer is empty. Called before nested lists
    /// start and when an item closes, both to preserve document-order metadata.
    ///
    /// The scan buffer excludes code text and flushes incrementally; the full
    /// text buffer is tokenized separately, once, in [`Self::end_item`], which
    /// also extracts dates, priority, and clean text from those tokens.
    fn flush_active_item_scan_buffer(&mut self) -> Option<FlushedMetadata> {
        // The marker state must be decided before `has_marker` is read: a
        // pending `- [x]` item flushes when a nested list starts, with no
        // trailing-whitespace text chunk ever arriving.
        self.resolve_pending_marker();
        let item = self.item_stack.last_mut()?;
        if item.buffers.is_scan_empty() {
            return None;
        }
        let text = item.buffers.take_scan();
        let shorthands = if item.marker.is_marked() {
            super::lexer::TaskShorthands::Include
        } else {
            super::lexer::TaskShorthands::Exclude
        };
        let raw_fields = super::lexer::scan_fields(&text, shorthands);
        let tags = super::tag::scan_tags(&text);
        // Two independently owned copies, not a borrow-checker workaround:
        // `item.fields` lets a task/list item resolve its own metadata
        // (`ListItem::fields`), while the returned copy feeds the caller's
        // document-order stream every page-level query already relies on. Both
        // outlive this function inside different serialized structs, so neither
        // can borrow from the other.
        for (key, value) in &raw_fields {
            item.fields.entry(key.clone()).or_default().push(value.clone());
        }
        item.tags.extend(tags.iter().cloned());
        Some(FlushedMetadata::new(raw_fields, tags))
    }

    /// Pushes a list frame and flushes any active parent item's scan buffer.
    ///
    /// Returns the flushed inline fields and tags, if any.
    pub(super) fn start_list(
        &mut self,
        is_ordered: bool,
    ) -> Option<FlushedMetadata> {
        let flushed = self.flush_active_item_scan_buffer();
        self.list_stack.push(ListFrame {
            is_ordered,
        });
        flushed
    }

    /// Closes the innermost list.
    ///
    /// Items of a list nested inside an active item carry that item's line as
    /// their `parent` and a deeper `depth`; a top-level list's items are
    /// appended to the tracker's completed lists.
    pub(super) fn end_list(&mut self) {
        self.list_stack.pop();
    }

    /// Starts tracking a new list item at `line`.
    ///
    /// `depth` is the number of currently open lists (0-indexed); `parent` is
    /// the innermost active item's line, if this item is nested inside another
    /// item's child list.
    pub(super) fn start_item(&mut self, line: SourceLine) {
        let depth = u8::try_from(self.list_stack.len().saturating_sub(1))
            .unwrap_or(u8::MAX);
        let parent = self.item_stack.last().map(|item| item.line);
        let is_ordered =
            self.list_stack.last().is_some_and(|frame| frame.is_ordered);
        self.item_stack.push(ItemFrame {
            buffers: ItemBuffers::new(),
            fields: IndexMap::new(),
            tags: Vec::new(),
            line,
            depth,
            parent,
            is_ordered,
            marker: MarkerAccumulator::new(),
            subtask_completion: SubTaskCompletion::initial(),
            descendants: Vec::new(),
        });
    }

    /// Flushes and records the innermost list item.
    ///
    /// The flush decides any pending leading marker (see
    /// [`Self::resolve_pending_marker`]); a decided marker resolves to
    /// [`ListItemType::Task`] if tag filters are empty or any item tag matches
    /// a configured tag filter, and to [`ListItemType::Checkbox`] otherwise.
    /// Returns the flushed inline fields and tags, if any.
    pub(super) fn end_item(
        &mut self,
        tag_filters: &[Tag],
        statuses: &TaskStatusMap,
    ) -> Option<FlushedMetadata> {
        let flushed = self.flush_active_item_scan_buffer();
        if let Some(item_frame) = self.item_stack.pop() {
            let fully_complete =
                item_frame.subtask_completion.is_fully_complete();
            // One tokenization pass feeds priority, date, and clean-text
            // extraction. Extraction reads must precede the move of the raw
            // text into `ListText` (NLL-enforced).
            let scan = super::task::TaskScan::scan(item_frame.buffers.text());
            let clean = scan.clean_text(tag_filters);
            let item_type = match item_frame.marker.marker_symbol() {
                Some(symbol) => {
                    let status = statuses.resolve(symbol);
                    if tag_filters.is_empty()
                        || item_frame
                            .tags
                            .iter()
                            .any(|tag| tag_filters.contains(tag))
                    {
                        let priority = scan.priority(&item_frame.fields);
                        let dates = scan.dates(&item_frame.fields);
                        ListItemType::Task(TaskListItem::new(
                            dates,
                            priority,
                            status,
                            fully_complete,
                        ))
                    } else {
                        ListItemType::Checkbox
                    }
                }
                None => ListItemType::Plain,
            };
            let text = ListText::new(item_frame.buffers.into_text(), clean);
            let (is_task, is_complete) = match &item_type {
                ListItemType::Task(task) => {
                    (true, task.status().kind().is_complete())
                }
                _ => (false, true),
            };
            let item = ListItem::new(item_frame.line, text, item_type)
                .with_depth(item_frame.depth)
                .with_parent(item_frame.parent)
                .with_is_ordered(item_frame.is_ordered)
                .with_fields(item_frame.fields)
                .with_tags(item_frame.tags);
            if let Some(parent_item) = self.item_stack.last_mut() {
                parent_item.subtask_completion.observe_child(
                    is_task,
                    is_complete,
                    item_frame.subtask_completion,
                );
                parent_item.descendants.push(item);
                parent_item.descendants.extend(item_frame.descendants);
            } else {
                self.lists.push(item);
                self.lists.extend(item_frame.descendants);
            }
        }
        flushed
    }

    /// Appends text to the active item's display text and scan buffer.
    ///
    /// Returns `false` if there is no active item.
    pub(super) fn push_text(
        &mut self,
        text: &str,
        in_code_block: bool,
    ) -> bool {
        let Some(item) = self.item_stack.last_mut() else {
            return false;
        };
        item.push_text(text, in_code_block);
        true
    }

    /// Appends a line break to the active item's buffers.
    ///
    /// Returns `false` if there is no active item.
    pub(super) fn push_break(&mut self) -> bool {
        let Some(item) = self.item_stack.last_mut() else {
            return false;
        };
        item.push_break();
        true
    }

    /// Pushes a literal character into the active item's scan buffer.
    ///
    /// Returns `false` if there is no active item.
    pub(super) fn push_scan_char(&mut self, ch: char) -> bool {
        let Some(item) = self.item_stack.last_mut() else {
            return false;
        };
        item.push_scan_char(ch);
        true
    }
}

/// Tracks whether any descendant task within a list item's sub-tree is
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

    /// Observes a closed child item, transitioning to [`Self::HasIncomplete`]
    /// if the child is an unresolved task or contains incomplete subtasks.
    #[inline]
    pub(super) fn observe_child(
        &mut self,
        is_task: bool,
        is_complete: bool,
        child_subtask_completion: Self,
    ) {
        if (is_task && !is_complete)
            || child_subtask_completion == Self::HasIncomplete
        {
            *self = Self::HasIncomplete;
        }
    }

    /// Returns `true` if all descendant tasks in the sub-tree are complete.
    #[inline]
    #[must_use]
    pub(super) const fn is_fully_complete(self) -> bool {
        matches!(self, Self::AllComplete)
    }
}

/// Dual write target for an active item's text.
///
/// `text` receives everything and becomes display text; `scan` mirrors it but
/// excludes code-span and code-block text so inline field/tag scanning never
/// sees code. Asymmetric writes (scan-only separators, text-only code) stay
/// direct field access inside this module; [`super::marker`] mutates the pair
/// only through these methods.
pub(super) struct ItemBuffers {
    text: String,
    scan: String,
}

impl ItemBuffers {
    pub(super) const fn new() -> Self {
        Self {
            text: String::new(),
            scan: String::new(),
        }
    }

    /// Appends `text` to `text`, and to `scan` unless code is hidden.
    pub(super) fn append(&mut self, text: &str, code_hidden: bool) {
        self.text.push_str(text);
        if !code_hidden {
            self.scan.push_str(text);
        }
    }

    /// Appends `text` to both buffers unconditionally.
    ///
    /// Only withheld marker-byte flushes use this; those bytes are never code.
    pub(super) fn append_verbatim(&mut self, text: &str) {
        self.text.push_str(text);
        self.scan.push_str(text);
    }

    /// Appends code text exclusively to the display text buffer.
    pub(super) fn push_code(&mut self, text: &str) {
        self.text.push_str(text);
    }

    /// Appends a character exclusively to the scan buffer.
    pub(super) fn push_scan_char(&mut self, ch: char) {
        self.scan.push(ch);
    }

    /// Returns a reference to the display text.
    pub(super) fn text(&self) -> &str {
        &self.text
    }

    /// Returns a reference to the scan buffer text.
    #[cfg(test)]
    pub(super) fn scan(&self) -> &str {
        &self.scan
    }

    /// Returns `true` if the scan buffer is empty.
    pub(super) fn is_scan_empty(&self) -> bool {
        self.scan.is_empty()
    }

    /// Separates a block-level child from prior content in both buffers.
    ///
    /// Writes a newline unless the buffer is empty or already newline-
    /// terminated, so nested block starts (a blockquote's inner paragraph, for
    /// example) never double-separate. While [`MarkerAccumulator`] is
    /// buffering, both buffers are empty, so a separator can never precede
    /// withheld marker bytes.
    fn separate_block(&mut self) {
        for buffer in [&mut self.text, &mut self.scan] {
            if !buffer.is_empty() && !buffer.ends_with('\n') {
                buffer.push('\n');
            }
        }
    }

    /// Writes the line terminator to both buffers.
    fn push_break(&mut self) {
        self.text.push('\n');
        self.scan.push('\n');
    }

    /// Takes the scan buffer's contents, leaving it empty.
    fn take_scan(&mut self) -> String {
        std::mem::take(&mut self.scan)
    }

    /// Consumes the buffer pair, returning the display text.
    fn into_text(self) -> String {
        self.text
    }
}

/// An active list item frame on the parser stack.
struct ItemFrame {
    buffers: ItemBuffers,
    /// Inline fields lexed from this item's own text.
    ///
    /// Kept separate from child items' fields so [`ListItem::fields`] resolves
    /// per-item, not per-list.
    fields: IndexMap<FieldKey, Vec<NoteFieldValue>>,
    /// Tags scanned from this item's own text, used to classify a
    /// status-marked item as [`ListItemType::Task`] or
    /// [`ListItemType::Checkbox`] against configured tag filters.
    tags: Vec<Tag>,
    line: SourceLine,
    depth: u8,
    parent: Option<SourceLine>,
    is_ordered: bool,
    marker: MarkerAccumulator,
    subtask_completion: SubTaskCompletion,
    descendants: Vec<ListItem>,
}

impl ItemFrame {
    fn push_text(&mut self, text: &str, in_code_block: bool) {
        self.marker.push_text(text, &mut self.buffers, in_code_block);
    }

    fn push_break(&mut self) {
        self.marker.resolve_at_line_end(&mut self.buffers);
        self.buffers.push_break();
    }

    fn reject_marker(&mut self) {
        self.marker.reject(&mut self.buffers);
    }

    fn resolve_pending_marker(&mut self) {
        self.marker.resolve_at_line_end(&mut self.buffers);
    }

    fn push_scan_char(&mut self, ch: char) {
        self.buffers.push_scan_char(ch);
    }

    fn push_code(&mut self, text: &str) {
        self.buffers.push_code(text);
    }
}

/// An active list frame on the parser stack.
struct ListFrame {
    is_ordered: bool,
}

#[cfg(test)]
mod tests {
    use chrono::NaiveDate;
    use rstest::rstest;

    use super::*;
    use crate::{
        DateValue, Note, TaskDateType, TaskPriority, TaskStatusType,
        note::{MarkdownParserInput, parse_markdown},
        parse_note_str as parse,
    };
    mod tracker_state {

        use super::*;
        #[test]
        fn is_item_active_returns_true_when_stack_nonempty() {
            let mut tracker = ListTracker::default();
            assert!(!tracker.is_item_active());

            tracker.start_list(false);
            tracker.start_item(SourceLine::new(1).expect("non-zero"));

            assert!(
                tracker.is_item_active(),
                "is_item_active must return true after start_item"
            );
        }

        #[test]
        fn inline_code_pushes_to_last_item_not_first() {
            let mut tracker = ListTracker::default();
            tracker.start_list(false);
            tracker.start_item(SourceLine::new(1).expect("non-zero"));
            tracker.push_text("before ", false);
            tracker.start_item(SourceLine::new(2).expect("non-zero"));

            tracker.inline_code("code");

            tracker.end_item(&[], &TaskStatusMap::default());
            tracker.end_item(&[], &TaskStatusMap::default());
            tracker.end_list();

            let item1_text =
                tracker.lists.first().expect("item 1 must exist").raw_text();
            let item2_text =
                tracker.lists.get(1).expect("item 2 must exist").raw_text();
            assert!(
                !item1_text.contains("code"),
                "inline code must not leak to item 1, got: {item1_text:?}"
            );
            assert!(
                item2_text.contains("code"),
                "inline code must be in item 2, got: {item2_text:?}"
            );
        }

        #[test]
        fn push_scan_char_returns_false_when_no_item_active() {
            let mut tracker = ListTracker::default();
            let result = tracker.push_scan_char('a');
            assert!(
                !result,
                "push_scan_char must return false when no item is active"
            );
        }

        #[test]
        fn start_nested_block_returns_false_when_no_item() {
            let mut tracker = ListTracker::default();
            assert!(
                !tracker.start_nested_block(),
                "start_nested_block must return false with no item"
            );
        }

        #[test]
        fn start_nested_block_returns_true_with_active_item() {
            let mut tracker = ListTracker::default();
            tracker.start_list(false);
            tracker.start_item(SourceLine::new(1).expect("non-zero"));
            assert!(
                tracker.start_nested_block(),
                "start_nested_block must return true with active item"
            );
        }

        #[test]
        fn start_list_flushes_active_item_scan_buffer() {
            let mut tracker = ListTracker::default();
            tracker.start_list(false);
            tracker.start_item(SourceLine::new(1).expect("non-zero"));
            tracker.push_text("Status:: Draft", false);

            let flushed = tracker.start_list(false);

            assert!(
                flushed.is_some(),
                "start_list must flush active item scan buffer"
            );
            let metadata = flushed.unwrap();
            let has_status = metadata
                .fields()
                .iter()
                .any(|(k, _)| k.is_canonical_match("status"));
            assert!(has_status, "flushed fields must contain Status");
        }

        #[test]
        fn end_item_flushes_scan_buffer() {
            let mut tracker = ListTracker::default();
            tracker.start_list(false);
            tracker.start_item(SourceLine::new(1).expect("non-zero"));
            tracker.push_text("Author:: Jane", false);

            let flushed = tracker.end_item(&[], &TaskStatusMap::default());
            assert!(flushed.is_some(), "end_item must flush scan buffer");
            let metadata = flushed.unwrap();
            let has_author = metadata
                .fields()
                .iter()
                .any(|(k, _)| k.is_canonical_match("author"));
            assert!(has_author, "flushed fields must contain Author");
        }

        #[test]
        fn retains_fields_across_nested_list_flush() {
            let mut tracker = ListTracker::default();
            tracker.start_list(false);
            tracker.start_item(SourceLine::new(1).expect("non-zero"));
            tracker.push_text("Status:: Draft", false);

            let flushed1 = tracker.start_list(false);
            assert!(flushed1.is_some());

            tracker.end_list();
            tracker.push_text("Author:: Jane", false);

            let flushed2 = tracker.end_item(&[], &TaskStatusMap::default());
            assert!(flushed2.is_some());

            assert_eq!(tracker.lists.len(), 1);
            let item = tracker.lists.first().unwrap();
            let fields = item.fields().expect("fields present");
            assert!(fields.iter().any(|(k, _)| k.is_canonical_match("status")));
            assert!(fields.iter().any(|(k, _)| k.is_canonical_match("author")));
        }

        #[test]
        fn push_text_and_push_break_return_false_when_no_item_active() {
            let mut tracker = ListTracker::default();
            assert!(
                !tracker.push_text("hello", false),
                "push_text must return false when no item active"
            );
            assert!(
                !tracker.push_break(),
                "push_break must return false when no item active"
            );
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
    }

    mod iteration {
        use pretty_assertions::assert_eq;

        use super::*;
        #[test]
        fn iterates_top_level_task_items() {
            let input = "- [ ] Task 1\n- Plain item\n- [x] Task 2";
            let note = parse(input);

            let tasks: Vec<&ListItem> = note.tasks().collect();
            assert_eq!(tasks.len(), 2);
            assert_eq!(
                tasks.first().copied().map(ListItem::raw_text),
                Some("Task 1")
            );
            assert_eq!(
                tasks.get(1).copied().map(ListItem::raw_text),
                Some("Task 2")
            );
        }

        #[test]
        #[expect(clippy::panic, reason = "test assertion on enum variant")]
        fn iterates_nested_sub_list_task_items() {
            let input = "- Plain parent\n  - [x] Subtask 1";
            let note = parse(input);

            let tasks: Vec<&ListItem> = note.tasks().collect();
            assert_eq!(tasks.len(), 1);
            assert_eq!(
                tasks.first().copied().map(ListItem::raw_text),
                Some("Subtask 1")
            );
            let ListItemType::Task(task) = tasks.first().unwrap().kind() else {
                panic!("subtask must be a Task");
            };
            assert_eq!(task.status().kind().completed(), Some(true));
        }
    }
    mod tag_filters {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn classifies_marked_item_as_task_when_tag_filters_are_empty() {
            let mut tracker = ListTracker::default();
            tracker.start_list(false);
            tracker.start_item(SourceLine::new(1).expect("non-zero"));
            tracker.push_text("[x] Task without tag", false);
            tracker.end_item(&[], &TaskStatusMap::default());
            tracker.end_list();

            let item = tracker.lists.first().expect("item present");
            assert!(matches!(item.kind(), ListItemType::Task(_)));
        }

        #[test]
        fn classifies_marked_item_as_task_when_tag_matches_filter() {
            let mut tracker = ListTracker::default();
            tracker.start_list(false);
            tracker.start_item(SourceLine::new(1).expect("non-zero"));
            tracker.push_text("[x] Task with tag #task", false);
            let tag_filters = [Tag::parse("#task").unwrap()];
            tracker.end_item(&tag_filters, &TaskStatusMap::default());
            tracker.end_list();

            let item = tracker.lists.first().expect("item present");
            assert!(matches!(item.kind(), ListItemType::Task(_)));
        }

        #[test]
        fn item_tags_survive_classification_as_queryable_data() {
            let mut tracker = ListTracker::default();
            tracker.start_list(false);
            tracker.start_item(SourceLine::new(1).expect("non-zero"));
            tracker.push_text("[x] Task #task #project", false);
            let tag_filters = [Tag::parse("#task").unwrap()];
            tracker.end_item(&tag_filters, &TaskStatusMap::default());
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
            tracker.start_item(SourceLine::new(1).expect("non-zero"));
            tracker.push_text("[x] Checkbox with different tag #other", false);
            let tag_filters = [Tag::parse("#task").unwrap()];
            tracker.end_item(&tag_filters, &TaskStatusMap::default());
            tracker.end_list();

            let item = tracker.lists.first().expect("item present");
            assert_eq!(item.kind(), &ListItemType::Checkbox);
        }

        #[test]
        fn classifies_marked_item_as_checkbox_when_item_has_no_tags_and_filter_is_non_empty()
         {
            let mut tracker = ListTracker::default();
            tracker.start_list(false);
            tracker.start_item(SourceLine::new(1).expect("non-zero"));
            tracker.push_text("[x] Checkbox without tags", false);
            let tag_filters = [Tag::parse("#task").unwrap()];
            tracker.end_item(&tag_filters, &TaskStatusMap::default());
            tracker.end_list();

            let item = tracker.lists.first().expect("item present");
            assert_eq!(item.kind(), &ListItemType::Checkbox);
        }

        #[test]
        fn classifies_marked_item_as_task_when_one_of_multiple_tags_matches_filter()
         {
            let mut tracker = ListTracker::default();
            tracker.start_list(false);
            tracker.start_item(SourceLine::new(1).expect("non-zero"));
            tracker.push_text(
                "[x] Task with multiple tags #other #task #work",
                false,
            );
            let tag_filters = [Tag::parse("#task").unwrap()];
            tracker.end_item(&tag_filters, &TaskStatusMap::default());
            tracker.end_list();

            let item = tracker.lists.first().expect("item present");
            assert!(matches!(item.kind(), ListItemType::Task(_)));
        }

        #[test]
        fn classifies_marked_item_as_task_when_tag_matches_one_of_multiple_filters()
         {
            let mut tracker = ListTracker::default();
            tracker.start_list(false);
            tracker.start_item(SourceLine::new(1).expect("non-zero"));
            tracker.push_text("[x] Task matching second filter #todo", false);
            let tag_filters =
                [Tag::parse("#task").unwrap(), Tag::parse("#todo").unwrap()];
            tracker.end_item(&tag_filters, &TaskStatusMap::default());
            tracker.end_list();

            let item = tracker.lists.first().expect("item present");
            assert!(matches!(item.kind(), ListItemType::Task(_)));
        }

        #[test]
        fn rejects_prefix_match_for_exact_nested_tag() {
            let mut tracker = ListTracker::default();
            tracker.start_list(false);
            tracker.start_item(SourceLine::new(1).expect("non-zero"));
            tracker
                .push_text("[x] Checkbox with nested tag #task/project", false);
            let tag_filters = [Tag::parse("#task").unwrap()];
            tracker.end_item(&tag_filters, &TaskStatusMap::default());
            tracker.end_list();

            let item = tracker.lists.first().expect("item present");
            assert_eq!(item.kind(), &ListItemType::Checkbox);
        }

        #[test]
        fn accepts_exact_nested_tag_match() {
            let mut tracker = ListTracker::default();
            tracker.start_list(false);
            tracker.start_item(SourceLine::new(1).expect("non-zero"));
            tracker.push_text("[x] Task with nested tag #task/project", false);
            let tag_filters = [Tag::parse("#task/project").unwrap()];
            tracker.end_item(&tag_filters, &TaskStatusMap::default());
            tracker.end_list();

            let item = tracker.lists.first().expect("item present");
            assert!(matches!(item.kind(), ListItemType::Task(_)));
        }

        #[test]
        fn keeps_unmarked_item_plain_even_with_matching_tag() {
            let mut tracker = ListTracker::default();
            tracker.start_list(false);
            tracker.start_item(SourceLine::new(1).expect("non-zero"));
            tracker.push_text("Plain item with tag #task", false);
            let tag_filters = [Tag::parse("#task").unwrap()];
            tracker.end_item(&tag_filters, &TaskStatusMap::default());
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

    mod raw_vs_clean {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn stores_raw_and_clean_text_with_inline_syntax() {
            let note = parse("- [ ] Buy milk 📅 2025-01-15 #task");
            let item = note.lists().first().expect("item present");

            assert_eq!(item.text().raw(), "Buy milk 📅 2025-01-15 #task");
            assert_eq!(item.text().clean(), "Buy milk #task");
        }

        #[test]
        fn preserves_non_task_inline_fields_in_clean_text() {
            let note = parse("- [ ] Buy milk [store:: Costco] 📅 2025-01-15");
            let item = note.lists().first().expect("item present");

            assert_eq!(
                item.text().raw(),
                "Buy milk [store:: Costco] 📅 2025-01-15"
            );
            assert_eq!(item.text().clean(), "Buy milk [store:: Costco]");
        }

        #[test]
        fn preserves_non_filtered_tags_in_clean_text() {
            let tag_filters = [Tag::parse("#task").unwrap()];
            let item = parse_item_with_filters(
                "[ ] Buy milk #groceries 📅 2025-01-15 #task",
                &tag_filters,
            );

            assert_eq!(
                item.text().raw(),
                "Buy milk #groceries 📅 2025-01-15 #task"
            );
            assert_eq!(item.text().clean(), "Buy milk #groceries");
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
    }

    mod clean_text_stripping {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn strips_marker_tag_and_date_with_configured_tag_filter() {
            // Worked example 1 with tag filter
            let tag_filters = [Tag::parse("#task").unwrap()];
            let item = parse_item_with_filters(
                "[ ] Buy milk 📅 2025-01-15 #task",
                &tag_filters,
            );

            assert_eq!(item.text().raw(), "Buy milk 📅 2025-01-15 #task");
            assert_eq!(item.text().clean(), "Buy milk");
        }

        #[test]
        fn strips_marker_and_date_without_configured_tag_filter() {
            // Worked example 1 without tag filter
            let item = parse_item_with_filters(
                "[ ] Buy milk 📅 2025-01-15 #task",
                &[],
            );

            assert_eq!(item.text().raw(), "Buy milk 📅 2025-01-15 #task");
            assert_eq!(item.text().clean(), "Buy milk #task");
        }

        #[test]
        fn strips_priority_and_inline_task_field() {
            // Worked example 2
            let note = parse("- [x] Pay rent 🔼 [due:: 2025-02-01]");
            let item = note.lists().first().expect("item present");

            assert_eq!(item.text().raw(), "Pay rent 🔼 [due:: 2025-02-01]");
            assert_eq!(item.text().clean(), "Pay rent");
        }

        #[test]
        fn strips_in_prescribed_order() {
            let tag_filters = [Tag::parse("#task").unwrap()];
            let item = parse_item_with_filters(
                "[ ] #task Review PR 🔺 📅 2025-04-01 [scheduled:: \
                 2025-03-25] [custom:: keep-me]",
                &tag_filters,
            );

            assert_eq!(item.text().clean(), "Review PR [custom:: keep-me]");
        }
    }

    mod characterization_regression_net {
        use pretty_assertions::assert_eq;

        use crate::{DateValue, TaskDateType, TaskPriority};

        #[test]
        fn skips_invalid_date_after_emoji_and_finds_subsequent_valid_date() {
            let note =
                crate::parse_note_str("- [ ] Task 📅 2026-13-45 📅 2025-01-15");
            let item = note.lists().first().expect("item present");
            let task = item.kind().as_task().expect("task item");
            assert_eq!(
                task.dates().get(TaskDateType::Due),
                Some(DateValue::parse_iso("2025-01-15").unwrap())
            );
            assert!(item.clean_text().contains("📅 2026-13-45"));
            assert!(!item.clean_text().contains("2025-01-15"));
        }

        #[test]
        fn strips_priority_emoji_with_variation_selector_from_clean_text() {
            let note =
                crate::parse_note_str("- [ ] Task 🔺\u{FE0F} remaining text");
            let item = note.lists().first().expect("item present");
            let task = item.kind().as_task().expect("task item");
            assert_eq!(task.priority(), Some(TaskPriority::Highest));
            assert!(!item.clean_text().contains("\u{1F53A}"));
            assert!(!item.clean_text().contains("\u{FE0F}"));
        }

        #[test]
        fn collapses_inline_field_normal_priority_to_none() {
            let note = crate::parse_note_str("- [ ] Task [priority:: normal]");
            let item = note.lists().first().expect("item present");
            let task = item.kind().as_task().expect("task item");
            assert_eq!(task.priority(), None);
        }

        #[test]
        fn prefers_done_alias_over_completion_alias_when_both_present() {
            let note = crate::parse_note_str(
                "- [ ] Task [done:: 2025-01-01] [completion:: 2025-01-02]",
            );
            let item = note.lists().first().expect("item present");
            let task = item.kind().as_task().expect("task item");
            assert_eq!(
                task.dates().get(TaskDateType::Done),
                Some(DateValue::parse_iso("2025-01-01").unwrap())
            );
        }

        #[test]
        fn strips_emoji_dates_from_plain_bullet_item_without_extracting_fields()
        {
            let note = crate::parse_note_str("- Plain bullet 📅 2025-01-15");
            let item = note.lists().first().expect("item present");
            assert!(item.kind().is_plain());
            assert!(item.fields().is_none());
            assert_eq!(item.clean_text(), "Plain bullet");
        }

        #[test]
        fn strips_multiple_distinct_due_emojis_from_clean_text() {
            let note = crate::parse_note_str(
                "- [ ] Task 📅 2025-01-15 and 🗓 2025-02-02",
            );
            let item = note.lists().first().expect("item present");
            assert_eq!(item.clean_text(), "Task and");
        }
    }

    fn parse_item_with_filters(text: &str, tag_filters: &[Tag]) -> ListItem {
        let mut tracker = ListTracker::default();
        tracker.start_list(false);
        tracker.start_item(SourceLine::new(1).expect("non-zero"));
        tracker.push_text(text, false);
        tracker.end_item(tag_filters, &TaskStatusMap::default());
        tracker.end_list();
        tracker.lists.into_iter().next().expect("item present")
    }
}
