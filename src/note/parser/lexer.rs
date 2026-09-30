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

/// Extracts inline fields from `text` in encounter order.
///
/// Date shorthand tokens contribute a field keyed by the date type's name when
/// `shorthands` includes them. Tags are extracted separately by
/// [`super::tag::scan_tags`], whose token set does not swallow field values.
#[inline]
#[must_use]
pub(super) fn scan_fields(
    text: &str,
    shorthands: TaskShorthands,
) -> Vec<(FieldKey, NoteFieldValue)> {
    let mut fields = Vec::new();
    for token in tokenize_item_text(text, shorthands) {
        match token.into_value() {
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
#[must_use]
pub(super) fn tokenize_item_text(
    text: &str,
    shorthands: TaskShorthands,
) -> Vec<Spanned<ItemToken>> {
    let mut lexer = ItemToken::lexer_with_extras(text, shorthands);
    let mut tokens = Vec::new();
    // The `skip(r"[\s\S]", priority = 0)` derive matches every byte, so the
    // lexer never yields `Err`; a `Some(Err(_))` here would mean the skip
    // rule was removed and would silently truncate the stream.
    while let Some(Ok(token)) = lexer.next() {
        let span = lexer.span();
        tokens.push(Spanned::new(token, span));
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
    lex.source()
        .get(..lex.span().start)
        .and_then(|prefix| prefix.chars().next_back())
}

/// Byte length of an ISO `YYYY-MM-DD` date, such as `2026-01-01`.
const ISO_DATE_LEN: usize = 10;

/// Field-token mode controlling whether task emoji shorthands are recognized.
///
/// Used as [`ItemToken`]'s logos `extras` value so [`scan_fields`]
/// chooses its lexer behavior without passing a bare `bool`.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub(super) enum TaskShorthands {
    /// Recognizes task emoji shorthands.
    Include,
    /// Ignores task emoji shorthands.
    #[default]
    Exclude,
}

impl TaskShorthands {
    /// Whether this mode recognizes task emoji shorthands.
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
#[logos(extras = TaskShorthands, skip(r"[\s\S]", priority = 0))]
pub(super) enum ItemToken {
    // 1. Priority emojis
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
    Priority(TaskPriority),

    // 2. Tags
    #[token("#", tag_callback)]
    Tag(Tag),

    // 3. Date emojis (when shorthands enabled); each emoji accepts an
    // optional VS16 so the variation selector never leaks into the value scan
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
    Date(TaskDate),

    // 4. Wrapped & bare fields
    #[regex(r"[ \t]*[A-Za-z][A-Za-z0-9_-]*::", body_field_callback)]
    #[token("[", |lex| wrapped_field_callback(lex, DelimiterType::Bracket))]
    #[token("(", |lex| wrapped_field_callback(lex, DelimiterType::Parenthesis))]
    Field((FieldKey, NoteFieldValue, FieldForm)),
}

/// Parses a bare inline field (`Key:: Value`) from the `Key::` prefix already
/// matched by [`ItemToken`]'s body-field pattern, consuming the rest of the
/// line as the raw value, equivalent to the regex:
/// `(?m)^[ \t]*key::[\t]*(.*)$`.
///
/// Logos has no look-behind support, so a line-start check replaces that
/// regex's `^` anchor. The match is rejected, skipping only the matched `Key::`
/// span rather than the rest of the line, unless it starts right after a
/// newline or at the start of the text.
fn body_field_callback(
    lex: &mut Lexer<'_, ItemToken>,
) -> Filter<(FieldKey, NoteFieldValue, FieldForm)> {
    let at_line_start = char_before(lex).is_none_or(|ch| ch == '\n');
    if !at_line_start {
        return Filter::Skip;
    }
    let slice = lex.slice();
    let key_end = slice.len().saturating_sub(2); // Strip the trailing "::".
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

/// Parses a wrapped inline field (`[Key:: Value]` or `(Key:: Value)`) starting
/// just after its already-consumed opening delimiter.
///
/// Rejects, skipping only the opening delimiter, when:
/// - there is no `::` separator before the text ends,
/// - the key is empty, contains a bracket character, or has an empty canonical
///   form (punctuation-only text), or
/// - no matching closing delimiter is found.
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
/// Starts after the already-consumed emoji token and emits a [`TaskDate`] of
/// kind `date_type` when the following text is optional inline whitespace plus
/// exactly [`ISO_DATE_LEN`] bytes forming a valid ISO date.
///
/// Always skips when `lex.extras` is [`TaskShorthands::Exclude`].
fn task_date_callback(
    lex: &mut Lexer<'_, ItemToken>,
    date_type: TaskDateType,
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
    Filter::Emit(TaskDate::new(date_type, value))
}

/// Parses a Markdown tag after its already-consumed leading `#`.
///
/// Shared by [`ItemToken`]'s tag token and the dedicated tag scanner in
/// [`super::tag`]. Rejects a mid-word `#`, such as `foo#bar`, and a `#` not
/// followed by an alphabetic character, such as `#1`.
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
            scan_fields(input, TaskShorthands::Exclude)
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
            let fields = scan_fields(input, TaskShorthands::Include);

            assert_eq!(fields.len(), 1);
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
                scan_fields("🗓️ 2026-07-30T12:00", TaskShorthands::Include);

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
            let fields = scan_fields(input, TaskShorthands::Include);

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
            let tokens = tokenize_item_text(text, TaskShorthands::Include);
            assert_eq!(tokens.len(), 4);
            assert_eq!(
                tokens.first().expect("token 0").value(),
                &ItemToken::Priority(TaskPriority::Highest)
            );
            assert_eq!(tokens.first().expect("token 0").span(), 5..9);
            assert_eq!(
                tokens.get(1).expect("token 1").value(),
                &ItemToken::Tag(Tag::parse("#task").unwrap())
            );
            assert_eq!(tokens.get(1).expect("token 1").span(), 10..15);
            let date_val = DateValue::parse_iso("2025-01-15").unwrap();
            assert_eq!(
                tokens.get(2).expect("token 2").value(),
                &ItemToken::Date(TaskDate::new(TaskDateType::Due, date_val))
            );
            assert_eq!(tokens.get(2).expect("token 2").span(), 16..31);
            let key = FieldKey::try_new("priority").unwrap();
            assert_eq!(
                tokens.get(3).expect("token 3").value(),
                &ItemToken::Field((
                    key,
                    NoteFieldValue::String("high".to_owned()),
                    FieldForm::Wrapped
                ))
            );
            assert_eq!(tokens.get(3).expect("token 3").span(), 32..49);
        }
    }
}
