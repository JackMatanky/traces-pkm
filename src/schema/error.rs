//! Errors and warnings produced while reading and resolving the Schema
//! registry.

use std::{fmt, path::PathBuf};

use thiserror::Error;

use super::name::SchemaName;

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
        schema: SchemaName,
        #[source]
        source: Box<toml::de::Error>,
    },
    /// A Field Definition declared neither `type` nor `$ref`, so its type
    /// cannot be determined.
    #[error(
        "field {field:?} in Schema {schema:?} has neither `type` nor `$ref`"
    )]
    MissingFieldType {
        schema: SchemaName,
        field: String,
    },
    /// The `extends` DAG contains a cycle; Kahn's topological sort could not
    /// order every Schema.
    #[error("cycle detected among Schemas: {}", .schemas.join(", "))]
    Cycle {
        schemas: Vec<SchemaName>,
    },
    /// A `$ref` value was not shaped `#<schema>/<field>`.
    #[error(
        "malformed $ref {reference:?} in field {field:?} of Schema \
         {schema:?}: expected `#<schema>/<field>`"
    )]
    MalformedRef {
        schema: SchemaName,
        field: String,
        reference: String,
    },
    /// A `$ref` named a Schema that is neither the Global Schema nor a
    /// transitive `extends` ancestor of the referencing Schema.
    #[error(
        "$ref {reference:?} in field {field:?} of Schema {schema:?} is out of \
         bounds: not the Global Schema or a transitive `extends` ancestor"
    )]
    RefOutOfBounds {
        schema: SchemaName,
        field: String,
        reference: String,
    },
    /// A `$ref` named an in-bounds Schema, but that Schema has no such
    /// field.
    #[error(
        "$ref {reference:?} in field {field:?} of Schema {schema:?} does not \
         resolve"
    )]
    RefFieldNotFound {
        schema: SchemaName,
        field: String,
        reference: String,
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
        schema: SchemaName,
        target: SchemaName,
    },
    /// The reserved Global Schema declared `field` as `required = true`.
    /// Global Schema fields can never be required, so resolution treats it
    /// as `false`; a Schema `$ref`-ing this field may still mark it required
    /// locally.
    StrayGlobalRequired {
        field: String,
    },
}

impl fmt::Display for SchemaWarning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingExtendsTarget {
                schema,
                target,
            } => write!(
                f,
                "Schema {schema:?} extends unknown Schema {target:?}; its own \
                 fields still resolve"
            ),
            Self::StrayGlobalRequired {
                field,
            } => write!(
                f,
                "the reserved Global Schema declared field {field:?} as \
                 required; ignoring, since Global Schema fields can never be \
                 required"
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_extends_target_message_names_schema_and_target() {
        let warning = SchemaWarning::MissingExtendsTarget {
            schema: SchemaName::from("sci_fi"),
            target: SchemaName::from("ghost"),
        };

        assert_eq!(
            warning.to_string(),
            "Schema \"sci_fi\" extends unknown Schema \"ghost\"; its own \
             fields still resolve"
        );
    }

    #[test]
    fn stray_global_required_message_names_the_field() {
        let warning = SchemaWarning::StrayGlobalRequired {
            field: "priority".to_owned(),
        };

        assert_eq!(
            warning.to_string(),
            "the reserved Global Schema declared field \"priority\" as \
             required; ignoring, since Global Schema fields can never be \
             required"
        );
    }
}
