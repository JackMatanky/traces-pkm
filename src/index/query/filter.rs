//! Filter expression AST, tokenizer, and recursive descent parser.

use std::{iter::Peekable, str::CharIndices};

use super::{
    IndexRecord, QueryError,
    field::FieldPath,
    operators::{CompareOp, LogicalOp},
    sort::fields_equal,
};
use crate::note::FieldValue;

/// A parsed `.filter()` expression AST supporting comparisons, functions
/// (e.g. `contains(tags, "#book")`), boolean logic (`AND`, `OR`, `NOT`), and
/// nested parentheses.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum FilterExpr {
    /// `<field> <op> <value>` comparison.
    Binary {
        field: FieldPath,
        op: CompareOp,
        value: FieldValue,
    },
    /// A recognized function call, e.g. `contains(tags, "#book")`.
    Function(FilterFunction),
    /// Logical `AND`, `OR`, or `NOT` combination of expressions.
    Logical {
        op: LogicalOp,
        exprs: Vec<FilterExpr>,
    },
}

impl FilterExpr {
    /// Parses a filter expression string into a [`FilterExpr`] AST.
    ///
    /// Supports:
    /// - **Comparisons**: `<field> <op> <value>` (`==`, `!=`, `>=`, `<=`, `>`,
    ///   `<`)
    /// - **Functions**: `contains(field, value)`
    /// - **Boolean Logic**: `AND` / `and` / `&&`, `OR` / `or` / `||`, `NOT` /
    ///   `not` / `!`
    /// - **Parentheses**: `( ... )`
    ///
    /// # Errors
    ///
    /// - [`QueryError::UnparsableFilterExpression`] if `expr` is malformed
    /// - [`QueryError::UnknownFieldPath`] if a field path is malformed
    pub(super) fn parse(expr: &str) -> Result<Self, QueryError> {
        let tokens = tokenize_filter_expr(expr)?;
        let mut parser = FilterParser::new(expr, tokens);
        let ast = parser.parse_expr()?;
        if parser.peek().is_some() {
            return Err(QueryError::UnparsableFilterExpression {
                expr: expr.to_owned(),
            });
        }
        Ok(ast)
    }

    /// Whether `record` satisfies this expression.
    pub(super) fn matches(&self, record: &IndexRecord) -> bool {
        match self {
            Self::Binary {
                field,
                op,
                value,
            } => op.is_satisfied_by(&record.resolve(field), value),
            Self::Function(function) => function.matches(record),
            Self::Logical {
                op,
                exprs,
            } => op.eval(exprs, record),
        }
    }
}

/// A recognized filter function call, e.g. `contains(tags, "#book")`.
///
/// Adding a function means adding a variant here, a name check in
/// [`Self::build`], and matching logic in [`Self::matches`].
#[derive(Clone, Debug, PartialEq)]
pub(super) enum FilterFunction {
    /// `contains(field, target)`: list membership (with tag-prefix
    /// matching, e.g. `#book` matching `#book/fiction`) or string substring
    /// containment.
    Contains {
        field: FieldPath,
        target: FieldValue,
    },
}

impl FilterFunction {
    /// Builds the function call named `name`, if `name` names a known
    /// function.
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

/// Evaluates `contains(field_val, target)` logic.
///
/// Lists match by equality or tag-prefix (e.g. `#book` matching
/// `#book/fiction`); everything else falls back to substring containment on
/// stringified values.
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

/// Whether list element `item` matches `target`: exact equality, or (for
/// tag-like strings) `item` equals `target` or nests under it as a sub-tag
/// (e.g. `#book/fiction` under `#book`).
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

/// Tokens parsed from a filter expression.
#[derive(Clone, Debug, PartialEq)]
enum FilterToken {
    LParen,
    RParen,
    Comma,
    And,
    Or,
    Not,
    Op(CompareOp),
    Literal(FieldValue),
    Ident(String),
}

/// Peekable character stream with byte offsets, threaded through the
/// tokenizer's per-token-kind scanners.
type Chars<'a> = Peekable<CharIndices<'a>>;

/// Builds the "unparsable filter expression" error for the full `expr`.
fn unparsable(expr: &str) -> QueryError {
    QueryError::UnparsableFilterExpression {
        expr: expr.to_owned(),
    }
}

/// Tokenizes `expr` into a vector of [`FilterToken`]s.
fn tokenize_filter_expr(expr: &str) -> Result<Vec<FilterToken>, QueryError> {
    let mut tokens = Vec::new();
    let mut chars = expr.char_indices().peekable();

    while let Some(&(i, c)) = chars.peek() {
        if c.is_whitespace() {
            chars.next();
            continue;
        }
        let token = match c {
            '(' => {
                chars.next();
                FilterToken::LParen
            }
            ')' => {
                chars.next();
                FilterToken::RParen
            }
            ',' => {
                chars.next();
                FilterToken::Comma
            }
            '"' => scan_string_literal(expr, &mut chars)?,
            '=' | '!' | '>' | '<' | '&' | '|' => {
                scan_operator(expr, i, &mut chars)?
            }
            _ => scan_word(expr, i, &mut chars)?,
        };
        tokens.push(token);
    }
    Ok(tokens)
}

/// Scans a double-quoted string literal starting at the opening `"`.
///
/// Supports `\`-escaped characters; consumes through the closing quote.
fn scan_string_literal(
    expr: &str,
    chars: &mut Chars<'_>,
) -> Result<FilterToken, QueryError> {
    chars.next(); // Consume the opening quote.
    let mut s = String::new();
    while let Some((_, ch)) = chars.next() {
        match ch {
            '\\' => {
                let escaped = chars
                    .next()
                    .map(|(_, esc)| esc)
                    .ok_or_else(|| unparsable(expr))?;
                s.push(escaped);
            }
            '"' => return Ok(FilterToken::Literal(FieldValue::String(s))),
            _ => s.push(ch),
        }
    }
    Err(unparsable(expr)) // Unterminated string.
}

/// Scans an operator or logical symbol (`==`, `!=`, `&&`, `||`, `!`, ...)
/// starting at byte offset `i`.
fn scan_operator(
    expr: &str,
    i: usize,
    chars: &mut Chars<'_>,
) -> Result<FilterToken, QueryError> {
    let rest = &expr[i..];
    if let Some((op, stripped)) = CompareOp::strip_prefix(rest) {
        let consumed = rest.len().saturating_sub(stripped.len());
        for _ in 0..consumed {
            chars.next();
        }
        return Ok(FilterToken::Op(op));
    }
    if rest.starts_with("&&") {
        chars.next();
        chars.next();
        return Ok(FilterToken::And);
    }
    if rest.starts_with("||") {
        chars.next();
        chars.next();
        return Ok(FilterToken::Or);
    }
    if rest.starts_with('!') {
        chars.next();
        return Ok(FilterToken::Not);
    }
    Err(unparsable(expr))
}

/// Scans a bare word starting at byte offset `start`: a boolean-logic
/// keyword (`AND`/`OR`/`NOT`), a literal (`true`/`false`/`null`/a number),
/// or a field identifier.
fn scan_word(
    expr: &str,
    start: usize,
    chars: &mut Chars<'_>,
) -> Result<FilterToken, QueryError> {
    while let Some(&(_, ch)) = chars.peek() {
        if ch.is_whitespace()
            || matches!(
                ch,
                '(' | ')' | ',' | '"' | '=' | '!' | '>' | '<' | '&' | '|'
            )
        {
            break;
        }
        chars.next();
    }
    let end = chars.peek().map_or(expr.len(), |&(k, _)| k);
    let word = expr[start..end].trim();
    if word.is_empty() {
        return Err(unparsable(expr));
    }
    Ok(if word.eq_ignore_ascii_case("AND") {
        FilterToken::And
    } else if word.eq_ignore_ascii_case("OR") {
        FilterToken::Or
    } else if word.eq_ignore_ascii_case("NOT") {
        FilterToken::Not
    } else if word == "true" {
        FilterToken::Literal(FieldValue::Bool(true))
    } else if word == "false" {
        FilterToken::Literal(FieldValue::Bool(false))
    } else if word == "null" || word == "Null" {
        FilterToken::Literal(FieldValue::Null)
    } else if let Ok(num) = word.parse::<f64>() {
        FilterToken::Literal(FieldValue::Number(num))
    } else {
        FilterToken::Ident(word.to_owned())
    })
}

/// Recursive descent parser for [`FilterExpr`] ASTs.
struct FilterParser<'a> {
    expr: &'a str,
    tokens: Vec<FilterToken>,
    pos: usize,
}

impl<'a> FilterParser<'a> {
    fn new(expr: &'a str, tokens: Vec<FilterToken>) -> Self {
        Self {
            expr,
            tokens,
            pos: 0,
        }
    }

    fn invalid(&self) -> QueryError {
        unparsable(self.expr)
    }

    fn peek(&self) -> Option<&FilterToken> {
        self.tokens.get(self.pos)
    }

    fn next(&mut self) -> Option<FilterToken> {
        let tok = self.tokens.get(self.pos).cloned()?;
        self.pos = self.pos.saturating_add(1);
        Some(tok)
    }

    fn expect(&mut self, expected: FilterToken) -> Result<(), QueryError> {
        if self.next() == Some(expected) {
            Ok(())
        } else {
            Err(self.invalid())
        }
    }

    fn parse_expr(&mut self) -> Result<FilterExpr, QueryError> {
        self.parse_or()
    }

    fn parse_or(&mut self) -> Result<FilterExpr, QueryError> {
        self.parse_logical_chain(
            &FilterToken::Or,
            LogicalOp::Or,
            Self::parse_and,
        )
    }

    fn parse_and(&mut self) -> Result<FilterExpr, QueryError> {
        self.parse_logical_chain(
            &FilterToken::And,
            LogicalOp::And,
            Self::parse_not,
        )
    }

    /// Parses a left-associative chain of `term`s separated by `sep`,
    /// combining more than one term into a [`FilterExpr::Logical`] under
    /// `op`. A lone term passes through unwrapped.
    fn parse_logical_chain(
        &mut self,
        sep: &FilterToken,
        op: LogicalOp,
        mut term: impl FnMut(&mut Self) -> Result<FilterExpr, QueryError>,
    ) -> Result<FilterExpr, QueryError> {
        let left = term(self)?;
        let mut arms = Vec::new();
        while self.peek() == Some(sep) {
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
            Ok(FilterExpr::Logical {
                op,
                exprs: arms,
            })
        }
    }

    fn parse_not(&mut self) -> Result<FilterExpr, QueryError> {
        if self.peek() == Some(&FilterToken::Not) {
            self.next();
            let expr = self.parse_not()?;
            Ok(FilterExpr::Logical {
                op: LogicalOp::Not,
                exprs: vec![expr],
            })
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
        Ok(FilterExpr::Binary {
            field,
            op,
            value,
        })
    }
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
