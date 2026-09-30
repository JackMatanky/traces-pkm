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

/// Scans `text` for configured task tag filters and records their byte spans.
pub(super) fn find_tag_filter_spans(
    text: &str,
    tag_filters: &[Tag],
    spans: &mut Vec<(usize, usize)>,
) {
    let mut iter = text.char_indices().peekable();
    let mut prev_char: Option<char> = None;

    while let Some((idx, ch)) = iter.next() {
        let is_word_char =
            prev_char.is_some_and(|c| c.is_alphanumeric() || c == '_');
        prev_char = Some(ch);
        if ch != '#' || is_word_char {
            continue;
        }
        if let Some((start, end, candidate)) =
            scan_tag_candidate(text, idx, &mut iter)
            && tag_filters.iter().any(|filter| filter.as_str() == candidate)
        {
            spans.push((start, end));
        }
    }
}

/// Scans a single tag candidate starting at `start_idx`.
fn scan_tag_candidate<'a>(
    text: &'a str,
    start_idx: usize,
    iter: &mut std::iter::Peekable<std::str::CharIndices<'_>>,
) -> Option<(usize, usize, &'a str)> {
    let tag_len = Tag::prefix_len(text.get(start_idx..)?)?;
    let tag_end = start_idx.saturating_add(tag_len);
    while iter.peek().is_some_and(|(idx, _)| *idx < tag_end) {
        iter.next();
    }
    let candidate = text.get(start_idx..tag_end)?;
    Some((start_idx, tag_end, candidate))
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

    mod find_tag_filter_spans {
        use pretty_assertions::assert_eq;

        use super::*;
        #[test]
        fn locates_exact_tag_matches() {
            let filters =
                [Tag::parse("#task").unwrap(), Tag::parse("#urgent").unwrap()];
            let mut spans = Vec::new();
            find_tag_filter_spans(
                "a #task and #urgent item",
                &filters,
                &mut spans,
            );
            assert_eq!(spans, vec![(2, 7), (12, 19)]);
        }

        #[test]
        fn ignores_non_matching_and_subtags() {
            let filters = [Tag::parse("#task").unwrap()];
            let mut spans = Vec::new();
            find_tag_filter_spans(
                "a #task/sub and #other item",
                &filters,
                &mut spans,
            );
            assert_eq!(spans, vec![]);
        }
    }
}
