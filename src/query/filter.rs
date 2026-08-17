//! Parsing and evaluation of record filter expressions.
//!
//! This module implements the filter expression language used by
//! [`super::QueryOutcome::filter`].
//!
//! # Record Filter Grammar
//!
//! Expressions combine comparisons, function calls, and logical operators:
//! - **Comparisons:** e.g., `rating > 5`, `status == "done"`, using `==`, `!=`,
//!   `<`, `<=`, `>`, `>=`.
//! - **Function Calls:** e.g., `contains(tags, "#book")` checks list/tag
//!   membership or substring containment.
//! - **Logical Operators:** `AND`/`and`/`&&`, `OR`/`or`/`||`, `NOT`/`not`/`!`,
//!   and parentheses for grouping.
//!
//! # Examples
//!
//! ```ignore
//! # use traces_pkm::query::FilterExpr;
//! let expr = FilterExpr::parse("rating > 5 and status == \"done\"").unwrap();
//! ```

use logos::{Lexer, Logos};
use miette::SourceSpan;

use super::{
    FieldPath, IndexRecord, QueryError,
    comparison::{CompareOp, ComparisonExpr},
    error::{QueryDialect, QuerySyntaxError},
    logic::{
        LogicalControl, LogicalExpr, LogicalGrammar, LogicalOp, Spanned,
        TokenCursor, parse_logical_expression,
    },
    sort::fields_equal,
};
use crate::note::FieldValue;

/// A parsed filter expression AST.
///
/// Wraps [`LogicalExpr`] with [`FilterAtom`] leaves, providing the concrete
/// type used by [`super::QueryOutcome::filter`].
pub(super) type FilterExpr = LogicalExpr<FilterAtom>;

/// Atomic predicate in a filter expression.
///
/// Either a field-to-literal comparison or a recognized function call
/// (such as `contains(tags, "#book")`).
#[derive(Clone, Debug, PartialEq)]
pub(super) enum FilterAtom {
    /// `<field> <op> <value>` comparison.
    Comparison(ComparisonExpr),
    /// Recognized function call, such as `contains(tags, "#book")`.
    Function(FilterFunction),
}

impl FilterAtom {
    fn matches(&self, record: &IndexRecord) -> bool {
        match self {
            Self::Comparison(comparison) => comparison.matches(record),
            Self::Function(function) => function.matches(record),
        }
    }
}

impl LogicalExpr<FilterAtom> {
    /// Parses a filter expression string into a logical expression tree.
    ///
    /// # Errors
    ///
    /// Returns [`QueryError::Syntax`] if the expression syntax is invalid.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// # use traces_pkm::query::FilterExpr;
    /// let expr = FilterExpr::parse("rating > 5").unwrap();
    /// ```
    pub(super) fn parse(input: &str) -> Result<Self, QueryError> {
        parse_logical_expression(
            input,
            tokenize_filter_expr(input)?,
            FilterGrammar,
        )
    }

    /// Whether `record` satisfies this expression.
    pub(super) fn matches(&self, record: &IndexRecord) -> bool {
        match self {
            Self::Atom(atom) => atom.matches(record),
            Self::And(expressions) => {
                expressions.iter().all(|expression| expression.matches(record))
            }
            Self::Or(expressions) => {
                expressions.iter().any(|expression| expression.matches(record))
            }
            Self::Not(expression) => !expression.matches(record),
        }
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
    #[token("true", |_| FieldValue::Bool(true), priority = 3)]
    #[token("false", |_| FieldValue::Bool(false), priority = 3)]
    #[token("null", |_| FieldValue::Null, priority = 3)]
    #[token("Null", |_| FieldValue::Null, priority = 3)]
    Literal(FieldValue),
    #[regex(r#"[^\s()",=!<>&|]+"#, |lex| lex.slice().to_owned())]
    Ident(String),
}

fn syntax_error(
    input: &str,
    span: SourceSpan,
    expected: &'static str,
) -> QueryError {
    QuerySyntaxError::new(QueryDialect::Filter, input, span, expected).into()
}

fn cursor_span(
    input: &str,
    tokens: &mut TokenCursor<Spanned<FilterToken>>,
) -> SourceSpan {
    tokens
        .peek()
        .map_or_else(|| SourceSpan::from((input.len(), 0)), |token| token.span)
}

/// Tokenizes `input`, preserving each token's original byte span.
fn tokenize_filter_expr(
    input: &str,
) -> Result<Vec<Spanned<FilterToken>>, QueryError> {
    let mut lexer = FilterToken::lexer(input);
    let mut tokens = Vec::new();
    while let Some(result) = lexer.next() {
        let range = lexer.span();
        let span = SourceSpan::from((range.start, range.len()));
        let value =
            result.map_err(|()| syntax_error(input, span, "a filter term"))?;
        let token = match value {
            FilterToken::Ident(word) => match word.parse::<f64>() {
                Ok(number) if number.is_finite() => Spanned::new(
                    FilterToken::Literal(FieldValue::Number(number)),
                    span,
                ),
                Ok(_) => {
                    return Err(syntax_error(
                        input,
                        span,
                        "a finite numeric literal",
                    ));
                }
                Err(_) => Spanned::new(FilterToken::Ident(word), span),
            },
            other => Spanned::new(other, span),
        };
        tokens.push(token);
    }
    Ok(tokens)
}

/// Unescapes a lexed double-quoted string literal into a
/// [`FieldValue::String`].
fn string_callback(lex: &mut Lexer<'_, FilterToken>) -> FieldValue {
    let inner = lex
        .slice()
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
        .unwrap_or_default();
    let mut value = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            if let Some(escaped) = chars.next() {
                value.push(escaped);
            }
        } else {
            value.push(ch);
        }
    }
    FieldValue::String(value)
}

struct FilterGrammar;

impl LogicalGrammar for FilterGrammar {
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
        tokens: &mut TokenCursor<Spanned<Self::Token>>,
    ) -> Result<Self::Atom, QueryError> {
        match tokens.next() {
            Some(Spanned {
                value: FilterToken::Ident(name),
                ..
            }) if tokens
                .peek()
                .is_some_and(|token| token.value == FilterToken::LParen) =>
            {
                parse_function_call(input, tokens, &name)
                    .map(FilterAtom::Function)
            }
            Some(Spanned {
                value: FilterToken::Ident(name),
                ..
            }) => parse_comparison(input, tokens, &name)
                .map(FilterAtom::Comparison),
            Some(token) => {
                Err(syntax_error(input, token.span, "a filter term"))
            }
            None => Err(syntax_error(
                input,
                SourceSpan::from((input.len(), 0)),
                "a filter term",
            )),
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

fn parse_literal_arg(
    input: &str,
    tokens: &mut TokenCursor<Spanned<FilterToken>>,
) -> Result<FieldValue, QueryError> {
    match tokens.next() {
        Some(Spanned {
            value: FilterToken::Literal(value),
            ..
        }) => Ok(value),
        Some(token) => Err(syntax_error(input, token.span, "a literal value")),
        None => Err(syntax_error(
            input,
            SourceSpan::from((input.len(), 0)),
            "a literal value",
        )),
    }
}

fn parse_function_call(
    input: &str,
    tokens: &mut TokenCursor<Spanned<FilterToken>>,
    name: &str,
) -> Result<FilterFunction, QueryError> {
    if !tokens.take(&FilterToken::LParen) {
        return Err(syntax_error(
            input,
            cursor_span(input, tokens),
            "`(` after a function name",
        ));
    }
    let Some(Spanned {
        value: FilterToken::Ident(field_ident),
        ..
    }) = tokens.next()
    else {
        return Err(syntax_error(
            input,
            cursor_span(input, tokens),
            "a field path",
        ));
    };
    let field = FieldPath::parse(&field_ident)?;
    if !tokens.take(&FilterToken::Comma) {
        return Err(syntax_error(
            input,
            cursor_span(input, tokens),
            "`,` after the field path",
        ));
    }
    let target = parse_literal_arg(input, tokens)?;
    if !tokens.take(&FilterToken::RParen) {
        return Err(syntax_error(
            input,
            cursor_span(input, tokens),
            "`)` after the function arguments",
        ));
    }
    FilterFunction::build(name, field, target).ok_or_else(|| {
        syntax_error(input, SourceSpan::from((0, name.len())), "`contains`")
    })
}

fn parse_comparison(
    input: &str,
    tokens: &mut TokenCursor<Spanned<FilterToken>>,
    field_ident: &str,
) -> Result<ComparisonExpr, QueryError> {
    let Some(Spanned {
        value: FilterToken::Op(operator),
        ..
    }) = tokens.next()
    else {
        return Err(syntax_error(
            input,
            cursor_span(input, tokens),
            "a comparison operator",
        ));
    };
    let field = FieldPath::parse(field_ident)?;
    let value = parse_literal_arg(input, tokens)?;
    Ok(ComparisonExpr::new(field, operator, value))
}

/// A recognized filter function call.
///
/// Adding a function requires adding a variant here, a name check in
/// [`Self::build`], and matching logic in [`Self::matches`].
///
/// # Examples
///
/// ```ignore
/// # use traces_pkm::query::filter::FilterFunction;
/// // e.g., FilterFunction::Contains
/// ```
#[derive(Clone, Debug, PartialEq)]
pub(super) enum FilterFunction {
    /// `contains(field, target)`.
    ///
    /// - Lists match by exact value or tag prefix, such as `#book` matching
    ///   `#book/fiction`.
    /// - Other field kinds fall back to substring containment.
    Contains {
        field: FieldPath,
        target: FieldValue,
    },
}

impl FilterFunction {
    /// Builds the function call named `name` if it names a known function.
    ///
    /// Returns `None` if the name does not match any known function.
    ///
    /// # Arguments
    ///
    /// * `name` - Function name to match, case-insensitively.
    /// * `field` - Already-parsed field path for the built call.
    /// * `target` - Comparison or membership target for the built call.
    fn build(name: &str, field: FieldPath, target: FieldValue) -> Option<Self> {
        if name.eq_ignore_ascii_case("contains") {
            Some(Self::Contains {
                field,
                target,
            })
        } else {
            None
        }
    }

    /// Returns whether `record` satisfies this function call.
    fn matches(&self, record: &IndexRecord) -> bool {
        match self {
            Self::Contains {
                field,
                target,
            } => eval_contains(&record.resolve(field), target),
        }
    }
}

/// Evaluates a `contains(field_val, target)` call.
///
/// For list fields, matches by exact value or tag prefix (for example,
/// `#book` matching `#book/fiction`). For other field kinds, falls back
/// to substring containment on stringified values.
fn eval_contains(field_val: &FieldValue, target: &FieldValue) -> bool {
    match field_val {
        FieldValue::List(items) => {
            items.iter().any(|item| tag_or_value_matches(item, target))
        }
        _ => match (field_val.as_str(), target.as_str()) {
            (Some(haystack), Some(needle)) => haystack.contains(needle),
            _ => false,
        },
    }
}

/// Returns whether list element `item` matches `target`.
///
/// Values match exactly. Tag values also match when `item` is nested
/// directly or transitively under `target` (for example, `#book/fiction`
/// under `#book`). Both parameters must be string values for tag prefix
/// matching; non-string pairs fall through to exact equality only.
fn tag_or_value_matches(item: &FieldValue, target: &FieldValue) -> bool {
    if fields_equal(item, target) {
        return true;
    }
    let (Some(item_str), Some(target_str)) = (item.as_str(), target.as_str())
    else {
        return false;
    };
    item_str.starts_with('#')
        && target_str.starts_with('#')
        && item_str
            .strip_prefix(target_str)
            .is_some_and(|suffix| suffix.starts_with('/'))
}

#[cfg(test)]
mod tests {
    use std::{fs, path::Path};

    use super::super::*;
    use crate::index::FileIndex;

    fn outcome_for_files(temp: &Path, files: &[(&str, &str)]) -> QueryOutcome {
        for (name, content) in files {
            fs::write(temp.join(name), content).expect("write note");
        }
        FileIndex::build(temp).expect("build index").query(&QuerySource::All)
    }

    fn outcome_for(temp: &Path, content: &str) -> QueryOutcome {
        outcome_for_files(temp, &[("note.md", content)])
    }

    fn rated_outcome(temp: &Path) -> QueryOutcome {
        outcome_for_files(temp, &[
            ("low.md", "---\nrating: 3\nstatus: draft\n---"),
            ("high.md", "---\nrating: 7\nstatus: done\n---"),
            ("unrated.md", "---\nstatus: done\n---"),
        ])
    }

    fn names(outcome: &QueryOutcome) -> Vec<String> {
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

            assert!(matches!(outcome.filter(expr), Err(QueryError::Syntax(_))));
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
                Err(QueryError::Syntax(_))
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
                matches!(result, Err(QueryError::Syntax(_))),
                "expected syntax error"
            );
            if let Err(QueryError::Syntax(error)) = result {
                assert_eq!(error.expected, "a finite numeric literal");
                assert_eq!(error.span, SourceSpan::from((offset, length)));
            }
        }

        #[test]
        fn rejects_malformed_field_path_in_expression() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let outcome = rated_outcome(temp.path());

            assert_eq!(
                outcome.filter("file.bogus == 1"),
                Err(QueryError::FieldPath(FieldPathError::new(
                    "file.bogus",
                    None
                )))
            );
        }

        #[test]
        fn rejects_malformed_field_path_in_function() {
            assert_eq!(
                FilterExpr::parse("contains(file.bogus, \"x\")"),
                Err(QueryError::FieldPath(FieldPathError::new(
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
}
