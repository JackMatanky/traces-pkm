//! Markdown tag values extracted from note text.

use serde::{Deserialize, Serialize};

/// A Markdown tag value, including its leading `#`.
///
/// Tags are extracted from paragraphs, headings, and list items. Mid-word
/// occurrences like `foo#bar` are rejected. Nested tags such as
/// `#projects/active` are stored as a single string; use
/// [`Self::is_nested_under`] for prefix matching at `/` boundaries.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct Tag(String);

impl Tag {
    /// Creates a tag from text that includes the leading `#`.
    #[inline]
    #[must_use]
    pub(crate) fn new(text: impl Into<String>) -> Self {
        Self(text.into())
    }

    /// Returns the tag text, including the leading `#`.
    #[inline]
    #[must_use]
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }

    /// Returns `true` if this tag equals `other` or is nested below it.
    ///
    /// A tag matches itself exactly, or matches a longer tag nested below it at
    /// a `/` boundary: `#projects` matches `#projects/active`, but `#project`
    /// does not match `#projects` because nesting must start at `/`, not merely
    /// share a prefix.
    #[inline]
    #[must_use]
    pub(crate) fn is_nested_under(&self, other: &str) -> bool {
        is_nested_under(&self.0, other)
    }
}

/// Returns `true` if `text` equals `prefix` or is nested below it at a `/`
/// boundary.
///
/// Shared by [`Tag::is_nested_under`] and [`crate::query::filter`]'s tag
/// containment check (`tag_or_value_matches`) so the nesting rule has one
/// implementation.
#[inline]
#[must_use]
pub(crate) fn is_nested_under(text: &str, prefix: &str) -> bool {
    text == prefix
        || text.strip_prefix(prefix).is_some_and(|rest| rest.starts_with('/'))
}

#[cfg(test)]
mod tests {

    use super::*;

    mod constructor {
        use pretty_assertions::assert_eq;

        use super::*;
        #[test]
        fn stores_given_text_with_leading_hash() {
            let tag = Tag::new("#book");

            assert_eq!(tag.as_str(), "#book");
        }
    }

    mod is_nested_under {
        use rstest::rstest;

        use super::*;

        #[rstest]
        #[case::identical_tag("#projects/active", "#projects/active")]
        #[case::direct_parent("#projects/active", "#projects")]
        #[case::grandparent("#projects/active/urgent", "#projects")]
        #[case::nested_query_target("#projects/active/sub", "#projects/active")]
        fn returns_true_when_tag_matches_or_is_nested(
            #[case] tag: &str,
            #[case] query: &str,
        ) {
            assert!(Tag::new(tag).is_nested_under(query));
        }

        #[rstest]
        #[case::unrelated_tag("#book", "#movie")]
        #[case::more_specific_than_the_tag("#projects", "#projects/active")]
        #[case::prefix_word_without_a_slash_boundary("#projects", "#project")]
        fn returns_false_when_unrelated_or_more_specific(
            #[case] tag: &str,
            #[case] query: &str,
        ) {
            assert!(!Tag::new(tag).is_nested_under(query));
        }
    }
}
