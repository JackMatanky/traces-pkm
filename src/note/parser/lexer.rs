//! Scan plain-text buffers for inline fields and Markdown tags.
//!
//! Operates on text already filtered by the Markdown parser: fenced code
//! blocks, indented code blocks, and inline code spans are excluded before
//! [`InlineTokenLexer`] runs.
//!
//! [`InlineTokenLexer`] extracts:
//!
//! - inline fields: `Key:: Value`, `[Key:: Value]`, and `(Key:: Value)` body
//!   metadata, plus (when `has_marker` is `true`) task emoji shorthand fields
//!   such as `🗓️2026-01-01`.
//! - tags: Markdown tags such as `#book` and `#projects/active`, unconditional
//!   on `has_marker`.

use logos::{Filter, Lexer, Logos};

use super::inline::parse_inline_value;
use crate::{
    DelimiterType, FieldKey, find_closing_delimiter,
    note::{NoteFieldValue, metadata::is_iso_date},
    tag::Tag,
};

/// Extracts inline fields and tags from a parser scan buffer.
///
/// `has_marker` controls whether [`Self::extract_fields`] recognizes task
/// emoji shorthand fields (dates, priority); [`Self::extract_tags`] is
/// unconditional on it. Both methods return flat token lists in encounter
/// order — the caller aggregates them into an `IndexMap`.
#[derive(Copy, Clone, Debug)]
pub(super) struct InlineTokenLexer {
    has_marker: bool,
}

impl InlineTokenLexer {
    /// Creates a lexer. `has_marker` is `true` for status-marked list items.
    #[inline]
    #[must_use]
    pub(super) const fn new(has_marker: bool) -> Self {
        Self {
            has_marker,
        }
    }

    /// Extracts inline fields from `text` in encounter order.
    ///
    /// Recognizes `Key:: Value`, `[Key:: Value]`, and `(Key:: Value)`. When
    /// `has_marker` is `true`, also recognizes task emoji shorthand fields
    /// such as `🗓️2026-01-01`. `text` must already exclude code spans and
    /// blocks.
    #[inline]
    #[must_use]
    pub(super) fn extract_fields(
        self,
        text: &str,
    ) -> Vec<(FieldKey, NoteFieldValue)> {
        let shorthands = if self.has_marker {
            TaskShorthands::Include
        } else {
            TaskShorthands::Exclude
        };
        let lexer = FieldToken::lexer_with_extras(text, shorthands);
        let mut fields = Vec::new();
        for result in lexer {
            if let Ok(FieldToken::Field(field)) = result {
                fields.push(field);
            }
        }
        fields
    }

    /// Extracts Markdown tags from `text` in encounter order, unconditional on
    /// `has_marker`.
    ///
    /// Tags keep their leading `#`. Mid-word occurrences like `foo#bar` are
    /// rejected.
    #[inline]
    #[must_use]
    #[expect(
        clippy::unused_self,
        reason = "method for API symmetry with extract_fields; has_marker \
                  deliberately does not affect tag extraction"
    )]
    pub(super) fn extract_tags(self, text: &str) -> Vec<Tag> {
        let lexer = TagToken::lexer(text);
        let mut tags = Vec::new();
        for result in lexer {
            if let Ok(TagToken::Tag(tag)) = result {
                tags.push(tag);
            }
        }
        tags
    }
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
/// Used as [`FieldToken`]'s logos `extras` value so
/// [`InlineTokenLexer::extract_fields`] chooses its lexer behavior without
/// passing a bare `bool`.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
enum TaskShorthands {
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

/// Token stream for inline fields in free-form Markdown text.
///
/// - [`Self::Field`] carries an emitted `(FieldKey, NoteFieldValue)`.
/// - [`Self::Ignored`] skips ordinary prose that matches none of the field
///   patterns.
///
/// Callbacks return [`Filter::Skip`] to discard non-matching candidates, such
/// as unclosed wrapped fields, and keep scanning.
#[derive(Clone, Debug, PartialEq, Logos)]
#[logos(extras = TaskShorthands)]
enum FieldToken {
    #[regex(r"[ \t]*[A-Za-z][A-Za-z0-9_-]*::", body_field_callback)]
    #[token("[", |lex| wrapped_field_callback(lex, DelimiterType::Bracket))]
    #[token("(", |lex| wrapped_field_callback(lex, DelimiterType::Parenthesis))]
    #[token("\u{1F5D3}\u{FE0F}", |lex| task_field_callback(lex, "due"))]
    #[token("\u{1F5D3}", |lex| task_field_callback(lex, "due"))]
    #[token("\u{2795}", |lex| task_field_callback(lex, "created"))]
    #[token("\u{1F6EB}", |lex| task_field_callback(lex, "start"))]
    #[token("\u{23F3}", |lex| task_field_callback(lex, "scheduled"))]
    #[token("\u{2705}", |lex| task_field_callback(lex, "completion"))]
    Field((FieldKey, NoteFieldValue)),
    #[regex(r"[\s\S]", priority = 0)]
    Ignored,
}

/// Parses a bare inline field (`Key:: Value`) from the `Key::` prefix already
/// matched by [`FieldToken`]'s body-field pattern, consuming the rest of the
/// line as the raw value, equivalent to the regex:
/// `(?m)^[ \t]*key::[\t]*(.*)$`.
///
/// Logos has no look-behind support, so a line-start check replaces that
/// regex's `^` anchor. The match is rejected, skipping only the matched `Key::`
/// span rather than the rest of the line, unless it starts right after a
/// newline or at the start of the text.
fn body_field_callback(
    lex: &mut Lexer<'_, FieldToken>,
) -> Filter<(FieldKey, NoteFieldValue)> {
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
    let field = (key, parse_inline_value(value));
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
/// - [`find_closing_delimiter`] finds no matching closing delimiter.
fn wrapped_field_callback(
    lex: &mut Lexer<'_, FieldToken>,
    kind: DelimiterType,
) -> Filter<(FieldKey, NoteFieldValue)> {
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
    let Some(close) = find_closing_delimiter(after_sep, kind) else {
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
    Filter::Emit((key, parse_inline_value(value)))
}

/// Parses a task emoji shorthand into an inline field.
///
/// Starts after the already-consumed emoji token and emits a field keyed by
/// `key` when the following text is optional inline whitespace plus exactly
/// [`ISO_DATE_LEN`] bytes forming a valid ISO date.
///
/// Always skips when `lex.extras` is [`TaskShorthands::Exclude`].
fn task_field_callback(
    lex: &mut Lexer<'_, FieldToken>,
    key: &'static str,
) -> Filter<(FieldKey, NoteFieldValue)> {
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
    if !is_iso_date(candidate) {
        return Filter::Skip;
    }
    let Ok(key) = FieldKey::try_from(key) else {
        return Filter::Skip;
    };
    lex.bump(ws_end.saturating_add(ISO_DATE_LEN));
    Filter::Emit((key, NoteFieldValue::Date(candidate.to_owned())))
}

/// Token stream for Markdown tags in free-form text.
///
/// - [`Self::Tag`] carries an emitted [`Tag`].
/// - [`Self::Ignored`] skips ordinary text.
///
/// [`tag_callback`] returns [`Filter::Skip`] to reject non-tag `#` characters
/// without swallowing the rest of the text.
#[derive(Clone, Debug, PartialEq, Logos)]
enum TagToken {
    #[token("#", tag_callback)]
    Tag(Tag),
    #[regex(r"[\s\S]", priority = 0)]
    Ignored,
}

/// Parses a Markdown tag after its already-consumed leading `#`.
///
/// Rejects a mid-word `#`, such as `foo#bar`, and a `#` not followed by an
/// alphabetic character, such as `#1`.
fn tag_callback(lex: &mut Lexer<'_, TagToken>) -> Filter<Tag> {
    let preceded_by_word_char =
        char_before(lex).is_some_and(|ch| ch.is_alphanumeric() || ch == '_');
    if preceded_by_word_char {
        return Filter::Skip;
    }
    let remainder = lex.remainder();
    if !remainder.chars().next().is_some_and(char::is_alphabetic) {
        return Filter::Skip;
    }
    let body_end = remainder
        .char_indices()
        .find(|&(_, ch)| {
            !(ch.is_alphanumeric() || matches!(ch, '_' | '/' | '-'))
        })
        .map_or(remainder.len(), |(offset, _)| offset);
    lex.bump(body_end);
    match Tag::parse(lex.slice()) {
        Ok(tag) => Filter::Emit(tag),
        Err(_) => Filter::Skip,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    mod inline_fields {
        use pretty_assertions::assert_eq;
        use rstest::rstest;

        use super::*;
        use crate::note::{Link, LinkType, NoteFieldValue};

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
            let fields = InlineTokenLexer::new(false).extract_fields(input);

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
            let fields = InlineTokenLexer::new(false)
                .extract_fields("This sentence has a :: but no key.");

            assert_eq!(fields.len(), 0);
        }

        #[rstest]
        #[case::visible_key("[Due Date:: 2024-01-01]", "Due Date")]
        #[case::hidden_key("(Due Date:: 2024-01-01)", "Due Date")]
        fn accepts_a_multi_word_key_when_delimiter_bounded(
            #[case] input: &str,
            #[case] expected_key: &str,
        ) {
            let fields = InlineTokenLexer::new(false).extract_fields(input);

            assert_eq!(
                fields.first().map(|(k, _)| k.name()),
                Some(expected_key)
            );
            assert_eq!(
                fields.first().and_then(|(_, v)| v.as_str()),
                Some("2024-01-01")
            );
        }

        #[test]
        fn extracts_a_bare_field_from_each_line_of_a_multiline_buffer() {
            let fields = InlineTokenLexer::new(false)
                .extract_fields("Status:: Draft\nAuthor:: Jane Doe");

            let keys: Vec<&str> =
                fields.iter().map(|(k, _)| k.name()).collect();
            assert_eq!(keys, ["Status", "Author"]);
        }

        #[test]
        fn trims_surrounding_whitespace_from_the_value() {
            let fields = InlineTokenLexer::new(false)
                .extract_fields("Status::    Draft   ");

            assert_eq!(
                fields.first().and_then(|(_, v)| v.as_str()),
                Some("Draft")
            );
        }

        #[test]
        fn extracts_an_empty_value_when_nothing_follows_the_double_colon() {
            let fields =
                InlineTokenLexer::new(false).extract_fields("Status::");

            assert_eq!(
                fields.first().map(|(_, v)| v),
                Some(&NoteFieldValue::Null)
            );
        }

        #[rstest]
        #[case::true_value("flag:: true", NoteFieldValue::Bool(true))]
        #[case::false_value("flag:: false", NoteFieldValue::Bool(false))]
        #[case::number("score:: 4.5", NoteFieldValue::Number(4.5))]
        #[case::date("due:: 2026-07-29", NoteFieldValue::Date("2026-07-29".to_owned()))]
        #[case::non_finite_number("score:: NaN", NoteFieldValue::String("NaN".to_owned()))]
        fn parses_inline_value_types(
            #[case] input: &str,
            #[case] expected: NoteFieldValue,
        ) {
            let fields = InlineTokenLexer::new(false).extract_fields(input);

            assert_eq!(fields.first().map(|(_, v)| v), Some(&expected));
        }

        #[test]
        fn parses_dataview_link_value() {
            let fields = InlineTokenLexer::new(false)
                .extract_fields("[link:: [[test]]]");

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
            let fields = InlineTokenLexer::new(false)
                .extract_fields("[link:: [[yes, no, and maybe]]]");

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
            let fields = InlineTokenLexer::new(false)
                .extract_fields(r#"[link:: <a href="Page">Value</a>]"#);

            assert_eq!(
                fields.first().and_then(|(_, v)| v.as_str()),
                Some(r#"<a href="Page">Value</a>"#)
            );
        }

        #[test]
        fn parses_dataview_embed_link_value() {
            let fields = InlineTokenLexer::new(false)
                .extract_fields("[embed:: ![[hello]]]");
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
            NoteFieldValue::List(vec![NoteFieldValue::Link(Link::new(
                "test",
                "test",
                LinkType::Wikilink
            ))])
        )]
        #[case::links(
            "[links:: [[test]], [[test2]]]",
            NoteFieldValue::List(vec![
                NoteFieldValue::Link(Link::new(
                    "test",
                    "test",
                    LinkType::Wikilink
                )),
                NoteFieldValue::Link(Link::new(
                    "test2",
                    "test2",
                    LinkType::Wikilink
                )),
            ])
        )]
        #[case::mixed_atoms(
            r#"[values:: 1, 2, 3, "hello"]"#,
            NoteFieldValue::List(vec![
                NoteFieldValue::Number(1.0),
                NoteFieldValue::Number(2.0),
                NoteFieldValue::Number(3.0),
                NoteFieldValue::String("hello".to_owned()),
            ])
        )]
        fn parses_dataview_comma_lists(
            #[case] input: &str,
            #[case] expected: NoteFieldValue,
        ) {
            let fields = InlineTokenLexer::new(false).extract_fields(input);

            assert_eq!(fields.first().map(|(_, v)| v), Some(&expected));
        }

        #[test]
        fn parses_quoted_string_with_comma() {
            let fields = InlineTokenLexer::new(false)
                .extract_fields(r#"[str:: "yes,"]"#);

            assert_eq!(
                fields.first().map(|(_, v)| v),
                Some(&NoteFieldValue::String("yes,".to_owned()))
            );
        }

        #[test]
        fn parses_quoted_string_with_escaped_quote() {
            let fields = InlineTokenLexer::new(false)
                .extract_fields(r#"[str:: "yes, \"maybe\""]"#);

            assert_eq!(
                fields.first().map(|(_, v)| v),
                Some(&NoteFieldValue::String(r#"yes, "maybe""#.to_owned()))
            );
        }

        #[test]
        fn extracts_nested_bracket_value() {
            let fields = InlineTokenLexer::new(false)
                .extract_fields("This is some text. [key:: [value]]");

            assert_eq!(fields.first().map(|(k, _)| k.name()), Some("key"));
            assert_eq!(
                fields.first().and_then(|(_, v)| v.as_str()),
                Some("[value]")
            );
        }

        #[test]
        fn accepts_punctuation_in_wrapped_keys() {
            let fields = InlineTokenLexer::new(false)
                .extract_fields(r"Hello? [key! :: \[value]");

            assert_eq!(fields.first().map(|(k, _)| k.name()), Some("key!"));
            assert_eq!(
                fields.first().and_then(|(_, v)| v.as_str()),
                Some(r"\[value")
            );
        }

        #[test]
        fn drops_a_wrapped_field_whose_key_has_no_searchable_characters() {
            let fields = InlineTokenLexer::new(false)
                .extract_fields("Hello [!!!:: value]");

            assert!(fields.is_empty());
        }

        #[test]
        fn keeps_escaped_closing_bracket_inside_visible_value() {
            let fields = InlineTokenLexer::new(false)
                .extract_fields(r"Hello [key:: \] value]");

            assert_eq!(fields.first().map(|(k, _)| k.name()), Some("key"));
            assert_eq!(
                fields.first().and_then(|(_, v)| v.as_str()),
                Some(r"\] value")
            );
        }

        #[test]
        fn extracts_wrapped_field_after_large_leading_whitespace() {
            let fields = InlineTokenLexer::new(false)
                .extract_fields("      - [ ] Huh! [p:: 1]");

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
            let fields = InlineTokenLexer::new(false).extract_fields(input);

            assert_eq!(
                fields.first().map(|(_, v)| v),
                Some(&NoteFieldValue::Duration(expected.to_owned()))
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
            let fields = InlineTokenLexer::new(true).extract_fields(input);

            assert_eq!(fields.len(), 1);
            assert_eq!(
                fields.first().map(|(k, _)| k.name()),
                Some(expected_key)
            );
            assert_eq!(
                fields.first().map(|(_, v)| v),
                Some(&NoteFieldValue::Date(expected_date.to_owned()))
            );
        }
        #[test]
        fn accepts_a_bare_key_preceded_by_leading_whitespace() {
            let fields =
                InlineTokenLexer::new(false).extract_fields("  Status:: Draft");

            assert_eq!(fields.first().map(|(k, _)| k.name()), Some("Status"));
        }

        #[test]
        fn orders_matches_by_position_across_forms() {
            let fields = InlineTokenLexer::new(false).extract_fields(
                "Status:: Draft\nSee [Reviewer:: Jane] and (Editor:: Sam).",
            );

            let keys: Vec<&str> =
                fields.iter().map(|(k, _)| k.name()).collect();
            assert_eq!(keys, ["Status", "Reviewer", "Editor"]);
        }

        #[test]
        fn body_field_value_swallows_a_nested_wrapped_field_look_alike() {
            let fields = InlineTokenLexer::new(false)
                .extract_fields("Status:: Draft [Key:: Value]");

            assert_eq!(fields.len(), 1);
            assert_eq!(fields.first().map(|(k, _)| k.name()), Some("Status"));
            assert_eq!(
                fields.first().and_then(|(_, v)| v.as_str()),
                Some("Draft [Key:: Value]")
            );
        }
    }

    mod tags {
        use pretty_assertions::assert_eq;
        use rstest::rstest;

        use super::*;
        use crate::tag::Tag;

        #[rstest]
        #[case::standalone(
            "Filed under #book for later.",
            &["#book"]
        )]
        #[case::nested_path(
            "#projects/active needs review.",
            &["#projects/active"]
        )]
        #[case::multiple_space_separated(
            "#book #fiction favorites.",
            &["#book", "#fiction"]
        )]
        #[case::hash_embedded_in_a_word(
            "The issue is foo#bar, not a tag.",
            &[]
        )]
        #[case::adjacent_separated_by_punctuation("(#a)(#b)", &["#a", "#b"])]
        #[case::glued_directly_onto_another_tag("#a#b", &["#a"])]
        #[case::preceded_by_multibyte_punctuation("café—#book", &["#book"])]
        #[case::glued_onto_a_multibyte_letter("café#book", &[])]
        fn extracts_tags_matching_the_expected_set(
            #[case] input: &str,
            #[case] expected: &[&str],
        ) {
            let tags = InlineTokenLexer::new(false).extract_tags(input);

            let expected: Vec<Tag> =
                expected.iter().map(|tag| Tag::parse(tag).unwrap()).collect();
            assert_eq!(tags, expected);
        }
    }
}
