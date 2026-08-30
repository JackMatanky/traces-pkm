//! Schema loading, resolution, and field-construction error types.
//!
//! [`SchemaError`] is a hard failure that stops resolution. [`SchemaWarning`]
//! is a recoverable defect that resolution degrades past.

use std::{fmt, path::PathBuf};

use thiserror::Error;

use super::{SchemaName, fields::SchemaFieldBuilderError};
use crate::field::FieldName;

/// A hard failure that stops Schema loading or resolution.
///
/// Defects that resolution can recover from are emitted as [`SchemaWarning`]
/// instead.
///
/// Deliberately `pub(crate)`, not `pub`: [`TemplateError::SchemaLoad`] wraps
/// this transparently for its `Display`/`Error` chain without exposing the
/// concrete type, via a scoped `#[expect(private_interfaces)]`; do not "fix"
/// that by making this `pub`, which would cascade the same requirement onto
/// every field type below (`SchemaName`, `FieldName`,
/// `SchemaFieldBuilderError`, …), all otherwise intentionally crate-internal.
///
/// [`TemplateError::SchemaLoad`]: crate::template::TemplateError::SchemaLoad
#[derive(Debug, Error)]
pub(crate) enum SchemaError {
    /// The registry directory exists but could not be read.
    #[error("failed to read Schema registry directory {directory}: {source}")]
    ReadDirectory {
        directory: PathBuf,
        #[source]
        source: std::io::Error,
    },
    /// A Schema TOML file could not be read or parsed.
    #[error("failed to load Schema file {path}: {source}")]
    File {
        path: PathBuf,
        #[source]
        source: SchemaFileError,
    },
    /// The `extends` DAG contains a cycle.
    #[error("cycle detected among Schemas: {}", .schemas.join(", "))]
    Cycle {
        schemas: Vec<SchemaName>,
    },
    /// Two effective fields share a [`FieldKey`](crate::field::FieldKey)
    /// canonical form.
    #[error(
        "Schema {schema:?} has ambiguous fields {first:?} and {second:?}: \
         both canonicalize to the same metadata key"
    )]
    AmbiguousFieldName {
        schema: SchemaName,
        first: FieldName,
        second: Box<FieldName>,
    },
    /// A field failed to build: unrecognized attribute, wrongly-shaped value,
    /// or `$ref` resolution failure.
    #[error(transparent)]
    FieldBuilder(Box<SchemaFieldBuilderError>),
}

/// Why a Schema TOML file could not be loaded, once its path is known.
///
/// Deliberately not shared with `select`'s file loader
/// (`SchemaSelectFieldFileError`, in `fields::error`, a private submodule
/// this doc comment can't link to):
/// Schema files are always TOML, values files can be TOML or JSON, so the two
/// don't have the same shape to share, even though both follow the same
/// "read, then parse" pattern.
#[derive(Debug, Error)]
pub(crate) enum SchemaFileError {
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Parse(#[from] Box<toml::de::Error>),
}

impl From<SchemaFieldBuilderError> for SchemaError {
    fn from(error: SchemaFieldBuilderError) -> Self {
        Self::FieldBuilder(Box::new(error))
    }
}

/// A recoverable Schema resolution defect.
///
/// Resolution skips the offending key or parent and continues. Every warning
/// is surfaced to the caller as diagnostic context.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum SchemaWarning {
    /// An `extends` target has no corresponding Schema file.
    ///
    /// Resolution skips the missing parent. The Schema's own fields still
    /// resolve, and other valid parents still contribute their fields.
    MissingExtendsTarget {
        schema: SchemaName,
        target: SchemaName,
    },
    /// The same `extends` target is declared more than once.
    ///
    /// Duplicates are ignored; only the first occurrence contributes fields.
    DuplicateExtendsTarget {
        schema: SchemaName,
        target: SchemaName,
    },
    /// A declared `extends` parent exists but failed to build its own
    /// fields. This Schema resolves without that parent's contribution;
    /// other valid parents still contribute.
    ParentFailedToResolve {
        schema: SchemaName,
        parent: SchemaName,
    },
    /// `required = true` on the Global Schema, which is always ignored.
    ///
    /// Global Schema fields can never be required; a referencing Schema may
    /// mark the field required locally instead.
    StrayGlobalRequired {
        field: String,
    },
    /// A bare `$ref` override declared an invalid attribute (unknown key,
    /// wrongly-shaped value, or invalid `number`/`select` configuration).
    /// The key (or whole `values` override) is dropped and the base field's
    /// value is used as-is.
    ///
    /// `message` is exactly what a hard failure for this same attribute
    /// would report — `SchemaFieldParserError`'s `Display` (a private
    /// `fields::error` type this doc comment can't link to) — rather than a
    /// second, hand-mirrored wording per
    /// failure kind: five variants used to restate
    /// `SchemaFieldParserError`'s five variants field-for-field just to
    /// change "field ... has no attribute" into "$ref override ... has no
    /// attribute"; the wrapped message already carries the address, kind,
    /// and key, so this variant only adds what a degraded override actually
    /// needs on top: that it *was* degraded.
    DegradedOverride {
        message: String,
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
            Self::DuplicateExtendsTarget {
                schema,
                target,
            } => write!(
                f,
                "Schema {schema:?} declares extends target {target:?} more \
                 than once; duplicates are ignored"
            ),
            Self::ParentFailedToResolve {
                schema,
                parent,
            } => write!(
                f,
                "Schema {schema:?} extends {parent:?}, which failed to \
                 resolve; its own fields still resolve, without {parent:?}'s \
                 fields"
            ),
            Self::StrayGlobalRequired {
                field,
            } => write!(
                f,
                "the reserved Global Schema declared field {field:?} as \
                 required; ignoring, since Global Schema fields can never be \
                 required"
            ),
            Self::DegradedOverride {
                message,
            } => write!(
                f,
                "$ref override degraded: {message}; using the base value \
                 instead"
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
        use crate::schema::fields::{
            FieldAddress, SchemaFieldParserError, SchemaFieldTypeTag,
        };

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
        fn file_formats_display_message_for_an_io_failure() {
            let error = SchemaError::File {
                path: PathBuf::from("/schemas/book.toml"),
                source: SchemaFileError::Io(std::io::Error::other("denied")),
            };

            assert_display(
                &error,
                "failed to load Schema file /schemas/book.toml: denied",
            );
        }

        #[test]
        fn file_formats_display_message_for_a_parse_failure() {
            let source = "not valid toml".parse::<toml::Value>().unwrap_err();
            let error = SchemaError::File {
                path: PathBuf::from("/schemas/book.toml"),
                source: SchemaFileError::Parse(Box::new(source)),
            };

            let message = error.to_string();
            assert!(
                message.starts_with(
                    "failed to load Schema file /schemas/book.toml: "
                ),
                "expected message to open with the file context, got: \
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
        fn field_builder_delegates_display_to_the_wrapped_error() {
            let inner = SchemaFieldBuilderError::Parser(vec![
                SchemaFieldParserError::UnknownKey {
                    address: FieldAddress::try_from("#book/status")
                        .expect("valid ref"),
                    kind: SchemaFieldTypeTag::Date,
                    key: "values".to_owned(),
                },
            ]);

            let error = SchemaError::from(inner);

            let msg = error.to_string();
            assert!(
                msg.contains("1 field option error(s)"),
                "expected wrapped error display, got: {msg}"
            );
        }

        #[test]
        fn stays_small() {
            // Regression guard (mem-assert-type-size): `UnresolvedRef` used
            // to carry 5 owned Strings (120 bytes) because `ref_schema`/
            // `ref_field` duplicated what `reference` already shows
            // verbatim. `RefOutOfBounds`/`RefFieldNotFound` box both their
            // own referencing address (`own`) and the `$ref` target
            // (`reference`) as `Box<FieldAddress>` now that a `$ref` is a
            // validated `SchemaName` + `FieldName` pair rather than a single
            // `String`. `AmbiguousFieldName` boxes `second` for the same
            // reason: two owned `FieldName`s alongside `schema` would tie it
            // for the largest variant. `FieldBuilder` boxes the whole wrapped
            // `SchemaFieldBuilderError`: its own largest variant is
            // `Parser(Vec<SchemaFieldParserError>)`, a fixed-size `Vec`
            // handle regardless of what's inside it, so boxing here mainly
            // decouples `SchemaError`'s size from that enum's future growth.
            // Keep every variant's payload
            // small enough that `Result<_, SchemaError>` stays cheap to move
            // through the resolution call chain.
            assert!(
                std::mem::size_of::<SchemaError>() <= 64,
                "SchemaError grew to {} bytes; box or trim the offending \
                 variant",
                std::mem::size_of::<SchemaError>()
            );
        }
    }

    mod schema_field_builder_error {
        use pretty_assertions::assert_eq;

        use super::super::*;
        use crate::schema::fields::{
            FieldAddress, SchemaFieldParserError, SchemaFieldTypeTag,
        };

        #[test]
        fn parser_wraps_multiple_errors() {
            let errors = vec![
                SchemaFieldParserError::UnknownKey {
                    address: FieldAddress::try_from("#book/status")
                        .expect("valid ref"),
                    kind: SchemaFieldTypeTag::Date,
                    key: "values".to_owned(),
                },
                SchemaFieldParserError::TypeMismatch {
                    address: FieldAddress::try_from("#book/rating")
                        .expect("valid ref"),
                    kind: SchemaFieldTypeTag::Number,
                    key: "min".to_owned(),
                    value: "\"abc\"".to_owned(),
                    expected: "a number",
                },
            ];
            let error = SchemaFieldBuilderError::Parser(errors);

            let msg = error.to_string();
            assert!(
                msg.contains("2 field option error(s)"),
                "expected count in display, got: {msg}"
            );
        }

        #[test]
        fn ref_out_of_bounds_formats_display_message() {
            let error = SchemaFieldBuilderError::RefOutOfBounds {
                own: Box::new(
                    FieldAddress::try_from("#movie/status").expect("valid ref"),
                ),
                reference: Box::new(
                    FieldAddress::try_from("#book/status").expect("valid ref"),
                ),
            };

            assert_eq!(
                error.to_string(),
                "$ref #book/status in field #movie/status is out of bounds: \
                 not the Global Schema or a transitive `extends` ancestor"
            );
        }

        #[test]
        fn ref_field_not_found_formats_display_message() {
            let error = SchemaFieldBuilderError::RefFieldNotFound {
                own: Box::new(
                    FieldAddress::try_from("#book/status").expect("valid ref"),
                ),
                reference: Box::new(
                    FieldAddress::try_from("#book/status").expect("valid ref"),
                ),
            };

            assert_eq!(
                error.to_string(),
                "$ref #book/status in field #book/status does not resolve"
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
        fn duplicate_extends_target_message_names_schema_and_target() {
            let warning = SchemaWarning::DuplicateExtendsTarget {
                schema: SchemaName::from("sci_fi"),
                target: SchemaName::from("book"),
            };

            assert_eq!(
                warning.to_string(),
                "Schema \"sci_fi\" declares extends target \"book\" more than \
                 once; duplicates are ignored"
            );
        }

        #[test]
        fn parent_failed_to_resolve_message_names_schema_and_parent() {
            let warning = SchemaWarning::ParentFailedToResolve {
                schema: SchemaName::from("sci_fi"),
                parent: SchemaName::from("book"),
            };

            assert_eq!(
                warning.to_string(),
                "Schema \"sci_fi\" extends \"book\", which failed to resolve; \
                 its own fields still resolve, without \"book\"'s fields"
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

        #[test]
        fn degraded_override_message_wraps_the_hard_failure_text() {
            let warning = SchemaWarning::DegradedOverride {
                message: "field #sci_fi/cover has no attribute \"folders\""
                    .to_owned(),
            };

            assert_eq!(
                warning.to_string(),
                "$ref override degraded: field #sci_fi/cover has no attribute \
                 \"folders\"; using the base value instead"
            );
        }
    }
}
