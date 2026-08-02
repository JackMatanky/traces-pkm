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
         name"
    )]
    UnknownFieldPath {
        /// The unparsable field path.
        path: String,
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
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_display(error: &QueryError, expected: &str) {
        let actual = error.to_string();
        assert!(
            actual == expected,
            "unexpected QueryError display\nactual: {actual}\nexpected: \
             {expected}"
        );
    }

    #[test]
    fn unknown_field_path_formats_display_message() {
        let error = QueryError::UnknownFieldPath {
            path: "file.bogus".to_owned(),
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
}
