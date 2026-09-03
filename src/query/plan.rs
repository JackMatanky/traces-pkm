//! The transform-plan optimization and execution engine.

use super::{QueryRecord, transform::QueryTransform};

/// Ordered, optimizable sequence of [`QueryTransform`] steps.
///
/// The single transform engine for the whole query subsystem, used two
/// ways: [`super::QueryRequest`] builds one fully before a single
/// [`Self::run`] call (pre-fetch, via [`super::QueryService::execute`]);
/// [`super::QueryRecordSet`] accumulates one incrementally across chained
/// calls and calls [`Self::run`] lazily on first read, memoizing the result
/// (post-fetch CTE chaining). [`Self::optimize`] is pure and idempotent —
/// safe to run on a plan built either way.
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct QueryPlan {
    steps: Vec<QueryTransform>,
}

impl QueryPlan {
    /// Returns `true` if this plan has no pending steps.
    pub(super) fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }

    pub(super) fn push(&mut self, transform: QueryTransform) {
        self.steps.push(transform);
    }

    /// Fuses this plan via [`Self::optimize`] and applies it to `records` in
    /// one pass. The only way a [`QueryTransform`] in this plan ever runs —
    /// used by [`super::QueryService::execute`] (pre-fetch, plan built once)
    /// and [`super::QueryRecordSet`]'s materialization (post-fetch, plan
    /// built incrementally then flushed on first read).
    pub(super) fn run(self, records: Vec<QueryRecord>) -> Vec<QueryRecord> {
        self.optimize().apply(records)
    }

    /// Fuses adjacent steps that can run cheaper combined. Internal to
    /// [`Self::run`] — nothing outside this type calls it directly, so a
    /// plan can never run unoptimized.
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
    /// into one `TopK` step, trading a full `sort_by_cached_key`
    /// (`O(n log n)`) for `select_nth_unstable_by` (`O(n)`). Preserves the
    /// position and relative order of every other step.
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
    fn apply(&self, mut records: Vec<QueryRecord>) -> Vec<QueryRecord> {
        for step in &self.steps {
            records = step.apply(records);
        }
        records
    }
}
