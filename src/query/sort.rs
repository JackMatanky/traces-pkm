//! Sort-key utilities and total-order comparison for resolved field values.

use std::{borrow::Cow, cmp::Ordering, num::NonZeroUsize};

use super::{
    QueryRow, error::QueryBuilderError, grammar::FieldPath,
    value::QueryFieldValueRef,
};
use crate::{
    DateTimeValue, DateValue, DurationSeconds, NoteFieldType, NoteFieldValue,
    NoteFieldValueRef,
};

/// Composite ordering clause made of one or more [`SortTerm`] values.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SortOrder {
    terms: Box<[SortTerm]>,
}

impl SortOrder {
    /// Builds a single-term sort order from one field path and direction.
    #[inline]
    #[must_use]
    pub(super) fn single(path: FieldPath, direction: SortDirection) -> Self {
        Self {
            terms: Box::new([SortTerm::new(path, direction)]),
        }
    }

    /// Appends `other` after this order.
    #[must_use]
    pub(super) fn concat(self, other: Self) -> Self {
        let mut terms = self.terms.into_vec();
        terms.extend(other.terms);
        Self {
            terms: terms.into_boxed_slice(),
        }
    }

    /// Builds one row-major [`SortKeys`] buffer for `rows`.
    ///
    /// # Panics
    ///
    /// - Panics if `self.terms` is empty; callers must guard non-empty terms
    ///   before calling (`sort_rows` does this).
    #[expect(
        clippy::expect_used,
        reason = "caller guarantees non-empty terms via sort_rows guard"
    )]
    pub(super) fn keys_for<'a>(&self, rows: &'a [QueryRow]) -> SortKeys<'a> {
        let stride = NonZeroUsize::new(self.terms.len())
            .expect("caller guards non-empty");
        let mut flat =
            Vec::with_capacity(rows.len().saturating_mul(stride.get()));
        for row in rows {
            for term in &self.terms {
                let val_ref = row.resolve_ref(&term.path);
                flat.push(SortKey::from_value_ref(val_ref));
            }
        }
        SortKeys {
            flat,
            stride,
        }
    }

    /// Compares key slices term-by-term.
    ///
    /// Applies each term's direction and null placement. Shared by full
    /// sorting and top-k selection so both execution paths keep identical
    /// ordering semantics.
    #[must_use]
    pub(super) fn compare_keys(
        &self,
        a_keys: &[SortKey<'_>],
        b_keys: &[SortKey<'_>],
    ) -> Ordering {
        for (i, term) in self.terms.iter().enumerate() {
            let (Some(a_k), Some(b_k)) = (a_keys.get(i), b_keys.get(i)) else {
                continue;
            };
            let descending = term.direction().is_descending();
            let ord = match (a_k, b_k, term.null_placement) {
                (SortKey::Null, SortKey::Null, _) => Ordering::Equal,
                (SortKey::Null, _, NullPlacement::First) => Ordering::Less,
                (SortKey::Null, _, NullPlacement::Last)
                | (_, SortKey::Null, NullPlacement::First) => Ordering::Greater,
                (_, SortKey::Null, NullPlacement::Last) => Ordering::Less,
                _ => {
                    let base = a_k.cmp(b_k);
                    if descending {
                        base.reverse()
                    } else {
                        base
                    }
                }
            };
            if ord != Ordering::Equal {
                return ord;
            }
        }
        Ordering::Equal
    }

    /// Sorts `rows`, preserving original relative order for ties.
    #[must_use]
    pub(super) fn sort_rows(&self, rows: Vec<QueryRow>) -> Vec<QueryRow> {
        if rows.len() <= 1 || self.terms.is_empty() {
            return rows;
        }
        let keys = self.keys_for(&rows);
        let mut order: Vec<usize> = (0..rows.len()).collect();
        order.sort_by(|&a_idx, &b_idx| {
            self.compare_keys(keys.get(a_idx), keys.get(b_idx))
        });

        let mut opt_rows: Vec<Option<QueryRow>> =
            rows.into_iter().map(Some).collect();
        order
            .into_iter()
            .filter_map(|idx| opt_rows.get_mut(idx).and_then(Option::take))
            .collect()
    }

    #[inline]
    #[must_use]
    #[cfg(test)]
    pub(super) fn terms(&self) -> &[SortTerm] {
        &self.terms
    }

    #[inline]
    #[must_use]
    #[cfg(test)]
    fn is_empty(&self) -> bool {
        self.terms.is_empty()
    }

    #[inline]
    #[must_use]
    #[cfg(test)]
    pub(super) fn len(&self) -> usize {
        self.terms.len()
    }

    /// Parses a comma-separated sort clause.
    ///
    /// Segments may carry `+` or `-`; unprefixed segments use
    /// `default_direction`. Blank segments are skipped, so blank input returns
    /// `Ok(None)`.
    ///
    /// # Errors
    ///
    /// - [`QueryBuilderError::FieldPath`] if any nonblank segment is not a
    ///   valid field path after prefix stripping.
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

/// Field path plus direction inside a composite [`SortOrder`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct SortTerm {
    path: FieldPath,
    direction: SortDirection,
    null_placement: NullPlacement,
}

impl SortTerm {
    #[inline]
    #[must_use]
    const fn new(path: FieldPath, direction: SortDirection) -> Self {
        Self {
            path,
            direction,
            null_placement: NullPlacement::Auto,
        }
    }

    #[inline]
    #[must_use]
    #[cfg(test)]
    fn path(&self) -> &FieldPath {
        &self.path
    }

    /// Returns this term's sort direction.
    #[inline]
    #[must_use]
    pub(super) const fn direction(&self) -> SortDirection {
        self.direction
    }
}

/// Controls whether a term keeps or reverses its comparison result.
///
/// Defaults to [`Self::Descending`] to match unprefixed CLI and template terms.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub(crate) enum SortDirection {
    /// Smallest value first.
    Ascending,
    /// Largest value first (default, matching unprefixed CLI/template terms).
    #[default]
    Descending,
}

impl SortDirection {
    #[inline]
    #[must_use]
    const fn is_descending(self) -> bool {
        matches!(self, Self::Descending)
    }
}

/// Controls where a missing (null) field value lands in sort order.
///
/// Dataview treats `null` unconditionally as the smallest value, which
/// [`Self::Auto`] reproduces (first ascending, last descending). No current
/// query syntax constructs [`Self::First`] or [`Self::Last`]; this is a
/// seam for a future `sort field asc nulls last` grammar addition, wired
/// through [`SortOrder::compare_keys`] with zero behavior change today.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "seam for future nulls first/last grammar; variants First \
                  and Last are exercised in unit tests"
    )
)]
pub(crate) enum NullPlacement {
    /// Null sorts first ascending, last descending (the Dataview default).
    #[default]
    Auto,
    /// Null always sorts before every non-null value.
    First,
    /// Null always sorts after every non-null value.
    Last,
}

/// Row-major buffer of precomputed sort keys.
pub(super) struct SortKeys<'a> {
    flat: Vec<SortKey<'a>>,
    stride: NonZeroUsize,
}

impl<'a> SortKeys<'a> {
    /// Returns the key slice for `row_idx`, or an empty slice when out of
    /// range.
    #[inline]
    #[must_use]
    pub(super) fn get(&self, row_idx: usize) -> &[SortKey<'a>] {
        let stride = self.stride.get();
        let start = row_idx.saturating_mul(stride);
        let end = start.saturating_add(stride);
        self.flat.get(start..end).unwrap_or(&[])
    }
}

/// Normalized scalar used for row-order comparisons.
///
/// Reduced projection of [`NoteFieldValueRef`]'s rank order for cheap,
/// precomputed sort comparisons: `List` and `Object` collapse to `Null`,
/// `Link` collapses to `Text` by its target. Values needing full fidelity
/// compare via [`NoteFieldValueRef::compare`] directly, not through
/// `SortKey`.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum SortKey<'a> {
    /// Missing or unindexable value; sorts below every other kind.
    Null,
    /// Boolean value.
    Bool(bool),
    /// Numeric value.
    Number(f64),
    /// Date or date-time value, normalized to a date-time.
    DateTime(DateTimeValue),
    /// Duration value, normalized to seconds.
    Duration(DurationSeconds),
    /// Text value, borrowed where possible.
    Text(Cow<'a, str>),
}

impl<'a> SortKey<'a> {
    /// Normalizes a resolved field value into a comparable scalar. Takes
    /// `val` by value (not by reference) so the rare `Owned` fallback can
    /// move its already-allocated `String` into `Text` with no extra clone,
    /// and so the common case can copy out `&'a str`/`&'a Link` fields tied
    /// to the caller's own `'a`, not a shorter local borrow.
    pub(super) fn from_value_ref(val: QueryFieldValueRef<'a>) -> Self {
        match val {
            QueryFieldValueRef::Note(note_ref) => Self::from_note_ref(note_ref),
            QueryFieldValueRef::Owned(NoteFieldValue::String(text)) => {
                Self::from_text(Cow::Owned(text))
            }
            QueryFieldValueRef::Owned(_) => Self::Null,
            QueryFieldValueRef::Tags(_) | QueryFieldValueRef::Inlinks(_) => {
                Self::Null
            }
        }
    }

    fn from_note_ref(val: NoteFieldValueRef<'a>) -> Self {
        match val {
            NoteFieldValueRef::Null => Self::Null,
            NoteFieldValueRef::Bool(b) => Self::Bool(b),
            NoteFieldValueRef::Number(n) => Self::Number(n),
            NoteFieldValueRef::DateTime(value) => Self::DateTime(value),
            NoteFieldValueRef::Date(value) => {
                Self::DateTime(DateTimeValue::from(value))
            }
            NoteFieldValueRef::Duration(dv) => Self::Duration(dv.to_seconds()),
            NoteFieldValueRef::String(s) => Self::from_text(Cow::Borrowed(s)),
            NoteFieldValueRef::Link(link) => {
                Self::Text(Cow::Borrowed(link.target()))
            }
            NoteFieldValueRef::Object(_) | NoteFieldValueRef::List(_) => {
                Self::Null
            }
        }
    }

    /// Opportunistically classifies free text as a date-time, date, or
    /// duration, falling back to text. Takes `Cow` so the common borrowed
    /// path and the rare owned fallback share one classification instead of
    /// two copies of the same match.
    fn from_text(s: Cow<'a, str>) -> Self {
        match TextShape::classify(&s) {
            TextShape::DateTime(value) => Self::DateTime(value),
            TextShape::Date(value) => {
                Self::DateTime(DateTimeValue::from(value))
            }
            TextShape::Duration(dv) => Self::Duration(dv.to_seconds()),
            TextShape::Plain => Self::Text(s),
        }
    }

    const fn kind(&self) -> NoteFieldType {
        match self {
            Self::Null => NoteFieldType::Null,
            Self::Bool(_) => NoteFieldType::Bool,
            Self::Number(_) => NoteFieldType::Number,
            Self::Duration(_) => NoteFieldType::Duration,
            Self::DateTime(_) => NoteFieldType::Temporal,
            Self::Text(_) => NoteFieldType::Text,
        }
    }

    /// Rank via the same [`NoteFieldType`] table [`NoteFieldValueRef::compare`]
    /// uses, so this can never diverge from its ordering for the kinds they
    /// share.
    const fn rank(&self) -> u8 {
        self.kind().rank()
    }

    /// Compares two normalized sort keys.
    ///
    /// Null sorts below all values; like kinds compare by value; unlike
    /// non-null kinds order by rank.
    pub(super) fn cmp(&self, other: &Self) -> Ordering {
        match (self, other) {
            (Self::Null, Self::Null) => Ordering::Equal,
            (Self::Null, _) => Ordering::Less,
            (_, Self::Null) => Ordering::Greater,
            (Self::Bool(a), Self::Bool(b)) => a.cmp(b),
            (Self::Number(a), Self::Number(b)) => {
                normalize_zero(*a).total_cmp(&normalize_zero(*b))
            }
            (Self::Duration(a), Self::Duration(b)) => a.cmp(b),
            (Self::DateTime(a), Self::DateTime(b)) => a.cmp(b),
            (Self::Text(a), Self::Text(b)) => a.cmp(b),
            _ => self.rank().cmp(&other.rank()),
        }
    }
}

/// `-0.0` and `0.0` both normalize to `0.0` before `total_cmp`, matching
/// [`NoteFieldValueRef::compare`]'s signed-zero handling so `SortKey::cmp`
/// and `NoteFieldValueRef::compare` never disagree on a `Number` pair.
fn normalize_zero(n: f64) -> f64 {
    if n == 0.0 {
        0.0
    } else {
        n
    }
}

/// Result of inspecting free text for a date, date-time, or duration shape.
/// Shared by [`SortKey::from_text`] and
/// `filter::ComparisonExpr::classify_literal` so both call one classifier,
/// not two copies of the same date/duration heuristic.
pub(super) enum TextShape {
    /// Text parsed as an ISO date-time.
    DateTime(DateTimeValue),
    /// Text parsed as an ISO date with no time component.
    Date(DateValue),
    /// Text parsed as a duration such as `1h30m`.
    Duration(crate::DurationValue),
    /// Text with none of the above shape; kept as plain text.
    Plain,
}

impl TextShape {
    /// Inspects free text for a date, date-time, or duration shape.
    ///
    /// Tries [`DateTimeValue::parse_iso`] before [`DateValue::parse_iso`]: a
    /// full date-time string always fails `DateValue`'s whole-string match,
    /// so trying it second never misclassifies. Duration parsing is guarded
    /// by [`crate::DurationValue::can_start`], an `O(1)` leading-character
    /// check, so non-duration-shaped text (e.g. a plain title) never pays
    /// for `DurationValue::parse`'s allocating error path.
    pub(super) fn classify(s: &str) -> Self {
        let trimmed = s.trim();
        if DateValue::has_four_digit_year(trimmed) {
            if let Ok(value) = DateTimeValue::parse_iso(s) {
                return Self::DateTime(value);
            }
            if let Ok(value) = DateValue::parse_iso(s) {
                return Self::Date(value);
            }
        }
        if crate::DurationValue::can_start(trimmed)
            && let Ok(dv) = crate::DurationValue::parse(s)
        {
            return Self::Duration(dv);
        }
        Self::Plain
    }
}

#[cfg(test)]
mod tests {
    use std::{path::Path, sync::Arc};

    use super::super::*;
    use crate::FileIndex;

    fn outcome_for_files(_temp: &Path, files: &[(&str, &str)]) -> QuerySet {
        let index = Arc::new(FileIndex::new_test(files));
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

            // Dataview treats null as the minimum value: first ascending, last
            // descending.
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

    mod sort_key_cmp {
        use std::{borrow::Cow, cmp::Ordering};

        use pretty_assertions::assert_eq;
        use rstest::rstest;

        use super::super::{
            NullPlacement, SortDirection, SortKey, SortOrder, SortTerm,
        };
        use crate::{DurationSeconds, query::grammar::FieldPath};

        #[rstest]
        #[case::number(SortKey::Number(1.0))]
        #[case::text(SortKey::Text(Cow::Borrowed("hello")))]
        #[case::boolean(SortKey::Bool(true))]
        fn null_sorts_below_every_non_null_key(
            #[case] non_null: SortKey<'static>,
        ) {
            assert_eq!(SortKey::Null.cmp(&non_null), Ordering::Less);
            assert_eq!(non_null.cmp(&SortKey::Null), Ordering::Greater);
        }

        #[test]
        fn compares_durations_numerically() {
            let one_hour =
                SortKey::Duration(DurationSeconds::try_from(3_600.0).unwrap());
            let thirty_mins =
                SortKey::Duration(DurationSeconds::try_from(1_800.0).unwrap());
            assert_eq!(one_hour.cmp(&thirty_mins), Ordering::Greater);
            assert_eq!(thirty_mins.cmp(&one_hour), Ordering::Less);
        }

        #[test]
        fn unlike_kinds_order_by_rank() {
            let number = SortKey::Number(100.0);
            let text = SortKey::Text(Cow::Borrowed("abc"));
            assert_eq!(number.cmp(&text), Ordering::Less);
        }

        #[test]
        fn nan_and_infinity_compare_via_total_cmp_not_partial_cmp() {
            let nan = SortKey::Number(f64::NAN);
            let infinity = SortKey::Number(f64::INFINITY);
            assert_eq!(nan.cmp(&infinity), f64::NAN.total_cmp(&f64::INFINITY));
        }

        #[test]
        fn negative_zero_and_positive_zero_compare_equal() {
            let negative_zero = SortKey::Number(-0.0);
            let zero = SortKey::Number(0.0);
            assert_eq!(negative_zero.cmp(&zero), Ordering::Equal);
        }

        #[test]
        fn null_placement_seam_controls_null_ordering_in_compare_keys() {
            let path = FieldPath::parse("rating").unwrap();
            let first_order = SortOrder {
                terms: Box::new([SortTerm {
                    path: path.clone(),
                    direction: SortDirection::Ascending,
                    null_placement: NullPlacement::First,
                }]),
            };
            let last_order = SortOrder {
                terms: Box::new([SortTerm {
                    path,
                    direction: SortDirection::Ascending,
                    null_placement: NullPlacement::Last,
                }]),
            };
            let null_key = [SortKey::Null];
            let num_key = [SortKey::Number(5.0)];

            // Null on the left.
            assert_eq!(
                first_order.compare_keys(&null_key, &num_key),
                Ordering::Less
            );
            assert_eq!(
                last_order.compare_keys(&null_key, &num_key),
                Ordering::Greater
            );

            // Null on the right (symmetric).
            assert_eq!(
                first_order.compare_keys(&num_key, &null_key),
                Ordering::Greater
            );
            assert_eq!(
                last_order.compare_keys(&num_key, &null_key),
                Ordering::Less
            );

            // Null on both sides is always Equal, regardless of placement.
            assert_eq!(
                first_order.compare_keys(&null_key, &null_key),
                Ordering::Equal
            );
            assert_eq!(
                last_order.compare_keys(&null_key, &null_key),
                Ordering::Equal
            );
        }

        #[test]
        fn auto_placement_puts_null_first_ascending_and_last_descending() {
            let path = FieldPath::parse("rating").unwrap();
            let asc_order = SortOrder {
                terms: Box::new([SortTerm {
                    path: path.clone(),
                    direction: SortDirection::Ascending,
                    null_placement: NullPlacement::Auto,
                }]),
            };
            let desc_order = SortOrder {
                terms: Box::new([SortTerm {
                    path,
                    direction: SortDirection::Descending,
                    null_placement: NullPlacement::Auto,
                }]),
            };
            let null_key = [SortKey::Null];
            let num_key = [SortKey::Number(5.0)];

            // Ascending: Null sorts first (Less).
            assert_eq!(
                asc_order.compare_keys(&null_key, &num_key),
                Ordering::Less
            );
            // Descending: Null sorts last (Less before reversal -> Greater).
            assert_eq!(
                desc_order.compare_keys(&null_key, &num_key),
                Ordering::Greater
            );
        }
    }

    mod sort_key_normalization {
        use std::{borrow::Cow, cmp::Ordering, path::PathBuf};

        use pretty_assertions::assert_eq;

        use super::super::{QueryFieldValueRef, SortKey};
        use crate::{
            DateTimeValue, DateValue, DurationSeconds, DurationValue,
            NoteFieldValue, NoteFieldValueRef, Tag,
        };

        fn date(s: &str) -> DateValue {
            DateValue::parse_iso(s).expect("valid date")
        }

        fn datetime(s: &str) -> DateTimeValue {
            DateTimeValue::parse_iso(s).expect("valid datetime")
        }

        #[test]
        fn from_value_ref_promotes_a_date_to_midnight_datetime() {
            let key = SortKey::from_value_ref(QueryFieldValueRef::Note(
                NoteFieldValueRef::Date(date("2026-07-29")),
            ));
            assert_eq!(
                key,
                SortKey::DateTime(DateTimeValue::from(date("2026-07-29")))
            );
        }

        #[test]
        fn from_value_ref_keeps_a_datetime_as_is() {
            let value = datetime("2026-07-29T14:30:00");
            let key = SortKey::from_value_ref(QueryFieldValueRef::Note(
                NoteFieldValueRef::DateTime(value),
            ));
            assert_eq!(key, SortKey::DateTime(value));
        }

        #[test]
        fn from_value_ref_sniffs_a_date_shaped_text_field() {
            let key = SortKey::from_value_ref(QueryFieldValueRef::Note(
                NoteFieldValueRef::String("2026-07-29"),
            ));
            assert_eq!(
                key,
                SortKey::DateTime(DateTimeValue::from(date("2026-07-29")))
            );
        }

        #[test]
        fn from_value_ref_sniffs_a_duration_shaped_text_field() {
            let key = SortKey::from_value_ref(QueryFieldValueRef::Note(
                NoteFieldValueRef::String("1h30m"),
            ));
            assert!(matches!(key, SortKey::Duration(_)));
        }

        #[test]
        fn from_value_ref_extracts_duration_directly() {
            let dv = DurationValue::parse("1h 30m").expect("valid duration");
            let key = SortKey::from_value_ref(QueryFieldValueRef::Note(
                NoteFieldValueRef::Duration(&dv),
            ));
            assert_eq!(
                key,
                SortKey::Duration(DurationSeconds::try_from(5_400.0).unwrap())
            );
        }

        #[test]
        fn from_value_ref_falls_back_to_text_for_plain_strings() {
            let key = SortKey::from_value_ref(QueryFieldValueRef::Note(
                NoteFieldValueRef::String("custom"),
            ));
            assert_eq!(key, SortKey::Text("custom".into()));
        }

        #[test]
        fn from_value_ref_classifies_an_owned_fallback_string() {
            let owned = QueryFieldValueRef::Owned(NoteFieldValue::String(
                "1h30m".to_owned(),
            ));
            let key = SortKey::from_value_ref(owned);
            assert!(matches!(key, SortKey::Duration(_)));
        }

        #[test]
        fn from_value_ref_borrows_text_for_note_string() {
            let note_str = "hello";
            let key = SortKey::from_value_ref(QueryFieldValueRef::Note(
                NoteFieldValueRef::String(note_str),
            ));
            assert!(
                matches!(key, SortKey::Text(Cow::Borrowed(s)) if s == "hello")
            );
        }

        #[test]
        fn from_value_ref_owns_text_for_fallback_string() {
            let owned_str = "hello".to_owned();
            let key = SortKey::from_value_ref(QueryFieldValueRef::Owned(
                NoteFieldValue::String(owned_str),
            ));
            assert!(
                matches!(key, SortKey::Text(Cow::Owned(ref s)) if s == "hello")
            );
        }

        #[test]
        fn from_value_ref_maps_tags_to_null() {
            let tags = [Tag::parse("#book").expect("valid tag")];
            assert_eq!(
                SortKey::from_value_ref(QueryFieldValueRef::Tags(&tags)),
                SortKey::Null
            );
        }

        #[test]
        fn from_value_ref_maps_inlinks_to_null() {
            let inlinks = [PathBuf::from("notes/a.md")];
            assert_eq!(
                SortKey::from_value_ref(QueryFieldValueRef::Inlinks(&inlinks)),
                SortKey::Null
            );
        }

        #[test]
        fn from_value_ref_maps_list_to_null() {
            let list = [NoteFieldValue::Number(1.0)];
            assert_eq!(
                SortKey::from_value_ref(QueryFieldValueRef::Note(
                    NoteFieldValueRef::List(&list)
                )),
                SortKey::Null
            );
        }

        #[test]
        fn from_value_ref_maps_object_to_null() {
            let object = indexmap::IndexMap::new();
            assert_eq!(
                SortKey::from_value_ref(QueryFieldValueRef::Note(
                    NoteFieldValueRef::Object(&object)
                )),
                SortKey::Null
            );
        }

        #[test]
        fn from_value_ref_maps_link_to_text_by_target() {
            let link = crate::note::Link::new(
                "target",
                "text",
                crate::note::LinkType::Markdown,
            );
            let key = SortKey::from_value_ref(QueryFieldValueRef::Note(
                NoteFieldValueRef::Link(&link),
            ));
            assert_eq!(key, SortKey::Text("target".into()));
        }

        #[test]
        fn a_date_key_sorts_before_a_same_day_afternoon_datetime_key() {
            let date_key = SortKey::from_value_ref(QueryFieldValueRef::Note(
                NoteFieldValueRef::Date(date("2026-07-29")),
            ));
            let datetime_key = SortKey::from_value_ref(
                QueryFieldValueRef::Note(NoteFieldValueRef::DateTime(
                    datetime("2026-07-29T14:30:00"),
                )),
            );
            assert_eq!(date_key.cmp(&datetime_key), Ordering::Less);
        }

        #[test]
        fn sort_key_and_note_field_value_ref_agree_on_shared_kinds() {
            let dv1 = DurationValue::parse("1h").unwrap();
            let dv2 = DurationValue::parse("2h").unwrap();
            let pairs: &[(NoteFieldValueRef<'_>, NoteFieldValueRef<'_>)] = &[
                (NoteFieldValueRef::Null, NoteFieldValueRef::Bool(true)),
                (NoteFieldValueRef::Bool(false), NoteFieldValueRef::Bool(true)),
                (
                    NoteFieldValueRef::Number(1.0),
                    NoteFieldValueRef::Number(2.0),
                ),
                (
                    NoteFieldValueRef::Duration(&dv1),
                    NoteFieldValueRef::Duration(&dv2),
                ),
                (
                    NoteFieldValueRef::Date(date("2026-01-01")),
                    NoteFieldValueRef::DateTime(datetime(
                        "2026-06-01T00:00:00",
                    )),
                ),
                (
                    NoteFieldValueRef::String("a"),
                    NoteFieldValueRef::String("b"),
                ),
                // Cross-kind pairs across each adjacent shared rank boundary.
                (NoteFieldValueRef::Bool(true), NoteFieldValueRef::Number(0.0)),
                (
                    NoteFieldValueRef::Number(100.0),
                    NoteFieldValueRef::Duration(&dv1),
                ),
                (
                    NoteFieldValueRef::Duration(&dv1),
                    NoteFieldValueRef::Date(date("2026-01-01")),
                ),
                (
                    NoteFieldValueRef::Date(date("2026-01-01")),
                    NoteFieldValueRef::String("a"),
                ),
            ];
            for (a, b) in pairs {
                let note_ord = a.compare(b);
                let sort_ord = SortKey::from_value_ref(
                    QueryFieldValueRef::Note(*a),
                )
                .cmp(&SortKey::from_value_ref(QueryFieldValueRef::Note(*b)));
                assert_eq!(note_ord, sort_ord, "diverged for {a:?} vs {b:?}");
            }
        }
    }

    mod text_shape {
        use super::super::TextShape;

        #[test]
        fn classifies_iso_datetime_string() {
            let shape = TextShape::classify("2026-07-29T14:30:00");
            assert!(matches!(shape, TextShape::DateTime(_)));
        }

        #[test]
        fn classifies_iso_date_string() {
            let shape = TextShape::classify("2026-07-29");
            assert!(matches!(shape, TextShape::Date(_)));
        }

        #[test]
        fn classifies_duration_string() {
            let shape = TextShape::classify("1h30m");
            assert!(matches!(shape, TextShape::Duration(_)));
        }

        #[test]
        fn classifies_plain_string() {
            let shape = TextShape::classify("custom title");
            assert!(matches!(shape, TextShape::Plain));
        }

        #[test]
        fn classifies_bare_number_string_as_plain() {
            let shape = TextShape::classify("42");
            assert!(matches!(shape, TextShape::Plain));
        }

        #[test]
        fn classifies_invalid_date_as_plain() {
            let shape = TextShape::classify("2026-99-99");
            assert!(matches!(shape, TextShape::Plain));
        }

        #[test]
        fn classifies_empty_string_as_plain() {
            let shape = TextShape::classify("");
            assert!(matches!(shape, TextShape::Plain));
        }
    }
}
