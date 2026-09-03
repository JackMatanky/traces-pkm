//! Nested list and list-item tracking, and item-leading task marker
//! classification.
//!
//! [`ListTracker`] holds explicit list and list-item stacks so nested Markdown
//! never recurses through the call stack. [`ItemFrame`] runs the incremental
//! [`ItemClassificationState`] state machine that determines an item's leading
//! marker and classification, gated like `pulldown-cmark`'s own first-pass
//! `scan_task_list_marker`: the marker is only valid at an item's content
//! start, so the decision is finalized before any inline content event or block
//! boundary. Status-marked items are evaluated against configured tag filters
//! to classify them into tasks or checkboxes.
use chrono::NaiveDate;
use indexmap::IndexMap;

use super::{
    FlushedFields,
    lexer::InlineTokenLexer,
    marker::{MarkerPrefix, scan_marker_at_line_end, scan_marker_prefix},
};
use crate::{
    FieldKey, SourceLine, Tag, TaskStatusMap,
    note::{
        List, ListItem, ListItemType, ListText, NoteFieldValue, TaskDates,
        TaskListItem, TaskPriority, lists::ListItemPosition,
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
            let clean =
                compute_clean_text(&item_frame.text_buffer, tag_filters);
            let text = ListText::new(item_frame.text_buffer, clean);
            let item_type = match item_frame.classification {
                ItemClassificationState::Marked(symbol) => {
                    let status = statuses.resolve(symbol);
                    if tag_filters.is_empty()
                        || item_frame
                            .tags
                            .iter()
                            .any(|tag| tag_filters.contains(tag))
                    {
                        let fully_complete =
                            is_descendant_tree_complete(&item_frame.children);
                        let priority = extract_task_priority(
                            text.raw(),
                            &item_frame.fields,
                        );
                        let dates =
                            extract_task_dates(text.raw(), &item_frame.fields);
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
                ItemClassificationState::Plain
                | ItemClassificationState::Pending => ListItemType::Plain,
            };
            let item =
                ListItem::with_children(text, item_type, item_frame.children)
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

/// Returns `true` if every descendant task under `children` is resolved
/// (done or cancelled), or if there are no descendant tasks.
///
/// Plain bullet items ([`ListItemType::Plain`]) and non-task checkboxes
/// ([`ListItemType::Checkbox`]) are ignored and do not block completion.
/// Short-circuits on the first incomplete task descendant.
fn is_descendant_tree_complete(children: &[List]) -> bool {
    for list in children {
        for item in list.items() {
            match item.kind() {
                ListItemType::Task(task) => {
                    if task.status().kind().completed() == Some(false)
                        || !task.is_fully_complete()
                    {
                        return false;
                    }
                }
                ListItemType::Plain | ListItemType::Checkbox => {
                    if !is_descendant_tree_complete(item.children()) {
                        return false;
                    }
                }
            }
        }
    }
    true
}

/// Extracts task priority from text emojis or an inline `[priority:: <level>]`
/// field.
///
/// Priority emojis take precedence over inline fields. When multiple priority
/// emojis are present, the first one in document order wins. Returns [`None`]
/// if no priority is specified.
fn extract_task_priority(
    text: &str,
    fields: &IndexMap<FieldKey, Vec<NoteFieldValue>>,
) -> Option<TaskPriority> {
    let mut first_priority = None;
    let mut first_pos = usize::MAX;

    for (emoji, priority) in [
        ("\u{1F53A}", TaskPriority::Highest),
        ("\u{23EB}", TaskPriority::High),
        ("\u{1F53C}", TaskPriority::Medium),
        ("\u{1F53D}", TaskPriority::Low),
        ("\u{23EC}", TaskPriority::Lowest),
    ] {
        if let Some(pos) = text.find(emoji)
            && pos < first_pos
        {
            first_pos = pos;
            first_priority = Some(priority);
        }
    }

    if let Some(priority) = first_priority {
        return Some(priority);
    }

    for (key, values) in fields {
        if !key.is_canonical_match("priority") {
            continue;
        }
        for val in values {
            let Some(s) = val.as_str() else {
                continue;
            };
            if let Ok(p) = s.parse::<TaskPriority>() {
                return Some(p);
            }
        }
    }

    None
}

/// Extracts task dates from text emoji syntax and inline field syntax.
///
/// Supported dates: created (`➕`), scheduled (`⏳`), start (`🛫`), due (`📅`),
/// done (`✅`), and cancelled (`❌`). When both emoji and inline field syntax
/// are present for the same date field, emoji syntax wins.
fn extract_task_dates(
    text: &str,
    fields: &IndexMap<FieldKey, Vec<NoteFieldValue>>,
) -> TaskDates {
    TaskDates::new(
        extract_single_date(text, fields, &["\u{2795}"], &["created"]),
        extract_single_date(text, fields, &["\u{23F3}"], &["scheduled"]),
        extract_single_date(text, fields, &["\u{1F6EB}"], &["start"]),
        extract_single_date(
            text,
            fields,
            &[
                "\u{1F4C5}\u{FE0F}",
                "\u{1F4C5}",
                "\u{1F5D3}\u{FE0F}",
                "\u{1F5D3}",
            ],
            &["due"],
        ),
        extract_single_date(text, fields, &["\u{2705}"], &[
            "done",
            "completion",
        ]),
        extract_single_date(text, fields, &["\u{274C}"], &["cancelled"]),
    )
}

/// Extracts a single date by first searching for any of `emojis` in `text`,
/// then falling back to checking `fields` for any of `field_keys`.
fn extract_single_date(
    text: &str,
    fields: &IndexMap<FieldKey, Vec<NoteFieldValue>>,
    emojis: &[&str],
    field_keys: &[&str],
) -> Option<NaiveDate> {
    for emoji in emojis {
        if let Some(date) = parse_emoji_date(text, emoji) {
            return Some(date);
        }
    }

    for key_name in field_keys {
        for (key, values) in fields {
            if !key.is_canonical_match(key_name) {
                continue;
            }
            for val in values {
                if let Some(date) = val.as_date() {
                    return Some(date);
                }
            }
        }
    }

    None
}

/// Parses an ISO date immediately following `emoji` (with optional whitespace).
fn parse_emoji_date(text: &str, emoji: &str) -> Option<NaiveDate> {
    let mut search_from = 0;
    while let Some(pos) = text.get(search_from..).and_then(|t| t.find(emoji)) {
        let match_start = search_from.saturating_add(pos);
        let emoji_end = match_start.saturating_add(emoji.len());
        let after_emoji = &text[emoji_end..];
        let var_len = if after_emoji.starts_with('\u{FE0F}') {
            '\u{FE0F}'.len_utf8()
        } else {
            0
        };
        let after_var = &after_emoji[var_len..];
        let ws_len = after_var
            .char_indices()
            .find(|&(_, c)| c != ' ' && c != '\t')
            .map_or(after_var.len(), |(offset, _)| offset);
        let after_ws = &after_var[ws_len..];
        if after_ws.len() >= 10 {
            let candidate = &after_ws[..10];
            let next_char_valid = after_ws[10..]
                .chars()
                .next()
                .is_none_or(|ch| !ch.is_alphanumeric());
            if next_char_valid
                && let Ok(date) =
                    NaiveDate::parse_from_str(candidate, "%Y-%m-%d")
            {
                return Some(date);
            }
        }
        search_from = emoji_end.saturating_add(var_len);
    }
    None
}

/// Computes normalized clean list text by stripping task marker prefix,
/// configured task tag filters, date syntax, priority emojis, and inline task
/// fields.
///
/// When `tag_filters` is empty, no tags are stripped.
fn compute_clean_text(raw_text: &str, tag_filters: &[Tag]) -> String {
    let mut remove_spans: Vec<(usize, usize)> = Vec::new();

    // 1. Tag filters (only if configured)
    if !tag_filters.is_empty() {
        find_tag_filter_spans(raw_text, tag_filters, &mut remove_spans);
    }

    // 2. Date syntax (emoji dates)
    find_emoji_date_spans(raw_text, &mut remove_spans);

    // 3. Priority emojis
    find_priority_emoji_spans(raw_text, &mut remove_spans);

    // 4. Inline task fields: [field:: value] or (field:: value)
    find_inline_task_field_spans(raw_text, &mut remove_spans);

    if remove_spans.is_empty() {
        return normalize_whitespace(raw_text);
    }

    // Merge overlapping/adjacent removal spans
    remove_spans.sort_unstable_by_key(|&(start, _)| start);
    let mut merged: Vec<(usize, usize)> =
        Vec::with_capacity(remove_spans.len());
    for (start, end) in remove_spans {
        if let Some(last) = merged.last_mut()
            && start <= last.1
        {
            last.1 = last.1.max(end);
            continue;
        }
        merged.push((start, end));
    }

    // Extract unremoved slices
    let mut cleaned = String::with_capacity(raw_text.len());
    let mut current_idx = 0;
    for (start, end) in merged {
        if start > current_idx
            && let Some(slice) = raw_text.get(current_idx..start)
        {
            cleaned.push_str(slice);
        }
        current_idx = current_idx.max(end);
    }
    if current_idx < raw_text.len()
        && let Some(slice) = raw_text.get(current_idx..)
    {
        cleaned.push_str(slice);
    }

    normalize_whitespace(&cleaned)
}

fn find_tag_filter_spans(
    text: &str,
    tag_filters: &[Tag],
    spans: &mut Vec<(usize, usize)>,
) {
    let mut iter = text.char_indices().peekable();
    let mut prev_char: Option<char> = None;

    while let Some((idx, ch)) = iter.next() {
        let is_word_char =
            prev_char.is_some_and(|c| c.is_alphanumeric() || c == '_');
        prev_char = Some(ch);
        if ch != '#' || is_word_char {
            continue;
        }
        if let Some((start, end, candidate)) =
            scan_tag_candidate(text, idx, &mut iter)
            && tag_filters.iter().any(|filter| filter.as_str() == candidate)
        {
            spans.push((start, end));
        }
    }
}

fn scan_tag_candidate<'a>(
    text: &'a str,
    start_idx: usize,
    iter: &mut std::iter::Peekable<std::str::CharIndices<'_>>,
) -> Option<(usize, usize, &'a str)> {
    let (_, next_ch) = iter.peek().copied()?;
    if !next_ch.is_alphabetic() {
        return None;
    }
    let mut tag_end = start_idx.saturating_add('#'.len_utf8());
    while let Some(&(cur_idx, cur_ch)) = iter.peek() {
        if !cur_ch.is_alphanumeric() && !matches!(cur_ch, '_' | '/' | '-') {
            break;
        }
        tag_end = cur_idx.saturating_add(cur_ch.len_utf8());
        iter.next();
    }
    let candidate = text.get(start_idx..tag_end)?;
    Some((start_idx, tag_end, candidate))
}

fn find_emoji_date_spans(text: &str, spans: &mut Vec<(usize, usize)>) {
    let date_emojis = [
        "\u{1F4C5}\u{FE0F}",
        "\u{1F4C5}",
        "\u{1F5D3}\u{FE0F}",
        "\u{1F5D3}",
        "\u{2795}",
        "\u{1F6EB}",
        "\u{23F3}",
        "\u{2705}",
        "\u{274C}",
    ];

    for emoji in date_emojis {
        let mut search_from = 0;
        while let Some(pos) =
            text.get(search_from..).and_then(|t| t.find(emoji))
        {
            let match_start = search_from.saturating_add(pos);
            let emoji_end = match_start.saturating_add(emoji.len());
            let after_emoji = &text[emoji_end..];
            let var_len = if after_emoji.starts_with('\u{FE0F}') {
                '\u{FE0F}'.len_utf8()
            } else {
                0
            };
            let after_var = &after_emoji[var_len..];
            let ws_len = after_var
                .char_indices()
                .find(|&(_, c)| c != ' ' && c != '\t')
                .map_or(after_var.len(), |(offset, _)| offset);
            let after_ws = &after_var[ws_len..];
            if after_ws.len() >= 10 {
                let candidate = &after_ws[..10];
                let next_char_valid = after_ws[10..]
                    .chars()
                    .next()
                    .is_none_or(|ch| !ch.is_alphanumeric());
                if next_char_valid
                    && NaiveDate::parse_from_str(candidate, "%Y-%m-%d").is_ok()
                {
                    let span_end = emoji_end
                        .saturating_add(var_len)
                        .saturating_add(ws_len)
                        .saturating_add(10);
                    spans.push((match_start, span_end));
                    search_from = span_end;
                    continue;
                }
            }
            search_from = emoji_end.saturating_add(var_len);
        }
    }
}

fn find_priority_emoji_spans(text: &str, spans: &mut Vec<(usize, usize)>) {
    let priority_emojis = [
        "\u{1F53A}\u{FE0F}",
        "\u{1F53A}",
        "\u{23EB}\u{FE0F}",
        "\u{23EB}",
        "\u{1F53C}\u{FE0F}",
        "\u{1F53C}",
        "\u{1F53D}\u{FE0F}",
        "\u{1F53D}",
        "\u{23EC}\u{FE0F}",
        "\u{23EC}",
    ];

    for emoji in priority_emojis {
        let mut search_from = 0;
        while let Some(pos) =
            text.get(search_from..).and_then(|t| t.find(emoji))
        {
            let match_start = search_from.saturating_add(pos);
            let match_end = match_start.saturating_add(emoji.len());
            spans.push((match_start, match_end));
            search_from = match_end;
        }
    }
}

fn find_inline_task_field_spans(text: &str, spans: &mut Vec<(usize, usize)>) {
    for (open_delim, close_delim) in [('[', ']'), ('(', ')')] {
        let mut search_from = 0;
        while let Some(open_pos) =
            text.get(search_from..).and_then(|t| t.find(open_delim))
        {
            let match_start = search_from.saturating_add(open_pos);
            let open_end = match_start.saturating_add(open_delim.len_utf8());
            let remainder = &text[open_end..];
            if let Some(match_end) = scan_inline_task_field(
                match_start,
                remainder,
                open_delim,
                close_delim,
            ) {
                spans.push((match_start, match_end));
                search_from = match_end;
            } else {
                search_from = open_end;
            }
        }
    }
}

fn scan_inline_task_field(
    match_start: usize,
    remainder: &str,
    open_delim: char,
    close_delim: char,
) -> Option<usize> {
    let sep_pos = remainder.find("::")?;
    let key = remainder.get(..sep_pos)?.trim();
    if key.is_empty()
        || key.chars().any(|ch| matches!(ch, '[' | ']' | '(' | ')'))
        || !is_task_field_key(key)
    {
        return None;
    }
    let after_sep = remainder.get(sep_pos.saturating_add(2)..)?;
    let close_pos = after_sep.find(close_delim)?;
    Some(
        match_start
            .saturating_add(open_delim.len_utf8())
            .saturating_add(sep_pos)
            .saturating_add(2)
            .saturating_add(close_pos)
            .saturating_add(close_delim.len_utf8()),
    )
}

fn is_task_field_key(key: &str) -> bool {
    matches!(
        key.trim().to_ascii_lowercase().as_str(),
        "created"
            | "start"
            | "scheduled"
            | "due"
            | "done"
            | "cancelled"
            | "priority"
            | "completion"
    )
}

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

    mod tracker_state {

        use super::*;
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
            let item1_text =
                items.get(1).expect("item 1 must exist").raw_text();
            let item2_text =
                items.first().expect("item 2 must exist").raw_text();
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
            let has_status =
                fields.keys().any(|k| k.is_canonical_match("status"));
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
            let has_author =
                fields.keys().any(|k| k.is_canonical_match("author"));
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
    }

    mod classification {
        use pretty_assertions::assert_eq;

        use super::*;
        #[test]
        fn item_classification_state_accessors() {
            assert!(!ItemClassificationState::Pending.is_marked());
            assert_eq!(ItemClassificationState::Pending.symbol(), None);

            assert!(!ItemClassificationState::Plain.is_marked());
            assert_eq!(ItemClassificationState::Plain.symbol(), None);

            assert!(ItemClassificationState::Marked('x').is_marked());
            assert_eq!(
                ItemClassificationState::Marked('x').symbol(),
                Some('x')
            );
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

            let list = note.lists().first().expect("list present");
            let item = list.items().first().expect("item present");
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

    #[expect(clippy::panic, reason = "test assertions on enum variants")]
    mod fully_complete {
        use pretty_assertions::assert_eq;

        use super::*;
        use crate::TaskConfig;

        #[test]
        fn returns_true_for_leaf_task_with_no_children() {
            let note = parse("- [ ] Lone task");

            let list = note.lists().first().expect("list present");
            let item = list.items().first().expect("item present");
            let ListItemType::Task(task) = item.kind() else {
                panic!("must be task");
            };
            assert_eq!(task.is_fully_complete(), true);
        }

        #[test]
        fn returns_true_when_all_child_tasks_are_done() {
            let note = parse("- [ ] Parent\n  - [x] Child 1\n  - [x] Child 2");

            let list = note.lists().first().expect("list present");
            let parent = list.items().first().expect("parent present");
            let ListItemType::Task(parent_task) = parent.kind() else {
                panic!("must be task");
            };
            assert_eq!(parent_task.is_fully_complete(), true);
        }

        #[test]
        fn returns_true_when_child_task_is_cancelled() {
            let note = parse("- [ ] Parent\n  - [-] Cancelled child");

            let list = note.lists().first().expect("list present");
            let parent = list.items().first().expect("parent present");
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

            let list = note.lists().first().expect("list present");
            let parent = list.items().first().expect("parent present");
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

            let list = note.lists().first().expect("list present");
            let parent = list.items().first().expect("parent present");
            let ListItemType::Task(parent_task) = parent.kind() else {
                panic!("must be task");
            };
            assert_eq!(parent_task.is_fully_complete(), false);
        }

        #[test]
        fn returns_false_when_child_task_is_in_progress() {
            let note = parse("- [x] Parent\n  - [/] In progress child");

            let list = note.lists().first().expect("list present");
            let parent = list.items().first().expect("parent present");
            let ListItemType::Task(parent_task) = parent.kind() else {
                panic!("must be task");
            };
            assert_eq!(parent_task.is_fully_complete(), false);
        }

        #[test]
        fn returns_true_when_only_plain_bullet_children_exist() {
            let note =
                parse("- [ ] Parent\n  - Plain child 1\n  - Plain child 2");

            let list = note.lists().first().expect("list present");
            let parent = list.items().first().expect("parent present");
            let ListItemType::Task(parent_task) = parent.kind() else {
                panic!("must be task");
            };
            assert_eq!(parent_task.is_fully_complete(), true);
        }

        #[test]
        fn returns_true_when_only_non_task_checkbox_children_exist() {
            let tasks =
                TaskConfig::for_test(vec![Tag::parse("#task").unwrap()]);
            let frontmatter = crate::config::FrontmatterConfig::default();
            let input = MarkdownParserInput::new(
                std::path::Path::new("note.md"),
                "- [ ] Parent #task\n  - [ ] Checkbox without tag",
                &tasks,
                &frontmatter,
            );
            let note = parse_markdown(&input);

            let list = note.lists().first().expect("list present");
            let parent = list.items().first().expect("parent present");
            let ListItemType::Task(parent_task) = parent.kind() else {
                panic!("must be task");
            };
            assert_eq!(parent_task.is_fully_complete(), true);
        }

        #[test]
        fn returns_true_when_three_levels_of_nested_tasks_are_all_done() {
            let note =
                parse("- [ ] Level 1\n  - [x] Level 2\n    - [x] Level 3");

            let list = note.lists().first().expect("list present");
            let l1 = list.items().first().expect("l1 present");
            let ListItemType::Task(t1) = l1.kind() else {
                panic!("must be task");
            };
            assert_eq!(t1.is_fully_complete(), true);

            let l2_list = l1.children().first().expect("l2 list present");
            let l2 = l2_list.items().first().expect("l2 item present");
            let ListItemType::Task(t2) = l2.kind() else {
                panic!("must be task");
            };
            assert_eq!(t2.is_fully_complete(), true);

            let l3_list = l2.children().first().expect("l3 list present");
            let l3 = l3_list.items().first().expect("l3 item present");
            let ListItemType::Task(t3) = l3.kind() else {
                panic!("must be task");
            };
            assert_eq!(t3.is_fully_complete(), true);
        }

        #[test]
        fn returns_false_when_grandchild_task_is_incomplete() {
            let note =
                parse("- [x] Level 1\n  - [x] Level 2\n    - [ ] Level 3");

            let list = note.lists().first().expect("list present");
            let l1 = list.items().first().expect("l1 present");
            let ListItemType::Task(t1) = l1.kind() else {
                panic!("must be task");
            };
            assert_eq!(t1.is_fully_complete(), false);

            let l2_list = l1.children().first().expect("l2 list present");
            let l2 = l2_list.items().first().expect("l2 item present");
            let ListItemType::Task(t2) = l2.kind() else {
                panic!("must be task");
            };
            assert_eq!(t2.is_fully_complete(), false);

            let l3_list = l2.children().first().expect("l3 list present");
            let l3 = l3_list.items().first().expect("l3 item present");
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

            let list = note.lists().first().expect("list present");
            let parent = list.items().first().expect("parent present");
            let ListItemType::Task(parent_task) = parent.kind() else {
                panic!("must be task");
            };
            assert_eq!(parent_task.is_fully_complete(), false);
        }

        #[test]
        fn returns_false_when_child_task_has_unknown_marker() {
            let note = parse("- [x] Parent\n  - [?] Unknown marker child");

            let list = note.lists().first().expect("list present");
            let parent = list.items().first().expect("parent present");
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
            let list = note.lists().first().expect("list present");
            let item = list.items().first().expect("item present");

            assert_eq!(item.text().raw(), "Buy milk 📅 2025-01-15 #task");
            assert_eq!(item.text().clean(), "Buy milk #task");
        }

        #[test]
        fn preserves_non_task_inline_fields_in_clean_text() {
            let note = parse("- [ ] Buy milk [store:: Costco] 📅 2025-01-15");
            let list = note.lists().first().expect("list present");
            let item = list.items().first().expect("item present");

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

    #[expect(clippy::panic, reason = "test assertion on enum variant")]
    fn expect_task(item: &ListItem) -> &TaskListItem {
        let ListItemType::Task(task) = item.kind() else {
            panic!("expected task item, got {:?}", item.kind());
        };
        task
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
            let tasks: Vec<&ListItem> = note.tasks().collect();
            let task_item = tasks.first().expect("task present");
            let task = expect_task(task_item);

            assert_eq!(task.priority(), Some(expected));
            assert_eq!(task_item.text().clean(), "Task");
        }

        #[test]
        fn returns_none_when_priority_emoji_is_missing() {
            let note = parse("- [ ] Plain task without priority");
            let tasks: Vec<&ListItem> = note.tasks().collect();
            let task_item = tasks.first().expect("task present");
            let task = expect_task(task_item);

            assert_eq!(task.priority(), None);
        }

        #[test]
        fn extracts_priority_from_inline_field_when_emoji_absent() {
            let note = parse("- [ ] Task [priority:: high]");
            let tasks: Vec<&ListItem> = note.tasks().collect();
            let task_item = tasks.first().expect("task present");
            let task = expect_task(task_item);

            assert_eq!(task.priority(), Some(TaskPriority::High));
            assert_eq!(task_item.text().clean(), "Task");
        }

        #[test]
        fn prefers_earliest_priority_emoji_when_multiple_present() {
            let note = parse("- [ ] Task ⏫ then 🔽 later");
            let tasks: Vec<&ListItem> = note.tasks().collect();
            let task_item = tasks.first().expect("task present");
            let task = expect_task(task_item);

            assert_eq!(task.priority(), Some(TaskPriority::High));
            assert_eq!(task_item.text().clean(), "Task then later");
        }
    }

    mod date_extraction {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn extracts_dates_from_emoji_syntax() {
            let input = "- [ ] Task ➕ 2025-01-01 🛫 2025-01-05 ⏳ 2025-01-10 \
                         📅 2025-01-15 ✅ 2025-01-20 ❌ 2025-01-25";
            let note = parse(input);
            let tasks: Vec<&ListItem> = note.tasks().collect();
            let task_item = tasks.first().expect("task present");
            let task = expect_task(task_item);
            let dates = task.dates();

            assert_eq!(dates.created, NaiveDate::from_ymd_opt(2025, 1, 1));
            assert_eq!(dates.start, NaiveDate::from_ymd_opt(2025, 1, 5));
            assert_eq!(dates.scheduled, NaiveDate::from_ymd_opt(2025, 1, 10));
            assert_eq!(dates.due, NaiveDate::from_ymd_opt(2025, 1, 15));
            assert_eq!(dates.done, NaiveDate::from_ymd_opt(2025, 1, 20));
            assert_eq!(dates.cancelled, NaiveDate::from_ymd_opt(2025, 1, 25));
            assert_eq!(task_item.text().clean(), "Task");
        }

        #[test]
        fn extracts_dates_from_emoji_syntax_with_variation_selectors() {
            let input = "- [ ] Task ➕\u{FE0F} 2025-01-01 🛫\u{FE0F} \
                         2025-01-05 ⏳\u{FE0F} 2025-01-10 📅\u{FE0F} \
                         2025-01-15 ✅\u{FE0F} 2025-01-20 ❌\u{FE0F} \
                         2025-01-25";
            let note = parse(input);
            let tasks: Vec<&ListItem> = note.tasks().collect();
            let task_item = tasks.first().expect("task present");
            let task = expect_task(task_item);
            let dates = task.dates();

            assert_eq!(dates.created, NaiveDate::from_ymd_opt(2025, 1, 1));
            assert_eq!(dates.start, NaiveDate::from_ymd_opt(2025, 1, 5));
            assert_eq!(dates.scheduled, NaiveDate::from_ymd_opt(2025, 1, 10));
            assert_eq!(dates.due, NaiveDate::from_ymd_opt(2025, 1, 15));
            assert_eq!(dates.done, NaiveDate::from_ymd_opt(2025, 1, 20));
            assert_eq!(dates.cancelled, NaiveDate::from_ymd_opt(2025, 1, 25));
            assert_eq!(task_item.text().clean(), "Task");
        }
        #[test]
        fn extracts_dates_from_inline_field_syntax() {
            let input = "- [ ] Task [created:: 2025-02-01] [start:: \
                         2025-02-05] [scheduled:: 2025-02-10] [due:: \
                         2025-02-15] [done:: 2025-02-20] [cancelled:: \
                         2025-02-25]";
            let note = parse(input);
            let tasks: Vec<&ListItem> = note.tasks().collect();
            let task_item = tasks.first().expect("task present");
            let task = expect_task(task_item);
            let dates = task.dates();

            assert_eq!(dates.created, NaiveDate::from_ymd_opt(2025, 2, 1));
            assert_eq!(dates.start, NaiveDate::from_ymd_opt(2025, 2, 5));
            assert_eq!(dates.scheduled, NaiveDate::from_ymd_opt(2025, 2, 10));
            assert_eq!(dates.due, NaiveDate::from_ymd_opt(2025, 2, 15));
            assert_eq!(dates.done, NaiveDate::from_ymd_opt(2025, 2, 20));
            assert_eq!(dates.cancelled, NaiveDate::from_ymd_opt(2025, 2, 25));
            assert_eq!(task_item.text().clean(), "Task");
        }

        #[test]
        fn prefers_emoji_date_over_inline_field_when_both_present() {
            let input = "- [ ] Task 📅 2025-03-01 [due:: 2025-03-15]";
            let note = parse(input);
            let tasks: Vec<&ListItem> = note.tasks().collect();
            let task_item = tasks.first().expect("task present");
            let task = expect_task(task_item);

            assert_eq!(task.dates().due, NaiveDate::from_ymd_opt(2025, 3, 1));
            assert_eq!(task_item.text().clean(), "Task");
        }

        #[test]
        fn returns_none_for_invalid_or_missing_date() {
            let input = "- [ ] Task 📅 2025-02-30 [start:: not-a-date]";
            let note = parse(input);
            let tasks: Vec<&ListItem> = note.tasks().collect();
            let task_item = tasks.first().expect("task present");
            let task = expect_task(task_item);

            assert_eq!(task.dates().due, None);
            assert_eq!(task.dates().start, None);
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
            let list = note.lists().first().expect("list present");
            let item = list.items().first().expect("item present");

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
        tracker.start_item(SourceLine::new(1));
        tracker.push_text(text, false);
        tracker.end_item(tag_filters, &TaskStatusMap::default());
        tracker.end_list();
        tracker
            .lists
            .into_iter()
            .next()
            .expect("list present")
            .items()
            .first()
            .cloned()
            .expect("item present")
    }
}
