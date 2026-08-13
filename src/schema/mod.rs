//! Resolve typed frontmatter Schemas into effective field definitions.
//!
//! Schema TOML files live under `.traces/schemas/`; each filename stem becomes
//! the Schema name. [`SchemaRegistry::load`] reads that directory and passes
//! the parsed TOML to [`resolve::resolve`]. The resolver linearizes the
//! `extends` DAG with Kahn's topological sort, resolves parents before
//! children, merges parent fields in declaration order, applies `excludes`, and
//! lets own fields override inherited fields. `$ref` targets are bounded to the
//! Global Schema or a transitive `extends` ancestor.
//!
//! # Main Types
//!
//! - [`SchemaRegistry`] reads and resolves every Schema under a directory.
//! - [`Schema`] stores effective [`model::FieldDefinition`]s and transitive
//!   `extends` ancestors for is-a matching.
//! - [`model::FieldOptions`] and [`model::FieldType`] describe resolved
//!   type-specific field shapes.
//! - [`SchemaError`] and [`SchemaWarning`] describe hard failures and recovered
//!   defects.
//!
//! # Scope
//!
//! Resolves Schemas from the filesystem only. Template exposure, `file` field
//! filtering against the note index, and note class queries consume
//! [`SchemaRegistry`] and [`Schema::is_a`] from outside this module.

mod address;
mod error;
mod graph;
mod model;
mod name;
mod raw;
mod registry;
mod resolve;

pub(crate) use error::SchemaError;
pub(crate) use model::{Schema, SchemaFileFieldFilter};
pub(crate) use registry::{SchemaRegistry, resolve_sources};

/// Name the reserved Global Schema file stem.
///
/// A flat `$ref`-able reference pool: never a Note's File Class, never inherits
/// via its own `extends`, and `required = true` fields degrade to `false` with
/// a [`SchemaWarning::StrayGlobalRequired`] during resolution.
pub(crate) const GLOBAL_SCHEMA_NAME: &str = "global";
