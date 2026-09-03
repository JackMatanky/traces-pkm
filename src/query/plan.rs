//! Query transformation plan optimization and execution engine.
//!
//! Defines [`QueryPlan`], which optimizes and executes ordered sequence of
//! [`QueryTransform`] steps.

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
///    [`QueryService::execute`](super::QueryService::execute).
/// 2. **Post-fetch CTE Chaining**: Accumulated incrementally by
///    [`QuerySet`](super::QuerySet) across method calls (`.filter()`,
///    `.sort()`, `.limit()`, `.group_by()`, `.flatten()`), and executed lazily
///    on first read via [`QuerySet::materialized`](super::QuerySet).
///
/// # Optimization Passes
///
/// Prior to applying transforms to row collections, [`Self::run`]
/// unconditionally runs [`Self::optimize`], executing two algebraic
/// optimization passes:
///
/// - **Filter Fusion**: Combines adjacent [`QueryTransform::Filter`] steps into
///   a single short-circuiting logical `AND` expression
///   ([`FilterExpr::and`](super::grammar::FilterExpr::and)), eliminating
///   intermediate vector allocations.
/// - **Sort-Limit Fusion**: Rewrites adjacent [`QueryTransform::Sort`] and
///   [`QueryTransform::Limit`] steps into a single [`QueryTransform::TopK`]
///   operation. This trades an $O(N \log N)$ full sort for an $O(N)$
///   quickselect selection via
///   [`select_nth_unstable_by`](std::slice::select_nth_unstable_by).
///
/// Optimization passes are pure and idempotent: executing `optimize` on an
/// already optimized plan produces an identical plan with no additional
/// overhead.
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct QueryPlan {
    steps: Vec<QueryTransform>,
}

impl QueryPlan {
    /// Returns `true` if this plan has no pending steps.
    pub(super) fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }

    /// Appends `transform` to this query transform plan.
    pub(super) fn push(&mut self, transform: QueryTransform) {
        self.steps.push(transform);
    }

    /// Fuses this plan via [`Self::optimize`] and applies it to `records` in
    /// one pass.
    ///
    /// This is the sole execution entry point for transformation plans. It is
    /// used by [`QueryService::execute`](super::QueryService::execute) during
    /// pre-fetch execution and by [`QuerySet`](super::QuerySet) during lazy
    /// materialization.
    pub(super) fn run(self, records: Vec<QueryRow>) -> Vec<QueryRow> {
        self.optimize().apply(records)
    }

    /// Fuses adjacent steps that can run more efficiently combined.
    ///
    /// Internal helper called by [`Self::run`]. Outside callers cannot invoke
    /// transformations without optimization.
    #[must_use]
    fn optimize(self) -> Self {
        self.fuse_filters().fuse_sort_limit()
    }

    /// Merges every run of consecutive `Filter` steps into one, via
    /// `FilterExpr::and`. Preserves the position and relative order of
    /// every other step.
    #[must_use]
    fn fuse_filters(mut self) -> Self {
        let mut fused = Vec::with_capacity(self.steps.len());
        let mut steps = self.steps.into_iter().peekable();
        while let Some(step) = steps.next() {
            if let QueryTransform::Filter(mut expr) = step {
                while let Some(QueryTransform::Filter(next)) =
                    steps.next_if(|s| matches!(s, QueryTransform::Filter(_)))
                {
                    expr = expr.and(next);
                }
                fused.push(QueryTransform::Filter(expr));
            } else {
                fused.push(step);
            }
        }
        self.steps = fused;
        self
    }

    /// Rewrites every `Sort` step immediately followed by a `Limit(n)` step
    /// into one `TopK` step, trading a full `sort_by_cached_key` (`O(n log n)`)
    /// for `select_nth_unstable_by` (`O(n)`). Preserves the position and
    /// relative order of every other step.
    #[must_use]
    fn fuse_sort_limit(mut self) -> Self {
        let mut fused = Vec::with_capacity(self.steps.len());
        let mut steps = self.steps.into_iter().peekable();
        while let Some(step) = steps.next() {
            if let QueryTransform::Sort {
                field,
                descending,
            } = &step
                && let Some(QueryTransform::Limit(n)) = steps.peek()
            {
                let n = *n;
                fused.push(QueryTransform::TopK {
                    field: field.clone(),
                    descending: *descending,
                    n,
                });
                steps.next();
            } else {
                fused.push(step);
            }
        }
        self.steps = fused;
        self
    }

    /// Applies every step in order. Internal to [`Self::run`].
    fn apply(&self, mut records: Vec<QueryRow>) -> Vec<QueryRow> {
        for step in &self.steps {
            records = step.apply(records);
        }
        records
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

    /// Applies this single transform to `records`, returning the transformed
    /// vec. Absorbs the bodies previously on `QuerySet` as
    /// `apply_filter`/`limit_to`/`flatten_field`/`sort_by_field`.
    pub(super) fn apply(&self, records: Vec<QueryRow>) -> Vec<QueryRow> {
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
                // `(SortKey, original index)` breaks ties by input position,
                // matching `sort_by_cached_key`'s stability guarantee (used by
                // the unfused `Sort` arm above). Without the index,
                // `select_nth_unstable_by`/`sort_unstable_by` are free to
                // reorder or reselect among tied keys arbitrarily (a real
                // behavior difference from the unfused path whenever the sort
                // field has duplicate values near the selection boundary, such
                // as a low-cardinality field like `status`).
                let mut keyed: Vec<(SortKey, usize, QueryRow)> = records
                    .into_iter()
                    .enumerate()
                    .map(|(index, record)| {
                        let key = SortKey {
                            value: record.resolve_owned(field),
                            descending: *descending,
                        };
                        (key, index, record)
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
                keyed.into_iter().map(|(.., record)| record).collect()
            }
        }
    }
}
