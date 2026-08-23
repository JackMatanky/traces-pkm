//! UTF-8 byte-offset helpers for the parser and wikilink parser.
//!
//! [`SourceText`] wraps a borrowed string and exposes validated byte-offset
//! arithmetic used by the lexer and [`Link`](super::Link) parser.

use std::ops::Range;

/// Borrowed parser input addressed by validated byte offsets.
pub(super) struct SourceText<'a>(&'a str);

impl<'a> SourceText<'a> {
    /// Creates byte-offset helpers for `source`.
    #[inline]
    #[must_use]
    pub(super) const fn new(source: &'a str) -> Self {
        Self(source)
    }

    /// Returns the source length in bytes.
    #[inline]
    #[must_use]
    pub(super) const fn len(&self) -> usize {
        self.0.len()
    }

    /// Returns the suffix beginning at `pos`, or `None` if out of bounds.
    #[inline]
    #[must_use]
    pub(super) fn from(&self, pos: usize) -> Option<&'a str> {
        self.0.get(pos..)
    }

    /// Returns the source slice for `range`, or `None` if out of bounds.
    #[inline]
    #[must_use]
    pub(super) fn get(&self, range: Range<usize>) -> Option<&'a str> {
        self.0.get(range)
    }

    /// Returns `true` if the source at `pos` starts with `needle`.
    #[inline]
    #[must_use]
    pub(super) fn starts_with(&self, pos: usize, needle: &str) -> bool {
        self.from(pos).is_some_and(|source| source.starts_with(needle))
    }

    /// Advances `pos` by `bytes` and returns the new offset.
    #[inline]
    #[must_use]
    #[expect(
        clippy::arithmetic_side_effects,
        reason = "parser byte offsets are derived from valid string slices"
    )]
    pub(super) fn advance(&self, pos: usize, bytes: usize) -> usize {
        debug_assert!(self.0.is_char_boundary(pos));
        pos + bytes
    }

    /// Advances `pos` past one `char` and returns the new offset.
    #[inline]
    #[must_use]
    pub(super) fn advance_char(&self, pos: usize, ch: char) -> usize {
        self.advance(pos, ch.len_utf8())
    }

    /// Calculates the byte offset just after a token-ending character.
    ///
    /// `pos` is the token start, `offset` is the character offset within that
    /// token, and `ch` is the token's final character.
    #[inline]
    #[must_use]
    pub(super) fn token_end(
        &self,
        pos: usize,
        offset: usize,
        ch: char,
    ) -> usize {
        self.advance(self.advance(pos, offset), ch.len_utf8())
    }
}

impl AsRef<str> for SourceText<'_> {
    #[inline]
    fn as_ref(&self) -> &str {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn len_returns_the_byte_length() {
        assert_eq!(SourceText::new("hello").len(), 5);
        assert_eq!(SourceText::new("").len(), 0);
    }

    #[test]
    fn starts_with_matches_at_the_given_position() {
        let source = SourceText::new("hello world");

        assert!(source.starts_with(6, "world"));
        assert!(source.starts_with(0, "hello"));
    }

    #[test]
    fn starts_with_returns_false_when_needle_does_not_match() {
        let source = SourceText::new("hello world");

        assert!(!source.starts_with(0, "goodbye"));
    }

    #[test]
    fn as_ref_returns_the_inner_string() {
        let source = SourceText::new("test content");

        assert_eq!(source.as_ref(), "test content");
    }
}
