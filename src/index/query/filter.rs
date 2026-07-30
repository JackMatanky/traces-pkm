//! Filter expression AST, tokenizer, and recursive descent parser.

use super::{
    FieldPath, IndexRecord, QueryError,
    operators::{CompareOp, LogicalOp, fields_equal},
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
