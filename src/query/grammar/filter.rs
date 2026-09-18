//! Record filter expression DSL for `--where` queries.
//!
//! Parses field path accessors, comparison operators, boolean operators, and
//! `contains` calls over [`QueryRow`] rows.

use logos::{Lexer, Logos};
use miette::SourceSpan;

use super::{
    FieldPath,
    expr::{
        AtomParser, BooleanExpr, LogicalControl, LogicalOp, parse_boolean_expr,
    },
};
use crate::{
    LexError, LexTokenStream, LexedToken, NoteFieldValue, NoteFieldValueRef,
    TokenSpec, lexical_unquote,
    query::{
        QueryRow,
        error::{QueryBuilderError, QueryDialect, QuerySyntaxError},
        sort::TextShape,
        value::QueryFieldValueRef,
    },
};

/// Parsed filter expression AST used by [`crate::query::QuerySet::filter`].
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct FilterExpr(BooleanExpr<FilterAtom>);

impl FilterExpr {
    /// Parses filter syntax into a logical expression tree.
    ///
    /// # Errors
    ///
    /// - [`Syntax`] if the expression syntax is invalid.
    /// - [`FieldPath`] if any field path is invalid.
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

    /// Combines with logical AND, flattening nested `And` nodes.
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

    pub(crate) fn is_matching(&self, row: &QueryRow) -> bool {
        self.0.is_satisfied_by(|atom| atom.is_matching(row))
    }
}

/// Atomic predicate: either a comparison or a recognized function call.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum FilterAtom {
    Comparison(ComparisonExpr),
    Function(FilterFunction),
}

impl FilterAtom {
    fn is_matching(&self, row: &QueryRow) -> bool {
        match self {
            Self::Comparison(comparison) => comparison.is_matching(row),
            Self::Function(function) => function.is_matching(row),
        }
    }
}

/// Filter function with type-specific matching semantics.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum FilterFunction {
    /// `contains(field, target)` predicate.
    ///
    /// Lists match by exact value or tag prefix, such as `#book` matching
    /// `#book/fiction`; other field kinds fall back to substring containment.
    Contains {
        field: FieldPath,
        target: NoteFieldValue,
    },
}

impl FilterFunction {
    /// Accepts `contains` case-insensitively.
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

    fn is_matching(&self, row: &QueryRow) -> bool {
        match self {
            Self::Contains {
                field,
                target,
            } => row.resolve_ref(field).is_containing(target),
        }
    }
}

/// Field comparison against a literal value.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ComparisonExpr {
    field: FieldPath,
    op: CompareOp,
    literal: NoteFieldValue,
}

impl ComparisonExpr {
    /// Pre-classifies `literal`'s date/duration shape once, so `is_matching`
    /// never re-runs text classification per row.
    pub(super) fn new(
        field: FieldPath,
        op: CompareOp,
        literal: NoteFieldValue,
    ) -> Self {
        Self {
            field,
            op,
            literal: Self::classify_literal(literal),
        }
    }

    /// Returns `true` if `row`'s field at `self.field` satisfies `self.op`
    /// against `self.literal`.
    pub(super) fn is_matching(&self, row: &QueryRow) -> bool {
        self.op.is_satisfied_by(&row.resolve_ref(&self.field), &self.literal)
    }

    /// Promotes a filter literal's `String` payload to `Date`/`DateTime`/
    /// `Duration` when its text has that shape, once, at query-build time
    /// (not per row). Only `NoteFieldValue::String` needs inspection: the
    /// filter grammar's `Literal` token never produces `Date`/`DateTime`/
    /// `Duration`/`Link`/`List`/`Object` directly (`Null`/`Bool`/`Number`/
    /// `String` are its only literal shapes). Reuses [`TextShape::classify`]
    /// (the same heuristic `SortKey::from_text` uses), so filter
    /// literals and sort-key text classify identically, not via a second
    /// hand-rolled copy.
    fn classify_literal(literal: NoteFieldValue) -> NoteFieldValue {
        let NoteFieldValue::String(text) = &literal else {
            return literal;
        };
        match TextShape::classify(text) {
            TextShape::DateTime(value) => NoteFieldValue::DateTime(value),
            TextShape::Date(value) => NoteFieldValue::Date(value),
            TextShape::Duration(value) => NoteFieldValue::Duration(value),
            TextShape::Plain => literal,
        }
    }
}

/// Comparison operator parsed from filter syntax.
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
    /// `Eq`/`Ne` use `is_equal_to_literal`'s existing cross-kind coercion
    /// (e.g. a `Date` field against a `DateTime` literal at midnight UTC).
    /// `Lt`/`Le`/`Gt`/`Ge` use [`NoteFieldValueRef::compare`]'s full rank
    /// order directly; a `Null` on either side never satisfies an ordering
    /// comparison (matches today's behavior: a missing field never passes a
    /// numeric/date threshold).
    pub(super) fn is_satisfied_by(
        self,
        field: &QueryFieldValueRef<'_>,
        literal: &NoteFieldValue,
    ) -> bool {
        if matches!(self, Self::Eq | Self::Ne) {
            let equal = field.is_equal_to_literal(literal);
            return if matches!(self, Self::Eq) {
                equal
            } else {
                !equal
            };
        }
        let Some(field_ref) = field.as_note_ref() else {
            return false;
        };
        if matches!(field_ref, NoteFieldValueRef::Null)
            || matches!(literal, NoteFieldValue::Null)
        {
            return false;
        }
        match field_ref.compare(&literal.as_ref()) {
            std::cmp::Ordering::Less => matches!(self, Self::Lt | Self::Le),
            std::cmp::Ordering::Equal => matches!(self, Self::Le | Self::Ge),
            std::cmp::Ordering::Greater => matches!(self, Self::Gt | Self::Ge),
        }
    }
}

impl TryFrom<&str> for CompareOp {
    type Error = ();

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

/// Filter grammar adapter for [`parse_boolean_expr`].
struct FilterGrammar;

impl FilterGrammar {
    /// Parses a literal where a comparison or function argument requires a
    /// value.
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

    /// Parses a call argument list after the function name.
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

    /// Parses a `<field> <op> <value>` comparison after the field token.
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

    /// Parses a function call when an identifier is followed by `(`; otherwise
    /// parses a comparison.
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

/// Filter expression lexer tokens.
#[derive(Clone, Debug, PartialEq, Logos)]
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
    #[regex(r#""([^"\\]|\\.)*"|'([^'\\]|\\.)*'"#, string_callback)]
    #[token("true", |_| NoteFieldValue::Bool(true), priority = 3)]
    #[token("false", |_| NoteFieldValue::Bool(false), priority = 3)]
    #[token("null", |_| NoteFieldValue::Null, priority = 3)]
    #[token("Null", |_| NoteFieldValue::Null, priority = 3)]
    Literal(NoteFieldValue),
    #[regex(r#"[^\s()'",=!<>&|]+"#, |lex| lex.slice().to_owned())]
    Ident(String),
}

/// Unescapes a quoted string literal into a [`NoteFieldValue::String`].
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
        IndexerService,
        query::{QueryError, *},
    };

    fn outcome_for_files(_temp: &Path, files: &[(&str, &str)]) -> QuerySet {
        let index = crate::build_test_index(files);
        QueryService::new("class")
            .run(&index, QueryBuilder::pages(SourceSelector::All))
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
            .map(|row| row.file().name().as_str().to_owned())
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
                Err(QueryError::Builder(QueryBuilderError::Syntax(_)))
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
                outcome.filter("file.zzzz == 1"),
                Err(QueryError::Builder(QueryBuilderError::FieldPath(
                    FieldPathError::new("file.zzzz", None)
                )))
            );
        }

        #[test]
        fn rejects_malformed_field_path_in_function() {
            assert_eq!(
                FilterExpr::parse("contains(file.zzzz, \"x\")"),
                Err(QueryBuilderError::FieldPath(FieldPathError::new(
                    "file.zzzz",
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
        fn cross_kind_ordering_follows_canonical_rank() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let outcome = rated_outcome(temp.path());

            // Text (rank 5) is greater than Number 5 (rank 2).
            let filtered =
                outcome.clone().filter("status > 5").expect("valid filter");
            assert_eq!(names(&filtered), ["high", "low", "unrated"]);

            // Number 5 is not greater than Text, so status < 5 matches nothing.
            let below = outcome.filter("status < 5").expect("valid filter");
            assert!(below.is_empty());
        }

        #[test]
        fn mixed_kind_column_ordering_matches_sort_order() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let outcome = outcome_for_files(temp.path(), &[
                ("num_low.md", "---\nrating: 3\n---"),
                ("num_high.md", "---\nrating: 7\n---"),
                ("text.md", "---\nrating: gold\n---"),
            ]);

            // In canonical rank: Number < Text.
            // rating > 5 matches num_high (7 > 5) and text ("gold" > 5 by
            // rank).
            let filtered =
                outcome.clone().filter("rating > 5").expect("valid filter");
            assert_eq!(names(&filtered), ["num_high", "text"]);

            // sort rating asc orders: num_low (3), num_high (7), text ("gold").
            let sorted = outcome.sort("rating", false).expect("valid sort");
            assert_eq!(names(&sorted), ["num_low", "num_high", "text"]);
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
        fn equal_null_matches_rows_with_a_null_field() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let outcome = rated_outcome(temp.path());

            let filtered =
                outcome.filter("rating == null").expect("valid filter");

            assert_eq!(names(&filtered), ["unrated"]);
        }

        #[test]
        fn not_equal_null_matches_rows_with_a_non_null_field() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let outcome = rated_outcome(temp.path());

            let filtered =
                outcome.filter("rating != null").expect("valid filter");

            assert_eq!(names(&filtered), ["high", "low"]);
        }

        #[test]
        fn r_where_alias_filters_records_identically_to_filter() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let outcome = rated_outcome(temp.path());

            let filtered = outcome.r#where("rating >= 7").expect("valid where");

            assert_eq!(names(&filtered), ["high"]);
        }

        #[test]
        fn evaluates_duration_comparisons_temporally() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let outcome = outcome_for_files(temp.path(), &[
                ("a.md", "---\nspent: 1h\n---"),
                ("b.md", "---\nspent: 30m\n---"),
            ]);
            let filtered =
                outcome.filter("spent > \"30m\"").expect("valid filter");
            assert_eq!(names(&filtered), ["a"]);
        }

        #[test]
        fn matches_duration_equality_across_differing_spellings() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let outcome = outcome_for_files(temp.path(), &[
                ("a.md", "---\nspent: 90m\n---"),
                ("b.md", "---\nspent: 45m\n---"),
            ]);
            let filtered =
                outcome.filter("spent == \"1h 30m\"").expect("valid filter");
            assert_eq!(names(&filtered), ["a"]);
        }

        #[test]
        fn evaluates_date_only_literal_at_midnight_utc() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let p1 = temp.path().join("jan1.md");
            let p2 = temp.path().join("jan2.md");
            fs::write(&p1, "# Jan 1\n").expect("write jan1.md");
            fs::write(&p2, "# Jan 2\n").expect("write jan2.md");

            let t1 = std::time::UNIX_EPOCH
                + std::time::Duration::from_mins(29_454_630);
            let t2 = std::time::UNIX_EPOCH
                + std::time::Duration::from_hours(490_928);

            let f1 =
                fs::File::options().write(true).open(&p1).expect("open p1");
            f1.set_times(std::fs::FileTimes::new().set_modified(t1))
                .expect("set mtime p1");
            drop(f1);

            let f2 =
                fs::File::options().write(true).open(&p2).expect("open p2");
            f2.set_times(std::fs::FileTimes::new().set_modified(t2))
                .expect("set mtime p2");
            drop(f2);

            let index = Arc::new(
                IndexerService::new(temp.path()).build().expect("build index"),
            );
            let outcome = QueryService::new("class")
                .run(&index, QueryBuilder::pages(SourceSelector::All));

            let filtered = outcome
                .filter(
                    "file.mtime >= \"2026-01-01\" and file.mtime < \
                     \"2026-01-02\"",
                )
                .expect("valid filter");

            assert_eq!(names(&filtered), ["jan1"]);
        }

        #[test]
        fn matches_calendar_days_via_mdate() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let p1 = temp.path().join("jan1.md");
            let p2 = temp.path().join("jan2.md");
            fs::write(&p1, "# Jan 1\n").expect("write jan1.md");
            fs::write(&p2, "# Jan 2\n").expect("write jan2.md");

            let t1 = std::time::UNIX_EPOCH
                + std::time::Duration::from_mins(29_454_630);
            let t2 = std::time::UNIX_EPOCH
                + std::time::Duration::from_hours(490_928);

            let f1 =
                fs::File::options().write(true).open(&p1).expect("open p1");
            f1.set_times(std::fs::FileTimes::new().set_modified(t1))
                .expect("set mtime p1");
            drop(f1);

            let f2 =
                fs::File::options().write(true).open(&p2).expect("open p2");
            f2.set_times(std::fs::FileTimes::new().set_modified(t2))
                .expect("set mtime p2");
            drop(f2);

            let index = Arc::new(
                IndexerService::new(temp.path()).build().expect("build index"),
            );
            let outcome = QueryService::new("class")
                .run(&index, QueryBuilder::pages(SourceSelector::All));

            let filtered = outcome
                .filter("file.mdate == \"2026-01-01\"")
                .expect("valid filter");

            assert_eq!(names(&filtered), ["jan1"]);
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

            // Regression: fields named `order`/`andrew` must stay whole
            // identifiers, not logical-op prefixes.
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

    mod classify_literal {
        use super::super::{ComparisonExpr, NoteFieldValue};

        #[test]
        fn promotes_iso_date_string_to_date_value() {
            let lit = NoteFieldValue::String("2026-07-29".to_owned());
            let classified = ComparisonExpr::classify_literal(lit);
            assert!(matches!(classified, NoteFieldValue::Date(_)));
        }

        #[test]
        fn promotes_iso_datetime_string_to_datetime_value() {
            let lit = NoteFieldValue::String("2026-07-29T14:30:00".to_owned());
            let classified = ComparisonExpr::classify_literal(lit);
            assert!(matches!(classified, NoteFieldValue::DateTime(_)));
        }

        #[test]
        fn promotes_duration_string_to_duration_value() {
            let lit = NoteFieldValue::String("1h30m".to_owned());
            let classified = ComparisonExpr::classify_literal(lit);
            assert!(matches!(classified, NoteFieldValue::Duration(_)));
        }

        #[test]
        fn preserves_plain_string_literal() {
            let lit = NoteFieldValue::String("active".to_owned());
            let classified = ComparisonExpr::classify_literal(lit);
            assert_eq!(classified, NoteFieldValue::String("active".to_owned()));
        }

        #[test]
        fn preserves_non_string_literals_unmodified() {
            assert_eq!(
                ComparisonExpr::classify_literal(NoteFieldValue::Number(42.0)),
                NoteFieldValue::Number(42.0)
            );
            assert_eq!(
                ComparisonExpr::classify_literal(NoteFieldValue::Bool(true)),
                NoteFieldValue::Bool(true)
            );
            assert_eq!(
                ComparisonExpr::classify_literal(NoteFieldValue::Null),
                NoteFieldValue::Null
            );
        }
    }
}
