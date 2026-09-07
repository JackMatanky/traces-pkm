//! Query transformation plan optimizer and executor.
//!
//! [`QueryBuilder`](super::QueryBuilder) runs a complete plan before fetching
//! rows from the index; [`QuerySet`](super::QuerySet) accumulates a plan across
//! chained operations and runs it lazily on first read. [`QueryPlan::run`]
//! fuses adjacent filters, merges consecutive sorts, and rewrites sort-limit
//! pairs into `TopK`, replacing an `O(n log n)` full sort with an `O(n)`
//! quickselect partition when a bounded selection is enough.
#[cfg(test)]
use super::sort::SortTerm;
use super::{
    QueryBuilderError, QueryRow,
    grammar::{FieldPath, FilterExpr},
    sort::{SortDirection, SortOrder},
};
use crate::note::NoteFieldValue;

/// Ordered, optimizable transformation pipeline over query rows.
///
/// Optimizations are algebraic and idempotent: running them again on an
/// optimized plan leaves the plan unchanged.
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct QueryPlan {
    ops: Vec<QueryTransform>,
}

impl QueryPlan {
    /// Optimizes the plan, then applies each transform to `rows`.
    pub(super) fn run(self, rows: Vec<QueryRow>) -> Vec<QueryRow> {
        self.fuse_filters().fuse_sorts().fuse_sort_limit().apply(rows)
    }

    pub(super) fn is_empty(&self) -> bool {
        self.ops.is_empty()
    }

    pub(super) fn push(&mut self, transform: QueryTransform) {
        self.ops.push(transform);
    }

    fn apply(&self, mut rows: Vec<QueryRow>) -> Vec<QueryRow> {
        for op in &self.ops {
            rows = op.apply(rows);
        }
        rows
    }

    /// Merges consecutive `Filter` runs with `FilterExpr::and`, preserving
    /// non-filter steps.
    #[must_use]
    fn fuse_filters(mut self) -> Self {
        let mut fused = Vec::with_capacity(self.ops.len());
        let mut items = self.ops.into_iter().peekable();
        while let Some(item) = items.next() {
            if let QueryTransform::Filter(mut expr) = item {
                while let Some(QueryTransform::Filter(next)) =
                    items.next_if(|s| matches!(s, QueryTransform::Filter(_)))
                {
                    expr = expr.and(next);
                }
                fused.push(QueryTransform::Filter(expr));
            } else {
                fused.push(item);
            }
        }
        self.ops = fused;
        self
    }

    /// Merges consecutive `Sort` runs into one composite `SortOrder`,
    /// preserving non-sort steps.
    #[must_use]
    fn fuse_sorts(mut self) -> Self {
        let mut fused = Vec::with_capacity(self.ops.len());
        let mut items = self.ops.into_iter().peekable();
        while let Some(item) = items.next() {
            if let QueryTransform::Sort {
                mut order,
            } = item
            {
                while let Some(QueryTransform::Sort {
                    order: next,
                }) =
                    items.next_if(|s| matches!(s, QueryTransform::Sort { .. }))
                {
                    order = order.concat(next);
                }
                fused.push(QueryTransform::Sort {
                    order,
                });
            } else {
                fused.push(item);
            }
        }
        self.ops = fused;
        self
    }

    /// Rewrites each adjacent `Sort`/`Limit(n)` pair into one `TopK` step.
    #[must_use]
    fn fuse_sort_limit(mut self) -> Self {
        let mut fused = Vec::with_capacity(self.ops.len());
        let mut items = self.ops.into_iter().peekable();
        while let Some(item) = items.next() {
            if let QueryTransform::Sort {
                order,
            } = &item
                && let Some(QueryTransform::Limit(n)) = items.peek()
            {
                let n = *n;
                fused.push(QueryTransform::TopK {
                    order: order.clone(),
                    n,
                });
                items.next();
            } else {
                fused.push(item);
            }
        }
        self.ops = fused;
        self
    }
}

/// Single operation in a [`QueryPlan`].
#[derive(Clone, Debug, PartialEq)]
pub(super) enum QueryTransform {
    Filter(FilterExpr),
    Sort {
        order: SortOrder,
    },
    Limit(usize),
    GroupBy(FieldPath),
    Flatten(FieldPath),
    /// Optimizer-produced sort-then-limit step; callers construct `Sort` plus
    /// `Limit`.
    TopK {
        order: SortOrder,
        n: usize,
    },
}

impl QueryTransform {
    /// Parses `expr` as a filter expression.
    ///
    /// # Errors
    ///
    /// - [`Syntax`] if `expr` is an invalid filter expression.
    /// - [`FieldPath`] if `expr` contains a malformed field path.
    ///
    /// [`Syntax`]: QueryBuilderError::Syntax
    /// [`FieldPath`]: QueryBuilderError::FieldPath
    pub(super) fn filter(expr: &str) -> Result<Self, QueryBuilderError> {
        Ok(Self::Filter(FilterExpr::parse(expr)?))
    }

    /// Builds a single-field sort transform.
    ///
    /// # Errors
    ///
    /// - [`FieldPath`] if `field` is not a valid field path.
    ///
    /// [`FieldPath`]: QueryBuilderError::FieldPath
    pub(super) fn sort(
        field: &str,
        descending: bool,
    ) -> Result<Self, QueryBuilderError> {
        let direction = if descending {
            SortDirection::Descending
        } else {
            SortDirection::Ascending
        };
        Ok(Self::Sort {
            order: SortOrder::single(FieldPath::parse(field)?, direction),
        })
    }

    pub(super) fn order(order: SortOrder) -> Self {
        Self::Sort {
            order,
        }
    }

    /// Builds a limit transform after validating `n` fits `usize`.
    ///
    /// # Errors
    ///
    /// - [`LimitOutOfRange`] if `n` is negative or exceeds `usize::MAX`.
    ///
    /// [`LimitOutOfRange`]: QueryBuilderError::LimitOutOfRange
    pub(super) fn limit(n: i64) -> Result<Self, QueryBuilderError> {
        let n = usize::try_from(n).map_err(|_source| {
            QueryBuilderError::LimitOutOfRange {
                value: n,
            }
        })?;
        Ok(Self::Limit(n))
    }

    /// Builds a group-by transform from `field`.
    ///
    /// # Errors
    ///
    /// - [`FieldPath`] if `field` is not a valid field path.
    ///
    /// [`FieldPath`]: QueryBuilderError::FieldPath
    pub(super) fn group_by(field: &str) -> Result<Self, QueryBuilderError> {
        Ok(Self::GroupBy(FieldPath::parse(field)?))
    }

    /// Builds a flatten transform from `field`.
    ///
    /// # Errors
    ///
    /// - [`FieldPath`] if `field` is not a valid field path.
    ///
    /// [`FieldPath`]: QueryBuilderError::FieldPath
    pub(super) fn flatten(field: &str) -> Result<Self, QueryBuilderError> {
        Ok(Self::Flatten(FieldPath::parse(field)?))
    }

    pub(super) fn apply(&self, rows: Vec<QueryRow>) -> Vec<QueryRow> {
        match self {
            Self::Filter(expr) => {
                let mut rows = rows;
                rows.retain(|row| expr.is_matching(row));
                rows
            }
            Self::Sort {
                order,
            } => order.sort_rows(rows),
            Self::Limit(n) => {
                let mut rows = rows;
                rows.truncate(*n);
                rows
            }
            Self::GroupBy(field) => {
                let order =
                    SortOrder::single(field.clone(), SortDirection::Ascending);
                order.sort_rows(rows)
            }
            Self::Flatten(field_path) => {
                let mut out = Vec::with_capacity(rows.len());
                for row in rows {
                    let NoteFieldValue::List(items) =
                        row.resolve_owned(field_path)
                    else {
                        out.push(row);
                        continue;
                    };
                    let mut items = items.into_vec();
                    let Some(last) = items.pop() else {
                        continue;
                    };
                    out.extend(items.into_iter().map(|item| {
                        row.clone().with_flattened(field_path.clone(), item)
                    }));
                    out.push(row.with_flattened(field_path.clone(), last));
                }
                out
            }
            Self::TopK {
                order,
                n,
            } => {
                let n = *n;
                if n == 0 || rows.is_empty() {
                    return Vec::new();
                }
                if n >= rows.len() {
                    return order.sort_rows(rows);
                }
                let keys = order.keys_for(&rows);
                let mut indexed: Vec<(usize, usize)> =
                    (0..rows.len()).map(|idx| (idx, idx)).collect();

                let cmp =
                    |&(a_idx, a_input): &(usize, usize),
                     &(b_idx, b_input): &(usize, usize)| {
                        order
                            .compare_keys(keys.get(a_idx), keys.get(b_idx))
                            .then_with(|| a_input.cmp(&b_input))
                    };

                let k = n.saturating_sub(1);
                indexed.select_nth_unstable_by(k, cmp);
                indexed.truncate(n);
                indexed.sort_unstable_by(cmp);

                let mut out = Vec::with_capacity(n);
                for (row_idx, _) in indexed {
                    if let Some(row) = rows.get(row_idx) {
                        out.push(row.clone());
                    }
                }
                out
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    mod optimization {
        use super::*;

        #[test]
        fn empty_plan_is_empty() {
            let plan = QueryPlan::default();
            assert!(plan.is_empty());
        }

        #[test]
        fn fuse_sorts_merges_consecutive_sort_operations_into_composite_order()
        {
            let mut plan = QueryPlan::default();
            plan.push(QueryTransform::sort("file.folder", false).unwrap());
            plan.push(QueryTransform::sort("file.mtime", true).unwrap());

            let fused = plan.fuse_sorts();
            assert_eq!(fused.ops.len(), 1);
            let QueryTransform::Sort {
                order,
            } = fused.ops.first().expect("expected Sort operation")
            else {
                return;
            };
            assert_eq!(order.len(), 2);
            assert_eq!(
                order.terms().first().map(SortTerm::direction),
                Some(SortDirection::Ascending)
            );
            assert_eq!(
                order.terms().get(1).map(SortTerm::direction),
                Some(SortDirection::Descending)
            );
        }

        #[test]
        fn fuse_sort_limit_rewrites_fused_sorts_and_limit_into_composite_topk()
        {
            let mut plan = QueryPlan::default();
            plan.push(QueryTransform::sort("author", false).unwrap());
            plan.push(QueryTransform::sort("rating", true).unwrap());
            plan.push(QueryTransform::limit(5).unwrap());

            let fused = plan.fuse_sorts().fuse_sort_limit();
            assert_eq!(fused.ops.len(), 1);
            let QueryTransform::TopK {
                order,
                n,
            } = fused.ops.first().expect("expected TopK operation")
            else {
                return;
            };
            assert_eq!(*n, 5);
            assert_eq!(order.len(), 2);
            assert_eq!(
                order.terms().first().map(SortTerm::direction),
                Some(SortDirection::Ascending)
            );
            assert_eq!(
                order.terms().get(1).map(SortTerm::direction),
                Some(SortDirection::Descending)
            );
        }
    }

    mod execution {
        use pretty_assertions::assert_eq;

        use super::*;
        #[test]
        fn empty_plan_run_returns_input_rows_unchanged() {
            let plan = QueryPlan::default();
            let rows = vec![];
            assert_eq!(plan.run(rows), vec![]);
        }

        #[test]
        fn chained_sort_matches_single_composite_sort() {
            use std::fs;

            use crate::{
                IndexerService, QueryService,
                query::{QueryBuilder, SourceSelector},
            };

            let temp = tempfile::tempdir().expect("create temp dir");
            for i in 0..500 {
                let folder = format!("folder-{}", i % 5);
                let dir = temp.path().join(&folder);
                fs::create_dir_all(&dir).expect("mkdir");
                let note_path = dir.join(format!("note-{i:03}.md"));
                let rating = (i * 37) % 20;
                fs::write(&note_path, format!("---\nrating: {rating}\n---\n"))
                    .expect("write note");
            }

            let index = std::sync::Arc::new(
                IndexerService::new(temp.path()).build().expect("build index"),
            );
            let rows1 = QueryService::new("class")
                .run(&index, QueryBuilder::pages(SourceSelector::All))
                .sort("file.folder", false)
                .expect("valid sort")
                .sort("rating", true)
                .expect("valid sort");

            let composite = SortOrder::parse(
                "+file.folder,-rating",
                SortDirection::Descending,
            )
            .expect("valid parse")
            .expect("some terms");

            let rows2 = QueryService::new("class")
                .run(&index, QueryBuilder::pages(SourceSelector::All))
                .order(composite);

            let paths1: Vec<_> =
                rows1.iter().map(|r| r.file().path().to_path_buf()).collect();
            let paths2: Vec<_> =
                rows2.iter().map(|r| r.file().path().to_path_buf()).collect();
            assert_eq!(paths1, paths2);
        }
    }
}
