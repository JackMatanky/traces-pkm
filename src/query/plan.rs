//! A single query transform step, and the plan that optimizes and runs an
//! ordered sequence of them.

use super::{
    QueryRecord, QueryRequestError,
    grammar::{FieldPath, FilterExpr},
    sort::SortKey,
};
use crate::note::NoteFieldValue;

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
/// One step in a [`QueryPlan`], produced by [`super::QueryRequest`]'s
/// builder methods and [`super::QueryRecordSet`]'s chained calls. Never
/// constructed or held apart from a [`QueryPlan`] — every constructor below
/// is immediately passed to [`QueryPlan::push`].
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
    pub(super) fn filter(expr: &str) -> Result<Self, QueryRequestError> {
        Ok(Self::Filter(FilterExpr::parse(expr)?))
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
                // `(SortKey, original index)` breaks ties by input position,
                // matching `sort_by_cached_key`'s stability guarantee (used by
                // the unfused `Sort` arm above). Without the index,
                // `select_nth_unstable_by`/`sort_unstable_by` are free to
                // reorder or reselect among tied keys arbitrarily — a real
                // behavior difference from the unfused path whenever the sort
                // field has duplicate values near the selection boundary (e.g.
                // a low-cardinality field like `status`).
                let mut keyed: Vec<(SortKey, usize, QueryRecord)> = records
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
                    |a: &(SortKey, usize, QueryRecord),
                     b: &(SortKey, usize, QueryRecord)| {
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
