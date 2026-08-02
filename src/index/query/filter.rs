//! Filter expression parsing and evaluation.
//!
//! # Main Types
//!
//! - [`FilterExpr`] - Parsed AST for `.filter()` and `.where()` expressions.
//! - [`FilterToken`] - Token stream produced from a filter expression by
//!   [`tokenize_filter_expr`].
//! - [`FilterParser`] - Turns tokens into a [`FilterExpr`].
//! - [`FilterFunction`] - Recognized calls such as `contains(tags, "#book")`.

use std::vec;

use logos::{Lexer, Logos};

use super::{
    FieldPath, IndexRecord, QueryError,
    operators::{CompareOp, ComparisonExpr, LogicalExpr, LogicalOp},
    sort::fields_equal,
};
use crate::note::FieldValue;

/// A parsed `.filter()`/`.where()` expression AST.
///
/// Built by [`Self::parse`] and evaluated against a record by
/// [`Self::matches`]; see [`Self::parse`] for the supported syntax.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum FilterExpr {
    /// `<field> <op> <value>` comparison.
    Comparison(ComparisonExpr),
    /// Recognized function call, such as `contains(tags, "#book")`.
    Function(FilterFunction),
    /// `AND`/`OR` combination of two or more expressions.
    Logical(LogicalExpr),
    /// `NOT` negation of a single expression.
    Not(Box<FilterExpr>),
}

impl FilterExpr {
    /// Parses a filter expression string into a [`FilterExpr`] AST.
    ///
    /// Supports:
    /// - Comparisons: `<field> <op> <value>` with `==`, `!=`, `>=`, `<=`, `>`,
    ///   or `<`.
    /// - Functions: `contains(field, value)`.
    /// - Boolean logic: `AND` / `and` / `&&`, `OR` / `or` / `||`, and `NOT` /
    ///   `not` / `!`.
    /// - Parentheses: `( ... )`.
    ///
    /// # Errors
    ///
    /// - [`QueryError::UnparsableFilterExpression`] if `expr` is malformed.
    /// - [`QueryError::UnknownFieldPath`] if a field path is malformed.
    pub(super) fn parse(expr: &str) -> Result<Self, QueryError> {
        let tokens = tokenize_filter_expr(expr)?;
        let mut parser = FilterParser::new(expr, tokens);
        let ast = parser.parse_expr()?;
        if parser.peek().is_some() {
            return Err(QueryError::unparsable_filter(expr));
        }
        Ok(ast)
    }

    /// Whether `record` satisfies this expression.
    pub(super) fn matches(&self, record: &IndexRecord) -> bool {
        match self {
            Self::Comparison(cmp) => cmp.matches(record),
            Self::Function(function) => function.matches(record),
            Self::Logical(logical) => logical.matches(record),
            Self::Not(expr) => !expr.matches(record),
        }
    }
}

/// Tokens parsed from a filter expression.
///
/// - `true`/`false`/`null`/`Null` are matched directly by their own
///   [`Self::Literal`] token pattern, so every spelling is visible here.
/// - [`Self::Op`] and [`Self::Logical`] delegate their spellings to
///   [`CompareOp`] and [`LogicalOp`] respectively, so this enum never repeats
///   operator semantics those types already own.
/// - Numbers cannot be matched by a fixed-literal pattern; they lex as
///   [`Self::Ident`] and are reclassified by [`Self::reclassify`].
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

impl FilterToken {
    /// Reclassifies numeric identifiers into [`Self::Literal`] values.
    ///
    /// Identifiers that parse as [`f64`] become numeric literals. Genuine field
    /// identifiers and other token kinds pass through unchanged. This relies on
    /// `f64`'s parser instead of duplicating support for exponents, signs, and
    /// edge cases in a regex.
    fn reclassify(self) -> Self {
        match self {
            Self::Ident(word) => match word.parse::<f64>() {
                Ok(num) => Self::Literal(FieldValue::Number(num)),
                Err(_) => Self::Ident(word),
            },
            other => other,
        }
    }
}

/// Tokenizes `expr` into a vector of [`FilterToken`]s.
///
/// Bare words that are not recognized keywords or literal tokens lex as
/// [`FilterToken::Ident`] and are reclassified by
/// [`FilterToken::reclassify`], since numeric literals cannot be matched by a
/// fixed-literal pattern.
///
/// # Errors
///
/// - [`QueryError::UnparsableFilterExpression`] if `expr` contains a character
///   sequence no token pattern matches
fn tokenize_filter_expr(expr: &str) -> Result<Vec<FilterToken>, QueryError> {
    FilterToken::lexer(expr)
        .collect::<Result<Vec<_>, _>>()
        .map(|tokens| tokens.into_iter().map(FilterToken::reclassify).collect())
        .map_err(|()| QueryError::unparsable_filter(expr))
}

/// Unescapes a lexed double-quoted string literal into a
/// [`FieldValue::String`].
///
/// Every `\X` pair pushes `X` verbatim, matching Dataview's simple escaping
/// instead of Rust-style escape sequences.
fn string_callback(lex: &mut Lexer<'_, FilterToken>) -> FieldValue {
    let inner = lex
        .slice()
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
        .unwrap_or_default();
    let mut value = String::new();
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

/// Recursive descent parser for [`FilterExpr`] ASTs.
struct FilterParser<'a> {
    expr: &'a str,
    tokens: std::iter::Peekable<vec::IntoIter<FilterToken>>,
}

impl<'a> FilterParser<'a> {
    fn new(expr: &'a str, tokens: Vec<FilterToken>) -> Self {
        Self {
            expr,
            tokens: tokens.into_iter().peekable(),
        }
    }

    fn peek(&mut self) -> Option<&FilterToken> {
        self.tokens.peek()
    }

    fn parse_expr(&mut self) -> Result<FilterExpr, QueryError> {
        self.parse_or()
    }

    fn invalid(&self) -> QueryError {
        QueryError::unparsable_filter(self.expr)
    }

    fn next(&mut self) -> Option<FilterToken> {
        self.tokens.next()
    }

    fn expect(&mut self, expected: FilterToken) -> Result<(), QueryError> {
        if self.next() == Some(expected) {
            Ok(())
        } else {
            Err(self.invalid())
        }
    }

    fn parse_or(&mut self) -> Result<FilterExpr, QueryError> {
        self.parse_logical_chain(LogicalOp::Or, Self::parse_and)
    }

    fn parse_and(&mut self) -> Result<FilterExpr, QueryError> {
        self.parse_logical_chain(LogicalOp::And, Self::parse_not)
    }

    /// Parses a left-associative chain of `term`s separated by `op`'s
    /// token spelling, combining more than one term into a
    /// [`FilterExpr::Logical`] under `op`. A lone term passes through
    /// unwrapped.
    fn parse_logical_chain(
        &mut self,
        op: LogicalOp,
        mut term: impl FnMut(&mut Self) -> Result<FilterExpr, QueryError>,
    ) -> Result<FilterExpr, QueryError> {
        let left = term(self)?;
        let mut arms = Vec::new();
        while self.peek() == Some(&FilterToken::Logical(op)) {
            self.next();
            let right = term(self)?;
            if arms.is_empty() {
                arms.push(left.clone());
            }
            arms.push(right);
        }
        if arms.is_empty() {
            Ok(left)
        } else {
            Ok(FilterExpr::Logical(LogicalExpr::new(op, arms)))
        }
    }

    fn parse_not(&mut self) -> Result<FilterExpr, QueryError> {
        if self.peek() == Some(&FilterToken::Not) {
            self.next();
            let expr = self.parse_not()?;
            Ok(FilterExpr::Not(Box::new(expr)))
        } else {
            self.parse_primary()
        }
    }

    fn parse_literal_arg(&mut self) -> Result<FieldValue, QueryError> {
        match self.next() {
            Some(FilterToken::Literal(val)) => Ok(val),
            _ => Err(self.invalid()),
        }
    }

    fn parse_primary(&mut self) -> Result<FilterExpr, QueryError> {
        let Some(tok) = self.next() else {
            return Err(self.invalid());
        };
        match tok {
            FilterToken::LParen => {
                let expr = self.parse_expr()?;
                self.expect(FilterToken::RParen)?;
                Ok(expr)
            }
            FilterToken::Ident(name)
                if self.peek() == Some(&FilterToken::LParen) =>
            {
                self.parse_function_call(&name).map(FilterExpr::Function)
            }
            FilterToken::Ident(name) => self.parse_comparison(&name),
            _ => Err(self.invalid()),
        }
    }

    /// Parses a function call's `(field, target)` argument list starting at
    /// the opening `(`, dispatching on `name` to build the matching
    /// [`FilterFunction`].
    fn parse_function_call(
        &mut self,
        name: &str,
    ) -> Result<FilterFunction, QueryError> {
        self.expect(FilterToken::LParen)?;
        let Some(FilterToken::Ident(field_ident)) = self.next() else {
            return Err(self.invalid());
        };
        let field = FieldPath::parse(&field_ident)?;
        self.expect(FilterToken::Comma)?;
        let target = self.parse_literal_arg()?;
        self.expect(FilterToken::RParen)?;
        FilterFunction::build(name, field, target).ok_or_else(|| self.invalid())
    }

    /// Parses a `<field> <op> <value>` comparison starting after the
    /// already-consumed `field_ident` token.
    fn parse_comparison(
        &mut self,
        field_ident: &str,
    ) -> Result<FilterExpr, QueryError> {
        let Some(FilterToken::Op(op)) = self.next() else {
            return Err(self.invalid());
        };
        let field = FieldPath::parse(field_ident)?;
        let value = self.parse_literal_arg()?;
        Ok(FilterExpr::Comparison(ComparisonExpr::new(field, op, value)))
    }
}

/// Recognized filter function call.
///
/// Adding a function means adding a variant here, a name check in
/// [`Self::build`], and matching logic in [`Self::matches`].
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
    /// Builds the function call named `name`, if it names a known function.
    ///
    /// # Arguments
    ///
    /// - `name`: function name to match, case-insensitively.
    /// - `field`: already-parsed field path for the built call.
    /// - `target`: comparison or membership target for the built call.
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

    /// Whether `record` satisfies this function call.
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
/// - Lists match by exact value or tag prefix, such as `#book` matching
///   `#book/fiction`.
/// - Other field kinds fall back to substring containment on stringified
///   values.
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

/// Whether list element `item` matches `target`.
///
/// Matches by exact equality. Tag-like strings also match when `item` equals
/// `target` or nests under it as a sub-tag, such as `#book/fiction` under
/// `#book`.
fn tag_or_value_matches(item: &FieldValue, target: &FieldValue) -> bool {
    if fields_equal(item, target) {
        return true;
    }
    let (Some(item_str), Some(target_str)) = (item.as_str(), target.as_str())
    else {
        return false;
    };
    item_str.starts_with(target_str) || item_str.contains(target_str)
}

#[cfg(test)]
mod tests {
    use std::{fs, path::Path};

    use pretty_assertions::assert_eq;
    use rstest::rstest;

    use super::super::*;
    use crate::index::FileIndex;

    fn outcome_for_files(temp: &Path, files: &[(&str, &str)]) -> QueryOutcome {
        for (name, content) in files {
            fs::write(temp.join(name), content).expect("write note");
        }
        FileIndex::build(temp).expect("build index").query(&Source::All)
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

        assert_eq!(
            outcome.filter(expr),
            Err(QueryError::UnparsableFilterExpression {
                expr: expr.to_owned()
            })
        );
    }

    #[test]
    fn rejects_malformed_field_path_in_expression() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let outcome = rated_outcome(temp.path());

        assert_eq!(
            outcome.filter("file.bogus == 1"),
            Err(QueryError::UnknownFieldPath {
                path: "file.bogus".to_owned()
            })
        );
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
    fn boolean_and_or_not_combinations() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let outcome = rated_outcome(temp.path());

        let and_match = outcome
            .clone()
            .filter("rating > 5 AND status == \"done\"")
            .expect("valid filter");
        assert_eq!(names(&and_match), ["high"]);

        let or_match = outcome
            .clone()
            .filter("rating == 3 OR status == \"done\"")
            .expect("valid filter");
        assert_eq!(names(&or_match), ["high", "low", "unrated"]);

        let not_match =
            outcome.filter("NOT status == \"done\"").expect("valid filter");
        assert_eq!(names(&not_match), ["low"]);
    }

    #[test]
    fn nested_parentheses_override_precedence() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let outcome = rated_outcome(temp.path());

        let nested = outcome
            .filter("(rating > 5 OR status == \"draft\") AND NOT rating == 3")
            .expect("valid filter");

        assert_eq!(names(&nested), ["high"]);
    }

    #[test]
    fn logical_op_spellings_do_not_swallow_identifier_prefixes() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let outcome = outcome_for(temp.path(), "---\norder: 5\nandrew: 3\n---");

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

    #[test]
    fn contains_function_on_tags_and_string_fields() {
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

        let tag_match = outcome
            .clone()
            .filter("contains(tags, \"#book\")")
            .expect("valid filter");
        assert_eq!(names(&tag_match), ["book"]);

        let title_match =
            outcome.filter("contains(title, \"Async\")").expect("valid filter");
        assert_eq!(names(&title_match), ["article"]);
    }
}
