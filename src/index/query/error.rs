//! Errors returned by field resolution and query outcome transformations.

use thiserror::Error;

/// Errors returned during field resolution or query transformations.
///
/// These report malformed *inputs*: an unparsable field path or filter
/// expression. A well-formed field path that a given [`super::IndexRecord`]
/// simply does not have a value for resolves to
/// [`crate::note::FieldValue::Null`] instead of erroring.
#[derive(Clone, Debug, Eq, PartialEq, Error)]
pub(crate) enum QueryError {
    /// A field path was empty, used an unknown `file.*`/`task.*` accessor,
    /// or had unexpected `.` structure.
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
        /// The unparsable field path.
        path: String,
        /// The closest known `file.*`/`task.*` accessor name (e.g.
        /// `"file.name"`) when `path` looks like a typo of one; `None`
        /// otherwise. Built by [`Self::unknown_field_path`].
        suggestion: Option<String>,
    },
    /// A filter expression did not match `<field> <op> <value>`.
    #[error(
        "invalid filter expression {expr:?}; expected `<field> <op> <value>` \
         with op one of ==, !=, >=, <=, >, < and value a quoted string, \
         number, or boolean"
    )]
    UnparsableFilterExpression {
        /// The unparsable filter expression.
        expr: String,
    },
    /// A limit count was negative or exceeded platform [`usize`] bounds.
    #[error("invalid limit {n}; expected a non-negative row count")]
    NegativeLimit {
        /// The rejected limit value.
        n: i64,
    },
    /// [`super::QueryOutcome::task_list`] was called on records with no
    /// `task.*` fields, meaning page-level records built by
    /// [`super::super::FileIndex::query`] rather than task-level records
    /// from [`super::super::FileIndex::query_tasks`].
    #[error(
        "task_list requires task-level records from the `tasks` namespace; \
         got page-level records with no task fields"
    )]
    TaskListOnPageRecords,
    /// [`super::QueryOutcome::table`]'s `headers` and `columns` had different
    /// lengths.
    #[error(
        "table headers ({headers}) and columns ({columns}) must have the same \
         length"
    )]
    TableColumnMismatch {
        /// Number of entries in `headers`.
        headers: usize,
        /// Number of entries in `columns`.
        columns: usize,
    },
}

impl QueryError {
    /// Builds an [`Self::UnparsableFilterExpression`] for the full `expr`.
    ///
    /// Shared by [`super::filter`]'s tokenizer and parser, whose parse
    /// errors always point at the entire expression rather than a
    /// sub-span.
    pub(in crate::index::query) fn unparsable_filter(expr: &str) -> Self {
        Self::UnparsableFilterExpression {
            expr: expr.to_owned(),
        }
    }

    /// Builds an [`Self::UnknownFieldPath`] for `path`, with `suggestion`
    /// (a known accessor name such as `"file.name"`) rendered into a "did
    /// you mean" hint by [`Self::UnknownFieldPath`]'s `#[error(...)]`
    /// message when given.
    ///
    /// Shared by every [`super::field::FieldPath::parse`] failure site: a
    /// malformed `file.<field>`/`task.<field>` accessor passes its closest
    /// match (see `closest_accessor` in that module); every other malformed
    /// path passes `None`, since there is no fixed accessor list to compare
    /// arbitrary frontmatter/inline-field keys against.
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
