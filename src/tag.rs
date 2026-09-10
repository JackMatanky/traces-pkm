//! Validated Markdown tag type with pre-computed hierarchical segments.
//!
//! Tags are `#`-prefixed identifiers matching `[a-zA-Z][a-zA-Z0-9_/]*`. Nested
//! tags like `#projects/active` are stored with their full segment hierarchy
//! pre-computed at construction time for efficient containment checks.

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// A validated Markdown tag, including its leading `#`.
///
/// Constructed via [`Tag::parse`], which validates the format and pre-computes
/// hierarchical segments for efficient nesting checks.
///
/// # Examples
///
/// ```
/// # use traces_pkm::Tag;
/// let tag = Tag::parse("#projects/active").unwrap();
/// assert_eq!(tag.as_str(), "#projects/active");
/// assert!(tag.is_contained_in("#projects"));
/// ```
#[derive(
    Clone, Debug, Eq, PartialEq, Hash, Ord, PartialOrd, Deserialize, Serialize,
)]
pub struct Tag(Box<str>);

impl Tag {
    /// Parses and validates a tag string.
    ///
    /// The input must start with `#` followed by `[a-zA-Z]`, then
    /// `[a-zA-Z0-9_/]*`.
    ///
    /// # Errors
    ///
    /// Returns [`TagError`] if the input does not match the tag format.
    ///
    /// # Examples
    ///
    /// ```
    /// # use traces_pkm::Tag;
    /// let tag = Tag::parse("#rust/pkm").unwrap();
    /// assert_eq!(tag.as_str(), "#rust/pkm");
    /// ```
    #[inline]
    pub fn parse(input: &str) -> Result<Self, TagError> {
        let rest = input.strip_prefix('#').ok_or(TagError::MissingHash)?;
        let mut chars = rest.chars();
        let first = chars.next().ok_or(TagError::MissingHash)?;
        if !first.is_alphabetic() {
            return Err(TagError::InvalidFirstCharacter {
                found: first,
            });
        }
        let body_bytes = rest.len();
        let mut end = first.len_utf8();
        while end < body_bytes {
            let ch = rest[end..].chars().next().ok_or(
                TagError::InvalidBodyCharacter {
                    offset: end,
                    found: '\0',
                },
            )?;
            if ch.is_alphanumeric() || matches!(ch, '_' | '/' | '-') {
                end = end.saturating_add(ch.len_utf8());
            } else {
                return Err(TagError::InvalidBodyCharacter {
                    offset: end,
                    found: ch,
                });
            }
        }
        let full = &input[..=end];
        Ok(Self(full.into()))
    }

    /// Returns the full tag string, including its leading `#`.
    #[inline]
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Yields ancestor segments on demand from root to leaf.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use traces_pkm::Tag;
    ///
    /// let tag = Tag::parse("#a/b/c").expect("valid tag");
    /// let segments: Vec<_> = tag.segments().collect();
    /// assert_eq!(segments, ["#a", "#a/b", "#a/b/c"]);
    /// ```
    #[inline]
    pub fn segments(&self) -> impl Iterator<Item = &str> {
        self.0
            .match_indices('/')
            .map(|(idx, _)| &self.0[..idx])
            .chain(std::iter::once(self.0.as_ref()))
    }

    /// Returns `true` if `self` equals `prefix` or is a hierarchical sub-tag.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use traces_pkm::Tag;
    ///
    /// let tag = Tag::parse("#projects/active").expect("valid tag");
    /// assert!(tag.is_contained_in("#projects"));
    /// assert!(tag.is_contained_in("#projects/active"));
    /// assert!(!tag.is_contained_in("#project"));
    /// ```
    #[inline]
    #[must_use]
    pub fn is_contained_in(&self, prefix: &str) -> bool {
        self.0.starts_with(prefix)
            && (self.0.len() == prefix.len()
                || self.0.as_bytes().get(prefix.len()) == Some(&b'/'))
    }

    /// Returns `true` if `self` and `other` are the exact same tag.
    ///
    /// Unlike [`Self::is_contained_in`], this performs no hierarchical
    /// containment check: `#task` does not exactly match `#task/project`.
    #[inline]
    #[must_use]
    pub fn is_exact_match(&self, other: &Self) -> bool {
        self == other
    }
}

/// Errors returned by [`Tag::parse`].
#[derive(Clone, Debug, Eq, PartialEq, Error)]
pub enum TagError {
    /// The input tag string is missing the leading `#` prefix.
    #[error("tag must start with `#`")]
    MissingHash,
    /// The character immediately following `#` is not an ASCII letter.
    #[error("tag must start with `#` followed by a letter, found `{found}`")]
    InvalidFirstCharacter {
        /// The invalid character found after `#`.
        found: char,
    },
    /// The tag body contains an invalid character.
    #[error(
        "invalid character `{found}` in tag; only letters, digits, `_`, `-`, \
         and `/` are allowed"
    )]
    InvalidBodyCharacter {
        /// Byte offset in the input where the invalid character occurred.
        offset: usize,
        /// The invalid character encountered.
        found: char,
    },
}

#[cfg(test)]
mod tests {

    use super::*;

    mod parse {
        use pretty_assertions::assert_eq;

        use super::*;
        #[test]
        fn accepts_simple_tag() {
            let tag = Tag::parse("#book").unwrap();
            assert_eq!(tag.as_str(), "#book");
            assert_eq!(tag.segments().collect::<Vec<_>>(), ["#book"]);
        }

        #[test]
        fn accepts_single_letter_tag() {
            let tag = Tag::parse("#a").unwrap();
            assert_eq!(tag.as_str(), "#a");
            assert_eq!(tag.segments().collect::<Vec<_>>(), ["#a"]);
        }

        #[test]
        fn accepts_nested_tag() {
            let tag = Tag::parse("#projects/active").unwrap();
            assert_eq!(tag.as_str(), "#projects/active");
            assert_eq!(tag.segments().collect::<Vec<_>>(), [
                "#projects",
                "#projects/active"
            ]);
        }

        #[test]
        fn accepts_deeply_nested_tag() {
            let tag = Tag::parse("#a/b/c").unwrap();
            assert_eq!(tag.segments().collect::<Vec<_>>(), [
                "#a", "#a/b", "#a/b/c"
            ]);
        }

        #[test]
        fn accepts_five_segments_tag() {
            let tag = Tag::parse("#a/b/c/d/e").unwrap();
            assert_eq!(tag.segments().collect::<Vec<_>>(), [
                "#a",
                "#a/b",
                "#a/b/c",
                "#a/b/c/d",
                "#a/b/c/d/e"
            ]);
        }

        #[test]
        fn accepts_underscores_and_hyphens() {
            let tag = Tag::parse("#my-tag/project_a").unwrap();
            assert_eq!(tag.as_str(), "#my-tag/project_a");
        }

        #[test]
        fn rejects_empty_hash_only() {
            let err = Tag::parse("#").unwrap_err();
            assert_eq!(err, TagError::MissingHash);
        }

        #[test]
        fn rejects_missing_hash() {
            let err = Tag::parse("book").unwrap_err();
            assert_eq!(err, TagError::MissingHash);
        }

        #[test]
        fn rejects_digit_first_char() {
            let err = Tag::parse("#1book").unwrap_err();
            assert!(matches!(err, TagError::InvalidFirstCharacter {
                found: '1'
            }));
        }

        #[test]
        fn rejects_dot_in_body() {
            let err = Tag::parse("#tag.name").unwrap_err();
            assert!(matches!(err, TagError::InvalidBodyCharacter {
                found: '.',
                offset: 3
            }));
        }

        #[test]
        fn rejects_space_in_body() {
            let err = Tag::parse("#tag name").unwrap_err();
            assert!(matches!(err, TagError::InvalidBodyCharacter { .. }));
        }

        #[test]
        fn error_message_for_invalid_first_char_includes_found() {
            let err = Tag::parse("#1").unwrap_err();
            assert!(err.to_string().contains("`1`"));
        }

        #[test]
        fn error_message_for_invalid_body_char_includes_found() {
            let err = Tag::parse("#tag!").unwrap_err();
            assert!(err.to_string().contains("`!`"));
        }
    }

    mod is_contained_in {
        use super::*;

        #[test]
        fn returns_true_when_tag_matches_itself() {
            let tag = Tag::parse("#projects/active").unwrap();
            assert!(tag.is_contained_in("#projects/active"));
        }

        #[test]
        fn returns_true_when_tag_matches_parent() {
            let tag = Tag::parse("#projects/active").unwrap();
            assert!(tag.is_contained_in("#projects"));
        }

        #[test]
        fn returns_true_when_tag_matches_grandparent() {
            let tag = Tag::parse("#a/b/c").unwrap();
            assert!(tag.is_contained_in("#a"));
        }

        #[test]
        fn returns_false_when_prefix_does_not_match() {
            let tag = Tag::parse("#projects").unwrap();
            assert!(!tag.is_contained_in("#project"));
        }

        #[test]
        fn returns_false_when_child_is_used_as_prefix() {
            let tag = Tag::parse("#projects").unwrap();
            assert!(!tag.is_contained_in("#projects/active"));
        }
    }

    mod is_exact_match {
        use super::*;

        #[test]
        fn returns_true_for_identical_tag() {
            let a = Tag::parse("#task").unwrap();
            let b = Tag::parse("#task").unwrap();
            assert!(a.is_exact_match(&b));
        }

        #[test]
        fn returns_false_for_nested_child_tag() {
            let parent = Tag::parse("#task").unwrap();
            let child = Tag::parse("#task/project").unwrap();
            assert!(!parent.is_exact_match(&child));
            assert!(!child.is_exact_match(&parent));
        }

        #[test]
        fn returns_false_for_unrelated_tag() {
            let a = Tag::parse("#task").unwrap();
            let b = Tag::parse("#todo").unwrap();
            assert!(!a.is_exact_match(&b));
        }
    }

    mod segments {
        use pretty_assertions::assert_eq;

        use super::*;
        #[test]
        fn yields_one_segment_for_simple_tag() {
            let tag = Tag::parse("#book").unwrap();
            assert_eq!(tag.segments().count(), 1);
        }

        #[test]
        fn yields_two_segments_for_nested_tag() {
            let tag = Tag::parse("#a/b").unwrap();
            assert_eq!(tag.segments().count(), 2);
        }

        #[test]
        fn yields_all_ancestor_segments_in_order() {
            let tag = Tag::parse("#a/b/c").unwrap();
            let segments: Vec<&str> = tag.segments().collect();
            assert_eq!(segments, ["#a", "#a/b", "#a/b/c"]);
        }
    }
}
