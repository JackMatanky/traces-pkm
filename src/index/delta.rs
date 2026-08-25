//! [`IndexDelta`]/[`IncrementalDelta`]: the persistence plan produced by
//! [`super::builder::IndexBuilder::build`], describing what changed since
//! the last persist. The diffing that produces an [`IncrementalDelta`]
//! lives in [`super::cache::RefreshCache`], not here — this module holds
//! only the resulting data shape.

use std::path::PathBuf;

/// Per-path persistence plan produced by [`IndexBuilder::build`].
///
/// [`IndexStore::persist_index`] reads this to choose between a full
/// [`IndexStore::replace_all`] rewrite (fresh build — no previous persisted
/// state to diff against) and a row-level incremental write (refresh — only
/// paths that actually changed since the last persist).
///
/// [`Incremental`]'s payload is boxed: `IndexDelta` is a field of
/// [`super::FileIndex`], and every [`Full`] build (the common case for a
/// first-time index) would otherwise pay for the largest variant's four inline
/// `Vec`s regardless of which variant is active. Boxing shrinks `IndexDelta`
/// from 96 bytes to 8.
///
/// `Full` and `Incremental` are not interchangeable, even when an `Incremental`
/// diff would come out empty: `Full` (`replace_all`) unconditionally wipes all
/// three tables before rewriting, so it never needs to know what was deleted.
/// `Incremental` (`persist_incremental`) only deletes paths its diff explicitly
/// names, which is only correct because that diff is always computed against a
/// `RefreshCache` loaded from the real, currently-persisted store (via
/// `RefreshCache::load`, the only constructor — private fields make this a
/// type-level guarantee). A `Full`-built `FileIndex` retagged `Incremental`
/// against a fabricated empty previous state would silently orphan any row for
/// a file deleted since the last persist.
///
/// [`IndexStore::persist_index`]: `super::store::IndexStore::persist_index`
/// [`IndexStore::replace_all`]: `super::store::IndexStore::replace_all`
/// [`IndexBuilder::build`]: `super::builder::IndexBuilder::build`
/// [`Incremental`]: IndexDelta::Incremental
/// [`Full`]: IndexDelta::Full
#[derive(Clone, Debug)]
pub(crate) enum IndexDelta {
    /// Produced by a fresh build: no previous state exists to diff against.
    Full,
    /// Produced by a refresh that reused a previous index.
    Incremental(Box<IncrementalDelta>),
}

/// The changed-path plan behind [`IndexDelta::Incremental`].
#[derive(Clone, Debug)]
pub(crate) struct IncrementalDelta {
    /// Paths whose `FileBase` (and `Note`, if applicable) must be upserted
    /// into `FILES`/`NOTES` — added or metadata-changed since the last
    /// persist.
    pub(crate) upserted: Vec<PathBuf>,
    /// Paths removed since the last persist — must be deleted from
    /// `FILES`/`NOTES`.
    pub(crate) deleted: Vec<PathBuf>,
    /// Target paths in `LINKS` whose source set is new or changed — `None`
    /// when inlinks were reused unchanged (nothing to write).
    pub(crate) links_upserted: Option<Vec<PathBuf>>,
    /// Target paths removed from `LINKS` — always present alongside
    /// `links_upserted` (both `None`/both populated; empty `Vec` is a valid
    /// "no removals" case, distinct from `None`'s "inlinks unchanged,
    /// nothing computed").
    pub(crate) links_deleted: Vec<PathBuf>,
}

impl IncrementalDelta {
    /// True if this delta names no changes at all — persisting it would
    /// open a write transaction only to commit nothing.
    pub(crate) fn is_empty(&self) -> bool {
        self.upserted.is_empty()
            && self.deleted.is_empty()
            && self.links_deleted.is_empty()
            && self.links_upserted.as_ref().is_none_or(Vec::is_empty)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    mod is_empty {
        use rstest::rstest;

        use super::*;

        /// Builds an [`IncrementalDelta`] with all-empty fields except the
        /// one under test, so each case isolates exactly one field's
        /// contribution to [`IncrementalDelta::is_empty`].
        fn delta(
            upserted: Vec<PathBuf>,
            deleted: Vec<PathBuf>,
            links_upserted: Option<Vec<PathBuf>>,
            links_deleted: Vec<PathBuf>,
        ) -> IncrementalDelta {
            IncrementalDelta {
                upserted,
                deleted,
                links_upserted,
                links_deleted,
            }
        }

        #[test]
        fn is_true_when_every_field_is_empty_or_none() {
            let delta = delta(vec![], vec![], None, vec![]);

            assert!(delta.is_empty());
        }

        #[test]
        fn is_true_when_links_upserted_is_an_empty_vec_not_none() {
            // `Some(vec![])` ("recomputed, nothing changed") must count as
            // empty alongside `None` ("never recomputed") — the two are
            // different reasons for the same "nothing to write" outcome.
            let delta = delta(vec![], vec![], Some(vec![]), vec![]);

            assert!(delta.is_empty());
        }

        #[rstest]
        #[case::upserted(delta(vec![PathBuf::from("a.md")], vec![], None, vec![]))]
        #[case::deleted(delta(vec![], vec![PathBuf::from("a.md")], None, vec![]))]
        #[case::links_deleted(delta(vec![], vec![], None, vec![PathBuf::from("a.md")]))]
        #[case::links_upserted_nonempty(delta(
            vec![],
            vec![],
            Some(vec![PathBuf::from("a.md")]),
            vec![]
        ))]
        fn is_false_when_any_field_names_a_change(
            #[case] delta: IncrementalDelta,
        ) {
            assert!(!delta.is_empty());
        }
    }
}
