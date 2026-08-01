//! Comparison and logical operators for filter expressions, and the AST nodes
//! ([`ComparisonExpr`], [`LogicalExpr`]) that pair each operator with its
//! operands.

use super::{
    FieldPath, IndexRecord,
    filter::FilterExpr,
    sort::{compare_field_values, fields_equal},
};
use crate::note::FieldValue;

/// A comparison operator parsed from a [`super::QueryOutcome::filter`]
/// expression.
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
    /// Whether `field` matches `literal` under this operator.
    ///
    /// `==`/`!=` are total: every value kind, including [`FieldValue::Null`],
    /// compares. They use [`fields_equal`], not raw [`FieldValue`] equality, so
    /// a `String`, `Date`, or `Duration` field matches a same-text literal of
    /// any of those three kinds — the same cross-kind text normalization
    /// [`compare_field_values`] applies to the ordering operators below.
    pub(super) fn is_satisfied_by(
        self,
        field: &FieldValue,
        literal: &FieldValue,
    ) -> bool {
        match self {
            Self::Eq => fields_equal(field, literal),
            Self::Ne => !fields_equal(field, literal),
            Self::Lt => {
                compare_field_values(field, literal)
                    == Some(std::cmp::Ordering::Less)
            }
            Self::Gt => {
                compare_field_values(field, literal)
                    == Some(std::cmp::Ordering::Greater)
            }
            Self::Le => matches!(
                compare_field_values(field, literal),
                Some(std::cmp::Ordering::Less | std::cmp::Ordering::Equal)
            ),
            Self::Ge => matches!(
                compare_field_values(field, literal),
                Some(std::cmp::Ordering::Greater | std::cmp::Ordering::Equal)
            ),
        }
    }
}

/// Parses an operator spelling matched by `FilterToken`'s tokenizer regex — the
/// single source of truth for how a [`CompareOp`] spells itself, so the
/// tokenizer never repeats operator semantics this type already owns.
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

/// A `<field> <op> <value>` comparison — [`FilterExpr::Comparison`]'s payload.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct ComparisonExpr {
    field: FieldPath,
    op: CompareOp,
    value: FieldValue,
}

impl ComparisonExpr {
    /// Pairs `op` with the `field` it resolves from a record and the literal
    /// `value` it compares that resolution against.
    pub(super) fn new(
        field: FieldPath,
        op: CompareOp,
        value: FieldValue,
    ) -> Self {
        Self {
            field,
            op,
            value,
        }
    }

    /// Whether `record` satisfies this comparison.
    pub(super) fn matches(&self, record: &IndexRecord) -> bool {
        self.op.is_satisfied_by(&record.resolve(&self.field), &self.value)
    }
}

/// A logical `AND`/`OR` combinator joining two or more [`FilterExpr`]s.
///
/// `NOT` negates exactly one sub-expression — an arity `AND`/`OR` don't share —
/// so it isn't a variant here; it's [`FilterExpr::Not`] instead, keeping every
/// [`Self::And`]/[`Self::Or`] combination well-formed by construction.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(super) enum LogicalOp {
    /// `AND` / `and` / `&&`
    And,
    /// `OR` / `or` / `||`
    Or,
}

impl LogicalOp {
    /// Combines boolean results for `exprs` evaluated against `record`.
    pub(super) fn eval(
        self,
        exprs: &[FilterExpr],
        record: &IndexRecord,
    ) -> bool {
        match self {
            Self::And => exprs.iter().all(|e| e.matches(record)),
            Self::Or => exprs.iter().any(|e| e.matches(record)),
        }
    }
}

/// Parses a logical-combinator spelling matched by `FilterToken`'s tokenizer
/// regex — the `and`/`or` word forms match case-insensitively, mirroring
/// [`Self::And`]/[`Self::Or`]'s documented spellings; `&&`/`||` are compared
/// verbatim (case has no effect on symbols).
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

/// An `AND`/`OR` combination of two or more expressions —
/// [`FilterExpr::Logical`]'s payload.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct LogicalExpr {
    op: LogicalOp,
    exprs: Vec<FilterExpr>,
}

impl LogicalExpr {
    /// Pairs `op` with the `exprs` it combines.
    pub(super) fn new(op: LogicalOp, exprs: Vec<FilterExpr>) -> Self {
        Self {
            op,
            exprs,
        }
    }

    /// Whether `record` satisfies this combination.
    pub(super) fn matches(&self, record: &IndexRecord) -> bool {
        self.op.eval(&self.exprs, record)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compare_op_is_satisfied_by() {
        let num_5 = FieldValue::Number(5.0);
        let num_10 = FieldValue::Number(10.0);

        assert!(CompareOp::Eq.is_satisfied_by(&num_5, &num_5));
        assert!(!CompareOp::Eq.is_satisfied_by(&num_5, &num_10));

        assert!(CompareOp::Ne.is_satisfied_by(&num_5, &num_10));
        assert!(!CompareOp::Ne.is_satisfied_by(&num_5, &num_5));

        assert!(CompareOp::Lt.is_satisfied_by(&num_5, &num_10));
        assert!(!CompareOp::Lt.is_satisfied_by(&num_10, &num_5));

        assert!(CompareOp::Le.is_satisfied_by(&num_5, &num_5));
        assert!(CompareOp::Le.is_satisfied_by(&num_5, &num_10));

        assert!(CompareOp::Gt.is_satisfied_by(&num_10, &num_5));
        assert!(!CompareOp::Gt.is_satisfied_by(&num_5, &num_10));

        assert!(CompareOp::Ge.is_satisfied_by(&num_5, &num_5));
        assert!(CompareOp::Ge.is_satisfied_by(&num_10, &num_5));
    }

    #[test]
    fn compare_op_parses_every_spelling() {
        assert_eq!(CompareOp::try_from("=="), Ok(CompareOp::Eq));
        assert_eq!(CompareOp::try_from("!="), Ok(CompareOp::Ne));
        assert_eq!(CompareOp::try_from(">="), Ok(CompareOp::Ge));
        assert_eq!(CompareOp::try_from("<="), Ok(CompareOp::Le));
        assert_eq!(CompareOp::try_from(">"), Ok(CompareOp::Gt));
        assert_eq!(CompareOp::try_from("<"), Ok(CompareOp::Lt));
        assert_eq!(CompareOp::try_from("invalid"), Err(()));
    }

    #[test]
    fn logical_op_parses_every_spelling_case_insensitively() {
        assert_eq!(LogicalOp::try_from("&&"), Ok(LogicalOp::And));
        assert_eq!(LogicalOp::try_from("and"), Ok(LogicalOp::And));
        assert_eq!(LogicalOp::try_from("AND"), Ok(LogicalOp::And));
        assert_eq!(LogicalOp::try_from("||"), Ok(LogicalOp::Or));
        assert_eq!(LogicalOp::try_from("or"), Ok(LogicalOp::Or));
        assert_eq!(LogicalOp::try_from("OR"), Ok(LogicalOp::Or));
        assert_eq!(LogicalOp::try_from("invalid"), Err(()));
    }
}
