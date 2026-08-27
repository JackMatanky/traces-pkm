//! Field-construction error types:
//! [`SchemaFieldBuilder`](super::SchemaFieldBuilder)
//! and [`SchemaFieldParser`](super::parser::SchemaFieldParser)'s own failure
//! modes.
//!
//! Both types are only ever constructed and matched within [`super`]; the
//! only outside touch is [`SchemaFieldBuilderError`] wrapping into
//! [`SchemaError::FieldBuilder`](super::super::error::SchemaError::FieldBuilder)
//! and [`SchemaFieldParserError`] converting into
//! [`SchemaWarning`](super::super::error::SchemaWarning) for a degraded
//! bare `$ref` override.

use thiserror::Error;

use super::{FieldAddress, SchemaFieldTypeTag};
use crate::schema::error::SchemaWarning;

/// Why [`super::SchemaFieldBuilder::build`] failed.
///
/// Always hard failures:
/// - [`Self::RefOutOfBounds`]
/// - [`Self::RefFieldNotFound`]
///
/// Hard failures for `Direct` fields and `$ref` with a `type` override,
/// degraded to [`SchemaWarning`][super::super::error::SchemaWarning] for bare
/// `$ref` overrides:
/// - [`Self::Parser`]
#[derive(Debug, Error)]
pub(crate) enum SchemaFieldBuilderError {
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
    /// One or more per-key validation failures from parsing a field type's
    /// `options`. Never empty: only constructed from a non-empty
    /// [`SchemaFieldParser::finish`](super::parser::SchemaFieldParser::finish)
    /// result.
    #[error(
        "{} field option error(s): {}",
        .0.len(),
        .0.iter().map(ToString::to_string).collect::<Vec<_>>().join("; ")
    )]
    Parser(Vec<SchemaFieldParserError>),
}

/// One per-key validation failure from parsing a field type's `options`.
///
/// Converts into:
/// - [`SchemaFieldBuilderError::Parser`] (hard failure) for `Direct` fields and
///   `$ref` with a `type` override.
/// - [`SchemaWarning`][super::super::error::SchemaWarning] (degraded) for bare
///   `$ref` overrides.
#[derive(Debug, Error)]
pub(crate) enum SchemaFieldParserError {
    /// An attribute key was not claimed by any typed extractor.
    #[error("field {address} of type {kind} has no attribute {key:?}")]
    UnknownKey {
        address: FieldAddress,
        kind: SchemaFieldTypeTag,
        key: String,
    },
    /// An attribute key was claimed, but its value is wrongly shaped.
    #[error(
        "field {address} of type {kind}'s {key:?} attribute must be \
         {expected}, got {value}"
    )]
    TypeMismatch {
        address: FieldAddress,
        kind: SchemaFieldTypeTag,
        key: String,
        value: String,
        expected: &'static str,
    },
    /// A values file path has an unsupported file extension.
    #[error(
        "field {address} values file {path:?} has unsupported extension (must \
         be .toml or .json)"
    )]
    BadValueFileExtension {
        address: FieldAddress,
        path: String,
    },
    /// A values file failed to load or parse.
    #[error("field {address} failed to load values file {path:?}: {error}")]
    ValueFileLoad {
        address: FieldAddress,
        path: String,
        error: String,
    },
    /// A values file was missing the required top-level `entries` list.
    #[error(
        "field {address} values file {path:?} missing top-level 'entries' list"
    )]
    ValueFileMissingEntries {
        address: FieldAddress,
        path: String,
    },
    /// A selector key was specified on bare string entries.
    #[error(
        "field {address} configures selector {selector:?} but values file \
         contains bare string entries"
    )]
    SelectorOnBareEntries {
        address: FieldAddress,
        selector: &'static str,
    },
    /// An entry object was missing a key named by a selector.
    #[error(
        "field {address} configures selector {selector:?} = {key:?}, but an \
         entry is missing this key"
    )]
    SelectorMissingKey {
        address: FieldAddress,
        selector: &'static str,
        key: String,
    },
}

impl SchemaFieldParserError {
    /// Address of the field that produced this error.
    #[must_use]
    pub(crate) fn address(&self) -> &FieldAddress {
        match self {
            Self::UnknownKey {
                address,
                ..
            }
            | Self::TypeMismatch {
                address,
                ..
            }
            | Self::BadValueFileExtension {
                address,
                ..
            }
            | Self::ValueFileLoad {
                address,
                ..
            }
            | Self::ValueFileMissingEntries {
                address,
                ..
            }
            | Self::SelectorOnBareEntries {
                address,
                ..
            }
            | Self::SelectorMissingKey {
                address,
                ..
            } => address,
        }
    }
}

impl From<SchemaFieldParserError> for SchemaWarning {
    fn from(error: SchemaFieldParserError) -> Self {
        match error {
            SchemaFieldParserError::UnknownKey {
                address,
                kind,
                key,
            } => Self::UnknownOverrideKey {
                address,
                kind,
                key,
            },
            SchemaFieldParserError::TypeMismatch {
                address,
                kind,
                key,
                value,
                expected,
            } => Self::OverrideValueTypeMismatch {
                address,
                kind,
                key,
                value,
                expected,
            },
            err => Self::SelectValuesOverrideDegraded {
                address: err.address().clone(),
                error: err.to_string(),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    fn assert_display(error: &impl std::fmt::Display, expected: &str) {
        assert_eq!(error.to_string(), expected, "unexpected display output");
    }

    mod schema_field_parser_error {
        use super::{super::*, assert_display};

        #[test]
        fn unknown_key_formats_display_message() {
            let error = SchemaFieldParserError::UnknownKey {
                address: FieldAddress::try_from("#book/status")
                    .expect("valid ref"),
                kind: SchemaFieldTypeTag::Date,
                key: "values".to_owned(),
            };

            assert_display(
                &error,
                "field #book/status of type date has no attribute \"values\"",
            );
        }

        #[test]
        fn type_mismatch_formats_display_message() {
            let error = SchemaFieldParserError::TypeMismatch {
                address: FieldAddress::try_from("#book/rating")
                    .expect("valid ref"),
                kind: SchemaFieldTypeTag::Number,
                key: "min".to_owned(),
                value: "\"abc\"".to_owned(),
                expected: "a number",
            };

            assert_display(
                &error,
                "field #book/rating of type number's \"min\" attribute must \
                 be a number, got \"abc\"",
            );
        }
    }

    mod conversions {
        use super::super::*;

        #[test]
        fn parser_unknown_key_converts_to_unknown_override_key_warning() {
            let error = SchemaFieldParserError::UnknownKey {
                address: FieldAddress::try_from("#sci_fi/cover")
                    .expect("valid ref"),
                kind: SchemaFieldTypeTag::Date,
                key: "values".to_owned(),
            };

            let warning = SchemaWarning::from(error);

            assert!(matches!(
                warning,
                SchemaWarning::UnknownOverrideKey {
                    ref key,
                    ..
                } if key == "values"
            ));
        }

        #[test]
        fn parser_type_mismatch_converts_to_override_value_type_mismatch_warning()
         {
            let error = SchemaFieldParserError::TypeMismatch {
                address: FieldAddress::try_from("#sci_fi/rating")
                    .expect("valid ref"),
                kind: SchemaFieldTypeTag::Number,
                key: "min".to_owned(),
                value: "\"abc\"".to_owned(),
                expected: "a number",
            };

            let warning = SchemaWarning::from(error);

            assert!(matches!(
                warning,
                SchemaWarning::OverrideValueTypeMismatch {
                    ref key,
                    expected,
                    ..
                } if key == "min" && expected == "a number"
            ));
        }

        #[test]
        fn select_values_error_converts_to_select_values_override_warning() {
            let error = SchemaFieldParserError::SelectorMissingKey {
                address: FieldAddress::try_from("#sci_fi/status")
                    .expect("valid ref"),
                selector: "label",
                key: "label".to_owned(),
            };

            let warning = SchemaWarning::from(error);

            assert!(matches!(
                warning,
                SchemaWarning::SelectValuesOverrideDegraded {
                    error: ref message,
                    ..
                } if message.contains("selector \"label\"")
            ));
        }
    }

    mod schema_field_builder_error {
        use super::super::*;

        #[test]
        fn parser_formats_display_message_joining_every_error() {
            let error = SchemaFieldBuilderError::Parser(vec![
                SchemaFieldParserError::UnknownKey {
                    address: FieldAddress::try_from("#book/published")
                        .expect("valid ref"),
                    kind: SchemaFieldTypeTag::Date,
                    key: "values".to_owned(),
                },
            ]);

            assert_eq!(
                error.to_string(),
                "1 field option error(s): field #book/published of type date \
                 has no attribute \"values\""
            );
        }

        #[test]
        fn parser_joins_multiple_simultaneous_errors() {
            let error = SchemaFieldBuilderError::Parser(vec![
                SchemaFieldParserError::UnknownKey {
                    address: FieldAddress::try_from("#book/rating")
                        .expect("valid ref"),
                    kind: SchemaFieldTypeTag::Number,
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
            ]);

            assert_eq!(
                error.to_string(),
                "2 field option error(s): field #book/rating of type number \
                 has no attribute \"values\"; field #book/rating of type \
                 number's \"min\" attribute must be a number, got \"abc\""
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
}
