//! Task list marker scanner and accumulator.
//!
//! Identifies and parses `[<symbol>]` task markers at the start of list items,
//! following `CommonMark` and Obsidian whitespace rules.
use super::list::ItemBuffers;
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

/// Classifies `text` as an item-leading marker prefix.
///
/// Evaluates whether `text` forms a complete marker (`[<symbol>] `), a partial
/// prefix awaiting additional characters, or an invalid marker sequence.
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

/// Parses `text` as an exact `[<symbol>]` marker shape.
///
/// Accepts single-character and multibyte symbols. The closing bracket (`]`) is
/// not a valid symbol character.
fn split_marker_exact(text: &str) -> Option<char> {
    let inner = text.strip_prefix(OPEN_BRACKET)?.strip_suffix(CLOSE_BRACKET)?;
    let mut chars = inner.chars();
    match (chars.next(), chars.next()) {
        (Some(symbol), None) if symbol != CLOSE_BRACKET => Some(symbol),
        _ => None,
    }
}

/// Scans `text` for an item-leading marker, treating end-of-input as trailing
/// whitespace.
///
/// A list item line ending structurally (such as by a nested list or line
/// break) satisfies the trailing whitespace requirement without an explicit
/// whitespace event.
///
/// Returns `Some` if `text` is a complete marker, or `None` otherwise.
#[inline]
#[must_use]
pub(super) fn scan_marker_at_line_end(text: &str) -> Option<MarkerScan<'_>> {
    match scan_marker_prefix(text) {
        MarkerPrefix::Complete(scan) => Some(scan),
        MarkerPrefix::Incomplete => {
            split_marker_exact(text).map(|symbol| MarkerScan {
                symbol,
                remainder: "",
            })
        }
        MarkerPrefix::Rejected => None,
    }
}

/// Returns `true` if `ch` counts as a task marker's trailing whitespace.
///
/// Matches ASCII whitespace characters only (` `, `\t`, `\n`, `\r`, vertical
/// tab, form feed).
#[inline]
#[must_use]
const fn is_marker_whitespace(ch: char) -> bool {
    matches!(ch, ' ' | '\t' | '\n' | '\u{0B}' | '\u{0C}' | '\r')
}

/// Returns the largest character-start offset in `text` not exceeding `max`.
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
/// Buffers up to 8 leading bytes of an item until marker classification is
/// resolved. Marker syntax is withheld from item display buffers.
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

    /// Appends incoming text to the accumulator and forwards resolved text to
    /// `buffers`.
    ///
    /// Resolution behavior:
    /// - When marked, subsequent text is appended to `buffers`.
    /// - When rejected, withheld bytes and new text are flushed to `buffers`.
    pub(super) fn push_text(
        &mut self,
        text: &str,
        buffers: &mut ItemBuffers,
        in_code_block: bool,
    ) {
        match self {
            Self::Decided(_) => {
                buffers.append(text, in_code_block);
            }
            Self::Buffering {
                ..
            } => {
                self.push_buffering(text, buffers, in_code_block);
            }
        }
    }

    /// Buffers incoming text while the leading marker is still undecided.
    fn push_buffering(
        &mut self,
        text: &str,
        buffers: &mut ItemBuffers,
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
            buffers.append(text, in_code_block);
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
            self.flush_and_decide_plain(text, buffers, in_code_block);
            return;
        }

        for (slot, byte) in buf
            .iter_mut()
            .skip(current_len)
            .zip(text.as_bytes().iter().take(take_bytes))
        {
            *slot = *byte;
        }
        *len = len.saturating_add(u8::try_from(take_bytes).unwrap_or(0));

        let candidate = Self::buffered_str(buf, *len);
        match scan_marker_prefix(candidate) {
            MarkerPrefix::Complete(scan) => {
                let symbol = scan.symbol();
                buffers.append(scan.remainder(), in_code_block);
                if let Some(rest) = text.get(take_bytes..) {
                    buffers.append(rest, in_code_block);
                }
                *self = Self::Decided(ItemMarker::Marked(symbol));
            }
            MarkerPrefix::Rejected => {
                self.flush_and_decide_plain(
                    text.get(take_bytes..).unwrap_or_default(),
                    buffers,
                    in_code_block,
                );
            }
            MarkerPrefix::Incomplete => {
                if take_bytes < text.len() {
                    self.flush_and_decide_plain(
                        text.get(take_bytes..).unwrap_or_default(),
                        buffers,
                        in_code_block,
                    );
                }
            }
        }
    }

    /// Returns the buffered marker bytes as a string slice.
    fn buffered_str(buf: &[u8; 8], len: u8) -> &str {
        let clamped = usize::from(len.min(8));
        let slice = buf.get(..clamped).unwrap_or(buf.as_slice());
        std::str::from_utf8(slice).unwrap_or_default()
    }

    /// Flushes buffered bytes plus `trailing` to both buffers and records the
    /// item as plain text.
    fn flush_and_decide_plain(
        &mut self,
        trailing: &str,
        buffers: &mut ItemBuffers,
        in_code_block: bool,
    ) {
        if let Self::Buffering {
            buf,
            len,
        } = self
        {
            let buffered = Self::buffered_str(buf, *len);
            buffers.append_verbatim(buffered);
        }
        buffers.append(trailing, in_code_block);
        *self = Self::Decided(ItemMarker::Plain);
    }

    /// Rejects any pending marker, flushing buffered bytes to `buffers`.
    pub(super) fn reject(&mut self, buffers: &mut ItemBuffers) {
        self.flush_and_decide_plain("", buffers, false);
    }

    /// Resolves any pending marker using line-end semantics.
    pub(super) fn resolve_at_line_end(&mut self, buffers: &mut ItemBuffers) {
        let Self::Buffering {
            buf,
            len,
        } = self
        else {
            return;
        };
        let buffered = Self::buffered_str(buf, *len);
        if let Some(scan) = scan_marker_at_line_end(buffered) {
            *self = Self::Decided(ItemMarker::Marked(scan.symbol()));
            return;
        }
        self.flush_and_decide_plain("", buffers, false);
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
    #[case::text_after_marker("[x]x")]
    fn rejects_non_markers_at_line_end(#[case] text: &str) {
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
            let mut buffers = ItemBuffers::new();
            acc.push_text("hello world", &mut buffers, false);
            assert!(!acc.is_marked());
            assert_eq!(acc.marker_symbol(), None);
            assert_eq!(buffers.text(), "hello world");
            assert_eq!(buffers.scan(), "hello world");
        }

        #[test]
        fn recognizes_marker_in_single_chunk() {
            let mut acc = MarkerAccumulator::new();
            let mut buffers = ItemBuffers::new();
            acc.push_text("[x] hello", &mut buffers, false);
            assert!(acc.is_marked());
            assert_eq!(acc.marker_symbol(), Some('x'));
            assert_eq!(buffers.text(), "hello");
            assert_eq!(buffers.scan(), "hello");
        }

        #[test]
        fn recognizes_marker_split_across_chunks() {
            let mut acc = MarkerAccumulator::new();
            let mut buffers = ItemBuffers::new();
            acc.push_text("[", &mut buffers, false);
            assert_eq!(buffers.text(), "");
            acc.push_text("x", &mut buffers, false);
            assert_eq!(buffers.text(), "");
            acc.push_text("]", &mut buffers, false);
            assert_eq!(buffers.text(), "");
            acc.push_text(" task", &mut buffers, false);
            assert!(acc.is_marked());
            assert_eq!(acc.marker_symbol(), Some('x'));
            assert_eq!(buffers.text(), "task");
            assert_eq!(buffers.scan(), "task");
        }

        #[test]
        fn preserves_a_multibyte_symbol_split_across_chunks() {
            let mut acc = MarkerAccumulator::new();
            let mut buffers = ItemBuffers::new();
            acc.push_text("[✓", &mut buffers, false);
            assert_eq!(buffers.text(), "");
            acc.push_text("] done", &mut buffers, false);
            assert!(acc.is_marked());
            assert_eq!(acc.marker_symbol(), Some('✓'));
            assert_eq!(buffers.text(), "done");
            assert_eq!(buffers.scan(), "done");
        }

        #[test]
        fn recognizes_bare_marker_at_line_end() {
            let mut acc = MarkerAccumulator::new();
            let mut buffers = ItemBuffers::new();
            acc.push_text("[x]", &mut buffers, false);
            assert_eq!(buffers.text(), "");
            acc.resolve_at_line_end(&mut buffers);
            assert!(acc.is_marked());
            assert_eq!(acc.marker_symbol(), Some('x'));
            assert_eq!(buffers.text(), "");
            assert_eq!(buffers.scan(), "");
        }

        #[test]
        fn rejects_unclosed_marker_at_line_end() {
            let mut acc = MarkerAccumulator::new();
            let mut buffers = ItemBuffers::new();
            acc.push_text("[x", &mut buffers, false);
            assert_eq!(buffers.text(), "");
            acc.resolve_at_line_end(&mut buffers);
            assert!(!acc.is_marked());
            assert_eq!(acc.marker_symbol(), None);
            assert_eq!(buffers.text(), "[x");
            assert_eq!(buffers.scan(), "[x");
        }

        #[test]
        fn rejects_pending_marker_on_reject() {
            let mut acc = MarkerAccumulator::new();
            let mut buffers = ItemBuffers::new();
            acc.push_text("[", &mut buffers, false);
            assert_eq!(buffers.text(), "");
            acc.reject(&mut buffers);
            assert!(!acc.is_marked());
            assert_eq!(acc.marker_symbol(), None);
            assert_eq!(buffers.text(), "[");
            assert_eq!(buffers.scan(), "[");
        }

        #[test]
        fn rejects_invalid_marker_immediately() {
            let mut acc = MarkerAccumulator::new();
            let mut buffers = ItemBuffers::new();
            acc.push_text("[xx] foo", &mut buffers, false);
            assert!(!acc.is_marked());
            assert_eq!(acc.marker_symbol(), None);
            assert_eq!(buffers.text(), "[xx] foo");
            assert_eq!(buffers.scan(), "[xx] foo");
        }
    }
}
