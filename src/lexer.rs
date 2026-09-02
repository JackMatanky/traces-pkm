//! Shared lexer primitives for tokenizing text into typed token streams.

use std::{iter::Peekable, vec};

use logos::Logos;
use miette::SourceSpan;
use thiserror::Error;

#[derive(Clone, Debug, Eq, PartialEq, Error)]
pub(crate) enum LexError {
    #[error("found `{found}`, expected {expected}")]
    UnexpectedToken {
        span: SourceSpan,
        found: String,
        expected: &'static str,
    },
    #[error("unexpected end of input, expected {expected}")]
    UnexpectedEndOfInput {
        span: SourceSpan,
        expected: &'static str,
    },
}

impl LexError {
    /// Returns the byte span of this error.
    pub(crate) fn span(&self) -> SourceSpan {
        match self {
            Self::UnexpectedToken {
                span,
                ..
            }
            | Self::UnexpectedEndOfInput {
                span,
                ..
            } => *span,
        }
    }
}

/// An owning one-token-lookahead cursor over a materialized token stream.
pub(crate) struct LexTokenStream<T> {
    tokens: Peekable<vec::IntoIter<T>>,
}

impl<T> LexTokenStream<T> {
    /// Creates a new token stream from a vector of tokens.
    #[inline]
    pub(crate) fn new(tokens: Vec<T>) -> Self {
        Self {
            tokens: tokens.into_iter().peekable(),
        }
    }

    /// Returns the next token without consuming it.
    #[inline]
    pub(crate) fn peek(&mut self) -> Option<&T> {
        self.tokens.peek()
    }

    /// Consumes and returns the next token.
    #[inline]
    pub(crate) fn next(&mut self) -> Option<T> {
        self.tokens.next()
    }
}

impl<T> LexTokenStream<LexedToken<T>> {
    /// Tokenizes `input`, applying `post` to each token before adding it
    /// to the stream.
    ///
    /// # Errors
    ///
    /// Returns [`LexError`] when the logos lexer encounters an unrecognized
    /// token or when `post` returns an error.
    pub(crate) fn tokenize_with<'a>(
        input: &'a str,
        mut post: impl FnMut(LexedToken<T>) -> Result<LexedToken<T>, LexError>,
    ) -> Result<Self, LexError>
    where
        T: Logos<'a, Source = str>,
        T::Extras: Default,
    {
        let mut lexer = T::lexer(input);
        let mut tokens = Vec::new();
        while let Some(result) = lexer.next() {
            let range = lexer.span();
            let span = SourceSpan::from((range.start, range.len()));
            let value = result.map_err(|e| LexError::UnexpectedToken {
                span,
                found: format!("{e:?}"),
                expected: "a valid token",
            })?;
            tokens.push(post(LexedToken::new(value, span))?);
        }
        Ok(Self::new(tokens))
    }

    /// Tokenizes `input` into a spanned token stream.
    ///
    /// # Errors
    ///
    /// Returns [`LexError`] when the logos lexer encounters an unrecognized
    /// token.
    pub(crate) fn tokenize<'a>(input: &'a str) -> Result<Self, LexError>
    where
        T: Logos<'a, Source = str>,
        T::Extras: Default,
    {
        Self::tokenize_with(input, Ok)
    }

    /// Resolves the span of the next token, or end-of-input if empty.
    pub(crate) fn next_span(&mut self, input: &str) -> SourceSpan {
        self.peek().map_or_else(
            || SourceSpan::from((input.len(), 0)),
            LexedToken::span,
        )
    }

    /// Returns `true` if the next token's inner value equals `expected`.
    pub(crate) fn peek_is_value<U>(&mut self, expected: &U) -> bool
    where
        T: PartialEq<U>,
        U: ?Sized,
    {
        self.peek().is_some_and(|spanned| *spanned.value() == *expected)
    }

    /// Consumes and returns the next token if it equals `expected`.
    ///
    /// On success, returns the span of the consumed token.
    /// On failure, returns a [`LexError::UnexpectedToken`] or
    /// [`LexError::UnexpectedEndOfInput`] with diagnostic context.
    pub(crate) fn expect<U>(
        &mut self,
        input: &str,
        expected: &U,
        expected_desc: &'static str,
    ) -> Result<SourceSpan, LexError>
    where
        T: PartialEq<U> + std::fmt::Debug,
    {
        match self.next() {
            Some(token) if *token.value() == *expected => Ok(token.span()),
            Some(token) => Err(LexError::UnexpectedToken {
                span: token.span(),
                found: format!("{:?}", token.value()),
                expected: expected_desc,
            }),
            None => Err(LexError::UnexpectedEndOfInput {
                span: SourceSpan::from((input.len(), 0)),
                expected: expected_desc,
            }),
        }
    }

    /// Consumes the next token, applies `f`, and returns the mapped result.
    ///
    /// The next token must exist and `f` must return `Some`. Otherwise
    /// returns a [`LexError`] with diagnostic context.
    pub(crate) fn expect_map<F, R>(
        &mut self,
        input: &str,
        expected_desc: &'static str,
        f: F,
    ) -> Result<LexedToken<R>, LexError>
    where
        T: std::fmt::Debug,
        F: FnOnce(LexedToken<T>) -> Option<R>,
    {
        match self.next() {
            Some(token) => {
                let span = token.span();
                let found = format!("{:?}", token.value());
                f(token).map(|value| LexedToken::new(value, span)).ok_or(
                    LexError::UnexpectedToken {
                        span,
                        found,
                        expected: expected_desc,
                    },
                )
            }
            None => Err(LexError::UnexpectedEndOfInput {
                span: SourceSpan::from((input.len(), 0)),
                expected: expected_desc,
            }),
        }
    }

    /// Consumes an opening delimiter `open`, runs `parse_inner`, and consumes
    /// the matching closing delimiter `close`.
    ///
    /// # Errors
    ///
    /// Returns [`LexError::UnexpectedToken`] or
    /// [`LexError::UnexpectedEndOfInput`] if the opening or closing token
    /// does not match.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "general token-stream combinator; tested in unit suite"
        )
    )]
    #[expect(
        clippy::too_many_arguments,
        reason = "delimited combinator requires open/close tokens and \
                  descriptions"
    )]
    pub(crate) fn delimited<U, R, F>(
        &mut self,
        input: &str,
        open: &U,
        open_desc: &'static str,
        close: &U,
        close_desc: &'static str,
        parse_inner: F,
    ) -> Result<R, LexError>
    where
        T: PartialEq<U> + std::fmt::Debug,
        F: FnOnce(&mut Self) -> Result<R, LexError>,
    {
        self.expect(input, open, open_desc)?;
        let result = parse_inner(self)?;
        self.expect(input, close, close_desc)?;
        Ok(result)
    }
}

impl<T> AsRef<T> for LexedToken<T> {
    fn as_ref(&self) -> &T {
        &self.value
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct LexedToken<T> {
    value: T,
    span: SourceSpan,
}

impl<T> LexedToken<T> {
    #[inline]
    #[must_use]
    pub(crate) const fn new(value: T, span: SourceSpan) -> Self {
        Self {
            value,
            span,
        }
    }

    #[inline]
    #[must_use]
    pub(crate) fn value(&self) -> &T {
        &self.value
    }

    #[inline]
    #[must_use]
    pub(crate) fn span(&self) -> SourceSpan {
        self.span
    }

    /// Consumes the [`LexedToken`] wrapper, returning the inner value.
    #[inline]
    #[must_use]
    pub(crate) fn into_value(self) -> T {
        self.value
    }
}

/// Strips backslash escapes from `input`, returning the unescaped string.
///
/// A backslash followed by any character consumes both and emits the second
/// character verbatim. A trailing backslash (with nothing after it) is kept
/// as-is.
///
/// # Examples
///
/// ```ignore
/// assert_eq!(lexical_backslash_unescape(r#"hello \"world\""#), "hello \"world\"");
/// assert_eq!(lexical_backslash_unescape(r#"back\\slash"#), "back\\slash");
/// assert_eq!(lexical_backslash_unescape("trailing\\"), "trailing\\");
/// ```
pub(crate) fn lexical_backslash_unescape(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut chars = input.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            if let Some(escaped) = chars.next() {
                output.push(escaped);
            } else {
                output.push('\\');
            }
        } else {
            output.push(ch);
        }
    }
    output
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
}

/// Finds the byte offset of the matching closing delimiter in `input`,
/// managing nested `()`, `[]`, `{}`, `[[]]`, and string quotes (`"..."`,
/// `'...'`).
///
/// `kind` specifies the root opening delimiter that began the span (e.g.
/// [`DelimiterType::Bracket`] for an already-consumed `[`).
///
/// Returns `Some(byte_offset)` of the matching closing delimiter, or `None` if
/// delimiters are unclosed, mismatched, or truncated.
#[must_use]
pub(crate) fn find_closing_delimiter(
    input: &str,
    kind: DelimiterType,
) -> Option<usize> {
    let mut stack = DelimiterStack::with_root(kind);
    let mut escaped = false;
    let mut byte_offset = 0usize;

    while byte_offset < input.len() {
        let rest = input.get(byte_offset..)?;
        let ch = rest.chars().next()?;
        let ch_len = ch.len_utf8();

        if escaped {
            escaped = false;
            byte_offset = byte_offset.saturating_add(ch_len);
            continue;
        }

        if ch == '\\' {
            escaped = true;
            byte_offset = byte_offset.saturating_add(ch_len);
            continue;
        }

        if let Some(active_quote) = stack.active_quote {
            if ch == active_quote.quote_char() {
                stack.active_quote = None;
            }
            byte_offset = byte_offset.saturating_add(ch_len);
            continue;
        }

        if let Some(quote) = QuoteType::from_char(ch) {
            stack.active_quote = Some(quote);
            byte_offset = byte_offset.saturating_add(ch_len);
            continue;
        }

        // Check for DoubleBracket closer `]]`
        if stack.current_kind() == Some(DelimiterType::DoubleBracket)
            && rest.starts_with("]]")
        {
            stack.len = stack.len.saturating_sub(1);
            if stack.len == 0 {
                return Some(byte_offset);
            }
            byte_offset = byte_offset.saturating_add(2);
            continue;
        }

        // Check for single-character closers
        if matches!(ch, ')' | ']' | '}') {
            let current = stack.current_kind()?;
            let matches = match (current, ch) {
                (DelimiterType::Parenthesis, ')')
                | (DelimiterType::Bracket, ']')
                | (DelimiterType::Brace, '}') => true,
                _ => false,
            };
            if !matches {
                if current == DelimiterType::DoubleBracket && ch == ']' {
                    byte_offset = byte_offset.saturating_add(ch_len);
                    continue;
                }
                return None;
            }
            stack.len = stack.len.saturating_sub(1);
            if stack.len == 0 {
                return Some(byte_offset);
            }
            byte_offset = byte_offset.saturating_add(ch_len);
            continue;
        }

        // Check for DoubleBracket opener `[[`
        if rest.starts_with("[[") {
            stack.push(DelimiterType::DoubleBracket);
            byte_offset = byte_offset.saturating_add(2);
            continue;
        }

        // Check for single-character openers
        if let Some(open_kind) = match ch {
            '(' => Some(DelimiterType::Parenthesis),
            '[' => Some(DelimiterType::Bracket),
            '{' => Some(DelimiterType::Brace),
            _ => None,
        } {
            stack.push(open_kind);
            byte_offset = byte_offset.saturating_add(ch_len);
            continue;
        }

        byte_offset = byte_offset.saturating_add(ch_len);
    }

    None
}

/// Strips matching single (`'...'`) or double (`"..."`) quotes from `raw` and
/// unescapes backslash sequences via [`lexical_backslash_unescape`].
///
/// If `raw` is not enclosed in matching quotes, returns `raw` unescaped.
#[must_use]
pub(crate) fn lexical_unquote(raw: &str) -> String {
    if (raw.starts_with('"') && raw.ends_with('"') && raw.len() >= 2)
        || (raw.starts_with('\'') && raw.ends_with('\'') && raw.len() >= 2)
    {
        let inner = raw.get(1..raw.len().saturating_sub(1)).unwrap_or_default();
        lexical_backslash_unescape(inner)
    } else {
        lexical_backslash_unescape(raw)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    mod lex_error {
        use super::*;

        #[test]
        fn display_unexpected_token_includes_found_and_expected() {
            let error = LexError::UnexpectedToken {
                span: SourceSpan::from((0, 3)),
                found: "foo".to_owned(),
                expected: "a filter term",
            };
            assert_eq!(
                error.to_string(),
                "found `foo`, expected a filter term"
            );
        }

        #[test]
        fn display_unexpected_eof_includes_expected() {
            let error = LexError::UnexpectedEndOfInput {
                span: SourceSpan::from((5, 0)),
                expected: "a closing parenthesis",
            };
            assert_eq!(
                error.to_string(),
                "unexpected end of input, expected a closing parenthesis"
            );
        }

        #[test]
        fn span_returns_offset_for_unexpected_token() {
            let error = LexError::UnexpectedToken {
                span: SourceSpan::from((3, 2)),
                found: "foo".to_owned(),
                expected: "bar",
            };
            assert_eq!(error.span(), SourceSpan::from((3, 2)));
        }

        #[test]
        fn span_returns_offset_for_unexpected_eof() {
            let error = LexError::UnexpectedEndOfInput {
                span: SourceSpan::from((10, 0)),
                expected: "baz",
            };
            assert_eq!(error.span(), SourceSpan::from((10, 0)));
        }
    }

    mod lexed_token {
        use super::*;

        #[test]
        fn new_wraps_value_and_span() {
            let token = LexedToken::new(42_i32, SourceSpan::from((0, 2)));
            assert_eq!(token.value(), &42);
            assert_eq!(token.span(), SourceSpan::from((0, 2)));
        }

        #[test]
        fn into_value_consumes_wrapper() {
            let token = LexedToken::new("hello", SourceSpan::from((0, 5)));
            let value = token.into_value();
            assert_eq!(value, "hello");
        }

        #[test]
        fn as_ref_borrows_inner() {
            let token = LexedToken::new(99_i32, SourceSpan::from((0, 2)));
            let r: &i32 = token.as_ref();
            assert_eq!(r, &99);
        }
    }

    mod token_stream {
        use super::*;

        #[test]
        fn peek_does_not_consume() {
            let mut ts = LexTokenStream::new(vec![1_i32, 2, 3]);
            assert_eq!(ts.peek(), Some(&1));
            assert_eq!(ts.peek(), Some(&1));
            assert_eq!(ts.next(), Some(1));
        }

        #[test]
        fn peek_is_value_returns_true_on_match() {
            let mut ts = LexTokenStream::new(vec![
                LexedToken::new("hello", SourceSpan::from((0, 5))),
                LexedToken::new("world", SourceSpan::from((6, 5))),
            ]);
            assert!(ts.peek_is_value(&"hello"));
        }

        #[test]
        fn peek_is_value_returns_false_on_mismatch() {
            let mut ts = LexTokenStream::new(vec![LexedToken::new(
                "hello",
                SourceSpan::from((0, 5)),
            )]);
            assert!(!ts.peek_is_value(&"world"));
        }

        #[test]
        fn next_span_returns_end_when_empty() {
            let mut ts: LexTokenStream<LexedToken<i32>> =
                LexTokenStream::new(vec![]);
            let span = ts.next_span("hello");
            assert_eq!(span, SourceSpan::from((5, 0)));
        }

        #[test]
        fn next_span_returns_current_token_span() {
            let mut ts = LexTokenStream::new(vec![LexedToken::new(
                1,
                SourceSpan::from((0, 3)),
            )]);
            let span = ts.next_span("input");
            assert_eq!(span, SourceSpan::from((0, 3)));
        }
    }

    mod expect {
        use logos::Logos;

        use super::*;

        #[derive(Logos, Debug, Clone, PartialEq)]
        #[logos(skip r"[ \t\n]+")]
        enum T {
            #[token("a")]
            A,
            #[token("b")]
            B,
        }

        #[test]
        fn returns_span_when_token_matches() {
            let mut ts =
                LexTokenStream::<LexedToken<T>>::tokenize("a b").unwrap();
            let span = ts.expect("a b", &T::A, "an `a` token").unwrap();
            assert_eq!(span, SourceSpan::from((0, 1)));
        }

        #[test]
        fn returns_unexpected_token_on_mismatch() {
            let mut ts =
                LexTokenStream::<LexedToken<T>>::tokenize("a b").unwrap();
            let err = ts.expect("a b", &T::B, "a `b` token").unwrap_err();
            assert!(matches!(err, LexError::UnexpectedToken { .. }));
        }

        #[test]
        fn returns_unexpected_eof_on_empty() {
            let mut ts = LexTokenStream::<LexedToken<T>>::tokenize("").unwrap();
            let err = ts.expect("", &T::A, "an `a` token").unwrap_err();
            assert!(matches!(err, LexError::UnexpectedEndOfInput { .. }));
        }
    }

    mod expect_map {
        use logos::Logos;

        use super::*;

        #[derive(Logos, Debug, Clone, PartialEq)]
        #[logos(skip r"[ \t\n]+")]
        enum T {
            #[token("a")]
            A,
            #[regex("[0-9]+", |lex| lex.slice().parse::<i32>().ok())]
            Num(i32),
        }

        #[test]
        fn returns_mapped_value_when_closure_succeeds() {
            let mut ts =
                LexTokenStream::<LexedToken<T>>::tokenize("a 42").unwrap();
            ts.next(); // skip A
            let result = ts
                .expect_map("a 42", "a number", |token| {
                    if let T::Num(n) = token.into_value() {
                        Some(n * 2)
                    } else {
                        None
                    }
                })
                .unwrap();
            assert_eq!(result.into_value(), 84);
        }

        #[test]
        fn returns_unexpected_token_when_closure_returns_none() {
            let mut ts =
                LexTokenStream::<LexedToken<T>>::tokenize("42 a").unwrap();
            let err = ts
                .expect_map("42 a", "a non-number", |token| {
                    if matches!(token.into_value(), T::Num(_)) {
                        None
                    } else {
                        Some(())
                    }
                })
                .unwrap_err();
            assert!(matches!(err, LexError::UnexpectedToken { .. }));
        }

        #[test]
        fn returns_unexpected_eof_on_empty() {
            let mut ts = LexTokenStream::<LexedToken<T>>::tokenize("").unwrap();
            let err = ts
                .expect_map("", "a token", |token| Some(token.into_value()))
                .unwrap_err();
            assert!(matches!(err, LexError::UnexpectedEndOfInput { .. }));
        }
    }

    mod tokenize {
        use logos::Logos;

        use super::*;

        #[derive(Logos, Debug, Clone, PartialEq)]
        #[logos(skip r"[ \t\n]+")]
        enum T {
            #[token("a")]
            A,
            #[token("b")]
            B,
        }

        #[test]
        fn returns_spanned_tokens() {
            let mut ts =
                LexTokenStream::<LexedToken<T>>::tokenize("a b").unwrap();
            assert!(ts.peek_is_value(&T::A));
            assert_eq!(
                ts.expect("a b", &T::A, "an `a` token").unwrap(),
                SourceSpan::from((0, 1))
            );
            assert_eq!(
                ts.expect("a b", &T::B, "a `b` token").unwrap(),
                SourceSpan::from((2, 1))
            );
            assert_eq!(ts.next(), None);
        }

        #[test]
        fn returns_error_for_unrecognized_input() {
            let result = LexTokenStream::<LexedToken<T>>::tokenize("a x");
            assert!(result.is_err());
        }
    }

    mod tokenize_with {
        use logos::Logos;

        use super::*;

        #[derive(Logos, Debug, Clone, PartialEq)]
        #[logos(skip r"[ \t\n]+")]
        enum T {
            #[token("a")]
            A,
            #[regex("[0-9]+", |lex| lex.slice().parse::<i32>().ok())]
            Num(i32),
        }

        fn clamp_post(token: LexedToken<T>) -> Result<LexedToken<T>, LexError> {
            let span = token.span();
            match token.into_value() {
                T::Num(n) if n > 100 => Err(LexError::UnexpectedToken {
                    span,
                    found: format!("{n}"),
                    expected: "a number <= 100",
                }),
                other => Ok(LexedToken::new(other, span)),
            }
        }

        #[test]
        fn applies_post_processing_to_each_token() {
            let mut ts = LexTokenStream::<LexedToken<T>>::tokenize_with(
                "a 42", clamp_post,
            )
            .unwrap();
            assert!(ts.peek_is_value(&T::A));
            ts.next();
            assert!(ts.peek_is_value(&T::Num(42)));
        }

        #[test]
        fn propagates_post_processing_errors() {
            let result = LexTokenStream::<LexedToken<T>>::tokenize_with(
                "200", clamp_post,
            );
            assert!(result.is_err());
        }
    }

    mod lexical_backslash_unescape {
        use super::*;

        #[test]
        fn strips_escape_before_quote() {
            assert_eq!(
                lexical_backslash_unescape(r#"say \"hello\""#),
                "say \"hello\""
            );
        }

        #[test]
        fn keeps_trailing_backslash() {
            assert_eq!(lexical_backslash_unescape("abc\\"), "abc\\");
        }

        #[test]
        fn passes_through_when_no_escapes() {
            assert_eq!(
                lexical_backslash_unescape("no escapes here"),
                "no escapes here"
            );
        }

        #[test]
        fn returns_empty_for_empty_input() {
            assert_eq!(lexical_backslash_unescape(""), "");
        }
    }

    mod delimiter_matching {
        use super::*;

        #[test]
        fn finds_simple_closing_bracket() {
            assert_eq!(
                find_closing_delimiter("value]", DelimiterType::Bracket),
                Some(5)
            );
        }

        #[test]
        fn finds_closing_parenthesis() {
            assert_eq!(
                find_closing_delimiter("hello)", DelimiterType::Parenthesis),
                Some(5)
            );
        }

        #[test]
        fn finds_closing_brace() {
            assert_eq!(
                find_closing_delimiter("key: val}", DelimiterType::Brace),
                Some(8)
            );
        }

        #[test]
        fn finds_closing_double_bracket() {
            assert_eq!(
                find_closing_delimiter(
                    "Target|Alias]]",
                    DelimiterType::DoubleBracket
                ),
                Some(12)
            );
        }

        #[test]
        fn handles_nested_same_kind_brackets() {
            assert_eq!(
                find_closing_delimiter(
                    "outer [inner]]",
                    DelimiterType::Bracket
                ),
                Some(13)
            );
        }

        #[test]
        fn handles_nested_mixed_brackets() {
            assert_eq!(
                find_closing_delimiter(
                    "outer (paren) and [bracket]]",
                    DelimiterType::Bracket
                ),
                Some(27)
            );
        }

        #[test]
        fn ignores_brackets_inside_double_quotes() {
            assert_eq!(
                find_closing_delimiter(
                    r#"outer "[bracket]" text]"#,
                    DelimiterType::Bracket
                ),
                Some(22)
            );
        }

        #[test]
        fn ignores_brackets_inside_single_quotes() {
            assert_eq!(
                find_closing_delimiter(
                    "outer '[bracket]' text]",
                    DelimiterType::Bracket
                ),
                Some(22)
            );
        }

        #[test]
        fn skips_escaped_delimiters() {
            assert_eq!(
                find_closing_delimiter(
                    r"escaped \] bracket]",
                    DelimiterType::Bracket
                ),
                Some(18)
            );
        }

        #[test]
        fn rejects_mismatched_intersections() {
            assert_eq!(
                find_closing_delimiter(
                    "cross ( [ ) ]",
                    DelimiterType::Parenthesis
                ),
                None
            );
        }

        #[test]
        fn rejects_unclosed_delimiter() {
            assert_eq!(
                find_closing_delimiter("never closed", DelimiterType::Bracket),
                None
            );
        }

        #[test]
        fn handles_lone_bracket_inside_double_bracket() {
            assert_eq!(
                find_closing_delimiter(
                    "lone ] bracket]]",
                    DelimiterType::DoubleBracket
                ),
                Some(14)
            );
        }
    }

    mod lexical_unquote {
        use super::*;

        #[test]
        fn unquotes_double_quoted_string() {
            assert_eq!(
                lexical_unquote(r#""hello \"world\"""#),
                r#"hello "world""#
            );
        }

        #[test]
        fn unquotes_single_quoted_string() {
            assert_eq!(
                lexical_unquote(r"'single \'quote\''"),
                "single 'quote'"
            );
        }

        #[test]
        fn unescapes_unquoted_string() {
            assert_eq!(lexical_unquote(r"plain\ text"), "plain text");
        }

        #[test]
        fn handles_empty_and_short_quotes() {
            assert_eq!(lexical_unquote(r#""""#), "");
            assert_eq!(lexical_unquote("''"), "");
            assert_eq!(lexical_unquote(r#"""#), "\"");
        }
    }

    mod delimited {
        use super::*;

        #[derive(Logos, Debug, PartialEq)]
        enum SimpleToken {
            #[token("(")]
            LParen,
            #[token(")")]
            RParen,
            #[token("x")]
            X,
        }

        #[test]
        fn parses_matching_delimited_content() {
            let mut stream =
                LexTokenStream::<LexedToken<SimpleToken>>::tokenize("(x)")
                    .unwrap();
            let result = stream.delimited(
                "(x)",
                &SimpleToken::LParen,
                "`(`",
                &SimpleToken::RParen,
                "`)`",
                |ts| {
                    let token = ts.next().unwrap();
                    assert_eq!(*token.value(), SimpleToken::X);
                    Ok("parsed")
                },
            );
            assert_eq!(result.unwrap(), "parsed");
        }

        #[test]
        fn rejects_missing_close_delimiter() {
            let mut stream =
                LexTokenStream::<LexedToken<SimpleToken>>::tokenize("(x")
                    .unwrap();
            let result = stream.delimited(
                "(x",
                &SimpleToken::LParen,
                "`(`",
                &SimpleToken::RParen,
                "`)`",
                |ts| {
                    let _ = ts.next();
                    Ok(())
                },
            );
            assert!(matches!(
                result,
                Err(LexError::UnexpectedEndOfInput { .. })
            ));
        }
    }
}
