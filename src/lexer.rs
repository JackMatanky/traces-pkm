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
    UnexpectedEof {
        span: SourceSpan,
        expected: &'static str,
    },
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "reserved for lexer-specific diagnostics")
    )]
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
) -> Result<Vec<LexedToken<T>>, LexError>
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
        tokens.push(LexedToken::new(value, span));
    }
    Ok(tokens)
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
        tokenize::<T>(input).map(Self::new)
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
    ) -> Result<LexedToken<R>, LexError>
    where
        F: FnOnce(LexedToken<T>) -> Option<R>,
    {
        match self.next() {
            Some(token) => {
                let span = token.span();
                f(token).map(|value| LexedToken::new(value, span)).ok_or_else(
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
        let first = tokens.first().unwrap();
        assert_eq!(first.value(), &TestToken::A);
        assert_eq!(first.span(), SourceSpan::from((0, 1)));
        let second = tokens.get(1).unwrap();
        assert_eq!(second.value(), &TestToken::B);
        assert_eq!(second.span(), SourceSpan::from((2, 1)));
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
    fn lexical_backslash_unescape_removes_escape_before_quote() {
        assert_eq!(
            lexical_backslash_unescape(r#"say \"hello\""#),
            "say \"hello\""
        );
    }

    #[test]
    fn lexical_backslash_unescape_keeps_trailing_backslash() {
        assert_eq!(lexical_backslash_unescape("abc\\"), "abc\\");
    }

    #[test]
    fn lexical_backslash_unescape_passes_through_no_escapes() {
        assert_eq!(
            lexical_backslash_unescape("no escapes here"),
            "no escapes here"
        );
    }

    #[test]
    fn lexical_backslash_unescape_handles_empty_string() {
        assert_eq!(lexical_backslash_unescape(""), "");
    }

    #[test]
    fn token_stream_peek_does_not_consume() {
        let mut ts = LexTokenStream::new(vec![1_i32, 2, 3]);
        assert_eq!(ts.peek(), Some(&1));
        assert_eq!(ts.peek(), Some(&1));
        assert_eq!(ts.next(), Some(1));
    }

    #[test]
    fn token_stream_peek_is_value_compares_inner() {
        let mut ts = LexTokenStream::new(vec![
            LexedToken::new("hello", SourceSpan::from((0, 5))),
            LexedToken::new("world", SourceSpan::from((6, 5))),
        ]);
        assert!(ts.peek_is_value(&"hello"));
        assert!(!ts.peek_is_value(&"world"));
    }

    #[test]
    fn token_stream_tokenize_builds_spanned_stream() {
        use logos::Logos;

        #[derive(Logos, Debug, Clone, PartialEq)]
        #[logos(skip r"[ \t\n]+")]
        enum T {
            #[token("a")]
            A,
            #[token("b")]
            B,
        }

        let mut ts = LexTokenStream::<LexedToken<T>>::tokenize("a b").unwrap();
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
    fn token_stream_next_span_returns_end_when_empty() {
        let mut ts: LexTokenStream<LexedToken<i32>> =
            LexTokenStream::new(vec![]);
        let span = ts.next_span("hello");
        assert_eq!(span, SourceSpan::from((5, 0)));
    }

    #[test]
    fn token_stream_next_span_returns_current_token_span() {
        let mut ts = LexTokenStream::new(vec![LexedToken::new(
            1,
            SourceSpan::from((0, 3)),
        )]);
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
        let mut ts = LexTokenStream::new(tokens);
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
        let mut ts = LexTokenStream::new(tokens);
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
        let mut ts = LexTokenStream::new(tokens);
        let err = ts.expect("", &T::A, "an `a` token").unwrap_err();
        assert!(matches!(err, LexError::UnexpectedEof { .. }));
    }
}
