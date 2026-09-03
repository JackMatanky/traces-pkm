//! Error types for query parsing, field resolution, and result transformation.
//!
//! The error hierarchy:
//!
//! - [`QueryError`]: top-level error type covering all query failures.
//! - [`QueryBuilderError`]: isolates failures during request construction.
//! - [`QuerySyntaxError`]: syntax errors with [`miette::Diagnostic`]
//!   integration for rich source-location-aware rendering.
//! - [`FieldPathError`]: invalid field paths with "did you mean" suggestions.

use std::fmt;

use miette::{Diagnostic, SourceSpan};
use thiserror::Error;

use crate::LexError;

/// Convenience alias for query operations that may fail.
pub type QueryResult<T> = std::result::Result<T, QueryError>;

/// Top-level error enum for query parsing and transformation.
///
/// Covers all failure modes from expression parsing through field resolution to
/// result rendering. Implements [`miette::Diagnostic`] by delegating to the
/// inner [`QuerySyntaxError`] for syntax errors.
///
/// # Examples
///
/// ```ignore
/// use traces_pkm::query::{QueryBuilderError, QueryError};
///
/// let error = QueryError::from(QueryBuilderError::LimitOutOfRange {
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
    Request(#[from] QueryBuilderError),
    /// A source or filter expression has invalid syntax.
    #[error(transparent)]
    Syntax(#[from] QuerySyntaxError),
    /// A field path cannot be parsed or names an unknown accessor.
    #[error(transparent)]
    FieldPath(#[from] FieldPathError),
    /// [`super::QuerySet::task_list`] received page-level records
    #[error(
        "task_list requires task-level records from the `tasks` namespace; \
         got page-level records with no task fields"
    )]
    TaskListRequiresTaskRows,
    /// [`super::QuerySet::table`] received `headers` and `columns` slices
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
            | Self::Request(QueryBuilderError::Syntax(source)) => Some(source),
            Self::Request(
                QueryBuilderError::FieldPath(_)
                | QueryBuilderError::LimitOutOfRange {
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

/// Error while building a [`super::QueryBuilder`].
///
/// Separates builder-construction failures from execution/rendering failures
/// while still embedding into [`QueryError`] for callers that want one query
/// error type.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum QueryBuilderError {
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
    #[label("{lex_error}")]
    pub(crate) span: SourceSpan,
    /// The underlying lexer error carrying diagnostic context.
    #[source]
    pub(crate) lex_error: Box<LexError>,
}

impl QuerySyntaxError {
    /// Constructs a syntax diagnostic for a single expression range.
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
            lex_error: Box::new(LexError::UnexpectedEndOfInput {
                span,
                expected,
            }),
        }
    }

    /// Wraps a [`LexError`] into a syntax diagnostic.
    pub(crate) fn from_lex(
        dialect: QueryDialect,
        input: &str,
        lex_error: LexError,
    ) -> Self {
        let span = lex_error.span();
        Self {
            dialect,
            input: input.to_owned(),
            span,
            lex_error: Box::new(lex_error),
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

#[cfg(test)]
mod tests {
    use miette::{Diagnostic, SourceSpan};

    use super::*;

    fn assert_display(error: &QueryError, expected: &str) {
        assert_eq!(
            error.to_string(),
            expected,
            "unexpected QueryError display"
        );
    }

    mod syntax_error {
        use super::*;

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
            assert_eq!(*error.lex_error, LexError::UnexpectedEndOfInput {
                span: SourceSpan::from((7, 0)),
                expected: "a literal value",
            });
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
                vec![(
                    7,
                    0,
                    Some(
                        "unexpected end of input, expected a literal value"
                            .to_owned()
                    )
                )]
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
    }

    mod field_path_error {
        use super::*;

        #[test]
        fn field_path_error_formats_display_message() {
            let error =
                QueryError::from(FieldPathError::new("file.bogus", None));

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
                "invalid field path \"file.nam\"; expected `file.<field>` \
                 (path, name, folder, size, ctime, cdate, mtime, mdate), \
                 `task.<field>` (completed, text), or a single frontmatter, \
                 inline field, or `tags` name (did you mean `file.name`?)",
            );
        }
    }

    mod display {
        use super::*;

        #[test]
        fn limit_out_of_range_formats_display_message() {
            assert_display(
                &QueryError::from(QueryBuilderError::LimitOutOfRange {
                    value: -5,
                }),
                "invalid limit -5; expected a non-negative row count",
            );
        }

        #[test]
        fn task_list_requires_task_rows_formats_display_message() {
            assert_display(
                &QueryError::TaskListRequiresTaskRows,
                "task_list requires task-level records from the `tasks` \
                 namespace; got page-level records with no task fields",
            );
        }

        #[test]
        fn table_column_count_mismatch_formats_display_message() {
            assert_display(
                &QueryError::TableColumnCountMismatch {
                    headers: 2,
                    columns: 1,
                },
                "table headers (2) and columns (1) must have the same length",
            );
        }
    }
}
