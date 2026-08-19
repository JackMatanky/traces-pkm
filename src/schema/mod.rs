//! Typed frontmatter Schema resolution.
//!
//! Reads `.traces/schemas/*.toml` files, linearizes the `extends` DAG via
//! Kahn's topological sort ([`graph::SchemaGraph`]), and resolves each Schema's
//! effective [`fields::SchemaFieldDef`]s by merging parent fields, applying
//! `excludes`, and building own fields with `$ref` resolution bounded to the
//! Global Schema or transitive `extends` ancestors.
//!
//! The domain's public entry point is [`SchemaService`], which wraps loading
//! ([`SchemaService::new`]) and hierarchy queries ([`SchemaService::get`],
//! [`SchemaService::children`], [`SchemaService::descendants`],
//! [`SchemaService::matches`]) behind a single facade.
//!
//! Template exposure, `file` field filtering, and class queries consume
//! [`SchemaService`] and [`Schema`] from outside this module.

mod error;
mod fields;
mod graph;
mod model;
mod name;
mod raw;
mod resolver;
mod service;

pub(crate) use error::SchemaError;
pub(crate) use fields::{SchemaFileFieldRef, SchemaSelectFieldEntry};
pub use model::Schema;
pub(crate) use name::{SchemaName, SchemaNameRef};
pub(crate) use raw::{
    RawSchema, RawSchemaFieldDef, RawSchemaFieldSource, RawSchemaFieldType,
};
pub use service::SchemaService;

/// The reserved Global Schema file stem: a flat `$ref`-able reference pool.
///
/// Never a Note's File Class, never inherits via `extends`, and
/// `required = true` fields degrade to `false`.
pub(crate) const GLOBAL_SCHEMA_NAME: &str = "global";
