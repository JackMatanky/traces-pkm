//! Query transformation plan optimization and execution engine.
//!
//! [`QueryPlan`] is the single transformation engine for the whole query
//! subsystem. [`QueryBuilder`](super::QueryBuilder) builds one fully before a
//! single pre-fetch [`QueryPlan::run`] call; [`QuerySet`](super::QuerySet)
//! accumulates one incrementally across chained calls and runs it lazily on
//! first read. Either way, [`QueryPlan::run`] fuses adjacent `Filter` steps
//! into one and rewrites `Sort` followed by `Limit` into a single `TopK`
//! step, trading an `O(n log n)` full sort for an `O(n)` quickselect
//! partition where possible.
//!
//! [`QueryTransform`] is the individual step type a [`QueryPlan`] holds.
use super::{
    QueryBuilderError, QueryRow,
    grammar::{FieldPath, FilterExpr},
    sort::{SortDirection, SortKey, SortOrder, SortTerm},
};
use crate::note::NoteFieldValue;

/// An ordered, optimizable transformation pipeline executed over query rows.
///
/// `QueryPlan` serves as the central transformation engine for the query
/// subsystem, operating in two complementary contexts:
///
/// 1. **Pre-fetch Execution**: Constructed by
///    [`QueryBuilder`](super::QueryBuilder) to define filter, sort, and limit
///    criteria before querying the borrowed
///    [`FileIndex`](crate::index::FileIndex) via
///    [`QueryService::run`](super::QueryService::run).
/// 2. **Post-fetch CTE Chaining**: Accumulated incrementally by
///    [`QuerySet`](super::QuerySet) across method calls (`.filter()`,
///    `.sort()`, `.limit()`, `.group_by()`, `.flatten()`), and executed lazily
///    on first read via [`QuerySet::rows`](super::QuerySet).
///
/// # Optimization Passes
///
/// Prior to applying transforms to row collections, [`Self::run`]
/// unconditionally executes algebraic optimization passes:
///
/// - **Filter Fusion**: Combines adjacent [`QueryTransform::Filter`] operations
///   into a single short-circuiting logical `AND` expression
///   ([`FilterExpr::and`](super::grammar::FilterExpr::and)), eliminating
///   intermediate vector allocations.
/// - **Sort-Limit Fusion**: Rewrites adjacent [`QueryTransform::Sort`] and
///   [`QueryTransform::Limit`] operations into a single
///   [`QueryTransform::TopK`] operation. This trades an `O(n log n)` full sort
///   for an `O(n)` quickselect selection via [`slice::select_nth_unstable_by`].
///
/// Optimization passes are pure and idempotent: executing them on an already
/// optimized plan produces an identical plan with no additional overhead.
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct QueryPlan {
    ops: Vec<QueryTransform>,
}

impl QueryPlan {
    /// Fuses filter and sort-limit operations, then applies them to `rows`
    /// in one pass.
    ///
    /// This is the sole execution entry point for transformation plans. It is
    /// used by [`QueryService::run`](super::QueryService::run) during
    /// pre-fetch execution and by [`QuerySet`](super::QuerySet) during lazy
    /// materialization.
    pub(super) fn run(self, rows: Vec<QueryRow>) -> Vec<QueryRow> {
        self.fuse_filters().fuse_sorts().fuse_sort_limit().apply(rows)
    }

    /// Returns `true` if this plan has no pending operations.
    pub(super) fn is_empty(&self) -> bool {
        self.ops.is_empty()
    }

    /// Appends `transform` to this query transform plan.
    pub(super) fn push(&mut self, transform: QueryTransform) {
        self.ops.push(transform);
    }

    /// Applies every transform in order. Internal to [`Self::run`].
    fn apply(&self, mut rows: Vec<QueryRow>) -> Vec<QueryRow> {
        for op in &self.ops {
            rows = op.apply(rows);
        }
        rows
    }

    /// Merges every run of consecutive `Filter` steps into one, via
    /// `FilterExpr::and`. Preserves the position and relative order of
    /// every other step.
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

    /// Merges every run of consecutive `Sort` steps into one composite
    /// `SortOrder` step. Preserves the position and relative order of every
    /// other step.
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

    /// Rewrites every `Sort` step immediately followed by a `Limit(n)` step
    /// into one `TopK` step, trading a full sort for `select_nth_unstable_by`.
    /// Preserves the position and relative order of every other step.
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

/// One step in a [`QueryPlan`], produced by
/// [`QueryBuilder`](super::QueryBuilder) and [`QuerySet`](super::QuerySet)
/// method calls.
///
/// `QueryTransform` steps are constructed exclusively within a [`QueryPlan`]
/// and passed immediately to [`QueryPlan::push`].
#[derive(Clone, Debug, PartialEq)]
pub(super) enum QueryTransform {
    Filter(FilterExpr),
    Sort {
        order: SortOrder,
    },
    Limit(usize),
    GroupBy(FieldPath),
    Flatten(FieldPath),
    /// Sort-then-limit fused by [`QueryPlan`]'s optimizer. Never constructed
    /// directly by the parse constructors below.
    TopK {
        order: SortOrder,
        n: usize,
    },
}

impl QueryTransform {
    /// Parses `expr` into a [`QueryTransform::Filter`] step.
    ///
    /// # Errors
    ///
    /// - [`Syntax`] if `expr` is an invalid filter expression.
    /// - [`FieldPath`] if `expr` contains a malformed field path.
    ///
    /// [`Syntax`]: QueryBuilderError::Syntax
    pub(super) fn filter(expr: &str) -> Result<Self, QueryBuilderError> {
        Ok(Self::Filter(FilterExpr::parse(expr)?))
    }

    /// Parses `field` into a [`QueryTransform::Sort`] step.
    ///
    /// # Errors
    ///
    /// - [`FieldPath`] if `field` is not a valid field path.
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

    /// Parses `n` into a [`QueryTransform::Limit`] step.
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

    /// Parses `field` into a [`QueryTransform::GroupBy`] step.
    ///
    /// # Errors
    ///
    /// - [`FieldPath`] if `field` is not a valid field path.
    ///
    /// [`FieldPath`]: QueryBuilderError::FieldPath
    pub(super) fn group_by(field: &str) -> Result<Self, QueryBuilderError> {
        Ok(Self::GroupBy(FieldPath::parse(field)?))
    }

    /// Parses `field` into a [`QueryTransform::Flatten`] step.
    ///
    /// # Errors
    ///
    /// - [`FieldPath`] if `field` is not a valid field path.
    ///
    /// [`FieldPath`]: QueryBuilderError::FieldPath
    pub(super) fn flatten(field: &str) -> Result<Self, QueryBuilderError> {
        Ok(Self::Flatten(FieldPath::parse(field)?))
    }

    /// Applies this single transform to `rows`, returning the transformed
    /// vec.
    pub(super) fn apply(&self, rows: Vec<QueryRow>) -> Vec<QueryRow> {
        match self {
            Self::Filter(expr) => {
                let mut rows = rows;
                rows.retain(|row| expr.is_matching(row));
                rows
            }
            Self::Sort {
                order,
            } => {
                let mut rows = rows;
                order.sort_rows(&mut rows);
                rows
            }
            Self::Limit(n) => {
                let mut rows = rows;
                rows.truncate(*n);
                rows
            }
            Self::GroupBy(field) => {
                let mut rows = rows;
                let order =
                    SortOrder::single(field.clone(), SortDirection::Ascending);
                order.sort_rows(&mut rows);
                rows
            }
            Self::Flatten(field_path) => {
                let mut out = Vec::with_capacity(rows.len());
                for row in rows {
                    let NoteFieldValue::List(mut items) =
                        row.resolve_owned(field_path)
                    else {
                        out.push(row);
                        continue;
                    };
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
                    let mut rows = rows;
                    order.sort_rows(&mut rows);
                    return rows;
                }
                let keys = order.keys_for(&rows);
                let mut indexed: Vec<(u32, u32)> =
                    (0..rows.len() as u32).map(|idx| (idx, idx)).collect();
                let terms = order.terms();

                let cmp =
                    |&(a_idx, a_input): &(u32, u32),
                     &(b_idx, b_input): &(u32, u32)| {
                        let ord = compare_composite_keys(
                            keys.get(a_idx as usize),
                            keys.get(b_idx as usize),
                            terms,
                        );
                        ord.then_with(|| a_input.cmp(&b_input))
                    };

                let k = n.saturating_sub(1);
                indexed.select_nth_unstable_by(k, cmp);
                indexed.truncate(n);
                indexed.sort_unstable_by(cmp);

                let mut opt_rows: Vec<Option<QueryRow>> =
                    rows.into_iter().map(Some).collect();
                let mut out = Vec::with_capacity(n);
                for (row_idx, _) in indexed {
                    if let Some(row) = opt_rows
                        .get_mut(row_idx as usize)
                        .and_then(Option::take)
                    {
                        out.push(row);
                    }
                }
                out
            }
        }
    }
}

/// Compares two strided key slices across a composite sort order.
fn compare_composite_keys(
    a_keys: &[SortKey],
    b_keys: &[SortKey],
    terms: &[SortTerm],
) -> std::cmp::Ordering {
    for (i, term) in terms.iter().enumerate() {
        let (Some(a_k), Some(b_k)) = (a_keys.get(i), b_keys.get(i)) else {
            continue;
        };
        let ord = a_k.total_cmp(b_k);
        let ord = match term.direction() {
            SortDirection::Ascending => ord,
            SortDirection::Descending => ord.reverse(),
        };
        if ord != std::cmp::Ordering::Equal {
            return ord;
        }
    }
    std::cmp::Ordering::Equal
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
                query::{QueryBuilder, SourceSelector, sort::SortTerm},
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

            let composite = SortOrder::new(vec![
                SortTerm::new(
                    FieldPath::parse("file.folder").unwrap(),
                    SortDirection::Ascending,
                ),
                SortTerm::new(
                    FieldPath::parse("rating").unwrap(),
                    SortDirection::Descending,
                ),
            ])
            .unwrap();

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
