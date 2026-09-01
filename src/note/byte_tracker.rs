//! Byte-offset to source-line conversion for Markdown parsing.
//!
//! [`ByteTracker`] precomputes line-start byte offsets once per document and
//! converts UTF-8 byte offsets into 1-indexed [`SourceLine`] values in
//! O(log n) via [`slice::partition_point`]. [`ByteOffset`] and [`SourceLine`]
//! are distinct newtypes so a byte offset can never be mistaken for a line
//! number at compile time.

use std::fmt;

use serde::{Deserialize, Serialize};

/// Converts UTF-8 byte offsets into 1-indexed source line numbers.
///
/// Precomputes line-start byte offsets once per document; each conversion is
/// an O(log n) binary search over them via [`slice::partition_point`].
pub(super) struct ByteTracker {
    /// Byte offset of the first character of each line, ascending; always
    /// starts with `0` for line 1.
    line_starts: Box<[usize]>,
}

impl ByteTracker {
    /// Precomputes line-start offsets for `source`.
    #[inline]
    #[must_use]
    pub(super) fn new(source: &str) -> Self {
        let mut line_starts = vec![0];
        line_starts.extend(
            source
                .match_indices('\n')
                .map(|(offset, _)| offset.saturating_add(1)),
        );
        Self {
            line_starts: line_starts.into_boxed_slice(),
        }
    }

    /// Converts a byte offset into its 1-indexed source line.
    ///
    /// An offset beyond the source length resolves to the last line.
    #[inline]
    #[must_use]
    pub(super) fn byte_to_line(&self, offset: ByteOffset) -> SourceLine {
        let offset = usize::from(offset);
        let line = self.line_starts.partition_point(|&start| start <= offset);
        SourceLine::new(u32::try_from(line).unwrap_or(u32::MAX))
    }
}

/// A 1-indexed source line number.
///
/// Distinct from [`ByteOffset`] so a byte offset can never be passed where a
/// line number is expected, or vice versa.
#[derive(
    Copy,
    Clone,
    Debug,
    Eq,
    Hash,
    Ord,
    PartialEq,
    PartialOrd,
    Deserialize,
    Serialize,
)]
pub(crate) struct SourceLine(u32);

impl SourceLine {
    /// Wraps `line` as a 1-indexed source line number.
    #[inline]
    #[must_use]
    pub(super) const fn new(line: u32) -> Self {
        Self(line)
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

/// A UTF-8 byte offset into Markdown source text.
///
/// Distinct from [`SourceLine`] so a line number can never be passed where a
/// byte offset is expected, or vice versa.
#[derive(Copy, Clone)]
pub(super) struct ByteOffset(usize);

impl ByteOffset {
    /// Wraps `offset` as a byte offset.
    #[inline]
    #[must_use]
    pub(super) const fn new(offset: usize) -> Self {
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
    use super::*;

    mod byte_to_line {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn resolves_any_offset_in_single_line_source_to_line_one() {
            let tracker = ByteTracker::new("no newlines here");

            assert_eq!(
                tracker.byte_to_line(ByteOffset::new(0)),
                SourceLine::new(1)
            );
            assert_eq!(
                tracker.byte_to_line(ByteOffset::new(10)),
                SourceLine::new(1)
            );
        }

        #[test]
        fn resolves_offsets_within_each_line_of_multi_line_source() {
            let tracker = ByteTracker::new("one\ntwo\nthree");

            assert_eq!(
                tracker.byte_to_line(ByteOffset::new(0)),
                SourceLine::new(1),
                "start of line 1"
            );
            assert_eq!(
                tracker.byte_to_line(ByteOffset::new(2)),
                SourceLine::new(1),
                "mid line 1"
            );
            assert_eq!(
                tracker.byte_to_line(ByteOffset::new(4)),
                SourceLine::new(2),
                "start of line 2"
            );
            assert_eq!(
                tracker.byte_to_line(ByteOffset::new(8)),
                SourceLine::new(3),
                "start of line 3"
            );
            assert_eq!(
                tracker.byte_to_line(ByteOffset::new(12)),
                SourceLine::new(3),
                "last byte of line 3"
            );
        }

        #[test]
        fn counts_empty_lines_as_separate_lines() {
            let tracker = ByteTracker::new("one\n\nthree");

            assert_eq!(
                tracker.byte_to_line(ByteOffset::new(4)),
                SourceLine::new(2),
                "the empty line"
            );
            assert_eq!(
                tracker.byte_to_line(ByteOffset::new(5)),
                SourceLine::new(3)
            );
        }

        #[test]
        fn resolves_empty_source_to_line_one() {
            let tracker = ByteTracker::new("");

            assert_eq!(
                tracker.byte_to_line(ByteOffset::new(0)),
                SourceLine::new(1)
            );
        }

        #[test]
        fn resolves_an_offset_beyond_source_length_to_the_last_line() {
            let tracker = ByteTracker::new("one\ntwo\nthree");

            assert_eq!(
                tracker.byte_to_line(ByteOffset::new(1000)),
                SourceLine::new(3)
            );
        }
    }

    mod source_line {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn displays_as_its_numeric_value() {
            assert_eq!(SourceLine::new(7).to_string(), "7");
        }
    }
}
