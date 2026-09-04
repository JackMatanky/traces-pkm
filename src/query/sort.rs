//! Sort-key utilities and total-order comparison for resolved field values.

use std::cmp::Ordering;

use super::{
    QueryRow, error::QueryBuilderError, grammar::FieldPath,
    value::QueryFieldValueRef,
};
use crate::{NoteFieldValue, file::Timestamp};

/// A composite ordering clause composed of one or more [`SortTerm`] items.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SortOrder {
    terms: Box<[SortTerm]>,
}

impl SortOrder {
    /// Constructs a single-term `SortOrder`.
    #[inline]
    #[must_use]
    pub(crate) fn single(path: FieldPath, direction: SortDirection) -> Self {
        Self {
            terms: Box::new([SortTerm::new(path, direction)]),
        }
    }

    /// Concatenates two sort order clauses together.
    #[must_use]
    pub(crate) fn concat(self, other: Self) -> Self {
        let mut terms = self.terms.into_vec();
        terms.extend(other.terms);
        Self {
            terms: terms.into_boxed_slice(),
        }
    }

    /// Evaluates each term's field path against each row into a flat
    /// [`SortKeys`] buffer.
    pub(crate) fn keys_for(&self, rows: &[QueryRow]) -> SortKeys {
        let stride = self.terms.len();
        let mut flat = Vec::with_capacity(rows.len().saturating_mul(stride));
        for row in rows {
            for term in &self.terms {
                let val_ref = row.resolve_ref(&term.path);
                flat.push(SortKey::from_value_ref(&val_ref));
            }
        }
        SortKeys {
            flat,
            stride,
        }
    }

    /// Compares two rows' precomputed key slices (as produced by
    /// [`Self::keys_for`]) across every term in this composite order, applying
    /// each term's [`SortDirection`] and short-circuiting on the first
    /// non-equal term. Shared by [`Self::sort_rows`]'s full permutation sort
    /// and [`super::plan::QueryTransform::TopK`]'s quickselect so both
    /// execution paths apply identical ordering semantics.
    #[must_use]
    pub(crate) fn compare_keys(
        &self,
        a_keys: &[SortKey],
        b_keys: &[SortKey],
    ) -> Ordering {
        for (i, term) in self.terms.iter().enumerate() {
            let (Some(a_k), Some(b_k)) = (a_keys.get(i), b_keys.get(i)) else {
                continue;
            };
            let ord = a_k.total_cmp(b_k);
            let ord = if term.direction().is_descending() {
                ord.reverse()
            } else {
                ord
            };
            if ord != Ordering::Equal {
                return ord;
            }
        }
        Ordering::Equal
    }

    /// Permutes `rows` in place according to this composite sort order.
    pub(crate) fn sort_rows(&self, rows: &mut [QueryRow]) {
        if rows.len() <= 1 || self.terms.is_empty() {
            return;
        }
        let keys = self.keys_for(rows);
        let mut perm: Vec<usize> = (0..rows.len()).collect();

        perm.sort_by(|&a_idx, &b_idx| {
            self.compare_keys(keys.get(a_idx), keys.get(b_idx))
                .then_with(|| a_idx.cmp(&b_idx))
        });

        let mut dest_perm = vec![0usize; perm.len()];
        for (dest, &src) in perm.iter().enumerate() {
            if let Some(slot) = dest_perm.get_mut(src) {
                *slot = dest;
            }
        }
        for i in 0..rows.len() {
            while dest_perm.get(i).copied() != Some(i) {
                let Some(&d) = dest_perm.get(i) else {
                    break;
                };
                rows.swap(i, d);
                dest_perm.swap(i, d);
            }
        }
    }

    /// Returns the slice of sort terms.
    #[inline]
    #[must_use]
    #[cfg(test)]
    pub(crate) fn terms(&self) -> &[SortTerm] {
        &self.terms
    }

    /// Returns the number of sort terms.
    #[inline]
    #[must_use]
    #[cfg(test)]
    pub(crate) fn is_empty(&self) -> bool {
        self.terms.is_empty()
    }

    /// Returns the number of sort terms.
    #[inline]
    #[must_use]
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.terms.len()
    }

    /// Parses a comma-separated sort clause into a composite `SortOrder`.
    ///
    /// Each comma-separated segment may carry a `+` (ascending) or `-`
    /// (descending) prefix; an unprefixed segment uses `default_direction`.
    /// Blank segments, such as those produced by a leading, trailing, or
    /// doubled comma, are skipped rather than rejected. This is the grammar
    /// behind the CLI's `--sort` flag, where `default_direction` comes from
    /// the `--asc`/`--desc` flags.
    ///
    /// Returns `Ok(None)` if `input` yields no terms once blank segments are
    /// skipped.
    ///
    /// # Errors
    ///
    /// - [`QueryBuilderError::FieldPath`] if any segment, after stripping its
    ///   `+`/`-` prefix, is not a valid field path.
    pub(crate) fn parse(
        input: &str,
        default_direction: SortDirection,
    ) -> Result<Option<Self>, QueryBuilderError> {
        let mut terms = Vec::new();
        for part in input.split(',') {
            let part = part.trim();
            if part.is_empty() {
                continue;
            }
            let (path_str, direction) =
                if let Some(stripped) = part.strip_prefix('+') {
                    (stripped, SortDirection::Ascending)
                } else if let Some(stripped) = part.strip_prefix('-') {
                    (stripped, SortDirection::Descending)
                } else {
                    (part, default_direction)
                };
            terms.push(SortTerm::new(FieldPath::parse(path_str)?, direction));
        }
        if terms.is_empty() {
            return Ok(None);
        }
        Ok(Some(Self {
            terms: terms.into_boxed_slice(),
        }))
    }
}

/// A single field path and direction in a composite [`SortOrder`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SortTerm {
    path: FieldPath,
    direction: SortDirection,
}

impl SortTerm {
    /// Constructs a new sort term.
    #[inline]
    #[must_use]
    pub(crate) const fn new(path: FieldPath, direction: SortDirection) -> Self {
        Self {
            path,
            direction,
        }
    }

    /// Returns the field path for this term.
    #[inline]
    #[must_use]
    #[cfg(test)]
    pub(crate) fn path(&self) -> &FieldPath {
        &self.path
    }

    /// Returns the sort direction for this term.
    #[inline]
    #[must_use]
    pub(crate) const fn direction(&self) -> SortDirection {
        self.direction
    }
}

/// Sort direction for sorting operations. Defaults to [`Self::Descending`],
/// matching every unprefixed/unflagged sort term across the CLI and template
/// callers.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub(crate) enum SortDirection {
    /// Ascending order.
    Ascending,
    /// Descending order (the default).
    #[default]
    Descending,
}

impl SortDirection {
    /// Returns `true` if this direction is [`Self::Descending`].
    #[inline]
    #[must_use]
    pub(crate) const fn is_descending(self) -> bool {
        matches!(self, Self::Descending)
    }
}

/// Precomputed flat strided buffer of sort keys across rows.
pub(crate) struct SortKeys {
    flat: Vec<SortKey>,
    stride: usize,
}

impl SortKeys {
    /// Returns the sort keys for the row at `row_idx`.
    #[inline]
    #[must_use]
    pub(crate) fn get(&self, row_idx: usize) -> &[SortKey] {
        let start = row_idx.saturating_mul(self.stride);
        let end = start.saturating_add(self.stride);
        self.flat.get(start..end).unwrap_or(&[])
    }
}

/// A compact, native sort scalar for row comparisons.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum SortKey {
    Null,
    Bool(bool),
    Number(f64),
    Date(Timestamp),
    Duration(f64),
    Text(Box<str>),
}

impl SortKey {
    /// Extracts a `SortKey` from a resolved field reference.
    pub(super) fn from_value_ref(val: &QueryFieldValueRef<'_>) -> Self {
        match val {
            QueryFieldValueRef::Null => Self::Null,
            QueryFieldValueRef::Bool(b) => Self::Bool(*b),
            QueryFieldValueRef::Number(n) => Self::Number(*n),
            QueryFieldValueRef::Timestamp(ts) => Self::Date(*ts),
            QueryFieldValueRef::Date(s) => {
                if let Some(ts) = Timestamp::parse_iso(s) {
                    Self::Date(ts)
                } else {
                    Self::Text((*s).into())
                }
            }
            QueryFieldValueRef::Duration(s) => {
                if let Some(secs) = crate::note::duration_seconds(s) {
                    Self::Duration(secs)
                } else {
                    Self::Text((*s).into())
                }
            }
            QueryFieldValueRef::Text(s) => {
                if let Some(ts) = Timestamp::parse_iso(s) {
                    Self::Date(ts)
                } else if let Some(secs) = crate::note::duration_seconds(s) {
                    Self::Duration(secs)
                } else {
                    Self::Text((*s).into())
                }
            }
            QueryFieldValueRef::Link(link) => Self::Text(link.target().into()),
            QueryFieldValueRef::Object(_) | QueryFieldValueRef::List(_) => {
                Self::Null
            }
            QueryFieldValueRef::Owned(owned) => Self::from_owned(owned),
        }
    }

    /// Extracts a `SortKey` from an owned note field value.
    pub(crate) fn from_owned(owned: &NoteFieldValue) -> Self {
        match owned {
            NoteFieldValue::Null
            | NoteFieldValue::List(_)
            | NoteFieldValue::Object(_) => Self::Null,
            NoteFieldValue::Bool(b) => Self::Bool(*b),
            NoteFieldValue::Number(n) => Self::Number(*n),
            NoteFieldValue::Date(s) => {
                if let Some(ts) = Timestamp::parse_iso(s) {
                    Self::Date(ts)
                } else {
                    Self::Text(s.as_str().into())
                }
            }
            NoteFieldValue::Duration(s) => {
                if let Some(secs) = crate::note::duration_seconds(s) {
                    Self::Duration(secs)
                } else {
                    Self::Text(s.as_str().into())
                }
            }
            NoteFieldValue::String(s) => {
                if let Some(ts) = Timestamp::parse_iso(s) {
                    Self::Date(ts)
                } else if let Some(secs) = crate::note::duration_seconds(s) {
                    Self::Duration(secs)
                } else {
                    Self::Text(s.as_str().into())
                }
            }
            NoteFieldValue::Link(link) => Self::Text(link.target().into()),
        }
    }

    /// Compares two sort keys establishing a total ordering.
    pub(crate) fn total_cmp(&self, other: &Self) -> Ordering {
        match (self, other) {
            (Self::Null, Self::Null) => Ordering::Equal,
            (Self::Null, _) => Ordering::Less,
            (_, Self::Null) => Ordering::Greater,
            (Self::Bool(a), Self::Bool(b)) => a.cmp(b),
            (Self::Number(a), Self::Number(b))
            | (Self::Duration(a), Self::Duration(b)) => a.total_cmp(b),
            (Self::Date(a), Self::Date(b)) => a.cmp(b),
            (Self::Text(a), Self::Text(b)) => a.cmp(b),
            _ => Ordering::Equal,
        }
    }
}

/// Compares two resolved [`NoteFieldValue`] instances of the same comparable
/// kind.
pub(super) fn compare_field_values(
    a: &NoteFieldValue,
    b: &NoteFieldValue,
) -> Option<Ordering> {
    match (a, b) {
        (NoteFieldValue::Number(x), NoteFieldValue::Number(y)) => {
            x.partial_cmp(y)
        }
        (NoteFieldValue::Bool(x), NoteFieldValue::Bool(y)) => Some(x.cmp(y)),
        _ => match (a.as_str(), b.as_str()) {
            (Some(x), Some(y)) => Some(x.cmp(y)),
            _ => None,
        },
    }
}
#[cfg(test)]
mod tests {
    use std::{fs, path::Path, sync::Arc};

    use super::super::*;
    use crate::IndexerService;

    fn outcome_for_files(temp: &Path, files: &[(&str, &str)]) -> QuerySet {
        for (name, content) in files {
            fs::write(temp.join(name), content).expect("write note");
        }
        let index =
            Arc::new(IndexerService::new(temp).build().expect("build index"));
        QueryService::new("class")
            .run(&index, QueryBuilder::pages(SourceSelector::All))
    }

    fn outcome_for(temp: &Path, content: &str) -> QuerySet {
        outcome_for_files(temp, &[("note.md", content)])
    }

    fn names(outcome: &QuerySet) -> Vec<String> {
        outcome
            .iter()
            .map(|row| row.file().name().as_str().to_owned())
            .collect()
    }

    mod sort {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn orders_ascending_by_default() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let outcome = outcome_for_files(temp.path(), &[
                ("b.md", "---\nrating: 7\n---"),
                ("a.md", "---\nrating: 3\n---"),
            ]);

            let sorted = outcome.sort("rating", false).expect("valid sort");

            assert_eq!(names(&sorted), ["a", "b"]);
        }

        #[test]
        fn orders_descending_when_requested() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let outcome = outcome_for_files(temp.path(), &[
                ("b.md", "---\nrating: 7\n---"),
                ("a.md", "---\nrating: 3\n---"),
            ]);

            let sorted = outcome.sort("rating", true).expect("valid sort");

            assert_eq!(names(&sorted), ["b", "a"]);
        }

        #[test]
        fn missing_field_sorts_as_the_minimum_value() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let outcome = outcome_for_files(temp.path(), &[
                ("rated.md", "---\nrating: 3\n---"),
                ("unrated.md", "no frontmatter"),
            ]);

            let ascending =
                outcome.clone().sort("rating", false).expect("valid sort");
            let descending = outcome.sort("rating", true).expect("valid sort");

            // Matches Dataview: Null is the minimum value, so it leads
            // ascending and trails descending, like any other value would.
            assert_eq!(names(&ascending), ["unrated", "rated"]);
            assert_eq!(names(&descending), ["rated", "unrated"]);
        }

        #[test]
        fn ties_keep_original_relative_order() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let outcome = outcome_for_files(temp.path(), &[
                ("a.md", "---\nrating: 5\n---"),
                ("b.md", "---\nrating: 5\n---"),
            ]);

            let sorted = outcome.sort("rating", false).expect("valid sort");

            assert_eq!(names(&sorted), ["a", "b"]);
        }

        #[test]
        fn rejects_malformed_field_path() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let outcome = outcome_for(temp.path(), "body");

            assert_eq!(
                outcome.sort("file.bogus", false),
                Err(QueryError::Builder(QueryBuilderError::FieldPath(
                    FieldPathError::new("file.bogus", None)
                )))
            );
        }

        #[test]
        fn sorts_boolean_field_false_before_true() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let outcome = outcome_for_files(temp.path(), &[
                ("true.md", "---\nactive: true\n---"),
                ("false.md", "---\nactive: false\n---"),
            ]);

            let sorted = outcome.sort("active", false).expect("valid sort");

            assert_eq!(names(&sorted), ["false", "true"]);
        }

        #[test]
        fn sorts_null_field_alongside_boolean_field() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let outcome = outcome_for_files(temp.path(), &[
                ("true.md", "---\nactive: true\n---"),
                ("none.md", "no frontmatter"),
            ]);

            let sorted = outcome.sort("active", false).expect("valid sort");

            assert_eq!(names(&sorted), ["none", "true"]);
        }
    }

    mod sort_order {
        use pretty_assertions::assert_eq;

        use super::super::*;

        #[test]
        fn constructs_single_and_concats() {
            let first = SortOrder::single(
                FieldPath::parse("file.folder").unwrap(),
                SortDirection::Ascending,
            );
            let second = SortOrder::single(
                FieldPath::parse("file.mtime").unwrap(),
                SortDirection::Descending,
            );
            let fused = first.concat(second);
            assert_eq!(fused.len(), 2);
            assert_eq!(
                fused.terms().first().expect("term").direction(),
                SortDirection::Ascending
            );
            assert_eq!(
                fused.terms().get(1).expect("term").direction(),
                SortDirection::Descending
            );
        }

        #[test]
        fn parses_default_descending() {
            let order =
                SortOrder::parse("file.mtime", SortDirection::Descending)
                    .expect("valid parse")
                    .expect("some terms");
            assert!(!order.is_empty());
            assert_eq!(order.len(), 1);
            let term = order.terms().first().expect("term");
            assert_eq!(term.direction(), SortDirection::Descending);
            assert_eq!(term.path(), &FieldPath::parse("file.mtime").unwrap());
        }

        #[test]
        fn unprefixed_segments_use_the_default_direction() {
            let order =
                SortOrder::parse("title, rating", SortDirection::Ascending)
                    .expect("valid parse")
                    .expect("some terms");
            assert_eq!(order.len(), 2);
            assert!(
                order
                    .terms()
                    .iter()
                    .all(|term| term.direction() == SortDirection::Ascending)
            );
        }

        #[test]
        fn prefix_modifiers_override_the_default_direction() {
            let order = SortOrder::parse(
                "+file.folder, -file.mtime",
                SortDirection::Descending,
            )
            .expect("valid parse")
            .expect("some terms");
            assert_eq!(order.len(), 2);
            assert_eq!(
                order.terms().first().expect("term").direction(),
                SortDirection::Ascending
            );
            assert_eq!(
                order.terms().get(1).expect("term").direction(),
                SortDirection::Descending
            );
        }

        #[test]
        fn skips_blank_segments_from_doubled_or_trailing_commas() {
            let order = SortOrder::parse(
                "file.folder,, file.mtime,",
                SortDirection::Descending,
            )
            .expect("valid parse")
            .expect("some terms");
            assert_eq!(order.len(), 2);
        }

        #[test]
        fn returns_none_for_blank_input() {
            assert_eq!(
                SortOrder::parse("", SortDirection::Descending)
                    .expect("valid parse"),
                None
            );
            assert_eq!(
                SortOrder::parse("   ", SortDirection::Descending)
                    .expect("valid parse"),
                None
            );
            assert_eq!(
                SortOrder::parse(",,", SortDirection::Descending)
                    .expect("valid parse"),
                None
            );
        }

        #[test]
        fn rejects_malformed_field_path() {
            assert!(
                SortOrder::parse("file..bad", SortDirection::Descending)
                    .is_err()
            );
        }
    }

    mod compare_field_values {
        use pretty_assertions::assert_eq;

        use super::super::compare_field_values;
        use crate::NoteFieldValue;

        #[test]
        fn orders_false_before_true() {
            let a = NoteFieldValue::Bool(false);
            let b = NoteFieldValue::Bool(true);

            assert_eq!(
                compare_field_values(&a, &b),
                Some(std::cmp::Ordering::Less)
            );
        }
    }

    mod sort_key_total_cmp {
        use std::cmp::Ordering;

        use pretty_assertions::assert_eq;

        use super::super::SortKey;

        #[test]
        fn null_is_less_than_any_non_null() {
            let null = SortKey::Null;
            let number = SortKey::Number(1.0);
            let text = SortKey::Text("hello".into());
            let boolean = SortKey::Bool(true);

            assert_eq!(null.total_cmp(&number), Ordering::Less);
            assert_eq!(null.total_cmp(&text), Ordering::Less);
            assert_eq!(null.total_cmp(&boolean), Ordering::Less);
            assert_eq!(number.total_cmp(&null), Ordering::Greater);
        }

        #[test]
        fn compares_durations_numerically() {
            let one_hour = SortKey::Duration(3600.0);
            let thirty_mins = SortKey::Duration(1800.0);
            assert_eq!(one_hour.total_cmp(&thirty_mins), Ordering::Greater);
            assert_eq!(thirty_mins.total_cmp(&one_hour), Ordering::Less);
        }

        #[test]
        fn cross_variant_comparison_yields_equal() {
            let number = SortKey::Number(100.0);
            let text = SortKey::Text("abc".into());
            assert_eq!(number.total_cmp(&text), Ordering::Equal);
        }

        #[test]
        fn sorts_non_finite_and_signed_zero_numbers_totally() {
            let nan = SortKey::Number(f64::NAN);
            let infinity = SortKey::Number(f64::INFINITY);
            let negative_zero = SortKey::Number(-0.0);
            let zero = SortKey::Number(0.0);

            assert_eq!(
                nan.total_cmp(&infinity),
                f64::NAN.total_cmp(&f64::INFINITY)
            );
            assert_eq!(negative_zero.total_cmp(&zero), Ordering::Less);
        }
    }
}
