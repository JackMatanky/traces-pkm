//! Dataview-compatible inline-field and Markdown tag lexer.
//!
//! Parses plain-text buffers produced by the Markdown parser. Those buffers
//! already exclude fenced code blocks, indented code blocks, and inline
//! code.
//!
//! Main components:
//! - [`extract_inline_fields`] / [`extract_task_inline_fields`]: Extract
//!   Dataview inline fields (`Key:: Value`, `[Key:: Value]`, `(Key:: Value)`)
//!   from Markdown text; the latter also recognizes task emoji shorthand fields
//!   (e.g. `🗓️2026-01-01`).
//! - [`extract_tags`]: Extract Markdown tags (`#book`, `#projects/active`) from
//!   Markdown text.

use logos::{Filter, Lexer, Logos};

use super::{
    FieldValue, InlineField, InlineFieldForm, Outlink, Tag, cursor::SourceText,
    metadata::is_iso_date,
};

/// Extracts Dataview inline fields (`Key:: Value`, `[Key:: Value]`,
/// `(Key:: Value)`) from `text`, in the order they occur.
///
/// `text` must already exclude code spans and blocks. Task emoji shorthand
/// fields are not recognized; use [`extract_task_inline_fields`] for those.
pub(super) fn extract_inline_fields(text: &str) -> Vec<InlineField> {
    extract_inline_fields_with_task_shorthands(text, TaskShorthands::Exclude)
}

/// Extracts Dataview inline fields, additionally recognizing task emoji
/// shorthand fields (e.g. `🗓️2026-01-01`), from `text`, in the order they
/// occur.
///
/// `text` must already exclude code spans and blocks.
pub(super) fn extract_task_inline_fields(text: &str) -> Vec<InlineField> {
    extract_inline_fields_with_task_shorthands(text, TaskShorthands::Include)
}

/// Extracts inline fields from `text` in the given `shorthands` mode.
fn extract_inline_fields_with_task_shorthands(
    text: &str,
    shorthands: TaskShorthands,
) -> Vec<InlineField> {
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

/// Returns the character immediately before the current match's start
/// (`None` if the match starts at the beginning of the source).
///
/// Shared by [`body_field_callback`] and [`tag_callback`], both of which
/// need a look-behind check that logos' regex dialect can't express.
fn char_before<'source, T>(lex: &Lexer<'source, T>) -> Option<char>
where
    T: Logos<'source, Source = str>,
{
    lex.source()
        .get(..lex.span().start)
        .and_then(|prefix| prefix.chars().next_back())
}

const ISO_DATE_LEN: usize = 10;

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

/// Whether task emoji shorthand fields (e.g. `🗓️2026-01-01`) participate in
/// tokenization.
///
/// [`FieldToken`]'s logos `extras`: names the two modes
/// [`extract_inline_fields`] and [`extract_task_inline_fields`] select, in
/// place of a bare `bool` flag.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
enum TaskShorthands {
    /// Task emoji shorthands are recognized.
    Include,
    /// Task emoji shorthands are ignored.
    #[default]
    Exclude,
}

impl TaskShorthands {
    /// Whether this mode recognizes task emoji shorthands.
    #[inline]
    #[must_use]
    fn is_included(self) -> bool {
        matches!(self, Self::Include)
    }
}

/// Tokens matched while extracting Dataview inline fields from free-form
/// Markdown text.
///
/// - [`Self::Field`] carries every emitted [`InlineField`]; callbacks return
///   [`Filter::Skip`] to discard a non-matching candidate (e.g. an unclosed
///   wrapped field) and keep scanning.
/// - [`Self::Ignored`] is a catch-all, single-character skip for ordinary prose
///   that matches none of the field patterns.
#[derive(logos::Logos, Debug, Clone, PartialEq)]
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
    Field(InlineField),
    #[regex(r"[\s\S]", logos::skip, priority = 0)]
    Ignored,
}

/// Parses a bare inline field (`Key:: Value`) from the `Key::` prefix
/// already matched by [`FieldToken`]'s body-field pattern, consuming the
/// rest of the line as the raw value — equivalent to the regex
/// `(?m)^[ \t]*key::[ \t]*(.*)$`.
///
/// Logos has no look-behind support, so a line-start check replaces that
/// regex's `^` anchor: the match is rejected (skipping only the matched
/// `Key::` span, not the rest of the line) unless it starts right after a
/// newline or at the start of the text.
fn body_field_callback(lex: &mut Lexer<'_, FieldToken>) -> Filter<InlineField> {
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
    let field = InlineField::new(
        key,
        parse_inline_value_str(value),
        InlineFieldForm::Body,
    );
    lex.bump(value_end);
    Filter::Emit(field)
}

/// Parses a wrapped inline field (`[Key:: Value]` or `(Key:: Value)`)
/// starting just after its already-consumed opening delimiter.
///
/// Rejects, skipping only the opening delimiter, when:
/// - there is no `::` separator before the text ends,
/// - the key is empty or contains a bracket character, or
/// - [`find_closing_delimiter`] finds no matching closing delimiter.
fn wrapped_field_callback(
    lex: &mut Lexer<'_, FieldToken>,
    pair: BracketPair,
) -> Filter<InlineField> {
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
    let value = after_sep.get(..close).unwrap_or_default().trim();
    let consumed = sep
        .saturating_add(2)
        .saturating_add(close)
        .saturating_add(pair.close.len_utf8());
    lex.bump(consumed);
    Filter::Emit(InlineField::new(
        key,
        parse_inline_value_str(value),
        pair.form,
    ))
}

/// Finds the byte offset of `pair`'s unescaped, unnested closing delimiter
/// in `after_sep` (the wrapped field's value text, starting just after its
/// `::` separator).
///
/// - Escaped delimiters (`\[`, `\]`, `\(`, `\)`) never close the field.
/// - Same-kind nesting inside the value (e.g. the inner `[value]` in `[key::
///   [value]]`) does not close it early; only a `pair.close` at zero nesting
///   depth does.
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

/// Parses a Dataview task emoji shorthand (e.g. `🗓️2026-01-01`) starting
/// just after its already-consumed emoji token, emitting an inline field
/// keyed by `key` when the following text — optional inline whitespace,
/// then exactly [`ISO_DATE_LEN`] bytes — is a valid ISO date.
///
/// Always skips when `lex.extras` is [`TaskShorthands::Exclude`]:
/// [`extract_inline_fields`] lexes in that mode.
fn task_field_callback(
    lex: &mut Lexer<'_, FieldToken>,
    key: &'static str,
) -> Filter<InlineField> {
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
    lex.bump(ws_end.saturating_add(ISO_DATE_LEN));
    Filter::Emit(InlineField::new(
        key,
        FieldValue::Date(candidate.to_owned()),
        InlineFieldForm::Body,
    ))
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
type Atom = (FieldValue, usize);

/// Parses raw inline value text into a [`FieldValue`].
fn parse_inline_value_str(raw: &str) -> FieldValue {
    ValueParser::new(raw).parse()
}

/// Recursive-descent parser for Dataview inline-field value text.
///
/// [`Self::parse`] is the entry point: it tries a comma-separated list of
/// atoms, then a single atom spanning the whole value, falling back to a
/// raw [`FieldValue::String`] when neither matches.
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

    /// Parses the whole (already-trimmed) value text into a [`FieldValue`].
    ///
    /// Tries [`Self::parse_comma_list`] first, then a single
    /// [`Self::parse_atom_at`] spanning the whole text, falling back to
    /// [`FieldValue::String`] holding the raw text when neither matches.
    /// Empty text parses as [`FieldValue::Null`].
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

    /// Parses one or more `,`-separated atoms starting at position `0`.
    ///
    /// Returns `None` unless the first atom is followed by a `,` — confirming
    /// this is a list, not a single atom — and every subsequent atom parses
    /// successfully. A trailing `,` followed only by whitespace ends the list.
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

    /// Parses a single atom at `pos` (after skipping leading whitespace),
    /// trying each value kind in priority order: quoted string, wikilink,
    /// duration, bool, null, ISO date, number, then tag.
    ///
    /// Returns the parsed value paired with the exclusive byte offset
    /// following it, or `None` if no kind matches at `pos`.
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

    /// Parses a Dataview wikilink or embed atom (`[[target]]`, `![[target]]`)
    /// at `pos`.
    fn parse_link_at(&self, pos: usize) -> Option<Atom> {
        let (link, consumed) =
            Outlink::parse_wikilink_prefix(self.source.from(pos)?)?;
        Some((FieldValue::Link(link), self.source.advance(pos, consumed)))
    }

    /// Parses a double-quoted string atom at `pos`. A backslash escapes the
    /// following character verbatim, so `\"` includes a literal quote.
    ///
    /// Returns `None` if `pos` isn't a `"` or the string has no closing,
    /// unescaped `"`.
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
                    FieldValue::String(value),
                    self.source.advance(self.source.advance(pos, offset), 2),
                ));
            } else {
                value.push(ch);
            }
        }
        None
    }

    /// Parses a Dataview duration atom at `pos` (e.g. `4h15m`,
    /// `4 yrs, 6 wks`): one or more `<number><unit>` parts, each validated by
    /// [`Self::parse_duration_part_end`] and optionally comma- and
    /// whitespace-separated.
    ///
    /// Returns the raw matched text as [`FieldValue::Duration`], stopping
    /// (without failing) at the first position that isn't a valid next part.
    fn parse_duration_at(&self, pos: usize) -> Option<Atom> {
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

    /// Finds the end offset of one `<number><unit>` duration part at `pos`, or
    /// `None` if `pos` isn't a number followed by a recognized
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
            .map(|end| (FieldValue::Bool(true), end))
            .or_else(|| {
                self.parse_keyword_at(pos, "false")
                    .map(|end| (FieldValue::Bool(false), end))
            })
    }

    /// Parses a case-insensitive `null` keyword atom at `pos`.
    fn parse_null_at(&self, pos: usize) -> Option<Atom> {
        self.parse_keyword_at(pos, "null").map(|end| (FieldValue::Null, end))
    }

    /// Finds the end offset of `keyword` at `pos` if it matches
    /// case-insensitively and the position immediately after it satisfies
    /// [`Self::is_atom_boundary`].
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
            .then(|| (FieldValue::Date(date.to_owned()), end))
    }

    /// Parses a finite `f64` number atom at `pos`.
    fn parse_number_at(&self, pos: usize) -> Option<Atom> {
        let end = self.parse_number_end(pos)?;
        let raw = self.source.get(pos..end)?;
        let num = raw.parse::<f64>().ok()?;
        (num.is_finite() && self.is_atom_boundary(end))
            .then_some((FieldValue::Number(num), end))
    }

    /// Finds the end offset of a numeric token at `pos`: digits and the
    /// characters `+-.eE`, without validating that they form a valid `f64` —
    /// callers ([`Self::parse_number_at`], [`Self::parse_duration_part_end`])
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
    /// as [`FieldValue::String`] holding the tag text, including the leading
    /// `#` — there's no dedicated tag value kind.
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
        Some((FieldValue::String(raw.to_owned()), end))
    }

    /// Whether `pos` is at the end of the text or immediately before
    /// whitespace or a `,` — the position an atom must end at to avoid
    /// greedily consuming into the next atom or trailing text.
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

/// Whether `unit` is a recognized Dataview duration unit, matched
/// case-insensitively against [`DURATION_UNITS`].
fn is_duration_unit(unit: &str) -> bool {
    DURATION_UNITS.iter().any(|candidate| unit.eq_ignore_ascii_case(candidate))
}

/// Tokens matched while extracting Markdown tags (`#book`,
/// `#projects/active`) from free-form text.
///
/// - [`Self::Tag`] carries every emitted [`Tag`]; [`tag_callback`] returns
///   [`Filter::Skip`] to reject a `#` that fails the lookbehind or
///   leading-letter check, consuming only the `#` so a rejected mid-word
///   occurrence like `foo#bar` does not swallow the rest of the text.
/// - [`Self::Ignored`] is a catch-all, single-character skip for ordinary text.
#[derive(logos::Logos, Debug, Clone, PartialEq)]
enum TagToken {
    #[token("#", tag_callback)]
    Tag(Tag),
    #[regex(r"[\s\S]", logos::skip, priority = 0)]
    Ignored,
}

/// Parses a Markdown tag starting just after its already-consumed leading
/// `#`. Rejects (skipping only the `#`) a `#` preceded by an alphanumeric
/// or `_` character (mid-word, e.g. `foo#bar`) or not followed by an
/// alphabetic character (e.g. `#1`).
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
    Filter::Emit(Tag::new(lex.slice()))
}

#[cfg(test)]
mod tests {
    use super::*;

    mod inline_fields {
        use pretty_assertions::assert_eq;
        use rstest::rstest;

        use super::*;
        use crate::note::{FieldValue, InlineFieldForm, LinkType, Outlink};

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
            assert!(matches!(
                fields.first().expect("field present").value(),
                FieldValue::Link(link) if
                    link.target() == "hello" &&
                    link.text() == "hello" &&
                    link.kind() == LinkType::Wikilink &&
                    link.is_embedded()
            ));
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

        #[test]
        fn body_field_value_swallows_a_nested_wrapped_field_look_alike() {
            let fields = extract_inline_fields("Status:: Draft [Key:: Value]");

            assert_eq!(fields.len(), 1);
            assert_eq!(fields.first().map(InlineField::key), Some("Status"));
            assert_eq!(
                fields.first().and_then(|field| field.value().as_str()),
                Some("Draft [Key:: Value]")
            );
        }
    }

    mod tags {
        use pretty_assertions::assert_eq;
        use rstest::rstest;

        use super::*;
        use crate::note::Tag;

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
