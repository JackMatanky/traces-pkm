//! Byte-offset helpers for parser-owned UTF-8 source text.

use std::ops::Range;

use serde::{Deserialize, Serialize};

/// Byte offsets into a UTF-8 source string.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub(crate) struct ByteSpan {
    start: usize,
    end: usize,
}

impl ByteSpan {
    /// Creates a range from start and end byte offsets.
    #[inline]
    #[must_use]
    pub(crate) fn new(start: usize, end: usize) -> Self {
        Self {
            start,
            end,
        }
    }

    /// Start byte offset.
    #[inline]
    #[must_use]
    pub(crate) fn start(&self) -> usize {
        self.start
    }

    /// End byte offset.
    #[inline]
    #[must_use]
    pub(crate) fn end(&self) -> usize {
        self.end
    }

    /// Returns a [`Range`] for slicing source text.
    #[inline]
    #[must_use]
    pub(crate) fn range(&self) -> Range<usize> {
        self.start..self.end
    }
}

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

    /// Finds `needle` at or after `pos` and returns an absolute byte offset.
    #[inline]
    #[must_use]
    pub(crate) fn find_char_from(
        &self,
        pos: usize,
        needle: char,
    ) -> Option<usize> {
        self.from(pos)?.find(needle).map(|offset| self.advance(pos, offset))
    }

    /// Finds `needle` at or after `pos` and returns an absolute byte offset.
    #[inline]
    #[must_use]
    pub(crate) fn find_str_from(
        &self,
        pos: usize,
        needle: &str,
    ) -> Option<usize> {
        self.from(pos)?.find(needle).map(|offset| self.advance(pos, offset))
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
