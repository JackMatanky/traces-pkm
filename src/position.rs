//! Source-position primitives and byte-to-line conversion shared across
//! text-parsing domains.
//!
//! # Key Types
//!
//! - [`LineIndex`]: precomputed line starts providing O(log n) byte-to-line
//!   conversions.
//! - [`BytePos`]: 0-indexed UTF-8 byte position into source text.
//! - [`ByteSpan`]: half-open `[start..end)` byte range in source text.
//! - [`SpanStart`]: open span anchor awaiting its closing boundary.
//! - [`SourceLine`]: 1-indexed source line number.
//! - [`PositionError`]: errors from invalid position or line conversions.
//! - [`Spanned`]: value paired with its [`ByteSpan`] in source text.
//!
//! # Examples
//!
//! ```
//! use traces_pkm::SourceLine;
//!
//! let line = SourceLine::new(42).expect("valid line number");
//! assert_eq!(line.get(), 42);
//! ```

use std::{cmp::Ordering, fmt, num::NonZeroU32, ops::Range};

use serde::{Deserialize, Serialize};

/// Precomputed line-start byte positions providing O(log n) line lookups.
///
/// Precomputes line-start byte positions once per document; each conversion is
/// an O(log n) binary search over them via [`slice::partition_point`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct LineIndex(Box<[BytePos]>);

impl LineIndex {
    /// Precomputes line-start positions for `source`.
    #[inline]
    #[must_use]
    pub(crate) fn new(source: &str) -> Self {
        let mut line_starts = Vec::with_capacity((source.len() / 32).max(16));
        line_starts.push(BytePos::new(0));
        for (pos, _) in source.match_indices('\n') {
            line_starts
                .push(BytePos::saturating_from(pos.saturating_add(1)));
        }
        Self(line_starts.into_boxed_slice())
    }

    /// Converts a byte position into its 1-indexed source line.
    ///
    /// A position beyond the source length resolves to the last line.
    #[inline]
    #[must_use]
    #[expect(
        clippy::expect_used,
        reason = "partition_point always returns >= 1, so line is non-zero"
    )]
    pub(crate) fn line_at(&self, pos: BytePos) -> SourceLine {
        let line = self.0.partition_point(|&start| start <= pos);
        SourceLine::new(u32::try_from(line).unwrap_or(u32::MAX))
            .expect("line number is always non-zero")
    }

    /// Converts a byte span into its starting and ending source lines.
    #[cfg(test)]
    #[inline]
    #[must_use]
    pub(crate) fn lines_of(&self, span: ByteSpan) -> (SourceLine, SourceLine) {
        (self.line_at(span.start()), self.line_at(span.end()))
    }
}

/// An immutable, half-open `[start..end)` UTF-8 byte span in source text.
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
pub(crate) struct ByteSpan {
    start: BytePos,
    end: BytePos,
}

impl ByteSpan {
    /// Constructs a span, enforcing the invariant `start <= end`.
    #[inline]
    #[must_use]
    pub(crate) fn new(start: BytePos, end: BytePos) -> Self {
        Self {
            start,
            end: end.max(start),
        }
    }

    /// Returns the starting byte position.
    #[inline]
    #[must_use]
    pub(crate) const fn start(self) -> BytePos {
        self.start
    }

    /// Returns the exclusive ending byte position.
    #[inline]
    #[must_use]
    pub(crate) const fn end(self) -> BytePos {
        self.end
    }

    /// Returns the starting byte position as `usize`.
    #[inline]
    #[must_use]
    pub(crate) fn start_usize(self) -> usize {
        usize::from(self.start)
    }

    /// Returns the exclusive ending byte position as `usize`.
    #[inline]
    #[must_use]
    pub(crate) fn end_usize(self) -> usize {
        usize::from(self.end)
    }

    /// Returns the length of the span in bytes.
    #[cfg(test)]
    #[inline]
    #[must_use]
    pub(crate) fn len(self) -> usize {
        usize::from(self.end).saturating_sub(usize::from(self.start))
    }

    /// Returns `true` if the span is empty (`start == end`).
    #[cfg(test)]
    #[inline]
    #[must_use]
    pub(crate) fn is_empty(self) -> bool {
        self.start >= self.end
    }

    /// Converts the span into a `Range<usize>`.
    #[inline]
    #[must_use]
    pub(crate) fn to_range(self) -> Range<usize> {
        self.start_usize()..self.end_usize()
    }
}

impl From<ByteSpan> for Range<usize> {
    #[inline]
    fn from(span: ByteSpan) -> Self {
        span.to_range()
    }
}

impl From<ByteSpan> for Range<BytePos> {
    #[inline]
    fn from(span: ByteSpan) -> Self {
        span.start..span.end
    }
}

impl From<Range<BytePos>> for ByteSpan {
    #[inline]
    fn from(range: Range<BytePos>) -> Self {
        Self::new(range.start, range.end)
    }
}

impl From<Range<usize>> for ByteSpan {
    #[inline]
    fn from(range: Range<usize>) -> Self {
        Self::new(
            BytePos::saturating_from(range.start),
            BytePos::saturating_from(range.end),
        )
    }
}

/// A 0-indexed UTF-8 byte position into source text.
///
/// Distinct from [`SourceLine`] so a line number can never be passed where a
/// byte position is expected, or vice versa. Backed by `u32`, so positions past
/// 4 `GiB` cannot be represented: saturating conversions clamp at
/// [`BytePos::MAX`].
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
pub(crate) struct BytePos(u32);

impl BytePos {
    /// The largest representable byte position, used to saturate conversions
    /// that would exceed the `u32` range.
    pub const MAX: Self = Self(u32::MAX);

    /// Wraps `pos` as a UTF-8 byte position into source text.
    #[inline]
    #[must_use]
    pub(crate) const fn new(pos: u32) -> Self {
        Self(pos)
    }

    /// Narrows `pos` to a UTF-8 byte position, clamping values beyond the `u32`
    /// range at [`BytePos::MAX`].
    ///
    /// This is the saturating counterpart of the fallible [`TryFrom`]
    /// conversion; use it at boundaries where oversized input must not fail.
    #[inline]
    #[must_use]
    pub(crate) fn saturating_from(pos: usize) -> Self {
        Self::try_from(pos).unwrap_or(Self::MAX)
    }

    /// Converts this position into an open span anchor.
    #[cfg(test)]
    #[inline]
    #[must_use]
    pub(crate) const fn to_start(self) -> SpanStart {
        SpanStart(self)
    }
}

impl From<u32> for BytePos {
    #[inline]
    fn from(pos: u32) -> Self {
        Self::new(pos)
    }
}

impl From<BytePos> for u32 {
    #[inline]
    fn from(pos: BytePos) -> Self {
        pos.0
    }
}

impl From<BytePos> for usize {
    /// Widens to `usize`, saturating at `usize::MAX` on targets narrower than
    /// 32 bits.
    #[inline]
    fn from(pos: BytePos) -> Self {
        Self::try_from(pos.0).unwrap_or(Self::MAX)
    }
}

/// Narrows a `usize` position to [`BytePos`].
///
/// # Errors
///
/// - [`PositionError::BytePosOverflow`] if `pos` exceeds `u32::MAX`.
impl TryFrom<usize> for BytePos {
    type Error = PositionError;

    #[inline]
    fn try_from(pos: usize) -> Result<Self, Self::Error> {
        u32::try_from(pos).map(Self).map_err(|_| PositionError::BytePosOverflow)
    }
}

/// An open span anchor whose start position is known, awaiting its closing
/// boundary.
#[derive(Copy, Clone, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) struct SpanStart(BytePos);

impl SpanStart {
    /// Creates a new open span anchor starting at `pos`.
    #[inline]
    #[must_use]
    pub(crate) const fn at(pos: BytePos) -> Self {
        Self(pos)
    }

    /// Closes the span at `end`, producing an immutable [`ByteSpan`].
    #[inline]
    #[must_use]
    pub(crate) fn close(self, end: BytePos) -> ByteSpan {
        ByteSpan::new(self.0, end)
    }

    /// Returns the start position.
    #[cfg(test)]
    #[inline]
    #[must_use]
    pub(crate) const fn pos(self) -> BytePos {
        self.0
    }
}

impl From<BytePos> for SpanStart {
    #[inline]
    fn from(pos: BytePos) -> Self {
        Self::at(pos)
    }
}

/// A 1-indexed source line number.
///
/// Distinct from `BytePos` so a byte position can never be passed where a line
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

/// Attempts to convert a `u32` into a 1-indexed [`SourceLine`].
///
/// # Errors
///
/// - [`PositionError::ZeroSourceLine`] if `line` is zero.
impl TryFrom<u32> for SourceLine {
    type Error = PositionError;

    #[inline]
    fn try_from(line: u32) -> Result<Self, Self::Error> {
        Self::new(line).ok_or(PositionError::ZeroSourceLine)
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
        Self::new(line).ok_or_else(|| {
            serde::de::Error::custom(PositionError::ZeroSourceLine)
        })
    }
}

/// A value paired with its [`ByteSpan`] in source text.
#[derive(Clone, Debug, Default, Eq, Hash, PartialEq)]
pub(crate) struct Spanned<T> {
    value: T,
    span: ByteSpan,
}

impl<T> Spanned<T> {
    /// Pairs `value` with its byte `span`.
    #[inline]
    #[must_use]
    pub(crate) const fn new(value: T, span: ByteSpan) -> Self {
        Self {
            value,
            span,
        }
    }

    /// Convenience constructor converting a raw `Range<usize>`.
    #[inline]
    #[must_use]
    pub(crate) fn from_usize_range(value: T, range: Range<usize>) -> Self {
        Self::new(value, ByteSpan::from(range))
    }

    /// Returns a reference to the inner value.
    #[inline]
    #[must_use]
    pub(crate) const fn value(&self) -> &T {
        &self.value
    }

    /// Returns the [`ByteSpan`] in source text.
    #[inline]
    #[must_use]
    pub(crate) const fn span(&self) -> ByteSpan {
        self.span
    }

    /// Returns the half-open byte range as `Range<usize>`.
    #[inline]
    #[must_use]
    pub(crate) fn span_usize(&self) -> Range<usize> {
        self.span.to_range()
    }

    /// Returns the starting byte position.
    #[cfg(test)]
    #[inline]
    #[must_use]
    pub(crate) const fn start(&self) -> BytePos {
        self.span.start()
    }

    /// Returns the exclusive ending byte position.
    #[cfg(test)]
    #[inline]
    #[must_use]
    pub(crate) const fn end(&self) -> BytePos {
        self.span.end()
    }

    /// Returns the starting byte position as `usize`.
    #[inline]
    #[must_use]
    pub(crate) fn start_usize(&self) -> usize {
        self.span.start_usize()
    }

    /// Returns the exclusive ending byte position as `usize`.
    #[inline]
    #[must_use]
    pub(crate) fn end_usize(&self) -> usize {
        self.span.end_usize()
    }

    /// Consumes the wrapper, returning the inner value.
    #[inline]
    #[must_use]
    pub(crate) fn into_value(self) -> T {
        self.value
    }

    /// Decomposes the wrapper into value and span.
    #[inline]
    #[must_use]
    pub(crate) fn into_parts(self) -> (T, ByteSpan) {
        (self.value, self.span)
    }

    /// Decomposes the wrapper into value and `Range<usize>`.
    #[inline]
    #[must_use]
    pub(crate) fn into_parts_usize(self) -> (T, Range<usize>) {
        (self.value, self.span.to_range())
    }
}

impl<T> AsRef<T> for Spanned<T> {
    #[inline]
    fn as_ref(&self) -> &T {
        &self.value
    }
}

impl<T: PartialOrd> PartialOrd for Spanned<T> {
    #[inline]
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        match self.value.partial_cmp(&other.value) {
            Some(Ordering::Equal) => self.span.partial_cmp(&other.span),
            ord => ord,
        }
    }
}

impl<T: Ord> Ord for Spanned<T> {
    #[inline]
    fn cmp(&self, other: &Self) -> Ordering {
        self.value.cmp(&other.value).then_with(|| self.span.cmp(&other.span))
    }
}

/// Errors arising from invalid source positions or line numbers.
#[derive(Copy, Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum PositionError {
    /// A byte position exceeds the `u32` range.
    #[error("byte position exceeds u32 range")]
    BytePosOverflow,

    /// A source line number is zero (line numbers are 1-indexed).
    #[error("source line number must be non-zero")]
    ZeroSourceLine,
}

#[cfg(test)]
mod tests {
    use super::*;

    mod line_index {
        use pretty_assertions::assert_eq;
        use rstest::rstest;

        use super::*;

        fn line(number: u32) -> SourceLine {
            SourceLine::new(number).expect("test line numbers are non-zero")
        }

        #[rstest]
        #[case::empty_source("", 0, 1)]
        #[case::single_line_start("no newlines here", 0, 1)]
        #[case::single_line_middle("no newlines here", 10, 1)]
        #[case::start_of_line_one("one\ntwo\nthree", 0, 1)]
        #[case::mid_line_one("one\ntwo\nthree", 2, 1)]
        #[case::newline_char_of_line_one("one\ntwo\nthree", 3, 1)]
        #[case::start_of_line_two("one\ntwo\nthree", 4, 2)]
        #[case::start_of_line_three("one\ntwo\nthree", 8, 3)]
        #[case::last_byte_of_line_three("one\ntwo\nthree", 12, 3)]
        #[case::exact_end_of_source("one\ntwo\nthree", 13, 3)]
        #[case::beyond_source_length("one\ntwo\nthree", 1000, 3)]
        #[case::empty_line_in_middle("one\n\nthree", 4, 2)]
        #[case::line_after_empty_line("one\n\nthree", 5, 3)]
        #[case::end_after_trailing_newline("one\n", 4, 2)]
        #[case::crlf_carriage_return("one\r\ntwo", 3, 1)]
        #[case::crlf_line_start("one\r\ntwo", 5, 2)]
        fn resolves_expected_line_for_pos_in_source(
            #[case] source: &str,
            #[case] pos: u32,
            #[case] expected_line: u32,
        ) {
            let index = LineIndex::new(source);

            assert_eq!(
                index.line_at(BytePos::new(pos)),
                line(expected_line)
            );
        }

        #[test]
        fn resolves_lines_of_span() {
            let index = LineIndex::new("one\ntwo\nthree");
            let span = ByteSpan::new(BytePos::new(2), BytePos::new(6));
            assert_eq!(index.lines_of(span), (line(1), line(2)));
        }
    }

    mod byte_pos {
        use pretty_assertions::assert_eq;
        use rstest::rstest;

        use super::*;

        fn pos_past_u32_max() -> usize {
            usize::try_from(u32::MAX)
                .expect("u32 always fits in usize")
                .saturating_add(1)
        }

        #[rstest]
        #[case::zero(0)]
        #[case::non_ascii_byte(128)]
        #[case::u32_max(u32::MAX)]
        fn round_trips_u32_boundary_values(#[case] value: u32) {
            let pos = BytePos::from(value);

            assert_eq!(u32::from(pos), value);
            assert_eq!(
                usize::from(pos),
                usize::try_from(value).expect("u32 always fits in usize")
            );
            assert_eq!(pos, BytePos::new(value));
        }

        #[test]
        fn defines_maximum_pos_as_u32_max() {
            assert_eq!(u32::from(BytePos::MAX), u32::MAX);
        }

        #[rstest]
        #[case::zero(0)]
        #[case::mid_range(1_048_576)]
        #[case::u32_max(u32::MAX)]
        fn narrows_from_usize_within_u32_range(#[case] value: u32) {
            let widened =
                usize::try_from(value).expect("u32 always fits in usize");

            let pos = BytePos::try_from(widened).expect("within u32 range");

            assert_eq!(pos, BytePos::from(value));
        }

        #[rstest]
        #[case::one_past_u32_max(pos_past_u32_max())]
        #[case::usize_max(usize::MAX)]
        fn returns_position_error_when_usize_exceeds_u32_max(
            #[case] oversized: usize,
        ) {
            let error =
                BytePos::try_from(oversized).expect_err("beyond u32 range");

            assert_eq!(error, PositionError::BytePosOverflow);
        }

        #[rstest]
        #[case::zero(0)]
        #[case::mid_range(1_048_576)]
        #[case::u32_max(u32::MAX)]
        fn keeps_in_range_positions_unsaturated(#[case] value: u32) {
            let widened =
                usize::try_from(value).expect("u32 always fits in usize");

            assert_eq!(BytePos::saturating_from(widened), BytePos::from(value));
        }

        #[rstest]
        #[case::one_past_u32_max(pos_past_u32_max())]
        #[case::usize_max(usize::MAX)]
        fn saturates_oversized_position_to_max(#[case] oversized: usize) {
            assert_eq!(BytePos::saturating_from(oversized), BytePos::MAX);
        }

        #[test]
        fn orders_positions_by_numeric_value() {
            assert!(BytePos::new(1) < BytePos::new(2));
            assert!(BytePos::new(0) < BytePos::MAX);
        }
    }

    mod span_start {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn creates_and_closes_into_byte_span() {
            let start = SpanStart::at(BytePos::new(10));
            assert_eq!(start.pos(), BytePos::new(10));

            let span = start.close(BytePos::new(25));
            assert_eq!(span.start(), BytePos::new(10));
            assert_eq!(span.end(), BytePos::new(25));
        }

        #[test]
        fn converts_from_byte_pos() {
            let start = BytePos::new(5).to_start();
            assert_eq!(start, SpanStart::from(BytePos::new(5)));
        }
    }

    mod byte_span {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn enforces_start_less_than_or_equal_end() {
            let normal = ByteSpan::new(BytePos::new(5), BytePos::new(10));
            assert_eq!(normal.start(), BytePos::new(5));
            assert_eq!(normal.end(), BytePos::new(10));
            assert_eq!(normal.len(), 5);
            assert!(!normal.is_empty());

            let inverted = ByteSpan::new(BytePos::new(15), BytePos::new(5));
            assert_eq!(inverted.start(), BytePos::new(15));
            assert_eq!(inverted.end(), BytePos::new(15));
            assert_eq!(inverted.len(), 0);
            assert!(inverted.is_empty());
        }

        #[test]
        fn converts_to_and_from_ranges() {
            let span = ByteSpan::from(3usize..12usize);
            assert_eq!(span.start_usize(), 3);
            assert_eq!(span.end_usize(), 12);
            assert_eq!(span.to_range(), 3..12);

            let range_pos: Range<BytePos> = span.into();
            assert_eq!(range_pos, BytePos::new(3)..BytePos::new(12));

            let round_trip = ByteSpan::from(range_pos);
            assert_eq!(round_trip, span);
        }
    }

    mod source_line {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn source_line_displays_as_its_numeric_value() {
            let line = SourceLine::new(7).expect("non-zero");

            assert_eq!(line.to_string(), "7");
        }

        #[test]
        fn round_trips_u32_value_through_source_line() {
            let line = SourceLine::try_from(42u32).expect("non-zero");

            assert_eq!(u32::from(line), 42);
            assert_eq!(line.get(), 42);
        }

        #[test]
        fn source_line_rejects_zero() {
            assert!(SourceLine::new(0).is_none());
            assert_eq!(
                SourceLine::try_from(0u32),
                Err(PositionError::ZeroSourceLine)
            );
        }

        #[test]
        fn defines_minimum_line_as_one() {
            assert_eq!(SourceLine::MIN.get(), 1);
        }
    }

    mod position_error {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn displays_expected_messages() {
            assert_eq!(
                PositionError::BytePosOverflow.to_string(),
                "byte position exceeds u32 range"
            );
            assert_eq!(
                PositionError::ZeroSourceLine.to_string(),
                "source line number must be non-zero"
            );
        }
    }

    mod serialization {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn round_trips_source_line_as_plain_u32() {
            let line = SourceLine::try_from(7u32).expect("non-zero");

            let encoded = serde_json::to_string(&line).expect("serializable");
            let decoded: SourceLine =
                serde_json::from_str(&encoded).expect("decodable");

            assert_eq!(encoded, "7");
            assert_eq!(decoded, line);
        }

        #[test]
        fn rejects_zero_when_deserializing() {
            let error = serde_json::from_str::<SourceLine>("0")
                .expect_err("zero is not a source line");

            assert!(
                error
                    .to_string()
                    .contains("source line number must be non-zero"),
                "unexpected error message: {error}"
            );
        }

        #[test]
        fn round_trips_byte_pos_as_plain_u32() {
            let pos = BytePos::new(5);

            let encoded = serde_json::to_string(&pos).expect("serializable");
            let decoded: BytePos =
                serde_json::from_str(&encoded).expect("decodable");

            assert_eq!(encoded, "5");
            assert_eq!(decoded, pos);
        }

        #[test]
        fn round_trips_byte_span_as_json() {
            let span = ByteSpan::new(BytePos::new(4), BytePos::new(10));

            let encoded = serde_json::to_string(&span).expect("serializable");
            let decoded: ByteSpan =
                serde_json::from_str(&encoded).expect("decodable");

            assert_eq!(decoded, span);
        }
    }

    mod spanned {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn carries_its_value_with_its_span() {
            let spanned = Spanned::from_usize_range("token", 4..9);
            assert_eq!(*spanned.value(), "token");
            assert_eq!(spanned.span_usize(), 4..9);
        }

        #[test]
        fn orders_by_value_then_span_start_then_span_end() {
            let same_value_earlier_start = Spanned::from_usize_range("a", 0..2);
            let same_value_longer_span = Spanned::from_usize_range("a", 0..3);
            let later_value = Spanned::from_usize_range("b", 0..1);

            assert!(same_value_earlier_start < same_value_longer_span);
            assert!(same_value_longer_span < later_value);
        }

        #[test]
        fn keeps_a_zero_length_span_ordered_by_its_position() {
            let at_five = Spanned::from_usize_range("a", 5..5);
            let at_six = Spanned::from_usize_range("a", 6..6);

            assert_eq!(at_five.span_usize(), 5..5);
            assert!(at_five < at_six);
        }

        #[test]
        fn provides_span_bounds_accessors() {
            let spanned = Spanned::from_usize_range("item", 3..10);

            assert_eq!(spanned.start(), BytePos::new(3));
            assert_eq!(spanned.end(), BytePos::new(10));
            assert_eq!(spanned.start_usize(), 3);
            assert_eq!(spanned.end_usize(), 10);
        }

        #[test]
        fn decomposes_into_parts_and_borrows_as_ref() {
            let spanned = Spanned::from_usize_range("data".to_owned(), 2..6);

            assert_eq!(spanned.as_ref(), "data");
            let (value, span) = spanned.into_parts_usize();
            assert_eq!(value, "data");
            assert_eq!(span, 2..6);
        }
    }
}
