//! Shared logical-expression tree and precedence parser.
//!
//! This module provides the generic [`LogicalExpr`] AST and
//! [`parse_logical_expression`] parser used by both the source selection
//! language ([`super::source`]) and the filter expression language
//! ([`super::filter`]). The parser enforces `not` > `and` > `or` precedence
//! with parenthetical grouping.
//!
//! # Main Types
//!
//! - [`LogicalExpr`] is a generic expression tree parameterized over a
//!   domain-specific atom type.
//! - [`LogicalOp`] represents the binary `AND`/`OR` operators.
//! - [`LogicalControl`] distinguishes logical control syntax from domain atoms.
//! - [`TokenCursor`] provides one-token lookahead over a materialized token
//!   stream.
//! - [`LogicalGrammar`] is the trait that domain-specific parsers implement to
//!   plug into the shared parser.

use std::{iter::Peekable, vec};

use miette::SourceSpan;

use super::{QueryError, error::QuerySyntaxError};

/// Binary logical operators shared by source and filter expressions.
///
/// Each variant corresponds to multiple syntactic spellings: `AND`/`and`/`&&`
/// for conjunction, `OR`/`or`/`||` for disjunction.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(super) enum LogicalOp {
    /// `AND` / `and` / `&&`.
    And,
    /// `OR` / `or` / `||`.
    Or,
}

impl TryFrom<&str> for LogicalOp {
    type Error = ();

    fn try_from(spelling: &str) -> Result<Self, Self::Error> {
        if spelling == "&&" || spelling.eq_ignore_ascii_case("and") {
            Ok(Self::And)
        } else if spelling == "||" || spelling.eq_ignore_ascii_case("or") {
            Ok(Self::Or)
        } else {
            Err(())
        }
    }
}

/// A token paired with its original byte span in the source expression.
///
/// Used throughout the parser to produce span-aware error diagnostics via
/// [`super::error::QuerySyntaxError`].
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Spanned<T> {
    /// The parsed token value.
    pub(super) value: T,
    /// Its byte range in the source expression.
    pub(super) span: SourceSpan,
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

impl<T> AsRef<T> for Spanned<T> {
    fn as_ref(&self) -> &T {
        &self.value
    }
}

/// Logical control syntax recognized independently of domain-specific atoms.
///
/// The shared parser uses these to build the expression tree without knowing
/// the specifics of the source or filter language.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(super) enum LogicalControl {
    /// A binary logical operator.
    Operator(LogicalOp),
    /// Unary logical negation.
    Not,
    /// An opening grouping parenthesis.
    LeftParen,
    /// A closing grouping parenthesis.
    RightParen,
}

/// A parsed logical expression tree over domain-local atom type `A`.
///
/// The tree preserves the original precedence and grouping of the parsed
/// expression. Domain-specific evaluation is delegated to atom predicates
/// via [`Self::evaluate`], [`Self::any_atom`], and [`Self::visit_atoms_mut`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum LogicalExpr<A> {
    /// A domain-local atom.
    Atom(A),
    /// Every child must match.
    And(Vec<Self>),
    /// Any child may match.
    Or(Vec<Self>),
    /// The child must not match.
    Not(Box<Self>),
}

impl<A> LogicalExpr<A> {
    /// Evaluates this tree with the supplied atom predicate.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "generic tree evaluation helper for future queries"
        )
    )]
    pub(super) fn evaluate(&self, atom_matches: impl Fn(&A) -> bool) -> bool {
        self.evaluate_with(&atom_matches)
    }

    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "generic tree evaluation helper for future queries"
        )
    )]
    fn evaluate_with(&self, atom_matches: &impl Fn(&A) -> bool) -> bool {
        match self {
            Self::Atom(atom) => atom_matches(atom),
            Self::And(expressions) => expressions
                .iter()
                .all(|expression| expression.evaluate_with(atom_matches)),
            Self::Or(expressions) => expressions
                .iter()
                .any(|expression| expression.evaluate_with(atom_matches)),
            Self::Not(expression) => !expression.evaluate_with(atom_matches),
        }
    }

    /// Returns whether any atom satisfies `predicate`.
    pub(super) fn any_atom(&self, predicate: impl Fn(&A) -> bool) -> bool {
        self.any_atom_with(&predicate)
    }

    fn any_atom_with(&self, predicate: &impl Fn(&A) -> bool) -> bool {
        match self {
            Self::Atom(atom) => predicate(atom),
            Self::And(expressions) | Self::Or(expressions) => expressions
                .iter()
                .any(|expression| expression.any_atom_with(predicate)),
            Self::Not(expression) => expression.any_atom_with(predicate),
        }
    }

    /// Visits each atom mutably without collecting intermediate references.
    pub(crate) fn visit_atoms_mut(&mut self, visitor: &mut impl FnMut(&mut A)) {
        match self {
            Self::Atom(atom) => visitor(atom),
            Self::And(expressions) | Self::Or(expressions) => {
                for expression in expressions {
                    expression.visit_atoms_mut(visitor);
                }
            }
            Self::Not(expression) => expression.visit_atoms_mut(visitor),
        }
    }
}

/// Owning one-token-lookahead cursor over a materialized token stream.
///
/// Wraps a `Vec<T>` into a [`Peekable`] iterator, providing [`peek`], [`next`],
/// and [`take`] for the recursive-descent parser.
///
/// [`peek`]: Self::peek
/// [`next`]: Self::next
/// [`take`]: Self::take
pub(super) struct TokenCursor<T> {
    tokens: Peekable<vec::IntoIter<T>>,
}

impl<T> TokenCursor<T> {
    fn new(tokens: Vec<T>) -> Self {
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
    pub(super) fn take<U>(&mut self, expected: &U) -> bool
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

/// Domain-specific atom parsing hooks for the shared logical grammar.
///
/// Implement this trait to plug a domain-specific token type and atom parser
/// into [`parse_logical_expression`]. The shared parser handles operator
/// precedence, grouping, and error recovery, delegating atom recognition to the
/// implementer.
pub(super) trait LogicalGrammar {
    /// The source/filter token type.
    type Token;
    /// The source/filter atom type.
    type Atom;

    /// Recognizes logical control syntax in a token.
    fn control(&self, token: &Self::Token) -> Option<LogicalControl>;
    /// Parses one domain-local atom from the token stream.
    fn parse_atom(
        &self,
        input: &str,
        tokens: &mut TokenCursor<Spanned<Self::Token>>,
    ) -> Result<Self::Atom, QueryError>;
    /// Builds a span-aware syntax diagnostic for this domain.
    fn syntax_error(
        &self,
        input: &str,
        span: SourceSpan,
        expected: &'static str,
    ) -> QuerySyntaxError;
}

/// Parses a complete expression with `not` > `and` > `or` precedence.
///
/// Accepts a pre-tokenized stream and a domain-specific [`LogicalGrammar`] that
/// handles atom recognition. Returns a [`LogicalExpr`] tree, or a
/// [`QueryError::Syntax`] diagnostic when the expression is malformed.
pub(super) fn parse_logical_expression<G>(
    input: &str,
    tokens: Vec<Spanned<G::Token>>,
    grammar: G,
) -> Result<LogicalExpr<G::Atom>, QueryError>
where
    G: LogicalGrammar,
{
    LogicalParser {
        input,
        tokens: TokenCursor::new(tokens),
        grammar,
    }
    .parse()
}

struct LogicalParser<'input, G: LogicalGrammar> {
    input: &'input str,
    tokens: TokenCursor<Spanned<G::Token>>,
    grammar: G,
}

type ParseTerm<'input, G> =
    fn(
        &mut LogicalParser<'input, G>,
    ) -> Result<LogicalExpr<<G as LogicalGrammar>::Atom>, QueryError>;

impl<'input, G: LogicalGrammar> LogicalParser<'input, G> {
    fn parse(&mut self) -> Result<LogicalExpr<G::Atom>, QueryError> {
        let expression = self.parse_or()?;
        let unexpected = self.tokens.peek().map(|token| token.span);
        if let Some(span) = unexpected {
            return Err(self
                .syntax_error(
                    span,
                    "a logical operator or the end of the expression",
                )
                .into());
        }
        Ok(expression)
    }

    fn parse_or(&mut self) -> Result<LogicalExpr<G::Atom>, QueryError> {
        self.parse_logical_chain(LogicalOp::Or, Self::parse_and)
    }

    fn parse_and(&mut self) -> Result<LogicalExpr<G::Atom>, QueryError> {
        self.parse_logical_chain(LogicalOp::And, Self::parse_not)
    }

    fn parse_logical_chain(
        &mut self,
        operator: LogicalOp,
        parse_term: ParseTerm<'input, G>,
    ) -> Result<LogicalExpr<G::Atom>, QueryError> {
        let first = parse_term(self)?;
        if !self.take_control(LogicalControl::Operator(operator)) {
            return Ok(first);
        }

        let mut expressions = Vec::with_capacity(2);
        expressions.push(first);
        loop {
            expressions.push(parse_term(self)?);
            if !self.take_control(LogicalControl::Operator(operator)) {
                break;
            }
        }
        Ok(match operator {
            LogicalOp::And => LogicalExpr::And(expressions),
            LogicalOp::Or => LogicalExpr::Or(expressions),
        })
    }

    fn parse_not(&mut self) -> Result<LogicalExpr<G::Atom>, QueryError> {
        let mut count = 0usize;
        while self.take_control(LogicalControl::Not) {
            count = count.saturating_add(1);
        }
        let mut expression = self.parse_primary()?;
        for _ in 0..count {
            expression = LogicalExpr::Not(Box::new(expression));
        }
        Ok(expression)
    }

    fn parse_primary(&mut self) -> Result<LogicalExpr<G::Atom>, QueryError> {
        if self.take_control(LogicalControl::LeftParen) {
            let expression = self.parse_or()?;
            if !self.take_control(LogicalControl::RightParen) {
                let span = self.next_span();
                return Err(self.syntax_error(span, "`)` to close `(`").into());
            }
            Ok(expression)
        } else {
            self.grammar
                .parse_atom(self.input, &mut self.tokens)
                .map(LogicalExpr::Atom)
        }
    }

    fn take_control(&mut self, expected: LogicalControl) -> bool {
        if self
            .tokens
            .peek()
            .and_then(|token| self.grammar.control(&token.value))
            == Some(expected)
        {
            self.tokens.next();
            true
        } else {
            false
        }
    }

    fn next_span(&mut self) -> SourceSpan {
        self.tokens.peek().map_or_else(
            || SourceSpan::from((self.input.len(), 0)),
            |token| token.span,
        )
    }

    fn syntax_error(
        &self,
        span: SourceSpan,
        expected: &'static str,
    ) -> QuerySyntaxError {
        self.grammar.syntax_error(self.input, span, expected)
    }
}

#[cfg(test)]
mod tests {

    use super::{super::QueryDialect, *};

    #[derive(Clone, Debug, Eq, PartialEq)]
    enum TestToken {
        Atom(&'static str),
        Control(LogicalControl),
    }

    struct TestGrammar;

    impl LogicalGrammar for TestGrammar {
        type Atom = &'static str;
        type Token = TestToken;

        fn control(&self, token: &Self::Token) -> Option<LogicalControl> {
            match token {
                TestToken::Control(control) => Some(*control),
                TestToken::Atom(_) => None,
            }
        }

        fn parse_atom(
            &self,
            input: &str,
            tokens: &mut TokenCursor<Spanned<Self::Token>>,
        ) -> Result<Self::Atom, QueryError> {
            match tokens.next() {
                Some(Spanned {
                    value: TestToken::Atom(atom),
                    ..
                }) => Ok(atom),
                Some(token) => {
                    Err(self.syntax_error(input, token.span, "an atom").into())
                }
                None => Err(self
                    .syntax_error(
                        input,
                        SourceSpan::from((input.len(), 0)),
                        "an atom",
                    )
                    .into()),
            }
        }

        fn syntax_error(
            &self,
            input: &str,
            span: SourceSpan,
            expected: &'static str,
        ) -> QuerySyntaxError {
            QuerySyntaxError::new(QueryDialect::Source, input, span, expected)
        }
    }

    fn token(value: TestToken, offset: usize) -> Spanned<TestToken> {
        Spanned::new(value, SourceSpan::from((offset, 1)))
    }

    mod parse {
        use pretty_assertions::assert_eq;
        use rstest::rstest;

        use super::*;

        #[test]
        fn parses_precedence_repeated_negation_and_flattened_chains() {
            use LogicalControl::{Not, Operator};
            use LogicalOp::{And, Or};
            use TestToken::{Atom, Control};

            let parsed = parse_logical_expression(
                "a or b and not not c and d",
                vec![
                    token(Atom("a"), 0),
                    token(Control(Operator(Or)), 2),
                    token(Atom("b"), 5),
                    token(Control(Operator(And)), 7),
                    token(Control(Not), 11),
                    token(Control(Not), 15),
                    token(Atom("c"), 19),
                    token(Control(Operator(And)), 21),
                    token(Atom("d"), 25),
                ],
                TestGrammar,
            );

            assert_eq!(
                parsed,
                Ok(LogicalExpr::Or(vec![
                    LogicalExpr::Atom("a"),
                    LogicalExpr::And(vec![
                        LogicalExpr::Atom("b"),
                        LogicalExpr::Not(Box::new(LogicalExpr::Not(Box::new(
                            LogicalExpr::Atom("c")
                        )))),
                        LogicalExpr::Atom("d"),
                    ]),
                ]))
            );
        }

        #[test]
        fn grouping_overrides_precedence() {
            use LogicalControl::{LeftParen, Operator, RightParen};
            use LogicalOp::{And, Or};
            use TestToken::{Atom, Control};

            assert_eq!(
                parse_logical_expression(
                    "(a or b) and c",
                    vec![
                        token(Control(LeftParen), 0),
                        token(Atom("a"), 1),
                        token(Control(Operator(Or)), 3),
                        token(Atom("b"), 6),
                        token(Control(RightParen), 7),
                        token(Control(Operator(And)), 9),
                        token(Atom("c"), 13),
                    ],
                    TestGrammar,
                ),
                Ok(LogicalExpr::And(vec![
                    LogicalExpr::Or(vec![
                        LogicalExpr::Atom("a"),
                        LogicalExpr::Atom("b"),
                    ]),
                    LogicalExpr::Atom("c"),
                ]))
            );
        }

        #[rstest]
        #[case::empty(vec![])]
        #[case::trailing_operator(vec![
            token(TestToken::Atom("a"), 0),
            token(TestToken::Control(LogicalControl::Operator(LogicalOp::And)), 2),
        ])]
        #[case::unmatched_left_parenthesis(vec![
            token(TestToken::Control(LogicalControl::LeftParen), 0),
            token(TestToken::Atom("a"), 1),
        ])]
        #[case::unmatched_right_parenthesis(vec![
            token(TestToken::Control(LogicalControl::RightParen), 0),
        ])]
        #[case::adjacent_atoms(vec![
            token(TestToken::Atom("a"), 0),
            token(TestToken::Atom("b"), 2),
        ])]
        fn rejects_incomplete_or_adjacent_tokens(
            #[case] tokens: Vec<Spanned<TestToken>>,
        ) {
            assert!(
                parse_logical_expression("fixture", tokens, TestGrammar)
                    .is_err()
            );
        }
    }

    mod evaluation {
        use super::*;

        #[test]
        fn tree_operations_delegate_without_allocating_leaf_collections() {
            let mut expression = LogicalExpr::And(vec![
                LogicalExpr::Atom(1),
                LogicalExpr::Not(Box::new(LogicalExpr::Atom(2))),
            ]);

            assert!(expression.evaluate(|atom| *atom == 1));
            assert!(expression.any_atom(|atom| *atom == 2));
            expression.visit_atoms_mut(&mut |atom| *atom += 1);
            assert!(expression.evaluate(|atom| *atom == 2));
        }

        #[test]
        fn evaluate_returns_false_when_any_atom_fails_in_and() {
            let expression = LogicalExpr::And(vec![
                LogicalExpr::Atom(1),
                LogicalExpr::Atom(2),
                LogicalExpr::Atom(3),
            ]);

            let result = expression.evaluate(|atom| *atom != 2);

            assert!(!result, "AND must return false when any atom fails");
        }

        #[test]
        fn evaluate_returns_true_when_all_atoms_match_in_and() {
            let expression = LogicalExpr::And(vec![
                LogicalExpr::Atom(1),
                LogicalExpr::Atom(2),
            ]);

            let result = expression.evaluate(|atom| *atom > 0);

            assert!(result, "AND must return true when all atoms match");
        }

        #[test]
        fn evaluate_returns_true_when_any_atom_matches_in_or() {
            let expression = LogicalExpr::Or(vec![
                LogicalExpr::Atom(1),
                LogicalExpr::Atom(2),
                LogicalExpr::Atom(3),
            ]);

            let result = expression.evaluate(|atom| *atom == 2);

            assert!(result, "OR must return true when any atom matches");
        }

        #[test]
        fn any_atom_returns_false_when_no_atom_matches() {
            let expression = LogicalExpr::And(vec![
                LogicalExpr::Atom(1),
                LogicalExpr::Atom(2),
            ]);

            let result = expression.any_atom(|atom| *atom == 99);

            assert!(!result, "any_atom must return false when no atom matches");
        }

        #[test]
        fn any_atom_returns_true_when_atom_matches_in_nested_expression() {
            let expression =
                LogicalExpr::Not(Box::new(LogicalExpr::And(vec![
                    LogicalExpr::Atom(10),
                    LogicalExpr::Atom(20),
                ])));

            let result = expression.any_atom(|atom| *atom == 10);

            assert!(result, "any_atom must find atoms inside NOT wrapper");
        }
    }
}
