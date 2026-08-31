//! Query execution request and transform plan.

use super::{
    QueryRecord, QueryRequestError,
    grammar::{FieldPath, FilterExpr, SourceSelector},
    sort::SortKey,
};
use crate::note::NoteFieldValue;

/// Query execution request.
pub struct QueryRequest {
    mode: QueryMode,
    source: SourceSelector,
    plan: QueryPlan,
}

impl QueryRequest {
    /// Builds a page-row query request for `source`.
    #[inline]
    #[must_use]
    pub fn pages(source: SourceSelector) -> Self {
        Self {
            mode: QueryMode::Pages,
            source,
            plan: QueryPlan::new(),
        }
    }

    /// Builds a task-row query request for `source`.
    #[inline]
    #[must_use]
    pub fn tasks(source: SourceSelector) -> Self {
        Self {
            mode: QueryMode::Tasks,
            source,
            plan: QueryPlan::new(),
        }
    }

    /// Appends a parsed filter transform.
    ///
    /// # Errors
    ///
    /// Returns [`QueryRequestError`] when `expr` is not a valid filter.
    #[inline]
    pub fn filter(mut self, expr: &str) -> Result<Self, QueryRequestError> {
        self.plan.push(QueryTransform::filter(expr)?);
        Ok(self)
    }

    /// Appends a parsed sort transform.
    ///
    /// # Errors
    ///
    /// Returns [`QueryRequestError`] when `field` is not a valid field path.
    #[inline]
    pub fn sort(
        mut self,
        field: &str,
        descending: bool,
    ) -> Result<Self, QueryRequestError> {
        self.plan.push(QueryTransform::sort(field, descending)?);
        Ok(self)
    }

    /// Appends a parsed limit transform.
    ///
    /// # Errors
    ///
    /// Returns [`QueryRequestError`] when `n` is negative or too large for this
    /// platform.
    #[inline]
    #[cfg_attr(
        not(any(test, feature = "test-utils")),
        expect(
            dead_code,
            reason = "public builder method on QueryRequest; exercised in \
                      tests and test-utils"
        )
    )]
    pub fn limit(mut self, n: i64) -> Result<Self, QueryRequestError> {
        self.plan.push(QueryTransform::limit(n)?);
        Ok(self)
    }

    /// Splits this request into its mode, source, and transform plan for
    /// [`super::QueryService::execute`]. The only way anything outside this
    /// file observes `QueryRequest`'s fields — they stay private.
    pub(super) fn into_parts(self) -> (QueryMode, SourceSelector, QueryPlan) {
        (self.mode, self.source, self.plan)
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(super) enum QueryMode {
    Pages,
    Tasks,
}

/// Ordered, optimizable sequence of [`QueryTransform`] steps.
pub(super) struct QueryPlan {
    steps: Vec<QueryTransform>,
}

impl QueryPlan {
    const fn new() -> Self {
        Self {
            steps: Vec::new(),
        }
    }

    fn push(&mut self, transform: QueryTransform) {
        self.steps.push(transform);
    }

    /// Fuses consecutive `Filter` steps into one (AND is commutative and
    /// associative, so any run of adjacent filters can always fuse
    /// regardless of what surrounds it), then rewrites an adjacent `Sort`
    /// immediately followed by `Limit(n)` into a single `TopK` step:
    /// partition-select the `n` smallest/largest records in O(records), then
    /// sort only the `n` survivors — cheaper than a full O(records · log
    /// records) sort when `n` is small relative to the record count. A
    /// `Filter`/`Flatten`/`GroupBy` between `Sort` and `Limit` blocks that
    /// fusion: an intervening step can change which records are still in
    /// play, so fusing across it would be incorrect.
    #[must_use]
    pub(super) fn optimize(mut self) -> Self {
        let mut optimized = Vec::with_capacity(self.steps.len());
        let mut steps = self.steps.into_iter().peekable();
        while let Some(step) = steps.next() {
            if let QueryTransform::Filter(mut expr) = step {
                while let Some(QueryTransform::Filter(next)) =
                    steps.next_if(|s| matches!(s, QueryTransform::Filter(_)))
                {
                    expr = expr.and(next);
                }
                optimized.push(QueryTransform::Filter(expr));
            } else if let QueryTransform::Sort {
                field,
                descending,
            } = &step
                && let Some(QueryTransform::Limit(n)) = steps.peek()
            {
                let n = *n;
                optimized.push(QueryTransform::TopK {
                    field: field.clone(),
                    descending: *descending,
                    n,
                });
                steps.next();
            } else {
                optimized.push(step);
            }
        }
        self.steps = optimized;
        self
    }

    /// Applies every step in order, used by [`super::QueryService::execute`].
    pub(super) fn apply(
        &self,
        mut records: Vec<QueryRecord>,
    ) -> Vec<QueryRecord> {
        for step in &self.steps {
            records = step.apply(records);
        }
        records
    }
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
    /// Sort-then-limit fused by [`QueryPlan::optimize`]. Never constructed
    /// directly by the parse constructors below.
    TopK {
        field: FieldPath,
        descending: bool,
        n: usize,
    },
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

    /// Applies this single transform to `records`, returning the transformed
    /// vec. Absorbs the bodies previously on `QueryRecordSet` as
    /// `apply_filter`/`limit_to`/`flatten_field`/`sort_by_field`.
    pub(super) fn apply(&self, records: Vec<QueryRecord>) -> Vec<QueryRecord> {
        match self {
            Self::Filter(expr) => {
                let mut records = records;
                records.retain(|record| expr.is_matching(record));
                records
            }
            Self::Sort {
                field,
                descending,
            } => {
                let mut records = records;
                records.sort_by_cached_key(|record| SortKey {
                    value: record.resolve_owned(field),
                    descending: *descending,
                });
                records
            }
            Self::Limit(n) => {
                let mut records = records;
                records.truncate(*n);
                records
            }
            Self::GroupBy(field) => {
                let mut records = records;
                records.sort_by_cached_key(|record| SortKey {
                    value: record.resolve_owned(field),
                    descending: false,
                });
                records
            }
            Self::Flatten(field_path) => {
                let mut out = Vec::with_capacity(records.len());
                for record in records {
                    let NoteFieldValue::List(mut items) =
                        record.resolve_owned(field_path)
                    else {
                        out.push(record);
                        continue;
                    };
                    let Some(last) = items.pop() else {
                        continue;
                    };
                    out.extend(items.into_iter().map(|item| {
                        record.clone().with_flattened(field_path.clone(), item)
                    }));
                    out.push(record.with_flattened(field_path.clone(), last));
                }
                out
            }
            Self::TopK {
                field,
                descending,
                n,
            } => {
                let n = *n;
                let mut keyed: Vec<(SortKey, QueryRecord)> = records
                    .into_iter()
                    .map(|record| {
                        let key = SortKey {
                            value: record.resolve_owned(field),
                            descending: *descending,
                        };
                        (key, record)
                    })
                    .collect();
                if n < keyed.len() {
                    let k = n.saturating_sub(1);
                    keyed.select_nth_unstable_by(k, |a, b| a.0.cmp(&b.0));
                    keyed.truncate(n);
                }
                keyed.sort_unstable_by(|a, b| a.0.cmp(&b.0));
                keyed.into_iter().map(|(_, record)| record).collect()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, path::Path, sync::Arc};

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
            let index = Arc::new(
                IndexerService::new(temp.path()).build().expect("build index"),
            );
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
            let request = QueryRequest::pages(SourceSelector::All)
                .sort("rating", true)
                .expect("valid sort")
                .limit(2)
                .expect("valid limit");

            let outcome = QueryService::new("class").execute(&index, request);

            assert_eq!(outcome.len(), 2);
            assert_eq!(
                outcome.get(0).expect("row").base().path(),
                Path::new("b.md")
            );
            assert_eq!(
                outcome.get(1).expect("row").base().path(),
                Path::new("d.md")
            );
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
            let request = QueryRequest::pages(SourceSelector::All)
                .sort("rating", true)
                .expect("valid sort")
                .filter("file.path != \"d.md\"")
                .expect("valid filter")
                .limit(2)
                .expect("valid limit");

            let outcome = QueryService::new("class").execute(&index, request);

            assert_eq!(outcome.len(), 2);
            assert_eq!(
                outcome.get(0).expect("row").base().path(),
                Path::new("b.md")
            );
            assert_eq!(
                outcome.get(1).expect("row").base().path(),
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
            let fused_request = QueryRequest::pages(SourceSelector::All)
                .filter("rating > 2")
                .expect("valid filter")
                .filter("rating < 8")
                .expect("valid filter");

            // Verify that optimize() fuses adjacent filters into a single
            // Filter step
            let (_, _, plan) = QueryRequest::pages(SourceSelector::All)
                .filter("rating > 2")
                .expect("valid filter")
                .filter("rating < 8")
                .expect("valid filter")
                .into_parts();
            assert_eq!(plan.optimize().steps.len(), 1);

            let fused_outcome =
                QueryService::new("class").execute(&index, fused_request);

            // Single combined filter for comparison
            let combined_request = QueryRequest::pages(SourceSelector::All)
                .filter("rating > 2 and rating < 8")
                .expect("valid filter");
            let combined_outcome =
                QueryService::new("class").execute(&index, combined_request);

            assert_eq!(fused_outcome, combined_outcome);
            assert_eq!(fused_outcome.len(), 3);
            let paths: Vec<&Path> =
                fused_outcome.iter().map(|r| r.base().path()).collect();
            assert_eq!(paths, [
                Path::new("b.md"),
                Path::new("c.md"),
                Path::new("d.md")
            ]);
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
            let index = Arc::new(
                IndexerService::new(temp.path()).build().expect("build index"),
            );
            let request = QueryRequest::pages(
                SourceSelector::parse("@book").expect("source"),
            );

            let outcome = QueryService::new("class").execute(&index, request);

            assert!(outcome.is_empty());
        }
    }
}
