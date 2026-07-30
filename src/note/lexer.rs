//! Dataview-compatible inline-field and Markdown tag lexer.
//!
//! Parses plain-text buffers produced by the Markdown parser. Those buffers
//! already exclude fenced code blocks, indented code blocks, and inline code.

use std::sync::LazyLock;

use regex::Regex;

use super::{
    FieldValue, InlineField, InlineFieldForm, Outlink, Tag,
    byte::{ByteSpan, SourceText},
    metadata::is_iso_date,
};

/// Matches full-line `Key:: Value` body fields.
///
/// Body fields require a single letter-led key without whitespace. Wrapped
/// forms are delimiter-bounded and allow multi-word keys.
#[expect(
    clippy::expect_used,
    reason = "static regex pattern is valid at compile time"
)]
static BODY_FIELD_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)^[ \t]*([A-Za-z][A-Za-z0-9_-]*)::[ \t]*(.*)$")
        .expect("BODY_FIELD_RE pattern is valid")
});

/// Matches Markdown tag tokens such as `#book` and `#projects/active`.
///
/// [`extract_tags`] rejects mid-word occurrences like `foo#bar`.
#[expect(
    clippy::expect_used,
    reason = "static regex pattern is valid at compile time"
)]
static TAG_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"#[[:alpha:]][[:alnum:]_/-]*").expect("TAG_RE pattern is valid")
});

const ISO_DATE_LEN: usize = 10;
/// Dataview task emoji shorthand mappings to inline field keys.
///
/// Supported emoji shorthands:
/// - `\u{1F5D3}\u{FE0F}` (`🗓️`): `due` (with variation selector `U+FE0F`)
/// - `\u{1F5D3}` (`🗓`): `due` (base text variant)
/// - `\u{2795}` (`➕`): `created`
/// - `\u{1F6EB}` (`🛫`): `start`
/// - `\u{23F3}` (`⏳`): `scheduled`
/// - `\u{2705}` (`✅`): `completion`
const TASK_EMOJI_FIELDS: &[(&str, &str)] = &[
    ("\u{1F5D3}\u{FE0F}", "due"),
    ("\u{1F5D3}", "due"),
    ("\u{2795}", "created"),
    ("\u{1F6EB}", "start"),
    ("\u{23F3}", "scheduled"),
    ("\u{2705}", "completion"),
];
const DURATION_UNITS: &[&str] = &[
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
];

/// Extracts inline fields from `text` in byte-position order.
///
/// `text` must already exclude code spans and blocks.
pub(super) fn extract_inline_fields(text: &str) -> Vec<InlineField> {
    extract_inline_fields_with_task_shorthands(text, false)
}

/// Extracts inline fields and Dataview task emoji shorthand fields.
pub(super) fn extract_task_inline_fields(text: &str) -> Vec<InlineField> {
    extract_inline_fields_with_task_shorthands(text, true)
}

/// Extracts inline fields from `text`, optionally including task emoji
/// shorthands.
fn extract_inline_fields_with_task_shorthands(
    text: &str,
    include_task_shorthands: bool,
) -> Vec<InlineField> {
    let mut lexer = InlineFieldLexer::new(text);
    lexer.scan_body_fields();
    lexer.scan_wrapped_fields(BracketPair::VISIBLE);
    lexer.scan_wrapped_fields(BracketPair::HIDDEN);
    if include_task_shorthands {
        lexer.scan_task_shorthands();
    }
    lexer.finish()
}

/// Extracts Markdown tags from `text` in encounter order.
///
/// Tags keep their leading `#`. Mid-word occurrences like `foo#bar` are
/// rejected.
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

/// Bracket delimiters and their corresponding [`InlineFieldForm`].
#[derive(Copy, Clone, Debug)]
struct BracketPair {
    open: char,
    close: char,
    form: InlineFieldForm,
}

impl BracketPair {
    const HIDDEN: Self = Self {
        open: '(',
        close: ')',
        form: InlineFieldForm::HiddenKey,
    };
    const VISIBLE: Self = Self {
        open: '[',
        close: ']',
        form: InlineFieldForm::VisibleKey,
    };
}

/// Candidate inline field match with its byte range in source text.
struct FieldMatch {
    range: ByteSpan,
    field: InlineField,
}

/// Stateful lexer for extracting [`InlineField`] candidates from Markdown text.
struct InlineFieldLexer<'a> {
    text: &'a str,
    source: SourceText<'a>,
    matches: Vec<FieldMatch>,
}

impl<'a> InlineFieldLexer<'a> {
    #[inline]
    fn new(text: &'a str) -> Self {
        Self {
            text,
            source: SourceText::new(text),
            matches: Vec::new(),
        }
    }

    fn scan_body_fields(&mut self) {
        for caps in BODY_FIELD_RE.captures_iter(self.text) {
            let (Some(whole), Some(key), Some(value)) =
                (caps.get(0), caps.get(1), caps.get(2))
            else {
                continue;
            };
            self.matches.push(FieldMatch {
                range: ByteSpan::new(whole.start(), whole.end()),
                field: InlineField::new(
                    key.as_str().trim(),
                    parse_inline_value_str(value.as_str()),
                    InlineFieldForm::Body,
                ),
            });
        }
    }

    fn scan_wrapped_fields(&mut self, pair: BracketPair) {
        let mut next = 0;
        while let Some(start) = self.source.find_char_from(next, pair.open) {
            if let Some(field_match) = self.find_wrapped_field(start, pair) {
                next = field_match.range.end();
                self.matches.push(field_match);
            } else {
                next = self.source.advance_char(start, pair.open);
            }
        }
    }

    fn find_wrapped_field(
        &self,
        start: usize,
        pair: BracketPair,
    ) -> Option<FieldMatch> {
        let (key, value_start) =
            self.find_separator(self.source.advance_char(start, pair.open))?;
        if key.is_empty()
            || key.chars().any(|ch| matches!(ch, '[' | ']' | '(' | ')'))
        {
            return None;
        }
        let (value, end) = self.find_closing(value_start, pair)?;
        Some(FieldMatch {
            range: ByteSpan::new(start, end),
            field: InlineField::new(
                key,
                parse_inline_value_str(value),
                pair.form,
            ),
        })
    }

    fn find_separator(&self, start: usize) -> Option<(&'a str, usize)> {
        let separator = self.source.find_str_from(start, "::")?;
        Some((
            self.source.get(start..separator)?.trim(),
            self.source.advance(separator, 2),
        ))
    }

    fn find_closing(
        &self,
        start: usize,
        pair: BracketPair,
    ) -> Option<(&'a str, usize)> {
        let mut nesting = 0usize;
        let mut escaped = false;
        for (offset, ch) in self.source.from(start)?.char_indices() {
            if ch == '\\' {
                escaped = !escaped;
                continue;
            }
            if escaped {
                escaped = false;
                continue;
            }
            if ch == pair.open {
                nesting = nesting.saturating_add(1);
            } else if ch == pair.close {
                if nesting == 0 {
                    let end = self.source.advance(start, offset);
                    return Some((
                        self.source.get(start..end)?.trim(),
                        self.source.advance_char(end, ch),
                    ));
                }
                nesting = nesting.saturating_sub(1);
            } else {
                // Other characters do not affect wrapper nesting.
            }
        }
        None
    }

    fn scan_task_shorthands(&mut self) {
        for &(emoji, key) in TASK_EMOJI_FIELDS {
            let mut next = 0;
            while let Some(start) = self.source.find_str_from(next, emoji) {
                let emoji_end = self.source.advance(start, emoji.len());
                let date_start = self.skip_inline_whitespace(emoji_end);
                let date_end = self.source.advance(date_start, ISO_DATE_LEN);
                if let Some(value) = self.source.get(date_start..date_end)
                    && is_iso_date(value)
                {
                    self.matches.push(FieldMatch {
                        range: ByteSpan::new(start, date_end),
                        field: InlineField::new(
                            key,
                            FieldValue::Date(value.to_owned()),
                            InlineFieldForm::Body,
                        ),
                    });
                }
                next = emoji_end;
            }
        }
    }

    fn skip_inline_whitespace(&self, pos: usize) -> usize {
        self.source
            .from(pos)
            .and_then(|rest| {
                rest.char_indices()
                    .find(|(_, ch)| !matches!(ch, ' ' | '\t'))
                    .map(|(offset, _)| self.source.advance(pos, offset))
            })
            .unwrap_or_else(|| self.source.len())
    }

    fn finish(mut self) -> Vec<InlineField> {
        self.matches.sort_by_key(|field| field.range.start());
        let mut filtered = Vec::new();
        let mut last_end = 0;
        for field_match in self.matches {
            if filtered.is_empty() || last_end <= field_match.range.start() {
                last_end = field_match.range.end();
                filtered.push(field_match.field);
            }
        }
        filtered
    }
}

/// Parses raw inline value text into a [`FieldValue`].
fn parse_inline_value_str(raw: &str) -> FieldValue {
    ValueParser::new(raw).parse()
}
struct ValueParser<'a> {
    text: &'a str,
    source: SourceText<'a>,
}

impl<'a> ValueParser<'a> {
    #[inline]
    fn new(text: &'a str) -> Self {
        Self {
            text,
            source: SourceText::new(text),
        }
    }

    fn parse(&self) -> FieldValue {
        let trimmed = self.text.trim();
        if trimmed.is_empty() {
            return FieldValue::Null;
        }
        let sub_parser = ValueParser::new(trimmed);
        if let Some(values) = sub_parser.parse_comma_list() {
            return FieldValue::List(values);
        }
        if let Some((value, end)) = sub_parser.parse_atom_at(0)
            && sub_parser.skip_whitespace(end) == sub_parser.source.len()
        {
            return value;
        }
        FieldValue::String(trimmed.to_owned())
    }

    fn parse_comma_list(&self) -> Option<Vec<FieldValue>> {
        let (first, mut pos) = self.parse_atom_at(0)?;
        pos = self.skip_whitespace(pos);
        if !self.source.from(pos)?.starts_with(',') {
            return None;
        }
        let mut values = vec![first];
        loop {
            pos = self.source.advance(pos, 1);
            pos = self.skip_whitespace(pos);
            if pos == self.source.len() {
                return Some(values);
            }
            let (value, end) = self.parse_atom_at(pos)?;
            values.push(value);
            pos = self.skip_whitespace(end);
            if pos == self.source.len() {
                return Some(values);
            }
            if !self.source.from(pos)?.starts_with(',') {
                return None;
            }
        }
    }

    fn parse_atom_at(&self, pos: usize) -> Option<(FieldValue, usize)> {
        let pos = self.skip_whitespace(pos);
        self.parse_quoted_string_at(pos)
            .or_else(|| self.parse_link_at(pos))
            .or_else(|| self.parse_duration_at(pos))
            .or_else(|| self.parse_bool_at(pos))
            .or_else(|| self.parse_null_at(pos))
            .or_else(|| self.parse_date_at(pos))
            .or_else(|| self.parse_number_at(pos))
            .or_else(|| self.parse_tag_at(pos))
    }

    fn parse_link_at(&self, pos: usize) -> Option<(FieldValue, usize)> {
        let (link, consumed) =
            Outlink::parse_wikilink_prefix(self.source.from(pos)?)?;
        Some((FieldValue::Link(link), self.source.advance(pos, consumed)))
    }

    fn parse_quoted_string_at(
        &self,
        pos: usize,
    ) -> Option<(FieldValue, usize)> {
        let rest = self.source.from(pos)?.strip_prefix('"')?;
        let mut value = String::new();
        let mut escaped = false;
        for (offset, ch) in rest.char_indices() {
            if escaped {
                value.push(ch);
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                return Some((
                    FieldValue::String(value),
                    self.source.advance(self.source.advance(pos, offset), 2),
                ));
            } else {
                value.push(ch);
            }
        }
        None
    }

    fn parse_duration_at(&self, pos: usize) -> Option<(FieldValue, usize)> {
        let mut end = self.parse_duration_part_end(pos)?;
        loop {
            let separator = self.skip_whitespace(end);
            if separator == self.source.len() {
                let raw = self.source.get(pos..end)?;
                return Some((FieldValue::Duration(raw.to_owned()), end));
            }
            let next = if self.source.from(separator)?.starts_with(',') {
                self.skip_whitespace(self.source.advance(separator, 1))
            } else {
                separator
            };
            if let Some(part_end) = self.parse_duration_part_end(next) {
                end = part_end;
            } else if separator == end {
                let raw = self.source.get(pos..end)?;
                return Some((FieldValue::Duration(raw.to_owned()), end));
            } else {
                return None;
            }
        }
    }

    fn parse_duration_part_end(&self, pos: usize) -> Option<usize> {
        let number_end = self.parse_number_end(pos)?;
        let unit_start = self.skip_whitespace(number_end);
        if unit_start == self.source.len() {
            return None;
        }
        let unit_end = self
            .source
            .from(unit_start)?
            .char_indices()
            .take_while(|(_, ch)| ch.is_ascii_alphabetic())
            .map(|(offset, ch)| self.source.token_end(unit_start, offset, ch))
            .last()?;
        let unit = self.source.get(unit_start..unit_end)?;
        is_duration_unit(unit).then_some(unit_end)
    }

    fn parse_bool_at(&self, pos: usize) -> Option<(FieldValue, usize)> {
        self.parse_keyword_at(pos, "true")
            .map(|end| (FieldValue::Bool(true), end))
            .or_else(|| {
                self.parse_keyword_at(pos, "false")
                    .map(|end| (FieldValue::Bool(false), end))
            })
    }

    fn parse_null_at(&self, pos: usize) -> Option<(FieldValue, usize)> {
        self.parse_keyword_at(pos, "null").map(|end| (FieldValue::Null, end))
    }

    fn parse_keyword_at(&self, pos: usize, keyword: &str) -> Option<usize> {
        let end = self.source.advance(pos, keyword.len());
        let token = self.source.get(pos..end)?;
        token.eq_ignore_ascii_case(keyword).then_some(())?;
        self.is_atom_boundary(end).then_some(end)
    }

    fn parse_date_at(&self, pos: usize) -> Option<(FieldValue, usize)> {
        let end = self.source.advance(pos, 10);
        let date = self.source.get(pos..end)?;
        (is_iso_date(date) && self.is_atom_boundary(end))
            .then(|| (FieldValue::Date(date.to_owned()), end))
    }

    fn parse_number_at(&self, pos: usize) -> Option<(FieldValue, usize)> {
        let end = self.parse_number_end(pos)?;
        let raw = self.source.get(pos..end)?;
        let num = raw.parse::<f64>().ok()?;
        (num.is_finite() && self.is_atom_boundary(end))
            .then_some((FieldValue::Number(num), end))
    }

    fn parse_number_end(&self, pos: usize) -> Option<usize> {
        self.source
            .from(pos)?
            .char_indices()
            .take_while(|(_, ch)| {
                ch.is_ascii_digit() || matches!(ch, '+' | '-' | '.' | 'e' | 'E')
            })
            .map(|(offset, ch)| self.source.token_end(pos, offset, ch))
            .last()
    }

    fn parse_tag_at(&self, pos: usize) -> Option<(FieldValue, usize)> {
        let rest = self.source.from(pos)?.strip_prefix('#')?;
        let mut chars = rest.chars();
        chars.next().filter(|ch| ch.is_alphabetic())?;
        let end = self
            .source
            .from(pos)?
            .char_indices()
            .skip(1)
            .take_while(|(_, ch)| {
                ch.is_alphanumeric() || matches!(ch, '_' | '/' | '-')
            })
            .map(|(offset, ch)| self.source.token_end(pos, offset, ch))
            .last()
            .unwrap_or_else(|| self.source.advance(pos, 1));
        let raw = self.source.get(pos..end)?;
        Some((FieldValue::String(raw.to_owned()), end))
    }

    fn is_atom_boundary(&self, pos: usize) -> bool {
        self.source.from(pos).is_some_and(|source| {
            source
                .chars()
                .next()
                .is_none_or(|ch| ch.is_whitespace() || ch == ',')
        })
    }

    fn skip_whitespace(&self, pos: usize) -> usize {
        self.source
            .from(pos)
            .and_then(|rest| {
                rest.char_indices()
                    .find(|(_, ch)| !ch.is_whitespace())
                    .map(|(offset, _)| self.source.advance(pos, offset))
            })
            .unwrap_or_else(|| self.source.len())
    }
}

fn is_duration_unit(unit: &str) -> bool {
    DURATION_UNITS.iter().any(|candidate| unit.eq_ignore_ascii_case(candidate))
}

#[cfg(test)]
mod tests {
    use super::*;

    mod inline_fields {
        use pretty_assertions::assert_eq;
        use rstest::rstest;

        use super::*;
        use crate::note::LinkType;

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
        fn parses_dataview_wikilink_value_with_commas_in_target() {
            let fields =
                extract_inline_fields("[link:: [[yes, no, and maybe]]]");

            assert_eq!(
                fields.first().map(InlineField::value),
                Some(&FieldValue::Link(Outlink::new(
                    "yes, no, and maybe",
                    "yes, no, and maybe",
                    LinkType::Wikilink
                )))
            );
        }

        #[test]
        fn preserves_dataview_html_link_values_as_text() {
            let fields =
                extract_inline_fields(r#"[link:: <a href="Page">Value</a>]"#);

            assert_eq!(
                fields.first().and_then(|field| field.value().as_str()),
                Some(r#"<a href="Page">Value</a>"#)
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
        fn parses_quoted_string_with_escaped_quote() {
            let fields = extract_inline_fields(r#"[str:: "yes, \"maybe\""]"#);

            assert_eq!(
                fields.first().map(InlineField::value),
                Some(&FieldValue::String(r#"yes, "maybe""#.to_owned()))
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

        #[test]
        fn keeps_escaped_closing_bracket_inside_visible_value() {
            let fields = extract_inline_fields(r"Hello [key:: \] value]");

            assert_eq!(fields.first().map(InlineField::key), Some("key"));
            assert_eq!(
                fields.first().and_then(|field| field.value().as_str()),
                Some(r"\] value")
            );
        }

        #[test]
        fn extracts_wrapped_field_after_large_leading_whitespace() {
            let fields = extract_inline_fields("      - [ ] Huh! [p:: 1]");

            assert_eq!(fields.first().map(InlineField::key), Some("p"));
            assert_eq!(
                fields.first().map(InlineField::value),
                Some(&FieldValue::Number(1.0))
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

        #[rstest]
        #[case::no_space("🗓️2026-07-30", "due", "2026-07-30")]
        #[case::single_space("🗓️ 2026-07-30", "due", "2026-07-30")]
        #[case::multiple_spaces("🗓️   2026-07-30", "due", "2026-07-30")]
        fn extracts_task_emoji_shorthands_with_optional_spaces(
            #[case] input: &str,
            #[case] expected_key: &str,
            #[case] expected_date: &str,
        ) {
            let fields = extract_task_inline_fields(input);

            assert_eq!(fields.len(), 1);
            assert_eq!(
                fields.first().map(InlineField::key),
                Some(expected_key)
            );
            assert_eq!(
                fields.first().map(InlineField::value),
                Some(&FieldValue::Date(expected_date.to_owned()))
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
