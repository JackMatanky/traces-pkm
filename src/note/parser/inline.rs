//! Recursive-descent parser for inline-field value text.
//!
//! Converts raw inline field value strings (Dataview syntax) into strongly
//! typed [`NoteFieldValue`] records, supporting comma-separated lists, quoted
//! strings, wikilinks, durations, booleans, nulls, ISO dates, numbers, and
//! tags.

use crate::{
    DateValue, DurationValue,
    note::{Link, NoteFieldValue, cursor::SourceText},
};

/// An atom parsed at some position: its value and the exclusive byte offset
/// immediately following it.
type Atom = (NoteFieldValue, usize);

/// Parses raw inline value text into a [`NoteFieldValue`].
#[inline]
#[must_use]
pub(super) fn parse_inline_value(raw: &str) -> NoteFieldValue {
    InlineValueParser::new(raw).parse()
}

/// Recursive-descent parser for inline-field value text.
///
/// [`Self::parse`] is the entry point: it tries a comma-separated list of
/// atoms, then a single atom spanning the whole value, falling back to a raw
/// [`NoteFieldValue::String`] when neither matches.
struct InlineValueParser<'a> {
    text: &'a str,
    source: SourceText<'a>,
}

impl<'a> InlineValueParser<'a> {
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
        let sub_parser = Self::new(trimmed);
        if let Some(values) = sub_parser.parse_comma_list() {
            return NoteFieldValue::List(values.into_boxed_slice());
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
    /// Recognizes one or more `<number><unit>` parts via
    /// [`DurationValue::parse_prefix`] and returns
    /// [`NoteFieldValue::Duration`].
    fn parse_duration_at(&self, pos: usize) -> Option<Atom> {
        let tail = self.source.from(pos)?;
        let (dv, consumed) = DurationValue::parse_prefix(tail)?;
        let end = self.source.advance(pos, consumed);
        Some((NoteFieldValue::Duration(dv), end))
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
        if !(DateValue::is_iso_shape(date) && self.is_atom_boundary(end)) {
            return None;
        }
        let value = DateValue::parse_iso(date).ok()?;
        Some((NoteFieldValue::Date(value), end))
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
    /// [`Self::parse_number_at`] checks that separately.
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

#[cfg(test)]
mod tests {
    use super::*;

    mod parse_null {
        use super::*;

        #[test]
        fn parses_null_keyword_case_insensitively() {
            let vp = InlineValueParser::new("null");
            let result = vp.parse_null_at(0);

            assert!(
                matches!(result, Some((NoteFieldValue::Null, 4))),
                "null must be recognized"
            );
        }

        #[test]
        fn rejects_non_null_keywords() {
            let vp = InlineValueParser::new("nil");
            let result = vp.parse_null_at(0);

            assert!(result.is_none(), "nil must not be recognized as null");
        }
    }

    mod parse_tag {
        use super::*;

        #[test]
        fn parses_tag_with_hash_prefix() {
            let vp = InlineValueParser::new("#book");
            let result = vp.parse_tag_at(0);

            assert!(
                matches!(
                    &result,
                    Some((NoteFieldValue::String(s), 5)) if s == "#book"
                ),
                "#book must be parsed as a tag"
            );
        }

        #[test]
        fn parses_tag_with_slashes_dashes_underscores() {
            let vp = InlineValueParser::new("#my-tag/project_a");
            let result = vp.parse_tag_at(0);

            assert!(
                matches!(
                    &result,
                    Some((NoteFieldValue::String(s), _))
                        if s == "#my-tag/project_a"
                ),
                "#my-tag/project_a must be parsed"
            );
        }

        #[test]
        fn rejects_tag_without_hash_prefix() {
            let vp = InlineValueParser::new("book");
            let result = vp.parse_tag_at(0);

            assert!(result.is_none(), "tag without # must not parse");
        }
    }

    mod parse_number {
        use super::*;

        #[test]
        fn rejects_nan_and_infinity() {
            let vp_nan = InlineValueParser::new("NaN");
            let vp_inf = InlineValueParser::new("Infinity");

            let result_nan = vp_nan.parse_number_at(0);
            let result_inf = vp_inf.parse_number_at(0);

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
            let vp = InlineValueParser::new("a,b");
            let is_boundary = vp.is_atom_boundary(1);

            assert!(is_boundary, "comma must be an atom boundary");
        }

        #[test]
        fn rejects_alphanumeric_as_atom_boundary() {
            let vp = InlineValueParser::new("ab");
            let is_boundary = vp.is_atom_boundary(1);

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
            let vp = InlineValueParser::new("1h 30m");
            let result = vp.parse_duration_at(0);

            assert!(
                matches!(
                    &result,
                    Some((NoteFieldValue::Duration(dv), _))
                        if dv.as_str() == "1h 30m"
                ),
                "1h 30m must parse as duration"
            );
        }

        #[test]
        fn parses_duration_without_separator() {
            let vp = InlineValueParser::new("1h30m");
            let result = vp.parse_duration_at(0);

            assert!(
                matches!(
                    &result,
                    Some((NoteFieldValue::Duration(dv), _))
                        if dv.as_str() == "1h30m"
                ),
                "1h30m must parse as duration"
            );
        }
    }

    mod parse_date {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn parses_a_valid_date_atom() {
            let vp = InlineValueParser::new("2026-07-29");
            let result = vp.parse_date_at(0);

            let expected =
                DateValue::parse_iso("2026-07-29").expect("valid date");
            assert_eq!(result, Some((NoteFieldValue::Date(expected), 10)));
        }

        #[test]
        fn rejects_a_date_immediately_followed_by_non_boundary_text() {
            let vp = InlineValueParser::new("2026-07-29abc");
            let result = vp.parse_date_at(0);

            assert_eq!(result, None);
        }

        #[test]
        fn rejects_an_invalid_calendar_date_with_valid_iso_shape() {
            let vp = InlineValueParser::new("9999-99-99");
            let result = vp.parse_date_at(0);

            assert_eq!(result, None);
        }

        #[test]
        fn rejects_text_shorter_than_an_iso_date() {
            let vp = InlineValueParser::new("2026-07");
            let result = vp.parse_date_at(0);

            assert_eq!(result, None);
        }

        #[test]
        fn rejects_a_non_iso_date_shape() {
            let vp = InlineValueParser::new("2026/07/29");
            let result = vp.parse_date_at(0);

            assert_eq!(result, None);
        }
    }
}
