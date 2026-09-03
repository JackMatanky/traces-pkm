//! Record filter expression DSL parser and evaluation engine for `--where`
//! queries.
//!
//! Defines [`FilterExpr`], which parses boolean filter expressions containing
//! field path accessors (`file.name`, `task.completed`, frontmatter keys),
//! comparison operators, and function calls (`contains`), evaluating candidate
//! [`QueryRow`] records.

use logos::{Lexer, Logos};
use miette::SourceSpan;

use super::{
    FieldPath,
    expr::{
        AtomParser, BooleanExpr, LogicalControl, LogicalOp, parse_boolean_expr,
    },
};
use crate::{
    LexError, LexTokenStream, LexedToken, TokenSpec, lexical_unquote,
    note::NoteFieldValue,
    query::{
        QueryRow,
        error::{QueryBuilderError, QueryDialect, QuerySyntaxError},
        value::QueryFieldValueRef,
    },
};

/// A parsed filter expression AST.
///
/// Wraps [`BooleanExpr`] with [`FilterAtom`] leaves, providing the concrete
/// type used by [`crate::query::QuerySet::filter`].
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct FilterExpr(BooleanExpr<FilterAtom>);

impl FilterExpr {
    /// Parses a filter expression string into a logical expression tree.
    ///
    /// # Errors
    ///
    /// - [`Syntax`] if the expression syntax is invalid or malformed.
    /// - [`FieldPath`] if any field path in the expression is invalid.
    ///
    /// [`Syntax`]: QueryBuilderError::Syntax
    /// [`FieldPath`]: QueryBuilderError::FieldPath
    pub(crate) fn parse(input: &str) -> Result<Self, QueryBuilderError> {
        let tokens = LexTokenStream::<LexedToken<FilterToken>>::tokenize_with(
            input,
            |token| {
                let span = token.span();
                match token.into_value() {
                    FilterToken::Ident(word) => match word.parse::<f64>() {
                        Ok(number) if number.is_finite() => {
                            Ok(LexedToken::new(
                                FilterToken::Literal(NoteFieldValue::Number(
                                    number,
                                )),
                                span,
                            ))
                        }
                        Ok(_) => Err(LexError::UnexpectedToken {
                            span,
                            found: "NaN or infinity".to_owned(),
                            expected: "a finite numeric literal",
                        }),
                        Err(_) => {
                            Ok(LexedToken::new(FilterToken::Ident(word), span))
                        }
                    },
                    other => Ok(LexedToken::new(other, span)),
                }
            },
        )
        .map_err(|e| {
            QuerySyntaxError::from_lex(QueryDialect::Filter, input, e)
        })?;
        parse_boolean_expr(input, tokens, FilterGrammar).map(Self)
    }

    /// Combines two filter expressions with logical AND, flattening nested
    /// `And` nodes.
    pub(crate) fn and(self, other: Self) -> Self {
        let mut children = match self.0 {
            BooleanExpr::And(children) => children,
            atom => vec![atom],
        };
        match other.0 {
            BooleanExpr::And(more) => children.extend(more),
            atom => children.push(atom),
        }
        Self(BooleanExpr::And(children))
    }

    /// Whether `record` satisfies this expression.
    pub(crate) fn is_matching(&self, record: &QueryRow) -> bool {
        self.0.is_satisfied_by(|atom| atom.is_matching(record))
    }
}

/// Atomic predicate in a filter expression.
///
/// Either a field-to-literal comparison or a recognized function call
/// (such as `contains(tags, "#book")`).
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum FilterAtom {
    /// `<field> <op> <value>` comparison.
    Comparison(ComparisonExpr),
    /// Recognized function call, such as `contains(tags, "#book")`.
    Function(FilterFunction),
}

impl FilterAtom {
    fn is_matching(&self, record: &QueryRow) -> bool {
        match self {
            Self::Comparison(comparison) => comparison.is_matching(record),
            Self::Function(function) => function.is_matching(record),
        }
    }
}

/// A recognized filter function call.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum FilterFunction {
    /// `contains(field, target)`.
    ///
    /// - Lists match by exact value or tag prefix, such as `#book` matching
    ///   `#book/fiction`.
    /// - Other field kinds fall back to substring containment.
    Contains {
        field: FieldPath,
        target: NoteFieldValue,
    },
}

impl FilterFunction {
    fn build(
        name: &str,
        field: FieldPath,
        target: NoteFieldValue,
    ) -> Option<Self> {
        if name.eq_ignore_ascii_case("contains") {
            Some(Self::Contains {
                field,
                target,
            })
        } else {
            None
        }
    }

    fn is_matching(&self, record: &QueryRow) -> bool {
        match self {
            Self::Contains {
                field,
                target,
            } => record.resolve_ref(field).is_containing(target),
        }
    }
}

/// A parsed `<field> <op> <value>` comparison node in a filter expression.
///
/// Pairs an already-parsed [`FieldPath`] with a [`CompareOp`] and a literal
/// [`NoteFieldValue`] to evaluate against the resolved field of each record.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ComparisonExpr {
    field: FieldPath,
    op: CompareOp,
    value: NoteFieldValue,
}

impl ComparisonExpr {
    pub(super) const fn new(
        field: FieldPath,
        op: CompareOp,
        value: NoteFieldValue,
    ) -> Self {
        Self {
            field,
            op,
            value,
        }
    }

    /// Returns whether the given index record satisfies this comparison
    /// expression.
    pub(super) fn is_matching(&self, record: &QueryRow) -> bool {
        self.op.is_satisfied_by(&record.resolve_ref(&self.field), &self.value)
    }
}

/// A comparison operator parsed from a filter expression.
///
/// Each variant maps to a syntactic operator in the filter language.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(super) enum CompareOp {
    /// `==`
    Eq,
    /// `!=`
    Ne,
    /// `<`
    Lt,
    /// `<=`
    Le,
    /// `>`
    Gt,
    /// `>=`
    Ge,
}

impl CompareOp {
    /// Evaluates this operator against a field value and literal.
    pub(super) fn is_satisfied_by(
        self,
        field: &QueryFieldValueRef<'_>,
        literal: &NoteFieldValue,
    ) -> bool {
        match self {
            Self::Eq => field.is_equal_to_literal(literal),
            Self::Ne => !field.is_equal_to_literal(literal),
            Self::Lt => {
                field.compare_to_literal(literal)
                    == Some(std::cmp::Ordering::Less)
            }
            Self::Gt => {
                field.compare_to_literal(literal)
                    == Some(std::cmp::Ordering::Greater)
            }
            Self::Le => matches!(
                field.compare_to_literal(literal),
                Some(std::cmp::Ordering::Less | std::cmp::Ordering::Equal)
            ),
            Self::Ge => matches!(
                field.compare_to_literal(literal),
                Some(std::cmp::Ordering::Greater | std::cmp::Ordering::Equal)
            ),
        }
    }
}

impl TryFrom<&str> for CompareOp {
    type Error = ();

    /// Attempts to parse a comparison operator from its string representation.
    fn try_from(spelling: &str) -> Result<Self, Self::Error> {
        match spelling {
            "==" => Ok(Self::Eq),
            "!=" => Ok(Self::Ne),
            ">=" => Ok(Self::Ge),
            "<=" => Ok(Self::Le),
            ">" => Ok(Self::Gt),
            "<" => Ok(Self::Lt),
            _ => Err(()),
        }
    }
}

struct FilterGrammar;

impl FilterGrammar {
    fn parse_literal_arg(
        input: &str,
        tokens: &mut LexTokenStream<LexedToken<FilterToken>>,
    ) -> Result<NoteFieldValue, QueryBuilderError> {
        let spanned = tokens
            .expect_map(input, "a literal value", |token| {
                let spanned = token;
                match spanned.into_value() {
                    FilterToken::Literal(value) => Some(value),
                    _ => None,
                }
            })
            .map_err(|e| {
                QuerySyntaxError::from_lex(QueryDialect::Filter, input, e)
            })?;
        Ok(spanned.into_value())
    }

    fn parse_function_call(
        input: &str,
        tokens: &mut LexTokenStream<LexedToken<FilterToken>>,
        name: &str,
    ) -> Result<FilterFunction, QueryBuilderError> {
        tokens
            .expect(
                input,
                TokenSpec::new(
                    &FilterToken::LParen,
                    "`(` after a function name",
                ),
            )
            .map_err(|e| {
                QuerySyntaxError::from_lex(QueryDialect::Filter, input, e)
            })?;

        let field_ident = tokens
            .expect_map(input, "a field path", |token| {
                let spanned = token;
                match spanned.into_value() {
                    FilterToken::Ident(ident) => Some(ident),
                    _ => None,
                }
            })
            .map_err(|e| {
                QuerySyntaxError::from_lex(QueryDialect::Filter, input, e)
            })?;
        let field = FieldPath::parse(field_ident.value())?;

        tokens
            .expect(
                input,
                TokenSpec::new(&FilterToken::Comma, "`,` after the field path"),
            )
            .map_err(|e| {
                QuerySyntaxError::from_lex(QueryDialect::Filter, input, e)
            })?;

        let target = Self::parse_literal_arg(input, tokens)?;

        tokens
            .expect(
                input,
                TokenSpec::new(
                    &FilterToken::RParen,
                    "`)` after the function arguments",
                ),
            )
            .map_err(|e| {
                QuerySyntaxError::from_lex(QueryDialect::Filter, input, e)
            })?;

        FilterFunction::build(name, field, target).ok_or_else(|| {
            QuerySyntaxError::new(
                QueryDialect::Filter,
                input,
                SourceSpan::from((0, name.len())),
                "`contains`",
            )
            .into()
        })
    }

    fn parse_comparison(
        input: &str,
        tokens: &mut LexTokenStream<LexedToken<FilterToken>>,
        field_ident: &str,
    ) -> Result<ComparisonExpr, QueryBuilderError> {
        let op_spanned = tokens
            .expect_map(input, "a comparison operator", |token| {
                let spanned = token;
                match spanned.into_value() {
                    FilterToken::Op(op) => Some(op),
                    _ => None,
                }
            })
            .map_err(|e| {
                QuerySyntaxError::from_lex(QueryDialect::Filter, input, e)
            })?;
        let field = FieldPath::parse(field_ident)?;
        let value = Self::parse_literal_arg(input, tokens)?;
        Ok(ComparisonExpr::new(field, *op_spanned.value(), value))
    }
}

impl AtomParser for FilterGrammar {
    type Atom = FilterAtom;
    type Token = FilterToken;

    fn control(&self, token: &Self::Token) -> Option<LogicalControl> {
        match token {
            FilterToken::Logical(operator) => {
                Some(LogicalControl::Operator(*operator))
            }
            FilterToken::Not => Some(LogicalControl::Not),
            FilterToken::LParen => Some(LogicalControl::LeftParen),
            FilterToken::RParen => Some(LogicalControl::RightParen),
            FilterToken::Comma
            | FilterToken::Op(_)
            | FilterToken::Literal(_)
            | FilterToken::Ident(_) => None,
        }
    }

    fn parse_atom(
        &self,
        input: &str,
        tokens: &mut LexTokenStream<LexedToken<Self::Token>>,
    ) -> Result<Self::Atom, QueryBuilderError> {
        let spanned_ident = tokens
            .expect_map(input, "a filter term", |token| {
                let spanned = token;
                match spanned.into_value() {
                    FilterToken::Ident(name) => Some(name),
                    _ => None,
                }
            })
            .map_err(|e| {
                QuerySyntaxError::from_lex(QueryDialect::Filter, input, e)
            })?;

        if tokens.peek_is_value(&FilterToken::LParen) {
            Self::parse_function_call(input, tokens, spanned_ident.value())
                .map(FilterAtom::Function)
        } else {
            Self::parse_comparison(input, tokens, spanned_ident.value())
                .map(FilterAtom::Comparison)
        }
    }

    fn syntax_error(
        &self,
        input: &str,
        span: SourceSpan,
        expected: &'static str,
    ) -> QuerySyntaxError {
        QuerySyntaxError::new(QueryDialect::Filter, input, span, expected)
    }
}

/// Lexical tokens parsed from a filter expression.
#[derive(Logos, Clone, Debug, PartialEq)]
#[logos(skip r"[ \t\n\r\f]+")]
enum FilterToken {
    #[token("(")]
    LParen,
    #[token(")")]
    RParen,
    #[token(",")]
    Comma,
    #[regex(
        "&&|and|\\|\\||or",
        |lex| LogicalOp::try_from(lex.slice()),
        ignore(case)
    )]
    Logical(LogicalOp),
    #[token("!")]
    #[token("not", ignore(case))]
    Not,
    #[regex("==|!=|>=|<=|>|<", |lex| CompareOp::try_from(lex.slice()))]
    Op(CompareOp),
    #[regex(r#""([^"\\]|\\.)*""#, string_callback)]
    #[regex(r"'([^'\\]|\\.)*'", string_callback)]
    #[token("true", |_| NoteFieldValue::Bool(true), priority = 3)]
    #[token("false", |_| NoteFieldValue::Bool(false), priority = 3)]
    #[token("null", |_| NoteFieldValue::Null, priority = 3)]
    #[token("Null", |_| NoteFieldValue::Null, priority = 3)]
    Literal(NoteFieldValue),
    #[regex(r#"[^\s()'",=!<>&|]+"#, |lex| lex.slice().to_owned())]
    Ident(String),
}

/// Unescapes a lexed single- or double-quoted string literal into a
/// [`NoteFieldValue::String`].
#[expect(
    clippy::needless_pass_by_ref_mut,
    reason = "logos Callback trait requires &mut Lexer"
)]
fn string_callback(lex: &mut Lexer<'_, FilterToken>) -> NoteFieldValue {
    NoteFieldValue::String(lexical_unquote(lex.slice()))
}

#[cfg(test)]
mod tests {
    use std::{fs, path::Path, sync::Arc};

    use super::FilterExpr;
    use crate::{
        index::IndexerService,
        query::{QueryError, *},
    };

    fn outcome_for_files(temp: &Path, files: &[(&str, &str)]) -> QuerySet {
        for (name, content) in files {
            fs::write(temp.join(name), content).expect("write note");
        }
        let index =
            Arc::new(IndexerService::new(temp).build().expect("build index"));
        QueryService::new("class")
            .execute(&index, QueryBuilder::pages(SourceSelector::All))
    }

    fn outcome_for(temp: &Path, content: &str) -> QuerySet {
        outcome_for_files(temp, &[("note.md", content)])
    }

    fn rated_outcome(temp: &Path) -> QuerySet {
        outcome_for_files(temp, &[
            ("low.md", "---\nrating: 3\nstatus: draft\n---"),
            ("high.md", "---\nrating: 7\nstatus: done\n---"),
            ("unrated.md", "---\nstatus: done\n---"),
        ])
    }

    fn names(outcome: &QuerySet) -> Vec<String> {
        outcome
            .iter()
            .map(|record| record.file().name().as_str().to_owned())
            .collect()
    }

    mod parse {
        use miette::SourceSpan;
        use pretty_assertions::assert_eq;
        use rstest::rstest;

        use super::*;
        use crate::LexError;

        #[rstest]
        #[case::no_operator("rating")]
        #[case::empty_field(" > 5")]
        #[case::empty_value("rating >")]
        #[case::unquoted_string("status == done")]
        #[case::unknown_function("unknown(tags, \"#book\")")]
        #[case::function_missing_target("contains(tags)")]
        fn rejects_malformed_expressions(#[case] expr: &str) {
            let temp = tempfile::tempdir().expect("create temp dir");
            let outcome = rated_outcome(temp.path());

            assert!(matches!(
                outcome.filter(expr),
                Err(QueryError::Request(QueryBuilderError::Syntax(_)))
            ));
        }

        #[rstest]
        #[case::empty("")]
        #[case::trailing_operator("rating > 5 and")]
        #[case::unmatched_left_parenthesis("(rating > 5")]
        #[case::unmatched_right_parenthesis("rating > 5)")]
        #[case::adjacent_expressions("rating > 5 status == \"done\"")]
        fn rejects_incomplete_boolean_logic(#[case] expr: &str) {
            assert!(matches!(
                FilterExpr::parse(expr),
                Err(QueryBuilderError::Syntax(_))
            ));
        }

        #[rstest]
        #[case::nan("rating > NaN", 9, 3)]
        #[case::positive_infinity("rating > inf", 9, 3)]
        #[case::negative_infinity("rating > -inf", 9, 4)]
        fn rejects_non_finite_numeric_literals(
            #[case] expr: &str,
            #[case] offset: usize,
            #[case] length: usize,
        ) {
            let result = FilterExpr::parse(expr);
            assert!(
                matches!(result, Err(QueryBuilderError::Syntax(_))),
                "expected syntax error"
            );
            if let Err(QueryBuilderError::Syntax(error)) = result {
                assert_eq!(*error.lex_error, LexError::UnexpectedToken {
                    span: SourceSpan::from((offset, length)),
                    found: "NaN or infinity".to_owned(),
                    expected: "a finite numeric literal",
                });
                assert_eq!(error.span, SourceSpan::from((offset, length)));
            }
        }

        #[test]
        fn rejects_malformed_field_path_in_expression() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let outcome = rated_outcome(temp.path());

            assert_eq!(
                outcome.filter("file.bogus == 1"),
                Err(QueryError::Request(QueryBuilderError::FieldPath(
                    FieldPathError::new("file.bogus", None)
                )))
            );
        }

        #[test]
        fn rejects_malformed_field_path_in_function() {
            assert_eq!(
                FilterExpr::parse("contains(file.bogus, \"x\")"),
                Err(QueryBuilderError::FieldPath(FieldPathError::new(
                    "file.bogus",
                    None
                )))
            );
        }
    }

    mod filter {
        use pretty_assertions::assert_eq;
        use rstest::rstest;

        use super::*;

        #[rstest]
        #[case::greater_than("rating > 5", &["high"])]
        #[case::greater_or_equal("rating >= 7", &["high"])]
        #[case::less_than("rating < 5", &["low"])]
        #[case::less_or_equal("rating <= 3", &["low"])]
        #[case::numeric_equal("rating == 7", &["high"])]
        #[case::string_equal("status == \"done\"", &["high", "unrated"])]
        #[case::single_quoted_string_equal("status == 'done'", &["high", "unrated"])]
        #[case::string_not_equal("status != \"done\"", &["low"])]
        fn keeps_only_matching_records(
            #[case] expr: &str,
            #[case] expected: &[&str],
        ) {
            let temp = tempfile::tempdir().expect("create temp dir");
            let outcome = rated_outcome(temp.path());

            let filtered = outcome.filter(expr).expect("valid filter");

            assert_eq!(names(&filtered), expected);
        }

        #[test]
        fn missing_field_never_matches_equality_or_ordering() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let outcome = rated_outcome(temp.path());

            let filtered = outcome.filter("rating > 0").expect("valid filter");

            assert_eq!(names(&filtered), ["high", "low"]);
        }

        #[test]
        fn missing_field_matches_not_equal() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let outcome = rated_outcome(temp.path());

            let filtered = outcome.filter("rating != 7").expect("valid filter");

            assert_eq!(names(&filtered), ["low", "unrated"]);
        }

        #[test]
        fn type_mismatch_never_matches_ordering_or_equality() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let outcome = rated_outcome(temp.path());

            let filtered = outcome.filter("status > 5").expect("valid filter");

            assert!(filtered.is_empty());
        }

        #[test]
        fn chains_across_multiple_filter_calls() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let outcome = rated_outcome(temp.path());

            let filtered = outcome
                .filter("status == \"done\"")
                .expect("valid filter")
                .filter("rating >= 7")
                .expect("valid filter");

            assert_eq!(names(&filtered), ["high"]);
        }

        #[test]
        fn equal_matches_a_date_field_against_a_string_literal() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let outcome = outcome_for(temp.path(), "---\ndue: 2026-01-01\n---");

            let filtered =
                outcome.filter("due == \"2026-01-01\"").expect("valid filter");

            assert_eq!(filtered.len(), 1);
        }

        #[test]
        fn filters_with_null_literal() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let outcome = rated_outcome(temp.path());

            let with_null =
                outcome.clone().filter("rating == null").expect("valid filter");
            let without_null =
                outcome.filter("rating != null").expect("valid filter");

            assert_eq!(names(&with_null), ["unrated"]);
            assert_eq!(names(&without_null), ["high", "low"]);
        }

        #[test]
        fn r_where_alias_filters_records_identically_to_filter() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let outcome = rated_outcome(temp.path());

            let filtered = outcome.r#where("rating >= 7").expect("valid where");

            assert_eq!(names(&filtered), ["high"]);
        }

        #[test]
        fn and_combination_keeps_only_records_matching_both_sides() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let outcome = rated_outcome(temp.path());

            let filtered = outcome
                .filter("rating > 5 AND status == \"done\"")
                .expect("valid filter");

            assert_eq!(names(&filtered), ["high"]);
        }

        #[test]
        fn or_combination_keeps_records_matching_either_side() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let outcome = rated_outcome(temp.path());

            let filtered = outcome
                .filter("rating == 3 OR status == \"done\"")
                .expect("valid filter");

            assert_eq!(names(&filtered), ["high", "low", "unrated"]);
        }

        #[test]
        fn not_combination_reverses_the_matching_condition() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let outcome = rated_outcome(temp.path());

            let filtered =
                outcome.filter("NOT status == \"done\"").expect("valid filter");

            assert_eq!(names(&filtered), ["low"]);
        }

        #[test]
        fn default_boolean_precedence_evaluates_correctly() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let outcome = rated_outcome(temp.path());

            let filtered = outcome
                .filter(
                    "status == \"done\" OR rating == 3 AND status == \"draft\"",
                )
                .expect("valid filter");

            assert_eq!(names(&filtered), ["high", "low", "unrated"]);
        }

        #[test]
        fn nested_parentheses_override_precedence() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let outcome = rated_outcome(temp.path());

            let nested = outcome
                .filter(
                    "(rating > 5 OR status == \"draft\") AND NOT rating == 3",
                )
                .expect("valid filter");

            assert_eq!(names(&nested), ["high"]);
        }

        #[test]
        fn logical_op_spellings_do_not_swallow_identifier_prefixes() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let outcome =
                outcome_for(temp.path(), "---\norder: 5\nandrew: 3\n---");

            let lower = outcome
                .clone()
                .filter("order == 5 and andrew == 3")
                .expect("valid filter: lowercase and");
            assert_eq!(lower.len(), 1);

            let symbolic_and = outcome
                .clone()
                .filter("order == 5 && andrew == 3")
                .expect("valid filter: &&");
            assert_eq!(symbolic_and.len(), 1);

            let symbolic_or = outcome
                .clone()
                .filter("order == 999 || andrew == 3")
                .expect("valid filter: ||");
            assert_eq!(symbolic_or.len(), 1);

            let lower_or = outcome
                .clone()
                .filter("order == 999 or andrew == 3")
                .expect("valid filter: lowercase or");
            assert_eq!(lower_or.len(), 1);

            // Fields literally named `order`/`andrew` must resolve as whole
            // identifiers, not get truncated by the Logical token's "or"/"and"
            // prefix.
            let ident_prefix =
                outcome.filter("order == 5").expect("valid filter: bare field");
            assert_eq!(ident_prefix.len(), 1);
        }
    }

    mod contains {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn contains_matches_tags_by_prefix_hierarchy() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let outcome = outcome_for_files(temp.path(), &[
                (
                    "book.md",
                    "---\ntitle: Rust Handbook\n---\nFiled under #book/fiction",
                ),
                (
                    "article.md",
                    "---\ntitle: Async Guide\n---\nFiled under #article",
                ),
            ]);

            let filtered = outcome
                .filter("contains(tags, \"#book\")")
                .expect("valid filter");

            assert_eq!(names(&filtered), ["book"]);
        }

        #[test]
        fn contains_matches_string_fields_by_substring() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let outcome = outcome_for_files(temp.path(), &[
                (
                    "book.md",
                    "---\ntitle: Rust Handbook\n---\nFiled under #book/fiction",
                ),
                (
                    "article.md",
                    "---\ntitle: Async Guide\n---\nFiled under #article",
                ),
            ]);

            let filtered = outcome
                .filter("contains(title, \"Async\")")
                .expect("valid filter");

            assert_eq!(names(&filtered), ["article"]);
        }

        #[test]
        fn contains_distinguishes_list_values_from_tag_hierarchy() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let outcome = outcome_for_files(temp.path(), &[
                (
                    "handbook.md",
                    "---\ncategories: [handbook]\n---\nTagged #bookworm",
                ),
                (
                    "book.md",
                    "---\ncategories: [book]\n---\nTagged #book/fiction",
                ),
            ]);

            let category_match = outcome
                .clone()
                .filter("contains(categories, \"book\")")
                .expect("valid filter");
            assert_eq!(names(&category_match), ["book"]);

            let tag_match = outcome
                .filter("contains(tags, \"#book\")")
                .expect("valid filter");
            assert_eq!(names(&tag_match), ["book"]);
        }
    }

    mod is_containing {
        use pretty_assertions::assert_eq;

        use crate::{
            note::NoteFieldValue,
            query::value::{QueryFieldValueRef, QueryListValueRef},
        };

        #[test]
        fn matches_identically_for_borrowed_and_owned_list_values() {
            // Regression: the Owned(List) arm used to re-implement
            // list_contains's matching inline instead of delegating to it;
            // both arms must now produce identical results for the same
            // logical list.
            let items =
                vec![NoteFieldValue::String("#book/fiction".to_owned())];
            let target = NoteFieldValue::String("#book".to_owned());

            let borrowed =
                QueryFieldValueRef::List(QueryListValueRef::Values(&items))
                    .is_containing(&target);
            let owned = QueryFieldValueRef::Owned(NoteFieldValue::List(items))
                .is_containing(&target);

            assert_eq!(borrowed, owned);
            assert!(borrowed, "#book must match #book/fiction by tag prefix");
        }
    }
}
