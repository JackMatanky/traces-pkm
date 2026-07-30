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

    /// Whether this tag is `other`, or a sub-tag nested under it.
    ///
    /// Mirrors Dataview's distinction between exact tags (`file.etags`) and
    /// unique tags including subtags (`file.tags`): `#projects/active` is
    /// nested under `#projects`, so a tag-source query for `#projects`
    /// matches Notes tagged `#projects/active` even though the tags are not
    /// textually equal. Matching stops at `/` boundaries, so `#project` does
    /// not spuriously match `#projects`.
    #[must_use]
    pub(crate) fn is_nested_under(&self, other: &str) -> bool {
        self.0 == other
            || self
                .0
                .strip_prefix(other)
                .is_some_and(|rest| rest.starts_with('/'))
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

    mod is_nested_under {
        use rstest::rstest;

        use super::*;

        #[rstest]
        #[case::identical_tag("#projects/active", "#projects/active")]
        #[case::direct_parent("#projects/active", "#projects")]
        #[case::grandparent("#projects/active/urgent", "#projects")]
        fn true_for_the_tag_itself_or_an_ancestor(
            #[case] tag: &str,
            #[case] query: &str,
        ) {
            assert!(Tag::new(tag).is_nested_under(query));
        }

        #[rstest]
        #[case::unrelated_tag("#book", "#movie")]
        #[case::more_specific_than_the_tag("#projects", "#projects/active")]
        #[case::prefix_word_without_a_slash_boundary("#projects", "#project")]
        fn false_for_unrelated_or_more_specific_queries(
            #[case] tag: &str,
            #[case] query: &str,
        ) {
            assert!(!Tag::new(tag).is_nested_under(query));
        }
    }
}
