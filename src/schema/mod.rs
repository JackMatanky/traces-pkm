//! Resolve typed frontmatter Schemas into effective field definitions.
//!
//! Schema TOML files live under `.traces/schemas/`; each filename stem becomes
//! the Schema name. [`SchemaService::resolve`] reads that directory
//! (`service::read_raw_schemas`, an impure edge) and hands the parsed TOML to
//! a pure resolution pass: linearizing the `extends` DAG with Kahn's
//! topological sort ([`graph::SchemaGraph`]), resolving parents before
//! children, merging parent fields in declaration order, applying `excludes`,
//! and letting own fields override inherited fields. `$ref` targets are
//! bounded to the Global Schema or a transitive `extends` ancestor
//! ([`fields::SchemaFieldBuilder`]).
//!
//! # Main Types
//!
//! - [`SchemaService`] is the domain's facade: loads Schemas
//!   ([`SchemaService::resolve`]) and answers hierarchy/class queries
//!   (`get`/`children`/`descendants`/`matches`/`expand_classes`) over the
//!   resulting [`SchemaRegistry`].
//! - [`Schema`] stores effective [`fields::SchemaFieldDef`]s, transitive
//!   `extends` ancestors, direct extenders (`children`), and transitive
//!   extenders (`descendants`).
//! - [`fields::SchemaFieldType`] and [`SchemaSelectFieldEntry`] describe
//!   resolved type-specific field shapes.
//! - [`error::SchemaError`] and [`error::SchemaWarning`] describe hard failures
//!   and recovered defects.
//!
//! # Scope
//!
//! Resolves Schemas from the filesystem only. Template exposure, `file` field
//! filtering against the note index, and note class queries consume
//! [`SchemaService`] and [`Schema::is_a`] from outside this module.

mod error;
mod fields;
mod graph;
mod model;
mod name;
mod raw;
mod service;

pub(crate) use error::SchemaError;
pub(crate) use fields::{SchemaFileFieldDefRef, SchemaSelectFieldEntry};
pub use model::Schema;
pub use service::SchemaService;
pub(crate) use service::{SchemaRegistry, resolve_sources};

/// Name the reserved Global Schema file stem.
///
/// A flat `$ref`-able reference pool: never a Note's File Class, never inherits
/// via its own `extends`, and `required = true` fields degrade to `false` with
/// a [`error::SchemaWarning::StrayGlobalRequired`] during resolution.
pub(crate) const GLOBAL_SCHEMA_NAME: &str = "global";
