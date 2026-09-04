//! Source-position primitives shared across text-parsing domains.
//!
//! [`SourceLine`] and [`ByteOffset`] are distinct newtypes so a byte offset
//! can never be mistaken for a line number at compile time. Domain-specific
//! parsers (Markdown notes, config files, templates) convert between the two
//! with their own local tracking strategy; only the vocabulary lives here.

use std::fmt;

use serde::{Deserialize, Serialize};

/// A 1-indexed source line number.
///
/// Distinct from [`ByteOffset`] so a byte offset can never be passed where a
/// line number is expected, or vice versa.
#[derive(
    Copy,
    Clone,
    Debug,
    Default,
    Eq,
    Hash,
    Ord,
    PartialEq,
    PartialOrd,
    Deserialize,
    Serialize,
)]
pub struct SourceLine(u32);

impl SourceLine {
    /// Wraps `line` as a 1-indexed source line number.
    #[inline]
    #[must_use]
    pub const fn new(line: u32) -> Self {
        Self(line)
    }

    /// Returns the 1-indexed line number as a `u32`.
    #[inline]
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

impl fmt::Display for SourceLine {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

impl From<SourceLine> for u32 {
    #[inline]
    fn from(line: SourceLine) -> Self {
        line.0
    }
}

impl From<u32> for SourceLine {
    #[inline]
    fn from(line: u32) -> Self {
        Self::new(line)
    }
}

/// A UTF-8 byte offset into source text.
///
/// Distinct from [`SourceLine`] so a line number can never be passed where a
/// byte offset is expected, or vice versa.
#[derive(
    Copy,
    Clone,
    Debug,
    Default,
    Eq,
    Hash,
    Ord,
    PartialEq,
    PartialOrd,
    Deserialize,
    Serialize,
)]
pub(crate) struct ByteOffset(usize);

impl ByteOffset {
    /// Wraps `offset` as a byte offset.
    #[inline]
    #[must_use]
    pub(crate) const fn new(offset: usize) -> Self {
        Self(offset)
    }
}

impl From<usize> for ByteOffset {
    #[inline]
    fn from(offset: usize) -> Self {
        Self::new(offset)
    }
}

impl From<ByteOffset> for usize {
    #[inline]
    fn from(offset: ByteOffset) -> Self {
        offset.0
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn source_line_displays_as_its_numeric_value() {
        assert_eq!(SourceLine::new(7).to_string(), "7");
    }

    #[test]
    fn source_line_conversions_and_accessors() {
        let line = SourceLine::from(42u32);
        assert_eq!(u32::from(line), 42);
    }

    #[test]
    fn byte_offset_conversions_and_accessors() {
        let offset = ByteOffset::from(128usize);
        assert_eq!(usize::from(offset), 128);
        assert_eq!(offset, ByteOffset::new(128));
    }
}
