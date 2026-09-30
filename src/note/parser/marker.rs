//! Custom item-leading task marker scanner.
//!
//! The functions here are the sole source of truth for task marker identity.
//! They mirror `pulldown-cmark`'s `scan_task_list_marker` first-pass gating:
//! the marker is only valid at a list item's content start, followed by one
//! ASCII whitespace character. Because that whitespace is frequently the item's
//! line terminator (which never reaches the parser as a
//! [`pulldown_cmark::Event::Text`] chunk), [`scan_marker_at_line_end`] treats
//! end-of-input as the trailing whitespace.
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
/// item's end), so `- [x]` as an entire item still carries a marker, exactly as
/// pulldown-cmark treats the line terminator as whitespace. Returns `None` when
/// `text` is not a complete `[<char>]` marker shape.
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

#[inline]
fn append_buffers(
    text: &str,
    text_buffer: &mut String,
    scan_buffer: &mut String,
    in_code_block: bool,
) {
    text_buffer.push_str(text);
    if !in_code_block {
        scan_buffer.push_str(text);
    }
}

fn char_boundary_le(text: &str, max: usize) -> usize {
    let mut boundary = 0;
    for (offset, _) in text.char_indices() {
        if offset <= max {
            boundary = offset;
        } else {
            break;
        }
    }
    boundary
}
/// Classification of a decided item leading marker.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(super) enum ItemMarker {
    Plain,
    Marked(char),
}

/// Fixed-size stack accumulator for leading task marker recognition.
///
/// Holds at most 8 bytes on the stack (`[` + 4-byte UTF-8 character + `]` +
/// 1-byte whitespace). Marker characters are never written to the item text
/// buffer.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(super) enum MarkerAccumulator {
    Buffering {
        buf: [u8; 8],
        len: u8,
    },
    Decided(ItemMarker),
}

impl MarkerAccumulator {
    /// Creates a new buffering accumulator.
    #[inline]
    #[must_use]
    pub(super) const fn new() -> Self {
        Self::Buffering {
            buf: [0; 8],
            len: 0,
        }
    }

    /// Returns `true` if a task marker was recognized.
    #[inline]
    #[must_use]
    pub(super) const fn is_marked(&self) -> bool {
        matches!(self, Self::Decided(ItemMarker::Marked(_)))
    }

    /// Returns the recognized marker symbol, or `None`.
    #[inline]
    #[must_use]
    pub(super) const fn marker_symbol(&self) -> Option<char> {
        match self {
            Self::Decided(ItemMarker::Marked(ch)) => Some(*ch),
            _ => None,
        }
    }

    /// Feeds incoming text into the accumulator.
    ///
    /// Marker characters are withheld in `buf` and never written to
    /// `text_buffer` or `scan_buffer`. Once decided as `Marked`, only
    /// subsequent text is appended. If rejected, buffered bytes are flushed and
    /// `text` is appended.
    pub(super) fn push_text(
        &mut self,
        text: &str,
        text_buffer: &mut String,
        scan_buffer: &mut String,
        in_code_block: bool,
    ) {
        match self {
            Self::Decided(_) => {
                append_buffers(text, text_buffer, scan_buffer, in_code_block);
            }
            Self::Buffering {
                ..
            } => {
                self.push_buffering(
                    text,
                    text_buffer,
                    scan_buffer,
                    in_code_block,
                );
            }
        }
    }

    fn push_buffering(
        &mut self,
        text: &str,
        text_buffer: &mut String,
        scan_buffer: &mut String,
        in_code_block: bool,
    ) {
        let Self::Buffering {
            buf,
            len,
        } = self
        else {
            return;
        };

        if *len == 0 && !text.starts_with('[') {
            *self = Self::Decided(ItemMarker::Plain);
            append_buffers(text, text_buffer, scan_buffer, in_code_block);
            return;
        }

        let current_len = usize::from(*len);
        let remaining = 8usize.saturating_sub(current_len);
        let take_bytes = if text.len() <= remaining {
            text.len()
        } else {
            char_boundary_le(text, remaining)
        };

        if take_bytes == 0 {
            self.flush_and_decide_plain(
                text,
                text_buffer,
                scan_buffer,
                in_code_block,
            );
            return;
        }

        let next_len = current_len.saturating_add(take_bytes);
        if let (Some(target), Some(src)) = (
            buf.get_mut(current_len..next_len),
            text.as_bytes().get(..take_bytes),
        ) {
            target.copy_from_slice(src);
        }
        if let Ok(added) = u8::try_from(take_bytes) {
            *len = len.saturating_add(added);
        }

        let candidate = buf
            .get(..usize::from(*len))
            .and_then(|slice| std::str::from_utf8(slice).ok())
            .unwrap_or_default();
        match scan_marker_prefix(candidate) {
            MarkerPrefix::Complete(scan) => {
                let symbol = scan.symbol();
                append_buffers(
                    scan.remainder(),
                    text_buffer,
                    scan_buffer,
                    in_code_block,
                );
                if let Some(rest) = text.get(take_bytes..) {
                    append_buffers(
                        rest,
                        text_buffer,
                        scan_buffer,
                        in_code_block,
                    );
                }
                *self = Self::Decided(ItemMarker::Marked(symbol));
            }
            MarkerPrefix::Rejected => {
                self.flush_and_decide_plain(
                    text.get(take_bytes..).unwrap_or_default(),
                    text_buffer,
                    scan_buffer,
                    in_code_block,
                );
            }
            MarkerPrefix::Incomplete => {
                if take_bytes < text.len() {
                    self.flush_and_decide_plain(
                        text.get(take_bytes..).unwrap_or_default(),
                        text_buffer,
                        scan_buffer,
                        in_code_block,
                    );
                }
            }
        }
    }

    fn flush_and_decide_plain(
        &mut self,
        trailing: &str,
        text_buffer: &mut String,
        scan_buffer: &mut String,
        in_code_block: bool,
    ) {
        if let Self::Buffering {
            buf,
            len,
        } = self
        {
            if let Some(slice) = buf.get(..usize::from(*len))
                && let Ok(buffered_str) = std::str::from_utf8(slice)
            {
                text_buffer.push_str(buffered_str);
                scan_buffer.push_str(buffered_str);
            }
            append_buffers(trailing, text_buffer, scan_buffer, in_code_block);
            *self = Self::Decided(ItemMarker::Plain);
        }
    }

    /// Rejects any pending marker, flushing buffered bytes to `text_buffer` and
    /// `scan_buffer`.
    pub(super) fn reject(
        &mut self,
        text_buffer: &mut String,
        scan_buffer: &mut String,
    ) {
        if let Self::Buffering {
            buf,
            len,
        } = self
        {
            if *len > 0
                && let Some(slice) = buf.get(..usize::from(*len))
                && let Ok(buffered_str) = std::str::from_utf8(slice)
            {
                text_buffer.push_str(buffered_str);
                scan_buffer.push_str(buffered_str);
            }
            *self = Self::Decided(ItemMarker::Plain);
        }
    }

    /// Resolves any pending marker using line-end semantics.
    pub(super) fn resolve_at_line_end(
        &mut self,
        text_buffer: &mut String,
        scan_buffer: &mut String,
    ) {
        if let Self::Buffering {
            buf,
            len,
        } = self
        {
            if *len == 0 {
                *self = Self::Decided(ItemMarker::Plain);
                return;
            }
            if let Some(slice) = buf.get(..usize::from(*len))
                && let Ok(buffered_str) = std::str::from_utf8(slice)
            {
                if let Some(scan) = scan_marker_at_line_end(buffered_str) {
                    *self = Self::Decided(ItemMarker::Marked(scan.symbol()));
                    return;
                }
                text_buffer.push_str(buffered_str);
                scan_buffer.push_str(buffered_str);
            }
            *self = Self::Decided(ItemMarker::Plain);
        }
    }
}

impl Default for MarkerAccumulator {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
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

    mod accumulator {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn starts_plain_on_non_bracket() {
            let mut acc = MarkerAccumulator::new();
            let mut tb = String::new();
            let mut sb = String::new();
            acc.push_text("hello world", &mut tb, &mut sb, false);
            assert!(!acc.is_marked());
            assert_eq!(acc.marker_symbol(), None);
            assert_eq!(tb, "hello world");
            assert_eq!(sb, "hello world");
        }

        #[test]
        fn recognizes_marker_in_single_chunk() {
            let mut acc = MarkerAccumulator::new();
            let mut tb = String::new();
            let mut sb = String::new();
            acc.push_text("[x] hello", &mut tb, &mut sb, false);
            assert!(acc.is_marked());
            assert_eq!(acc.marker_symbol(), Some('x'));
            assert_eq!(tb, "hello");
            assert_eq!(sb, "hello");
        }

        #[test]
        fn recognizes_marker_split_across_chunks() {
            let mut acc = MarkerAccumulator::new();
            let mut tb = String::new();
            let mut sb = String::new();
            acc.push_text("[", &mut tb, &mut sb, false);
            assert_eq!(tb, "");
            acc.push_text("x", &mut tb, &mut sb, false);
            assert_eq!(tb, "");
            acc.push_text("]", &mut tb, &mut sb, false);
            assert_eq!(tb, "");
            acc.push_text(" task", &mut tb, &mut sb, false);
            assert!(acc.is_marked());
            assert_eq!(acc.marker_symbol(), Some('x'));
            assert_eq!(tb, "task");
            assert_eq!(sb, "task");
        }

        #[test]
        fn recognizes_bare_marker_at_line_end() {
            let mut acc = MarkerAccumulator::new();
            let mut tb = String::new();
            let mut sb = String::new();
            acc.push_text("[x]", &mut tb, &mut sb, false);
            assert_eq!(tb, "");
            acc.resolve_at_line_end(&mut tb, &mut sb);
            assert!(acc.is_marked());
            assert_eq!(acc.marker_symbol(), Some('x'));
            assert_eq!(tb, "");
            assert_eq!(sb, "");
        }

        #[test]
        fn rejects_unclosed_marker_at_line_end() {
            let mut acc = MarkerAccumulator::new();
            let mut tb = String::new();
            let mut sb = String::new();
            acc.push_text("[x", &mut tb, &mut sb, false);
            assert_eq!(tb, "");
            acc.resolve_at_line_end(&mut tb, &mut sb);
            assert!(!acc.is_marked());
            assert_eq!(acc.marker_symbol(), None);
            assert_eq!(tb, "[x");
            assert_eq!(sb, "[x");
        }

        #[test]
        fn rejects_pending_marker_on_reject() {
            let mut acc = MarkerAccumulator::new();
            let mut tb = String::new();
            let mut sb = String::new();
            acc.push_text("[", &mut tb, &mut sb, false);
            assert_eq!(tb, "");
            acc.reject(&mut tb, &mut sb);
            assert!(!acc.is_marked());
            assert_eq!(acc.marker_symbol(), None);
            assert_eq!(tb, "[");
            assert_eq!(sb, "[");
        }

        #[test]
        fn rejects_invalid_marker_immediately() {
            let mut acc = MarkerAccumulator::new();
            let mut tb = String::new();
            let mut sb = String::new();
            acc.push_text("[xx] foo", &mut tb, &mut sb, false);
            assert!(!acc.is_marked());
            assert_eq!(acc.marker_symbol(), None);
            assert_eq!(tb, "[xx] foo");
            assert_eq!(sb, "[xx] foo");
        }
    }
}
