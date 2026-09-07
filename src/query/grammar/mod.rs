//! Query DSL parsers for source selection, record filtering, and field access.
//!
//! [`source`] parses `--from`, [`filter`] parses `--where`, [`expr`] supplies
//! the shared boolean-expression grammar, and [`field`] resolves field paths.

mod expr;
mod field;
mod filter;
mod source;

pub(crate) use expr::BooleanExpr;
pub(crate) use field::{FieldPath, FileField, TaskField};
pub(crate) use filter::FilterExpr;
pub use source::SourceSelector;
pub(crate) use source::{
    ClassExpansionMode, FileClassExpander, SourceAtom, SourceExpr,
};
