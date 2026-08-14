use std::{iter::Peekable, vec};

use super::{QueryError, operators::LogicalOp};

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(super) enum BooleanControl {
    Logical(LogicalOp),
    Not,
    LeftParen,
    RightParen,
}

pub(super) struct TokenCursor<T> {
    tokens: Peekable<vec::IntoIter<T>>,
}

impl<T> TokenCursor<T> {
    fn new(tokens: Vec<T>) -> Self {
        Self {
            tokens: tokens.into_iter().peekable(),
        }
    }

    pub(super) fn peek(&mut self) -> Option<&T> {
        self.tokens.peek()
    }

    pub(super) fn next(&mut self) -> Option<T> {
        self.tokens.next()
    }

    pub(super) fn take(&mut self, expected: &T) -> bool
    where
        T: PartialEq,
    {
        if self.peek() == Some(expected) {
            self.next();
            true
        } else {
            false
        }
    }
}

pub(super) trait BooleanGrammar {
    type Token;
    type Expr;

    fn control(&self, token: &Self::Token) -> Option<BooleanControl>;
    fn parse_atom(
        &self,
        input: &str,
        tokens: &mut TokenCursor<Self::Token>,
    ) -> Result<Self::Expr, QueryError>;
    fn logical(
        &self,
        operator: LogicalOp,
        expressions: Vec<Self::Expr>,
    ) -> Self::Expr;
    fn not(&self, expression: Self::Expr) -> Self::Expr;
    fn invalid(&self, input: &str) -> QueryError;
}

pub(super) fn parse_boolean<G>(
    input: &str,
    tokens: Vec<G::Token>,
    grammar: G,
) -> Result<G::Expr, QueryError>
where
    G: BooleanGrammar,
{
    BooleanParser {
        input,
        tokens: TokenCursor::new(tokens),
        grammar,
    }
    .parse()
}

struct BooleanParser<'input, G: BooleanGrammar> {
    input: &'input str,
    tokens: TokenCursor<G::Token>,
    grammar: G,
}

type ParseTerm<'input, G> =
    fn(
        &mut BooleanParser<'input, G>,
    ) -> Result<<G as BooleanGrammar>::Expr, QueryError>;

impl<'input, G: BooleanGrammar> BooleanParser<'input, G> {
    fn parse(&mut self) -> Result<G::Expr, QueryError> {
        let expression = self.parse_or()?;
        if self.tokens.peek().is_some() {
            return Err(self.grammar.invalid(self.input));
        }
        Ok(expression)
    }

    fn parse_or(&mut self) -> Result<G::Expr, QueryError> {
        self.parse_logical_chain(LogicalOp::Or, Self::parse_and)
    }

    fn parse_and(&mut self) -> Result<G::Expr, QueryError> {
        self.parse_logical_chain(LogicalOp::And, Self::parse_not)
    }

    fn parse_logical_chain(
        &mut self,
        operator: LogicalOp,
        parse_term: ParseTerm<'input, G>,
    ) -> Result<G::Expr, QueryError> {
        let first = parse_term(self)?;
        if !self.take_control(BooleanControl::Logical(operator)) {
            return Ok(first);
        }

        let mut expressions = Vec::with_capacity(2);
        expressions.push(first);
        loop {
            expressions.push(parse_term(self)?);
            if !self.take_control(BooleanControl::Logical(operator)) {
                break;
            }
        }
        Ok(self.grammar.logical(operator, expressions))
    }

    fn parse_not(&mut self) -> Result<G::Expr, QueryError> {
        let mut count = 0usize;
        while self.take_control(BooleanControl::Not) {
            count = count.saturating_add(1);
        }

        let mut expression = self.parse_primary()?;
        for _ in 0..count {
            expression = self.grammar.not(expression);
        }
        Ok(expression)
    }

    fn parse_primary(&mut self) -> Result<G::Expr, QueryError> {
        if self.take_control(BooleanControl::LeftParen) {
            let expression = self.parse_or()?;
            if !self.take_control(BooleanControl::RightParen) {
                return Err(self.grammar.invalid(self.input));
            }
            Ok(expression)
        } else {
            self.grammar.parse_atom(self.input, &mut self.tokens)
        }
    }

    fn take_control(&mut self, expected: BooleanControl) -> bool {
        if self.tokens.peek().and_then(|token| self.grammar.control(token))
            == Some(expected)
        {
            self.tokens.next();
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use rstest::rstest;

    use super::*;

    #[derive(Clone, Debug, Eq, PartialEq)]
    enum TestToken {
        Atom(&'static str),
        Control(BooleanControl),
    }

    #[derive(Debug, Eq, PartialEq)]
    enum TestExpr {
        Atom(&'static str),
        And(Vec<TestExpr>),
        Or(Vec<TestExpr>),
        Not(Box<TestExpr>),
    }

    struct TestGrammar;

    impl BooleanGrammar for TestGrammar {
        type Expr = TestExpr;
        type Token = TestToken;

        fn control(&self, token: &Self::Token) -> Option<BooleanControl> {
            match token {
                TestToken::Control(control) => Some(*control),
                TestToken::Atom(_) => None,
            }
        }

        fn parse_atom(
            &self,
            input: &str,
            tokens: &mut TokenCursor<Self::Token>,
        ) -> Result<Self::Expr, QueryError> {
            match tokens.next() {
                Some(TestToken::Atom(atom)) => Ok(TestExpr::Atom(atom)),
                _ => Err(self.invalid(input)),
            }
        }

        fn logical(
            &self,
            operator: LogicalOp,
            expressions: Vec<Self::Expr>,
        ) -> Self::Expr {
            match operator {
                LogicalOp::And => TestExpr::And(expressions),
                LogicalOp::Or => TestExpr::Or(expressions),
            }
        }

        fn not(&self, expression: Self::Expr) -> Self::Expr {
            TestExpr::Not(Box::new(expression))
        }

        fn invalid(&self, input: &str) -> QueryError {
            QueryError::unparsable_source(input)
        }
    }

    #[test]
    fn parses_precedence_repeated_negation_and_flattened_chains() {
        use BooleanControl::{Logical, Not};
        use LogicalOp::{And, Or};
        use TestToken::{Atom, Control};

        let parsed = parse_boolean(
            "fixture",
            vec![
                Atom("a"),
                Control(Logical(Or)),
                Atom("b"),
                Control(Logical(And)),
                Control(Not),
                Control(Not),
                Atom("c"),
                Control(Logical(And)),
                Atom("d"),
            ],
            TestGrammar,
        );

        assert_eq!(
            parsed,
            Ok(TestExpr::Or(vec![
                TestExpr::Atom("a"),
                TestExpr::And(vec![
                    TestExpr::Atom("b"),
                    TestExpr::Not(Box::new(TestExpr::Not(Box::new(
                        TestExpr::Atom("c")
                    )))),
                    TestExpr::Atom("d"),
                ]),
            ]))
        );
    }

    #[test]
    fn parses_grouping_before_outer_chain() {
        use BooleanControl::{LeftParen, Logical, RightParen};
        use LogicalOp::{And, Or};
        use TestToken::{Atom, Control};

        let parsed = parse_boolean(
            "fixture",
            vec![
                Control(LeftParen),
                Atom("a"),
                Control(Logical(Or)),
                Atom("b"),
                Control(RightParen),
                Control(Logical(And)),
                Atom("c"),
            ],
            TestGrammar,
        );

        assert_eq!(
            parsed,
            Ok(TestExpr::And(vec![
                TestExpr::Or(vec![TestExpr::Atom("a"), TestExpr::Atom("b")]),
                TestExpr::Atom("c"),
            ]))
        );
    }

    #[rstest]
    #[case::empty(vec![])]
    #[case::trailing_and(vec![
        TestToken::Atom("a"),
        TestToken::Control(BooleanControl::Logical(LogicalOp::And)),
    ])]
    #[case::unmatched_left_parenthesis(vec![
        TestToken::Control(BooleanControl::LeftParen),
        TestToken::Atom("a"),
    ])]
    #[case::unexpected_right_parenthesis(vec![
        TestToken::Control(BooleanControl::RightParen),
    ])]
    #[case::adjacent_atoms(vec![
        TestToken::Atom("a"),
        TestToken::Atom("b"),
    ])]
    fn rejects_incomplete_and_adjacent_token_streams(
        #[case] tokens: Vec<TestToken>,
    ) {
        assert_eq!(
            parse_boolean("fixture", tokens, TestGrammar),
            Err(QueryError::unparsable_source("fixture"))
        );
    }
}
