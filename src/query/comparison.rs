//! Comparison operators and expressions for filter evaluation.
//!
//! This module implements the six comparison operators (`==`, `!=`, `<`,
//! `<=`, `>`, `>=`) used in `.filter()` expressions. Equality operators
//! are total: every value kind, including [`FieldValue::Null`], compares
//! using cross-kind text normalization so string, date, and duration fields
//! match same-text literals interchangeably.

use super::{
    FieldPath, IndexRecord,
    sort::{compare_field_values, fields_equal},
};
use crate::note::FieldValue;

/// A comparison operator parsed from a
/// [`QueryOutcome::filter`][`super::QueryOutcome::filter`] expression.
///
/// Each variant maps to a syntactic operator in the filter language.
/// [`Self::Eq`] and [`Self::Ne`] are total: they compare every value kind
/// including [`FieldValue::Null`]. Ordering operators (`<`, `<=`, `>`, `>=`)
/// return `false` for unorderable or incomparable value pairs.
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
    /// Returns whether `field` satisfies this operator against `literal`.
    ///
    /// [`Self::Eq`] and [`Self::Ne`] are total: they use
    /// [`fields_equal`][`super::sort::fields_equal`] for cross-kind text
    /// normalization, so a [`String`], `Date`, or `Duration` field matches a
    /// same-text literal of any of those three kinds. Ordering operators
    /// compare via
    /// [`compare_field_values`][`super::sort::compare_field_values`],
    /// returning `false` for mismatched or unorderable value pairs.
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

/// A parsed `<field> <op> <value>` comparison node in a filter expression.
///
/// Pairs an already-parsed [`FieldPath`] with a [`CompareOp`] and a literal
/// [`FieldValue`] to evaluate against each record's resolved field.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct ComparisonExpr {
    field: FieldPath,
    op: CompareOp,
    value: FieldValue,
}

impl ComparisonExpr {
    /// Pairs `op` with the `field` path it resolves from a record and the
    /// literal `value` it compares that resolution against.
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

    /// Returns whether `record` satisfies this comparison.
    pub(super) fn matches(&self, record: &IndexRecord) -> bool {
        self.op.is_satisfied_by(&record.resolve(&self.field), &self.value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    mod is_satisfied_by {
        use pretty_assertions::assert_eq;
        use rstest::rstest;

        use super::*;

        #[rstest]
        #[case(CompareOp::Eq, 5.0, 5.0, true)]
        #[case(CompareOp::Eq, 5.0, 10.0, false)]
        #[case(CompareOp::Ne, 5.0, 10.0, true)]
        #[case(CompareOp::Ne, 5.0, 5.0, false)]
        #[case(CompareOp::Lt, 5.0, 10.0, true)]
        #[case(CompareOp::Lt, 10.0, 5.0, false)]
        #[case(CompareOp::Le, 5.0, 5.0, true)]
        #[case(CompareOp::Le, 5.0, 10.0, true)]
        #[case(CompareOp::Gt, 10.0, 5.0, true)]
        #[case(CompareOp::Gt, 5.0, 10.0, false)]
        #[case(CompareOp::Ge, 5.0, 5.0, true)]
        #[case(CompareOp::Ge, 10.0, 5.0, true)]
        fn evaluates_number_comparisons(
            #[case] op: CompareOp,
            #[case] left: f64,
            #[case] right: f64,
            #[case] expected: bool,
        ) {
            // Arrange
            let left_val = FieldValue::Number(left);
            let right_val = FieldValue::Number(right);

            // Act
            let result = op.is_satisfied_by(&left_val, &right_val);

            // Assert
            assert_eq!(result, expected);
        }
    }

    mod parse {
        use pretty_assertions::assert_eq;
        use rstest::rstest;

        use super::*;

        #[rstest]
        #[case("==", Ok(CompareOp::Eq))]
        #[case("!=", Ok(CompareOp::Ne))]
        #[case(">=", Ok(CompareOp::Ge))]
        #[case("<=", Ok(CompareOp::Le))]
        #[case(">", Ok(CompareOp::Gt))]
        #[case("<", Ok(CompareOp::Lt))]
        #[case("invalid", Err(()))]
        fn parses_from_lexer_spelling(
            #[case] spelling: &str,
            #[case] expected: Result<CompareOp, ()>,
        ) {
            // Act
            let result = CompareOp::try_from(spelling);

            // Assert
            assert_eq!(result, expected);
        }
    }
}
