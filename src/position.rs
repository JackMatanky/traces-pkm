//! Source-position primitives shared across text-parsing domains.
//!
//! [`SourceLine`] and `ByteOffset` are distinct newtypes so a byte offset can
//! never be mistaken for a line number at compile time. Domain-specific parsers
//! (Markdown notes, config files, templates) convert between the two with their
//! own local tracking strategy; only the vocabulary lives here.

use std::{fmt, num::NonZeroU32};

use serde::{Deserialize, Serialize};

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

impl fmt::Display for SourceLine {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
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

/// Error returned when converting a zero value to [`SourceLine`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SourceLineError;

impl fmt::Display for SourceLineError {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("source line number must be non-zero")
    }
}

impl std::error::Error for SourceLineError {}

/// Serde support for [`SourceLine`].
mod source_line_serde {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    use super::{SourceLine, SourceLineError};

    impl Serialize for SourceLine {
        #[inline]
        fn serialize<S: Serializer>(
            &self,
            serializer: S,
        ) -> Result<S::Ok, S::Error> {
            self.0.get().serialize(serializer)
        }
    }

    impl<'de> Deserialize<'de> for SourceLine {
        #[inline]
        fn deserialize<D: Deserializer<'de>>(
            deserializer: D,
        ) -> Result<Self, D::Error> {
            let line = u32::deserialize(deserializer)?;
            Self::new(line)
                .ok_or_else(|| serde::de::Error::custom(SourceLineError))
        }
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

    #[test]
    fn byte_offset_conversions_and_accessors() {
        let offset = ByteOffset::from(128usize);
        assert_eq!(usize::from(offset), 128);
        assert_eq!(offset, ByteOffset::new(128));
    }
}
