//! Sorted-by-path view over stored index rows.
//!
//! Every producer that loads rows from disk or the store returns rows sorted
//! by [`HasPath::path`]. Keeping that ordering in the type lets binary
//! searches replace linear scans and turns "keep it sorted" from a comment
//! into an invariant the constructor enforces.

use std::path::Path;

use crate::path::HasPath;

/// Rows sorted ascending by [`HasPath::path`].
///
/// Construct through [`SortedByPath::sorted`] (sorts) or
/// [`SortedByPath::assumed_sorted`] (debug-asserts); lookup methods rely on
/// that order.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SortedByPath<T>(Vec<T>);

impl<T> Default for SortedByPath<T> {
    fn default() -> Self {
        Self(Vec::new())
    }
}

impl<T> SortedByPath<T> {
    /// Wraps already-sorted rows without reordering.
    #[inline]
    pub(super) fn into_vec(self) -> Vec<T> {
        self.0
    }
}

impl<T: HasPath> SortedByPath<T> {
    /// Sorts `rows` ascending by path.
    pub fn sorted(mut rows: Vec<T>) -> Self {
        rows.sort_by(|a, b| a.path().cmp(b.path()));
        Self(rows)
    }

    /// Wraps `rows` the caller has already sorted by path.
    ///
    /// # Panics
    ///
    /// Panics in debug builds when `rows` is not ascending by path.
    #[inline]
    pub fn assumed_sorted(rows: Vec<T>) -> Self {
        debug_assert!(
            rows.windows(2).all(|pair| match pair {
                [a, b] => a.path() <= b.path(),
                _ => true,
            }),
            "assumed_sorted rows must be ascending by path"
        );
        Self(rows)
    }

    /// Returns the rows as a path-ascending slice.
    #[inline]
    #[must_use]
    pub fn as_slice(&self) -> &[T] {
        &self.0
    }

    /// Binary-searches for `path`; see [`slice::binary_search_by`].
    #[inline]
    pub(super) fn binary_search_by_path(
        &self,
        path: &Path,
    ) -> Result<usize, usize> {
        self.0.binary_search_by(|row| row.path().cmp(path))
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[derive(Debug, PartialEq)]
    struct TestRow {
        path: PathBuf,
        payload: u8,
    }

    impl HasPath for TestRow {
        fn path(&self) -> &Path {
            &self.path
        }
    }

    fn row(path: &str, payload: u8) -> TestRow {
        TestRow {
            path: PathBuf::from(path),
            payload,
        }
    }

    mod construction {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn sorts_rows_ascending_by_path() {
            let sorted = SortedByPath::sorted(vec![
                row("c.md", 3),
                row("a.md", 1),
                row("b.md", 2),
            ]);

            let paths: Vec<_> =
                sorted.as_slice().iter().map(HasPath::path).collect();
            assert_eq!(paths, [
                Path::new("a.md"),
                Path::new("b.md"),
                Path::new("c.md")
            ]);
        }

        #[test]
        fn exposes_payloads_unchanged() {
            let sorted =
                SortedByPath::sorted(vec![row("b.md", 2), row("a.md", 1)]);

            let payloads: Vec<_> =
                sorted.as_slice().iter().map(|r| r.payload).collect();
            assert_eq!(payloads, [1, 2]);
        }

        #[test]
        fn wraps_caller_sorted_rows_as_given() {
            let rows = vec![row("a.md", 1), row("b.md", 2)];
            let sorted = SortedByPath::assumed_sorted(rows);

            assert_eq!(sorted.as_slice().len(), 2);
        }

        #[test]
        #[cfg_attr(not(debug_assertions), ignore)]
        #[should_panic(expected = "assumed_sorted rows must be ascending")]
        fn panics_on_unsorted_assumed_input() {
            let _ = SortedByPath::assumed_sorted(vec![
                row("b.md", 2),
                row("a.md", 1),
            ]);
        }
    }

    mod lookup {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn finds_an_existing_path() {
            let sorted = SortedByPath::sorted(vec![
                row("a.md", 1),
                row("b.md", 2),
                row("c.md", 3),
            ]);

            let index =
                sorted.binary_search_by_path(Path::new("b.md")).expect("hit");
            let found = sorted.as_slice().get(index).expect("hit in bounds");
            assert_eq!(found.payload, 2);
        }

        #[test]
        fn reports_insertion_point_for_a_missing_path() {
            let sorted =
                SortedByPath::sorted(vec![row("a.md", 1), row("c.md", 3)]);

            let miss = sorted
                .binary_search_by_path(Path::new("b.md"))
                .expect_err("miss");
            assert_eq!(miss, 1);
        }
    }

    mod redistribute {
        use pretty_assertions::assert_eq;

        use super::*;

        /// Mutating payload fields must not disturb path-order lookup: this
        /// mirrors `redistribute_inlinks`, which only rewrites entry inlinks.
        #[test]
        fn keeps_rows_findable_after_payload_mutation() {
            let mut sorted =
                SortedByPath::sorted(vec![row("a.md", 1), row("b.md", 2)]);
            {
                let rows = sorted.0.as_mut_slice();
                let first = rows.first_mut().expect("two rows sorted");
                first.payload = 99;
            }

            let index = sorted
                .binary_search_by_path(Path::new("a.md"))
                .expect("still findable");
            let found = sorted.as_slice().get(index).expect("hit in bounds");
            assert_eq!(found.payload, 99);
        }
    }
}
