//! A single query transform step and its application logic.

use super::{
    QueryRecord, QueryRequestError,
    grammar::{FieldPath, FilterExpr},
    sort::SortKey,
};
use crate::note::NoteFieldValue;

/// One step in a [`super::plan::QueryPlan`], produced by
/// [`super::QueryRequest`]'s builder methods and [`super::QueryRecordSet`]'s
/// chained calls.
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
    /// Sort-then-limit fused by [`super::plan::QueryPlan`]'s optimizer. Never
    /// constructed directly by the parse constructors below.
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
