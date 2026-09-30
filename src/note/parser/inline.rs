//! Recursive-descent parser for inline-field value text.
//!
//! Converts raw inline field value strings (Dataview syntax) into strongly
//! typed [`NoteFieldValue`] records, supporting comma-separated lists, quoted
//! strings, wikilinks, durations, booleans, nulls, ISO dates, numbers, and
//! tags.

use crate::{
    DateValue, DurationValue, Tag,
    note::{Link, NoteFieldValue, cursor::SourceText},
};

/// An atom parsed at some position: its value and the exclusive byte offset
/// immediately following it.
#[derive(Clone, Debug, PartialEq)]
struct ParsedAtom {
    value: NoteFieldValue,
    end: usize,
}

impl ParsedAtom {
    #[inline]
    const fn new(value: NoteFieldValue, end: usize) -> Self {
        Self {
            value,
            end,
        }
    }
}

/// Parses raw inline value text into a [`NoteFieldValue`].
///
/// Trims surrounding whitespace; empty or whitespace-only text parses as
/// [`NoteFieldValue::Null`].
#[inline]
#[must_use]
pub(super) fn parse_inline_value(raw: &str) -> NoteFieldValue {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return NoteFieldValue::Null;
    }
    InlineValueParser::new(trimmed).parse()
}

/// Recursive-descent parser for inline-field value text.
///
/// Constructed with already-trimmed, non-empty text. [`Self::parse`] is the
/// entry point: it parses the first atom once, returning it when it spans the
/// whole value. When the atom is followed by a comma, it continues via
/// [`Self::parse_comma_list_from`]. It falls back to a raw
/// [`NoteFieldValue::String`] when neither matches.
struct InlineValueParser<'a> {
    source: SourceText<'a>,
}

impl<'a> InlineValueParser<'a> {
    #[inline]
    const fn new(source: &'a str) -> Self {
        Self {
            source: SourceText::new(source),
        }
    }

    /// Parses the whole (already-trimmed, non-empty) value text into a
    /// [`NoteFieldValue`].
    ///
    /// Parses the first atom once, returning it if it spans the whole text. If
    /// followed by a comma, parses remaining atoms via
    /// [`Self::parse_comma_list_from`]. Falls back to
    /// [`NoteFieldValue::String`] holding the text when neither matches.
    fn parse(&self) -> NoteFieldValue {
        let Some(first) = self.parse_atom_at(0) else {
            return NoteFieldValue::String(self.source.as_ref().to_owned());
        };
        let after_first = self.skip_whitespace(first.end);
        if after_first == self.source.len() {
            return first.value;
        }
        if self.source.from(after_first).is_some_and(|s| s.starts_with(','))
            && let Some(values) =
                self.parse_comma_list_from(first.value, after_first)
        {
            return NoteFieldValue::List(values.into_boxed_slice());
        }
        NoteFieldValue::String(self.source.as_ref().to_owned())
    }

    /// Parses `,`-separated atoms starting after the first atom at `pos`.
    ///
    /// `pos` must point to the `,` following the first atom.
    /// Returns `Some` if all subsequent items parse as valid atoms.
    fn parse_comma_list_from(
        &self,
        first: NoteFieldValue,
        mut pos: usize,
    ) -> Option<Vec<NoteFieldValue>> {
        let mut values = vec![first];
        loop {
            pos = self.source.advance(pos, 1);
            pos = self.skip_whitespace(pos);
            if pos == self.source.len() {
                return Some(values);
            }
            let atom = self.parse_atom_at(pos)?;
            values.push(atom.value);
            pos = self.skip_whitespace(atom.end);
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
    fn parse_atom_at(&self, pos: usize) -> Option<ParsedAtom> {
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
    /// A backslash escapes the following character verbatim, so `\"` includes a
    /// literal quote. Returns `None` if `pos` is not a `"` or the string has no
    /// closing, unescaped `"`.
    fn parse_quoted_string_at(&self, pos: usize) -> Option<ParsedAtom> {
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
                return Some(ParsedAtom::new(
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
    fn parse_link_at(&self, pos: usize) -> Option<ParsedAtom> {
        let (link, consumed) =
            Link::parse_wikilink_prefix(self.source.from(pos)?)?;
        Some(ParsedAtom::new(
            NoteFieldValue::Link(link),
            self.source.advance(pos, consumed),
        ))
    }

    /// Parses a duration atom at `pos`.
    ///
    /// Recognizes one or more `<number><unit>` parts via
    /// [`DurationValue::parse_prefix`] and returns
    /// [`NoteFieldValue::Duration`].
    fn parse_duration_at(&self, pos: usize) -> Option<ParsedAtom> {
        let tail = self.source.from(pos)?;
        let (dv, consumed) = DurationValue::parse_prefix(tail)?;
        let end = self.source.advance(pos, consumed);
        Some(ParsedAtom::new(NoteFieldValue::Duration(dv), end))
    }

    /// Parses a case-insensitive `true`/`false` keyword atom at `pos`.
    fn parse_bool_at(&self, pos: usize) -> Option<ParsedAtom> {
        self.parse_keyword_at(pos, "true")
            .map(|end| ParsedAtom::new(NoteFieldValue::Bool(true), end))
            .or_else(|| {
                self.parse_keyword_at(pos, "false").map(|end| {
                    ParsedAtom::new(NoteFieldValue::Bool(false), end)
                })
            })
    }

    /// Parses a case-insensitive `null` keyword atom at `pos`.
    fn parse_null_at(&self, pos: usize) -> Option<ParsedAtom> {
        self.parse_keyword_at(pos, "null")
            .map(|end| ParsedAtom::new(NoteFieldValue::Null, end))
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
    fn parse_date_at(&self, pos: usize) -> Option<ParsedAtom> {
        let end = self.source.advance(pos, 10);
        let date = self.source.get(pos..end)?;
        if !(DateValue::is_iso_shape(date) && self.is_atom_boundary(end)) {
            return None;
        }
        let value = DateValue::parse_iso(date).ok()?;
        Some(ParsedAtom::new(NoteFieldValue::Date(value), end))
    }

    /// Parses a finite `f64` number atom at `pos`.
    fn parse_number_at(&self, pos: usize) -> Option<ParsedAtom> {
        let end = self.parse_number_end(pos)?;
        let raw = self.source.get(pos..end)?;
        let num = raw.parse::<f64>().ok()?;
        (num.is_finite() && self.is_atom_boundary(end))
            .then_some(ParsedAtom::new(NoteFieldValue::Number(num), end))
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
    fn parse_tag_at(&self, pos: usize) -> Option<ParsedAtom> {
        let tail = self.source.from(pos)?;
        let tag_len = Tag::prefix_len(tail)?;
        let end = self.source.advance(pos, tag_len);
        let raw = self.source.get(pos..end)?;
        Some(ParsedAtom::new(NoteFieldValue::String(raw.to_owned()), end))
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
                matches!(
                    result,
                    Some(ParsedAtom {
                        value: NoteFieldValue::Null,
                        end: 4
                    })
                ),
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
                    Some(ParsedAtom { value: NoteFieldValue::String(s), end: 5 })
                        if s == "#book"
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
                    Some(ParsedAtom { value: NoteFieldValue::String(s), .. })
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
                    Some(ParsedAtom { value: NoteFieldValue::Duration(dv), .. })
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
                    Some(ParsedAtom { value: NoteFieldValue::Duration(dv), .. })
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
            assert_eq!(
                result,
                Some(ParsedAtom::new(NoteFieldValue::Date(expected), 10))
            );
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
