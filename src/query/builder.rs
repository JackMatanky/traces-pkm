//! Declarative query builder for source selection, execution mode, and
//! transform pipelines.
//!
//! Defines [`QueryBuilder`], which configures index query execution before
//! passing the request to
//! [`QueryService::execute`](super::QueryService::execute).

use super::{
    QueryBuilderError, QueryPlan, QueryTransform, grammar::SourceSelector,
};

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(super) enum QueryMode {
    Pages,
    Tasks,
}

/// Declarative query specification for index queries.
///
/// `QueryBuilder` specifies whether to return page-level rows
/// ([`pages`](Self::pages)) or task-level rows ([`tasks`](Self::tasks)),
/// selects candidate files via a [`SourceSelector`], and builds an ordered
/// sequence of transformation steps ([`filter`](Self::filter),
/// [`sort`](Self::sort), [`limit`](Self::limit)).
///
/// # Execution Lifecycle
///
/// Constructing a `QueryBuilder` does not touch the filesystem or execute query
/// expressions. The builder accumulates transformation steps into an internal
/// [`QueryPlan`](super::QueryPlan) and passes them to
/// [`QueryService::execute`](super::QueryService::execute), which evaluates the
/// plan against a borrowed [`FileIndex`](crate::index::FileIndex).
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
    /// Builds a page-row query builder for `source`.
    ///
    /// Page-level queries evaluate candidate files against `source` and produce
    /// one [`QueryRow`](super::QueryRow) per matching note.
    #[inline]
    #[must_use]
    pub fn pages(source: SourceSelector) -> Self {
        Self {
            mode: QueryMode::Pages,
            source,
            plan: QueryPlan::default(),
        }
    }

    /// Builds a task-row query builder for `source`.
    ///
    /// Task-level queries evaluate candidate files against `source` and produce
    /// one [`QueryRow`](super::QueryRow) per task list item in each matching
    /// note.
    #[inline]
    #[must_use]
    pub fn tasks(source: SourceSelector) -> Self {
        Self {
            mode: QueryMode::Tasks,
            source,
            plan: QueryPlan::default(),
        }
    }

    /// Appends a filter expression to the query transform plan.
    ///
    /// Evaluates `expr` against candidate note frontmatter, task metadata,
    /// tags, and file properties when the query executes.
    ///
    /// # Errors
    ///
    /// - [`Syntax`] if `expr` cannot be parsed as a valid boolean filter
    ///   expression.
    /// - [`FieldPath`] if `expr` references an invalid or malformed field path.
    ///
    /// [`Syntax`]: QueryBuilderError::Syntax
    /// [`FieldPath`]: QueryBuilderError::FieldPath
    #[inline]
    pub fn filter(mut self, expr: &str) -> Result<Self, QueryBuilderError> {
        self.plan.push(QueryTransform::filter(expr)?);
        Ok(self)
    }

    /// Appends a sort transform for `field` to the query transform plan.
    ///
    /// Sorts matching rows in ascending order when `descending` is `false`, or
    /// descending order when `descending` is `true`.
    ///
    /// # Errors
    ///
    /// - [`FieldPath`] if `field` cannot be parsed as a valid field path.
    ///
    /// [`FieldPath`]: QueryBuilderError::FieldPath
    #[inline]
    pub fn sort(
        mut self,
        field: &str,
        descending: bool,
    ) -> Result<Self, QueryBuilderError> {
        self.plan.push(QueryTransform::sort(field, descending)?);
        Ok(self)
    }

    /// Appends a limit transform to restrict the outcome to at most `n` leading
    /// rows.
    ///
    /// Retains up to `n` rows from the evaluated result set.
    ///
    /// # Errors
    ///
    /// - [`LimitOutOfRange`] if `n` is negative or exceeds `usize::MAX`.
    ///
    /// [`LimitOutOfRange`]: QueryBuilderError::LimitOutOfRange
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

    /// Splits this builder into its mode, source, and transform plan for
    /// [`super::QueryService::execute`].
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

            let outcome = QueryService::new("class").execute(&index, request);

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

            let outcome = QueryService::new("class").execute(&index, request);

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
            // 200 notes across only 4 distinct rating values (a
            // low-cardinality field like `status` at PKM scale), large enough
            // to exercise `select_nth_unstable_by`'s real partitioning logic
            // (not a small-slice fast path that could coincidentally
            // preserve order without a stability guarantee).
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
                    QueryService::new("class").execute(&index, topk_request);
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
                let full_outcome = QueryService::new("class")
                    .execute(&index, full_sort_request);
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
            // Sorted descending by rating: b(9), d(7), e(5), a(3), c(1).
            // A naive fusion would pick the top 2 (b, d) before the filter
            // removes d, leaving only [b]. The filter must run between the
            // sort and the limit, so the correct result is [b, e].
            let request = QueryBuilder::pages(SourceSelector::All)
                .sort("rating", true)
                .expect("valid sort")
                .filter("file.path != \"d.md\"")
                .expect("valid filter")
                .limit(2)
                .expect("valid limit");

            let outcome = QueryService::new("class").execute(&index, request);

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

            // Build sequential filters (simulating multiple --where flags)
            let fused_request = QueryBuilder::pages(SourceSelector::All)
                .filter("rating > 2")
                .expect("valid filter")
                .filter("rating < 8")
                .expect("valid filter");

            let fused_outcome =
                QueryService::new("class").execute(&index, fused_request);

            // Single combined filter for comparison
            let combined_request = QueryBuilder::pages(SourceSelector::All)
                .filter("rating > 2 and rating < 8")
                .expect("valid filter");
            let combined_outcome =
                QueryService::new("class").execute(&index, combined_request);

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
                    .sort("file.bogus", false)
                    .err(),
                Some(QueryBuilderError::FieldPath(FieldPathError::new(
                    "file.bogus",
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

            let outcome = QueryService::new("class").execute(&index, request);

            assert!(outcome.is_empty());
        }
    }
}
