//! Task date, priority, inline task field, and text cleaning logic.

use std::ops::Range;

use indexmap::IndexMap;

use crate::{
    DateValue, FieldKey, Tag, TaskDate, TaskDateSet, TaskDateType,
    TaskPriority, note::NoteFieldValue,
};

/// Scans `text` starting at byte offset `from` for `emoji` followed by an ISO
/// date.
///
/// Returns the byte range spanning the emoji, optional variation selector,
/// whitespace, and the 10-byte ISO date string, paired with the parsed
/// [`DateValue`].
pub(super) fn scan_date_after(
    text: &str,
    from: usize,
    emoji: &str,
) -> Option<(Range<usize>, DateValue)> {
    let sub = text.get(from..)?;
    let pos = sub.find(emoji)?;
    let match_start = from.saturating_add(pos);
    let emoji_end = match_start.saturating_add(emoji.len());
    let after_emoji = text.get(emoji_end..)?;
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
            && DateValue::is_iso_shape(candidate)
            && let Ok(date) = DateValue::parse_iso(candidate)
        {
            let span_end = emoji_end
                .saturating_add(var_len)
                .saturating_add(ws_len)
                .saturating_add(10);
            return Some((match_start..span_end, date));
        }
    }
    scan_date_after(text, emoji_end.saturating_add(var_len), emoji)
}

/// Extracts task lifecycle dates from emoji shorthands and inline task fields.
///
/// Emoji dates take precedence over inline fields. When duplicate dates appear
/// for the same lifecycle slot, first-wins semantics apply.
pub(super) fn extract_task_dates(
    text: &str,
    fields: &IndexMap<FieldKey, Vec<NoteFieldValue>>,
) -> TaskDateSet {
    let mut set = TaskDateSet::default();

    // 1. Emoji dates (first-wins per slot)
    for &(base_emoji, kind) in TaskDateType::EMOJIS {
        let mut from = 0;
        while from < text.len() {
            if let Some((range, date)) = scan_date_after(text, from, base_emoji)
            {
                set.insert(TaskDate::new(kind, date));
                from = range.end;
            } else {
                break;
            }
        }
    }

    // 2. Inline fields fallback
    for kind in TaskDateType::ALL {
        if set.get(kind).is_some() {
            continue;
        }
        for key_name in kind.field_keys() {
            if let Some(date) = first_date_in_field(fields, key_name) {
                set.insert(TaskDate::new(kind, date));
                break;
            }
        }
    }

    set
}

/// Returns the first date value matching `key_name` across `fields`.
fn first_date_in_field(
    fields: &IndexMap<FieldKey, Vec<NoteFieldValue>>,
    key_name: &str,
) -> Option<DateValue> {
    for (key, values) in fields {
        if !key.is_canonical_match(key_name) {
            continue;
        }
        for val in values {
            if let Some(date) = val.as_date() {
                return Some(date.into());
            }
        }
    }
    None
}

/// Extracts task priority from text emojis or an inline `[priority:: <level>]`
/// field.
///
/// Priority emojis take precedence over inline fields. When multiple priority
/// emojis are present, the first one in document order wins. Returns [`None`]
/// if no priority is specified or if priority resolves to
/// [`TaskPriority::Normal`].
pub(super) fn extract_task_priority(
    text: &str,
    fields: &IndexMap<FieldKey, Vec<NoteFieldValue>>,
) -> Option<TaskPriority> {
    let mut first_priority = None;
    let mut first_pos = usize::MAX;

    for &(emoji, priority) in TaskPriority::EMOJIS {
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
                return if matches!(p, TaskPriority::Normal) {
                    None
                } else {
                    Some(p)
                };
            }
        }
    }

    None
}

/// Scans `text` for date shorthand emojis and records their removal byte spans.
fn find_date_emoji_spans(text: &str, spans: &mut Vec<(usize, usize)>) {
    for &(emoji, _) in TaskDateType::EMOJIS {
        let mut from = 0;
        while from < text.len() {
            if let Some((range, _)) = scan_date_after(text, from, emoji) {
                from = range.end;
                spans.push((range.start, range.end));
            } else {
                break;
            }
        }
    }
}

/// Scans `text` for priority emojis and records their removal byte spans.
fn find_priority_emoji_spans(text: &str, spans: &mut Vec<(usize, usize)>) {
    for &(emoji, _) in TaskPriority::EMOJIS {
        let mut search_from = 0;
        while let Some(pos) =
            text.get(search_from..).and_then(|t| t.find(emoji))
        {
            let match_start = search_from.saturating_add(pos);
            let mut match_end = match_start.saturating_add(emoji.len());
            if text
                .get(match_end..)
                .is_some_and(|tail| tail.starts_with('\u{FE0F}'))
            {
                match_end = match_end.saturating_add('\u{FE0F}'.len_utf8());
            }
            spans.push((match_start, match_end));
            search_from = match_end;
        }
    }
}

/// Computes normalized clean list text by stripping task marker prefix,
/// configured task tag filters, date syntax, priority emojis, and inline task
/// fields.
pub(super) fn clean_task_text(raw_text: &str, tag_filters: &[Tag]) -> String {
    let mut remove_spans: Vec<(usize, usize)> = Vec::new();

    // 1. Tag filters (only if configured)
    if !tag_filters.is_empty() {
        super::tag::find_tag_filter_spans(
            raw_text,
            tag_filters,
            &mut remove_spans,
        );
    }

    // 2. Date syntax (emoji dates)
    find_date_emoji_spans(raw_text, &mut remove_spans);

    // 3. Priority emojis
    find_priority_emoji_spans(raw_text, &mut remove_spans);

    // 4. Inline task fields: [field:: value] or (field:: value)
    find_inline_task_field_spans(raw_text, &mut remove_spans);

    if remove_spans.is_empty() {
        return normalize_whitespace(raw_text);
    }

    // Sort spans by start offset
    remove_spans.sort_unstable_by_key(|&(start, _)| start);

    // Merge overlapping spans
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

/// Scans `text` for Dataview-style inline task fields and records their removal
/// byte spans.
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

/// Scans a single bracketed or parenthesized inline task field starting at
/// `match_start`.
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

/// Returns `true` if `key` matches a recognized task metadata field name.
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

/// Collapses consecutive whitespace in `text` while preserving newlines.
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

#[cfg(test)]
mod tests {
    use super::*;

    mod scan_date_after {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn parses_iso_date_with_and_without_variation_selector() {
            let with_vs = "Task 📅\u{FE0F} 2025-01-15";
            let without_vs = "Task 📅 2025-01-15";
            let expected_date = DateValue::parse_iso("2025-01-15").unwrap();

            let (range1, d1) =
                scan_date_after(with_vs, 0, "\u{1F4C5}").unwrap();
            assert_eq!(d1, expected_date);
            assert_eq!(&with_vs[range1], "📅\u{FE0F} 2025-01-15");

            let (range2, d2) =
                scan_date_after(without_vs, 0, "\u{1F4C5}").unwrap();
            assert_eq!(d2, expected_date);
            assert_eq!(&without_vs[range2], "📅 2025-01-15");
        }

        #[test]
        fn skips_invalid_date_continuing_to_valid_match() {
            let text = "Task 📅 2026-13-45 then 📅 2025-01-15";
            let (range, date) = scan_date_after(text, 0, "\u{1F4C5}").unwrap();
            assert_eq!(date, DateValue::parse_iso("2025-01-15").unwrap());
            assert_eq!(&text[range], "📅 2025-01-15");
        }

        #[test]
        fn rejects_alphanumeric_date_terminator() {
            let text = "Task 📅 2025-01-15T12:00:00";
            assert_eq!(scan_date_after(text, 0, "\u{1F4C5}"), None);
        }
    }

    mod extract_task_dates {
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
            let set = extract_task_dates(text, &fields);
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
            let set = extract_task_dates(text, &fields);
            assert_eq!(
                set.get(TaskDateType::Done),
                Some(DateValue::parse_iso("2025-01-01").unwrap())
            );
        }
    }

    mod extract_task_priority {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn selects_first_in_document_order() {
            let text = "Task 🔺 and 🔽";
            let fields = IndexMap::new();
            let priority = extract_task_priority(text, &fields);
            assert_eq!(priority, Some(TaskPriority::Highest));
        }

        #[test]
        fn collapses_normal_field_to_none() {
            let text = "Task";
            let mut fields = IndexMap::new();
            fields.insert(FieldKey::try_from("priority").unwrap(), vec![
                NoteFieldValue::String("normal".to_owned()),
            ]);
            let priority = extract_task_priority(text, &fields);
            assert_eq!(priority, None);
        }
    }

    mod clean_task_text {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn strips_dates_priorities_tags_and_inline_fields() {
            let filters = [Tag::parse("#task").unwrap()];
            let cleaned = clean_task_text(
                "Task #task 🔺 📅 2025-01-01 [custom:: value] [created:: \
                 2024-12-01]",
                &filters,
            );
            assert_eq!(cleaned, "Task [custom:: value]");
        }

        #[test]
        fn preserves_unparsed_invalid_dates() {
            let cleaned = clean_task_text("Task 📅 2026-13-45 keep", &[]);
            assert_eq!(cleaned, "Task 📅 2026-13-45 keep");
        }
    }
}
