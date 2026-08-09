//! Errors and warnings produced while reading and resolving the Schema
//! registry.
//!
//! - [`SchemaError`]: a hard failure; registry loading and resolution stop.
//! - [`SchemaWarning`]: a recoverable defect; resolution continues after
//!   degrading to a documented fallback.

use std::{fmt, path::PathBuf};

use thiserror::Error;

use super::{name::SchemaName, raw::FieldRef};
use crate::field::FieldName;

/// Represents a hard failure while reading, parsing, or resolving the Schema
/// registry.
///
/// Contrast [`SchemaWarning`], which is emitted for a defect resolution
/// recovers from (a missing `extends` target, a stray `required = true` on the
/// reserved Global Schema).
///
/// A malformed `$ref` or a Field Definition declaring neither `type` nor `$ref`
/// both fail earlier, during TOML parsing (surfacing as [`Self::Parse`]): see
/// [`super::raw::FieldRef`] and [`super::raw::RawFieldDefError`].
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
    /// A Schema TOML file failed to parse: malformed TOML, an unknown key, a
    /// malformed `$ref`, or a Field Definition with neither `type` nor `$ref`.
    #[error("failed to parse Schema {schema}: {source}")]
    Parse {
        schema: SchemaName,
        #[source]
        source: Box<toml::de::Error>,
    },
    /// The `extends` DAG contains a cycle; Kahn's topological sort could not
    /// order every Schema.
    #[error("cycle detected among Schemas: {}", .schemas.join(", "))]
    Cycle {
        schemas: Vec<SchemaName>,
    },
    /// A `$ref` named a Schema that is neither the Global Schema nor a
    /// transitive `extends` ancestor of the referencing Schema.
    #[error(
        "$ref {reference:?} in field {field:?} of Schema {schema:?} is out of \
         bounds: not the Global Schema or a transitive `extends` ancestor"
    )]
    RefOutOfBounds {
        schema: SchemaName,
        field: FieldName,
        reference: Box<FieldRef>,
    },
    /// A `$ref` named an in-bounds Schema, but that Schema has no such field.
    #[error(
        "$ref {reference:?} in field {field:?} of Schema {schema:?} does not \
         resolve"
    )]
    RefFieldNotFound {
        schema: SchemaName,
        field: FieldName,
        reference: Box<FieldRef>,
    },
    /// Two of a Schema's effective fields share a
    /// [`FieldKey`](crate::field::FieldKey) canonical form: Note metadata could
    /// never disambiguate which one a value belongs to.
    #[error(
        "Schema {schema:?} has ambiguous fields {first:?} and {second:?}: \
         both canonicalize to the same metadata key"
    )]
    AmbiguousFieldName {
        schema: SchemaName,
        first: FieldName,
        second: Box<FieldName>,
    },
}

/// Represents a recoverable defect during Schema registry resolution:
/// resolution continues, degrading to the documented fallback.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum SchemaWarning {
    /// `schema`'s `extends` list named `target`, which has no corresponding
    /// Schema file. `schema` degrades to exact match: parent-provided fields
    /// are dropped, but its own fields still resolve.
    MissingExtendsTarget {
        schema: SchemaName,
        target: SchemaName,
    },
    /// The reserved Global Schema declared `field` as `required = true`. Global
    /// Schema fields can never be required, so resolution treats it as `false`.
    /// A Schema `$ref`-ing this field may still mark it required locally.
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
        fn cycle_formats_display_message_joining_every_schema() {
            let error = SchemaError::Cycle {
                schemas: vec![SchemaName::from("a"), SchemaName::from("b")],
            };

            assert_display(&error, "cycle detected among Schemas: a, b");
        }

        #[test]
        fn ref_out_of_bounds_formats_display_message() {
            let error = SchemaError::RefOutOfBounds {
                schema: SchemaName::from("movie"),
                field: FieldName::try_from("status").expect("valid name"),
                reference: Box::new(
                    FieldRef::try_from("#book/status").expect("valid ref"),
                ),
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
                field: FieldName::try_from("status").expect("valid name"),
                reference: Box::new(
                    FieldRef::try_from("#book/status").expect("valid ref"),
                ),
            };

            assert_display(
                &error,
                "$ref \"#book/status\" in field \"status\" of Schema \"book\" \
                 does not resolve",
            );
        }

        #[test]
        fn ambiguous_field_name_formats_display_message() {
            let error = SchemaError::AmbiguousFieldName {
                schema: SchemaName::from("book"),
                first: FieldName::try_from("status").expect("valid name"),
                second: Box::new(
                    FieldName::try_from("Status").expect("valid name"),
                ),
            };

            assert_display(
                &error,
                "Schema \"book\" has ambiguous fields \"status\" and \
                 \"Status\": both canonicalize to the same metadata key",
            );
        }

        #[test]
        fn stays_small() {
            // Regression guard (mem-assert-type-size): `UnresolvedRef` used
            // to carry 5 owned Strings (120 bytes) because
            // `ref_schema`/`ref_field` duplicated what `reference` already
            // shows verbatim. `RefOutOfBounds`/`RefFieldNotFound` box their
            // `FieldRef` payload for the same reason now that a `$ref` is a
            // validated `SchemaName` + `FieldName` pair rather than a single
            // `String`. `AmbiguousFieldName` boxes `second` for the same
            // reason: two owned `FieldName`s alongside `schema` would have
            // tied it for the largest variant again (measured: boxing one
            // field lands the enum at 64 bytes, not the payload-only 56,
            // since no variant's layout leaves the discriminant a free
            // niche once two variants tie for largest). Keep every variant's
            // payload small enough that `Result<_, SchemaError>` stays cheap
            // to move through the resolution call chain.
            assert!(
                std::mem::size_of::<SchemaError>() <= 64,
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
