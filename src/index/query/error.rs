//! Defines errors returned during field resolution and query transformations.

use thiserror::Error;

/// Represents errors encountered during field resolution or query
/// transformations.
///
/// These errors report malformed inputs, such as invalid field paths or filter
/// expressions. A well-formed field path for which a [`super::IndexRecord`]
/// has no value resolves to [`crate::note::FieldValue::Null`] rather than
/// producing an error.
#[derive(Clone, Debug, Eq, PartialEq, Error)]
pub(crate) enum QueryError {
    /// Indicates that a field path was empty, used an unknown accessor,
    /// or contained an unexpected structure.
    ///
    /// The `suggestion` field holds [`Some`] with the closest matching
    /// `file.*` or `task.*` accessor name when `path` resembles a typo,
    /// or [`None`] when no close match exists or `path` targets arbitrary
    /// frontmatter or inline fields.
    #[error(
        "invalid field path {path:?}; expected `file.<field>` (path, name, \
         folder, size, ctime, cdate, mtime, mdate), `task.<field>` \
         (completed, text), or a single frontmatter, inline field, or `tags` \
         name{}",
        suggestion.as_deref().map_or_else(String::new, |name| format!(
            " (did you mean `{name}`?)"
        ))
    )]
    UnknownFieldPath {
        /// The raw, unparsable field path string.
        path: String,
        /// The closest matching `file.*` or `task.*` accessor name when `path`
        /// resembles a typo of a known accessor (`Some`), or [`None`] when no
        /// close match exists.
        suggestion: Option<String>,
    },
    /// Indicates that a filter expression failed to match the expected
    /// `<field> <op> <value>` structure.
    #[error(
        "invalid filter expression {expr:?}; expected `<field> <op> <value>` \
         with op one of ==, !=, >=, <=, >, < and value a quoted string, \
         number, or boolean"
    )]
    UnparsableFilterExpression {
        /// The raw filter expression string that failed to parse.
        expr: String,
    },
    /// Indicates that a query limit count was negative or exceeded platform
    /// [`usize`] bounds.
    #[error("invalid limit {n}; expected a non-negative row count")]
    NegativeLimit {
        /// The rejected limit count value.
        n: i64,
    },
    /// Indicates that [`super::QueryOutcome::task_list`] was invoked on
    /// page-level records lacking task fields.
    ///
    /// Page-level records are constructed by
    /// [`super::super::FileIndex::query`], whereas task-list
    /// transformations require task-level records produced
    /// by [`super::super::FileIndex::query_tasks`].
    #[error(
        "task_list requires task-level records from the `tasks` namespace; \
         got page-level records with no task fields"
    )]
    TaskListOnPageRecords,
    /// Indicates that the `headers` and `columns` passed to
    /// [`super::QueryOutcome::table`] had unequal lengths.
    #[error(
        "table headers ({headers}) and columns ({columns}) must have the same \
         length"
    )]
    TableColumnMismatch {
        /// The number of header titles provided.
        headers: usize,
        /// The number of column data vectors provided.
        columns: usize,
    },
}

impl QueryError {
    /// Constructs a [`QueryError::UnparsableFilterExpression`] error for
    /// `expr`.
    ///
    /// This constructor is shared by the tokenizer and parser in
    /// [`super::filter`], where parse errors point at the entire filter
    /// expression rather than an individual sub-span.
    pub(in crate::index::query) fn unparsable_filter(expr: &str) -> Self {
        Self::UnparsableFilterExpression {
            expr: expr.to_owned(),
        }
    }

    /// Constructs a [`QueryError::UnknownFieldPath`] error for `path`,
    /// optionally attaching a did-you-mean `suggestion`.
    ///
    /// When `suggestion` is [`Some`], it supplies a known accessor name (such
    /// as `"file.name"`) to be rendered as a did-you-mean hint. When
    /// `suggestion` is [`None`], no close match exists.
    ///
    /// This constructor is shared across all [`super::field::FieldPath::parse`]
    /// failure sites: malformed `file.*` or `task.*` accessors supply their
    /// closest matching accessor name, whereas invalid frontmatter or inline
    /// field paths pass [`None`] because no fixed accessor list exists for
    /// custom fields.
    pub(in crate::index::query) fn unknown_field_path(
        path: &str,
        suggestion: Option<&str>,
    ) -> Self {
        Self::UnknownFieldPath {
            path: path.to_owned(),
            suggestion: suggestion.map(str::to_owned),
        }
    }
}

#[cfg(test)]
mod tests {
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
    fn unknown_field_path_formats_display_message() {
        let error = QueryError::UnknownFieldPath {
            path: "file.bogus".to_owned(),
            suggestion: None,
        };

        assert_display(
            &error,
            "invalid field path \"file.bogus\"; expected `file.<field>` \
             (path, name, folder, size, ctime, cdate, mtime, mdate), \
             `task.<field>` (completed, text), or a single frontmatter, \
             inline field, or `tags` name",
        );
    }

    #[test]
    fn unknown_field_path_appends_a_did_you_mean_suggestion_when_built_with_one()
     {
        let error =
            QueryError::unknown_field_path("file.nam", Some("file.name"));

        assert_display(
            &error,
            "invalid field path \"file.nam\"; expected `file.<field>` (path, \
             name, folder, size, ctime, cdate, mtime, mdate), `task.<field>` \
             (completed, text), or a single frontmatter, inline field, or \
             `tags` name (did you mean `file.name`?)",
        );
    }

    #[test]
    fn unparsable_filter_expression_formats_display_message() {
        let error = QueryError::UnparsableFilterExpression {
            expr: "rating >".to_owned(),
        };

        assert_display(
            &error,
            "invalid filter expression \"rating >\"; expected `<field> <op> \
             <value>` with op one of ==, !=, >=, <=, >, < and value a quoted \
             string, number, or boolean",
        );
    }

    #[test]
    fn negative_limit_formats_display_message() {
        let error = QueryError::NegativeLimit {
            n: -5,
        };

        assert_display(
            &error,
            "invalid limit -5; expected a non-negative row count",
        );
    }

    #[test]
    fn task_list_on_page_records_formats_display_message() {
        assert_display(
            &QueryError::TaskListOnPageRecords,
            "task_list requires task-level records from the `tasks` \
             namespace; got page-level records with no task fields",
        );
    }

    #[test]
    fn table_column_mismatch_formats_display_message() {
        assert_display(
            &QueryError::TableColumnMismatch {
                headers: 2,
                columns: 1,
            },
            "table headers (2) and columns (1) must have the same length",
        );
    }
}
