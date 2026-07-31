//! Byte-offset helpers for parser-owned UTF-8 source text.

use std::ops::Range;

/// Borrowed UTF-8 source text with byte-offset helpers.
pub(crate) struct SourceText<'a>(&'a str);

impl<'a> SourceText<'a> {
    /// Wraps source text for byte-offset operations.
    #[inline]
    #[must_use]
    pub(crate) fn new(source: &'a str) -> Self {
        Self(source)
    }

    /// Source length in bytes.
    #[inline]
    #[must_use]
    pub(crate) fn len(&self) -> usize {
        self.0.len()
    }

    /// Returns the suffix beginning at `pos`.
    #[inline]
    #[must_use]
    pub(crate) fn from(&self, pos: usize) -> Option<&'a str> {
        self.0.get(pos..)
    }

    /// Returns the source slice for `range`.
    #[inline]
    #[must_use]
    pub(crate) fn get(&self, range: Range<usize>) -> Option<&'a str> {
        self.0.get(range)
    }

    /// Returns whether the source at `pos` starts with `needle`.
    #[inline]
    #[must_use]
    pub(crate) fn starts_with(&self, pos: usize, needle: &str) -> bool {
        self.from(pos).is_some_and(|source| source.starts_with(needle))
    }

    /// Adds `bytes` to `pos`.
    #[inline]
    #[must_use]
    #[expect(
        clippy::arithmetic_side_effects,
        reason = "parser byte offsets are derived from valid string slices"
    )]
    pub(crate) fn advance(&self, pos: usize, bytes: usize) -> usize {
        debug_assert!(self.0.is_char_boundary(pos));
        pos + bytes
    }

    /// Advances `pos` past `ch`.
    #[inline]
    #[must_use]
    pub(crate) fn advance_char(&self, pos: usize, ch: char) -> usize {
        self.advance(pos, ch.len_utf8())
    }

    /// Returns the end offset for a token ending with `ch`.
    ///
    /// # Arguments
    ///
    /// * `pos` - Token start offset.
    /// * `offset` - Character offset within the token.
    /// * `ch` - Final token character.
    #[inline]
    #[must_use]
    pub(crate) fn token_end(
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
