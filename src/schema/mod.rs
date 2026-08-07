//! Schema registry and Field Resolution.
//!
//! The filesystem is the Schema registry: a Schema is a TOML file under
//! `.traces/schemas/` whose filename stem is the Schema name.
//! [`SchemaRegistry::load`] reads that directory and resolves the `extends`
//! DAG via [`resolve::resolve`], the crate's pure Field Resolution engine
//! (Kahn's topological sort, own-fields-override-parents, first-listed-wins,
//! `excludes`, bounded `$ref`).
//!
//! # Main Types
//!
//! - [`SchemaRegistry`]: reads and resolves every Schema under a directory
//!   (`registry.rs`, the module's only filesystem access).
//! - [`Schema`]: one Schema's effective Field Definitions plus its transitive
//!   `extends` ancestors for is-a matching, alongside
//!   [`model::FieldDefinition`]/[`model::FieldOptions`]/[`model::FieldType`]
//!   (`model.rs`, the resolved domain shapes).
//! - [`SchemaError`]/[`SchemaWarning`]: hard failures and recoverable degrades.
//!   See [`resolve::resolve`] for which conditions produce each.
//!
//! # Scope
//!
//! This module resolves Schemas from the filesystem only. It does not expose
//! them to minijinja templates, filter `file`-typed fields against a note
//! index, or run is-a class queries over notes; those integrations consume
//! [`SchemaRegistry`] and [`Schema::is_a`] from outside this module.

mod error;
mod model;
mod name;
mod raw;
mod registry;
mod resolve;

#[expect(
    unused_imports,
    reason = "declared by the schema-registry ticket for crate-wide reuse; \
              consumed by the schema-namespace ticket \
              (.scratch/metadata-schemas/issues/03-schema-minijinja-namespace.\
              md)"
)]
pub(crate) use error::{SchemaError, SchemaWarning};
#[expect(
    unused_imports,
    reason = "declared by the schema-registry ticket for crate-wide reuse; \
              consumed by the schema-namespace ticket \
              (.scratch/metadata-schemas/issues/03-schema-minijinja-namespace.\
              md)"
)]
pub(crate) use model::Schema;
#[expect(
    unused_imports,
    reason = "declared by the schema-registry ticket for crate-wide reuse; \
              consumed by the schema-namespace ticket \
              (.scratch/metadata-schemas/issues/03-schema-minijinja-namespace.\
              md)"
)]
pub(crate) use registry::SchemaRegistry;

/// The reserved Global Schema name (`global.toml`).
///
/// A reserved reference pool: never itself a Note's File Class, and its own
/// `required = true` fields degrade to `false` with a
/// [`SchemaWarning::StrayGlobalRequired`] during resolution.
pub(crate) const GLOBAL_SCHEMA_NAME: &str = "global";
