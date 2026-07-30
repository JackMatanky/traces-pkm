//! Comparison and logical operators for filter expressions.

use super::{
    IndexRecord,
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
    /// Strips a leading comparison operator from `s`.
    ///
    /// Returns the operator and the remaining text after it. Multi-character
    /// operators (`==`, `!=`, `>=`, `<=`) are checked before single-character
    /// prefixes (`>`, `<`).
    pub(super) fn strip_prefix(s: &str) -> Option<(Self, &str)> {
        const OPERATORS: [(&str, CompareOp); 6] = [
            ("==", CompareOp::Eq),
            ("!=", CompareOp::Ne),
            (">=", CompareOp::Ge),
            ("<=", CompareOp::Le),
            (">", CompareOp::Gt),
            ("<", CompareOp::Lt),
        ];
        OPERATORS.into_iter().find_map(|(token, op)| {
            s.strip_prefix(token).map(|rest| (op, rest))
        })
    }

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

/// A logical operator parsed from a [`super::QueryOutcome::filter`]
/// expression.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(super) enum LogicalOp {
    /// `AND` / `and` / `&&`
    And,
    /// `OR` / `or` / `||`
    Or,
    /// `NOT` / `not` / `!`
    Not,
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
            Self::Not => !exprs.first().is_some_and(|e| e.matches(record)),
        }
    }
}
