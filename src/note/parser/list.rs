//! Nested list and list-item tracking.
//!
//! [`ListTracker`] maintains explicit list and list-item stacks so nested
//! Markdown structures never recurse through the call stack. [`ItemFrame`]
//! pairs an active item's buffers, marker state, and scanned metadata, then
//! finalizes the frame into a classified [`ListItem`].
use indexmap::IndexMap;

use super::{
    FlushedMetadata,
    marker::{MarkerAccumulator, MarkerAction},
    task::SubTaskCompletion,
};
use crate::{
    FieldKey, SourceLine, Tag, TaskStatusMap,
    note::{ListItem, ListItemType, ListText, NoteFieldValue},
};

/// Nested list and list-item state for one Markdown event stream.
///
/// Completed top-level lists live in `lists`; still-open lists and items live
/// on explicit stacks.
#[derive(Default)]
pub(super) struct ListTracker {
    lists: Vec<ListItem>,
    list_stack: Vec<ListFrame>,
    item_stack: Vec<ItemFrame>,
}

impl ListTracker {
    /// Consumes the tracker, returning completed top-level lists.
    pub(super) fn into_lists(self) -> Vec<ListItem> {
        self.lists
    }

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

    /// Scans and flushes the active list item's scan buffer.
    ///
    /// Returns the flushed metadata for inclusion in document-level
    /// collections, or `None` if no item is active or the scan buffer is empty.
    fn flush_active_item_scan_buffer(&mut self) -> Option<FlushedMetadata> {
        self.item_stack.last_mut()?.flush_scan_buffer()
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
    /// Sets initial item hierarchy:
    /// - `depth`: 0-indexed nesting level derived from the list stack
    /// - `parent`: source line of the enclosing active list item, if any
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
    /// [`Self::resolve_pending_marker`]) before the item's final classification
    /// reads its scanned metadata. Returns the flushed inline fields and tags,
    /// if any.
    pub(super) fn end_item(
        &mut self,
        tag_filters: &[Tag],
        statuses: &TaskStatusMap,
    ) -> Option<FlushedMetadata> {
        let flushed = self.flush_active_item_scan_buffer();
        if let Some(frame) = self.item_stack.pop() {
            let finalized = frame.finish(tag_filters, statuses);
            if let Some(parent_item) = self.item_stack.last_mut() {
                parent_item.subtask_completion.observe_child(
                    finalized.is_task,
                    finalized.is_complete,
                    finalized.completion,
                );
                parent_item.descendants.push(finalized.item);
                parent_item.descendants.extend(finalized.descendants);
            } else {
                self.lists.push(finalized.item);
                self.lists.extend(finalized.descendants);
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

/// Dual buffer pair maintaining display text and metadata scan text for an
/// item.
///
/// The display buffer receives all item text, while the scan buffer excludes
/// code spans and blocks to prevent false positive field and tag matches.
struct ItemBuffers {
    text: String,
    scan: String,
    code_spans: Vec<std::ops::Range<usize>>,
}

impl ItemBuffers {
    /// Creates an empty pair of display text and scan buffers.
    const fn new() -> Self {
        Self {
            text: String::new(),
            scan: String::new(),
            code_spans: Vec::new(),
        }
    }

    /// Appends `text` to `text`, and to `scan` unless code is hidden.
    fn append(&mut self, text: &str, code_hidden: bool) {
        if code_hidden {
            let start = self.text.len();
            self.text.push_str(text);
            self.code_spans.push(start..self.text.len());
        } else {
            self.text.push_str(text);
            self.scan.push_str(text);
        }
    }

    /// Appends `text` to both buffers unconditionally.
    ///
    /// Only withheld marker-byte flushes use this; those bytes are never code.
    fn append_verbatim(&mut self, text: &str) {
        self.text.push_str(text);
        self.scan.push_str(text);
    }

    /// Appends code text exclusively to the display text buffer.
    fn push_code(&mut self, text: &str) {
        let start = self.text.len();
        self.text.push_str(text);
        self.code_spans.push(start..self.text.len());
    }

    /// Appends a character exclusively to the scan buffer.
    fn push_scan_char(&mut self, ch: char) {
        self.scan.push(ch);
    }

    /// Returns a reference to the display text.
    fn text(&self) -> &str {
        &self.text
    }

    /// Returns a reference to tracked code byte spans.
    fn code_spans(&self) -> &[std::ops::Range<usize>] {
        &self.code_spans
    }

    /// Returns `true` if the scan buffer is empty.
    fn is_scan_empty(&self) -> bool {
        self.scan.is_empty()
    }

    /// Separates a block-level child from prior content in both buffers.
    ///
    /// Inserts a newline delimiter unless the buffer is empty or already
    /// newline-terminated.
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
    /// Forwards text through the marker machine and applies the returned
    /// [`MarkerAction`] to the item's buffers.
    fn push_text(&mut self, text: &str, in_code_block: bool) {
        match self.marker.push_text(text) {
            MarkerAction::None => {}
            MarkerAction::Append(rem) => {
                self.buffers.append(rem, in_code_block);
            }
            MarkerAction::FlushPlain {
                buffered,
                trailing,
            } => {
                self.buffers.append_verbatim(buffered);
                self.buffers.append(trailing, in_code_block);
            }
        }
    }

    /// Resolves any pending marker, then writes the line terminator to both
    /// buffers.
    fn push_break(&mut self) {
        self.resolve_pending_marker();
        self.buffers.push_break();
    }

    /// Rejects a pending marker: inline content occupies the item's leading
    /// slot, so no marker can be recognized there.
    fn reject_marker(&mut self) {
        if let MarkerAction::FlushPlain {
            buffered,
            trailing,
        } = self.marker.reject()
        {
            self.buffers.append_verbatim(buffered);
            self.buffers.append(trailing, false);
        }
    }

    /// Force-decides a pending marker as if the item's first line ended.
    fn resolve_pending_marker(&mut self) {
        if let MarkerAction::FlushPlain {
            buffered,
            trailing,
        } = self.marker.resolve_at_line_end()
        {
            self.buffers.append_verbatim(buffered);
            self.buffers.append(trailing, false);
        }
    }

    /// Appends a literal character exclusively to the scan buffer.
    fn push_scan_char(&mut self, ch: char) {
        self.buffers.push_scan_char(ch);
    }

    /// Appends code text exclusively to the display text buffer.
    fn push_code(&mut self, text: &str) {
        self.buffers.push_code(text);
    }

    /// Scans and flushes this item's scan buffer into its local metadata.
    ///
    /// The marker state must be decided before the scan mode is read: a
    /// pending `- [x]` item flushes when a nested list starts, with no
    /// trailing-whitespace text chunk ever arriving. A marked item scans with
    /// task emoji shorthands enabled; a plain item scans without them.
    ///
    /// Returns the flushed metadata, or `None` if the scan buffer is empty.
    fn flush_scan_buffer(&mut self) -> Option<FlushedMetadata> {
        self.resolve_pending_marker();
        if self.buffers.is_scan_empty() {
            return None;
        }
        let text = self.buffers.take_scan();
        let mode = if self.marker.is_marked() {
            super::lexer::TaskFieldEmojis::Include
        } else {
            super::lexer::TaskFieldEmojis::Exclude
        };
        let flushed = super::lexer::scan_metadata(&text, mode);
        for (key, value) in flushed.fields() {
            self.fields.entry(key.clone()).or_default().push(value.clone());
        }
        self.tags.extend_from_slice(flushed.tags());
        Some(flushed)
    }

    /// Consumes the frame into a classified [`ListItem`].
    ///
    /// One tokenization pass feeds priority, date, and clean-text extraction.
    /// Extraction reads must precede the move of the raw text into `ListText`
    /// (NLL-enforced).
    fn finish(
        self,
        tag_filters: &[Tag],
        statuses: &TaskStatusMap,
    ) -> FinalizedItem {
        let scan = super::task::TaskScan::scan(
            self.buffers.text(),
            self.buffers.code_spans(),
        );
        let (item_type, clean) =
            scan.classify(super::task::TaskClassificationParams {
                marker: self.marker.marker_symbol(),
                fields: &self.fields,
                tags: &self.tags,
                tag_filters,
                statuses,
                fully_complete: self.subtask_completion.is_fully_complete(),
            });
        let text = ListText::new(self.buffers.into_text(), clean);
        let (is_task, is_complete) = match &item_type {
            ListItemType::Task(task) => {
                (true, task.status().kind().is_complete())
            }
            _ => (false, true),
        };
        FinalizedItem {
            item: ListItem::new(self.line, text, item_type)
                .with_depth(self.depth)
                .with_parent(self.parent)
                .with_is_ordered(self.is_ordered)
                .with_fields(self.fields)
                .with_tags(self.tags),
            is_task,
            is_complete,
            completion: self.subtask_completion,
            descendants: self.descendants,
        }
    }
}

/// A list item separated from its frame, ready for hierarchy attachment.
struct FinalizedItem {
    /// The classified item with its position, text, fields, and tags.
    item: ListItem,
    /// Whether the item classified as a task.
    is_task: bool,
    /// Whether the item's own task status is complete; `true` for non-tasks.
    is_complete: bool,
    /// The item's descendant-task completion state at close time.
    completion: SubTaskCompletion,
    /// Child items accumulated while the frame was open.
    descendants: Vec<ListItem>,
}

/// An active list frame on the parser stack.
struct ListFrame {
    is_ordered: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Note, SourceLine, Tag, TaskStatusMap, note::ListItem,
        parse_note_str as parse,
    };

    /// Collects the raw text of every task in document order.
    fn task_raw_texts(note: &Note) -> Vec<&str> {
        note.tasks().map(ListItem::raw_text).collect()
    }

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

    mod iteration {
        use pretty_assertions::assert_eq;

        use super::*;
        #[test]
        fn iterates_top_level_task_items() {
            let input = "- [ ] Task 1\n- Plain item\n- [x] Task 2";
            let note = parse(input);

            assert_eq!(task_raw_texts(&note), ["Task 1", "Task 2"]);
        }

        #[test]
        fn iterates_nested_sub_list_task_items() {
            let input = "- Plain parent\n  - [x] Subtask 1";
            let note = parse(input);

            assert_eq!(task_raw_texts(&note), ["Subtask 1"]);
            let task = note
                .tasks()
                .next()
                .expect("task present")
                .kind()
                .as_task()
                .expect("task kind");
            assert_eq!(task.status().kind().completed(), Some(true));
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

    fn parse_item_with_filters(text: &str, tag_filters: &[Tag]) -> ListItem {
        let mut tracker = ListTracker::default();
        tracker.start_list(false);
        tracker.start_item(SourceLine::new(1).expect("non-zero"));
        tracker.push_text(text, false);
        tracker.end_item(tag_filters, &TaskStatusMap::default());
        tracker.end_list();
        tracker.into_lists().into_iter().next().expect("item present")
    }
}
