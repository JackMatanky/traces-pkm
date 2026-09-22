//! Query source selection, record filtering, field resolution, and
//! transformations.
//!
//! The query subsystem evaluates declarative queries over indexed Markdown
//! notes and flat list items stored in a [`WorkspaceIndex`]. It provides a
//! query planning and execution engine supporting filtering, sorting, limiting,
//! grouping, flattening, and formatting into Markdown tables and lists.
//!
//! # Architecture and Pipeline
//!
//! 1. **Specification**: Callers construct a [`QueryBuilder`] in page, list, or
//!    task row granularity using a [`SourceSelector`].
//! 2. **Evaluation**: [`QueryService::run`] evaluates the source expression
//!    against a [`WorkspaceIndex`], optionally expanding File Class hierarchies
//!    via a [`FileClassExpander`].
//! 3. **Row Instantiation**: Matching notes generate [`QueryRow`] items. For
//!    page queries, each note forms one row. For list and task queries, each
//!    list item (or task item) within matching notes forms a zero-allocation
//!    positional list row.
//! 4. **Transformation**: The query planner optimizes operations by fusing
//!    adjacent filters, merging consecutive sort terms, and rewriting
//!    sort-limit pairs into bounded top-k selections.
//! 5. **Materialization**: Results are exposed lazily via [`QuerySet`], caching
//!    intermediate representations across repeat reads and formatting requests.
//!
//! # Query Expressions
//!
//! - **Source Selectors (`--from`)**: Filter notes by `#tag` patterns,
//!   directory paths, file paths, glob patterns, and `@Class` hierarchies
//!   combined with boolean operators (`and`, `or`, `not`, parentheses).
//! - **Filter Expressions (`--where`)**: Filter individual rows using
//!   comparison operators (`==`, `!=`, `<`, `<=`, `>`, `>=`) and
//!   `contains(...)` calls.
//! - **Field Resolution**: Access fields across `file.<field>` metadata,
//!   canonical `list.<field>` properties, note tags, project-relative
//!   `inlinks`, and frontmatter or inline metadata keys.
//!
//! # Key Types
//!
//! - [`QueryBuilder`]: Declarative builder for query configuration.
//! - [`QueryService`]: Evaluation service executing plans against an index.
//! - [`QuerySet`]: Lazily transformed collection of query result rows.
//! - [`QueryRow`]: Positional record view over an indexed file or list item.
//! - [`SourceSelector`]: Source filter specifying target documents.
//! - [`QueryError`]: Top-level error covering syntax, field path, and execution
//!   errors.
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
//! [`WorkspaceIndex`]: crate::index::WorkspaceIndex
//! [`FileClassExpander`]: crate::query::grammar::FileClassExpander
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
pub(crate) use builder::QueryMode;
#[cfg(test)]
pub(crate) use error::{FieldPathError, QuerySyntaxError};
pub use error::{QueryBuilderError, QueryDialect, QueryError, QueryResult};
pub(crate) use format::TaskPathStyle;
pub use grammar::SourceSelector;
pub(crate) use grammar::{
    ClassExpansionMode, FieldPath, FileClassExpander, FileField, ListField,
    SourceAtom, SourceExpr,
};
use plan::{QueryPlan, QueryTransform};
pub use results::{QueryRow, QuerySet};
pub use service::QueryService;
pub(crate) use sort::{SortDirection, SortOrder};

#[cfg(test)]
pub(super) mod test_support {
    use std::path::Path;

    use super::*;
    /// Writes `files` under `temp` and returns an all-notes page query.
    pub(super) fn outcome_for_files(
        _temp: &Path,
        files: &[(&str, &str)],
    ) -> QuerySet {
        let index = crate::build_test_index(files);
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
