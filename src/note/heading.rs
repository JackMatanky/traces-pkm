//! Markdown heading data retained by a [`Note`](super::Note).

use serde::{Deserialize, Serialize};

use crate::SourceLine;

/// A Markdown heading with display text and its 1-indexed source line.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct Heading {
    /// Heading level, from 1 through 6.
    pub level: u8,
    /// Plain display text with Markdown markup removed.
    pub text: String,
    /// Source line containing the heading start.
    pub line: SourceLine,
}
