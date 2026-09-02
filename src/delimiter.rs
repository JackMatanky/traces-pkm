//! Balanced delimiter tracking and string-level delimiter scanning.
//!
//! Provides zero-allocation stack validation for paired delimiters
//! ([`DelimiterType`], [`DelimiterStack`]) and quote states ([`QuoteType`]),
//! with support for parentheses, square brackets, curly braces, and Obsidian
//! wikilink double brackets.

const MAX_DELIMITER_DEPTH: usize = 16;

/// A stack-allocated delimiter validator supporting nested pairs and quote
/// states.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct DelimiterStack {
    entries: [DelimiterType; MAX_DELIMITER_DEPTH],
    len: usize,
    active_quote: Option<QuoteType>,
}

impl DelimiterStack {
    /// Creates a stack initialized with an outer root expected delimiter.
    #[inline]
    #[must_use]
    pub(crate) fn with_root(kind: DelimiterType) -> Self {
        let mut stack = Self {
            entries: [DelimiterType::Parenthesis; MAX_DELIMITER_DEPTH],
            len: 0,
            active_quote: None,
        };
        stack.push(kind);
        stack
    }

    /// Pushes a nested delimiter kind onto the stack.
    #[inline]
    pub(crate) fn push(&mut self, kind: DelimiterType) {
        if let Some(entry) = self.entries.get_mut(self.len) {
            *entry = kind;
            self.len = self.len.saturating_add(1);
        }
    }

    /// Returns the active quote kind if scanning inside a string literal.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "kept for DelimiterStack inspection; tested in unit suite"
        )
    )]
    pub(crate) const fn active_quote(&self) -> Option<QuoteType> {
        self.active_quote
    }

    /// Returns the current delimiter kind at the top of the stack.
    #[inline]
    #[must_use]
    pub(crate) fn current_kind(&self) -> Option<DelimiterType> {
        let index = self.len.checked_sub(1)?;
        self.entries.get(index).copied()
    }

    /// Updates active quote state on encountering `ch`.
    ///
    /// Returns `true` if `ch` was consumed as part of quote tracking.
    #[inline]
    pub(crate) fn advance_quote_state(&mut self, ch: char) -> bool {
        if let Some(active_quote) = self.active_quote {
            if ch == active_quote.quote_char() {
                self.active_quote = None;
            }
            true
        } else if let Some(quote) = QuoteType::from_char(ch) {
            self.active_quote = Some(quote);
            true
        } else {
            false
        }
    }

    /// Checks for a double-bracket closer `]]`.
    ///
    /// Returns `Some(true)` if the root double bracket was cleanly closed,
    /// `Some(false)` if an inner double bracket was closed, or `None` if
    /// `rest` does not start with `]]` matching an active double bracket.
    #[inline]
    pub(crate) fn check_double_bracket_close(
        &mut self,
        rest: &str,
    ) -> Option<bool> {
        if self.current_kind() == Some(DelimiterType::DoubleBracket)
            && rest.starts_with("]]")
        {
            self.len = self.len.saturating_sub(1);
            Some(self.len == 0)
        } else {
            None
        }
    }

    /// Handles a single closing character.
    ///
    /// - Returns `Ok(true)` if the root delimiter was cleanly closed.
    /// - Returns `Ok(false)` if an inner nested delimiter was closed or if `ch`
    ///   is allowed content (e.g. lone `]` inside double brackets).
    ///
    /// # Errors
    ///
    /// - `Err(())` if `ch` mismatched the expected closing delimiter.
    #[inline]
    pub(crate) fn handle_char_close(&mut self, ch: char) -> Result<bool, ()> {
        let Some(current) = self.current_kind() else {
            return Err(());
        };
        if current.matches_char_close(ch) {
            self.len = self.len.saturating_sub(1);
            Ok(self.len == 0)
        } else if current == DelimiterType::DoubleBracket && ch == ']' {
            Ok(false)
        } else {
            Err(())
        }
    }
}

/// Paired delimiter kinds recognized across lexers and grammar parsers.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u8)]
pub(crate) enum DelimiterType {
    /// Standard parentheses `(` and `)`.
    Parenthesis,
    /// Square brackets `[` and `]`.
    Bracket,
    /// Curly braces `{` and `}`.
    Brace,
    /// Obsidian and Markdown wikilink double brackets `[[` and `]]`.
    DoubleBracket,
}

impl DelimiterType {
    /// Returns the expected closing string representation.
    #[inline]
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "kept for DelimiterType API completeness; tested in unit \
                      suite"
        )
    )]
    pub(crate) const fn close_str(self) -> &'static str {
        match self {
            Self::Parenthesis => ")",
            Self::Bracket => "]",
            Self::Brace => "}",
            Self::DoubleBracket => "]]",
        }
    }

    /// Number of bytes in the closing delimiter.
    #[inline]
    #[must_use]
    pub(crate) const fn close_len(self) -> usize {
        match self {
            Self::Parenthesis | Self::Bracket | Self::Brace => 1,
            Self::DoubleBracket => 2,
        }
    }

    /// Returns `true` if `ch` is the single-character closer for this
    /// delimiter.
    #[inline]
    #[must_use]
    pub(crate) const fn matches_char_close(self, ch: char) -> bool {
        match (self, ch) {
            (Self::Parenthesis, ')')
            | (Self::Bracket, ']')
            | (Self::Brace, '}') => true,
            _ => false,
        }
    }

    /// Returns the single opening character for single-character delimiters,
    /// or `None` for multi-character delimiters (e.g. [`Self::DoubleBracket`]).
    #[inline]
    #[must_use]
    pub(crate) const fn open_char(self) -> Option<char> {
        match self {
            Self::Parenthesis => Some('('),
            Self::Bracket => Some('['),
            Self::Brace => Some('{'),
            Self::DoubleBracket => None,
        }
    }

    /// Returns the single closing character for single-character delimiters,
    /// or `None` for multi-character delimiters (e.g. [`Self::DoubleBracket`]).
    #[inline]
    #[must_use]
    pub(crate) const fn close_char(self) -> Option<char> {
        match self {
            Self::Parenthesis => Some(')'),
            Self::Bracket => Some(']'),
            Self::Brace => Some('}'),
            Self::DoubleBracket => None,
        }
    }

    /// Returns the delimiter kind for an opening character, if recognized.
    #[inline]
    #[must_use]
    pub(crate) const fn from_open_char(ch: char) -> Option<Self> {
        match ch {
            '(' => Some(Self::Parenthesis),
            '[' => Some(Self::Bracket),
            '{' => Some(Self::Brace),
            _ => None,
        }
    }

    /// Finds the byte offset of this delimiter's matching closing token in
    /// `input`, managing nested `()`, `[]`, `{}`, `[[]]`, and string quotes
    /// (`"..."`, `'...'`).
    ///
    /// `input` begins immediately after this opening delimiter was consumed.
    /// Returns `Some(byte_offset)` of the matching closing delimiter, or `None`
    /// if delimiters are unclosed, mismatched, or truncated.
    #[must_use]
    pub(crate) fn find_closing(self, input: &str) -> Option<usize> {
        let mut stack = DelimiterStack::with_root(self);
        let mut escaped = false;
        let mut byte_offset = 0usize;

        while byte_offset < input.len() {
            let rest = input.get(byte_offset..)?;
            let ch = rest.chars().next()?;
            let ch_len = ch.len_utf8();

            if Self::advance_escaped(ch, &mut escaped) {
                byte_offset = byte_offset.saturating_add(ch_len);
                continue;
            }

            if stack.advance_quote_state(ch) {
                byte_offset = byte_offset.saturating_add(ch_len);
                continue;
            }

            if let Some(is_closed) = stack.check_double_bracket_close(rest) {
                if is_closed {
                    return Some(byte_offset);
                }
                byte_offset = byte_offset.saturating_add(2);
                continue;
            }

            if matches!(ch, ')' | ']' | '}') {
                match stack.handle_char_close(ch) {
                    Ok(true) => return Some(byte_offset),
                    Ok(false) => {
                        byte_offset = byte_offset.saturating_add(ch_len);
                        continue;
                    }
                    Err(()) => return None,
                }
            }

            if rest.starts_with("[[") {
                stack.push(Self::DoubleBracket);
                byte_offset = byte_offset.saturating_add(2);
                continue;
            }

            if let Some(open_kind) = Self::from_open_char(ch) {
                stack.push(open_kind);
                byte_offset = byte_offset.saturating_add(ch_len);
                continue;
            }

            byte_offset = byte_offset.saturating_add(ch_len);
        }

        None
    }

    /// Advances the backslash escape state on seeing `ch`.
    ///
    /// Returns `true` if `ch` was consumed by escape processing.
    #[inline]
    fn advance_escaped(ch: char, escaped: &mut bool) -> bool {
        if *escaped {
            *escaped = false;
            true
        } else if ch == '\\' {
            *escaped = true;
            true
        } else {
            false
        }
    }
}

/// String quote kinds recognized by lexical scanners.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u8)]
pub(crate) enum QuoteType {
    /// Double quote `"`.
    Double,
    /// Single quote `'`.
    Single,
}

impl QuoteType {
    /// Returns the [`QuoteType`] if `ch` is a single or double quote character.
    #[inline]
    #[must_use]
    pub(crate) const fn from_char(ch: char) -> Option<Self> {
        match ch {
            '"' => Some(Self::Double),
            '\'' => Some(Self::Single),
            _ => None,
        }
    }

    /// Returns the character representing this quote kind.
    #[inline]
    #[must_use]
    pub(crate) const fn quote_char(self) -> char {
        match self {
            Self::Double => '"',
            Self::Single => '\'',
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    mod delimiter_matching {
        use super::*;

        #[test]
        fn finds_simple_closing_bracket() {
            assert_eq!(DelimiterType::Bracket.find_closing("value]"), Some(5));
        }

        #[test]
        fn finds_closing_parenthesis() {
            assert_eq!(
                DelimiterType::Parenthesis.find_closing("hello)"),
                Some(5)
            );
        }

        #[test]
        fn finds_closing_brace() {
            assert_eq!(DelimiterType::Brace.find_closing("key: val}"), Some(8));
        }

        #[test]
        fn finds_closing_double_bracket() {
            assert_eq!(
                DelimiterType::DoubleBracket.find_closing("Target|Alias]]"),
                Some(12)
            );
        }

        #[test]
        fn handles_nested_same_kind_brackets() {
            assert_eq!(
                DelimiterType::Bracket.find_closing("outer [inner]]"),
                Some(13)
            );
        }

        #[test]
        fn handles_nested_mixed_brackets() {
            assert_eq!(
                DelimiterType::Bracket
                    .find_closing("outer (paren) and [bracket]]"),
                Some(27)
            );
        }

        #[test]
        fn ignores_brackets_inside_double_quotes() {
            assert_eq!(
                DelimiterType::Bracket
                    .find_closing(r#"outer "[bracket]" text]"#),
                Some(22)
            );
        }

        #[test]
        fn ignores_brackets_inside_single_quotes() {
            assert_eq!(
                DelimiterType::Bracket.find_closing("outer '[bracket]' text]"),
                Some(22)
            );
        }

        #[test]
        fn skips_escaped_delimiters() {
            assert_eq!(
                DelimiterType::Bracket.find_closing(r"escaped \] bracket]"),
                Some(18)
            );
        }

        #[test]
        fn rejects_mismatched_intersections() {
            assert_eq!(
                DelimiterType::Parenthesis.find_closing("cross ( [ ) ]"),
                None
            );
        }

        #[test]
        fn rejects_unclosed_delimiter() {
            assert_eq!(
                DelimiterType::Bracket.find_closing("never closed"),
                None
            );
        }

        #[test]
        fn handles_lone_bracket_inside_double_bracket() {
            assert_eq!(
                DelimiterType::DoubleBracket.find_closing("lone ] bracket]]"),
                Some(14)
            );
        }
    }
}
