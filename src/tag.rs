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
/// ```ignore
/// # use crate::tag::Tag;
/// let tag = Tag::parse("#projects/active").unwrap();
/// assert_eq!(tag.as_str(), "#projects/active");
/// assert!(tag.is_contained_in("#projects"));
/// ```
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct Tag {
    full: String,
    segments: Vec<String>,
}

/// Errors returned by [`Tag::parse`].
#[derive(Clone, Debug, Eq, PartialEq, Error)]
pub enum TagError {
    #[error("tag must start with `#`")]
    MissingHash,
    #[error("tag must start with `#` followed by a letter, found `{found}`")]
    InvalidFirstCharacter {
        found: char,
    },
    #[error(
        "invalid character `{found}` in tag; only letters, digits, `_`, `-`, \
         and `/` are allowed"
    )]
    InvalidBodyCharacter {
        offset: usize,
        found: char,
    },
}

impl Tag {
    /// Parses and validates a tag string.
    ///
    /// The input must start with `#` followed by `[a-zA-Z]`, then
    /// `[a-zA-Z0-9_/]*`. Hierarchical segments are pre-computed at construction
    /// time for efficient containment checks.
    ///
    /// # Errors
    ///
    /// Returns [`TagError`] if the input does not match the tag format.
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
        let full = input[..=end].to_string();
        let segments: Vec<String> = full
            .split('/')
            .scan(String::new(), |acc, part| {
                if acc.is_empty() {
                    *acc = String::from(part);
                } else {
                    acc.push('/');
                    acc.push_str(part);
                }
                Some(acc.clone())
            })
            .collect();
        Ok(Self {
            full,
            segments,
        })
    }

    /// Returns the full tag string, including its leading `#`.
    #[inline]
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.full
    }

    /// Returns the hierarchical tag segments from root to leaf.
    #[inline]
    #[must_use]
    #[cfg_attr(
        not(any(test, feature = "test-utils")),
        expect(
            dead_code,
            reason = "no current caller outside tests; kept for Tag accessor \
                      symmetry with its fields"
        )
    )]
    pub fn segments(&self) -> &[String] {
        &self.segments
    }

    /// Returns `true` if this tag is contained in `prefix` — either
    /// identical to `prefix` or nested below it at a `/` boundary.
    #[inline]
    #[must_use]
    pub fn is_contained_in(&self, prefix: &str) -> bool {
        self.segments.iter().any(|seg| seg == prefix)
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

#[cfg(test)]
mod tests {
    use super::*;

    mod parse {
        use super::*;

        #[test]
        fn accepts_simple_tag() {
            let tag = Tag::parse("#book").unwrap();
            assert_eq!(tag.as_str(), "#book");
            assert_eq!(tag.segments(), &["#book"]);
        }

        #[test]
        fn accepts_nested_tag() {
            let tag = Tag::parse("#projects/active").unwrap();
            assert_eq!(tag.as_str(), "#projects/active");
            assert_eq!(tag.segments(), &["#projects", "#projects/active"]);
        }

        #[test]
        fn accepts_deeply_nested_tag() {
            let tag = Tag::parse("#a/b/c").unwrap();
            assert_eq!(tag.segments(), &["#a", "#a/b", "#a/b/c"]);
        }

        #[test]
        fn accepts_underscores_and_hyphens() {
            let tag = Tag::parse("#my-tag/project_a").unwrap();
            assert_eq!(tag.as_str(), "#my-tag/project_a");
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
        fn tag_matches_itself() {
            let tag = Tag::parse("#projects/active").unwrap();
            assert!(tag.is_contained_in("#projects/active"));
        }

        #[test]
        fn tag_matches_parent() {
            let tag = Tag::parse("#projects/active").unwrap();
            assert!(tag.is_contained_in("#projects"));
        }

        #[test]
        fn tag_matches_grandparent() {
            let tag = Tag::parse("#a/b/c").unwrap();
            assert!(tag.is_contained_in("#a"));
        }

        #[test]
        fn tag_rejects_non_matching_prefix() {
            let tag = Tag::parse("#projects").unwrap();
            assert!(!tag.is_contained_in("#project"));
        }

        #[test]
        fn tag_rejects_child_as_prefix() {
            let tag = Tag::parse("#projects").unwrap();
            assert!(!tag.is_contained_in("#projects/active"));
        }
    }

    mod is_exact_match {
        use super::*;

        #[test]
        fn matches_the_identical_tag() {
            let a = Tag::parse("#task").unwrap();
            let b = Tag::parse("#task").unwrap();
            assert!(a.is_exact_match(&b));
        }

        #[test]
        fn rejects_a_nested_child_tag() {
            let parent = Tag::parse("#task").unwrap();
            let child = Tag::parse("#task/project").unwrap();
            assert!(!parent.is_exact_match(&child));
            assert!(!child.is_exact_match(&parent));
        }

        #[test]
        fn rejects_an_unrelated_tag() {
            let a = Tag::parse("#task").unwrap();
            let b = Tag::parse("#todo").unwrap();
            assert!(!a.is_exact_match(&b));
        }
    }

    mod segments {
        use super::*;

        #[test]
        fn simple_tag_has_one_segment() {
            let tag = Tag::parse("#book").unwrap();
            assert_eq!(tag.segments().len(), 1);
        }

        #[test]
        fn nested_tag_has_two_segments() {
            let tag = Tag::parse("#a/b").unwrap();
            assert_eq!(tag.segments().len(), 2);
        }
    }
}
