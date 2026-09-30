//! Task date, priority, inline task field, and text cleaning logic.

use indexmap::IndexMap;

use super::lexer::{FieldForm, ItemToken};
use crate::{
    DateValue, FieldKey, FieldKeyRef, Spanned, Tag, TaskDate, TaskDateSet,
    TaskDateType, TaskPriority, note::NoteFieldValue,
};

/// Extracts task lifecycle dates from emoji shorthands and inline task fields.
///
/// Emoji dates take precedence over inline fields. When duplicate dates appear
/// for the same lifecycle slot, first-wins semantics apply.
pub(super) fn extract_task_dates(
    tokens: &[Spanned<ItemToken>],
    fields: &IndexMap<FieldKey, Vec<NoteFieldValue>>,
) -> TaskDateSet {
    let mut set = TaskDateSet::default();

    // 1. Emoji dates (first-wins per slot)
    for token in tokens {
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
            if let Some(date) = first_date_in_field(fields, key_name) {
                set.insert(TaskDate::new(kind, date));
                break;
            }
        }
    }

    set
}

/// Returns the first date value matching `key_name` across `fields`.
///
/// [`FieldKeyRef`] resolves the canonical entry in O(1); keys in `fields` are
/// unique under canonical equality, so at most one entry matches.
fn first_date_in_field(
    fields: &IndexMap<FieldKey, Vec<NoteFieldValue>>,
    key_name: &str,
) -> Option<DateValue> {
    fields
        .get(&FieldKeyRef::new(key_name))?
        .iter()
        .find_map(|val| val.as_date().map(Into::into))
}

/// Extracts task priority from text emojis or an inline `[priority:: <level>]`
/// field.
///
/// Priority emojis take precedence over inline fields. When multiple priority
/// emojis are present, the first one in document order wins. Returns [`None`]
/// if no priority is specified or if priority resolves to
/// [`TaskPriority::Normal`].
pub(super) fn extract_task_priority(
    tokens: &[Spanned<ItemToken>],
    fields: &IndexMap<FieldKey, Vec<NoteFieldValue>>,
) -> Option<TaskPriority> {
    for token in tokens {
        if let ItemToken::Priority(priority) = token.value() {
            return Some(*priority);
        }
    }

    let value =
        fields.get(&FieldKeyRef::new("priority"))?.iter().find_map(|val| {
            val.as_str().and_then(|s| s.parse::<TaskPriority>().ok())
        })?;
    if matches!(value, TaskPriority::Normal) {
        return None;
    }
    Some(value)
}

/// Computes normalized clean list text by stripping configured task tag
/// filters, date syntax, priority emojis, and inline task fields.
///
/// Expects `raw_text` to already have any leading task marker prefix removed.
pub(super) fn clean_task_text(
    raw_text: &str,
    tokens: &[Spanned<ItemToken>],
    tag_filters: &[Tag],
) -> String {
    let mut remove_spans: Vec<(usize, usize)> =
        Vec::with_capacity(tokens.len());

    for token in tokens {
        let span = token.span();
        match token.value() {
            ItemToken::Priority(_) | ItemToken::Date(_) => {
                remove_spans.push((span.start, span.end));
            }
            ItemToken::Tag(tag) => {
                if tag_filters.contains(tag) {
                    remove_spans.push((span.start, span.end));
                }
            }
            ItemToken::Field((key, _, form)) => {
                if matches!(form, FieldForm::Wrapped)
                    && TaskDateType::is_field_key(key.canonical())
                {
                    remove_spans.push((span.start, span.end));
                }
            }
            ItemToken::Ignored => {}
        }
    }

    // Token spans from a single logos pass are disjoint and ordered, so no
    // merge step is required; `current_idx.max(end)` tolerates adjacency.
    let mut cleaned = String::with_capacity(raw_text.len());
    let mut current_idx = 0;
    for (start, end) in remove_spans {
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
    use crate::note::parser::lexer::{TaskShorthands, tokenize_item_text};

    /// Tokenizes `text` the way `ListTracker::end_item` does before extraction.
    fn tokens(text: &str) -> Vec<Spanned<ItemToken>> {
        tokenize_item_text(text, TaskShorthands::Include)
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
            let set = extract_task_dates(&tokens(text), &fields);
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
            let set = extract_task_dates(&tokens(text), &fields);
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
            let priority = extract_task_priority(&tokens(text), &fields);
            assert_eq!(priority, Some(TaskPriority::Highest));
        }

        #[test]
        fn gives_precedence_to_emoji_over_inline_field() {
            let text = "Task 🔺";
            let mut fields = IndexMap::new();
            fields.insert(FieldKey::try_from("priority").unwrap(), vec![
                NoteFieldValue::String("low".to_owned()),
            ]);

            let priority = extract_task_priority(&tokens(text), &fields);

            assert_eq!(priority, Some(TaskPriority::Highest));
        }

        #[test]
        fn collapses_normal_field_to_none() {
            let text = "Task";
            let mut fields = IndexMap::new();
            fields.insert(FieldKey::try_from("priority").unwrap(), vec![
                NoteFieldValue::String("normal".to_owned()),
            ]);
            let priority = extract_task_priority(&tokens(text), &fields);
            assert_eq!(priority, None);
        }
    }

    mod clean_task_text {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn strips_dates_priorities_tags_and_inline_fields() {
            let filters = [Tag::parse("#task").unwrap()];
            let raw = "Task #task 🔺 📅 2025-01-01 [custom:: value] \
                       [created:: 2024-12-01]";
            let cleaned = clean_task_text(raw, &tokens(raw), &filters);
            assert_eq!(cleaned, "Task [custom:: value]");
        }

        #[test]
        fn preserves_tags_outside_the_filter_set() {
            let filters = [Tag::parse("#task").unwrap()];
            let raw = "Task #other 🔺";
            let cleaned = clean_task_text(raw, &tokens(raw), &filters);
            assert_eq!(cleaned, "Task #other");
        }

        #[test]
        fn preserves_unparsed_invalid_dates() {
            let raw = "Task 📅 2026-13-45 keep";
            let cleaned = clean_task_text(raw, &tokens(raw), &[]);
            assert_eq!(cleaned, "Task 📅 2026-13-45 keep");
        }

        #[test]
        fn strips_priority_emoji_with_variation_selector() {
            let raw = "Task 🔺\u{FE0F} remaining text";
            let cleaned = clean_task_text(raw, &tokens(raw), &[]);
            assert_eq!(cleaned, "Task remaining text");
        }

        #[test]
        fn strips_inline_fields_with_nested_delimiters_and_wikilinks() {
            let raw = "Task [due:: [[2025-01-15]]] and (scheduled:: \
                       2025-01-01 (tentative)) remaining";
            let cleaned = clean_task_text(raw, &tokens(raw), &[]);
            assert_eq!(cleaned, "Task and remaining");
        }

        #[test]
        fn strips_inline_fields_with_quoted_bracket_content() {
            let raw = r#"Task [due:: "meeting [sync]"] remaining"#;
            let cleaned = clean_task_text(raw, &tokens(raw), &[]);
            assert_eq!(cleaned, "Task remaining");
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
}
