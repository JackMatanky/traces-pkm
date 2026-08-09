//! Schema resolution engine for typed frontmatter.
//!
//! A Schema is a TOML file under `.traces/schemas/` whose filename stem is the
//! Schema name. [`SchemaRegistry::load`] reads that directory and resolves the
//! `extends` DAG via [`resolve::resolve`], a pure function implementing Kahn's
//! topological sort: own fields override parents, first-listed parent wins on
//! ties, `excludes` drops inherited fields, and `$ref` targets the Global
//! Schema or a transitive `extends` ancestor.
//!
//! # Main Types
//!
//! - [`SchemaRegistry`]: reads and resolves every Schema under a directory
//!   (`registry.rs`), the module's only filesystem access.
//! - [`Schema`]: one Schema's effective [`model::FieldDefinition`]s plus
//!   transitive `extends` ancestors for is-a matching (`model.rs`).
//! - [`model::FieldOptions`]/[`model::FieldType`]: type-specific resolved
//!   shapes that each field carries.
//! - [`SchemaError`]/[`SchemaWarning`]: hard failures and recoverable degrades;
//!   see [`resolve::resolve`] for which conditions produce each.
//!
//! # Scope
//!
//! Resolves Schemas from the filesystem only. Does not expose them to
//! minijinja templates, filter `file`-typed fields against a note index, or
//! run is-a class queries over notes. Those integrations consume
//! [`SchemaRegistry`] and [`Schema::is_a`] from outside this module.

mod address;
mod error;
mod model;
mod name;
mod raw;
mod registry;
mod resolve;

pub(crate) use error::{SchemaError, SchemaWarning};
pub(crate) use model::Schema;
pub(crate) use registry::SchemaRegistry;

/// Reserved Global Schema name (`global.toml`).
///
/// A flat `$ref`-able reference pool: never a Note's File Class, never inherits
/// via its own `extends`, and `required = true` fields degrade to `false` with
/// a [`SchemaWarning::StrayGlobalRequired`] during resolution.
pub(crate) const GLOBAL_SCHEMA_NAME: &str = "global";
