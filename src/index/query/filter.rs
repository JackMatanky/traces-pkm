//! Filter expression AST, tokenizer, and recursive descent parser.

use super::{
    IndexRecord, QueryError,
    field::FieldPath,
    operator::{CompareOp, LogicalOp},
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
    /// Function call: `contains(field, target)`.
    Function {
        name: String,
        field: FieldPath,
        args: Vec<FieldValue>,
    },
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
            Self::Function {
                name,
                field,
                args,
            } => {
                if name.eq_ignore_ascii_case("contains")
                    && let Some(target) = args.first()
                {
                    let field_val = record.resolve(field);
                    eval_contains(&field_val, target)
                } else {
                    false
                }
            }
            Self::Logical {
                op,
                exprs,
            } => op.eval(exprs, record),
        }
    }
}

/// Evaluates `contains(field_val, target)` logic.
fn eval_contains(field_val: &FieldValue, target: &FieldValue) -> bool {
    match field_val {
        FieldValue::List(items) => items.iter().any(|item| {
            fields_equal(item, target)
                || matches!(
                    item.as_str(),
                    Some(s) if matches!(
                        target.as_str(),
                        Some(t) if s.starts_with(t) || s.contains(t)
                    )
                )
        }),
        _ => match (field_val.as_str(), target.as_str()) {
            (Some(haystack), Some(needle)) => haystack.contains(needle),
            _ => false,
        },
    }
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

/// Tokenizes `expr` into a vector of [`FilterToken`]s.
#[expect(
    clippy::too_many_lines,
    reason = "tokenizer covers all filter token rules in one scanner loop"
)]
fn tokenize_filter_expr(expr: &str) -> Result<Vec<FilterToken>, QueryError> {
    let invalid = || QueryError::UnparsableFilterExpression {
        expr: expr.to_owned(),
    };
    let mut tokens = Vec::new();
    let mut chars = expr.char_indices().peekable();

    while let Some(&(i, c)) = chars.peek() {
        if c.is_whitespace() {
            chars.next();
            continue;
        }
        match c {
            '(' => {
                tokens.push(FilterToken::LParen);
                chars.next();
            }
            ')' => {
                tokens.push(FilterToken::RParen);
                chars.next();
            }
            ',' => {
                tokens.push(FilterToken::Comma);
                chars.next();
            }
            '"' => {
                chars.next();
                let mut s = String::new();
                let mut closed = false;
                while let Some((_, ch)) = chars.next() {
                    if ch == '\\' {
                        let escaped = chars
                            .next()
                            .map(|(_, esc_char)| esc_char)
                            .ok_or_else(invalid)?;
                        s.push(escaped);
                    } else if ch == '"' {
                        closed = true;
                        break;
                    } else {
                        s.push(ch);
                    }
                }
                if !closed {
                    return Err(invalid());
                }
                tokens.push(FilterToken::Literal(FieldValue::String(s)));
            }
            '=' | '!' | '>' | '<' | '&' | '|' => {
                let rest = &expr[i..];
                if let Some((op, stripped)) = CompareOp::strip_prefix(rest) {
                    tokens.push(FilterToken::Op(op));
                    let consumed = rest.len().saturating_sub(stripped.len());
                    for _ in 0..consumed {
                        chars.next();
                    }
                } else if rest.starts_with("&&") {
                    tokens.push(FilterToken::And);
                    chars.next();
                    chars.next();
                } else if rest.starts_with("||") {
                    tokens.push(FilterToken::Or);
                    chars.next();
                    chars.next();
                } else if rest.starts_with('!') {
                    tokens.push(FilterToken::Not);
                    chars.next();
                } else {
                    return Err(invalid());
                }
            }
            _ => {
                let start = i;
                while let Some(&(_, ch)) = chars.peek() {
                    if ch.is_whitespace()
                        || matches!(
                            ch,
                            '(' | ')'
                                | ','
                                | '"'
                                | '='
                                | '!'
                                | '>'
                                | '<'
                                | '&'
                                | '|'
                        )
                    {
                        break;
                    }
                    chars.next();
                }
                let end = chars.peek().map_or(expr.len(), |&(k, _)| k);
                let word = expr[start..end].trim();
                if word.is_empty() {
                    return Err(invalid());
                }
                if word.eq_ignore_ascii_case("AND") {
                    tokens.push(FilterToken::And);
                } else if word.eq_ignore_ascii_case("OR") {
                    tokens.push(FilterToken::Or);
                } else if word.eq_ignore_ascii_case("NOT") {
                    tokens.push(FilterToken::Not);
                } else if word == "true" {
                    tokens.push(FilterToken::Literal(FieldValue::Bool(true)));
                } else if word == "false" {
                    tokens.push(FilterToken::Literal(FieldValue::Bool(false)));
                } else if word == "null" || word == "Null" {
                    tokens.push(FilterToken::Literal(FieldValue::Null));
                } else if let Ok(num) = word.parse::<f64>() {
                    tokens.push(FilterToken::Literal(FieldValue::Number(num)));
                } else {
                    tokens.push(FilterToken::Ident(word.to_owned()));
                }
            }
        }
    }
    Ok(tokens)
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
        QueryError::UnparsableFilterExpression {
            expr: self.expr.to_owned(),
        }
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
        let left = self.parse_and()?;
        let mut arms = Vec::new();
        while self.peek() == Some(&FilterToken::Or) {
            self.next();
            let right = self.parse_and()?;
            if arms.is_empty() {
                arms.push(left.clone());
            }
            arms.push(right);
        }
        if arms.is_empty() {
            Ok(left)
        } else {
            Ok(FilterExpr::Logical {
                op: LogicalOp::Or,
                exprs: arms,
            })
        }
    }

    fn parse_and(&mut self) -> Result<FilterExpr, QueryError> {
        let left = self.parse_not()?;
        let mut arms = Vec::new();
        while self.peek() == Some(&FilterToken::And) {
            self.next();
            let right = self.parse_not()?;
            if arms.is_empty() {
                arms.push(left.clone());
            }
            arms.push(right);
        }
        if arms.is_empty() {
            Ok(left)
        } else {
            Ok(FilterExpr::Logical {
                op: LogicalOp::And,
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
            FilterToken::Ident(name) => {
                if self.peek() == Some(&FilterToken::LParen) {
                    self.next();
                    let Some(FilterToken::Ident(field_ident)) = self.next()
                    else {
                        return Err(self.invalid());
                    };
                    let field = FieldPath::parse(&field_ident)?;
                    let mut args = Vec::new();
                    if self.peek() != Some(&FilterToken::RParen) {
                        self.expect(FilterToken::Comma)?;
                        args.push(self.parse_literal_arg()?);
                    }
                    self.expect(FilterToken::RParen)?;
                    Ok(FilterExpr::Function {
                        name,
                        field,
                        args,
                    })
                } else if let Some(FilterToken::Op(op)) = self.peek().cloned() {
                    self.next();
                    let field = FieldPath::parse(&name)?;
                    let value = self.parse_literal_arg()?;
                    Ok(FilterExpr::Binary {
                        field,
                        op,
                        value,
                    })
                } else {
                    Err(self.invalid())
                }
            }
            _ => Err(self.invalid()),
        }
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
