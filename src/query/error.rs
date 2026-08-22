//! Error types for query parsing, field resolution, and result transformation.
//!
//! This module defines the error hierarchy returned by [`super::QuerySource`]
//! parsing, [`super::QueryRecordSet`] transformation methods, and
//! [`super::QueryRecord`] field resolution.
//!
//! # Error Hierarchy and Integration
//!
//! - [`QueryError`] is the top-level error type.
//! - [`QueryRequestError`] isolates failures while building a query request.
//! - [`QuerySyntaxError`] handles syntax errors and integrates with [`miette`]
//!   using the [`Diagnostic`][`miette::Diagnostic`] trait to render rich
//!   diagnostics.
//! - [`FieldPathError`] represents invalid field paths or query namespace
//!   errors.
//! # Examples
//!
//! ```ignore
//! use traces_pkm::query::{QueryError, QueryRequestError};
//!
//! let error = QueryError::from(QueryRequestError::LimitOutOfRange {
//!     value: -5,
//! });
//! assert_eq!(
//!     error.to_string(),
//!     "invalid limit -5; expected a non-negative row count"
//! );
//! ```

use std::fmt;

use miette::{Diagnostic, SourceSpan};
use thiserror::Error;

/// Identifies the query language that rejected an expression.
///
/// Used by [`QuerySyntaxError`] to produce a human-readable message that names
/// the failing dialect (for example, "invalid filter expression").
///
/// # Examples
///
/// ```ignore
/// use traces_pkm::query::QueryDialect;
///
/// assert_eq!(QueryDialect::Source.to_string(), "source");
/// assert_eq!(QueryDialect::Filter.to_string(), "filter");
/// ```
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum QueryDialect {
    /// The `--from` source-selection language.
    Source,
    /// The `--where` record-filtering language.
    Filter,
}

impl fmt::Display for QueryDialect {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Source => formatter.write_str("source"),
            Self::Filter => formatter.write_str("filter"),
        }
    }
}

/// A syntax error in a source or filter expression.
///
/// Implements [`miette::Diagnostic`] to provide source-location-aware error
/// rendering with labeled spans and repair hints. The [`input`] field contains
/// the complete expression, and [`span`] pinpoints the invalid token range.
///
/// [`input`]: Self::input
/// [`span`]: Self::span
///
/// # Examples
///
/// ```ignore
/// # use miette::SourceSpan;
/// # use traces_pkm::query::error::{QueryDialect, QuerySyntaxError};
/// let error = QuerySyntaxError::new(
///     QueryDialect::Source,
///     "input",
///     SourceSpan::from((0, 5)),
///     "expected atom",
/// );
/// ```
#[derive(Clone, Debug, Diagnostic, Eq, Error, PartialEq)]
#[error("invalid {dialect} expression")]
pub struct QuerySyntaxError {
    /// The query language that rejected the expression.
    pub(crate) dialect: QueryDialect,
    /// The complete input expression.
    #[source_code]
    pub(crate) input: String,
    /// The invalid token range, or the end of input when a token is missing.
    #[label("{expected}")]
    pub(crate) span: SourceSpan,
    /// Concrete repair text supplied by the parser.
    pub(crate) expected: &'static str,
}

impl QuerySyntaxError {
    /// Constructs a syntax diagnostic for a single expression range.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// # use miette::SourceSpan;
    /// # use traces_pkm::query::error::{QueryDialect, QuerySyntaxError};
    /// let error = QuerySyntaxError::new(
    ///     QueryDialect::Source,
    ///     "input",
    ///     SourceSpan::from((0, 5)),
    ///     "expected atom",
    /// );
    /// ```
    pub(crate) fn new(
        dialect: QueryDialect,
        input: &str,
        span: SourceSpan,
        expected: &'static str,
    ) -> Self {
        Self {
            dialect,
            input: input.to_owned(),
            span,
            expected,
        }
    }
}

/// A malformed field path with an optional closest-accessor suggestion.
///
/// The error message lists all valid accessor prefixes (`file.<field>`,
/// `task.<field>`, frontmatter keys, `tags`, `inlinks`) and appends a "did you
/// mean" hint when the input resembles a known accessor.
///
/// # Examples
///
/// ```ignore
/// # use traces_pkm::query::error::FieldPathError;
/// let error = FieldPathError::new("file.nmae", Some("file.name"));
/// ```
#[derive(Clone, Debug, Eq, Error, PartialEq)]
#[error(
    "invalid field path {path:?}; expected `file.<field>` (path, name, \
     folder, size, ctime, cdate, mtime, mdate), `task.<field>` \
     (completed, text), or a single frontmatter, inline field, or `tags` \
     name{}",
    suggestion.as_deref().map_or_else(String::new, |name| format!(
        " (did you mean `{name}`?)"
    ))
)]
pub struct FieldPathError {
    /// The raw, unparsable field path string.
    pub(crate) path: String,
    /// The closest matching accessor when `path` resembles a typo.
    pub(crate) suggestion: Option<String>,
}

impl FieldPathError {
    /// Constructs a field-path error with an optional repair suggestion.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// # use traces_pkm::query::error::FieldPathError;
    /// let error = FieldPathError::new("file.nmae", Some("file.name"));
    /// ```
    pub(in crate::query) fn new(path: &str, suggestion: Option<&str>) -> Self {
        Self {
            path: path.to_owned(),
            suggestion: suggestion.map(str::to_owned),
        }
    }
}

/// Error while building a [`super::QueryRequest`].
///
/// Separates request-construction failures from execution/rendering failures
/// while still embedding into [`QueryError`] for callers that want one query
/// error type.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum QueryRequestError {
    /// A source or filter expression has invalid syntax.
    #[error(transparent)]
    Syntax(#[from] QuerySyntaxError),
    /// A field path cannot be parsed or names an unknown accessor.
    #[error(transparent)]
    FieldPath(#[from] FieldPathError),
    /// A query limit was negative or exceeded platform [`usize`] bounds.
    #[error("invalid limit {value}; expected a non-negative row count")]
    LimitOutOfRange {
        /// The rejected limit count.
        value: i64,
    },
}

impl QueryRequestError {
    pub(crate) fn from_query_error(error: QueryError) -> Self {
        match error {
            QueryError::Syntax(error) => Self::Syntax(error),
            QueryError::FieldPath(error) => Self::FieldPath(error),
            QueryError::Request(error) => error,
            QueryError::TaskListRequiresTaskRows
            | QueryError::TableColumnCountMismatch {
                ..
            } => Self::Syntax(QuerySyntaxError::new(
                QueryDialect::Filter,
                "",
                (0, 0).into(),
                "a valid filter expression",
            )),
        }
    }
}

/// Top-level error enum for query parsing and transformation.
///
/// Covers all failure modes from expression parsing through field resolution to
/// result rendering. Implements [`miette::Diagnostic`] by delegating to the
/// inner [`QuerySyntaxError`] for syntax errors.
///
/// # Examples
///
/// ```ignore
/// use traces_pkm::query::{QueryError, QueryRequestError};
///
/// let error = QueryError::from(QueryRequestError::LimitOutOfRange {
///     value: -1,
/// });
/// assert_eq!(
///     error.to_string(),
///     "invalid limit -1; expected a non-negative row count"
/// );
/// ```
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum QueryError {
    /// A request builder rejected syntax, field paths, or limits.
    #[error(transparent)]
    Request(#[from] QueryRequestError),
    /// A source or filter expression has invalid syntax.
    #[error(transparent)]
    Syntax(#[from] QuerySyntaxError),
    /// A field path cannot be parsed or names an unknown accessor.
    #[error(transparent)]
    FieldPath(#[from] FieldPathError),
    /// [`super::QueryRecordSet::task_list`] received page-level records
    #[error(
        "task_list requires task-level records from the `tasks` namespace; \
         got page-level records with no task fields"
    )]
    TaskListRequiresTaskRows,
    /// [`super::QueryRecordSet::table`] received `headers` and `columns` slices
    /// of unequal length.
    #[error(
        "table headers ({headers}) and columns ({columns}) must have the same \
         length"
    )]
    TableColumnCountMismatch {
        /// The number of header titles provided.
        headers: usize,
        /// The number of column field paths provided.
        columns: usize,
    },
}

/// Establishes diagnostic capabilities for `QueryError`.
impl Diagnostic for QueryError {
    fn diagnostic_source(&self) -> Option<&dyn Diagnostic> {
        match self {
            Self::Syntax(source)
            | Self::Request(QueryRequestError::Syntax(source)) => Some(source),
            Self::Request(
                QueryRequestError::FieldPath(_)
                | QueryRequestError::LimitOutOfRange {
                    ..
                },
            )
            | Self::FieldPath(_)
            | Self::TaskListRequiresTaskRows
            | Self::TableColumnCountMismatch {
                ..
            } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use miette::{Diagnostic, SourceSpan};
    use pretty_assertions::assert_eq;

    use super::*;

    fn assert_display(error: &QueryError, expected: &str) {
        assert_eq!(
            error.to_string(),
            expected,
            "unexpected QueryError display"
        );
    }

    #[test]
    fn syntax_error_preserves_dialect_input_span_and_label() {
        let error = QuerySyntaxError::new(
            QueryDialect::Filter,
            "rating >",
            SourceSpan::from((7, 0)),
            "a literal value",
        );

        assert_eq!(error.dialect, QueryDialect::Filter);
        assert_eq!(error.input, "rating >");
        assert_eq!(error.span, SourceSpan::from((7, 0)));
        assert_eq!(error.expected, "a literal value");
        assert_eq!(
            error
                .labels()
                .expect("syntax diagnostic has a label")
                .map(|label| (
                    label.offset(),
                    label.len(),
                    label.label().map(str::to_owned)
                ))
                .collect::<Vec<_>>(),
            vec![(7, 0, Some("a literal value".to_owned()))]
        );
        assert!(error.source_code().is_some());
    }

    #[test]
    fn query_error_exposes_nested_syntax_diagnostic() {
        let error = QueryError::from(QuerySyntaxError::new(
            QueryDialect::Source,
            "#book and",
            SourceSpan::from((9, 0)),
            "a source term",
        ));

        assert_eq!(error.to_string(), "invalid source expression");
        assert!(error.diagnostic_source().is_some());
    }

    #[test]
    fn field_path_error_formats_display_message() {
        let error = QueryError::from(FieldPathError::new("file.bogus", None));

        assert_display(
            &error,
            "invalid field path \"file.bogus\"; expected `file.<field>` \
             (path, name, folder, size, ctime, cdate, mtime, mdate), \
             `task.<field>` (completed, text), or a single frontmatter, \
             inline field, or `tags` name",
        );
    }

    #[test]
    fn field_path_error_appends_a_did_you_mean_suggestion() {
        let error = QueryError::from(FieldPathError::new(
            "file.nam",
            Some("file.name"),
        ));

        assert_display(
            &error,
            "invalid field path \"file.nam\"; expected `file.<field>` (path, \
             name, folder, size, ctime, cdate, mtime, mdate), `task.<field>` \
             (completed, text), or a single frontmatter, inline field, or \
             `tags` name (did you mean `file.name`?)",
        );
    }

    #[test]
    fn direct_operation_errors_format_display_messages() {
        assert_display(
            &QueryError::from(QueryRequestError::LimitOutOfRange {
                value: -5,
            }),
            "invalid limit -5; expected a non-negative row count",
        );
        assert_display(
            &QueryError::TaskListRequiresTaskRows,
            "task_list requires task-level records from the `tasks` \
             namespace; got page-level records with no task fields",
        );
        assert_display(
            &QueryError::TableColumnCountMismatch {
                headers: 2,
                columns: 1,
            },
            "table headers (2) and columns (1) must have the same length",
        );
    }
}
