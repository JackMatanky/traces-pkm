//! Tag token scanning and filter matching for Markdown text buffers.

use logos::{Filter, Lexer, Logos};

use super::lexer::char_before;
use crate::Tag;

/// Token stream for Markdown tags in free-form text.
///
/// - [`Self::Tag`] carries an emitted [`Tag`].
/// - [`Self::Ignored`] skips ordinary text.
///
/// [`tag_callback`] returns [`Filter::Skip`] to reject non-tag `#` characters
/// without swallowing the rest of the text.
#[derive(Clone, Debug, PartialEq, Logos)]
enum TagToken {
    #[token("#", tag_callback)]
    Tag(Tag),
    #[regex(r"[\s\S]", priority = 0)]
    Ignored,
}

/// Parses a Markdown tag after its already-consumed leading `#`.
///
/// Rejects a mid-word `#`, such as `foo#bar`, and a `#` not followed by an
/// alphabetic character, such as `#1`.
fn tag_callback(lex: &mut Lexer<'_, TagToken>) -> Filter<Tag> {
    let preceded_by_word_char =
        char_before(lex).is_some_and(|ch| ch.is_alphanumeric() || ch == '_');
    if preceded_by_word_char {
        return Filter::Skip;
    }
    let tag_start = lex.span().start;
    let Some(tail) = lex.source().get(tag_start..) else {
        return Filter::Skip;
    };
    let Some(tag_len) = Tag::prefix_len(tail) else {
        return Filter::Skip;
    };
    lex.bump(tag_len.saturating_sub('#'.len_utf8()));
    match Tag::parse(lex.slice()) {
        Ok(tag) => Filter::Emit(tag),
        Err(_) => Filter::Skip,
    }
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

        use super::*;

        #[test]
        fn extracts_valid_tags_preserving_order() {
            let tags = scan_tags("hello #rust and #dev/tools world");
            assert_eq!(tags, vec![
                Tag::parse("#rust").unwrap(),
                Tag::parse("#dev/tools").unwrap(),
            ]);
        }

        #[test]
        fn skips_mid_word_hashes_and_non_alpha_initials() {
            let tags = scan_tags("foo#bar #123 #_not_alpha #valid");
            assert_eq!(tags, vec![Tag::parse("#valid").unwrap()]);
        }
    }
}
