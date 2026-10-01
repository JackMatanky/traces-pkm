//! Single-pass [`ItemToken`] tokenizer for list item text.
//!
//! One logos pass yields inline fields, task date shorthands, priority emojis,
//! and tags, together with the callbacks and look-behind helpers they share.

use logos::{Filter, Lexer, Logos};

use super::inline::parse_inline_value;
use crate::{
    DateValue, DelimiterType, FieldKey, Spanned, Tag, TaskDate, TaskDateType,
    TaskPriority, note::NoteFieldValue,
};

/// Extracts inline fields from `text` in document order.
///
/// Converts task emoji shorthands into date fields when `mode` is
/// [`TaskFieldEmojis::Include`]. Tags are ignored during field scanning.
#[must_use]
pub(super) fn scan_fields(
    text: &str,
    mode: TaskFieldEmojis,
) -> Vec<(FieldKey, NoteFieldValue)> {
    let mut fields = Vec::new();
    let mut lexer = ItemToken::lexer_with_extras(text, mode);
    while let Some(Ok(token)) = lexer.next() {
        match token {
            ItemToken::Field((key, value, _)) => fields.push((key, value)),
            ItemToken::Date(date) => {
                if let Ok(key) = FieldKey::try_new(date.kind().as_str()) {
                    fields.push((key, NoteFieldValue::Date(date.date())));
                }
            }
            ItemToken::Tag(_) | ItemToken::Priority(_) => {}
        }
    }
    fields
}

/// Tokenizes `text` into spanned item tokens in a single pass.
pub(super) fn tokenize_item_text(
    text: &str,
    mode: TaskFieldEmojis,
) -> Vec<Spanned<ItemToken>> {
    let mut lexer = ItemToken::lexer_with_extras(text, mode);
    let mut tokens = Vec::new();
    // The skip rule matches all bytes; logos yields only valid tokens.
    while let Some(Ok(token)) = lexer.next() {
        let span = lexer.span();
        tokens.push(Spanned::from_usize_range(token, span));
    }
    tokens
}

/// Returns the character immediately before the current match.
///
/// Returns `None` if the match starts at the beginning of the source. Shared by
/// [`body_field_callback`] and [`tag_callback`], both of which need a
/// look-behind check that logos' regex dialect cannot express.
fn char_before<'source, T>(lex: &Lexer<'source, T>) -> Option<char>
where
    T: Logos<'source, Source = str>,
{
    let prefix = lex.source().get(..lex.span().start)?;
    let last_byte = *prefix.as_bytes().last()?;
    if last_byte.is_ascii() {
        return Some(char::from(last_byte));
    }
    prefix.chars().next_back()
}

/// Byte length of an ISO `YYYY-MM-DD` date, such as `2026-01-01`.
const ISO_DATE_LEN: usize = 10;

/// Switch controlling whether task field emoji shorthands are recognized.
///
/// Used as [`ItemToken`]'s logos `extras` value so [`scan_fields`] chooses its
/// lexer behavior without passing a bare `bool`.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub(super) enum TaskFieldEmojis {
    /// Recognizes task field emoji shorthands.
    Include,
    /// Ignores task field emoji shorthands.
    #[default]
    Exclude,
}

impl TaskFieldEmojis {
    /// Returns `true` if task field emoji shorthands are recognized.
    #[inline]
    #[must_use]
    const fn is_included(self) -> bool {
        matches!(self, Self::Include)
    }
}

/// Syntactic form of a scanned inline field token.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum FieldForm {
    /// `Key:: Value` occupying the start of a line.
    Bare,
    /// `[Key:: Value]` or `(Key:: Value)` embedded in running text.
    Wrapped,
}

/// Token stream for item components in free-form Markdown text.
#[derive(Debug, PartialEq, Logos)]
#[logos(extras = TaskFieldEmojis, skip(r"[\s\S]", priority = 0))]
pub(super) enum ItemToken {
    #[token("\u{1F53A}\u{FE0F}", |_| TaskPriority::Highest)]
    #[token("\u{1F53A}", |_| TaskPriority::Highest)]
    #[token("\u{23EB}\u{FE0F}", |_| TaskPriority::High)]
    #[token("\u{23EB}", |_| TaskPriority::High)]
    #[token("\u{1F53C}\u{FE0F}", |_| TaskPriority::Medium)]
    #[token("\u{1F53C}", |_| TaskPriority::Medium)]
    #[token("\u{1F53D}\u{FE0F}", |_| TaskPriority::Low)]
    #[token("\u{1F53D}", |_| TaskPriority::Low)]
    #[token("\u{23EC}\u{FE0F}", |_| TaskPriority::Lowest)]
    #[token("\u{23EC}", |_| TaskPriority::Lowest)]
    /// Task priority indicator emoji.
    Priority(TaskPriority),

    #[token("#", tag_callback)]
    /// Markdown hashtag (`#tag`).
    Tag(Tag),

    #[token("\u{1F4C5}\u{FE0F}", |lex| task_date_callback(lex, TaskDateType::Due))]
    #[token("\u{1F4C5}", |lex| task_date_callback(lex, TaskDateType::Due))]
    #[token("\u{1F5D3}\u{FE0F}", |lex| task_date_callback(lex, TaskDateType::Due))]
    #[token("\u{1F5D3}", |lex| task_date_callback(lex, TaskDateType::Due))]
    #[token("\u{2795}\u{FE0F}", |lex| task_date_callback(lex, TaskDateType::Created))]
    #[token("\u{2795}", |lex| task_date_callback(lex, TaskDateType::Created))]
    #[token("\u{1F6EB}\u{FE0F}", |lex| task_date_callback(lex, TaskDateType::Start))]
    #[token("\u{1F6EB}", |lex| task_date_callback(lex, TaskDateType::Start))]
    #[token("\u{23F3}\u{FE0F}", |lex| task_date_callback(lex, TaskDateType::Scheduled))]
    #[token("\u{23F3}", |lex| task_date_callback(lex, TaskDateType::Scheduled))]
    #[token("\u{2705}\u{FE0F}", |lex| task_date_callback(lex, TaskDateType::Done))]
    #[token("\u{2705}", |lex| task_date_callback(lex, TaskDateType::Done))]
    #[token("\u{274C}\u{FE0F}", |lex| task_date_callback(lex, TaskDateType::Cancelled))]
    #[token("\u{274C}", |lex| task_date_callback(lex, TaskDateType::Cancelled))]
    /// Task lifecycle date shorthand emoji with associated ISO date.
    Date(TaskDate),

    #[regex(r"[ \t]*[A-Za-z][A-Za-z0-9_-]*::", body_field_callback)]
    #[token("[", |lex| wrapped_field_callback(lex, DelimiterType::Bracket))]
    #[token("(", |lex| wrapped_field_callback(lex, DelimiterType::Parenthesis))]
    /// Inline key-value field (`Key:: Value`, `[Key:: Value]`, `(Key::
    /// Value)`).
    Field((FieldKey, NoteFieldValue, FieldForm)),
}
/// Parses a bare inline field (`Key:: Value`) starting at line begin.
///
/// Rejects the match if it does not begin at the start of the source or
/// immediately following a newline. Consumes the rest of the line as the raw
/// value.
fn body_field_callback(
    lex: &mut Lexer<'_, ItemToken>,
) -> Filter<(FieldKey, NoteFieldValue, FieldForm)> {
    let at_line_start = char_before(lex).is_none_or(|ch| ch == '\n');
    if !at_line_start {
        return Filter::Skip;
    }
    let slice = lex.slice();
    let key_end = slice.len().saturating_sub(2);
    let key = slice.get(..key_end).unwrap_or_default().trim();
    let remainder = lex.remainder();
    let value_end = remainder.find('\n').unwrap_or(remainder.len());
    let value = remainder.get(..value_end).unwrap_or_default().trim();
    let Ok(key) = FieldKey::try_from(key) else {
        return Filter::Skip;
    };
    let field = (key, parse_inline_value(value), FieldForm::Bare);
    lex.bump(value_end);
    Filter::Emit(field)
}

/// Parses a wrapped inline field (`[Key:: Value]` or `(Key:: Value)`).
///
/// Rejects the match and skips the opening delimiter when:
/// - No `::` separator is found before the end of the text
/// - The extracted key is empty or contains bracket characters
/// - No matching closing delimiter is found
fn wrapped_field_callback(
    lex: &mut Lexer<'_, ItemToken>,
    kind: DelimiterType,
) -> Filter<(FieldKey, NoteFieldValue, FieldForm)> {
    let remainder = lex.remainder();
    let Some(sep) = remainder.find("::") else {
        return Filter::Skip;
    };
    let key = remainder.get(..sep).unwrap_or_default().trim();
    if key.is_empty()
        || key.chars().any(|ch| matches!(ch, '[' | ']' | '(' | ')'))
    {
        return Filter::Skip;
    }
    let after_sep = remainder.get(sep.saturating_add(2)..).unwrap_or_default();
    let Some(close) = kind.find_closing(after_sep) else {
        return Filter::Skip;
    };
    let Ok(key) = FieldKey::try_from(key) else {
        return Filter::Skip;
    };
    let value = after_sep.get(..close).unwrap_or_default().trim();
    let consumed = sep
        .saturating_add(2)
        .saturating_add(close)
        .saturating_add(kind.close_len());
    lex.bump(consumed);
    Filter::Emit((key, parse_inline_value(value), FieldForm::Wrapped))
}

/// Parses a task emoji shorthand into a [`TaskDate`].
///
/// Emits a [`TaskDate`] when followed by optional inline whitespace and a
/// 10-byte ISO date (`YYYY-MM-DD`). Skips when `lex.extras` is
/// [`TaskFieldEmojis::Exclude`].
fn task_date_callback(
    lex: &mut Lexer<'_, ItemToken>,
    kind: TaskDateType,
) -> Filter<TaskDate> {
    if !lex.extras.is_included() {
        return Filter::Skip;
    }
    let remainder = lex.remainder();
    let ws_end = remainder
        .char_indices()
        .find(|&(_, ch)| !matches!(ch, ' ' | '\t'))
        .map_or(remainder.len(), |(offset, _)| offset);
    let after_ws = remainder.get(ws_end..).unwrap_or_default();
    let Some(candidate) = after_ws.get(..ISO_DATE_LEN) else {
        return Filter::Skip;
    };
    if !DateValue::is_iso_shape(candidate) {
        return Filter::Skip;
    }
    let Ok(value) = DateValue::parse_iso(candidate) else {
        return Filter::Skip;
    };
    if after_ws
        .get(ISO_DATE_LEN..)
        .and_then(|tail| tail.chars().next())
        .is_some_and(char::is_alphanumeric)
    {
        return Filter::Skip;
    }
    lex.bump(ws_end.saturating_add(ISO_DATE_LEN));
    Filter::Emit(TaskDate::new(kind, value))
}

/// Parses a Markdown tag following the leading `#` character.
///
/// Rejects the tag when:
/// - Preceded by an alphanumeric character or underscore (`_`)
/// - The tag does not begin with an alphabetic character
pub(super) fn tag_callback<'source, T: Logos<'source, Source = str>>(
    lex: &mut Lexer<'source, T>,
) -> Filter<Tag> {
    let preceded_by_word_char =
        char_before(lex).is_some_and(|ch| ch.is_alphanumeric() || ch == '_');
    if preceded_by_word_char {
        return Filter::Skip;
    }
    let tag_start = lex.span().start;
    let Some(tail) = lex.source().get(tag_start..) else {
        return Filter::Skip;
    };
    let Some(tag_len) = Tag::prefix_len(tail) else {
        return Filter::Skip;
    };
    lex.bump(tag_len.saturating_sub('#'.len_utf8()));
    match Tag::parse(lex.slice()) {
        Ok(tag) => Filter::Emit(tag),
        Err(_) => Filter::Skip,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DurationValue;
    mod inline_fields {
        use pretty_assertions::assert_eq;
        use rstest::rstest;

        use super::*;
        use crate::note::{Link, LinkType, NoteFieldValue};

        fn extract_fields(input: &str) -> Vec<(FieldKey, NoteFieldValue)> {
            scan_fields(input, TaskFieldEmojis::Exclude)
        }
        #[rstest]
        #[case::body("Author:: Jane Doe", "Author", "Jane Doe")]
        #[case::visible_key(
            "See the [Status:: Draft] note.",
            "Status",
            "Draft"
        )]
        #[case::hidden_key("See the (Status:: Draft) note.", "Status", "Draft")]
        fn extracts_a_field_in_its_declared_form(
            #[case] input: &str,
            #[case] expected_key: &str,
            #[case] expected_value: &str,
        ) {
            let fields = extract_fields(input);

            assert_eq!(fields.len(), 1);
            assert_eq!(
                fields.first().map(|(k, _)| k.name()),
                Some(expected_key)
            );
            assert_eq!(
                fields.first().and_then(|(_, v)| v.as_str()),
                Some(expected_value)
            );
        }

        #[test]
        fn rejects_a_multi_word_bare_key() {
            let fields = extract_fields("This sentence has a :: but no key.");

            assert_eq!(fields.len(), 0);
        }

        #[rstest]
        #[case::visible_key("[Due Date:: 2024-01-01]", "Due Date")]
        #[case::hidden_key("(Due Date:: 2024-01-01)", "Due Date")]
        fn accepts_a_multi_word_key_when_delimiter_bounded(
            #[case] input: &str,
            #[case] expected_key: &str,
        ) {
            let fields = extract_fields(input);

            assert_eq!(
                fields.first().map(|(k, _)| k.name()),
                Some(expected_key)
            );
            assert_eq!(
                fields.first().map(|(_, v)| v),
                Some(&NoteFieldValue::Date(
                    DateValue::parse_iso("2024-01-01").expect("valid date")
                ))
            );
        }

        #[test]
        fn extracts_a_bare_field_from_each_line_of_a_multiline_buffer() {
            let fields = extract_fields("Status:: Draft\nAuthor:: Jane Doe");

            let keys: Vec<&str> =
                fields.iter().map(|(k, _)| k.name()).collect();
            assert_eq!(keys, ["Status", "Author"]);
        }

        #[test]
        fn trims_surrounding_whitespace_from_the_value() {
            let fields = extract_fields("Status::    Draft   ");

            assert_eq!(
                fields.first().and_then(|(_, v)| v.as_str()),
                Some("Draft")
            );
        }

        #[test]
        fn extracts_an_empty_value_when_nothing_follows_the_double_colon() {
            let fields = extract_fields("Status::");

            assert_eq!(
                fields.first().map(|(_, v)| v),
                Some(&NoteFieldValue::Null)
            );
        }

        #[rstest]
        #[case::true_value("flag:: true", NoteFieldValue::Bool(true))]
        #[case::false_value("flag:: false", NoteFieldValue::Bool(false))]
        #[case::number("score:: 4.5", NoteFieldValue::Number(4.5))]
        #[case::date(
            "due:: 2026-07-29",
            NoteFieldValue::Date(
                DateValue::parse_iso("2026-07-29").expect("valid date")
            )
        )]
        #[case::non_finite_number("score:: NaN", NoteFieldValue::String("NaN".to_owned()))]
        fn parses_inline_value_types(
            #[case] input: &str,
            #[case] expected: NoteFieldValue,
        ) {
            let fields = extract_fields(input);

            assert_eq!(fields.first().map(|(_, v)| v), Some(&expected));
        }

        #[test]
        fn parses_dataview_link_value() {
            let fields = extract_fields("[link:: [[test]]]");

            assert_eq!(
                fields.first().map(|(_, v)| v),
                Some(&NoteFieldValue::Link(Link::new(
                    "test",
                    "test",
                    LinkType::Wikilink
                )))
            );
        }

        #[test]
        fn parses_dataview_wikilink_value_with_commas_in_target() {
            let fields = extract_fields("[link:: [[yes, no, and maybe]]]");

            assert_eq!(
                fields.first().map(|(_, v)| v),
                Some(&NoteFieldValue::Link(Link::new(
                    "yes, no, and maybe",
                    "yes, no, and maybe",
                    LinkType::Wikilink
                )))
            );
        }

        #[test]
        fn preserves_dataview_html_link_values_as_text() {
            let fields = extract_fields(r#"[link:: <a href="Page">Value</a>]"#);

            assert_eq!(
                fields.first().and_then(|(_, v)| v.as_str()),
                Some(r#"<a href="Page">Value</a>"#)
            );
        }

        #[test]
        fn parses_dataview_embed_link_value() {
            let fields = extract_fields("[embed:: ![[hello]]]");
            let (_, value) = fields.first().expect("field present");
            assert!(matches!(
                value,
                NoteFieldValue::Link(link) if
                    link.target() == "hello" &&
                    link.text() == "hello" &&
                    link.kind() == LinkType::Wikilink &&
                    link.is_embedded()
            ));
        }

        #[rstest]
        #[case::trailing_comma(
            "[links:: [[test]],]",
            NoteFieldValue::List(
                vec![NoteFieldValue::Link(Link::new(
                    "test",
                    "test",
                    LinkType::Wikilink,
                ))]
                .into(),
            )
        )]
        #[case::links(
            "[links:: [[test]], [[test2]]]",
            NoteFieldValue::List(
                vec![
                    NoteFieldValue::Link(Link::new(
                        "test",
                        "test",
                        LinkType::Wikilink,
                    )),
                    NoteFieldValue::Link(Link::new(
                        "test2",
                        "test2",
                        LinkType::Wikilink,
                    )),
                ]
                .into(),
            )
        )]
        #[case::mixed_atoms(
            r#"[values:: 1, 2, 3, "hello"]"#,
            NoteFieldValue::List(
                vec![
                    NoteFieldValue::Number(1.0),
                    NoteFieldValue::Number(2.0),
                    NoteFieldValue::Number(3.0),
                    NoteFieldValue::String("hello".to_owned()),
                ]
                .into(),
            )
        )]
        fn parses_dataview_comma_lists(
            #[case] input: &str,
            #[case] expected: NoteFieldValue,
        ) {
            let fields = extract_fields(input);

            assert_eq!(fields.first().map(|(_, v)| v), Some(&expected));
        }

        #[test]
        fn parses_quoted_string_with_comma() {
            let fields = extract_fields(r#"[str:: "yes,"]"#);

            assert_eq!(
                fields.first().map(|(_, v)| v),
                Some(&NoteFieldValue::String("yes,".to_owned()))
            );
        }

        #[test]
        fn parses_quoted_string_with_escaped_quote() {
            let fields = extract_fields(r#"[str:: "yes, \"maybe\""]"#);

            assert_eq!(
                fields.first().map(|(_, v)| v),
                Some(&NoteFieldValue::String(r#"yes, "maybe""#.to_owned()))
            );
        }

        #[test]
        fn extracts_nested_bracket_value() {
            let fields = extract_fields("This is some text. [key:: [value]]");

            assert_eq!(fields.first().map(|(k, _)| k.name()), Some("key"));
            assert_eq!(
                fields.first().and_then(|(_, v)| v.as_str()),
                Some("[value]")
            );
        }

        #[test]
        fn accepts_punctuation_in_wrapped_keys() {
            let fields = extract_fields(r"Hello? [key! :: \[value]");

            assert_eq!(fields.first().map(|(k, _)| k.name()), Some("key!"));
            assert_eq!(
                fields.first().and_then(|(_, v)| v.as_str()),
                Some(r"\[value")
            );
        }

        #[test]
        fn drops_a_wrapped_field_whose_key_has_no_searchable_characters() {
            let fields = extract_fields("Hello [!!!:: value]");

            assert_eq!(fields, []);
        }

        #[test]
        fn keeps_escaped_closing_bracket_inside_visible_value() {
            let fields = extract_fields(r"Hello [key:: \] value]");

            assert_eq!(fields.first().map(|(k, _)| k.name()), Some("key"));
            assert_eq!(
                fields.first().and_then(|(_, v)| v.as_str()),
                Some(r"\] value")
            );
        }

        #[test]
        fn extracts_wrapped_field_after_large_leading_whitespace() {
            let fields = extract_fields("      - [ ] Huh! [p:: 1]");

            assert_eq!(fields.first().map(|(k, _)| k.name()), Some("p"));
            assert_eq!(
                fields.first().map(|(_, v)| v),
                Some(&NoteFieldValue::Number(1.0))
            );
        }

        #[rstest]
        #[case::full_unit("[duration:: 7 hours]", "7 hours")]
        #[case::abbreviated_unit("[duration:: 4hr]", "4hr")]
        #[case::adjacent_units("[duration:: 4h15m]", "4h15m")]
        #[case::comma_separated_units(
            "[duration:: 4 hours, 15 minutes]",
            "4 hours, 15 minutes"
        )]
        #[case::mixed_abbreviated_units(
            "[duration:: 4 yrs, 6 wks, 9 mins, 3 s]",
            "4 yrs, 6 wks, 9 mins, 3 s"
        )]
        fn parses_dataview_duration_value(
            #[case] input: &str,
            #[case] expected: &str,
        ) {
            let fields = extract_fields(input);

            let expected_dv =
                DurationValue::parse(expected).expect("valid duration");
            assert_eq!(
                fields.first().map(|(_, v)| v),
                Some(&NoteFieldValue::Duration(expected_dv))
            );
        }
        #[rstest]
        #[case::no_space("🗓️2026-07-30", "due", "2026-07-30")]
        #[case::single_space("🗓️ 2026-07-30", "due", "2026-07-30")]
        #[case::multiple_spaces("🗓️   2026-07-30", "due", "2026-07-30")]
        fn extracts_task_emoji_shorthands_with_optional_spaces(
            #[case] input: &str,
            #[case] expected_key: &str,
            #[case] expected_date: &str,
        ) {
            let fields = scan_fields(input, TaskFieldEmojis::Include);
            assert_eq!(
                fields.first().map(|(k, _)| k.name()),
                Some(expected_key)
            );
            assert_eq!(
                fields.first().map(|(_, v)| v),
                Some(&NoteFieldValue::Date(
                    DateValue::parse_iso(expected_date).expect("valid date")
                ))
            );
        }
        #[test]
        fn rejects_task_emoji_shorthand_with_alphanumeric_terminator() {
            // A datetime suffix is not a valid shorthand date, so no field is
            // emitted.
            let fields =
                scan_fields("🗓️ 2026-07-30T12:00", TaskFieldEmojis::Include);
            assert_eq!(fields, []);
        }
        #[rstest]
        #[case::multibyte_candidate_then_valid_date(
            "📅 你好你好 📅 2025-01-15",
            Some("2025-01-15")
        )]
        #[case::multibyte_candidate_only("📅 你好你好", None)]
        fn skips_multibyte_date_candidates_without_panicking(
            #[case] input: &str,
            #[case] expected: Option<&str>,
        ) {
            // Byte 10 of a multibyte candidate falls inside a character, so the
            // candidate must be skipped rather than sliced.
            let fields = scan_fields(input, TaskFieldEmojis::Include);

            let expected = expected.map(|date| {
                NoteFieldValue::Date(
                    DateValue::parse_iso(date).expect("valid date"),
                )
            });
            assert_eq!(fields.first().map(|(_, v)| v), expected.as_ref());
        }

        #[test]
        fn accepts_a_bare_key_preceded_by_leading_whitespace() {
            let fields = extract_fields("  Status:: Draft");

            assert_eq!(fields.first().map(|(k, _)| k.name()), Some("Status"));
        }

        #[test]
        fn orders_matches_by_position_across_forms() {
            let fields = extract_fields(
                "Status:: Draft\nSee [Reviewer:: Jane] and (Editor:: Sam).",
            );

            let keys: Vec<&str> =
                fields.iter().map(|(k, _)| k.name()).collect();
            assert_eq!(keys, ["Status", "Reviewer", "Editor"]);
        }

        #[test]
        fn body_field_value_swallows_a_nested_wrapped_field_look_alike() {
            let fields = extract_fields("Status:: Draft [Key:: Value]");

            assert_eq!(fields.len(), 1);
            assert_eq!(fields.first().map(|(k, _)| k.name()), Some("Status"));
            assert_eq!(
                fields.first().and_then(|(_, v)| v.as_str()),
                Some("Draft [Key:: Value]")
            );
        }
    }

    mod tokenize_item_text {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn extracts_item_tokens_in_single_pass() {
            let text = "Task 🔺 #task 📅 2025-01-15 [priority:: high] end";
            let tokens = tokenize_item_text(text, TaskFieldEmojis::Include);
            assert_eq!(tokens.len(), 4);
            assert_eq!(
                tokens.first().expect("token 0").value(),
                &ItemToken::Priority(TaskPriority::Highest)
            );
            assert_eq!(
                tokens.get(1).expect("token 1").value(),
                &ItemToken::Tag(Tag::parse("#task").unwrap())
            );
            assert_eq!(tokens.get(1).expect("token 1").span_usize(), 10..15);
            let date_val = DateValue::parse_iso("2025-01-15").unwrap();
            assert_eq!(
                tokens.get(2).expect("token 2").value(),
                &ItemToken::Date(TaskDate::new(TaskDateType::Due, date_val))
            );
            assert_eq!(tokens.get(2).expect("token 2").span_usize(), 16..31);
            let key = FieldKey::try_new("priority").unwrap();
            assert_eq!(
                tokens.get(3).expect("token 3").value(),
                &ItemToken::Field((
                    key,
                    NoteFieldValue::String("high".to_owned()),
                    FieldForm::Wrapped
                ))
            );
            assert_eq!(tokens.get(3).expect("token 3").span_usize(), 32..49);
        }
    }
}
