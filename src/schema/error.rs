//! Schema loading, resolution, and field-construction error types.
//!
//! [`SchemaError`] is a hard failure that stops resolution. [`SchemaWarning`]
//! is a recoverable defect that resolution degrades past.

use std::{fmt, path::PathBuf};

use thiserror::Error;

use super::{fields::FieldAddress, name::SchemaName, raw::RawSchemaFieldType};
use crate::field::FieldName;

/// A hard failure that stops Schema loading or resolution.
///
/// Contrast [`SchemaWarning`], which is emitted for defects resolution
/// recovers from.
#[derive(Debug, Error)]
pub(crate) enum SchemaError {
    /// The registry directory exists but could not be read.
    #[error("failed to read Schema registry directory {directory}: {source}")]
    ReadDirectory {
        directory: PathBuf,
        #[source]
        source: std::io::Error,
    },
    /// A Schema TOML file could not be read.
    #[error("failed to read Schema file {path}: {source}")]
    ReadFile {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    /// A Schema TOML file failed to parse.
    #[error("failed to parse Schema {schema}: {source}")]
    Parse {
        schema: SchemaName,
        #[source]
        source: Box<toml::de::Error>,
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

impl From<SchemaFieldBuilderError> for SchemaError {
    fn from(error: SchemaFieldBuilderError) -> Self {
        Self::FieldBuilder(Box::new(error))
    }
}

/// Why [`super::fields::SchemaFieldBuilder::build`] failed.
///
/// [`Self::RefOutOfBounds`] and [`Self::RefFieldNotFound`] are always hard
/// failures. [`Self::UnknownAttributeKey`] and
/// [`Self::AttributeValueTypeMismatch`] are hard failures for `Direct` fields
/// and `$ref` fields with a local `type` override, but degrade to
/// [`SchemaWarning`] for bare `$ref` overrides.
#[derive(Debug, Error)]
pub(crate) enum SchemaFieldBuilderError {
    /// An attribute key is not valid for the field's resolved type.
    #[error("field {address} of type {kind} has no attribute {key:?}")]
    UnknownAttributeKey {
        address: FieldAddress,
        kind: RawSchemaFieldType,
        key: String,
    },
    /// An attribute key is valid for the field's type, but its value is
    /// wrongly shaped.
    #[error(
        "field {address} of type {kind}'s {key:?} attribute must be \
         {expected}, got {value}"
    )]
    AttributeValueTypeMismatch {
        address: FieldAddress,
        kind: RawSchemaFieldType,
        key: String,
        value: String,
        expected: &'static str,
    },
    /// A `$ref` names a Schema that is neither the Global Schema nor a
    /// transitive `extends` ancestor of the referencing Schema.
    #[error(
        "$ref {reference} in field {own} is out of bounds: not the Global \
         Schema or a transitive `extends` ancestor"
    )]
    RefOutOfBounds {
        own: Box<FieldAddress>,
        reference: Box<FieldAddress>,
    },
    /// A `$ref` names an in-bounds Schema that lacks the referenced field.
    #[error("$ref {reference} in field {own} does not resolve")]
    RefFieldNotFound {
        own: Box<FieldAddress>,
        reference: Box<FieldAddress>,
    },
}

/// A recoverable Schema resolution defect.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum SchemaWarning {
    /// An `extends` target has no corresponding Schema file.
    ///
    /// Resolution skips the missing parent; the Schema's own fields still
    /// resolve, and other valid parents still contribute.
    MissingExtendsTarget {
        schema: SchemaName,
        target: SchemaName,
    },
    /// `required = true` on the Global Schema, which is ignored.
    StrayGlobalRequired {
        field: String,
    },
    /// A bare `$ref` override declares an attribute key that does not belong
    /// to the resolved base field's type. The key is dropped.
    UnknownOverrideKey {
        address: FieldAddress,
        kind: RawSchemaFieldType,
        key: String,
    },
    /// A bare `$ref` override declares a valid attribute key with a
    /// wrongly-shaped value. The key is dropped, falling back to the base.
    OverrideValueTypeMismatch {
        address: FieldAddress,
        kind: RawSchemaFieldType,
        key: String,
        value: String,
        expected: &'static str,
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
            Self::UnknownOverrideKey {
                address,
                kind,
                key,
            } => write!(
                f,
                "$ref override {address} has no attribute {key:?} on its \
                 resolved type {kind}; ignoring the key"
            ),
            Self::OverrideValueTypeMismatch {
                address,
                kind,
                key,
                value,
                expected,
            } => write!(
                f,
                "$ref override {address}'s {key:?} attribute on its resolved \
                 type {kind} must be {expected}, got {value}; ignoring the key"
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
            let inner = SchemaFieldBuilderError::UnknownAttributeKey {
                address: FieldAddress::try_from("#book/status")
                    .expect("valid ref"),
                kind: RawSchemaFieldType::Date,
                key: "values".to_owned(),
            };

            let error = SchemaError::from(inner);

            assert_display(
                &error,
                "field #book/status of type date has no attribute \"values\"",
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
            // `SchemaFieldBuilderError`, whose own `AttributeValueTypeMismatch`
            // variant carries two owned `String`s and would otherwise be this
            // enum's largest member by far. Keep every variant's payload
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

        #[test]
        fn unknown_attribute_key_formats_display_message() {
            let error = SchemaFieldBuilderError::UnknownAttributeKey {
                address: FieldAddress::try_from("#book/published")
                    .expect("valid ref"),
                kind: RawSchemaFieldType::Date,
                key: "values".to_owned(),
            };

            assert_eq!(
                error.to_string(),
                "field #book/published of type date has no attribute \
                 \"values\""
            );
        }

        #[test]
        fn attribute_value_type_mismatch_formats_display_message() {
            let error = SchemaFieldBuilderError::AttributeValueTypeMismatch {
                address: FieldAddress::try_from("#book/rating")
                    .expect("valid ref"),
                kind: RawSchemaFieldType::Number,
                key: "min".to_owned(),
                value: "\"abc\"".to_owned(),
                expected: "a number",
            };

            assert_eq!(
                error.to_string(),
                "field #book/rating of type number's \"min\" attribute must \
                 be a number, got \"abc\""
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
        fn unknown_override_key_message_names_address_kind_and_key() {
            let warning = SchemaWarning::UnknownOverrideKey {
                address: FieldAddress::try_from("#sci_fi/cover")
                    .expect("valid ref"),
                kind: RawSchemaFieldType::Select,
                key: "folders".to_owned(),
            };

            assert_eq!(
                warning.to_string(),
                "$ref override #sci_fi/cover has no attribute \"folders\" on \
                 its resolved type select; ignoring the key"
            );
        }

        #[test]
        fn override_value_type_mismatch_message_names_every_field() {
            let warning = SchemaWarning::OverrideValueTypeMismatch {
                address: FieldAddress::try_from("#sci_fi/rating")
                    .expect("valid ref"),
                kind: RawSchemaFieldType::Number,
                key: "min".to_owned(),
                value: "\"abc\"".to_owned(),
                expected: "a number",
            };

            assert_eq!(
                warning.to_string(),
                "$ref override #sci_fi/rating's \"min\" attribute on its \
                 resolved type number must be a number, got \"abc\"; ignoring \
                 the key"
            );
        }
    }
}
