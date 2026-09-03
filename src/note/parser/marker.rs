//! Custom item-leading task marker scanner.
//!
//! The functions here are the sole source of truth for task marker identity.
//! They mirror `pulldown-cmark`'s `scan_task_list_marker` first-pass gating:
//! the marker is only valid at a list item's content start, followed by one
//! ASCII whitespace character. Because that whitespace is frequently the
//! item's line terminator (which never reaches the parser as a
//! [`pulldown_cmark::Event::Text`] chunk), [`scan_marker_at_line_end`]
//! treats end-of-input as the trailing whitespace.
use crate::DelimiterType;

/// Opening bracket character for task markers (`[`).
const OPEN_BRACKET: char = match DelimiterType::Bracket.open_char() {
    Some(ch) => ch,
    None => '[',
};

/// Closing bracket character for task markers (`]`).
const CLOSE_BRACKET: char = match DelimiterType::Bracket.close_char() {
    Some(ch) => ch,
    None => ']',
};
/// A recognized item-leading task marker.
///
/// `symbol` is the character inside `[<symbol>]`; `remainder` is the scanned
/// text with the marker and its single trailing ASCII whitespace character
/// trimmed.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(super) struct MarkerScan<'a> {
    symbol: char,
    remainder: &'a str,
}

impl MarkerScan<'_> {
    /// Returns the character inside the marker's brackets.
    #[inline]
    #[must_use]
    pub(super) const fn symbol(&self) -> char {
        self.symbol
    }

    /// Returns the text after the marker and its trailing whitespace.
    #[inline]
    #[must_use]
    pub(super) const fn remainder(&self) -> &str {
        self.remainder
    }
}

/// Classification of assembled item-leading text against the marker shape:
/// `[`, one non-`]` character, `]`, then one ASCII whitespace character.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(super) enum MarkerPrefix<'a> {
    /// `text` is a strict prefix of a potential marker (or exactly `[<char>]`
    /// awaiting its trailing whitespace); keep accumulating text.
    Incomplete,
    /// `text` cannot become an item-leading marker.
    Rejected,
    /// A complete marker was recognized.
    Complete(MarkerScan<'a>),
}

/// Classifies `text` as an item-leading marker prefix, tolerating truncation at
/// every position.
///
/// Markdown may split a leading `[<char>] ` marker across several text chunks
/// (observed: `"["`, `"x"`, `"]"`, `" Task text"`), so the parser feeds every
/// leading chunk through this function until it decides.
#[inline]
#[must_use]
pub(super) fn scan_marker_prefix(text: &str) -> MarkerPrefix<'_> {
    let mut chars = text.char_indices();
    if !matches!(chars.next(), Some((_, OPEN_BRACKET))) {
        return MarkerPrefix::Rejected;
    }
    let symbol = match chars.next() {
        None => return MarkerPrefix::Incomplete,
        Some((_, ch)) if ch != CLOSE_BRACKET => ch,
        Some(_) => return MarkerPrefix::Rejected,
    };
    match chars.next() {
        None => return MarkerPrefix::Incomplete,
        Some((_, ch)) if ch != CLOSE_BRACKET => return MarkerPrefix::Rejected,
        _ => {}
    }
    match chars.next() {
        None => MarkerPrefix::Incomplete,
        Some((ws_offset, ws)) if is_marker_whitespace(ws) => {
            let remainder_start = ws_offset.saturating_add(ws.len_utf8());
            MarkerPrefix::Complete(MarkerScan {
                symbol,
                remainder: text.get(remainder_start..).unwrap_or_default(),
            })
        }
        Some(_) => MarkerPrefix::Rejected,
    }
}

/// Scans `text` for an item-leading marker, treating end-of-input as the
/// trailing whitespace.
///
/// A list item's line ends without a whitespace [`Event::Text`] chunk (the
/// newline is consumed structurally by a nested list, a soft break, or the
/// item's end), so `- [x]` as an entire item still carries a marker, exactly
/// as pulldown-cmark treats the line terminator as whitespace. Returns `None`
/// when `text` is not a complete `[<char>]` marker shape.
///
/// [`Event::Text`]: pulldown_cmark::Event::Text
#[inline]
#[must_use]
pub(super) fn scan_marker_at_line_end(text: &str) -> Option<MarkerScan<'_>> {
    match scan_marker_prefix(text) {
        MarkerPrefix::Complete(scan) => Some(scan),
        MarkerPrefix::Incomplete => {
            let mut chars = text.chars();
            match (chars.next(), chars.next(), chars.next(), chars.next()) {
                (Some(o), Some(symbol), Some(c), None)
                    if o == OPEN_BRACKET
                        && c == CLOSE_BRACKET
                        && symbol != CLOSE_BRACKET =>
                {
                    Some(MarkerScan {
                        symbol,
                        remainder: "",
                    })
                }
                _ => None,
            }
        }
        MarkerPrefix::Rejected => None,
    }
}

/// Whether `ch` counts as the marker's trailing whitespace.
///
/// ASCII whitespace only, mirroring `pulldown-cmark`'s `is_ascii_whitespace`:
/// Unicode spaces such as NBSP are ordinary text and do not complete a marker.
#[inline]
#[must_use]
const fn is_marker_whitespace(ch: char) -> bool {
    matches!(ch, ' ' | '\t' | '\n' | '\u{0B}' | '\u{0C}' | '\r')
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case::space(' ')]
    #[case::lowercase_done('x')]
    #[case::uppercase_done('X')]
    #[case::in_progress('/')]
    #[case::cancelled('-')]
    #[case::on_hold('!')]
    #[case::unknown('?')]
    fn recognizes_every_marker_symbol_at_item_leading_position(
        #[case] symbol: char,
    ) {
        let text = format!("[{symbol}] Task text");

        assert_eq!(
            scan_marker_prefix(&text),
            MarkerPrefix::Complete(MarkerScan {
                symbol,
                remainder: "Task text",
            }),
            "[{symbol}] must be recognized"
        );
    }

    #[test]
    fn preserves_a_multibyte_marker_symbol() {
        assert_eq!(
            scan_marker_prefix("[✓] Done"),
            MarkerPrefix::Complete(MarkerScan {
                symbol: '✓',
                remainder: "Done",
            })
        );
    }

    #[test]
    fn consumes_exactly_one_trailing_whitespace_character() {
        assert_eq!(
            scan_marker_prefix("[x]  extra space"),
            MarkerPrefix::Complete(MarkerScan {
                symbol: 'x',
                remainder: " extra space",
            })
        );
    }

    #[rstest]
    #[case::open_bracket("[")]
    #[case::symbol_only("[x")]
    #[case::closed_marker("[x]")]
    fn treats_truncated_markers_as_incomplete(#[case] text: &str) {
        assert_eq!(
            scan_marker_prefix(text),
            MarkerPrefix::Incomplete,
            "{text:?} must stay pending"
        );
    }

    #[rstest]
    #[case::empty("")]
    #[case::unicode_whitespace("[x]\u{00A0}Task")]
    #[case::empty_marker("[] Task")]
    #[case::multi_character_marker("[xx] Task")]
    #[case::no_leading_bracket("Task without a marker")]
    #[case::unclosed_marker("[x Task")]
    #[case::bracket_text_not_at_start("Check [x] later")]
    fn rejects_text_that_cannot_become_a_marker(#[case] text: &str) {
        assert_eq!(
            scan_marker_prefix(text),
            MarkerPrefix::Rejected,
            "{text:?} must be rejected"
        );
    }

    #[rstest]
    #[case::plain_text("Task")]
    #[case::open_bracket("[")]
    #[case::symbol_only("[x")]
    fn rejects_truncated_non_markers_at_line_end(#[case] text: &str) {
        assert_eq!(scan_marker_at_line_end(text), None);
    }

    #[test]
    fn treats_end_of_input_as_the_trailing_whitespace_at_line_end() {
        let scan =
            scan_marker_at_line_end("[x]").expect("bare marker recognized");

        assert_eq!(scan.symbol(), 'x');
        assert_eq!(scan.remainder(), "");
    }

    #[test]
    fn still_recognizes_a_complete_marker_at_line_end() {
        let scan = scan_marker_at_line_end("[x] Task")
            .expect("complete marker recognized");

        assert_eq!(scan.symbol(), 'x');
        assert_eq!(scan.remainder(), "Task");
    }

    #[test]
    fn rejects_a_unicode_whitespace_terminator() {
        assert_eq!(
            scan_marker_prefix("[x]\u{00A0}Task"),
            MarkerPrefix::Rejected
        );
    }

    #[rstest]
    #[case::space(' ')]
    #[case::tab('\t')]
    #[case::newline('\n')]
    #[case::vertical_tab('\u{0B}')]
    #[case::form_feed('\u{0C}')]
    #[case::carriage_return('\r')]
    fn accepts_every_ascii_whitespace_terminator(#[case] ws: char) {
        let text = format!("[x]{ws}Task");

        assert_eq!(
            scan_marker_prefix(&text),
            MarkerPrefix::Complete(MarkerScan {
                symbol: 'x',
                remainder: "Task",
            }),
            "ASCII whitespace {ws:?} must complete the marker"
        );
    }
}
