//! Parsing and evaluation for the query module's two DSLs: the `--from`
//! source-selection language ([`source`]) and the `--where` record-filter
//! language ([`filter`]), sharing the generic precedence parser in
//! [`logic`]. [`comparison`] and [`field`] implement the
//! comparison-operator and field-path accessor grammars each DSL builds
//! on.

mod comparison;
mod field;
mod filter;
mod logic;
mod source;

pub(crate) use field::{FieldPath, FileField, TaskField};
pub(crate) use filter::FilterExpr;
pub use source::{ClassExpansionMode, QuerySource};
pub(crate) use source::{
    FileClassExpander, QuerySourceExpr, SourceAtom, compile_glob,
    resolve_classes,
};
