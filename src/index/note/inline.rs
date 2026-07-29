//! Lexer for Dataview-compatible inline fields and markdown tags.
//!
//! The parser passes this module plain-text buffers for each text block and
//! list item. Those buffers already exclude fenced code blocks, indented code
//! blocks, and inline code, so the lexer does not inspect [`super::CodeRegion`]
//! ranges.

use std::sync::LazyLock;

use regex::{Captures, Regex};

use super::{
    FieldValue, InlineField, InlineFieldForm, Outlink, Tag,
    metadata::is_iso_date,
};

/// Matches full-line `Key:: Value` body fields.
///
/// Body fields require a single letter-led key token with no whitespace. The
/// bracketed forms below are safely delimited, so they can allow multi-word
/// keys.
#[expect(
    clippy::expect_used,
    reason = "static regex pattern is valid at compile time"
)]
static BODY_FIELD_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)^[ \t]*([A-Za-z][A-Za-z0-9_-]*)::[ \t]*(.*)$")
        .expect("BODY_FIELD_RE pattern is valid")
});

/// Matches markdown tag tokens such as `#book` and `#projects/active`.
///
/// [`extract_tags`] checks the byte before each match and rejects mid-word
/// occurrences like `foo#bar`.
#[expect(
    clippy::expect_used,
    reason = "static regex pattern is valid at compile time"
)]
static TAG_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"#[[:alpha:]][[:alnum:]_/-]*").expect("TAG_RE pattern is valid")
});

/// Extracts inline fields from `text`, sorted by byte position.
///
/// The input must already exclude code spans and blocks.
pub(super) fn extract_inline_fields(text: &str) -> Vec<InlineField> {
    extract_inline_fields_with_task_shorthands(text, false)
}

/// Extracts inline fields and Dataview task emoji shorthand fields.
pub(super) fn extract_task_inline_fields(text: &str) -> Vec<InlineField> {
    extract_inline_fields_with_task_shorthands(text, true)
}

fn extract_inline_fields_with_task_shorthands(
    text: &str,
    include_task_shorthands: bool,
) -> Vec<InlineField> {
    let mut matches: Vec<FieldMatch> = Vec::new();
    for caps in BODY_FIELD_RE.captures_iter(text) {
        push_body_field(&mut matches, &caps);
    }
    scan_wrapped_fields(
        text,
        '[',
        ']',
        InlineFieldForm::VisibleKey,
        &mut matches,
    );
    scan_wrapped_fields(
        text,
        '(',
        ')',
        InlineFieldForm::HiddenKey,
        &mut matches,
    );
    if include_task_shorthands {
        push_task_shorthand_fields(text, &mut matches);
    }
    matches.sort_by_key(|field| field.start);
    let mut filtered = Vec::new();
    let mut last_end = 0;
    for field_match in matches {
        if filtered.is_empty() || last_end <= field_match.start {
            last_end = field_match.end;
            filtered.push(field_match.field);
        }
    }
    filtered
}

/// Extracts markdown tags from `text` in encounter order.
///
/// Tags keep their leading `#`. Matches whose `#` is immediately preceded by
/// an alphanumeric character or `_` are rejected so `foo#bar` is not a tag.
pub(super) fn extract_tags(text: &str) -> Vec<Tag> {
    TAG_RE
        .find_iter(text)
        .filter(|found| {
            text[..found.start()]
                .chars()
                .next_back()
                .is_none_or(|c| !c.is_alphanumeric() && c != '_')
        })
        .map(|found| Tag::new(found.as_str()))
        .collect()
}

struct FieldMatch {
    start: usize,
    end: usize,
    field: InlineField,
}

/// Pushes a captured body field and its byte range onto `matches`.
fn push_body_field(matches: &mut Vec<FieldMatch>, caps: &Captures<'_>) {
    let (Some(whole), Some(key), Some(value)) =
        (caps.get(0), caps.get(1), caps.get(2))
    else {
        return;
    };
    matches.push(FieldMatch {
        start: whole.start(),
        end: whole.end(),
        field: InlineField::new(
            key.as_str().trim(),
            parse_inline_value_str(value.as_str()),
            InlineFieldForm::Body,
        ),
    });
}

#[expect(
    clippy::arithmetic_side_effects,
    reason = "scanner byte offsets are derived from valid string slices"
)]
fn scan_wrapped_fields(
    text: &str,
    open: char,
    close: char,
    form: InlineFieldForm,
    matches: &mut Vec<FieldMatch>,
) {
    let mut next = 0;
    while let Some(found) = text[next..].find(open) {
        let start = next + found;
        if let Some(field_match) =
            find_wrapped_field(text, start, open, close, form)
        {
            next = field_match.end;
            matches.push(field_match);
        } else {
            next = start + open.len_utf8();
        }
    }
}

#[expect(
    clippy::arithmetic_side_effects,
    reason = "scanner byte offsets are derived from valid string slices"
)]
fn find_wrapped_field(
    text: &str,
    start: usize,
    open: char,
    close: char,
    form: InlineFieldForm,
) -> Option<FieldMatch> {
    let (key, value_start) = find_separator(text, start + open.len_utf8())?;
    if key.is_empty()
        || key.chars().any(|ch| matches!(ch, '[' | ']' | '(' | ')'))
    {
        return None;
    }
    let (value, end) = find_closing(text, value_start, open, close)?;
    Some(FieldMatch {
        start,
        end,
        field: InlineField::new(key, parse_inline_value_str(value), form),
    })
}

#[expect(
    clippy::arithmetic_side_effects,
    reason = "separator byte offsets are derived from valid string slices"
)]
fn find_separator(text: &str, start: usize) -> Option<(&str, usize)> {
    let separator = text[start..].find("::")? + start;
    Some((text[start..separator].trim(), separator + 2))
}

#[expect(
    clippy::arithmetic_side_effects,
    reason = "scanner byte offsets are derived from valid string slices"
)]
fn find_closing(
    text: &str,
    start: usize,
    open: char,
    close: char,
) -> Option<(&str, usize)> {
    let mut nesting = 0;
    let mut escaped = false;
    for (offset, ch) in text[start..].char_indices() {
        if ch == '\\' {
            escaped = !escaped;
            continue;
        }
        if escaped {
            escaped = false;
            continue;
        }
        if ch == open {
            nesting += 1;
        } else if ch == close {
            nesting -= 1;
        } else {
            // Other characters do not affect wrapper nesting.
        }
        if nesting < 0 {
            let end = start + offset;
            return Some((text[start..end].trim(), end + ch.len_utf8()));
        }
    }
    None
}

#[expect(
    clippy::arithmetic_side_effects,
    reason = "emoji shorthand offsets are derived from valid string slices"
)]
fn push_task_shorthand_fields(text: &str, matches: &mut Vec<FieldMatch>) {
    for (emoji, key) in [
        ("🗓️", "due"),
        ("🗓", "due"),
        ("➕", "created"),
        ("🛫", "start"),
        ("⏳", "scheduled"),
        ("✅", "completion"),
    ] {
        let mut next = 0;
        while let Some(found) = text[next..].find(emoji) {
            let start = next + found;
            let value_start = start + emoji.len();
            if let Some(value) = text.get(value_start..value_start + 10)
                && is_iso_date(value)
            {
                matches.push(FieldMatch {
                    start,
                    end: value_start + 10,
                    field: InlineField::new(
                        key,
                        FieldValue::Date(value.to_owned()),
                        InlineFieldForm::Body,
                    ),
                });
            }
            next = value_start;
        }
    }
}

/// Parses a raw string value into a [`FieldValue`].
fn parse_inline_value_str(raw: &str) -> FieldValue {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return FieldValue::Null;
    }
    if let Some(values) = parse_comma_list(trimmed) {
        return FieldValue::List(values);
    }
    if let Some((value, end)) = parse_atom_at(trimmed, 0)
        && skip_whitespace(trimmed, end) == trimmed.len()
    {
        return value;
    }
    FieldValue::String(trimmed.to_owned())
}

#[expect(
    clippy::arithmetic_side_effects,
    reason = "comma separators are one ASCII byte"
)]
fn parse_comma_list(s: &str) -> Option<Vec<FieldValue>> {
    let (first, mut pos) = parse_atom_at(s, 0)?;
    pos = skip_whitespace(s, pos);
    if !s[pos..].starts_with(',') {
        return None;
    }
    let mut values = vec![first];
    loop {
        pos += 1;
        pos = skip_whitespace(s, pos);
        if pos == s.len() {
            return Some(values);
        }
        let (value, end) = parse_atom_at(s, pos)?;
        values.push(value);
        pos = skip_whitespace(s, end);
        if pos == s.len() {
            return Some(values);
        }
        if !s[pos..].starts_with(',') {
            return None;
        }
    }
}

fn parse_atom_at(s: &str, pos: usize) -> Option<(FieldValue, usize)> {
    let pos = skip_whitespace(s, pos);
    parse_quoted_string_at(s, pos)
        .or_else(|| parse_link_at(s, pos))
        .or_else(|| parse_duration_at(s, pos))
        .or_else(|| parse_bool_at(s, pos))
        .or_else(|| parse_null_at(s, pos))
        .or_else(|| parse_date_at(s, pos))
        .or_else(|| parse_number_at(s, pos))
        .or_else(|| parse_tag_at(s, pos))
}

#[expect(
    clippy::arithmetic_side_effects,
    reason = "link parser returns a consumed byte count for this slice"
)]
fn parse_link_at(s: &str, pos: usize) -> Option<(FieldValue, usize)> {
    let (link, consumed) = Outlink::parse_wikilink_prefix(&s[pos..])?;
    Some((FieldValue::Link(link), pos + consumed))
}

#[expect(
    clippy::arithmetic_side_effects,
    reason = "quoted string offsets are derived from valid string slices"
)]
fn parse_quoted_string_at(s: &str, pos: usize) -> Option<(FieldValue, usize)> {
    let rest = s[pos..].strip_prefix('"')?;
    let mut value = String::new();
    let mut escaped = false;
    for (offset, ch) in rest.char_indices() {
        if escaped {
            value.push(ch);
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if ch == '"' {
            return Some((FieldValue::String(value), pos + 1 + offset + 1));
        } else {
            value.push(ch);
        }
    }
    None
}

#[expect(
    clippy::arithmetic_side_effects,
    reason = "duration token offsets are derived from valid string slices"
)]
fn parse_duration_at(s: &str, pos: usize) -> Option<(FieldValue, usize)> {
    let mut end = parse_duration_part_end(s, pos)?;
    loop {
        let separator = skip_whitespace(s, end);
        if separator == s.len() {
            return Some((FieldValue::Duration(s[pos..end].to_owned()), end));
        }
        let next = if s[separator..].starts_with(',') {
            skip_whitespace(s, separator + 1)
        } else {
            separator
        };
        if let Some(part_end) = parse_duration_part_end(s, next) {
            end = part_end;
        } else if separator == end {
            return Some((FieldValue::Duration(s[pos..end].to_owned()), end));
        } else {
            return None;
        }
    }
}

#[expect(
    clippy::arithmetic_side_effects,
    reason = "duration token offsets are derived from valid string slices"
)]
fn parse_duration_part_end(s: &str, pos: usize) -> Option<usize> {
    let number_end = parse_number_end(s, pos)?;
    let unit_start = skip_whitespace(s, number_end);
    if unit_start == s.len() {
        return None;
    }
    let unit_end = s[unit_start..]
        .char_indices()
        .take_while(|(_, ch)| ch.is_ascii_alphabetic())
        .map(|(offset, ch)| unit_start + offset + ch.len_utf8())
        .last()?;
    is_duration_unit(&s[unit_start..unit_end]).then_some(unit_end)
}

fn is_duration_unit(unit: &str) -> bool {
    [
        "year",
        "years",
        "yr",
        "yrs",
        "month",
        "months",
        "mo",
        "mos",
        "week",
        "weeks",
        "wk",
        "wks",
        "w",
        "day",
        "days",
        "d",
        "hour",
        "hours",
        "hr",
        "hrs",
        "h",
        "minute",
        "minutes",
        "min",
        "mins",
        "m",
        "second",
        "seconds",
        "sec",
        "secs",
        "s",
        "millisecond",
        "milliseconds",
        "ms",
    ]
    .iter()
    .any(|candidate| unit.eq_ignore_ascii_case(candidate))
}

fn parse_bool_at(s: &str, pos: usize) -> Option<(FieldValue, usize)> {
    parse_keyword_at(s, pos, "true")
        .map(|end| (FieldValue::Bool(true), end))
        .or_else(|| {
            parse_keyword_at(s, pos, "false")
                .map(|end| (FieldValue::Bool(false), end))
        })
}

fn parse_null_at(s: &str, pos: usize) -> Option<(FieldValue, usize)> {
    parse_keyword_at(s, pos, "null").map(|end| (FieldValue::Null, end))
}

#[expect(clippy::arithmetic_side_effects, reason = "keywords are ASCII tokens")]
fn parse_keyword_at(s: &str, pos: usize, keyword: &str) -> Option<usize> {
    let end = pos + keyword.len();
    let token = s.get(pos..end)?;
    token.eq_ignore_ascii_case(keyword).then_some(())?;
    is_atom_boundary(s, end).then_some(end)
}

#[expect(
    clippy::arithmetic_side_effects,
    reason = "ISO dates are ten ASCII bytes"
)]
fn parse_date_at(s: &str, pos: usize) -> Option<(FieldValue, usize)> {
    let end = pos + 10;
    let date = s.get(pos..end)?;
    (is_iso_date(date) && is_atom_boundary(s, end))
        .then(|| (FieldValue::Date(date.to_owned()), end))
}

fn parse_number_at(s: &str, pos: usize) -> Option<(FieldValue, usize)> {
    let end = parse_number_end(s, pos)?;
    let num = s[pos..end].parse::<f64>().ok()?;
    (num.is_finite() && is_atom_boundary(s, end))
        .then_some((FieldValue::Number(num), end))
}

#[expect(
    clippy::arithmetic_side_effects,
    reason = "number token offsets are derived from valid string slices"
)]
fn parse_number_end(s: &str, pos: usize) -> Option<usize> {
    s[pos..]
        .char_indices()
        .take_while(|(_, ch)| {
            ch.is_ascii_digit() || matches!(ch, '+' | '-' | '.' | 'e' | 'E')
        })
        .map(|(offset, ch)| pos + offset + ch.len_utf8())
        .last()
}

#[expect(
    clippy::arithmetic_side_effects,
    reason = "tag token offsets are derived from valid string slices"
)]
fn parse_tag_at(s: &str, pos: usize) -> Option<(FieldValue, usize)> {
    let rest = s[pos..].strip_prefix('#')?;
    let mut chars = rest.chars();
    chars.next().filter(|ch| ch.is_alphabetic())?;
    let end = s[pos..]
        .char_indices()
        .skip(1)
        .take_while(|(_, ch)| {
            ch.is_alphanumeric() || matches!(ch, '_' | '/' | '-')
        })
        .map(|(offset, ch)| pos + offset + ch.len_utf8())
        .last()
        .unwrap_or(pos + 1);
    Some((FieldValue::String(s[pos..end].to_owned()), end))
}

fn is_atom_boundary(s: &str, pos: usize) -> bool {
    s[pos..].chars().next().is_none_or(|ch| ch.is_whitespace() || ch == ',')
}

#[expect(
    clippy::arithmetic_side_effects,
    reason = "whitespace offsets are derived from valid string slices"
)]
fn skip_whitespace(s: &str, pos: usize) -> usize {
    s[pos..]
        .char_indices()
        .find(|(_, ch)| !ch.is_whitespace())
        .map_or(s.len(), |(offset, _)| pos + offset)
}

#[cfg(test)]
mod tests {
    use super::*;

    mod inline_fields {
        use pretty_assertions::assert_eq;
        use rstest::rstest;

        use super::*;
        use crate::index::LinkType;

        #[rstest]
        #[case::body(
            "Author:: Jane Doe",
            "Author",
            "Jane Doe",
            InlineFieldForm::Body
        )]
        #[case::visible_key(
            "See the [Status:: Draft] note.",
            "Status",
            "Draft",
            InlineFieldForm::VisibleKey
        )]
        #[case::hidden_key(
            "See the (Status:: Draft) note.",
            "Status",
            "Draft",
            InlineFieldForm::HiddenKey
        )]
        fn extracts_a_field_in_its_declared_form(
            #[case] input: &str,
            #[case] expected_key: &str,
            #[case] expected_value: &str,
            #[case] expected_form: InlineFieldForm,
        ) {
            let fields = extract_inline_fields(input);

            assert_eq!(fields.len(), 1);
            assert_eq!(
                fields.first().map(InlineField::key),
                Some(expected_key)
            );
            assert_eq!(
                fields.first().and_then(|field| field.value().as_str()),
                Some(expected_value)
            );
            assert_eq!(
                fields.first().map(InlineField::form),
                Some(expected_form)
            );
        }

        #[test]
        fn rejects_a_multi_word_bare_key() {
            let fields =
                extract_inline_fields("This sentence has a :: but no key.");

            assert_eq!(fields.len(), 0);
        }

        #[rstest]
        #[case::visible_key("[Due Date:: 2024-01-01]", "Due Date")]
        #[case::hidden_key("(Due Date:: 2024-01-01)", "Due Date")]
        fn accepts_a_multi_word_key_when_delimiter_bounded(
            #[case] input: &str,
            #[case] expected_key: &str,
        ) {
            let fields = extract_inline_fields(input);

            assert_eq!(
                fields.first().map(InlineField::key),
                Some(expected_key)
            );
            assert_eq!(
                fields.first().and_then(|field| field.value().as_str()),
                Some("2024-01-01")
            );
        }

        #[test]
        fn extracts_a_bare_field_from_each_line_of_a_multiline_buffer() {
            let fields =
                extract_inline_fields("Status:: Draft\nAuthor:: Jane Doe");

            let keys: Vec<&str> = fields.iter().map(InlineField::key).collect();
            assert_eq!(keys, ["Status", "Author"]);
        }

        #[test]
        fn trims_surrounding_whitespace_from_the_value() {
            let fields = extract_inline_fields("Status::    Draft   ");

            assert_eq!(
                fields.first().and_then(|field| field.value().as_str()),
                Some("Draft")
            );
        }

        #[test]
        fn extracts_an_empty_value_when_nothing_follows_the_double_colon() {
            let fields = extract_inline_fields("Status::");

            assert_eq!(
                fields.first().map(InlineField::value),
                Some(&FieldValue::Null)
            );
        }

        #[rstest]
        #[case::true_value("flag:: true", FieldValue::Bool(true))]
        #[case::false_value("flag:: false", FieldValue::Bool(false))]
        #[case::number("score:: 4.5", FieldValue::Number(4.5))]
        #[case::date("due:: 2026-07-29", FieldValue::Date("2026-07-29".to_owned()))]
        #[case::non_finite_number("score:: NaN", FieldValue::String("NaN".to_owned()))]
        fn parses_inline_value_types(
            #[case] input: &str,
            #[case] expected: FieldValue,
        ) {
            let fields = extract_inline_fields(input);

            assert_eq!(fields.first().map(InlineField::value), Some(&expected));
        }

        #[test]
        fn parses_dataview_link_value() {
            let fields = extract_inline_fields("[link:: [[test]]]");

            assert_eq!(
                fields.first().map(InlineField::value),
                Some(&FieldValue::Link(Outlink::new(
                    "test",
                    "test",
                    LinkType::Wikilink
                )))
            );
        }

        #[test]
        fn parses_dataview_embed_link_value() {
            let fields = extract_inline_fields("[embed:: ![[hello]]]");

            let FieldValue::Link(link) =
                fields.first().expect("field present").value()
            else {
                panic!("expected link value");
            };
            assert_eq!(link.target(), "hello");
            assert_eq!(link.text(), "hello");
            assert_eq!(link.kind(), LinkType::Wikilink);
            assert_eq!(link.is_embedded(), true);
        }

        #[rstest]
        #[case::trailing_comma(
            "[links:: [[test]],]",
            FieldValue::List(vec![FieldValue::Link(Outlink::new(
                "test",
                "test",
                LinkType::Wikilink
            ))])
        )]
        #[case::links(
            "[links:: [[test]], [[test2]]]",
            FieldValue::List(vec![
                FieldValue::Link(Outlink::new(
                    "test",
                    "test",
                    LinkType::Wikilink
                )),
                FieldValue::Link(Outlink::new(
                    "test2",
                    "test2",
                    LinkType::Wikilink
                )),
            ])
        )]
        #[case::mixed_atoms(
            r#"[values:: 1, 2, 3, "hello"]"#,
            FieldValue::List(vec![
                FieldValue::Number(1.0),
                FieldValue::Number(2.0),
                FieldValue::Number(3.0),
                FieldValue::String("hello".to_owned()),
            ])
        )]
        fn parses_dataview_comma_lists(
            #[case] input: &str,
            #[case] expected: FieldValue,
        ) {
            let fields = extract_inline_fields(input);

            assert_eq!(fields.first().map(InlineField::value), Some(&expected));
        }

        #[test]
        fn parses_quoted_string_with_comma() {
            let fields = extract_inline_fields(r#"[str:: "yes,"]"#);

            assert_eq!(
                fields.first().map(InlineField::value),
                Some(&FieldValue::String("yes,".to_owned()))
            );
        }

        #[test]
        fn extracts_nested_bracket_value() {
            let fields =
                extract_inline_fields("This is some text. [key:: [value]]");

            assert_eq!(fields.first().map(InlineField::key), Some("key"));
            assert_eq!(
                fields.first().and_then(|field| field.value().as_str()),
                Some("[value]")
            );
        }

        #[test]
        fn accepts_punctuation_in_wrapped_keys() {
            let fields = extract_inline_fields(r"Hello? [key! :: \[value]");

            assert_eq!(fields.first().map(InlineField::key), Some("key!"));
            assert_eq!(
                fields.first().and_then(|field| field.value().as_str()),
                Some(r"\[value")
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
            let fields = extract_inline_fields(input);

            assert_eq!(
                fields.first().map(InlineField::value),
                Some(&FieldValue::Duration(expected.to_owned()))
            );
        }

        #[test]
        fn accepts_a_bare_key_preceded_by_leading_whitespace() {
            let fields = extract_inline_fields("  Status:: Draft");

            assert_eq!(fields.first().map(InlineField::key), Some("Status"));
        }

        #[test]
        fn orders_matches_by_position_across_forms() {
            let fields = extract_inline_fields(
                "Status:: Draft\nSee [Reviewer:: Jane] and (Editor:: Sam).",
            );

            let keys: Vec<&str> = fields.iter().map(InlineField::key).collect();
            assert_eq!(keys, ["Status", "Reviewer", "Editor"]);
        }
    }

    mod tags {
        use pretty_assertions::assert_eq;
        use rstest::rstest;

        use super::*;

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
            let tags = extract_tags(input);

            let expected: Vec<Tag> =
                expected.iter().map(|tag| Tag::new(*tag)).collect();
            assert_eq!(tags, expected);
        }
    }
}
