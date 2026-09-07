//! Query parsing, field-resolution, and result-transformation errors.
//!
//! Primary errors:
//!
//! - [`QueryError`]: top-level query failure.
//! - [`QueryBuilderError`]: request-construction failure.
//! - [`QuerySyntaxError`]: syntax error with [`miette::Diagnostic`] spans.
//! - [`FieldPathError`]: invalid field path with typo suggestions.

use std::fmt;

use miette::{Diagnostic, SourceSpan};
use thiserror::Error;

use crate::LexError;

/// Result alias carrying `QueryError`.
pub type QueryResult<T> = std::result::Result<T, QueryError>;

/// Top-level query parsing and transformation error.
///
/// Delegates [`miette::Diagnostic`] to the inner [`QuerySyntaxError`] for
/// syntax failures.
#[derive(Clone, Debug, Eq, PartialEq, Error)]
pub enum QueryError {
    /// Builder-construction failure.
    #[error(transparent)]
    Builder(#[from] QueryBuilderError),
    /// Source or filter expression syntax failure.
    #[error(transparent)]
    Syntax(#[from] QuerySyntaxError),
    /// Field-path parse or accessor failure.
    #[error(transparent)]
    FieldPath(#[from] FieldPathError),
    /// [`super::QuerySet::task_list`] received page-level records instead of
    /// task rows.
    #[error(
        "task_list requires task-level records from the `tasks` namespace; \
         got page-level records with no task fields"
    )]
    TaskListRequiresTaskRows,
    /// [`super::QuerySet::table`] received mismatched header and column counts.
    #[error(
        "table headers ({headers}) and columns ({columns}) must have the same \
         length"
    )]
    TableColumnCountMismatch {
        /// Header count.
        headers: usize,
        /// Column count.
        columns: usize,
    },
}

impl Diagnostic for QueryError {
    fn diagnostic_source(&self) -> Option<&dyn Diagnostic> {
        match self {
            Self::Syntax(source)
            | Self::Builder(QueryBuilderError::Syntax(source)) => Some(source),
            Self::Builder(
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
/// Separates request-construction failures from execution/rendering failures.
#[derive(Clone, Debug, Eq, PartialEq, Error)]
pub enum QueryBuilderError {
    /// Source or filter expression syntax failure.
    #[error(transparent)]
    Syntax(#[from] QuerySyntaxError),
    /// Field-path parse or accessor failure.
    #[error(transparent)]
    FieldPath(#[from] FieldPathError),
    /// Query limit is negative or exceeds platform [`usize`] bounds.
    #[error("invalid limit {value}; expected a non-negative row count")]
    LimitOutOfRange {
        /// The rejected limit count.
        value: i64,
    },
}

/// Syntax error in a source or filter expression.
///
/// Provides [`miette::Diagnostic`] spans and repair hints from the underlying
/// `LexError`.
#[derive(Clone, Debug, Eq, PartialEq, Diagnostic, Error)]
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
    /// Lexer error rendered in the diagnostic label.
    #[source]
    pub(crate) lex_error: Box<LexError>,
}

impl QuerySyntaxError {
    /// Builds an unexpected-end diagnostic for `span`.
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

    /// Preserves `lex_error`'s span as the diagnostic label.
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

/// Malformed field path with an optional closest-accessor suggestion.
///
/// Formats the accepted accessor prefixes and "did you mean" hint for display.
#[derive(Clone, Debug, Eq, PartialEq, Error)]
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
    /// Original field path string.
    pub(crate) path: String,
    /// Closest matching accessor for typo hints.
    pub(crate) suggestion: Option<String>,
}

impl FieldPathError {
    pub(in crate::query) fn new(path: &str, suggestion: Option<&str>) -> Self {
        Self {
            path: path.to_owned(),
            suggestion: suggestion.map(str::to_owned),
        }
    }
}

/// Query language that rejected an expression.
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
