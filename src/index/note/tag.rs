//! Markdown tags extracted from note text.

use serde::{Deserialize, Serialize};

/// Markdown tag including its leading `#`.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub(crate) struct Tag(String);

impl Tag {
    /// Creates a tag from text that includes the leading `#`.
    #[inline]
    #[must_use]
    pub(crate) fn new(text: impl Into<String>) -> Self {
        Self(text.into())
    }

    /// Full tag text, including the leading `#`.
    #[inline]
    #[must_use]
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn stores_the_given_text() {
        let tag = Tag::new("#book");

        assert_eq!(tag.as_str(), "#book");
    }
}
