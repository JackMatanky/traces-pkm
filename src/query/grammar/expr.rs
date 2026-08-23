use miette::SourceSpan;

use super::lex::{Spanned, TokenStream};
use crate::query::{QueryError, error::QuerySyntaxError};

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
/// via [`Self::is_matching`], [`Self::has_any_atom`], and
/// [`Self::visit_atoms_mut`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum BooleanExpr<A> {
    /// A domain-local atom.
    Atom(A),
    /// Every child must match.
    And(Vec<Self>),
    /// Any child may match.
    Or(Vec<Self>),
    /// The child must not match.
    Not(Box<Self>),
}

/// Domain-specific atom parsing hooks for the shared logical grammar.
///
/// Implement this trait to plug a domain-specific token type and atom parser
/// into [`parse_boolean_expr`]. The shared parser handles operator
/// precedence, grouping, and error recovery, delegating atom recognition to the
/// implementer.
pub(super) trait AtomParser {
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
        tokens: &mut TokenStream<Spanned<Self::Token>>,
    ) -> Result<Self::Atom, QueryError>;

    /// Builds a span-aware syntax diagnostic for this domain.
    fn syntax_error(
        &self,
        input: &str,
        span: SourceSpan,
        expected: &'static str,
    ) -> QuerySyntaxError;
}

struct BooleanExprParser<'input, G: AtomParser> {
    input: &'input str,
    tokens: TokenStream<Spanned<G::Token>>,
    grammar: G,
}

type ParseTerm<'input, G> =
    fn(
        &mut BooleanExprParser<'input, G>,
    ) -> Result<BooleanExpr<<G as AtomParser>::Atom>, QueryError>;

impl<A> BooleanExpr<A> {
    /// Evaluates this tree with the supplied atom predicate.
    pub(super) fn is_satisfied_by(
        &self,
        atom_matches: impl Fn(&A) -> bool,
    ) -> bool {
        self.is_satisfied_by_with(&atom_matches)
    }

    fn is_satisfied_by_with(&self, atom_matches: &impl Fn(&A) -> bool) -> bool {
        match self {
            Self::Atom(atom) => atom_matches(atom),
            Self::And(expressions) => expressions.iter().all(|expression| {
                expression.is_satisfied_by_with(atom_matches)
            }),
            Self::Or(expressions) => expressions.iter().any(|expression| {
                expression.is_satisfied_by_with(atom_matches)
            }),
            Self::Not(expression) => {
                !expression.is_satisfied_by_with(atom_matches)
            }
        }
    }

    /// Returns whether any atom satisfies `predicate`.
    pub(super) fn has_any_atom(&self, predicate: impl Fn(&A) -> bool) -> bool {
        self.has_any_atom_with(&predicate)
    }

    fn has_any_atom_with(&self, predicate: &impl Fn(&A) -> bool) -> bool {
        match self {
            Self::Atom(atom) => predicate(atom),
            Self::And(expressions) | Self::Or(expressions) => expressions
                .iter()
                .any(|expression| expression.has_any_atom_with(predicate)),
            Self::Not(expression) => expression.has_any_atom_with(predicate),
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

impl<'input, G: AtomParser> BooleanExprParser<'input, G> {
    fn parse(&mut self) -> Result<BooleanExpr<G::Atom>, QueryError> {
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

    fn parse_or(&mut self) -> Result<BooleanExpr<G::Atom>, QueryError> {
        self.parse_logical_chain(LogicalOp::Or, Self::parse_and)
    }

    fn parse_and(&mut self) -> Result<BooleanExpr<G::Atom>, QueryError> {
        self.parse_logical_chain(LogicalOp::And, Self::parse_not)
    }

    fn parse_logical_chain(
        &mut self,
        operator: LogicalOp,
        parse_term: ParseTerm<'input, G>,
    ) -> Result<BooleanExpr<G::Atom>, QueryError> {
        let first = parse_term(self)?;
        if !self.is_control_taken(LogicalControl::Operator(operator)) {
            return Ok(first);
        }

        let mut expressions = Vec::with_capacity(2);
        expressions.push(first);
        loop {
            expressions.push(parse_term(self)?);
            if !self.is_control_taken(LogicalControl::Operator(operator)) {
                break;
            }
        }
        Ok(match operator {
            LogicalOp::And => BooleanExpr::And(expressions),
            LogicalOp::Or => BooleanExpr::Or(expressions),
        })
    }

    fn parse_not(&mut self) -> Result<BooleanExpr<G::Atom>, QueryError> {
        let mut count = 0usize;
        while self.is_control_taken(LogicalControl::Not) {
            count = count.saturating_add(1);
        }
        let mut expression = self.parse_primary()?;
        for _ in 0..count {
            expression = BooleanExpr::Not(Box::new(expression));
        }
        Ok(expression)
    }

    fn parse_primary(&mut self) -> Result<BooleanExpr<G::Atom>, QueryError> {
        if self.is_control_taken(LogicalControl::LeftParen) {
            let expression = self.parse_or()?;
            if !self.is_control_taken(LogicalControl::RightParen) {
                let span = self.next_span();
                return Err(self.syntax_error(span, "`)` to close `(`").into());
            }
            Ok(expression)
        } else {
            self.grammar
                .parse_atom(self.input, &mut self.tokens)
                .map(BooleanExpr::Atom)
        }
    }

    fn is_control_taken(&mut self, expected: LogicalControl) -> bool {
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

/// Parses a complete expression with `not` > `and` > `or` precedence.
///
/// Accepts a pre-tokenized stream and a domain-specific [`AtomParser`] that
/// handles atom recognition. Returns a [`BooleanExpr`] tree, or a
/// [`QueryError::Syntax`] diagnostic when the expression is malformed.
pub(super) fn parse_boolean_expr<G>(
    input: &str,
    tokens: TokenStream<Spanned<G::Token>>,
    grammar: G,
) -> Result<BooleanExpr<G::Atom>, QueryError>
where
    G: AtomParser,
{
    BooleanExprParser {
        input,
        tokens,
        grammar,
    }
    .parse()
}

#[cfg(test)]
mod tests {

    use super::*;
    use crate::query::QueryDialect;

    #[derive(Clone, Debug, Eq, PartialEq)]
    enum TestToken {
        Atom(&'static str),
        Control(LogicalControl),
    }

    struct TestGrammar;

    impl AtomParser for TestGrammar {
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
            tokens: &mut TokenStream<Spanned<Self::Token>>,
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

            let parsed = parse_boolean_expr(
                "a or b and not not c and d",
                TokenStream::new(vec![
                    token(Atom("a"), 0),
                    token(Control(Operator(Or)), 2),
                    token(Atom("b"), 5),
                    token(Control(Operator(And)), 7),
                    token(Control(Not), 11),
                    token(Control(Not), 15),
                    token(Atom("c"), 19),
                    token(Control(Operator(And)), 21),
                    token(Atom("d"), 25),
                ]),
                TestGrammar,
            );

            assert_eq!(
                parsed,
                Ok(BooleanExpr::Or(vec![
                    BooleanExpr::Atom("a"),
                    BooleanExpr::And(vec![
                        BooleanExpr::Atom("b"),
                        BooleanExpr::Not(Box::new(BooleanExpr::Not(Box::new(
                            BooleanExpr::Atom("c")
                        )))),
                        BooleanExpr::Atom("d"),
                    ]),
                ]))
            );
        }

        #[test]
        fn parses_grouped_expression_overriding_precedence() {
            use LogicalControl::{LeftParen, Operator, RightParen};
            use LogicalOp::{And, Or};
            use TestToken::{Atom, Control};

            assert_eq!(
                parse_boolean_expr(
                    "(a or b) and c",
                    TokenStream::new(vec![
                        token(Control(LeftParen), 0),
                        token(Atom("a"), 1),
                        token(Control(Operator(Or)), 3),
                        token(Atom("b"), 6),
                        token(Control(RightParen), 7),
                        token(Control(Operator(And)), 9),
                        token(Atom("c"), 13),
                    ]),
                    TestGrammar,
                ),
                Ok(BooleanExpr::And(vec![
                    BooleanExpr::Or(vec![
                        BooleanExpr::Atom("a"),
                        BooleanExpr::Atom("b"),
                    ]),
                    BooleanExpr::Atom("c"),
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
                parse_boolean_expr(
                    "fixture",
                    TokenStream::new(tokens),
                    TestGrammar
                )
                .is_err()
            );
        }
    }

    mod matching {

        use super::*;

        #[test]
        fn delegates_tree_operations_without_allocating() {
            let mut expression = BooleanExpr::And(vec![
                BooleanExpr::Atom(1),
                BooleanExpr::Not(Box::new(BooleanExpr::Atom(2))),
            ]);

            assert!(expression.is_satisfied_by(|atom| *atom == 1));
            assert!(expression.has_any_atom(|atom| *atom == 2));
            expression.visit_atoms_mut(&mut |atom| *atom += 1);
            assert!(expression.is_satisfied_by(|atom| *atom == 2));
        }

        #[test]
        fn returns_false_when_any_atom_fails_in_and() {
            let expression = BooleanExpr::And(vec![
                BooleanExpr::Atom(1),
                BooleanExpr::Atom(2),
                BooleanExpr::Atom(3),
            ]);

            let result = expression.is_satisfied_by(|atom| *atom != 2);

            assert!(!result, "AND must return false when any atom fails");
        }

        #[test]
        fn returns_true_when_all_atoms_match_in_and() {
            let expression = BooleanExpr::And(vec![
                BooleanExpr::Atom(1),
                BooleanExpr::Atom(2),
            ]);

            let result = expression.is_satisfied_by(|atom| *atom > 0);

            assert!(result, "AND must return true when all atoms match");
        }

        #[test]
        fn returns_true_when_any_atom_matches_in_or() {
            let expression = BooleanExpr::Or(vec![
                BooleanExpr::Atom(1),
                BooleanExpr::Atom(2),
                BooleanExpr::Atom(3),
            ]);

            let result = expression.is_satisfied_by(|atom| *atom == 2);

            assert!(result, "OR must return true when any atom matches");
        }

        #[test]
        fn returns_false_when_no_atom_matches() {
            let expression = BooleanExpr::And(vec![
                BooleanExpr::Atom(1),
                BooleanExpr::Atom(2),
            ]);

            let result = expression.has_any_atom(|atom| *atom == 99);

            assert!(!result, "any_atom must return false when no atom matches");
        }

        #[test]
        fn returns_true_when_atom_matches_in_nested_expression() {
            let expression =
                BooleanExpr::Not(Box::new(BooleanExpr::And(vec![
                    BooleanExpr::Atom(10),
                    BooleanExpr::Atom(20),
                ])));

            let result = expression.has_any_atom(|atom| *atom == 10);

            assert!(result, "any_atom must find atoms inside NOT wrapper");
        }
    }
}
