//! Schema registry and Field Resolution.
//!
//! The filesystem is the Schema registry: a Schema is a TOML file under
//! `.traces/schemas/` whose filename stem is the Schema name (spec User
//! Story 1). [`SchemaRegistry::load`] reads that directory and resolves the
//! `extends` DAG via [`resolve::resolve`], the crate's pure Field Resolution
//! engine (Kahn's topological sort, own-fields-override-parents,
//! first-listed-wins, `excludes`, bounded `$ref`).
//!
//! # Main Types
//!
//! - [`SchemaRegistry`]: Reads and resolves every Schema under a directory
//!   (`registry.rs`, the module's only filesystem access).
//! - [`Schema`]: One Schema's effective Field Definitions plus its transitive
//!   `extends` ancestors for is-a matching, alongside
//!   `model::FieldDefinition`/`model::FieldOptions`/`model::FieldType`
//!   (`model.rs`, the resolved domain shapes).
//! - [`SchemaError`] / [`SchemaWarning`]: Hard failures and recoverable
//!   degrades (see [`resolve::resolve`]'s doc comment for which is which).
//!
//! # Out of Scope
//!
//! The `schema` minijinja namespace, `file`-field `FileIndex` resolution, and
//! `query.from_class`/`tasks.from_class` consume this registry but are built
//! in later tickets
//! (`.scratch/metadata-schemas/issues/{03,04,05}-*.md`).

mod error;
mod model;
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
/// A never-required reference pool: forbidden as a Note's File Class value
/// (enforced by the class-query consumer, ticket 05) and its own `required =
/// true` fields degrade to `false` with a
/// [`SchemaWarning::StrayGlobalRequired`] during resolution.
pub(crate) const GLOBAL_SCHEMA_NAME: &str = "global";
