//! Errors and warnings produced while reading and resolving the Schema
//! registry.

use std::path::PathBuf;

use thiserror::Error;

/// Represents a hard failure while reading, parsing, or resolving the Schema
/// registry.
///
/// Contrast [`SchemaWarning`], which is emitted for a defect resolution
/// recovers from (a missing `extends` target, a stray `required = true` on
/// the reserved Global Schema).
#[derive(Debug, Error)]
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "declared by the schema-registry ticket; consumed by the \
                  schema-namespace ticket \
                  (.scratch/metadata-schemas/issues/\
                  03-schema-minijinja-namespace.md)"
    )
)]
pub(crate) enum SchemaError {
    /// The Schema registry directory exists but could not be read.
    #[error("failed to read Schema registry directory {directory}: {source}")]
    ReadDirectory {
        directory: PathBuf,
        #[source]
        source: std::io::Error,
    },
    /// A `.toml` file under the registry directory could not be read.
    #[error("failed to read Schema file {path}: {source}")]
    ReadFile {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    /// A Schema TOML file failed to parse: malformed TOML or an unknown key.
    #[error("failed to parse Schema {schema}: {source}")]
    Parse {
        schema: String,
        #[source]
        source: Box<toml::de::Error>,
    },
    /// A Field Definition declared neither `type` nor `$ref`, so its type
    /// cannot be determined.
    #[error(
        "field {field:?} in Schema {schema:?} has neither `type` nor `$ref`"
    )]
    MissingFieldType {
        schema: String,
        field: String,
    },
    /// The `extends` DAG contains a cycle; Kahn's topological sort could not
    /// order every Schema.
    #[error("cycle detected among Schemas: {}", .schemas.join(", "))]
    Cycle {
        schemas: Vec<String>,
    },
    /// A `$ref` value was not shaped `#<schema>/<field>`.
    #[error(
        "malformed $ref {reference:?} in field {field:?} of Schema \
         {schema:?}: expected `#<schema>/<field>`"
    )]
    MalformedRef {
        schema: String,
        field: String,
        reference: String,
    },
    /// A `$ref` named a Schema or field that does not exist, or a Schema not
    /// yet resolved (not an ancestor of the referencing Schema, nor Global).
    #[error(
        "$ref {reference:?} in field {field:?} of Schema {schema:?} does not \
         resolve: no field {ref_field:?} in Schema {ref_schema:?}"
    )]
    UnresolvedRef {
        schema: String,
        field: String,
        reference: String,
        ref_schema: String,
        ref_field: String,
    },
}

/// Represents a recoverable defect encountered while resolving the Schema
/// registry: resolution continues, degrading to the documented fallback.
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "declared by the schema-registry ticket; consumed by the \
                  schema-namespace ticket \
                  (.scratch/metadata-schemas/issues/\
                  03-schema-minijinja-namespace.md)"
    )
)]
pub(crate) enum SchemaWarning {
    /// `schema`'s `extends` list named `target`, which has no corresponding
    /// Schema file. `schema` degrades to exact match: parent-provided fields
    /// are dropped, but its own fields still resolve.
    MissingExtendsTarget {
        schema: String,
        target: String,
    },
    /// The reserved Global Schema declared `field` as `required = true`.
    /// Global Schema fields can never be required, so resolution treats it
    /// as `false`; a Schema `$ref`-ing this field may still mark it required
    /// locally.
    StrayGlobalRequired {
        field: String,
    },
}
