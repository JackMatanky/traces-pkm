//! Scan plain-text buffers for inline fields and Markdown tags.
//!
//! Operates on text already filtered by the Markdown parser: fenced code
//! blocks, indented code blocks, and inline code spans are excluded before
//! these functions run.
//!
//! # Main Functions
//!
//! - [`extract_inline_fields`]: extracts `Key:: Value`, `[Key:: Value]`, and
//!   `(Key:: Value)` body metadata.
//! - [`extract_task_inline_fields`]: also recognizes task emoji shorthand
//!   fields such as `🗓️2026-01-01`.
//! - [`extract_tags`]: extracts Markdown tags such as `#book` and
//!   `#projects/active`.

use logos::{Filter, Lexer, Logos};

use super::{Link, NoteFieldValue, cursor::SourceText, metadata::is_iso_date};
use crate::{field::FieldKey, tag::Tag};

/// Extracts inline fields from `text` in encounter order.
///
/// Recognizes `Key:: Value`, `[Key:: Value]`, and `(Key:: Value)`. `text` must
/// already exclude code spans and blocks. Use [`extract_task_inline_fields`]
/// when task emoji shorthand fields should be recognized.
pub(super) fn extract_inline_fields(
    text: &str,
) -> Vec<(FieldKey, NoteFieldValue)> {
    extract_inline_fields_with_task_shorthands(text, TaskShorthands::Exclude)
}

/// Extracts inline fields and task emoji shorthand fields from `text`.
///
/// Recognizes `Key:: Value`, `[Key:: Value]`, `(Key:: Value)`, and task
/// shorthand fields such as `🗓️2026-01-01`. `text` must already exclude code
/// spans and blocks.
pub(super) fn extract_task_inline_fields(
    text: &str,
) -> Vec<(FieldKey, NoteFieldValue)> {
    extract_inline_fields_with_task_shorthands(text, TaskShorthands::Include)
}

/// Extracts inline fields from `text` in the given `shorthands` mode.
fn extract_inline_fields_with_task_shorthands(
    text: &str,
    shorthands: TaskShorthands,
) -> Vec<(FieldKey, NoteFieldValue)> {
    let lexer = FieldToken::lexer_with_extras(text, shorthands);
    let mut fields = Vec::new();
    for result in lexer {
        if let Ok(FieldToken::Field(field)) = result {
            fields.push(field);
        }
    }
    fields
}

/// Extracts Markdown tags from `text` in encounter order.
///
/// Tags keep their leading `#`. Mid-word occurrences like `foo#bar` are
/// rejected.
pub(super) fn extract_tags(text: &str) -> Vec<Tag> {
    let lexer = TagToken::lexer(text);
    let mut tags = Vec::new();
    for result in lexer {
        if let Ok(TagToken::Tag(tag)) = result {
            tags.push(tag);
        }
    }
    tags
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

/// Bracket delimiters for wrapped inline fields.
#[derive(Copy, Clone, Debug)]
struct BracketPair {
    open: char,
    close: char,
}

impl BracketPair {
    const HIDDEN: Self = Self {
        open: '(',
        close: ')',
    };
    const VISIBLE: Self = Self {
        open: '[',
        close: ']',
    };
}

/// Field-token mode controlling whether task emoji shorthands are recognized.
///
/// Used as [`FieldToken`]'s logos `extras` value so [`extract_inline_fields`]
/// and [`extract_task_inline_fields`] choose their lexer behavior without
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
    #[token("[", |lex| wrapped_field_callback(lex, BracketPair::VISIBLE))]
    #[token("(", |lex| wrapped_field_callback(lex, BracketPair::HIDDEN))]
    #[token("\u{1F5D3}\u{FE0F}", |lex| task_field_callback(lex, "due"))]
    #[token("\u{1F5D3}", |lex| task_field_callback(lex, "due"))]
    #[token("\u{2795}", |lex| task_field_callback(lex, "created"))]
    #[token("\u{1F6EB}", |lex| task_field_callback(lex, "start"))]
    #[token("\u{23F3}", |lex| task_field_callback(lex, "scheduled"))]
    #[token("\u{2705}", |lex| task_field_callback(lex, "completion"))]
    Field((FieldKey, NoteFieldValue)),
    #[regex(r"[\s\S]", logos::skip, priority = 0)]
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
    let field = (key, parse_inline_value_str(value));
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
    pair: BracketPair,
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
    let Some(close) = find_closing_delimiter(after_sep, pair) else {
        return Filter::Skip;
    };
    let Ok(key) = FieldKey::try_from(key) else {
        return Filter::Skip;
    };
    let value = after_sep.get(..close).unwrap_or_default().trim();
    let consumed = sep
        .saturating_add(2)
        .saturating_add(close)
        .saturating_add(pair.close.len_utf8());
    lex.bump(consumed);
    Filter::Emit((key, parse_inline_value_str(value)))
}

/// Finds `pair`'s closing delimiter in wrapped field value text.
///
/// `after_sep` starts immediately after the wrapped field's `::` separator.
///
/// - Escaped delimiters (`\[`, `\]`, `\(`, `\)`) never close the field.
/// - Same-kind nesting inside the value, such as `[value]` in `[key::
///   [value]]`, does not close it early.
fn find_closing_delimiter(after_sep: &str, pair: BracketPair) -> Option<usize> {
    let mut nesting = 0usize;
    let mut escaped = false;
    for (offset, ch) in after_sep.char_indices() {
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
                return Some(offset);
            }
            nesting = nesting.saturating_sub(1);
        } else {
            // Other characters do not affect wrapper nesting.
        }
    }
    None
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

/// An atom parsed at some position: its value and the exclusive byte offset
/// immediately following it.
type Atom = (NoteFieldValue, usize);

/// Parses raw inline value text into a [`NoteFieldValue`].
fn parse_inline_value_str(raw: &str) -> NoteFieldValue {
    ValueParser::new(raw).parse()
}

/// Recursive-descent parser for inline-field value text.
///
/// [`Self::parse`] is the entry point: it tries a comma-separated list of
/// atoms, then a single atom spanning the whole value, falling back to a raw
/// [`NoteFieldValue::String`] when neither matches.
struct ValueParser<'a> {
    text: &'a str,
    source: SourceText<'a>,
}

impl<'a> ValueParser<'a> {
    #[inline]
    const fn new(text: &'a str) -> Self {
        Self {
            text,
            source: SourceText::new(text),
        }
    }

    /// Parses the whole (already-trimmed) value text into a [`NoteFieldValue`].
    ///
    /// Tries [`Self::parse_comma_list`] first, then a single
    /// [`Self::parse_atom_at`] spanning the whole text, falling back to
    /// [`NoteFieldValue::String`] holding the raw text when neither matches.
    /// Empty text parses as [`NoteFieldValue::Null`].
    fn parse(&self) -> NoteFieldValue {
        let trimmed = self.text.trim();
        if trimmed.is_empty() {
            return NoteFieldValue::Null;
        }
        let sub_parser = ValueParser::new(trimmed);
        if let Some(values) = sub_parser.parse_comma_list() {
            return NoteFieldValue::List(values);
        }
        if let Some((value, end)) = sub_parser.parse_atom_at(0)
            && sub_parser.skip_whitespace(end) == sub_parser.source.len()
        {
            return value;
        }
        NoteFieldValue::String(trimmed.to_owned())
    }

    /// Parses one or more `,`-separated atoms starting at position `0`.
    ///
    /// Returns `Some` only when a `,` follows the first atom (confirming this
    /// is a list, not a single atom) and every subsequent atom parses
    /// successfully. A trailing `,` followed by whitespace ends the list.
    fn parse_comma_list(&self) -> Option<Vec<NoteFieldValue>> {
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

    /// Parses a single atom at `pos` (after skipping leading whitespace),
    /// trying each value kind in priority order: quoted string, wikilink,
    /// duration, bool, null, ISO date, number, then tag.
    ///
    /// Returns the parsed value paired with the exclusive byte offset following
    /// it, or `None` if no kind matches at `pos`.
    fn parse_atom_at(&self, pos: usize) -> Option<Atom> {
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

    /// Parses a double-quoted string atom at `pos`.
    ///
    /// A backslash escapes the following character verbatim, so `\"` includes
    /// a literal quote. Returns `None` if `pos` is not a `"` or the string has
    /// no closing, unescaped `"`.
    fn parse_quoted_string_at(&self, pos: usize) -> Option<Atom> {
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
                    NoteFieldValue::String(value),
                    self.source.advance(self.source.advance(pos, offset), 2),
                ));
            } else {
                value.push(ch);
            }
        }
        None
    }

    /// Parses a wikilink or embed atom (`[[target]]`, `![[target]]`) at `pos`.
    fn parse_link_at(&self, pos: usize) -> Option<Atom> {
        let (link, consumed) =
            Link::parse_wikilink_prefix(self.source.from(pos)?)?;
        Some((NoteFieldValue::Link(link), self.source.advance(pos, consumed)))
    }

    /// Parses a duration atom at `pos`.
    ///
    /// Recognizes one or more `<number><unit>` parts, such as `4h15m` or
    /// `4 yrs, 6 wks`. Parts are validated by [`Self::parse_duration_part_end`]
    /// and may be comma- and whitespace-separated. Returns the raw matched text
    /// as [`NoteFieldValue::Duration`].
    fn parse_duration_at(&self, pos: usize) -> Option<Atom> {
        let mut end = self.parse_duration_part_end(pos)?;
        loop {
            let separator = self.skip_whitespace(end);
            if separator == self.source.len() {
                let raw = self.source.get(pos..end)?;
                return Some((NoteFieldValue::Duration(raw.to_owned()), end));
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
                return Some((NoteFieldValue::Duration(raw.to_owned()), end));
            } else {
                return None;
            }
        }
    }

    /// Finds the end offset of one `<number><unit>` duration part at `pos`.
    ///
    /// Returns `None` if `pos` is not a number followed by a recognized
    /// [`is_duration_unit`] unit.
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

    /// Parses a case-insensitive `true`/`false` keyword atom at `pos`.
    fn parse_bool_at(&self, pos: usize) -> Option<Atom> {
        self.parse_keyword_at(pos, "true")
            .map(|end| (NoteFieldValue::Bool(true), end))
            .or_else(|| {
                self.parse_keyword_at(pos, "false")
                    .map(|end| (NoteFieldValue::Bool(false), end))
            })
    }

    /// Parses a case-insensitive `null` keyword atom at `pos`.
    fn parse_null_at(&self, pos: usize) -> Option<Atom> {
        self.parse_keyword_at(pos, "null")
            .map(|end| (NoteFieldValue::Null, end))
    }

    /// Finds the end offset of `keyword` at `pos` on a case-insensitive match
    /// followed by an [`Self::is_atom_boundary`] position.
    fn parse_keyword_at(&self, pos: usize, keyword: &str) -> Option<usize> {
        let end = self.source.advance(pos, keyword.len());
        let token = self.source.get(pos..end)?;
        token.eq_ignore_ascii_case(keyword).then_some(())?;
        self.is_atom_boundary(end).then_some(end)
    }

    /// Parses an ISO `YYYY-MM-DD` date atom at `pos`.
    fn parse_date_at(&self, pos: usize) -> Option<Atom> {
        let end = self.source.advance(pos, 10);
        let date = self.source.get(pos..end)?;
        (is_iso_date(date) && self.is_atom_boundary(end))
            .then(|| (NoteFieldValue::Date(date.to_owned()), end))
    }

    /// Parses a finite `f64` number atom at `pos`.
    fn parse_number_at(&self, pos: usize) -> Option<Atom> {
        let end = self.parse_number_end(pos)?;
        let raw = self.source.get(pos..end)?;
        let num = raw.parse::<f64>().ok()?;
        (num.is_finite() && self.is_atom_boundary(end))
            .then_some((NoteFieldValue::Number(num), end))
    }

    /// Finds the end offset of a numeric token at `pos`: digits and the
    /// characters `+-.eE`, without validating that they form a valid `f64`.
    /// Callers ([`Self::parse_number_at`], [`Self::parse_duration_part_end`])
    /// check that separately.
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

    /// Parses a `#tag`-shaped atom (`#book`, `#projects/active`) at `pos`.
    ///
    /// Requires `#` followed by an alphabetic character. The match is returned
    /// as [`NoteFieldValue::String`] holding the tag text, including the
    /// leading `#`, since there's no dedicated tag value kind.
    fn parse_tag_at(&self, pos: usize) -> Option<Atom> {
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
        Some((NoteFieldValue::String(raw.to_owned()), end))
    }

    /// Whether `pos` is at the end of the text, immediately before whitespace,
    /// or immediately before a `,`. An atom must end at such a position to
    /// avoid greedily consuming into the next atom or trailing text.
    fn is_atom_boundary(&self, pos: usize) -> bool {
        self.source.from(pos).is_some_and(|source| {
            source
                .chars()
                .next()
                .is_none_or(|ch| ch.is_whitespace() || ch == ',')
        })
    }

    /// Returns the offset of the first non-whitespace character at or after
    /// `pos`, or the text's length if only whitespace remains.
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

/// Whether `unit` is a recognized duration unit, matched case-insensitively
/// against [`DURATION_UNITS`].
fn is_duration_unit(unit: &str) -> bool {
    DURATION_UNITS.iter().any(|candidate| unit.eq_ignore_ascii_case(candidate))
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
    #[regex(r"[\s\S]", logos::skip, priority = 0)]
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
            let fields = extract_inline_fields(input);

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
            let fields =
                extract_inline_fields("Status:: Draft\nAuthor:: Jane Doe");

            let keys: Vec<&str> =
                fields.iter().map(|(k, _)| k.name()).collect();
            assert_eq!(keys, ["Status", "Author"]);
        }

        #[test]
        fn trims_surrounding_whitespace_from_the_value() {
            let fields = extract_inline_fields("Status::    Draft   ");

            assert_eq!(
                fields.first().and_then(|(_, v)| v.as_str()),
                Some("Draft")
            );
        }

        #[test]
        fn extracts_an_empty_value_when_nothing_follows_the_double_colon() {
            let fields = extract_inline_fields("Status::");

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
            let fields = extract_inline_fields(input);

            assert_eq!(fields.first().map(|(_, v)| v), Some(&expected));
        }

        #[test]
        fn parses_dataview_link_value() {
            let fields = extract_inline_fields("[link:: [[test]]]");

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
            let fields =
                extract_inline_fields("[link:: [[yes, no, and maybe]]]");

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
            let fields =
                extract_inline_fields(r#"[link:: <a href="Page">Value</a>]"#);

            assert_eq!(
                fields.first().and_then(|(_, v)| v.as_str()),
                Some(r#"<a href="Page">Value</a>"#)
            );
        }

        #[test]
        fn parses_dataview_embed_link_value() {
            let fields = extract_inline_fields("[embed:: ![[hello]]]");
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
            let fields = extract_inline_fields(input);

            assert_eq!(fields.first().map(|(_, v)| v), Some(&expected));
        }

        #[test]
        fn parses_quoted_string_with_comma() {
            let fields = extract_inline_fields(r#"[str:: "yes,"]"#);

            assert_eq!(
                fields.first().map(|(_, v)| v),
                Some(&NoteFieldValue::String("yes,".to_owned()))
            );
        }

        #[test]
        fn parses_quoted_string_with_escaped_quote() {
            let fields = extract_inline_fields(r#"[str:: "yes, \"maybe\""]"#);

            assert_eq!(
                fields.first().map(|(_, v)| v),
                Some(&NoteFieldValue::String(r#"yes, "maybe""#.to_owned()))
            );
        }

        #[test]
        fn extracts_nested_bracket_value() {
            let fields =
                extract_inline_fields("This is some text. [key:: [value]]");

            assert_eq!(fields.first().map(|(k, _)| k.name()), Some("key"));
            assert_eq!(
                fields.first().and_then(|(_, v)| v.as_str()),
                Some("[value]")
            );
        }

        #[test]
        fn accepts_punctuation_in_wrapped_keys() {
            let fields = extract_inline_fields(r"Hello? [key! :: \[value]");

            assert_eq!(fields.first().map(|(k, _)| k.name()), Some("key!"));
            assert_eq!(
                fields.first().and_then(|(_, v)| v.as_str()),
                Some(r"\[value")
            );
        }

        #[test]
        fn drops_a_wrapped_field_whose_key_has_no_searchable_characters() {
            let fields = extract_inline_fields("Hello [!!!:: value]");

            assert!(fields.is_empty());
        }

        #[test]
        fn keeps_escaped_closing_bracket_inside_visible_value() {
            let fields = extract_inline_fields(r"Hello [key:: \] value]");

            assert_eq!(fields.first().map(|(k, _)| k.name()), Some("key"));
            assert_eq!(
                fields.first().and_then(|(_, v)| v.as_str()),
                Some(r"\] value")
            );
        }

        #[test]
        fn extracts_wrapped_field_after_large_leading_whitespace() {
            let fields = extract_inline_fields("      - [ ] Huh! [p:: 1]");

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
            let fields = extract_inline_fields(input);

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
            let fields = extract_task_inline_fields(input);

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
            let fields = extract_inline_fields("  Status:: Draft");

            assert_eq!(fields.first().map(|(k, _)| k.name()), Some("Status"));
        }

        #[test]
        fn orders_matches_by_position_across_forms() {
            let fields = extract_inline_fields(
                "Status:: Draft\nSee [Reviewer:: Jane] and (Editor:: Sam).",
            );

            let keys: Vec<&str> =
                fields.iter().map(|(k, _)| k.name()).collect();
            assert_eq!(keys, ["Status", "Reviewer", "Editor"]);
        }

        #[test]
        fn body_field_value_swallows_a_nested_wrapped_field_look_alike() {
            let fields = extract_inline_fields("Status:: Draft [Key:: Value]");

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
            let tags = extract_tags(input);

            let expected: Vec<Tag> =
                expected.iter().map(|tag| Tag::parse(tag).unwrap()).collect();
            assert_eq!(tags, expected);
        }
    }

    mod parse_null {
        use super::*;

        #[test]
        fn parses_null_keyword_case_insensitively() {
            // Arrange
            let vp = ValueParser::new("null");

            // Act
            let result = vp.parse_null_at(0);

            // Assert
            assert!(result.is_some(), "null must be recognized");
            let (value, end) = result.unwrap();
            assert_eq!(value, NoteFieldValue::Null);
            assert_eq!(end, 4);
        }

        #[test]
        fn rejects_non_null_keywords() {
            // Arrange
            let vp = ValueParser::new("nil");

            // Act
            let result = vp.parse_null_at(0);

            // Assert
            assert!(result.is_none(), "nil must not be recognized as null");
        }
    }

    mod parse_tag {
        use super::*;

        #[test]
        fn parses_tag_with_hash_prefix() {
            // Arrange
            let vp = ValueParser::new("#book");

            // Act
            let result = vp.parse_tag_at(0);

            // Assert
            assert!(result.is_some(), "#book must be parsed as a tag");
            let (value, end) = result.unwrap();
            assert_eq!(value, NoteFieldValue::String("#book".to_owned()));
            assert_eq!(end, 5);
        }

        #[test]
        fn parses_tag_with_slashes_dashes_underscores() {
            // Arrange
            let vp = ValueParser::new("#my-tag/project_a");

            // Act
            let result = vp.parse_tag_at(0);

            // Assert
            assert!(result.is_some(), "#my-tag/project_a must be parsed");
            let (value, _) = result.unwrap();
            assert_eq!(
                value,
                NoteFieldValue::String("#my-tag/project_a".to_owned())
            );
        }

        #[test]
        fn rejects_tag_without_hash_prefix() {
            // Arrange
            let vp = ValueParser::new("book");

            // Act
            let result = vp.parse_tag_at(0);

            // Assert
            assert!(result.is_none(), "tag without # must not parse");
        }
    }

    mod parse_number {
        use super::*;

        #[test]
        fn rejects_nan_and_infinity() {
            // Arrange
            let vp_nan = ValueParser::new("NaN");
            let vp_inf = ValueParser::new("Infinity");

            // Act
            let result_nan = vp_nan.parse_number_at(0);
            let result_inf = vp_inf.parse_number_at(0);

            // Assert
            assert!(result_nan.is_none(), "NaN must not be parsed as number");
            assert!(
                result_inf.is_none(),
                "Infinity must not be parsed as number"
            );
        }
    }

    mod boundary {
        use super::*;

        #[test]
        fn treats_comma_as_atom_boundary() {
            // Arrange
            let vp = ValueParser::new("a,b");

            // Act — position 1 is ','
            let is_boundary = vp.is_atom_boundary(1);

            // Assert
            assert!(is_boundary, "comma must be an atom boundary");
        }

        #[test]
        fn rejects_alphanumeric_as_atom_boundary() {
            // Arrange
            let vp = ValueParser::new("ab");

            // Act — position 1 is 'b'
            let is_boundary = vp.is_atom_boundary(1);

            // Assert
            assert!(
                !is_boundary,
                "alphanumeric char must not be an atom boundary"
            );
        }
    }

    mod parse_duration {
        use super::*;

        #[test]
        fn parses_duration_with_space_separator() {
            // Arrange
            let vp = ValueParser::new("1h 30m");

            // Act
            let result = vp.parse_duration_at(0);

            // Assert
            assert!(result.is_some(), "1h 30m must parse as duration");
            let (value, _) = result.unwrap();
            assert_eq!(value, NoteFieldValue::Duration("1h 30m".to_owned()));
        }

        #[test]
        fn parses_duration_without_separator() {
            // Arrange
            let vp = ValueParser::new("1h30m");

            // Act
            let result = vp.parse_duration_at(0);

            // Assert
            assert!(result.is_some(), "1h30m must parse as duration");
            let (value, _) = result.unwrap();
            assert_eq!(value, NoteFieldValue::Duration("1h30m".to_owned()));
        }
    }

    mod duration_unit {
        use rstest::rstest;

        use super::*;

        #[rstest]
        #[case::hours("h")]
        #[case::minutes("m")]
        #[case::seconds("s")]
        #[case::days("d")]
        #[case::hours_upper("H")]
        #[case::minutes_upper("M")]
        #[case::seconds_upper("S")]
        #[case::days_upper("D")]
        fn accepts_valid_duration_units(#[case] unit: &str) {
            assert!(
                is_duration_unit(unit),
                "{unit} must be a valid duration unit"
            );
        }

        #[rstest]
        #[case::years("y")]
        #[case::empty("")]
        #[case::single_char_invalid("x")]
        fn rejects_invalid_duration_units(#[case] unit: &str) {
            assert!(
                !is_duration_unit(unit),
                "{unit} must not be a valid duration unit"
            );
        }
    }
}
