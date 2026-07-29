use std::ops::Range;

use serde::{Deserialize, Serialize};

/// Source byte range of inline code or a code block excluded from metadata
/// scanning.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub(crate) struct CodeRegion {
    start: usize,
    end: usize,
}

impl CodeRegion {
    /// Creates a code region from start and end byte offsets.
    #[inline]
    #[must_use]
    pub(crate) fn new(start: usize, end: usize) -> Self {
        Self {
            start,
            end,
        }
    }

    /// Byte range in the original markdown source.
    #[inline]
    #[must_use]
    pub(crate) fn range(&self) -> Range<usize> {
        self.start..self.end
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
