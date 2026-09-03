//! Query source selection, field resolution, and result transformation.
//!
//! [`QueryService`] borrows a [`FileIndex`] and executes a [`QueryBuilder`].
//! The pipeline selects Notes via [`SourceSelector`], pairs each matching Note
//! with its [`FileBase`] as a [`QueryRow`], and applies chained
//! transformations through [`QuerySet`].
//!
//! # Source Expression Language
//!
//! A source expression is a boolean combination of **leaves** joined by
//! **logical operators** (`and`, `or`, `not`) and grouped with **parentheses**.
//!
//! ## Leaves
//!
//! ### Tags
//!
//! A `#`-prefixed identifier matches Notes carrying that tag or any nested
//! sub-tag. Tag names may contain letters, digits, underscores, hyphens, dots,
//! and forward slashes.
//!
//! ### Paths
//!
//! A path leaf matches an exact file path, every file under a folder prefix,
//! or an explicit glob.
//!
//! ### File Classes
//!
//! File Class leaves match Notes whose frontmatter class field contains the
//! named class or a transitive descendant.
//!
//! # Main Types
//!
//! - [`QueryService`] drives query execution: [`QueryService::run`] borrows a
//!   [`FileIndex`] and a [`QueryBuilder`], producing a [`QuerySet`].
//! - [`QueryBuilder`] describes page/task mode, source selection, and ordered
//!   transformations.
//! - [`SourceSelector`] is the top-level entry point: either all Notes or a
//!   parsed expression.
//! - [`QueryRow`] pairs a [`FileBase`] with its parsed [`Note`] and resolves
//!   `file.*`, `task.*`, frontmatter, tag, and inlinks fields.
//! - [`QuerySet`] stores result rows and provides chained transformation
//!   methods ([`filter`](QuerySet::filter), [`sort`](QuerySet::sort),
//!   [`limit`](QuerySet::limit), [`group_by`](QuerySet::group_by),
//!   [`flatten`](QuerySet::flatten)) and terminal rendering methods
//!   ([`table`](QuerySet::table), [`list`](QuerySet::list),
//!   [`task_list`](QuerySet::task_list)).
//! - [`QueryError`] reports malformed field paths, invalid expressions, and
//!   transformation constraint violations.
//!
//! # Examples
//!
//! ```rust
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
pub(crate) use sort::SortOrder;

#[cfg(test)]
pub(super) mod test_support {
    use std::{fs, path::Path, sync::Arc};

    use super::*;
    use crate::index::IndexerService;

    /// Builds a [`QuerySet`] over every Markdown Note in `files`
    /// written under `temp`.
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

    /// Builds a single-record [`QuerySet`] from a single Markdown
    /// Note's content.
    pub(super) fn outcome_for(temp: &Path, content: &str) -> QuerySet {
        outcome_for_files(temp, &[("note.md", content)])
    }

    /// Finds a [`FileEntry`] by path in a sorted entries slice.
    pub(super) fn find_entry<'a>(
        entries: &'a [crate::index::FileEntry],
        path: &Path,
    ) -> &'a crate::index::FileEntry {
        entries
            .iter()
            .find(|e| e.file().path() == path)
            .expect("entry not found")
    }

    /// Finds a [`FileBase`] by path in a sorted entries slice.
    pub(super) fn find_base<'a>(
        entries: &'a [crate::index::FileEntry],
        path: &Path,
    ) -> &'a crate::file::FileBase {
        find_entry(entries, path).file()
    }
}
