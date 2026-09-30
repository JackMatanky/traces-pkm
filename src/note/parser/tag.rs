//! Tag token scanning for Markdown text buffers.
//!
//! Tags are lexed separately from [`super::lexer::ItemToken`] because the field
//! tokens consume the rest of a line, hiding tags inside field values; this
//! scanner sees them. Both token sets share [`super::lexer::tag_callback`].

use logos::Logos;

use super::lexer::tag_callback;
use crate::Tag;

/// Token stream for Markdown tags in free-form text.
///
/// [`Self::Tag`] carries an emitted [`Tag`]; ordinary text is skipped by the
/// tokenizer's `skip` directive.
///
/// [`tag_callback`] returns logos' `Filter::Skip` to reject non-tag `#`
/// characters without swallowing the rest of the text.
#[derive(Clone, Debug, PartialEq, Logos)]
#[logos(skip(r"[\s\S]", priority = 0))]
enum TagToken {
    #[token("#", tag_callback)]
    Tag(Tag),
}

/// Extracts Markdown tags from `text` in encounter order.
#[inline]
#[must_use]
pub(super) fn scan_tags(text: &str) -> Vec<Tag> {
    let lexer = TagToken::lexer(text);
    let mut tags = Vec::new();
    for result in lexer {
        if let Ok(TagToken::Tag(tag)) = result {
            tags.push(tag);
        }
    }
    tags
}

#[cfg(test)]
mod tests {
    use super::*;

    mod scan_tags {
        use pretty_assertions::assert_eq;
        use rstest::rstest;

        use super::*;

        #[rstest]
        #[case::standalone("Filed under #book for later.", &["#book"])]
        #[case::nested_path(
            "#projects/active needs review.",
            &["#projects/active"]
        )]
        #[case::multiple_space_separated(
            "#book #fiction favorites.",
            &["#book", "#fiction"]
        )]
        #[case::hash_embedded_in_a_word(
            "The issue is foo#bar, not a tag.",
            &[]
        )]
        #[case::adjacent_separated_by_punctuation("(#a)(#b)", &["#a", "#b"])]
        #[case::glued_directly_onto_another_tag("#a#b", &["#a"])]
        #[case::preceded_by_multibyte_punctuation("café—#book", &["#book"])]
        #[case::glued_onto_a_multibyte_letter("café#book", &[])]
        fn extracts_tags_matching_the_expected_set(
            #[case] input: &str,
            #[case] expected: &[&str],
        ) {
            let expected: Vec<Tag> =
                expected.iter().map(|tag| Tag::parse(tag).unwrap()).collect();

            assert_eq!(scan_tags(input), expected);
        }

        #[test]
        fn skips_mid_word_hashes_and_non_alpha_initials() {
            let tags = scan_tags("foo#bar #123 #_not_alpha #valid");
            assert_eq!(tags, vec![Tag::parse("#valid").unwrap()]);
        }

        #[test]
        fn finds_tags_inside_field_values_the_item_lexer_swallows() {
            let tags = scan_tags("Status:: Draft #urgent");

            assert_eq!(tags, vec![Tag::parse("#urgent").unwrap()]);
        }
    }
}
