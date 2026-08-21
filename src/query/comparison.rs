//! Comparison operators and expressions for filter evaluation.
//!
//! This module implements the six comparison operators (`==`, `!=`, `<`,
//! `<=`, `>`, `>=`) used in filter expressions.
//!
//! # Semantics and Type Normalization
//!
//! - **Equality (`==`, `!=`):** Equality checks are total and compare all types
//!   (including [`NoteFieldValue::Null`]). If the operands have different
//!   types, the comparison normalizes them to a common textual representation
//!   (e.g. comparing a date to its string equivalent evaluates to `true`).
//! - **Ordering (`<`, `<=`, `>`, `>=`):** Ordering checks return `false` if the
//!   two values are incomparable or of different types (no cross-type
//!   normalization is performed for inequality).
//!
//! # Examples
//!
//! ```ignore
//! # use traces_pkm::query::comparison::{CompareOp, ComparisonExpr};
//! # use traces_pkm::query::FieldPath;
//! # use traces_pkm::note::NoteFieldValue;
//! # use traces_pkm::query::QueryRecord;
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let field = FieldPath::parse("rating")?;
//! let op = CompareOp::Gt;
//! let value = NoteFieldValue::Number(5.0);
//!
//! let expr = ComparisonExpr::new(field, op, value);
//! # Ok(())
//! # }
//! ```

use super::{
    FieldPath, QueryRecord,
    sort::{compare_field_values, fields_equal},
};
use crate::note::NoteFieldValue;

/// A comparison operator parsed from a filter expression.
///
/// Each variant maps to a syntactic operator in the filter language.
///
/// # Examples
///
/// ```ignore
/// # use traces_pkm::query::comparison::CompareOp;
/// # use traces_pkm::note::NoteFieldValue;
/// let op = CompareOp::Eq;
/// assert!(op.is_satisfied_by(&NoteFieldValue::Number(5.0), &NoteFieldValue::Number(5.0)));
/// ```
///
/// # Operator Rules
///
/// - **Equality (`==`, `!=`):** [`Self::Eq`] and [`Self::Ne`] are total. They
///   compare every value kind including [`NoteFieldValue::Null`].
/// - **Ordering (`<`, `<=`, `>`, `>=`):** Return `false` for unorderable or
///   incomparable value pairs.
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
    /// Returns whether a field value satisfies this operator against a literal.
    ///
    /// Checks the operator conditions using the provided field value and
    /// literal.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// # use traces_pkm::query::comparison::CompareOp;
    /// # use traces_pkm::note::NoteFieldValue;
    /// let op = CompareOp::Eq;
    /// assert!(op.is_satisfied_by(&NoteFieldValue::Number(5.0), &NoteFieldValue::Number(5.0)));
    /// ```
    ///
    /// # Behavior
    ///
    /// - **Equality (`==`, `!=`):** Uses
    ///   [`fields_equal`][`super::sort::fields_equal`] to normalize values
    ///   across different data types (such as comparing a date to a string).
    /// - **Ordering (`<`, `<=`, `>`, `>=`):** Compares values via
    ///   [`compare_field_values`][`super::sort::compare_field_values`],
    ///   returning `false` for mismatched or incomparable kinds.
    pub(super) fn is_satisfied_by(
        self,
        field: &NoteFieldValue,
        literal: &NoteFieldValue,
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

    /// Attempts to parse a comparison operator from its string representation.
    ///
    /// # Errors
    ///
    /// Returns `Err(())` if the spelling is not one of the six valid operators:
    /// `==`, `!=`, `>=`, `<=`, `>`, `<`.
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
/// [`NoteFieldValue`] to evaluate against the resolved field of each record.
///
/// # Examples
///
/// ```ignore
/// # use traces_pkm::query::comparison::{CompareOp, ComparisonExpr};
/// # use traces_pkm::query::FieldPath;
/// # use traces_pkm::note::NoteFieldValue;
/// let expr = ComparisonExpr::new(
///     FieldPath::parse("rating").unwrap(),
///     CompareOp::Gt,
///     NoteFieldValue::Number(5.0),
/// );
/// ```
#[derive(Clone, Debug, PartialEq)]
pub(super) struct ComparisonExpr {
    field: FieldPath,
    op: CompareOp,
    value: NoteFieldValue,
}

impl ComparisonExpr {
    /// Constructs a new [`ComparisonExpr`].
    ///
    /// # Examples
    ///
    /// ```ignore
    /// # use traces_pkm::query::comparison::{CompareOp, ComparisonExpr};
    /// # use traces_pkm::query::FieldPath;
    /// # use traces_pkm::note::NoteFieldValue;
    /// let expr = ComparisonExpr::new(
    ///     FieldPath::parse("rating").unwrap(),
    ///     CompareOp::Gt,
    ///     NoteFieldValue::Number(5.0),
    /// );
    /// ```
    pub(super) const fn new(
        field: FieldPath,
        op: CompareOp,
        value: NoteFieldValue,
    ) -> Self {
        Self {
            field,
            op,
            value,
        }
    }

    /// Returns whether the given index record satisfies this comparison
    /// expression.
    pub(super) fn matches(&self, record: &QueryRecord) -> bool {
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
            let left_val = NoteFieldValue::Number(left);
            let right_val = NoteFieldValue::Number(right);

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
