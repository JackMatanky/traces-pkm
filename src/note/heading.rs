//! Markdown heading data retained by a [`Note`](super::Note).

use serde::{Deserialize, Serialize};

use crate::SourceLine;

/// A Markdown heading with display text and its 1-indexed source line.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct Heading {
    level: u8,
    text: String,
    line: SourceLine,
}

impl Heading {
    /// Creates a heading from parsed content.
    #[inline]
    #[must_use]
    pub(crate) const fn new(level: u8, text: String, line: SourceLine) -> Self {
        Self {
            level,
            text,
            line,
        }
    }

    /// Returns the heading level from 1 through 6.
    #[inline]
    #[must_use]
    pub const fn level(&self) -> u8 {
        self.level
    }

    /// Returns the markup-stripped display text.
    #[inline]
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Returns the 1-indexed line containing the heading start.
    #[inline]
    #[must_use]
    pub const fn line(&self) -> SourceLine {
        self.line
    }

    /// Appends an inline text fragment to the heading display text.
    #[inline]
    pub(crate) fn push_text(&mut self, text: &str) {
        self.text.push_str(text);
    }
}
