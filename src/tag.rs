//! Validated Markdown tag type.
//!
//! Tags are `#`-prefixed identifiers: a Unicode alphabetic first character
//! followed by any mix of Unicode alphanumeric characters, `_`, `/`, or `-`.
//! Validation happens once, at construction; hierarchy segments and containment
//! checks are computed on demand from the stored string.

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// A validated Markdown tag; the stored string starts with `#`.
///
/// Constructed via [`Tag::parse`], which validates the format once; nesting
/// checks ([`Self::is_contained_in`]) and segments ([`Self::segments`]) are
/// computed on demand from the stored string.
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
    /// The input must start with `#` followed by a Unicode alphabetic
    /// character, then any mix of Unicode alphanumeric characters, `_`, `/`, or
    /// `-`.
    ///
    /// # Errors
    ///
    /// - [`MissingHash`] if the input has no leading `#`, or is a lone `#`.
    /// - [`InvalidFirstCharacter`] if the character after `#` isn't alphabetic.
    /// - [`InvalidBodyCharacter`] if a later character isn't alphanumeric, `_`,
    ///   `/`, or `-`.
    ///
    /// [`MissingHash`]: TagError::MissingHash
    /// [`InvalidFirstCharacter`]: TagError::InvalidFirstCharacter
    /// [`InvalidBodyCharacter`]: TagError::InvalidBodyCharacter
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
        let mut chars = rest.char_indices();
        let (_, first) = chars.next().ok_or(TagError::MissingHash)?;
        if !first.is_alphabetic() {
            return Err(TagError::InvalidFirstCharacter {
                found: first,
            });
        }
        let mut end = first.len_utf8();
        for (offset, ch) in chars {
            if !Self::is_body_char(ch) {
                return Err(TagError::InvalidBodyCharacter {
                    offset,
                    found: ch,
                });
            }
            end = offset.saturating_add(ch.len_utf8());
        }
        Ok(Self(input[..=end].into()))
    }

    /// Parses `input` leniently; see [`Self::parse_lenient_into`].
    ///
    /// Allocates its own scratch buffer: for a caller that parses many tags in
    /// a loop, prefer [`Self::parse_lenient_into`] with a reused buffer.
    ///
    /// # Errors
    ///
    /// Same conditions as [`Self::parse_lenient_into`].
    #[inline]
    pub(crate) fn parse_lenient(input: &str) -> Result<Self, TagError> {
        let mut buf = String::new();
        Self::parse_lenient_into(input, &mut buf)
    }

    /// Parses `input` leniently, appending the normalized form into `buf`
    /// (cleared first, reused across calls).
    ///
    /// Trims whitespace and treats a missing leading `#` as implicit, so
    /// `"book"` and `"#book"` both parse to the same [`Tag`]. An
    /// already-present leading `#` is kept as-is, so doubled hashes still fail
    /// the validation performed by [`Self::parse`] instead of silently
    /// normalizing away.
    ///
    /// # Errors
    ///
    /// - [`MissingHash`] if the trimmed input is empty or a lone `#`.
    /// - [`InvalidFirstCharacter`] if the character after the (implicit or
    ///   explicit) `#` isn't alphabetic.
    /// - [`InvalidBodyCharacter`] if a later character isn't alphanumeric, `_`,
    ///   `/`, or `-`.
    ///
    /// [`MissingHash`]: TagError::MissingHash
    /// [`InvalidFirstCharacter`]: TagError::InvalidFirstCharacter
    /// [`InvalidBodyCharacter`]: TagError::InvalidBodyCharacter
    #[inline]
    pub(crate) fn parse_lenient_into(
        input: &str,
        buf: &mut String,
    ) -> Result<Self, TagError> {
        let trimmed = input.trim();
        let body = trimmed.strip_prefix('#').unwrap_or(trimmed);
        buf.clear();
        buf.push('#');
        buf.push_str(body);
        Self::parse(buf)
    }

    /// Returns the byte length of the valid tag prefix at the start of `input`.
    #[inline]
    #[must_use]
    pub(crate) fn prefix_len(input: &str) -> Option<usize> {
        let rest = input.strip_prefix('#')?;
        let mut chars = rest.char_indices();
        let (_, first) = chars.next()?;
        if !first.is_alphabetic() {
            return None;
        }

        let mut end = '#'.len_utf8().saturating_add(first.len_utf8());
        for (offset, ch) in chars {
            if !Self::is_body_char(ch) {
                break;
            }
            end = '#'
                .len_utf8()
                .saturating_add(offset)
                .saturating_add(ch.len_utf8());
        }
        Some(end)
    }

    /// Returns `true` if `ch` is a valid tag body character: Unicode
    /// alphanumeric, `_`, `/`, or `-`.
    #[inline]
    #[must_use]
    fn is_body_char(ch: char) -> bool {
        ch.is_alphanumeric() || matches!(ch, '_' | '/' | '-')
    }

    /// Returns the full tag string, leading `#` included.
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

    /// Returns `true` if `item` and `target` are both valid tags where `item`
    /// is `target` or a hierarchical sub-tag of it (e.g. `#book/fiction`
    /// matches `#book`).
    ///
    /// Pure hierarchy check: unlike [`Self::is_contained_in`], `item` and
    /// `target` are raw strings that may not be `#`-prefixed at all, in which
    /// case this returns `false` (rejected by [`Self::parse`] or
    /// [`Self::is_contained_in`] without either needing the other to be
    /// `#`-prefixed). Callers wanting an exact-string shortcut too (`item ==
    /// target` regardless of tag shape) must add that check themselves; baking
    /// it in here would let non-tag values with a coincidentally matching
    /// string collide.
    #[inline]
    #[must_use]
    pub(crate) fn is_hierarchical_match(item: &str, target: &str) -> bool {
        Self::parse(item).is_ok_and(|tag| tag.is_contained_in(target))
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
    /// The input has no leading `#`, or nothing follows it.
    #[error("tag must start with `#`")]
    MissingHash,
    /// The character immediately following `#` is not alphabetic.
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
        use rstest::rstest;

        use super::*;

        #[rstest]
        #[case::simple("#book")]
        #[case::single_letter("#a")]
        #[case::nested("#projects/active")]
        #[case::underscores_and_hyphens("#my-tag/project_a")]
        #[case::unicode_first_character("#café")]
        fn accepts_valid_tags(#[case] input: &str) {
            let tag = Tag::parse(input).unwrap();
            assert_eq!(tag.as_str(), input);
        }

        #[rstest]
        #[case::lone_hash("#")]
        #[case::no_hash("book")]
        fn rejects_when_hash_prefix_is_missing_or_empty(#[case] input: &str) {
            let err = Tag::parse(input).unwrap_err();
            assert_eq!(err, TagError::MissingHash);
        }

        // `_` is a valid body character but not alphabetic, so it must be
        // rejected as a first character even though `is_body_char` accepts it
        // everywhere else in the tag.
        #[rstest]
        #[case::digit("#1book", '1')]
        #[case::underscore("#_tag", '_')]
        fn rejects_non_alphabetic_first_character(
            #[case] input: &str,
            #[case] found: char,
        ) {
            let err = Tag::parse(input).unwrap_err();
            assert!(
                matches!(err, TagError::InvalidFirstCharacter { found: f } if f == found),
                "expected InvalidFirstCharacter{{found: {found:?}}}, got \
                 {err:?}"
            );
        }

        #[test]
        fn rejects_dot_in_body() {
            let err = Tag::parse("#tag.name").unwrap_err();
            assert!(
                matches!(err, TagError::InvalidBodyCharacter {
                    found: '.',
                    offset: 3
                }),
                "expected InvalidBodyCharacter{{found: '.', offset: 3}}, got \
                 {err:?}"
            );
        }

        #[test]
        fn rejects_space_in_body() {
            let err = Tag::parse("#tag name").unwrap_err();
            assert!(
                matches!(err, TagError::InvalidBodyCharacter { .. }),
                "expected InvalidBodyCharacter, got {err:?}"
            );
        }

        #[test]
        fn returns_the_byte_offset_for_a_multi_byte_invalid_character() {
            // 'é' is a 2-byte UTF-8 sequence, so the byte offset of the invalid
            // '!' that follows it (5) differs from its char index (4).
            let err = Tag::parse("#café!").unwrap_err();
            assert!(
                matches!(err, TagError::InvalidBodyCharacter {
                    found: '!',
                    offset: 5
                }),
                "expected InvalidBodyCharacter{{found: '!', offset: 5}}, got \
                 {err:?}"
            );
        }
    }

    mod formatting {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn returns_the_missing_hash_message() {
            assert_eq!(
                TagError::MissingHash.to_string(),
                "tag must start with `#`"
            );
        }

        #[test]
        fn includes_the_found_character_for_an_invalid_first_character() {
            let err = TagError::InvalidFirstCharacter {
                found: '1',
            };
            assert!(
                err.to_string().contains("`1`"),
                "expected message to mention the found character, got: {err}"
            );
        }

        #[test]
        fn includes_the_found_character_for_an_invalid_body_character() {
            let err = TagError::InvalidBodyCharacter {
                offset: 0,
                found: '!',
            };
            assert!(
                err.to_string().contains("`!`"),
                "expected message to mention the found character, got: {err}"
            );
        }
    }

    mod parse_lenient {
        use pretty_assertions::assert_eq;
        use rstest::rstest;

        use super::*;

        #[rstest]
        #[case::already_hash_prefixed("#book")]
        #[case::missing_hash_prefix("book")]
        #[case::leading_and_trailing_whitespace("  book  ")]
        #[case::hash_prefixed_with_whitespace("  #book  ")]
        fn normalizes_to_the_same_tag(#[case] input: &str) {
            assert_eq!(
                Tag::parse_lenient(input).unwrap(),
                Tag::parse("#book").unwrap()
            );
        }

        #[test]
        fn rejects_a_doubled_hash_prefix() {
            let err = Tag::parse_lenient("##book").unwrap_err();
            assert!(
                matches!(err, TagError::InvalidFirstCharacter {
                    found: '#'
                }),
                "expected InvalidFirstCharacter{{found: '#'}}, got {err:?}"
            );
        }

        #[rstest]
        #[case::whitespace_only("   ")]
        #[case::lone_hash("#")]
        fn rejects_empty_or_whitespace_only_input(#[case] input: &str) {
            assert_eq!(
                Tag::parse_lenient(input).unwrap_err(),
                TagError::MissingHash
            );
        }

        #[test]
        fn reuses_the_given_buffer_across_successive_calls() {
            let mut buf = String::new();
            let first = Tag::parse_lenient_into("book", &mut buf).unwrap();
            let second = Tag::parse_lenient_into("#recipe", &mut buf).unwrap();

            assert_eq!(first, Tag::parse("#book").unwrap());
            assert_eq!(second, Tag::parse("#recipe").unwrap());
        }
    }

    mod prefix_len {
        use pretty_assertions::assert_eq;
        use rstest::rstest;

        use super::*;

        #[rstest]
        #[case::exact_tag("#book", 5)]
        #[case::trailing_non_tag_text("#book more text", 5)]
        // "#café" is 1 (`#`) + 3 (c, a, f) + 2 (é) = 6 bytes.
        #[case::multi_byte_characters("#café", 6)]
        fn returns_the_valid_prefix_length(
            #[case] input: &str,
            #[case] expected: usize,
        ) {
            assert_eq!(Tag::prefix_len(input), Some(expected));
        }

        #[rstest]
        #[case::missing_hash("book")]
        #[case::lone_hash("#")]
        #[case::non_alphabetic_first_character("#1book")]
        fn returns_none_when_no_valid_prefix_exists(#[case] input: &str) {
            assert_eq!(Tag::prefix_len(input), None);
        }
    }

    mod is_contained_in {
        use rstest::rstest;

        use super::*;

        #[rstest]
        #[case::itself("#projects/active", "#projects/active")]
        #[case::parent("#projects/active", "#projects")]
        #[case::grandparent("#a/b/c", "#a")]
        fn returns_true_when_prefix_is_self_or_an_ancestor(
            #[case] input: &str,
            #[case] prefix: &str,
        ) {
            let tag = Tag::parse(input).unwrap();
            assert!(
                tag.is_contained_in(prefix),
                "expected {:?} to be contained in {prefix:?}",
                tag.as_str()
            );
        }

        #[rstest]
        #[case::unrelated_prefix_collision("#projects", "#project")]
        #[case::child_used_as_prefix("#projects", "#projects/active")]
        #[case::empty_prefix("#book", "")]
        #[case::prefix_without_hash("#book", "book")]
        fn returns_false_when_prefix_is_not_self_or_an_ancestor(
            #[case] input: &str,
            #[case] prefix: &str,
        ) {
            let tag = Tag::parse(input).unwrap();
            assert!(
                !tag.is_contained_in(prefix),
                "expected {:?} not to be contained in {prefix:?}",
                tag.as_str()
            );
        }
    }

    mod is_exact_match {
        use super::*;

        #[test]
        fn returns_true_for_identical_tag() {
            let a = Tag::parse("#task").unwrap();
            let b = Tag::parse("#task").unwrap();
            assert!(
                a.is_exact_match(&b),
                "expected {a:?} to exactly match {b:?}"
            );
        }

        #[test]
        fn returns_false_for_nested_child_tag() {
            let parent = Tag::parse("#task").unwrap();
            let child = Tag::parse("#task/project").unwrap();
            assert!(
                !parent.is_exact_match(&child),
                "expected {parent:?} not to exactly match child {child:?}"
            );
            assert!(
                !child.is_exact_match(&parent),
                "expected {child:?} not to exactly match parent {parent:?}"
            );
        }

        #[test]
        fn returns_false_for_unrelated_tag() {
            let a = Tag::parse("#task").unwrap();
            let b = Tag::parse("#todo").unwrap();
            assert!(
                !a.is_exact_match(&b),
                "expected {a:?} not to match unrelated {b:?}"
            );
        }
    }

    mod is_hierarchical_match {
        use rstest::rstest;

        use super::*;

        #[rstest]
        #[case::subtag_matches_parent("#book/fiction", "#book")]
        #[case::identical_tag_strings("#book", "#book")]
        fn returns_true_for_matching_or_identical_tags(
            #[case] item: &str,
            #[case] target: &str,
        ) {
            assert!(
                Tag::is_hierarchical_match(item, target),
                "expected {item:?} to match {target:?}"
            );
        }

        #[rstest]
        #[case::parent_tested_against_child("#book", "#book/fiction")]
        #[case::prefix_collision_without_slash("#bookworm", "#book")]
        #[case::item_lacks_hash_prefix("book", "#book")]
        #[case::target_lacks_hash_prefix("#book", "book")]
        #[case::invalid_item_syntax("#1invalid", "#book")]
        #[case::empty_target("#book", "")]
        fn returns_false_for_unrelated_invalid_or_empty_inputs(
            #[case] item: &str,
            #[case] target: &str,
        ) {
            assert!(
                !Tag::is_hierarchical_match(item, target),
                "expected {item:?} not to match {target:?}"
            );
        }
    }

    mod segments {
        use pretty_assertions::assert_eq;
        use rstest::rstest;

        use super::*;

        #[rstest]
        #[case::single_segment("#book", &["#book"])]
        #[case::two_segments("#a/b", &["#a", "#a/b"])]
        #[case::three_segments("#a/b/c", &["#a", "#a/b", "#a/b/c"])]
        #[case::five_segments(
            "#a/b/c/d/e",
            &["#a", "#a/b", "#a/b/c", "#a/b/c/d", "#a/b/c/d/e"]
        )]
        fn yields_all_ancestor_segments_in_order(
            #[case] input: &str,
            #[case] expected: &[&str],
        ) {
            let tag = Tag::parse(input).unwrap();
            assert_eq!(tag.segments().collect::<Vec<_>>(), expected);
        }
    }
}
