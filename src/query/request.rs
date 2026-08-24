//! Query execution request and transform plan.

use super::{
    QueryRequestError,
    grammar::{FieldPath, FilterExpr, SourceSelector},
};

/// Query execution request.
pub struct QueryRequest {
    pub(super) mode: QueryMode,
    pub(super) source: SourceSelector,
    pub(super) transforms: Vec<QueryTransform>,
}

impl QueryRequest {
    /// Builds a page-row query request for `source`.
    #[inline]
    #[must_use]
    pub fn pages(source: SourceSelector) -> Self {
        Self {
            mode: QueryMode::Pages,
            source,
            transforms: Vec::new(),
        }
    }

    /// Builds a task-row query request for `source`.
    #[inline]
    #[must_use]
    pub fn tasks(source: SourceSelector) -> Self {
        Self {
            mode: QueryMode::Tasks,
            source,
            transforms: Vec::new(),
        }
    }

    /// Appends a parsed filter transform.
    ///
    /// # Errors
    ///
    /// Returns [`QueryRequestError`] when `expr` is not a valid filter.
    #[cfg(test)]
    pub(crate) fn filter(
        mut self,
        expr: &str,
    ) -> Result<Self, QueryRequestError> {
        self.transforms.push(QueryTransform::filter(expr)?);
        Ok(self)
    }

    /// Appends a parsed sort transform.
    ///
    /// # Errors
    ///
    /// Returns [`QueryRequestError`] when `field` is not a valid field path.
    #[cfg(test)]
    pub(crate) fn sort(
        mut self,
        field: &str,
        descending: bool,
    ) -> Result<Self, QueryRequestError> {
        self.transforms.push(QueryTransform::sort(field, descending)?);
        Ok(self)
    }

    /// Appends a parsed limit transform.
    ///
    /// # Errors
    ///
    /// Returns [`QueryRequestError`] when `n` is negative or too large for this
    /// platform.
    #[cfg(test)]
    pub(crate) fn limit(mut self, n: i64) -> Result<Self, QueryRequestError> {
        self.transforms.push(QueryTransform::limit(n)?);
        Ok(self)
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(super) enum QueryMode {
    Pages,
    Tasks,
}

pub(super) enum QueryTransform {
    Filter(FilterExpr),
    Sort {
        field: FieldPath,
        descending: bool,
    },
    Limit(usize),
    GroupBy(FieldPath),
    Flatten(FieldPath),
}

impl QueryTransform {
    pub(super) fn filter(expr: &str) -> Result<Self, QueryRequestError> {
        Ok(Self::Filter(
            FilterExpr::parse(expr)
                .map_err(QueryRequestError::from_query_error)?,
        ))
    }

    pub(super) fn sort(
        field: &str,
        descending: bool,
    ) -> Result<Self, QueryRequestError> {
        Ok(Self::Sort {
            field: FieldPath::parse(field)?,
            descending,
        })
    }

    pub(super) fn limit(n: i64) -> Result<Self, QueryRequestError> {
        let n = usize::try_from(n).map_err(|_source| {
            QueryRequestError::LimitOutOfRange {
                value: n,
            }
        })?;
        Ok(Self::Limit(n))
    }

    pub(super) fn group_by(field: &str) -> Result<Self, QueryRequestError> {
        Ok(Self::GroupBy(FieldPath::parse(field)?))
    }

    pub(super) fn flatten(field: &str) -> Result<Self, QueryRequestError> {
        Ok(Self::Flatten(FieldPath::parse(field)?))
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, path::Path};

    use super::*;
    use crate::{
        index::IndexerService,
        query::{FieldPathError, QueryService},
    };

    mod query_request {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn query_request_preserves_transform_order() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("a.md"), "---\nrating: 1\n---\n")
                .expect("write a.md");
            fs::write(temp.path().join("b.md"), "---\nrating: 5\n---\n")
                .expect("write b.md");
            fs::write(temp.path().join("c.md"), "---\nrating: 9\n---\n")
                .expect("write c.md");
            let index =
                IndexerService::new(temp.path()).build().expect("build index");
            let request = QueryRequest::pages(SourceSelector::All)
                .limit(2)
                .expect("valid limit")
                .filter("rating >= 5")
                .expect("valid filter");

            let outcome = QueryService::new("class").execute(&index, request);

            assert_eq!(outcome.len(), 1);
            assert_eq!(
                outcome.get(0).expect("row").base().path(),
                Path::new("b.md")
            );
        }

        #[test]
        fn wraps_request_builder_errors() {
            assert!(matches!(
                QueryRequest::pages(SourceSelector::All).filter("rating >"),
                Err(QueryRequestError::Syntax(_))
            ));
            assert_eq!(
                QueryRequest::pages(SourceSelector::All)
                    .sort("file.bogus", false)
                    .err(),
                Some(QueryRequestError::FieldPath(FieldPathError::new(
                    "file.bogus",
                    None
                )))
            );
            assert_eq!(
                QueryRequest::pages(SourceSelector::All).limit(-1).err(),
                Some(QueryRequestError::LimitOutOfRange {
                    value: -1
                })
            );
        }

        #[test]
        fn leaves_class_source_empty_without_an_expander() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("book.md"), "---\nclass: book\n---\n")
                .expect("write book.md");
            let index =
                IndexerService::new(temp.path()).build().expect("build index");
            let request = QueryRequest::pages(
                SourceSelector::parse("@book").expect("source"),
            );

            let outcome = QueryService::new("class").execute(&index, request);

            assert!(outcome.is_empty());
        }
    }
}
