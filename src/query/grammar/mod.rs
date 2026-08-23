//! Parsing and evaluation for the query module's two DSLs: the `--from`
//! source-selection language ([`source`]) and the `--where` record-filter
//! language ([`filter`]), sharing the generic boolean-expression parser
//! in [`expr`]. [`field`] implements the field-path accessor grammar
//! both DSLs build on, and [`lex`] provides the shared token stream.
//!
//! # Main Types
//!
//! - [`FieldPath`] - Resolved field accessor (`file.*`, `task.*`, tags,
//!   inlinks, or bare frontmatter keys)
//! - [`FileField`] - `file.<field>` accessor backed by file metadata
//! - [`TaskField`] - `task.<field>` accessor valid on task-level records
//! - [`FilterExpr`] - Parsed `--where` filter expression AST
//! - [`SourceSelector`] - Top-level `--from` source selector (all Notes or a
//!   parsed expression)

mod expr;
mod field;
mod filter;
mod lex;
mod source;

pub(crate) use field::{FieldPath, FileField, TaskField};
pub(crate) use filter::FilterExpr;
pub use source::{ClassExpansionMode, SourceSelector};
pub(crate) use source::{FileClassExpander, SourceAtom, SourceExpr};
