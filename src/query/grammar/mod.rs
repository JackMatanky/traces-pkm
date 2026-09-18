//! Domain-specific query languages for source selection, record filtering, and
//! field access.
//!
//! This module encapsulates the lexing, grammar parsing, and abstract syntax
//! trees for user-supplied query strings:
//! - [`source`]: Parses `--from` source selection expressions into a
//!   [`SourceSelector`].
//! - [`filter`]: Parses `--where` filter expressions into executable
//!   [`FilterExpr`] trees.
//! - [`expr`]: Supplies the shared boolean algebra parser (`not` > `and` >
//!   `or`).
//! - [`field`]: Resolves and validates dotted field paths into structured
//!   [`FieldPath`] accessors.
mod expr;
mod field;
mod filter;
mod source;

pub(crate) use expr::BooleanExpr;
pub(crate) use field::{FieldPath, FileField, ListField, TaskField};
pub(crate) use filter::FilterExpr;
pub use source::SourceSelector;
pub(crate) use source::{
    ClassExpansionMode, FileClassExpander, SourceAtom, SourceExpr,
};
