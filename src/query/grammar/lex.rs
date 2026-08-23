use std::{iter::Peekable, vec};

use miette::SourceSpan;

use crate::query::{QueryDialect, QueryError, error::QuerySyntaxError};

/// A token paired with its original byte span in the source expression.
///
/// Used throughout the parser to produce span-aware error diagnostics via
/// [`crate::query::error::QuerySyntaxError`].
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Spanned<T> {
    /// The parsed token value.
    pub(super) value: T,
    /// Its byte range in the source expression.
    pub(super) span: SourceSpan,
}

/// Owning one-token-lookahead cursor over a materialized token stream.
///
/// Wraps a [`Vec`] into a [`Peekable`] iterator, providing [`Self::peek`],
/// [`Self::next`], and [`Self::is_taken`] for the recursive-descent parser.
pub(super) struct TokenStream<T> {
    tokens: Peekable<vec::IntoIter<T>>,
}

impl<T> Spanned<T> {
    /// Pairs a token with its original source span.
    pub(super) const fn new(value: T, span: SourceSpan) -> Self {
        Self {
            value,
            span,
        }
    }
}

impl<T> TokenStream<T> {
    /// Creates a new token stream from a vector of tokens.
    pub(super) fn new(tokens: Vec<T>) -> Self {
        Self {
            tokens: tokens.into_iter().peekable(),
        }
    }

    /// Returns the next token without consuming it.
    pub(super) fn peek(&mut self) -> Option<&T> {
        self.tokens.peek()
    }

    /// Consumes and returns the next token.
    pub(super) fn next(&mut self) -> Option<T> {
        self.tokens.next()
    }

    /// Consumes `expected` only when it is the next token value.
    pub(super) fn is_taken<U>(&mut self, expected: &U) -> bool
    where
        T: AsRef<U>,
        U: PartialEq + ?Sized,
    {
        if self.peek().is_some_and(|token| token.as_ref() == expected) {
            self.next();
            true
        } else {
            false
        }
    }
}

impl<T> TokenStream<Spanned<T>> {
    /// Resolves the span of the next token in the stream, or the end of input
    /// if empty.
    pub(super) fn next_span(&mut self, input: &str) -> SourceSpan {
        self.peek().map_or_else(
            || SourceSpan::from((input.len(), 0)),
            |token| token.span,
        )
    }

    /// Consumes the next token only if it matches the expected value, returning
    /// its span.
    ///
    /// # Errors
    ///
    /// Returns a [`QueryError::Syntax`] diagnostic if the next token does not
    /// match `expected` or the stream is empty.
    pub(super) fn expect<U>(
        &mut self,
        input: &str,
        expected: &U,
        expected_desc: &'static str,
        dialect: QueryDialect,
    ) -> Result<SourceSpan, QueryError>
    where
        T: PartialEq<U>,
    {
        let next_span = self.next_span(input);
        match self.next() {
            Some(token) if token.value == *expected => Ok(token.span),
            _ => Err(QuerySyntaxError::new(
                dialect,
                input,
                next_span,
                expected_desc,
            )
            .into()),
        }
    }

    /// Consumes the next token and applies a mapper function, returning the
    /// mapped spanned result.
    ///
    /// # Errors
    ///
    /// Returns a [`QueryError::Syntax`] diagnostic if the stream is empty or
    /// the mapper returns `None`.
    pub(super) fn expect_map<F, R>(
        &mut self,
        input: &str,
        expected_desc: &'static str,
        dialect: QueryDialect,
        f: F,
    ) -> Result<Spanned<R>, QueryError>
    where
        F: FnOnce(T) -> Option<R>,
    {
        let next_span = self.next_span(input);
        match self.next() {
            Some(token) => {
                let span = token.span;
                if let Some(mapped) = f(token.value) {
                    Ok(Spanned::new(mapped, span))
                } else {
                    Err(QuerySyntaxError::new(
                        dialect,
                        input,
                        span,
                        expected_desc,
                    )
                    .into())
                }
            }
            None => Err(QuerySyntaxError::new(
                dialect,
                input,
                next_span,
                expected_desc,
            )
            .into()),
        }
    }
}

impl<T> AsRef<T> for Spanned<T> {
    fn as_ref(&self) -> &T {
        &self.value
    }
}
