//! Comparison and logical operators, field value comparison, and sort key
//! ordering.

use std::cmp::Ordering;

use super::{
    super::file::FileRecord, IndexRecord, QueryError, filter::FilterExpr,
};
use crate::note::FieldValue;

/// General `file.*` metadata accessors available to query field paths.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(super) enum FileField {
    /// [`FileRecord::path`].
    Path,
    /// [`FileRecord::name`].
    Name,
    /// [`FileRecord::folder`].
    Folder,
    /// [`FileRecord::size`].
    Size,
    /// [`FileRecord::created_at_or_modified`], as a datetime with no UTC
    /// offset.
    CreatedDateTime,
    /// [`FileRecord::created_at_or_modified`], as a bare date.
    CreatedDate,
    /// [`FileRecord::modified_at`], as a datetime with no UTC offset.
    ModifiedDateTime,
    /// [`FileRecord::modified_at`], as a bare date.
    ModifiedDate,
}

impl FileField {
    /// Parses a `file.<field>` accessor name (the part after `"file."`).
    ///
    /// # Errors
    ///
    /// Returns [`QueryError::UnknownFieldPath`] if `name` is not a known
    /// accessor.
    pub(super) fn parse(name: &str) -> Result<Self, QueryError> {
        match name {
            "path" => Ok(Self::Path),
            "name" => Ok(Self::Name),
            "folder" => Ok(Self::Folder),
            "size" => Ok(Self::Size),
            "created_at" | "ctime" => Ok(Self::CreatedDateTime),
            "cdate" => Ok(Self::CreatedDate),
            "modified_at" | "mtime" => Ok(Self::ModifiedDateTime),
            "mdate" => Ok(Self::ModifiedDate),
            _ => Err(QueryError::UnknownFieldPath {
                path: format!("file.{name}"),
            }),
        }
    }

    /// Resolves this accessor against `file`.
    pub(super) fn resolve(self, file: &FileRecord) -> FieldValue {
        match self {
            Self::Path => {
                FieldValue::String(file.path().to_string_lossy().into_owned())
            }
            Self::Name => FieldValue::String(file.name().as_str().to_owned()),
            Self::Folder => {
                FieldValue::String(file.folder().to_string_lossy().into_owned())
            }
            #[expect(
                clippy::as_conversions,
                clippy::cast_precision_loss,
                reason = "file sizes stay well under 2^53 bytes for PKM-scale \
                          projects, so f64 keeps exact byte counts"
            )]
            Self::Size => FieldValue::Number(file.size() as f64),
            Self::CreatedDateTime => FieldValue::Date(
                file.created_at_or_modified().to_datetime_string(),
            ),
            Self::CreatedDate => {
                FieldValue::Date(file.created_at_or_modified().to_date_string())
            }
            Self::ModifiedDateTime => {
                FieldValue::Date(file.modified_at().to_datetime_string())
            }
            Self::ModifiedDate => {
                FieldValue::Date(file.modified_at().to_date_string())
            }
        }
    }
}

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
                compare_field_values(field, literal) == Some(Ordering::Less)
            }
            Self::Gt => {
                compare_field_values(field, literal) == Some(Ordering::Greater)
            }
            Self::Le => matches!(
                compare_field_values(field, literal),
                Some(Ordering::Less | Ordering::Equal)
            ),
            Self::Ge => matches!(
                compare_field_values(field, literal),
                Some(Ordering::Greater | Ordering::Equal)
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

/// Compares two resolved field values of the same comparable kind.
///
/// Orders numbers by magnitude, strings/dates/durations lexicographically, and
/// booleans with `false < true`. Returns `None` for differing kinds or
/// unorderable values.
pub(super) fn compare_field_values(
    a: &FieldValue,
    b: &FieldValue,
) -> Option<Ordering> {
    match (a, b) {
        (FieldValue::Number(x), FieldValue::Number(y)) => x.partial_cmp(y),
        (FieldValue::Bool(x), FieldValue::Bool(y)) => Some(x.cmp(y)),
        _ => match (a.as_str(), b.as_str()) {
            (Some(x), Some(y)) => Some(x.cmp(y)),
            _ => None,
        },
    }
}

/// Whether `a` and `b` represent the same value for `.filter()`'s `==`/`!=`.
///
/// Falls back to [`compare_field_values`] returning [`Ordering::Equal`] when
/// [`FieldValue`]'s own structural equality says no — the same cross-kind text
/// normalization that lets a `String` literal match a `Date`/`Duration` field
/// for the ordering operators.
pub(super) fn fields_equal(a: &FieldValue, b: &FieldValue) -> bool {
    a == b || compare_field_values(a, b) == Some(Ordering::Equal)
}

/// Total order for [`super::QueryOutcome::sort`] and
/// [`super::QueryOutcome::group_by`].
///
/// Matches Dataview's `compareValue` semantics:
/// - **Null Values**: [`FieldValue::Null`] acts as the minimum value.
/// - **Direction**: `descending` reverses the entire comparator uniformly (so
///   `Null` leads ascending and trails descending).
/// - **Non-Null Ordering**: Ordered by [`compare_field_values`], falling back
///   to [`Ordering::Equal`] to maintain stable relative order for incomparable
///   kinds.
pub(super) fn sort_key_cmp(
    a: &FieldValue,
    b: &FieldValue,
    descending: bool,
) -> Ordering {
    let ord = match (a, b) {
        (FieldValue::Null, FieldValue::Null) => Ordering::Equal,
        (FieldValue::Null, _) => Ordering::Less,
        (_, FieldValue::Null) => Ordering::Greater,
        _ => compare_field_values(a, b).unwrap_or(Ordering::Equal),
    };
    if descending {
        ord.reverse()
    } else {
        ord
    }
}
