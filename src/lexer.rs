//! Shared lexer primitives for tokenizing text into typed token streams.

use std::{iter::Peekable, vec};

use logos::Logos;
use miette::SourceSpan;
use thiserror::Error;

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Spanned<T> {
    value: T,
    span: SourceSpan,
}

impl<T> Spanned<T> {
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

    /// Consumes the `Spanned` wrapper, returning the inner value.
    #[inline]
    #[must_use]
    pub(crate) fn into_value(self) -> T {
        self.value
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Error)]
pub(crate) enum LexError {
    #[error("found `{found}`, expected {expected}")]
    UnexpectedToken {
        span: SourceSpan,
        found: String,
        expected: &'static str,
    },
    #[error("unexpected end of input, expected {expected}")]
    UnexpectedEof {
        span: SourceSpan,
        expected: &'static str,
    },
    #[error("invalid character `{char}`; {expected}")]
    InvalidCharacter {
        span: SourceSpan,
        char: char,
        expected: &'static str,
    },
}

/// Tokenizes `input` using a logos lexer, returning spanned tokens.
///
/// Each token is paired with its byte span for error diagnostics.
/// Returns `Err(LexError)` when the logos lexer encounters an
/// unrecognized token.
pub(crate) fn tokenize<'a, T>(
    input: &'a str,
) -> Result<Vec<Spanned<T>>, LexError>
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
        tokens.push(Spanned::new(value, span));
    }
    Ok(tokens)
}

/// Strips backslash escapes from `input`, returning the unescaped string.
///
/// A backslash followed by any character consumes both and emits the
/// second character verbatim. A trailing backslash (with nothing after
/// it) is kept as-is.
///
/// # Examples
///
/// ```
/// assert_eq!(unescape_backslash(r#"hello \"world\""#), "hello \"world\"");
/// assert_eq!(unescape_backslash(r#"back\\slash"#), "back\\slash");
/// assert_eq!(unescape_backslash("trailing\\"), "trailing\\");
/// ```
pub(crate) fn unescape_backslash(input: &str) -> String {
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

/// An owning one-token-lookahead cursor over a materialized token stream.
pub(crate) struct TokenStream<T> {
    tokens: Peekable<vec::IntoIter<T>>,
}

impl<T> TokenStream<T> {
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

    /// Returns `true` if the stream is exhausted.
    #[inline]
    pub(crate) fn is_empty(&mut self) -> bool {
        self.tokens.peek().is_none()
    }

    /// Consumes `expected` only when it is the next token value.
    pub(crate) fn is_taken<U>(&mut self, expected: &U) -> bool
    where
        T: PartialEq<U>,
        U: ?Sized,
    {
        if self.peek().is_some_and(|token| *token == *expected) {
            self.next();
            true
        } else {
            false
        }
    }

    /// Returns `true` if the next token equals `expected` without consuming it.
    pub(crate) fn peek_is(&mut self, expected: &T) -> bool
    where
        T: PartialEq,
    {
        self.peek().is_some_and(|token| token == expected)
    }
}

impl<T> TokenStream<Spanned<T>> {
    /// Resolves the span of the next token, or end-of-input if empty.
    pub(crate) fn next_span(&mut self, input: &str) -> SourceSpan {
        self.peek().map_or_else(
            || SourceSpan::from((input.len(), 0)),
            |token| token.span(),
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
    /// [`LexError::UnexpectedEof`] with diagnostic context.
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
            None => Err(LexError::UnexpectedEof {
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
    ) -> Result<Spanned<R>, LexError>
    where
        F: FnOnce(Spanned<T>) -> Option<R>,
    {
        match self.next() {
            Some(token) => {
                let span = token.span();
                f(token).map(|value| Spanned::new(value, span)).ok_or_else(
                    || LexError::UnexpectedToken {
                        span,
                        found: expected_desc.to_owned(),
                        expected: expected_desc,
                    },
                )
            }
            None => Err(LexError::UnexpectedEof {
                span: SourceSpan::from((input.len(), 0)),
                expected: expected_desc,
            }),
        }
    }
}

impl<T> AsRef<T> for Spanned<T> {
    fn as_ref(&self) -> &T {
        &self.value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unexpected_token_message_includes_found_and_expected() {
        let error = LexError::UnexpectedToken {
            span: SourceSpan::from((0, 3)),
            found: "foo".to_owned(),
            expected: "a filter term",
        };
        assert_eq!(error.to_string(), "found `foo`, expected a filter term");
    }

    #[test]
    fn unexpected_eof_message_includes_expected() {
        let error = LexError::UnexpectedEof {
            span: SourceSpan::from((5, 0)),
            expected: "a closing parenthesis",
        };
        assert_eq!(
            error.to_string(),
            "unexpected end of input, expected a closing parenthesis"
        );
    }

    #[test]
    fn invalid_character_message_includes_char_and_expected() {
        let error = LexError::InvalidCharacter {
            span: SourceSpan::from((2, 1)),
            char: '@',
            expected: "a letter, digit, underscore, slash, or hyphen",
        };
        assert_eq!(
            error.to_string(),
            "invalid character `@`; a letter, digit, underscore, slash, or \
             hyphen"
        );
    }

    #[test]
    fn tokenize_collects_spanned_tokens() {
        use logos::Logos;

        #[derive(Logos, Debug, Clone, PartialEq)]
        #[logos(skip r"[ \t\n]+")]
        enum TestToken {
            #[token("a")]
            A,
            #[token("b")]
            B,
        }

        let tokens = tokenize::<TestToken>("a b").unwrap();
        assert_eq!(tokens.len(), 2);
        assert_eq!(tokens[0].value(), &TestToken::A);
        assert_eq!(tokens[0].span(), SourceSpan::from((0, 1)));
        assert_eq!(tokens[1].value(), &TestToken::B);
        assert_eq!(tokens[1].span(), SourceSpan::from((2, 1)));
    }

    #[test]
    fn tokenize_returns_error_for_unrecognized_input() {
        use logos::Logos;

        #[derive(Logos, Debug, Clone, PartialEq)]
        #[logos(skip r"[ \t\n]+")]
        enum TestToken {
            #[token("a")]
            A,
        }

        let result = tokenize::<TestToken>("a x");
        assert!(result.is_err());
    }

    #[test]
    fn unescape_backslash_removes_escape_before_quote() {
        assert_eq!(unescape_backslash(r#"say \"hello\""#), "say \"hello\"");
    }

    #[test]
    fn unescape_backslash_keeps_trailing_backslash() {
        assert_eq!(unescape_backslash("abc\\"), "abc\\");
    }

    #[test]
    fn unescape_backslash_passes_through_no_escapes() {
        assert_eq!(unescape_backslash("no escapes here"), "no escapes here");
    }

    #[test]
    fn unescape_backslash_handles_empty_string() {
        assert_eq!(unescape_backslash(""), "");
    }

    #[test]
    fn token_stream_peek_does_not_consume() {
        let mut ts = TokenStream::new(vec![1_i32, 2, 3]);
        assert_eq!(ts.peek(), Some(&1));
        assert_eq!(ts.peek(), Some(&1));
        assert_eq!(ts.next(), Some(1));
    }

    #[test]
    fn token_stream_is_taken_consumes_on_match() {
        let mut ts = TokenStream::new(vec![1, 2, 3]);
        assert!(ts.is_taken(&1));
        assert_eq!(ts.next(), Some(2));
    }

    #[test]
    fn token_stream_is_taken_rejects_non_match() {
        let mut ts = TokenStream::new(vec![1, 2, 3]);
        assert!(!ts.is_taken(&99));
        assert_eq!(ts.next(), Some(1));
    }

    #[test]
    fn token_stream_peek_is_does_not_consume() {
        let mut ts = TokenStream::new(vec![42_i32, 43]);
        assert!(ts.peek_is(&42));
        assert!(!ts.peek_is(&43));
        assert_eq!(ts.next(), Some(42));
    }

    #[test]
    fn token_stream_peek_is_value_compares_inner() {
        let mut ts = TokenStream::new(vec![
            Spanned::new("hello", SourceSpan::from((0, 5))),
            Spanned::new("world", SourceSpan::from((6, 5))),
        ]);
        assert!(ts.peek_is_value(&"hello"));
        assert!(!ts.peek_is_value(&"world"));
    }

    #[test]
    fn token_stream_next_span_returns_end_when_empty() {
        let mut ts: TokenStream<Spanned<i32>> = TokenStream::new(vec![]);
        let span = ts.next_span("hello");
        assert_eq!(span, SourceSpan::from((5, 0)));
    }

    #[test]
    fn token_stream_next_span_returns_current_token_span() {
        let mut ts =
            TokenStream::new(vec![Spanned::new(1, SourceSpan::from((0, 3)))]);
        let span = ts.next_span("input");
        assert_eq!(span, SourceSpan::from((0, 3)));
    }

    #[test]
    fn token_stream_expect_consumes_matching_token() {
        use logos::Logos;

        #[derive(Logos, Debug, Clone, PartialEq)]
        #[logos(skip r"[ \t\n]+")]
        enum T {
            #[token("a")]
            A,
            #[token("b")]
            B,
        }

        let tokens = tokenize::<T>("a b").unwrap();
        let mut ts = TokenStream::new(tokens);
        let span = ts.expect("a b", &T::A, "an `a` token").unwrap();
        assert_eq!(span, SourceSpan::from((0, 1)));
    }

    #[test]
    fn token_stream_expect_returns_unexpected_token_on_mismatch() {
        use logos::Logos;

        #[derive(Logos, Debug, Clone, PartialEq)]
        #[logos(skip r"[ \t\n]+")]
        enum T {
            #[token("a")]
            A,
            #[token("b")]
            B,
        }

        let tokens = tokenize::<T>("a b").unwrap();
        let mut ts = TokenStream::new(tokens);
        let err = ts.expect("a b", &T::B, "a `b` token").unwrap_err();
        assert!(matches!(err, LexError::UnexpectedToken { .. }));
    }

    #[test]
    fn token_stream_expect_returns_unexpected_eof_on_empty() {
        use logos::Logos;

        #[derive(Logos, Debug, Clone, PartialEq)]
        #[logos(skip r"[ \t\n]+")]
        enum T {
            #[token("a")]
            A,
        }

        let tokens = tokenize::<T>("").unwrap();
        let mut ts = TokenStream::new(tokens);
        let err = ts.expect("", &T::A, "an `a` token").unwrap_err();
        assert!(matches!(err, LexError::UnexpectedEof { .. }));
    }
}
