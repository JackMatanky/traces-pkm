//! Query source selection, field resolution, and result transformation.
//!
//! Call [`QueryService::run`] with a [`QueryBuilder`] and [`SourceSelector`]
//! to select [`Note`]s from a [`FileIndex`], pair each match with
//! [`FileBase`] metadata in a [`QueryRow`], and transform rows through
//! [`QuerySet`].
//!
//! # Source expression language
//!
//! Source expressions combine leaves with `and`, `or`, `not`, and parentheses.
//! Leaves are:
//!
//! - Tags: `#`-prefixed identifiers matching exact or nested tags. Names may
//!   contain letters, digits, underscores, hyphens, dots, and forward slashes.
//! - Paths: exact file paths, folder prefixes, or explicit globs.
//! - File classes: frontmatter class values matching the named class or a
//!   transitive descendant.
//!
//! Field resolution supports `file.*`, `task.*`, frontmatter, tag, and
//! inlinks fields. [`QueryError`] reports malformed field paths, invalid
//! expressions, and transformation constraint violations.
//!
//! # Examples
//!
//! ```rust
//! # #[cfg(feature = "test-utils")]
//! # {
//! use std::sync::Arc;
//!
//! use traces_pkm::{
//!     IndexerService, QueryBuilder, QueryService, SourceSelector,
//! };
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let temp = tempfile::tempdir()?;
//! std::fs::write(temp.path().join("a.md"), "---\nrating: 5\n---\n")?;
//!
//! let index = Arc::new(IndexerService::new(temp.path()).build()?);
//! let service = QueryService::new("class");
//! let builder =
//!     QueryBuilder::pages(SourceSelector::All).filter("rating >= 5")?;
//!
//! let set = service.run(&index, builder);
//! assert_eq!(set.len(), 1);
//! # Ok(())
//! # }
//! # }
//! ```
//!
//! [`FileBase`]: crate::file::FileBase
//! [`FileIndex`]: crate::index::FileIndex
//! [`Note`]: crate::note::Note
mod builder;
mod error;
mod format;
mod grammar;
mod plan;
mod results;
mod service;
mod sort;
mod value;

pub use builder::QueryBuilder;
use builder::QueryMode;
#[cfg(test)]
pub(crate) use error::{FieldPathError, QuerySyntaxError};
pub use error::{QueryBuilderError, QueryDialect, QueryError, QueryResult};
pub(crate) use format::TaskPathStyle;
pub use grammar::SourceSelector;
pub(crate) use grammar::{
    ClassExpansionMode, FieldPath, FileClassExpander, FileField, SourceAtom,
    SourceExpr,
};
use plan::{QueryPlan, QueryTransform};
pub use results::{QueryRow, QuerySet};
pub use service::QueryService;
pub(crate) use sort::{SortDirection, SortOrder};

#[cfg(test)]
pub(super) mod test_support {
    use std::{fs, path::Path, sync::Arc};

    use super::*;
    use crate::index::IndexerService;

    /// Writes `files` under `temp` and returns an all-notes page query.
    pub(super) fn outcome_for_files(
        temp: &Path,
        files: &[(&str, &str)],
    ) -> QuerySet {
        for (name, content) in files {
            fs::write(temp.join(name), content).expect("write note");
        }
        let index =
            Arc::new(IndexerService::new(temp).build().expect("build index"));
        QueryService::new("class")
            .run(&index, QueryBuilder::pages(SourceSelector::All))
    }

    /// Writes one Markdown Note and returns an all-notes page query.
    pub(super) fn outcome_for(temp: &Path, content: &str) -> QuerySet {
        outcome_for_files(temp, &[("note.md", content)])
    }

    pub(super) fn find_entry<'a>(
        entries: &'a [crate::index::FileEntry],
        path: &Path,
    ) -> &'a crate::index::FileEntry {
        entries
            .iter()
            .find(|e| e.file().path() == path)
            .expect("entry not found")
    }

    pub(super) fn find_base<'a>(
        entries: &'a [crate::index::FileEntry],
        path: &Path,
    ) -> &'a crate::file::FileBase {
        find_entry(entries, path).file()
    }
}
