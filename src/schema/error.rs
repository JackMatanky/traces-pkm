//! Errors and warnings produced while reading and resolving the Schema
//! registry.
//!
//! - [`SchemaError`]: a hard failure; registry loading and resolution stop.
//! - [`SchemaWarning`]: a recoverable defect; resolution continues after
//!   degrading to a documented fallback.

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
    /// A `$ref` named an in-bounds Schema, but that Schema has no such field.
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
    mod schema_error {
        use std::path::PathBuf;

        use pretty_assertions::assert_eq;

        use super::super::*;

        fn assert_display(error: &SchemaError, expected: &str) {
            assert_eq!(
                error.to_string(),
                expected,
                "unexpected SchemaError display"
            );
        }

        #[test]
        fn read_directory_formats_display_message() {
            let error = SchemaError::ReadDirectory {
                directory: PathBuf::from("/schemas"),
                source: std::io::Error::other("denied"),
            };

            assert_display(
                &error,
                "failed to read Schema registry directory /schemas: denied",
            );
        }

        #[test]
        fn read_file_formats_display_message() {
            let error = SchemaError::ReadFile {
                path: PathBuf::from("/schemas/book.toml"),
                source: std::io::Error::other("denied"),
            };

            assert_display(
                &error,
                "failed to read Schema file /schemas/book.toml: denied",
            );
        }

        #[test]
        fn parse_formats_display_message_wrapping_the_toml_source() {
            let source = "not valid toml".parse::<toml::Value>().unwrap_err();
            let error = SchemaError::Parse {
                schema: SchemaName::from("book"),
                source: Box::new(source),
            };

            let message = error.to_string();
            assert!(
                message.starts_with("failed to parse Schema book: "),
                "expected message to open with the Schema context, got: \
                 {message:?}"
            );
        }

        #[test]
        fn missing_field_type_formats_display_message() {
            let error = SchemaError::MissingFieldType {
                schema: SchemaName::from("book"),
                field: "status".to_owned(),
            };

            assert_display(
                &error,
                "field \"status\" in Schema \"book\" has neither `type` nor \
                 `$ref`",
            );
        }

        #[test]
        fn cycle_formats_display_message_joining_every_schema() {
            let error = SchemaError::Cycle {
                schemas: vec![SchemaName::from("a"), SchemaName::from("b")],
            };

            assert_display(&error, "cycle detected among Schemas: a, b");
        }

        #[test]
        fn malformed_ref_formats_display_message() {
            let error = SchemaError::MalformedRef {
                schema: SchemaName::from("book"),
                field: "status".to_owned(),
                reference: "book/status".to_owned(),
            };

            assert_display(
                &error,
                "malformed $ref \"book/status\" in field \"status\" of Schema \
                 \"book\": expected `#<schema>/<field>`",
            );
        }

        #[test]
        fn ref_out_of_bounds_formats_display_message() {
            let error = SchemaError::RefOutOfBounds {
                schema: SchemaName::from("movie"),
                field: "status".to_owned(),
                reference: "#book/status".to_owned(),
            };

            assert_display(
                &error,
                "$ref \"#book/status\" in field \"status\" of Schema \
                 \"movie\" is out of bounds: not the Global Schema or a \
                 transitive `extends` ancestor",
            );
        }

        #[test]
        fn ref_field_not_found_formats_display_message() {
            let error = SchemaError::RefFieldNotFound {
                schema: SchemaName::from("book"),
                field: "status".to_owned(),
                reference: "#book/status".to_owned(),
            };

            assert_display(
                &error,
                "$ref \"#book/status\" in field \"status\" of Schema \"book\" \
                 does not resolve",
            );
        }

        #[test]
        fn stays_small() {
            // Regression guard (mem-assert-type-size): `UnresolvedRef` used
            // to carry 5 owned Strings (120 bytes) because
            // `ref_schema`/`ref_field` duplicated what `reference` already
            // shows verbatim. Keep every variant's payload small enough
            // that `Result<_, SchemaError>` stays cheap to move through the
            // resolution call chain.
            assert!(
                std::mem::size_of::<SchemaError>() <= 80,
                "SchemaError grew to {} bytes; box or trim the offending \
                 variant",
                std::mem::size_of::<SchemaError>()
            );
        }
    }

    mod schema_warning {
        use pretty_assertions::assert_eq;

        use super::super::*;

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
}
