//! Declarative query builder for source selection, row granularity, and
//! transformations.
//!
//! This module provides [`QueryBuilder`], the primary user-facing builder for
//! constructing index queries. A query combines a [`SourceSelector`], a
//! [`QueryMode`] governing row granularity ([`QueryBuilder::pages`] vs.
//! [`QueryBuilder::tasks`]), and a sequence of pending transformations
//! including filters, sorts, and limits.
//!
//! Plans remain inert specifications until passed to
//! [`QueryService::run`](super::QueryService::run) or
//! [`QueryService::run_from_store`](super::QueryService::run_from_store).

use super::{
    QueryBuilderError, QueryPlan, QueryTransform, grammar::SourceSelector,
    sort::SortOrder,
};

/// Row granularity produced by a [`QueryBuilder`].
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum QueryMode {
    /// One row per matching note.
    Pages,
    /// One row per list item in each matching note (plain bullets, checkboxes,
    /// and tasks).
    Lists,
    /// One row per task list item in each matching note.
    Tasks,
}

/// Declarative query specification for index queries.
///
/// Plans stay inert until [`QueryService::run`](super::QueryService::run)
/// evaluates them against a borrowed [`FileIndex`](crate::index::FileIndex).
///
/// # Examples
///
/// ```rust
/// # #[cfg(feature = "test-utils")]
/// # {
/// use traces_pkm::{QueryBuilder, SourceSelector};
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let builder = QueryBuilder::pages(SourceSelector::All)
///     .filter("rating >= 4")?
///     .sort("file.name", false)?
///     .limit(10)?;
/// # Ok(())
/// # }
/// # }
/// ```
#[derive(Clone, Debug, PartialEq)]
pub struct QueryBuilder {
    mode: QueryMode,
    source: SourceSelector,
    plan: QueryPlan,
}

impl QueryBuilder {
    /// Builds a page-row query that emits one row per matching note.
    #[inline]
    #[must_use]
    pub fn pages(source: SourceSelector) -> Self {
        Self {
            mode: QueryMode::Pages,
            source,
            plan: QueryPlan::default(),
        }
    }

    /// Builds a list-row query that emits one row per list item (plain
    /// bullets, checkboxes, and tasks) in each matching note.
    #[inline]
    #[must_use]
    pub fn lists(source: SourceSelector) -> Self {
        Self {
            mode: QueryMode::Lists,
            source,
            plan: QueryPlan::default(),
        }
    }

    /// Builds a task-row query that emits one row per task list item.
    #[inline]
    #[must_use]
    pub fn tasks(source: SourceSelector) -> Self {
        Self {
            mode: QueryMode::Tasks,
            source,
            plan: QueryPlan::default(),
        }
    }

    /// Builds a query for `mode`, dispatching to the mode-specific
    /// constructor so callers can hold a bare [`QueryMode`] without knowing
    /// which constructor it selects.
    #[inline]
    #[must_use]
    pub(crate) fn from_mode(mode: QueryMode, source: SourceSelector) -> Self {
        match mode {
            QueryMode::Pages => Self::pages(source),
            QueryMode::Lists => Self::lists(source),
            QueryMode::Tasks => Self::tasks(source),
        }
    }

    /// Appends a filter expression to the query transform plan.
    ///
    /// Evaluates `expr` against candidate note frontmatter, task metadata,
    /// tags, and file properties when the query executes.
    ///
    /// # Errors
    ///
    /// - [`QueryBuilderError::Syntax`] if `expr` cannot be parsed as a valid
    ///   boolean filter expression.
    /// - [`QueryBuilderError::FieldPath`] if `expr` references an invalid or
    ///   malformed field path.
    #[inline]
    pub fn filter(mut self, expr: &str) -> Result<Self, QueryBuilderError> {
        self.plan.push(QueryTransform::filter(expr)?);
        Ok(self)
    }

    /// Appends a sort transform on `field`.
    ///
    /// Set `descending` to reverse natural ascending order.
    ///
    /// # Errors
    ///
    /// - [`QueryBuilderError::FieldPath`] if `field` cannot be parsed as a
    ///   valid field path.
    #[cfg_attr(
        not(any(test, feature = "test-utils")),
        expect(
            dead_code,
            reason = "public builder method on QueryBuilder; exercised in \
                      tests and test-utils"
        )
    )]
    #[inline]
    pub fn sort(
        mut self,
        field: &str,
        descending: bool,
    ) -> Result<Self, QueryBuilderError> {
        self.plan.push(QueryTransform::sort(field, descending)?);
        Ok(self)
    }

    /// Appends a composite sort order transform to the query transform plan.
    #[inline]
    pub(crate) fn order(mut self, order: SortOrder) -> Self {
        self.plan.push(QueryTransform::order(order));
        self
    }

    /// Appends a limit that retains at most `n` leading rows.
    ///
    /// # Errors
    ///
    /// - [`QueryBuilderError::LimitOutOfRange`] if `n` is negative or exceeds
    ///   [`usize::MAX`].
    #[inline]
    #[cfg_attr(
        not(any(test, feature = "test-utils")),
        expect(
            dead_code,
            reason = "public builder method on QueryBuilder; exercised in \
                      tests and test-utils"
        )
    )]
    pub fn limit(mut self, n: i64) -> Result<Self, QueryBuilderError> {
        self.plan.push(QueryTransform::limit(n)?);
        Ok(self)
    }

    pub(super) fn into_parts(self) -> (QueryMode, SourceSelector, QueryPlan) {
        (self.mode, self.source, self.plan)
    }
}
#[cfg(test)]
mod tests {
    use std::{fs, path::Path, sync::Arc};

    use super::*;
    use crate::{
        IndexerService,
        query::{FieldPathError, QueryService},
    };

    mod query_builder {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn query_builder_preserves_transform_order() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("a.md"), "---\nrating: 1\n---\n")
                .expect("write a.md");
            fs::write(temp.path().join("b.md"), "---\nrating: 5\n---\n")
                .expect("write b.md");
            fs::write(temp.path().join("c.md"), "---\nrating: 9\n---\n")
                .expect("write c.md");
            let index = Arc::new(
                IndexerService::new(temp.path()).build().expect("build index"),
            );
            let request = QueryBuilder::pages(SourceSelector::All)
                .limit(2)
                .expect("valid limit")
                .filter("rating >= 5")
                .expect("valid filter");

            let outcome = QueryService::new("class").run(&index, request);

            assert_eq!(outcome.len(), 1);
            assert_eq!(
                outcome.get(0).expect("row").file().path(),
                Path::new("b.md")
            );
        }

        #[test]
        fn sort_then_limit_matches_full_sort_order() {
            let temp = tempfile::tempdir().expect("create temp dir");
            for (name, rating) in [
                ("a.md", 3),
                ("b.md", 9),
                ("c.md", 1),
                ("d.md", 7),
                ("e.md", 5),
            ] {
                fs::write(
                    temp.path().join(name),
                    format!("---\nrating: {rating}\n---\n"),
                )
                .expect("write note");
            }
            let index = Arc::new(
                IndexerService::new(temp.path()).build().expect("build index"),
            );
            let request = QueryBuilder::pages(SourceSelector::All)
                .sort("rating", true)
                .expect("valid sort")
                .limit(2)
                .expect("valid limit");

            let outcome = QueryService::new("class").run(&index, request);

            assert_eq!(outcome.len(), 2);
            assert_eq!(
                outcome.get(0).expect("row").file().path(),
                Path::new("b.md")
            );
            assert_eq!(
                outcome.get(1).expect("row").file().path(),
                Path::new("d.md")
            );
        }

        #[test]
        fn top_k_matches_full_sort_order_for_tied_keys() {
            let temp = tempfile::tempdir().expect("create temp dir");
            // Low-cardinality keys force ties through top-k partitioning,
            // catching accidental reliance on small-slice or stable behavior.
            for i in 0..200 {
                fs::write(
                    temp.path().join(format!("note-{i:03}.md")),
                    format!("---\nrating: {}\n---\n", i % 4),
                )
                .expect("write note");
            }
            let index = Arc::new(
                IndexerService::new(temp.path()).build().expect("build index"),
            );

            for n in [5_usize, 50, 100] {
                let topk_request = QueryBuilder::pages(SourceSelector::All)
                    .sort("rating", false)
                    .expect("valid sort")
                    .limit(i64::try_from(n).expect("limit fits i64"))
                    .expect("valid limit");
                let topk_outcome =
                    QueryService::new("class").run(&index, topk_request);
                let topk_paths: Vec<_> = (0..topk_outcome.len())
                    .map(|i| {
                        topk_outcome
                            .get(i)
                            .expect("row")
                            .file()
                            .path()
                            .to_path_buf()
                    })
                    .collect();

                let full_sort_request =
                    QueryBuilder::pages(SourceSelector::All)
                        .sort("rating", false)
                        .expect("valid sort");
                let full_outcome =
                    QueryService::new("class").run(&index, full_sort_request);
                let full_first_n: Vec<_> = (0..n)
                    .map(|i| {
                        full_outcome
                            .get(i)
                            .expect("row")
                            .file()
                            .path()
                            .to_path_buf()
                    })
                    .collect();

                assert_eq!(
                    topk_paths, full_first_n,
                    "TopK(n={n}) must match a full stable sort's first {n} \
                     rows, including tie order"
                );
            }
        }
        #[test]
        fn filter_between_sort_and_limit_blocks_top_k_fusion() {
            let temp = tempfile::tempdir().expect("create temp dir");
            for (name, rating) in [
                ("a.md", 3),
                ("b.md", 9),
                ("c.md", 1),
                ("d.md", 7),
                ("e.md", 5),
            ] {
                fs::write(
                    temp.path().join(name),
                    format!("---\nrating: {rating}\n---\n"),
                )
                .expect("write note");
            }
            let index = Arc::new(
                IndexerService::new(temp.path()).build().expect("build index"),
            );
            // Fusing sort+limit across the filter would return only [b].
            // Correct order leaves [b, e].
            let request = QueryBuilder::pages(SourceSelector::All)
                .sort("rating", true)
                .expect("valid sort")
                .filter("file.path != \"d.md\"")
                .expect("valid filter")
                .limit(2)
                .expect("valid limit");

            let outcome = QueryService::new("class").run(&index, request);

            assert_eq!(outcome.len(), 2);
            assert_eq!(
                outcome.get(0).expect("row").file().path(),
                Path::new("b.md")
            );
            assert_eq!(
                outcome.get(1).expect("row").file().path(),
                Path::new("e.md")
            );
        }

        #[test]
        fn filter_fusion_matches_sequential_filters() {
            let temp = tempfile::tempdir().expect("create temp dir");
            for (name, rating) in [
                ("a.md", 1),
                ("b.md", 3),
                ("c.md", 5),
                ("d.md", 7),
                ("e.md", 9),
            ] {
                fs::write(
                    temp.path().join(name),
                    format!("---\nrating: {rating}\n---\n"),
                )
                .expect("write note");
            }
            let index = Arc::new(
                IndexerService::new(temp.path()).build().expect("build index"),
            );

            let fused_request = QueryBuilder::pages(SourceSelector::All)
                .filter("rating > 2")
                .expect("valid filter")
                .filter("rating < 8")
                .expect("valid filter");

            let fused_outcome =
                QueryService::new("class").run(&index, fused_request);

            let combined_request = QueryBuilder::pages(SourceSelector::All)
                .filter("rating > 2 and rating < 8")
                .expect("valid filter");
            let combined_outcome =
                QueryService::new("class").run(&index, combined_request);

            assert_eq!(fused_outcome, combined_outcome);
            assert_eq!(fused_outcome.len(), 3);
            let paths: Vec<&Path> =
                fused_outcome.iter().map(|r| r.file().path()).collect();
            assert_eq!(paths, [
                Path::new("b.md"),
                Path::new("c.md"),
                Path::new("d.md")
            ]);
        }

        #[test]
        fn returns_syntax_error_for_invalid_filter_expression() {
            assert!(matches!(
                QueryBuilder::pages(SourceSelector::All).filter("rating >"),
                Err(QueryBuilderError::Syntax(_))
            ));
        }

        #[test]
        fn returns_field_path_error_for_invalid_sort_field() {
            assert_eq!(
                QueryBuilder::pages(SourceSelector::All)
                    .sort("file.zzzz", false)
                    .err(),
                Some(QueryBuilderError::FieldPath(FieldPathError::new(
                    "file.zzzz",
                    None
                )))
            );
        }

        #[test]
        fn returns_limit_out_of_range_error_for_negative_limit() {
            assert_eq!(
                QueryBuilder::pages(SourceSelector::All).limit(-1).err(),
                Some(QueryBuilderError::LimitOutOfRange {
                    value: -1
                })
            );
        }

        #[test]
        fn leaves_class_source_empty_without_an_expander() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("book.md"), "---\nclass: book\n---\n")
                .expect("write book.md");
            let index = Arc::new(
                IndexerService::new(temp.path()).build().expect("build index"),
            );
            let request = QueryBuilder::pages(
                SourceSelector::parse("@book").expect("source"),
            );

            let outcome = QueryService::new("class").run(&index, request);

            assert!(outcome.is_empty());
        }

        #[test]
        fn lists_builder_constructs_query_with_lists_mode() {
            let (mode, source, plan) =
                QueryBuilder::lists(SourceSelector::All).into_parts();

            assert_eq!(mode, QueryMode::Lists);
            assert_eq!(source, SourceSelector::All);
            assert!(plan.is_empty());
        }
    }
}
