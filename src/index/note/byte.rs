use std::ops::Range;

use serde::{Deserialize, Serialize};

/// Byte range into a UTF-8 source string.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub(crate) struct ByteRange {
    start: usize,
    end: usize,
}

impl ByteRange {
    /// Creates a byte range from source offsets.
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

    /// Standard library range for slicing.
    #[inline]
    #[must_use]
    pub(crate) fn range(&self) -> Range<usize> {
        self.start..self.end
    }
}

/// UTF-8 byte-offset helper for parser-local source text.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct ByteSource<'a>(&'a str);

impl<'a> ByteSource<'a> {
    /// Wraps parser source text.
    #[inline]
    #[must_use]
    pub(crate) fn new(source: &'a str) -> Self {
        Self(source)
    }

    /// Source byte length.
    #[inline]
    #[must_use]
    pub(crate) fn len(&self) -> usize {
        self.0.len()
    }

    /// Returns the suffix starting at `pos`.
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

    /// Advances `pos` by a validated byte count.
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

    /// Returns the byte offset immediately after `ch` at `pos`.
    #[inline]
    #[must_use]
    pub(crate) fn advance_char(&self, pos: usize, ch: char) -> usize {
        self.advance(pos, ch.len_utf8())
    }

    /// Returns the absolute end offset for a char-indexed token.
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
