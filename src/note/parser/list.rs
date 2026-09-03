//! Nested list and list-item tracking, and item-leading task marker
//! classification.
//!
//! [`ListTracker`] holds explicit list and list-item stacks so nested Markdown
//! never recurses through the call stack. [`ItemFrame`] runs the incremental
//! [`ItemClassificationState`] state machine that determines an item's leading
//! marker and classification, gated like `pulldown-cmark`'s own first-pass
//! `scan_task_list_marker`: the marker is only valid at an item's content
//! start, so the decision is finalized before any inline content event or block
//! boundary.

use indexmap::IndexMap;

use super::{
    FlushedFields,
    lexer::InlineTokenLexer,
    marker::{MarkerPrefix, scan_marker_at_line_end, scan_marker_prefix},
};
use crate::{
    FieldKey, SourceLine, Tag, TaskStatusMap,
    note::{
        List, ListItem, ListItemType, NoteFieldValue, lists::ListItemPosition,
    },
};

/// Nested list and list-item state for one Markdown event stream.
///
/// Completed top-level lists live in `lists`; still-open lists and items live
/// on explicit stacks.
#[derive(Default)]
pub(super) struct ListTracker {
    pub(super) lists: Vec<List>,
    list_stack: Vec<ListFrame>,
    item_stack: Vec<ItemFrame>,
}

impl ListTracker {
    /// Starts a nested text block inside the active item.
    ///
    /// Separates it from prior scan-buffer content with a newline. Returns
    /// `false` if no list item is active, allowing the caller to treat the
    /// block as top-level text.
    pub(super) fn start_nested_text_block(&mut self) -> bool {
        let Some(item) = self.item_stack.last_mut() else {
            return false;
        };
        if !item.scan_buffer.is_empty() {
            item.scan_buffer.push('\n');
        }
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
    fn flush_active_item_scan_buffer(&mut self) -> FlushedFields {
        // The marker state must be decided before `has_marker` is read: a
        // pending `- [x]` item flushes when a nested list starts, with no
        // trailing-whitespace text chunk ever arriving.
        self.resolve_pending_marker();
        let item = self.item_stack.last_mut()?;
        if item.scan_buffer.is_empty() {
            return None;
        }
        let text = std::mem::take(&mut item.scan_buffer);
        let lexer = InlineTokenLexer::new(item.classification.is_marked());
        let raw_fields = lexer.extract_fields(&text);
        // Two independently owned copies, not a borrow-checker workaround:
        // `item.fields` lets a task/list item resolve its own metadata
        // (`ListItem::fields`), while the returned copy feeds the caller's
        // document-order stream every page-level query already relies on. Both
        // outlive this function inside different serialized structs, so neither
        // can borrow from the other.
        let mut item_fields: IndexMap<FieldKey, Vec<NoteFieldValue>> =
            IndexMap::new();
        let mut page_fields: IndexMap<FieldKey, Vec<NoteFieldValue>> =
            IndexMap::new();
        for (key, value) in raw_fields {
            // ponytail: clone needed for two-out pattern (item + page fields)
            item_fields.entry(key.clone()).or_default().push(value.clone());
            page_fields.entry(key).or_default().push(value);
        }
        item.fields = item_fields;
        let tags = lexer.extract_tags(&text);
        item.tags.extend(tags.iter().cloned());
        Some((page_fields, tags))
    }

    /// Pushes a list frame and flushes any active parent item's scan buffer.
    ///
    /// Returns the flushed inline fields and tags, if any.
    pub(super) fn start_list(&mut self, is_ordered: bool) -> FlushedFields {
        let flushed = self.flush_active_item_scan_buffer();
        self.list_stack.push(ListFrame {
            is_ordered,
            items: Vec::new(),
        });
        flushed
    }

    /// Closes the innermost list.
    ///
    /// A list nested inside an active item is stored under
    /// [`ListItem::children`]. Otherwise, it becomes a top-level completed
    /// list.
    pub(super) fn end_list(&mut self) {
        if let Some(frame) = self.list_stack.pop() {
            let list = List::new(frame.is_ordered, frame.items);
            if let Some(item) = self.item_stack.last_mut() {
                item.children.push(list);
            } else {
                self.lists.push(list);
            }
        }
    }

    /// Starts tracking a new list item at `line`.
    ///
    /// `depth` is the number of currently open lists (0-indexed); `parent` is
    /// the innermost active item's line, if this item is nested inside another
    /// item's child list.
    pub(super) fn start_item(&mut self, line: SourceLine) {
        let depth = u8::try_from(self.list_stack.len().saturating_sub(1))
            .unwrap_or(u8::MAX);
        let parent = self.item_stack.last().map(|item| item.position.line());
        self.item_stack.push(ItemFrame {
            classification: ItemClassificationState::Pending,
            text_buffer: String::new(),
            scan_buffer: String::new(),
            fields: IndexMap::new(),
            tags: Vec::new(),
            children: Vec::new(),
            position: ListItemPosition::new(line, depth, parent),
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
    ) -> FlushedFields {
        let flushed = self.flush_active_item_scan_buffer();
        if let Some(item_frame) = self.item_stack.pop() {
            let item_type = match item_frame.classification {
                ItemClassificationState::Marked(symbol) => {
                    let status = statuses.resolve(symbol);
                    if tag_filters.is_empty()
                        || item_frame
                            .tags
                            .iter()
                            .any(|tag| tag_filters.contains(tag))
                    {
                        ListItemType::Task(status)
                    } else {
                        ListItemType::Checkbox
                    }
                }
                ItemClassificationState::Plain
                | ItemClassificationState::Pending => ListItemType::Plain,
            };
            let item = ListItem::with_children(
                item_frame.text_buffer,
                item_type,
                item_frame.children,
            )
            .with_fields(item_frame.fields)
            .with_position(item_frame.position);
            if let Some(list_frame) = self.list_stack.last_mut() {
                list_frame.items.push(item);
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

/// The incremental classification state of an active list item during parsing.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum ItemClassificationState {
    /// The item's first chunks may still assemble into a marker; `text_buffer`
    /// holds the candidate bytes so far.
    Pending,
    /// The item has no task marker (plain bullet / regular list item).
    Plain,
    /// A task marker was recognized, carrying its marker symbol character.
    Marked(char),
}

impl ItemClassificationState {
    /// Returns `true` if a task marker was detected on this item.
    #[inline]
    #[must_use]
    const fn is_marked(self) -> bool {
        matches!(self, Self::Marked(_))
    }

    /// Returns the detected marker symbol, if any.
    #[inline]
    #[must_use]
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "kept for ItemClassificationState accessor symmetry; \
                      tested in unit suite"
        )
    )]
    const fn symbol(self) -> Option<char> {
        match self {
            Self::Marked(symbol) => Some(symbol),
            Self::Pending | Self::Plain => None,
        }
    }
}

/// An active list item frame on the parser stack.
struct ItemFrame {
    text_buffer: String,
    /// Mirrors `text_buffer` but excludes code text.
    ///
    /// [`ListTracker::flush_active_item_scan_buffer`] lexes this buffer for
    /// inline fields and tags.
    scan_buffer: String,
    /// Inline fields lexed from this item's own text.
    ///
    /// Kept separate from child items' fields so [`ListItem::fields`] resolves
    /// per-item, not per-list.
    fields: IndexMap<FieldKey, Vec<NoteFieldValue>>,
    /// Tags scanned from this item's own text, used to classify a
    /// status-marked item as [`ListItemType::Task`] or
    /// [`ListItemType::Checkbox`] against configured tag filters.
    tags: Vec<Tag>,
    children: Vec<List>,
    /// This item's source position: line, nesting depth, and parent line.
    position: ListItemPosition,
    /// Classification decision state for the list item. Mirrors
    /// pulldown-cmark's first-pass gating: the marker is only valid at the
    /// item's content start, so the decision is finalized before any inline
    /// content event or block boundary.
    classification: ItemClassificationState,
}

impl ItemFrame {
    /// Appends text to display text and, outside code blocks, the scan buffer.
    /// While classification is [`ItemClassificationState::Pending`], the chunk
    /// first feeds the incremental marker scan: Markdown splits a leading
    /// `[<char>]` marker across several `Event::Text` runs (observed: `"["`,
    /// `"x"`, `"]"`, `" Task"`), so each chunk extends the candidate and
    /// re-classifies it. A recognized marker is trimmed from both buffers.
    fn push_text(&mut self, text: &str, in_code_block: bool) {
        let pending =
            matches!(self.classification, ItemClassificationState::Pending);
        self.text_buffer.push_str(text);
        if !in_code_block {
            self.scan_buffer.push_str(text);
        }
        if pending {
            match scan_marker_prefix(&self.text_buffer) {
                MarkerPrefix::Incomplete => {}
                MarkerPrefix::Rejected => {
                    self.classification = ItemClassificationState::Plain;
                }
                MarkerPrefix::Complete(scan) => {
                    let symbol = scan.symbol();
                    let prefix_len = self
                        .text_buffer
                        .len()
                        .saturating_sub(scan.remainder().len());
                    self.trim_marker_prefix(prefix_len);
                    self.classification =
                        ItemClassificationState::Marked(symbol);
                }
            }
        }
    }

    /// Appends a line break to both the display text and scan buffer.
    ///
    /// A pending marker is decided first: the break terminates the marker's
    /// trailing-whitespace slot (`- [x]` wrapped over two lines still carries
    /// a marker).
    fn push_break(&mut self) {
        self.decide_pending_at_line_end();
        self.text_buffer.push('\n');
        self.scan_buffer.push('\n');
    }

    /// Rejects a pending marker: inline content (emphasis, code, links, images,
    /// inline HTML) occupies the item's leading slot, so the item does not
    /// start with a marker.
    fn reject_marker(&mut self) {
        if let ItemClassificationState::Pending = self.classification {
            self.classification = ItemClassificationState::Plain;
        }
    }

    /// Force-decides a pending marker as if the item's first line ended: a
    /// complete `[<char>]` shape becomes a marker with empty text.
    fn resolve_pending_marker(&mut self) {
        self.decide_pending_at_line_end();
    }

    /// Decides a pending marker using end-of-line semantics. Always leaves the
    /// classification non-pending.
    fn decide_pending_at_line_end(&mut self) {
        if let ItemClassificationState::Pending = self.classification {
            let decision = match scan_marker_at_line_end(&self.text_buffer) {
                Some(scan) => {
                    let symbol = scan.symbol();
                    let prefix_len = self.text_buffer.len();
                    self.trim_marker_prefix(prefix_len);
                    ItemClassificationState::Marked(symbol)
                }
                None => ItemClassificationState::Plain,
            };
            self.classification = decision;
        }
    }

    /// Drains the first `prefix_len` bytes from `text_buffer`, and from
    /// `scan_buffer` only when it mirrors those bytes (inline code and link
    /// brackets make the buffers diverge, in which case the scan buffer keeps
    /// its own content).
    fn trim_marker_prefix(&mut self, prefix_len: usize) {
        if self.scan_buffer.as_bytes().get(..prefix_len)
            == self.text_buffer.as_bytes().get(..prefix_len)
        {
            self.scan_buffer.drain(..prefix_len);
        }
        self.text_buffer.drain(..prefix_len);
    }

    /// Pushes a literal character into the scan buffer only.
    ///
    /// Used to reconstruct Markdown link brackets for visible-key inline field
    /// scanning.
    fn push_scan_char(&mut self, ch: char) {
        self.scan_buffer.push(ch);
    }

    /// Appends inline code text to display text only.
    ///
    /// Inline code is excluded from inline field and tag scanning.
    fn push_code(&mut self, text: &str) {
        self.text_buffer.push_str(text);
    }
}

/// An active list frame on the parser stack.
struct ListFrame {
    is_ordered: bool,
    items: Vec<ListItem>,
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use pretty_assertions::assert_eq;
    use rstest::rstest;

    use super::*;
    use crate::{
        TaskStatusType,
        note::{
            Note,
            parser::{MarkdownParserInput, parse_markdown},
        },
    };
    fn parse(src: &str) -> Note {
        let input = MarkdownParserInput::for_test(Path::new("note.md"), src);
        parse_markdown(&input)
    }
    #[test]
    fn item_classification_state_accessors() {
        assert!(!ItemClassificationState::Pending.is_marked());
        assert_eq!(ItemClassificationState::Pending.symbol(), None);

        assert!(!ItemClassificationState::Plain.is_marked());
        assert_eq!(ItemClassificationState::Plain.symbol(), None);

        assert!(ItemClassificationState::Marked('x').is_marked());
        assert_eq!(ItemClassificationState::Marked('x').symbol(), Some('x'));
    }
    #[test]
    fn is_item_active_returns_true_when_stack_nonempty() {
        let mut tracker = ListTracker::default();
        assert!(!tracker.is_item_active());

        tracker.start_list(false);
        tracker.start_item(SourceLine::new(1));

        assert!(
            tracker.is_item_active(),
            "is_item_active must return true after start_item"
        );
    }

    #[test]
    fn inline_code_pushes_to_last_item_not_first() {
        let mut tracker = ListTracker::default();
        tracker.start_list(false);
        tracker.start_item(SourceLine::new(1));
        tracker.push_text("before ", false);
        tracker.start_item(SourceLine::new(2));

        tracker.inline_code("code");

        tracker.end_item(&[], &TaskStatusMap::default());
        tracker.end_item(&[], &TaskStatusMap::default());
        tracker.end_list();

        // end_item pops LIFO: item2 is index 0, item1 is index 1
        let list = tracker.lists.first().expect("list must exist");
        let items = list.items();
        let item1_text = items.get(1).expect("item 1 must exist").text();
        let item2_text = items.first().expect("item 2 must exist").text();
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
    fn start_nested_text_block_returns_false_when_no_item() {
        let mut tracker = ListTracker::default();
        assert!(
            !tracker.start_nested_text_block(),
            "start_nested_text_block must return false with no item"
        );
    }

    #[test]
    fn start_nested_text_block_returns_true_with_active_item() {
        let mut tracker = ListTracker::default();
        tracker.start_list(false);
        tracker.start_item(SourceLine::new(1));
        assert!(
            tracker.start_nested_text_block(),
            "start_nested_text_block must return true with active item"
        );
    }

    #[test]
    fn start_list_flushes_active_item_scan_buffer() {
        let mut tracker = ListTracker::default();
        tracker.start_list(false);
        tracker.start_item(SourceLine::new(1));
        tracker.push_text("Status:: Draft", false);

        let flushed = tracker.start_list(false);

        assert!(
            flushed.is_some(),
            "start_list must flush active item scan buffer"
        );
        let (fields, _) = flushed.unwrap();
        let has_status = fields.keys().any(|k| k.is_canonical_match("status"));
        assert!(has_status, "flushed fields must contain Status");
    }

    #[test]
    fn end_item_flushes_scan_buffer() {
        let mut tracker = ListTracker::default();
        tracker.start_list(false);
        tracker.start_item(SourceLine::new(1));
        tracker.push_text("Author:: Jane", false);

        let flushed = tracker.end_item(&[], &TaskStatusMap::default());
        assert!(flushed.is_some(), "end_item must flush scan buffer");
        let (fields, _) = flushed.unwrap();
        let has_author = fields.keys().any(|k| k.is_canonical_match("author"));
        assert!(has_author, "flushed fields must contain Author");
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

    #[test]
    fn iterates_top_level_task_items() {
        let input = "- [ ] Task 1\n- Plain item\n- [x] Task 2";
        let note = parse(input);

        let tasks: Vec<&ListItem> = note.tasks().collect();
        assert_eq!(tasks.len(), 2);
        assert_eq!(tasks.first().map(|t| t.text()), Some("Task 1"));
        assert_eq!(tasks.get(1).map(|t| t.text()), Some("Task 2"));
    }

    #[test]
    #[expect(clippy::panic, reason = "test assertion on enum variant")]
    fn iterates_nested_sub_list_task_items() {
        let input = "- Plain parent\n  - [x] Subtask 1";
        let note = parse(input);

        let tasks: Vec<&ListItem> = note.tasks().collect();
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks.first().map(|t| t.text()), Some("Subtask 1"));
        let ListItemType::Task(status) = tasks.first().unwrap().kind() else {
            panic!("subtask must be a Task");
        };
        assert_eq!(status.kind().completed(), Some(true));
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
        assert_eq!(tasks.first().map(|t| t.text()), Some("Task text"));

        let item = tasks.first().expect("task present");
        assert!(
            matches!(item.kind(), ListItemType::Task(status) if
                status.kind() == expected_kind),
            "marker {symbol:?} must resolve to {expected_kind:?}, got {:?}",
            item.kind()
        );
    }

    #[test]
    #[expect(clippy::panic, reason = "test assertion on enum variant")]
    fn preserves_and_classifies_an_unknown_marker_as_an_incomplete_task() {
        let note = parse("- [?] Mystery task");

        let list = note.lists().first().expect("list present");
        let item = list.items().first().expect("item present");
        assert_eq!(item.text(), "Mystery task");
        let ListItemType::Task(status) = item.kind() else {
            panic!(
                "unknown marker must never be downgraded to a plain bullet, \
                 got {:?}",
                item.kind()
            );
        };
        assert_eq!(
            status.kind().completed(),
            Some(false),
            "unknown markers resolve as incomplete todos"
        );
    }

    #[test]
    fn does_not_treat_bracket_text_in_the_item_body_as_a_marker() {
        let note = parse("- Check [x] later");

        let list = note.lists().first().expect("list present");
        let item = list.items().first().expect("item present");
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

        let list = note.lists().first().expect("list present");
        let item = list.items().first().expect("item present");
        assert_eq!(item.text(), "");
        assert_eq!(note.tasks().count(), 1);
    }

    #[test]
    fn resolves_a_pending_marker_before_a_nested_list_flush() {
        // `- [x]` + nested list: no whitespace text chunk arrives before
        // the child list starts, but the parent still carries a marker.
        let note = parse("- [x]\n  - sub");

        let list = note.lists().first().expect("list present");
        let item = list.items().first().expect("item present");
        assert_eq!(item.text(), "");
        assert_eq!(note.tasks().count(), 1);
    }

    #[test]
    fn classifies_a_marker_before_a_soft_break_as_a_task() {
        let note = parse("- [x]\n  continued");

        let tasks: Vec<&ListItem> = note.tasks().collect();
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks.first().map(|t| t.text()), Some("\ncontinued"));
    }

    #[test]
    fn keeps_an_item_starting_with_inline_markup_plain() {
        // The emphasis opens the item's content, so `[x]` is not at the
        // item-leading position and must not become a marker.
        let note = parse("- **[x] Task**");

        let list = note.lists().first().expect("list present");
        let item = list.items().first().expect("item present");
        assert_eq!(item.text(), "[x] Task");
        assert_eq!(item.kind(), &ListItemType::Plain);
        assert_eq!(note.tasks().count(), 0);
    }

    #[test]
    fn keeps_an_item_starting_with_inline_code_plain() {
        let note = parse("- `[x]` Task");

        let list = note.lists().first().expect("list present");
        let item = list.items().first().expect("item present");
        assert_eq!(item.text(), "[x] Task");
        assert_eq!(item.kind(), &ListItemType::Plain);
        assert_eq!(note.tasks().count(), 0);
    }

    #[test]
    fn keeps_a_link_lookalike_plain() {
        // `- [x](y)` is a link whose text abuts the closing bracket —
        // no whitespace after `]`, so no marker.
        let note = parse("- [x](y) z");

        let list = note.lists().first().expect("list present");
        let item = list.items().first().expect("item present");
        assert_eq!(item.text(), "x z");
        assert_eq!(item.kind(), &ListItemType::Plain);
        assert_eq!(note.tasks().count(), 0);
    }

    #[test]
    fn rejects_unicode_whitespace_after_the_marker() {
        // NBSP is ordinary text in Markdown, not the marker's trailing
        // whitespace (ASCII whitespace only, mirroring pulldown-cmark).
        let note = parse("- [x]\u{00A0}Task");

        let list = note.lists().first().expect("list present");
        let item = list.items().first().expect("item present");
        assert_eq!(item.text(), "[x]\u{00A0}Task");
        assert_eq!(item.kind(), &ListItemType::Plain);
        assert_eq!(note.tasks().count(), 0);
    }

    #[test]
    fn classifies_a_multibyte_symbol_marker_as_an_incomplete_task() {
        let note = parse("- [β] Task");

        let tasks: Vec<&ListItem> = note.tasks().collect();
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks.first().map(|t| t.text()), Some("Task"));
        let item = tasks.first().expect("task present");
        assert!(matches!(
            item.kind(),
            ListItemType::Task(status) if status.kind().completed() == Some(false)
        ));
    }

    mod tag_filters {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn classifies_marked_item_as_task_when_tag_filters_are_empty() {
            let mut tracker = ListTracker::default();
            tracker.start_list(false);
            tracker.start_item(SourceLine::new(1));
            tracker.push_text("[x] Task without tag", false);
            tracker.end_item(&[], &TaskStatusMap::default());
            tracker.end_list();

            let list = tracker.lists.first().expect("list present");
            let item = list.items().first().expect("item present");
            assert!(matches!(item.kind(), ListItemType::Task(_)));
        }

        #[test]
        fn classifies_marked_item_as_task_when_tag_matches_filter() {
            let mut tracker = ListTracker::default();
            tracker.start_list(false);
            tracker.start_item(SourceLine::new(1));
            tracker.push_text("[x] Task with tag #task", false);
            let tag_filters = [Tag::parse("#task").unwrap()];
            tracker.end_item(&tag_filters, &TaskStatusMap::default());
            tracker.end_list();

            let list = tracker.lists.first().expect("list present");
            let item = list.items().first().expect("item present");
            assert!(matches!(item.kind(), ListItemType::Task(_)));
        }

        #[test]
        fn classifies_marked_item_as_checkbox_when_tag_does_not_match_filter() {
            let mut tracker = ListTracker::default();
            tracker.start_list(false);
            tracker.start_item(SourceLine::new(1));
            tracker.push_text("[x] Checkbox with different tag #other", false);
            let tag_filters = [Tag::parse("#task").unwrap()];
            tracker.end_item(&tag_filters, &TaskStatusMap::default());
            tracker.end_list();

            let list = tracker.lists.first().expect("list present");
            let item = list.items().first().expect("item present");
            assert_eq!(item.kind(), &ListItemType::Checkbox);
        }

        #[test]
        fn classifies_marked_item_as_checkbox_when_item_has_no_tags_and_filter_is_non_empty()
         {
            let mut tracker = ListTracker::default();
            tracker.start_list(false);
            tracker.start_item(SourceLine::new(1));
            tracker.push_text("[x] Checkbox without tags", false);
            let tag_filters = [Tag::parse("#task").unwrap()];
            tracker.end_item(&tag_filters, &TaskStatusMap::default());
            tracker.end_list();

            let list = tracker.lists.first().expect("list present");
            let item = list.items().first().expect("item present");
            assert_eq!(item.kind(), &ListItemType::Checkbox);
        }

        #[test]
        fn classifies_marked_item_as_task_when_one_of_multiple_tags_matches_filter()
         {
            let mut tracker = ListTracker::default();
            tracker.start_list(false);
            tracker.start_item(SourceLine::new(1));
            tracker.push_text(
                "[x] Task with multiple tags #other #task #work",
                false,
            );
            let tag_filters = [Tag::parse("#task").unwrap()];
            tracker.end_item(&tag_filters, &TaskStatusMap::default());
            tracker.end_list();

            let list = tracker.lists.first().expect("list present");
            let item = list.items().first().expect("item present");
            assert!(matches!(item.kind(), ListItemType::Task(_)));
        }

        #[test]
        fn classifies_marked_item_as_task_when_tag_matches_one_of_multiple_filters()
         {
            let mut tracker = ListTracker::default();
            tracker.start_list(false);
            tracker.start_item(SourceLine::new(1));
            tracker.push_text("[x] Task matching second filter #todo", false);
            let tag_filters =
                [Tag::parse("#task").unwrap(), Tag::parse("#todo").unwrap()];
            tracker.end_item(&tag_filters, &TaskStatusMap::default());
            tracker.end_list();

            let list = tracker.lists.first().expect("list present");
            let item = list.items().first().expect("item present");
            assert!(matches!(item.kind(), ListItemType::Task(_)));
        }

        #[test]
        fn rejects_prefix_match_for_exact_nested_tag() {
            let mut tracker = ListTracker::default();
            tracker.start_list(false);
            tracker.start_item(SourceLine::new(1));
            tracker
                .push_text("[x] Checkbox with nested tag #task/project", false);
            let tag_filters = [Tag::parse("#task").unwrap()];
            tracker.end_item(&tag_filters, &TaskStatusMap::default());
            tracker.end_list();

            let list = tracker.lists.first().expect("list present");
            let item = list.items().first().expect("item present");
            assert_eq!(item.kind(), &ListItemType::Checkbox);
        }

        #[test]
        fn accepts_exact_nested_tag_match() {
            let mut tracker = ListTracker::default();
            tracker.start_list(false);
            tracker.start_item(SourceLine::new(1));
            tracker.push_text("[x] Task with nested tag #task/project", false);
            let tag_filters = [Tag::parse("#task/project").unwrap()];
            tracker.end_item(&tag_filters, &TaskStatusMap::default());
            tracker.end_list();

            let list = tracker.lists.first().expect("list present");
            let item = list.items().first().expect("item present");
            assert!(matches!(item.kind(), ListItemType::Task(_)));
        }

        #[test]
        fn keeps_unmarked_item_plain_even_with_matching_tag() {
            let mut tracker = ListTracker::default();
            tracker.start_list(false);
            tracker.start_item(SourceLine::new(1));
            tracker.push_text("Plain item with tag #task", false);
            let tag_filters = [Tag::parse("#task").unwrap()];
            tracker.end_item(&tag_filters, &TaskStatusMap::default());
            tracker.end_list();

            let list = tracker.lists.first().expect("list present");
            let item = list.items().first().expect("item present");
            assert_eq!(item.kind(), &ListItemType::Plain);
        }
    }
}
