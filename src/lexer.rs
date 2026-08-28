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
}
