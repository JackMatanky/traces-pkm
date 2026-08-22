//! Scan, persist, load, and refresh a file index over a project root.
//!
//! [`IndexerService`] owns a project root and drives the index lifecycle:
//! build, persist, load, and refresh. [`FileIndex`] is the value it
//! produces — a snapshot of every indexed [`FileRecord`] (from
//! [`crate::file`]), each Markdown file's parsed [`Note`], and derived
//! inbound links. `FileIndex` carries no `&Path` of its own; construction and
//! persistence flow entirely through [`IndexerService`].
//!
//! Query execution lives in [`crate::query`]: `QueryService` borrows a
//! [`FileIndex`] through its entry view, keeping `index` focused on indexed
//! data and `query` focused on query semantics.
//!
//! Persistence uses a redb-backed database managed by the [`store`]
//! submodule; callers use [`IndexerService`]'s methods instead of touching
//! redb tables directly.
//!
//! Inbound links between Notes are derived from outlinks during build and
//! refresh, then persisted alongside them; see [`inlinks`].
//!
//! The build pipeline is composed internally by [`builder::IndexBuilder`],
//! which holds a scan result and reuse directive, deferring note parsing,
//! sorting, and inlink derivation to build time.
//!
//! # Lifecycle
//!
//! - Build a fresh index: [`IndexerService::build`]
//! - Persist to disk: [`IndexerService::persist`]
//! - Load from disk: [`IndexerService::load`]
//! - Refresh against the filesystem: [`IndexerService::refresh`]
//!
//! - [`FileIndex::records`] and [`FileIndex::notes`] expose sorted indexed data
//!   for direct inspection.
//! - [`FileIndex::entries`] exposes a borrowed, allocation-free view pairing
//!   records with optional Notes and inbound links for query execution.
//!
//! [`store`]: mod@store
//! [`inlinks`]: mod@inlinks
//! [`builder::IndexBuilder`]: mod@builder

mod builder;
mod entry;
mod error;
mod inlinks;
mod scan;
mod service;
mod store;

use std::path::Path;

pub(crate) use entry::FileIndexEntry;
#[allow(unused_imports, reason = "re-exported for downstream callers")]
pub use error::{FileIndexError, IndexBuilderError};
pub(crate) use inlinks::InlinkMap;
pub use service::IndexerService;

pub(crate) use crate::file::FileFormat;
pub use crate::file::FileRecord;
use crate::note::Note;

/// Project-relative path of the persisted [`FileIndex`] database.
const INDEX_FILE: &str = ".traces/index.redb";

/// Persisted cache of file records, parsed Note metadata, and derived inbound
/// links.
///
/// Every regular file under the project root contributes a [`FileRecord`].
/// Markdown files also contribute a [`Note`], accessible through
/// [`Self::notes`]. A pure value type: [`IndexerService`] produces, persists,
/// and loads it; `FileIndex` itself carries no `&Path`.
#[derive(Clone, Debug)]
pub struct FileIndex {
    records: Vec<FileRecord>,
    notes: Vec<Note>,
    /// Inbound links, keyed by target path; see [`inlinks::derive_inlinks`].
    ///
    /// Recomputed in full whenever [`IndexerService::refresh`] finds changed
    /// content. Reused unchanged from the last persisted computation
    /// otherwise.
    inlinks: InlinkMap,
}

impl FileIndex {
    /// Returns indexed [`FileRecord`]s, sorted by path.
    ///
    /// Every regular file under the project root contributes one record.
    /// Markdown files also have a corresponding [`Note`] accessible via
    /// [`Self::notes`].
    #[inline]
    #[must_use]
    pub fn records(&self) -> &[FileRecord] {
        &self.records
    }

    /// Returns indexed [`Note`] records, sorted by path.
    ///
    /// Only markdown files produce notes. Non-markdown files appear in
    /// [`Self::records`] but not here.
    #[inline]
    #[must_use]
    #[cfg_attr(
        not(any(test, feature = "test-utils")),
        expect(
            dead_code,
            reason = "no current caller outside tests; CLI exposes \
                      FileIndex::records but not the parsed Note view yet"
        )
    )]
    pub fn notes(&self) -> &[Note] {
        &self.notes
    }

    /// Returns the [`Note`] for the note at `path`, if indexed.
    ///
    /// # Performance
    ///
    /// O(log n): [`Self::notes`] is kept sorted by path, so this binary
    /// searches rather than scanning.
    #[inline]
    #[must_use]
    #[cfg(test)]
    pub(crate) fn note(&self, path: &Path) -> Option<&Note> {
        find_by_path(&self.notes, path)
    }
}

/// Binary-searches path-sorted `notes` for an exact path match.
///
/// Shared by the [`inlinks`] submodule, which needs the same search over a
/// bare `&[Note]` slice while resolving link targets during
/// [`IndexerService::build`]/[`IndexerService::refresh`].
///
/// [`inlinks`]: mod@inlinks
fn find_by_path<'a>(notes: &'a [Note], path: &Path) -> Option<&'a Note> {
    let idx = notes.binary_search_by(|note| note.path().cmp(path)).ok()?;
    notes.get(idx)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    /// Shared test fixtures live here so `scan.rs` and `store.rs` tests can
    /// import them without duplicating the definitions.
    pub(crate) mod fixtures {
        use std::{fs, path::Path};

        /// Restores a locked directory's permissions on drop, even if the
        /// test panics. Otherwise, a `0o000` or `0o500` directory blocks the
        /// tempdir's own cleanup.
        #[cfg(unix)]
        pub struct RestorePermissions<'a>(pub &'a Path);

        #[cfg(unix)]
        impl Drop for RestorePermissions<'_> {
            fn drop(&mut self) {
                use std::os::unix::fs::PermissionsExt as _;

                let _ = fs::set_permissions(
                    self.0,
                    fs::Permissions::from_mode(0o700),
                );
            }
        }
    }

    mod lookup {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn returns_none_when_note_path_is_not_indexed() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let index =
                IndexerService::new(temp.path()).build().expect("build index");

            assert_eq!(index.note(Path::new("nonexistent.md")), None);
        }

        #[test]
        fn returns_the_matching_note_when_path_is_indexed() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("a.md"), "# A").expect("write a");
            fs::write(temp.path().join("b.md"), "# B").expect("write b");
            fs::write(temp.path().join("c.md"), "# C").expect("write c");
            let index =
                IndexerService::new(temp.path()).build().expect("build index");

            assert_eq!(
                index.note(Path::new("b.md")).map(Note::path),
                Some(Path::new("b.md"))
            );
        }
    }
}
