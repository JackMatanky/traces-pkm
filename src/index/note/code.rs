//! Code-region spans excluded from metadata scanning.

use std::ops::Range;

use serde::{Deserialize, Serialize};

use super::byte::ByteRange;

/// Source byte range of inline code or a code block.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(transparent)]
pub(crate) struct CodeRegion {
    range: ByteRange,
}

impl CodeRegion {
    /// Creates a code region from source byte offsets.
    #[inline]
    #[must_use]
    pub(crate) fn new(start: usize, end: usize) -> Self {
        Self {
            range: ByteRange::new(start, end),
        }
    }

    /// Byte range in the original Markdown source.
    #[inline]
    #[must_use]
    pub(crate) fn range(&self) -> Range<usize> {
        self.range.range()
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn returns_the_original_source_range() {
        let region = CodeRegion::new(3, 7);

        assert_eq!(region.range(), 3..7);
    }
}
