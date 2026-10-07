//! Record filter expression language and abstract syntax tree for `--where`
//! queries.
//!
//! This module parses and evaluates row-level filter expressions. Expressions
//! support:
//! - Dotted and bare field path lookups resolved via [`FieldPath`].
//! - Comparison operators (`==`, `!=`, `<`, `<=`, `>`, `>=`) with type
//!   coercion.
//! - Function calls such as `contains(field, target)` with tag prefix matching.
//! - Value expressions: date ± duration, date − date (duration result),
//!   duration ± duration and duration × number, plus `dur`, `date_add`,
//!   `date_diff`, and `date_component` calls. Arithmetic groups left-to-right
//!   without precedence; null and invalid row values do not satisfy ordered
//!   comparisons. Static invalid function arguments fail at parse time.
//! - Boolean combinators (`and`, `or`, `not`, parentheses) parsed via the
//!   shared boolean expression grammar.
use logos::{Lexer, Logos};

use super::{
    FieldPath,
    expr::{
        AtomParser, BooleanExpr, LogicalControl, LogicalOp, parse_boolean_expr,
    },
};
use crate::{
    DateTimeValue, DurationSeconds, DurationUnit, DurationValue, LexError,
    NoteFieldValue, NoteFieldValueRef, Spanned, SpannedTokenStream, TokenSpec,
    date::{DateDiff, DateError, DatePoint, Precision},
    lexical_unquote,
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
        let tokens =
            SpannedTokenStream::<FilterToken>::tokenize_with(input, |token| {
                let (value, span) = token.into_parts();
                match value {
                    FilterToken::Ident(word) => match word.parse::<f64>() {
                        Ok(number) if number.is_finite() => Ok(Spanned::new(
                            FilterToken::Literal(NoteFieldValue::Number(
                                number,
                            )),
                            span,
                        )),
                        Ok(_) => Err(LexError::UnexpectedToken {
                            span: span.to_range(),
                            found: "NaN or infinity".to_owned(),
                            expected: "a finite numeric literal",
                        }),
                        Err(_) => {
                            if let Some(Ok(dv)) = DurationValue::classify(&word)
                            {
                                Ok(Spanned::new(
                                    FilterToken::Literal(
                                        NoteFieldValue::Duration(dv),
                                    ),
                                    span,
                                ))
                            } else {
                                Ok(Spanned::new(FilterToken::Ident(word), span))
                            }
                        }
                    },
                    other => Ok(Spanned::new(other, span)),
                }
            })
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

/// Comparison expression between two value expressions.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ComparisonExpr {
    lhs: ValueExpr,
    op: CompareOp,
    rhs: ValueExpr,
}

impl ComparisonExpr {
    pub(super) fn new(lhs: ValueExpr, op: CompareOp, rhs: ValueExpr) -> Self {
        Self {
            lhs,
            op,
            rhs,
        }
    }

    /// Returns `true` if `row` satisfies `self.op` between `self.lhs` and
    /// `self.rhs`.
    pub(super) fn is_matching(&self, row: &QueryRow) -> bool {
        if let (ValueExpr::Field(path), ValueExpr::Literal(literal)) =
            (&self.lhs, &self.rhs)
        {
            return self.op.is_satisfied_by(&row.resolve_ref(path), literal);
        }
        let Some(lhs_val) = self.lhs.evaluate(row) else {
            return false;
        };
        let Some(rhs_val) = self.rhs.evaluate(row) else {
            return false;
        };
        self.op.is_satisfied_by_values(&lhs_val, &rhs_val)
    }

    /// Promotes a filter literal's `String` payload to `Date`/`DateTime`/
    /// `Duration` when its text has that shape, once, at query-build time (not
    /// per row). Only `NoteFieldValue::String` needs inspection: the filter
    /// grammar's `Literal` token never produces `Date`/`DateTime`/
    /// `Duration`/`Link`/`List`/`Object` directly (`Null`/`Bool`/`Number`/
    /// `String` are its only literal shapes). Reuses [`TextShape::classify`]
    /// (the same heuristic `SortKey::from_text` uses), so filter literals and
    /// sort-key text classify identically, not via a second hand-rolled copy.
    pub(super) fn classify_literal(literal: NoteFieldValue) -> NoteFieldValue {
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

/// A value expression in a query filter.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum ValueExpr {
    /// Field path lookup on a query row.
    Field(FieldPath),
    /// A literal value.
    Literal(NoteFieldValue),
    /// A value-returning function call (e.g. `dur`, `date_add`, `date_diff`,
    /// `date_component`).
    Call {
        name: String,
        args: Vec<Self>,
    },
    /// An arithmetic binary operation evaluated left-to-right.
    Binary {
        lhs: Box<Self>,
        op: ArithOp,
        rhs: Box<Self>,
    },
}

/// Binary arithmetic operator.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum ArithOp {
    /// `+`
    Add,
    /// `-`
    Sub,
    /// `*`
    Mul,
}

impl ValueExpr {
    /// Evaluates a value expression; an invalid row value cannot satisfy a
    /// comparison.
    fn evaluate(&self, row: &QueryRow) -> Option<NoteFieldValue> {
        match self {
            Self::Field(path) => Some(row.resolve_ref(path).to_owned_value()),
            Self::Literal(value) => Some(value.clone()),
            Self::Call {
                name,
                args,
            } => {
                let values = args
                    .iter()
                    .map(|arg| arg.evaluate(row))
                    .collect::<Option<Vec<_>>>()?;
                evaluate_registry_call(name, &values)
            }
            Self::Binary {
                lhs,
                op,
                rhs,
            } => evaluate_binary(lhs.evaluate(row)?, *op, rhs.evaluate(row)?),
        }
    }
}

fn date_point(value: &NoteFieldValue) -> Option<DatePoint> {
    match value {
        NoteFieldValue::Date(date) => Some(DatePoint::new(
            date.into_inner().and_hms_opt(0, 0, 0)?,
            DateTimeValue::from(*date).into_inner(),
            Precision::Date,
        )),
        NoteFieldValue::DateTime(datetime) => Some(DatePoint::new(
            datetime.wall_or_utc(),
            datetime.into_inner(),
            Precision::DateTime,
        )),
        _ => None,
    }
}

#[expect(
    clippy::arithmetic_side_effects,
    reason = "duration operators preserve overflow as non-finite; consuming \
              conversions reject it"
)]
fn evaluate_binary(
    left: NoteFieldValue,
    op: ArithOp,
    right: NoteFieldValue,
) -> Option<NoteFieldValue> {
    if matches!(left, NoteFieldValue::Null)
        || matches!(right, NoteFieldValue::Null)
    {
        return Some(NoteFieldValue::Null);
    }
    match (left, op, right) {
        (
            NoteFieldValue::Date(date),
            ArithOp::Add,
            NoteFieldValue::Duration(duration),
        )
        | (
            NoteFieldValue::Duration(duration),
            ArithOp::Add,
            NoteFieldValue::Date(date),
        ) => Some(NoteFieldValue::Date(date.apply(&duration).ok()?)),
        (
            NoteFieldValue::DateTime(datetime),
            ArithOp::Add,
            NoteFieldValue::Duration(duration),
        )
        | (
            NoteFieldValue::Duration(duration),
            ArithOp::Add,
            NoteFieldValue::DateTime(datetime),
        ) => Some(NoteFieldValue::DateTime(datetime.apply(&duration).ok()?)),
        (
            NoteFieldValue::Date(date),
            ArithOp::Sub,
            NoteFieldValue::Duration(duration),
        ) => Some(NoteFieldValue::Date(date.apply(&(duration * -1.0)).ok()?)),
        (
            NoteFieldValue::DateTime(datetime),
            ArithOp::Sub,
            NoteFieldValue::Duration(duration),
        ) => Some(NoteFieldValue::DateTime(
            datetime.apply(&(duration * -1.0)).ok()?,
        )),
        (
            date1 @ (NoteFieldValue::Date(_) | NoteFieldValue::DateTime(_)),
            ArithOp::Sub,
            date2 @ (NoteFieldValue::Date(_) | NoteFieldValue::DateTime(_)),
        ) => {
            let diff = date_point(&date2)?
                .diff(date_point(&date1)?, DurationUnit::Second)
                .ok()?;
            let seconds = match diff {
                DateDiff::Whole(n) => num_traits::ToPrimitive::to_f64(&n)?,
                DateDiff::Exact(n) => n,
            };
            Some(NoteFieldValue::Duration(DurationValue::from_seconds(
                DurationSeconds::try_from(seconds).ok()?,
            )))
        }
        (
            NoteFieldValue::Duration(a),
            ArithOp::Add,
            NoteFieldValue::Duration(b),
        ) => Some(NoteFieldValue::Duration(a + b)),
        (
            NoteFieldValue::Duration(a),
            ArithOp::Sub,
            NoteFieldValue::Duration(b),
        ) => Some(NoteFieldValue::Duration(a - b)),
        (
            NoteFieldValue::Duration(duration),
            ArithOp::Mul,
            NoteFieldValue::Number(n),
        )
        | (
            NoteFieldValue::Number(n),
            ArithOp::Mul,
            NoteFieldValue::Duration(duration),
        ) => Some(NoteFieldValue::Duration(duration * n)),
        _ => Some(NoteFieldValue::Null),
    }
}

#[expect(
    clippy::too_many_lines,
    clippy::large_stack_frames,
    reason = "evaluates all registered temporal functions across query rows"
)]
fn evaluate_registry_call(
    name: &str,
    args: &[NoteFieldValue],
) -> Option<NoteFieldValue> {
    if name.eq_ignore_ascii_case("dur") {
        return match args {
            [NoteFieldValue::Duration(duration)] => {
                Some(NoteFieldValue::Duration(duration.clone()))
            }
            [NoteFieldValue::String(text)] => {
                Some(NoteFieldValue::Duration(DurationValue::parse(text).ok()?))
            }
            [_] => Some(NoteFieldValue::Null),
            _ => None,
        };
    }
    if name.eq_ignore_ascii_case("date_add") {
        return match args {
            [date, NoteFieldValue::Duration(duration)] => evaluate_binary(
                date.clone(),
                ArithOp::Add,
                NoteFieldValue::Duration(duration.clone()),
            ),
            [date, NoteFieldValue::String(text)] => evaluate_binary(
                date.clone(),
                ArithOp::Add,
                NoteFieldValue::Duration(DurationValue::parse(text).ok()?),
            ),
            [date, NoteFieldValue::Number(n), NoteFieldValue::String(unit)] => {
                use num_traits::ToPrimitive as _;
                let unit = DurationUnit::parse(unit)?;
                let n = (n.fract() == 0.0).then(|| n.to_i64()).flatten()?;
                let shifted = date_point(date)?.shift(n, unit).ok()?;
                match date {
                    NoteFieldValue::Date(_) => Some(NoteFieldValue::Date(
                        crate::DateValue::from(shifted.wall.date()),
                    )),
                    NoteFieldValue::DateTime(_) => {
                        Some(NoteFieldValue::DateTime(DateTimeValue::from(
                            shifted.instant,
                        )))
                    }
                    _ => Some(NoteFieldValue::Null),
                }
            }
            _ => None,
        };
    }
    if name.eq_ignore_ascii_case("date_diff") {
        let (date1, date2, unit) = match args {
            [date1, date2] => (date1, date2, DurationUnit::Day),
            [date1, date2, NoteFieldValue::String(unit)] => {
                (date1, date2, DurationUnit::parse(unit)?)
            }
            _ => return None,
        };
        if matches!(date1, NoteFieldValue::Null)
            || matches!(date2, NoteFieldValue::Null)
        {
            return Some(NoteFieldValue::Null);
        }
        let diff = date_point(date1)?.diff(date_point(date2)?, unit).ok()?;
        return Some(NoteFieldValue::Number(match diff {
            DateDiff::Whole(n) => num_traits::ToPrimitive::to_f64(&n)?,
            DateDiff::Exact(n) => n,
        }));
    }
    if name.eq_ignore_ascii_case("date_component") {
        return evaluate_date_component(args);
    }
    if name.eq_ignore_ascii_case("sow") {
        return date_boundary(args, |pt| pt.week_boundary(false));
    }
    if name.eq_ignore_ascii_case("eow") {
        return date_boundary(args, |pt| pt.week_boundary(true));
    }
    if name.eq_ignore_ascii_case("som")
        || name.eq_ignore_ascii_case("start_of_month")
    {
        return date_boundary(args, |pt| pt.month_boundary(false));
    }
    if name.eq_ignore_ascii_case("eom")
        || name.eq_ignore_ascii_case("end_of_month")
    {
        return date_boundary(args, |pt| pt.month_boundary(true));
    }
    if name.eq_ignore_ascii_case("soy") {
        return date_boundary(args, |pt| pt.year_boundary(false));
    }
    if name.eq_ignore_ascii_case("eoy") {
        return date_boundary(args, |pt| pt.year_boundary(true));
    }
    if name.eq_ignore_ascii_case("date") {
        return match args {
            [NoteFieldValue::Date(d)]
            | [NoteFieldValue::Date(d), NoteFieldValue::String(_)] => {
                Some(NoteFieldValue::Date(*d))
            }
            [NoteFieldValue::DateTime(dt)]
            | [NoteFieldValue::DateTime(dt), NoteFieldValue::String(_)] => {
                Some(NoteFieldValue::DateTime(*dt))
            }
            [NoteFieldValue::String(s)] => {
                let rec = crate::DateValue::classify(s)?.ok()?;
                Some(if rec.precision.has_time() {
                    NoteFieldValue::DateTime(DateTimeValue::from(rec.instant()))
                } else {
                    NoteFieldValue::Date(rec.date())
                })
            }
            [NoteFieldValue::String(s), NoteFieldValue::String(fmt)] => {
                let rec = crate::DateValue::parse_with(s, fmt).ok()?;
                Some(if rec.precision.has_time() {
                    NoteFieldValue::DateTime(DateTimeValue::from(rec.instant()))
                } else {
                    NoteFieldValue::Date(rec.date())
                })
            }
            [NoteFieldValue::Null] | [NoteFieldValue::Null, _] => {
                Some(NoteFieldValue::Null)
            }
            _ => None,
        };
    }
    None
}

fn date_boundary(
    args: &[NoteFieldValue],
    op: impl FnOnce(DatePoint) -> Result<DatePoint, DateError>,
) -> Option<NoteFieldValue> {
    let date = args.first()?;
    if matches!(date, NoteFieldValue::Null) {
        return Some(NoteFieldValue::Null);
    }
    let pt = date_point(date)?;
    let shifted = op(pt).ok()?;
    match date {
        NoteFieldValue::Date(_) => Some(NoteFieldValue::Date(
            crate::DateValue::from(shifted.wall.date()),
        )),
        NoteFieldValue::DateTime(_) => {
            Some(NoteFieldValue::DateTime(DateTimeValue::from(shifted.instant)))
        }
        _ => Some(NoteFieldValue::Null),
    }
}
fn evaluate_date_component(args: &[NoteFieldValue]) -> Option<NoteFieldValue> {
    use chrono::{Datelike as _, Timelike as _};
    let [date, NoteFieldValue::String(component)] = args else {
        return Some(NoteFieldValue::Null);
    };
    let wall = date_point(date)?.wall;
    let number = if component.eq_ignore_ascii_case("year") {
        f64::from(wall.year())
    } else if component.eq_ignore_ascii_case("month") {
        f64::from(wall.month())
    } else if component.eq_ignore_ascii_case("day") {
        f64::from(wall.day())
    } else if component.eq_ignore_ascii_case("hour") {
        f64::from(wall.hour())
    } else if component.eq_ignore_ascii_case("minute") {
        f64::from(wall.minute())
    } else if component.eq_ignore_ascii_case("second") {
        f64::from(wall.second())
    } else if component.eq_ignore_ascii_case("weekday") {
        f64::from(wall.weekday().number_from_monday())
    } else if component.eq_ignore_ascii_case("week") {
        f64::from(wall.iso_week().week())
    } else {
        return None;
    };
    Some(NoteFieldValue::Number(number))
}

fn validate_registry_function_name(
    name: &str,
    input: &str,
    span: std::ops::Range<usize>,
) -> Result<(), QueryBuilderError> {
    if [
        "dur",
        "date_add",
        "date_diff",
        "date_component",
        "sow",
        "eow",
        "som",
        "start_of_month",
        "eom",
        "end_of_month",
        "soy",
        "eoy",
        "date",
    ]
    .iter()
    .any(|entry| name.eq_ignore_ascii_case(entry))
    {
        Ok(())
    } else {
        Err(QuerySyntaxError::unexpected_end(
            QueryDialect::Filter,
            input,
            span,
            "recognized filter function",
        )
        .into())
    }
}

#[expect(
    clippy::cognitive_complexity,
    reason = "checks the four registry signatures and literal arguments in \
              one pass"
)]
fn validate_call_args(
    name: &str,
    args: &[ValueExpr],
    input: &str,
    span: std::ops::Range<usize>,
) -> Result<(), QueryBuilderError> {
    let expected = if name.eq_ignore_ascii_case("dur") {
        if args.len() != 1 {
            Some("1 argument for dur()")
        } else if matches!(args.first(), Some(ValueExpr::Literal(NoteFieldValue::String(text))) if DurationValue::parse(text).is_err())
        {
            Some("valid duration")
        } else {
            None
        }
    } else if name.eq_ignore_ascii_case("date_add") {
        if !matches!(args.len(), 2 | 3) {
            Some("2 or 3 arguments for date_add()")
        } else if matches!(args.get(1), Some(ValueExpr::Literal(NoteFieldValue::String(text))) if DurationValue::parse(text).is_err())
            && args.len() == 2
        {
            Some("valid duration")
        } else if args.len() == 3
            && matches!(args.get(2), Some(ValueExpr::Literal(NoteFieldValue::String(unit))) if DurationUnit::parse(unit).is_none())
        {
            Some("recognized duration unit")
        } else {
            None
        }
    } else if name.eq_ignore_ascii_case("date_diff") {
        if !matches!(args.len(), 2 | 3) {
            Some("2 or 3 arguments for date_diff()")
        } else if matches!(args.get(2), Some(ValueExpr::Literal(NoteFieldValue::String(unit))) if DurationUnit::parse(unit).is_none())
        {
            Some("recognized duration unit")
        } else {
            None
        }
    } else if name.eq_ignore_ascii_case("date_component") {
        if args.len() != 2 {
            Some("2 arguments for date_component()")
        } else if matches!(args.get(1), Some(ValueExpr::Literal(NoteFieldValue::String(comp))) if !["year", "month", "day", "hour", "minute", "second", "weekday", "week"].iter().any(|valid| comp.eq_ignore_ascii_case(valid)))
        {
            Some(
                "valid component: year, month, day, hour, minute, second, \
                 weekday, week",
            )
        } else {
            None
        }
    } else if matches!(
        name.to_ascii_lowercase().as_str(),
        "sow"
            | "eow"
            | "som"
            | "start_of_month"
            | "eom"
            | "end_of_month"
            | "soy"
            | "eoy"
    ) {
        if args.len() == 1 {
            None
        } else {
            Some("1 argument for bucketing function")
        }
    } else if name.eq_ignore_ascii_case("date") {
        if matches!(args.len(), 1 | 2) {
            None
        } else {
            Some("1 or 2 arguments for date()")
        }
    } else {
        None
    };
    if let Some(expected) = expected {
        return Err(QuerySyntaxError::unexpected_end(
            QueryDialect::Filter,
            input,
            span,
            expected,
        )
        .into());
    }
    Ok(())
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
    fn is_satisfied_by(
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
        let Some(value) = field.as_note_ref() else {
            return false;
        };
        if matches!(value, NoteFieldValueRef::Null)
            || matches!(literal, NoteFieldValue::Null)
        {
            return false;
        }
        match value.compare(&literal.as_ref()) {
            std::cmp::Ordering::Less => matches!(self, Self::Lt | Self::Le),
            std::cmp::Ordering::Equal => matches!(self, Self::Le | Self::Ge),
            std::cmp::Ordering::Greater => matches!(self, Self::Gt | Self::Ge),
        }
    }

    pub(super) fn is_satisfied_by_values(
        self,
        lhs: &NoteFieldValue,
        rhs: &NoteFieldValue,
    ) -> bool {
        if matches!(self, Self::Eq | Self::Ne) {
            let equal = lhs.as_ref().is_equal_to_literal(rhs);
            return if matches!(self, Self::Eq) {
                equal
            } else {
                !equal
            };
        }
        if matches!(lhs, NoteFieldValue::Null)
            || matches!(rhs, NoteFieldValue::Null)
        {
            return false;
        }
        match lhs.as_ref().compare(&rhs.as_ref()) {
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
        tokens: &mut SpannedTokenStream<FilterToken>,
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
        tokens: &mut SpannedTokenStream<FilterToken>,
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
            QuerySyntaxError::unexpected_end(
                QueryDialect::Filter,
                input,
                0..name.len(),
                "`contains`",
            )
            .into()
        })
    }

    fn parse_registry_call(
        input: &str,
        tokens: &mut SpannedTokenStream<FilterToken>,
        name: String,
        span: std::ops::Range<usize>,
    ) -> Result<ValueExpr, QueryBuilderError> {
        validate_registry_function_name(&name, input, span.clone())?;
        tokens.next();
        let mut args = Vec::new();
        if !tokens.peek_is_value(&FilterToken::RParen) {
            loop {
                args.push(Self::parse_value_expr(input, tokens)?);
                if !tokens.peek_is_value(&FilterToken::Comma) {
                    break;
                }
                tokens.next();
            }
        }
        tokens
            .expect(
                input,
                TokenSpec::new(
                    &FilterToken::RParen,
                    "`)` after function arguments",
                ),
            )
            .map_err(|err| {
                QuerySyntaxError::from_lex(QueryDialect::Filter, input, err)
            })?;
        validate_call_args(&name, &args, input, span)?;
        Ok(ValueExpr::Call {
            name,
            args,
        })
    }

    #[expect(
        clippy::arithmetic_side_effects,
        reason = "duration negation preserves non-finite results without \
                  panicking"
    )]
    fn parse_primary(
        input: &str,
        tokens: &mut SpannedTokenStream<FilterToken>,
    ) -> Result<ValueExpr, QueryBuilderError> {
        let negative = tokens.peek_is_value(&FilterToken::Minus);
        if negative {
            tokens.next();
        }
        let span = tokens.next_span(input);
        let token = tokens.next().ok_or_else(|| {
            QuerySyntaxError::unexpected_end(
                QueryDialect::Filter,
                input,
                span.clone(),
                "a field, literal, or function call",
            )
        })?;
        let expression = match token.into_value() {
            FilterToken::Literal(value) => {
                ValueExpr::Literal(ComparisonExpr::classify_literal(value))
            }
            FilterToken::Ident(name)
                if tokens.peek_is_value(&FilterToken::LParen) =>
            {
                Self::parse_registry_call(input, tokens, name, span.clone())?
            }
            FilterToken::Ident(name) => {
                ValueExpr::Field(FieldPath::parse(&name)?)
            }
            _ => {
                return Err(QuerySyntaxError::unexpected_end(
                    QueryDialect::Filter,
                    input,
                    span,
                    "a field, literal, or function call",
                )
                .into());
            }
        };
        if !negative {
            return Ok(expression);
        }
        match expression {
            ValueExpr::Literal(NoteFieldValue::Number(number)) => {
                Ok(ValueExpr::Literal(NoteFieldValue::Number(-number)))
            }
            ValueExpr::Literal(NoteFieldValue::Duration(duration)) => Ok(
                ValueExpr::Literal(NoteFieldValue::Duration(duration * -1.0)),
            ),
            _ => Err(QuerySyntaxError::unexpected_end(
                QueryDialect::Filter,
                input,
                span,
                "a number or duration after `-`",
            )
            .into()),
        }
    }

    /// Folds arithmetic in written order; comparison and logical tokens end it.
    fn parse_value_expr(
        input: &str,
        tokens: &mut SpannedTokenStream<FilterToken>,
    ) -> Result<ValueExpr, QueryBuilderError> {
        let mut lhs = Self::parse_primary(input, tokens)?;
        loop {
            let op = if tokens.peek_is_value(&FilterToken::Plus) {
                ArithOp::Add
            } else if tokens.peek_is_value(&FilterToken::Minus) {
                ArithOp::Sub
            } else if tokens.peek_is_value(&FilterToken::Star) {
                ArithOp::Mul
            } else {
                break;
            };
            tokens.next();
            let rhs = Self::parse_primary(input, tokens)?;
            lhs = ValueExpr::Binary {
                lhs: Box::new(lhs),
                op,
                rhs: Box::new(rhs),
            };
        }
        Ok(lhs)
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
            | FilterToken::Plus
            | FilterToken::Minus
            | FilterToken::Star
            | FilterToken::Op(_)
            | FilterToken::Literal(_)
            | FilterToken::Ident(_) => None,
        }
    }

    /// Parses a `contains` predicate or compares two value expressions.
    fn parse_atom(
        &self,
        input: &str,
        tokens: &mut SpannedTokenStream<Self::Token>,
    ) -> Result<Self::Atom, QueryBuilderError> {
        if matches!(
            tokens.peek().map(Spanned::value),
            Some(FilterToken::Ident(name)) if name.eq_ignore_ascii_case("contains")
        ) {
            let name = tokens
                .expect_map(input, "`contains`", |token| {
                    match token.into_value() {
                        FilterToken::Ident(name) => Some(name),
                        _ => None,
                    }
                })
                .map_err(|err| {
                    QuerySyntaxError::from_lex(QueryDialect::Filter, input, err)
                })?;
            let name = name.into_value();
            return Self::parse_function_call(input, tokens, &name)
                .map(FilterAtom::Function);
        }
        let lhs = Self::parse_value_expr(input, tokens)?;
        let operator = tokens
            .expect_map(input, "a comparison operator", |token| {
                match token.into_value() {
                    FilterToken::Op(op) => Some(op),
                    _ => None,
                }
            })
            .map_err(|err| {
                QuerySyntaxError::from_lex(QueryDialect::Filter, input, err)
            })?;
        let rhs = Self::parse_value_expr(input, tokens)?;
        Ok(FilterAtom::Comparison(ComparisonExpr::new(
            lhs,
            *operator.value(),
            rhs,
        )))
    }

    fn syntax_error(
        &self,
        input: &str,
        span: std::ops::Range<usize>,
        expected: &'static str,
    ) -> QuerySyntaxError {
        QuerySyntaxError::unexpected_end(
            QueryDialect::Filter,
            input,
            span,
            expected,
        )
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
    #[token("+")]
    Plus,
    #[token("-")]
    Minus,
    #[token("*")]
    Star,
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
    #[regex(r#"[^\s()'",=!<>&|+*-]+"#, |lex| lex.slice().to_owned())]
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

    fn rows_for_files(_temp: &Path, files: &[(&str, &str)]) -> QuerySet {
        let index = crate::build_test_index(files);
        QueryService::new("class")
            .run(&index, QueryBuilder::pages(SourceSelector::All))
    }

    fn rows_for(temp: &Path, content: &str) -> QuerySet {
        rows_for_files(temp, &[("note.md", content)])
    }

    fn rated_rows(temp: &Path) -> QuerySet {
        rows_for_files(temp, &[
            ("low.md", "---\nrating: 3\nstatus: draft\n---"),
            ("high.md", "---\nrating: 7\nstatus: done\n---"),
            ("unrated.md", "---\nstatus: done\n---"),
        ])
    }

    fn names(rows: &QuerySet) -> Vec<String> {
        rows.iter().map(|row| row.file().name().as_str().to_owned()).collect()
    }

    mod parse {
        use pretty_assertions::assert_eq;
        use rstest::rstest;

        use super::*;

        #[rstest]
        #[case::no_operator("rating")]
        #[case::empty_field(" > 5")]
        #[case::empty_value("rating >")]
        #[case::unknown_function("unknown(tags, \"#book\")")]
        #[case::function_missing_target("contains(tags)")]
        fn rejects_malformed_expressions(#[case] expr: &str) {
            let temp = tempfile::tempdir().expect("create temp dir");
            let rows = rated_rows(temp.path());

            assert!(matches!(
                rows.filter(expr),
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
        #[case("rating > NaN")]
        #[case("rating > inf")]
        #[case("rating > -inf")]
        fn rejects_non_finite_numeric_literals(#[case] expr: &str) {
            assert!(matches!(
                FilterExpr::parse(expr),
                Err(QueryBuilderError::Syntax(_))
            ));
        }

        #[test]
        fn rejects_malformed_field_path_in_expression() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let rows = rated_rows(temp.path());

            assert_eq!(
                rows.filter("file.zzzz == 1"),
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
        use crate::TzGuard;

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
            let rows = rated_rows(temp.path());

            let filtered = rows.filter(expr).expect("valid filter");

            assert_eq!(names(&filtered), expected);
        }

        #[test]
        fn missing_field_never_matches_equality_or_ordering() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let rows = rated_rows(temp.path());

            let filtered = rows.filter("rating > 0").expect("valid filter");

            assert_eq!(names(&filtered), ["high", "low"]);
        }

        #[test]
        fn missing_field_matches_not_equal() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let rows = rated_rows(temp.path());

            let filtered = rows.filter("rating != 7").expect("valid filter");

            assert_eq!(names(&filtered), ["low", "unrated"]);
        }

        #[test]
        fn cross_kind_ordering_follows_canonical_rank() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let rows = rated_rows(temp.path());

            // Text (rank 5) is greater than Number 5 (rank 2).
            let filtered =
                rows.clone().filter("status > 5").expect("valid filter");
            assert_eq!(names(&filtered), ["high", "low", "unrated"]);

            // Number 5 is not greater than Text, so status < 5 matches nothing.
            let below = rows.filter("status < 5").expect("valid filter");
            assert!(below.is_empty());
        }

        #[test]
        fn mixed_kind_column_ordering_matches_sort_order() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let rows = rows_for_files(temp.path(), &[
                ("num_low.md", "---\nrating: 3\n---"),
                ("num_high.md", "---\nrating: 7\n---"),
                ("text.md", "---\nrating: gold\n---"),
            ]);

            // In canonical rank: Number < Text.
            // rating > 5 matches num_high (7 > 5) and text ("gold" > 5 by
            // rank).
            let filtered =
                rows.clone().filter("rating > 5").expect("valid filter");
            assert_eq!(names(&filtered), ["num_high", "text"]);

            // sort rating asc orders: num_low (3), num_high (7), text ("gold").
            let sorted = rows.sort("rating", false).expect("valid sort");
            assert_eq!(names(&sorted), ["num_low", "num_high", "text"]);
        }

        #[test]
        fn chains_across_multiple_filter_calls() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let rows = rated_rows(temp.path());

            let filtered = rows
                .filter("status == \"done\"")
                .expect("valid filter")
                .filter("rating >= 7")
                .expect("valid filter");

            assert_eq!(names(&filtered), ["high"]);
        }

        #[test]
        fn equal_matches_a_date_field_against_a_string_literal() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let rows = rows_for(temp.path(), "---\ndue: 2026-01-01\n---");

            let filtered =
                rows.filter("due == \"2026-01-01\"").expect("valid filter");

            assert_eq!(filtered.len(), 1);
        }

        #[test]
        fn equal_null_matches_rows_with_a_null_field() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let rows = rated_rows(temp.path());

            let filtered = rows.filter("rating == null").expect("valid filter");

            assert_eq!(names(&filtered), ["unrated"]);
        }

        #[test]
        fn not_equal_null_matches_rows_with_a_non_null_field() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let rows = rated_rows(temp.path());

            let filtered = rows.filter("rating != null").expect("valid filter");

            assert_eq!(names(&filtered), ["high", "low"]);
        }

        #[test]
        fn r_where_alias_filters_records_identically_to_filter() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let rows = rated_rows(temp.path());

            let filtered = rows.r#where("rating >= 7").expect("valid where");

            assert_eq!(names(&filtered), ["high"]);
        }

        #[test]
        fn evaluates_duration_comparisons_temporally() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let rows = rows_for_files(temp.path(), &[
                ("a.md", "---\nspent: 1h\n---"),
                ("b.md", "---\nspent: 30m\n---"),
            ]);
            let filtered =
                rows.filter("spent > \"30m\"").expect("valid filter");
            assert_eq!(names(&filtered), ["a"]);
        }

        #[test]
        fn matches_duration_equality_across_differing_spellings() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let rows = rows_for_files(temp.path(), &[
                ("a.md", "---\nspent: 90m\n---"),
                ("b.md", "---\nspent: 45m\n---"),
            ]);
            let filtered =
                rows.filter("spent == \"1h 30m\"").expect("valid filter");
            assert_eq!(names(&filtered), ["a"]);
        }

        #[test]
        fn evaluates_a_date_only_literal_at_local_midnight() {
            // A naive literal means the reader's zone: the boundaries here
            // are local midnight in the pinned UTC+2 zone (2025-12-31T22:00Z
            // through 2026-01-01T22:00Z), which still separates the two
            // fixture instants.
            TzGuard::set("Etc/GMT-2");

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
                IndexerService::for_tests(temp.path())
                    .build()
                    .expect("build index"),
            );
            let rows = QueryService::new("class")
                .run(&index, QueryBuilder::pages(SourceSelector::All));

            let filtered = rows
                .filter(
                    "file.mtime >= \"2026-01-01\" and file.mtime < \
                     \"2026-01-02\"",
                )
                .expect("valid filter");

            assert_eq!(names(&filtered), ["jan1"]);
        }

        #[test]
        fn matches_calendar_days_via_mdate() {
            // `mdate` is the mtime's local calendar date, so the expectation
            // only holds in the pinned zone.
            TzGuard::set("UTC");

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
                IndexerService::for_tests(temp.path())
                    .build()
                    .expect("build index"),
            );
            let rows = QueryService::new("class")
                .run(&index, QueryBuilder::pages(SourceSelector::All));

            let filtered = rows
                .filter("file.mdate == \"2026-01-01\"")
                .expect("valid filter");

            assert_eq!(names(&filtered), ["jan1"]);
        }

        #[test]
        fn and_combination_keeps_only_records_matching_both_sides() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let rows = rated_rows(temp.path());

            let filtered = rows
                .filter("rating > 5 AND status == \"done\"")
                .expect("valid filter");

            assert_eq!(names(&filtered), ["high"]);
        }

        #[test]
        fn or_combination_keeps_records_matching_either_side() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let rows = rated_rows(temp.path());

            let filtered = rows
                .filter("rating == 3 OR status == \"done\"")
                .expect("valid filter");

            assert_eq!(names(&filtered), ["high", "low", "unrated"]);
        }

        #[test]
        fn not_combination_reverses_the_matching_condition() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let rows = rated_rows(temp.path());

            let filtered =
                rows.filter("NOT status == \"done\"").expect("valid filter");

            assert_eq!(names(&filtered), ["low"]);
        }

        #[test]
        fn default_boolean_precedence_evaluates_correctly() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let rows = rated_rows(temp.path());

            let filtered = rows
                .filter(
                    "status == \"done\" OR rating == 3 AND status == \"draft\"",
                )
                .expect("valid filter");

            assert_eq!(names(&filtered), ["high", "low", "unrated"]);
        }

        #[test]
        fn nested_parentheses_override_precedence() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let rows = rated_rows(temp.path());

            let nested = rows
                .filter(
                    "(rating > 5 OR status == \"draft\") AND NOT rating == 3",
                )
                .expect("valid filter");

            assert_eq!(names(&nested), ["high"]);
        }

        #[test]
        fn logical_op_spellings_do_not_swallow_identifier_prefixes() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let rows = rows_for(temp.path(), "---\norder: 5\nandrew: 3\n---");

            let lower = rows
                .clone()
                .filter("order == 5 and andrew == 3")
                .expect("valid filter: lowercase and");
            assert_eq!(lower.len(), 1);

            let symbolic_and = rows
                .clone()
                .filter("order == 5 && andrew == 3")
                .expect("valid filter: &&");
            assert_eq!(symbolic_and.len(), 1);

            let symbolic_or = rows
                .clone()
                .filter("order == 999 || andrew == 3")
                .expect("valid filter: ||");
            assert_eq!(symbolic_or.len(), 1);

            let lower_or = rows
                .clone()
                .filter("order == 999 or andrew == 3")
                .expect("valid filter: lowercase or");
            assert_eq!(lower_or.len(), 1);

            // Regression: fields named `order`/`andrew` must stay whole
            // identifiers, not logical-op prefixes.
            let ident_prefix =
                rows.filter("order == 5").expect("valid filter: bare field");
            assert_eq!(ident_prefix.len(), 1);
        }
    }

    mod contains {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn contains_matches_tags_by_prefix_hierarchy() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let rows = rows_for_files(temp.path(), &[
                (
                    "book.md",
                    "---\ntitle: Rust Handbook\n---\nFiled under #book/fiction",
                ),
                (
                    "article.md",
                    "---\ntitle: Async Guide\n---\nFiled under #article",
                ),
            ]);

            let filtered =
                rows.filter("contains(tags, \"#book\")").expect("valid filter");

            assert_eq!(names(&filtered), ["book"]);
        }

        #[test]
        fn contains_matches_string_fields_by_substring() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let rows = rows_for_files(temp.path(), &[
                (
                    "book.md",
                    "---\ntitle: Rust Handbook\n---\nFiled under #book/fiction",
                ),
                (
                    "article.md",
                    "---\ntitle: Async Guide\n---\nFiled under #article",
                ),
            ]);

            let filtered = rows
                .filter("contains(title, \"Async\")")
                .expect("valid filter");

            assert_eq!(names(&filtered), ["article"]);
        }

        #[test]
        fn contains_distinguishes_list_values_from_tag_hierarchy() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let rows = rows_for_files(temp.path(), &[
                (
                    "handbook.md",
                    "---\ncategories: [handbook]\n---\nTagged #bookworm",
                ),
                (
                    "book.md",
                    "---\ncategories: [book]\n---\nTagged #book/fiction",
                ),
            ]);

            let category_match = rows
                .clone()
                .filter("contains(categories, \"book\")")
                .expect("valid filter");
            assert_eq!(names(&category_match), ["book"]);

            let tag_match =
                rows.filter("contains(tags, \"#book\")").expect("valid filter");
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

    #[test]
    fn query_date_arithmetic_matches_calendar_application_and_written_order() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let rows = rows_for_files(temp.path(), &[
            ("jan.md", "---\nwhen: 2026-01-31\n---"),
            ("feb.md", "---\nwhen: 2026-02-15\n---"),
        ]);
        let shifted = rows
            .clone()
            .filter("when + dur(\"1 month\") == \"2026-02-28\"")
            .expect("valid filter");
        assert_eq!(names(&shifted), vec!["jan"]);
        let compound = rows
            .filter("date_add(when, dur(\"1mo 1d\")) == \"2026-03-01\"")
            .expect("valid filter");
        assert_eq!(names(&compound), vec!["jan"]);
    }

    #[test]
    fn query_temporal_values_support_difference_components_and_scaling() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let rows = rows_for_files(temp.path(), &[
            ("jan.md", "---\nwhen: 2026-01-31\n---"),
            ("feb.md", "---\nwhen: 2026-02-15\n---"),
        ]);
        assert_eq!(
            names(
                &rows
                    .clone()
                    .filter("when - \"2026-01-31\" == dur(\"15d\")")
                    .expect("date difference")
            ),
            vec!["feb"],
        );
        assert_eq!(
            names(
                &rows
                    .clone()
                    .filter("date_diff(\"2026-01-31\", when, \"days\") == 15")
                    .expect("forward date_diff")
            ),
            vec!["feb"],
        );
        assert_eq!(
            names(
                &rows
                    .clone()
                    .filter("date_diff(when, \"2026-01-31\", \"days\") == -15")
                    .expect("backward date_diff")
            ),
            vec!["feb"],
        );
        assert_eq!(
            names(
                &rows
                    .clone()
                    .filter("date_component(when, \"month\") == 2")
                    .expect("date_component")
            ),
            vec!["feb"],
        );
        assert_eq!(
            names(
                &rows
                    .filter(
                        "date_add(when, dur(\"1 day\") * -2) == \"2026-02-13\""
                    )
                    .expect("duration scaling")
            ),
            vec!["feb"],
        );
    }
    #[test]
    fn query_arithmetic_uses_written_order_and_handles_null_and_wrong_types() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let rows = rows_for(temp.path(), "---\nwhen: 2026-01-31\n---");
        for expression in [
            "dur(\"1d\") + dur(\"2d\") * 2 == dur(\"6d\")",
            "dur(\"5d\") - dur(\"2d\") == dur(\"3d\")",
            "when - dur(\"1d\") == \"2026-01-30\"",
            "date_add(when, 1, \"months\") == \"2026-02-28\"",
            "date_add(when, 1, \"mo\") == \"2026-02-28\"",
            "date_component(when, \"week\") == 5",
            "date_component(when, \"weekday\") == 6",
            "date_add(null, dur(\"1d\")) == null",
            "date_add(3, dur(\"1d\")) == null",
        ] {
            assert_eq!(
                names(&rows.clone().filter(expression).expect(expression)),
                vec!["note"],
                "{expression}"
            );
        }
        for expression in [
            "dur(\"1d\") + dur(\"2d\") * 2 == dur(\"5d\")",
            "date_add(when, 0.5, \"months\") == \"2026-01-31\"",
            "date_add(missing, dur(\"1d\")) == \"2026-02-01\"",
        ] {
            assert!(
                rows.clone().filter(expression).expect(expression).is_empty(),
                "{expression}"
            );
        }
    }

    #[test]
    fn query_registry_rejects_invalid_static_arguments_and_unlisted_value_forms()
     {
        for expression in [
            "date_component(\"2026-01-31\", \"week99\") == 5",
            "date_add(\"2026-01-31\", 1, \"fortnights\") == \"2026-02-01\"",
            "date_diff(\"2026-01-31\", \"2026-02-01\", \"nonsense\") == 1",
            "dur(\"not a duration\") == dur(\"1d\")",
            "unknown(\"2026-01-31\") == 1",
            "date_add(\"2026-01-31\", 1, \"day\" and true) == null",
            "date_add((\"2026-01-31\"), dur(\"1d\")) == null",
        ] {
            assert!(FilterExpr::parse(expression).is_err(), "{expression}");
        }
    }

    #[test]
    fn query_sow_returns_iso_start_of_week() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let rows = rows_for_files(temp.path(), &[
            ("sun.md", "---\nwhen: 2026-02-01\n---"),
            ("wed.md", "---\nwhen: 2026-07-29\n---"),
        ]);
        assert_eq!(
            names(&rows.filter("sow(when) == \"2026-01-26\"").expect("sow")),
            vec!["sun"],
        );
    }

    #[test]
    fn query_eow_returns_iso_end_of_week() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let rows = rows_for_files(temp.path(), &[
            ("sun.md", "---\nwhen: 2026-02-01\n---"),
            ("wed.md", "---\nwhen: 2026-07-29\n---"),
        ]);
        assert_eq!(
            names(&rows.filter("eow(when) == \"2026-02-01\"").expect("eow")),
            vec!["sun"],
        );
    }

    #[test]
    fn query_som_returns_first_day_of_month() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let rows = rows_for_files(temp.path(), &[
            ("sun.md", "---\nwhen: 2026-02-01\n---"),
            ("wed.md", "---\nwhen: 2026-07-29\n---"),
        ]);
        assert_eq!(
            names(
                &rows
                    .clone()
                    .filter("som(when) == \"2026-07-01\"")
                    .expect("som")
            ),
            vec!["wed"],
        );
        assert_eq!(
            names(
                &rows
                    .filter("start_of_month(when) == \"2026-07-01\"")
                    .expect("start_of_month")
            ),
            vec!["wed"],
        );
    }

    #[test]
    fn query_eom_returns_last_day_of_month() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let rows = rows_for_files(temp.path(), &[
            ("sun.md", "---\nwhen: 2026-02-01\n---"),
            ("wed.md", "---\nwhen: 2026-07-29\n---"),
        ]);
        assert_eq!(
            names(
                &rows
                    .clone()
                    .filter("eom(when) == \"2026-07-31\"")
                    .expect("eom")
            ),
            vec!["wed"],
        );
        assert_eq!(
            names(
                &rows
                    .filter("end_of_month(when) == \"2026-07-31\"")
                    .expect("end_of_month")
            ),
            vec!["wed"],
        );
    }

    #[test]
    fn query_soy_returns_first_day_of_year() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let rows = rows_for_files(temp.path(), &[
            ("sun.md", "---\nwhen: 2026-02-01\n---"),
            ("wed.md", "---\nwhen: 2026-07-29\n---"),
        ]);
        assert_eq!(
            names(&rows.filter("soy(when) == \"2026-01-01\"").expect("soy")),
            vec!["sun", "wed"],
        );
    }

    #[test]
    fn query_eoy_returns_last_day_of_year() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let rows = rows_for_files(temp.path(), &[
            ("sun.md", "---\nwhen: 2026-02-01\n---"),
            ("wed.md", "---\nwhen: 2026-07-29\n---"),
        ]);
        assert_eq!(
            names(&rows.filter("eoy(when) == \"2026-12-31\"").expect("eoy")),
            vec!["sun", "wed"],
        );
    }

    #[test]
    fn query_date_function_evaluates_parse_with_format() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let rows = rows_for_files(temp.path(), &[
            ("sun.md", "---\nwhen: 2026-02-01\n---"),
            ("wed.md", "---\nwhen: 2026-07-29\n---"),
        ]);
        assert_eq!(
            names(
                &rows
                    .filter("date(\"29/07/2026\", \"%d/%m/%Y\") == when")
                    .expect("date parse_with")
            ),
            vec!["wed"],
        );
    }

    #[test]
    fn query_date_function_evaluates_iso_literal() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let rows = rows_for_files(temp.path(), &[
            ("sun.md", "---\nwhen: 2026-02-01\n---"),
            ("wed.md", "---\nwhen: 2026-07-29\n---"),
        ]);
        assert_eq!(
            names(
                &rows
                    .filter("date(\"2026-07-29\") == when")
                    .expect("date classify")
            ),
            vec!["wed"],
        );
    }
}
