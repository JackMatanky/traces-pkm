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
    sort::SortKey,
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
///   for an `O(n)` quickselect selection via
///   [`select_nth_unstable_by`](std::slice::select_nth_unstable_by).
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
        self.fuse_filters().fuse_sort_limit().apply(rows)
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

    /// Rewrites every `Sort` step immediately followed by a `Limit(n)` step
    /// into one `TopK` step, trading a full `sort_by_cached_key` (`O(n log n)`)
    /// for `select_nth_unstable_by` (`O(n)`). Preserves the position and
    /// relative order of every other step.
    #[must_use]
    fn fuse_sort_limit(mut self) -> Self {
        let mut fused = Vec::with_capacity(self.ops.len());
        let mut items = self.ops.into_iter().peekable();
        while let Some(item) = items.next() {
            if let QueryTransform::Sort {
                field,
                descending,
            } = &item
                && let Some(QueryTransform::Limit(n)) = items.peek()
            {
                let n = *n;
                fused.push(QueryTransform::TopK {
                    field: field.clone(),
                    descending: *descending,
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
        field: FieldPath,
        descending: bool,
    },
    Limit(usize),
    GroupBy(FieldPath),
    Flatten(FieldPath),
    /// Sort-then-limit fused by [`QueryPlan`]'s optimizer. Never constructed
    /// directly by the parse constructors below.
    TopK {
        field: FieldPath,
        descending: bool,
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
        Ok(Self::Sort {
            field: FieldPath::parse(field)?,
            descending,
        })
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
                field,
                descending,
            } => {
                let mut rows = rows;
                rows.sort_by_cached_key(|row| SortKey {
                    value: row.resolve_owned(field),
                    descending: *descending,
                });
                rows
            }
            Self::Limit(n) => {
                let mut rows = rows;
                rows.truncate(*n);
                rows
            }
            Self::GroupBy(field) => {
                let mut rows = rows;
                rows.sort_by_cached_key(|row| SortKey {
                    value: row.resolve_owned(field),
                    descending: false,
                });
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
                field,
                descending,
                n,
            } => {
                let n = *n;
                // `(SortKey, original index)` breaks ties by input position,
                // matching `sort_by_cached_key`'s stability guarantee (used by
                // the unfused `Sort` arm above). Without the index,
                // `select_nth_unstable_by`/`sort_unstable_by` are free to
                // reorder or reselect among tied keys arbitrarily (a real
                // behavior difference from the unfused path whenever the sort
                // field has duplicate values near the selection boundary, such
                // as a low-cardinality field like `status`).
                let mut keyed: Vec<(SortKey, usize, QueryRow)> = rows
                    .into_iter()
                    .enumerate()
                    .map(|(index, row)| {
                        let key = SortKey {
                            value: row.resolve_owned(field),
                            descending: *descending,
                        };
                        (key, index, row)
                    })
                    .collect();
                let cmp =
                    |a: &(SortKey, usize, QueryRow),
                     b: &(SortKey, usize, QueryRow)| {
                        a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1))
                    };
                if n < keyed.len() {
                    let k = n.saturating_sub(1);
                    keyed.select_nth_unstable_by(k, cmp);
                    keyed.truncate(n);
                }
                keyed.sort_unstable_by(cmp);
                keyed.into_iter().map(|(.., row)| row).collect()
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
    }
}
