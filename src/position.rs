//! Source-position primitives and byte-to-line conversion shared across
//! text-parsing domains.
//!
//! Main types:
//! - [`ByteTracker`] - Converts byte offsets into [`SourceLine`]s over
//!   precomputed line starts.
//! - [`ByteOffset`] - UTF-8 byte offset into source text.
//! - [`ByteOffsetError`] - Error for byte offsets that exceed the `u32` range.
//! - [`SourceLine`] - 1-indexed source line number.
//! - [`SourceLineError`] - Error for invalid line-number conversions.
//!
//! [`SourceLine`] and [`ByteOffset`] are distinct newtypes so a byte offset can
//! never be mistaken for a line number at compile time. [`ByteTracker`] is the
//! shared conversion infrastructure: a domain-specific parser (Markdown notes,
//! config files, templates) precomputes line starts once per document and
//! resolves any offset with an O(log n) lookup; only the choice of when to
//! build a tracker stays local to each parser.

use std::{fmt, num::NonZeroU32};

use serde::{Deserialize, Serialize};

/// Converts UTF-8 byte offsets into 1-indexed source line numbers.
///
/// Precomputes line-start byte offsets once per document; each conversion is an
/// O(log n) binary search over them via [`slice::partition_point`].
pub(crate) struct ByteTracker {
    /// Byte offset of the first character of each line, ascending; always
    /// starts with `0` for line 1.
    line_starts: Box<[usize]>,
}

impl ByteTracker {
    /// Precomputes line-start offsets for `source`.
    #[inline]
    #[must_use]
    pub(crate) fn new(source: &str) -> Self {
        let mut line_starts = Vec::with_capacity((source.len() / 32).max(16));
        line_starts.push(0);
        for (offset, _) in source.match_indices('\n') {
            line_starts.push(offset.saturating_add(1));
        }
        Self {
            line_starts: line_starts.into_boxed_slice(),
        }
    }

    /// Converts a byte offset into its 1-indexed source line.
    ///
    /// An offset beyond the source length resolves to the last line.
    #[inline]
    #[must_use]
    #[expect(
        clippy::expect_used,
        reason = "partition_point always returns >= 1, so line is non-zero"
    )]
    pub(crate) fn line_at(&self, offset: ByteOffset) -> SourceLine {
        let offset = usize::from(offset);
        let line = self.line_starts.partition_point(|&start| start <= offset);
        SourceLine::new(u32::try_from(line).unwrap_or(u32::MAX))
            .expect("line number is always non-zero")
    }
}

/// A UTF-8 byte offset into source text.
///
/// Distinct from [`SourceLine`] so a line number can never be passed where a
/// byte offset is expected, or vice versa. Backed by `u32`, so offsets past
/// 4 `GiB` cannot be represented: saturating conversions clamp at
/// [`ByteOffset::MAX`].
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
pub(crate) struct ByteOffset(u32);

impl ByteOffset {
    /// The largest representable byte offset, used to saturate conversions that
    /// would exceed the `u32` range.
    pub const MAX: Self = Self(u32::MAX);

    /// Wraps `offset` as a UTF-8 byte offset into source text.
    #[inline]
    #[must_use]
    pub(crate) const fn new(offset: u32) -> Self {
        Self(offset)
    }

    /// Narrows `offset` to a UTF-8 byte offset, clamping values beyond the
    /// `u32` range at [`ByteOffset::MAX`].
    ///
    /// This is the saturating counterpart of the fallible [`TryFrom`]
    /// conversion; use it at boundaries where oversized input must not fail.
    #[inline]
    #[must_use]
    pub(crate) fn saturating_from(offset: usize) -> Self {
        Self::try_from(offset).unwrap_or(Self::MAX)
    }
}

impl From<u32> for ByteOffset {
    #[inline]
    fn from(offset: u32) -> Self {
        Self::new(offset)
    }
}

impl From<ByteOffset> for u32 {
    #[inline]
    fn from(offset: ByteOffset) -> Self {
        offset.0
    }
}

impl From<ByteOffset> for usize {
    /// Widens to `usize`, saturating at `usize::MAX` on targets narrower than
    /// 32 bits.
    #[inline]
    fn from(offset: ByteOffset) -> Self {
        Self::try_from(offset.0).unwrap_or(Self::MAX)
    }
}

/// Narrows a `usize` position to [`ByteOffset`].
///
/// # Errors
///
/// Returns [`ByteOffsetError`] when `offset` exceeds `u32::MAX`.
impl TryFrom<usize> for ByteOffset {
    type Error = ByteOffsetError;

    #[inline]
    fn try_from(offset: usize) -> Result<Self, Self::Error> {
        u32::try_from(offset).map(Self).map_err(|_| ByteOffsetError)
    }
}

/// Error returned when a byte offset exceeds the `u32` range.
#[derive(Copy, Clone, Debug, Eq, PartialEq, thiserror::Error)]
#[error("byte offset exceeds u32 range")]
pub struct ByteOffsetError;

/// A 1-indexed source line number.
///
/// Distinct from `ByteOffset` so a byte offset can never be passed where a line
/// number is expected, or vice versa.
///
/// # Examples
///
/// ```
/// use traces_pkm::SourceLine;
///
/// let line = SourceLine::new(42).expect("non-zero");
/// assert_eq!(line.get(), 42);
/// assert_eq!(line.to_string(), "42");
/// ```
#[derive(Copy, Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SourceLine(NonZeroU32);

impl SourceLine {
    /// The minimum valid line number (1).
    pub const MIN: Self = Self(NonZeroU32::MIN);

    /// Wraps `line` as a 1-indexed source line number.
    ///
    /// Returns `None` if `line` is zero.
    ///
    /// # Examples
    ///
    /// ```
    /// use traces_pkm::SourceLine;
    ///
    /// assert!(SourceLine::new(1).is_some());
    /// assert!(SourceLine::new(0).is_none());
    /// ```
    #[inline]
    #[must_use]
    pub const fn new(line: u32) -> Option<Self> {
        match NonZeroU32::new(line) {
            Some(nz) => Some(Self(nz)),
            None => None,
        }
    }

    /// Returns the line number as a `u32`.
    #[inline]
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0.get()
    }
}

impl From<SourceLine> for u32 {
    #[inline]
    fn from(line: SourceLine) -> Self {
        line.0.get()
    }
}

impl TryFrom<u32> for SourceLine {
    type Error = SourceLineError;

    #[inline]
    fn try_from(line: u32) -> Result<Self, Self::Error> {
        Self::new(line).ok_or(SourceLineError)
    }
}

impl fmt::Display for SourceLine {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

impl Serialize for SourceLine {
    #[inline]
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        self.0.get().serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for SourceLine {
    #[inline]
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Self, D::Error> {
        let line = u32::deserialize(deserializer)?;
        Self::new(line).ok_or_else(|| serde::de::Error::custom(SourceLineError))
    }
}

/// Error returned when converting a zero value to [`SourceLine`].
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
#[error("source line number must be non-zero")]
pub struct SourceLineError;

#[cfg(test)]
mod tests {
    use super::*;

    mod source_line {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn source_line_displays_as_its_numeric_value() {
            assert_eq!(SourceLine::new(7).expect("non-zero").to_string(), "7");
        }

        #[test]
        fn source_line_conversions_and_accessors() {
            let line = SourceLine::try_from(42u32).expect("non-zero");
            assert_eq!(u32::from(line), 42);
            assert_eq!(line.get(), 42);
        }

        #[test]
        fn source_line_rejects_zero() {
            assert!(SourceLine::new(0).is_none());
            assert!(SourceLine::try_from(0u32).is_err());
        }
    }

    mod byte_offset {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn round_trips_u32_boundary_values() {
            for value in [0_u32, 128, u32::MAX] {
                let offset = ByteOffset::from(value);
                assert_eq!(u32::from(offset), value);
                assert_eq!(
                    usize::from(offset),
                    usize::try_from(value).expect("u32 always fits in usize")
                );
                assert_eq!(offset, ByteOffset::new(value));
            }
        }

        #[test]
        fn narrows_from_usize_within_u32_range() {
            for value in [0_usize, 1_048_576] {
                let offset =
                    ByteOffset::try_from(value).expect("within u32 range");
                let expected = u32::try_from(value).expect("value fits u32");
                assert_eq!(offset, ByteOffset::from(expected));
            }
        }

        #[test]
        fn returns_max_when_narrowing_widened_u32_max() {
            let widened =
                usize::try_from(u32::MAX).expect("u32 always fits in usize");

            let offset =
                ByteOffset::try_from(widened).expect("u32::MAX narrows");

            assert_eq!(offset, ByteOffset::MAX);
        }

        #[test]
        fn returns_byte_offset_error_when_usize_exceeds_u32_max() {
            let oversized = usize::try_from(u32::MAX)
                .expect("u32 always fits in usize")
                .saturating_add(1);

            let error =
                ByteOffset::try_from(oversized).expect_err("beyond u32 range");

            assert!(matches!(error, ByteOffsetError));
        }

        #[test]
        fn displays_exceeds_u32_range_message() {
            assert_eq!(
                ByteOffsetError.to_string(),
                "byte offset exceeds u32 range"
            );
        }

        #[test]
        fn saturates_oversized_offset_to_max() {
            let oversized = usize::try_from(u32::MAX)
                .expect("u32 always fits in usize")
                .saturating_add(1);

            assert_eq!(ByteOffset::saturating_from(oversized), ByteOffset::MAX);
            assert_eq!(ByteOffset::saturating_from(0), ByteOffset::from(0_u32));
        }
    }

    mod byte_tracker {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn resolves_any_offset_in_single_line_source_to_line_one() {
            let tracker = ByteTracker::new("no newlines here");

            assert_eq!(tracker.line_at(ByteOffset::new(0)), SourceLine::MIN);
            assert_eq!(tracker.line_at(ByteOffset::new(10)), SourceLine::MIN);
        }

        #[test]
        fn resolves_offsets_within_each_line_of_multi_line_source() {
            let tracker = ByteTracker::new("one\ntwo\nthree");

            assert_eq!(
                tracker.line_at(ByteOffset::new(0)),
                SourceLine::MIN,
                "start of line 1"
            );
            assert_eq!(
                tracker.line_at(ByteOffset::new(2)),
                SourceLine::MIN,
                "mid line 1"
            );
            assert_eq!(
                tracker.line_at(ByteOffset::new(4)),
                SourceLine::new(2).expect("non-zero"),
                "start of line 2"
            );
            assert_eq!(
                tracker.line_at(ByteOffset::new(8)),
                SourceLine::new(3).expect("non-zero"),
                "start of line 3"
            );
            assert_eq!(
                tracker.line_at(ByteOffset::new(12)),
                SourceLine::new(3).expect("non-zero"),
                "last byte of line 3"
            );
        }

        #[test]
        fn counts_empty_lines_as_separate_lines() {
            let tracker = ByteTracker::new("one\n\nthree");

            assert_eq!(
                tracker.line_at(ByteOffset::new(4)),
                SourceLine::new(2).expect("non-zero"),
                "the empty line"
            );
            assert_eq!(
                tracker.line_at(ByteOffset::new(5)),
                SourceLine::new(3).expect("non-zero")
            );
        }

        #[test]
        fn resolves_empty_source_to_line_one() {
            let tracker = ByteTracker::new("");

            assert_eq!(tracker.line_at(ByteOffset::new(0)), SourceLine::MIN);
        }

        #[test]
        fn resolves_an_offset_beyond_source_length_to_the_last_line() {
            let tracker = ByteTracker::new("one\ntwo\nthree");

            assert_eq!(
                tracker.line_at(ByteOffset::new(1000)),
                SourceLine::new(3).expect("non-zero")
            );
        }

        #[test]
        fn resolves_offset_past_trailing_newline_to_final_line() {
            let tracker = ByteTracker::new("one\n");

            assert_eq!(
                tracker.line_at(ByteOffset::new(4)),
                SourceLine::new(2).expect("non-zero")
            );
        }
    }
}
