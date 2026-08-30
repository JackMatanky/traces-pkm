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
use crate::{path::PathError, schema::error::SchemaWarning};

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
    /// A numeric attribute was present but cannot form a valid field invariant.
    #[error(
        "field {address} of type number's {key:?} attribute must be \
         {expected}, got {value}"
    )]
    NumberConstraint {
        address: FieldAddress,
        key: String,
        value: String,
        expected: &'static str,
    },
    /// A number field declared `min > max`.
    #[error(
        "field {address} of type number's \"min\" attribute must be <= \
         \"max\", got min {min} and max {max}"
    )]
    NumberRange {
        address: FieldAddress,
        min: String,
        max: String,
    },
    /// `select`/`multi` values configuration is invalid.
    #[error("field {address} {source}")]
    SelectValues {
        address: FieldAddress,
        #[source]
        source: SelectValuesError,
    },
}

/// A validation failure inside a `select`/`multi` values definition.
#[derive(Debug, Error)]
pub(crate) enum SelectValuesError {
    /// A values file failed confinement, extension, I/O, parsing, or shape
    /// validation.
    #[error("values file {path:?} is invalid: {source}")]
    ValuesFile {
        path: String,
        #[source]
        source: SelectValuesFileError,
    },
    /// A file subtable omitted its required `path` attribute.
    #[error("values file subtable is missing required attribute \"path\"")]
    MissingPath,
    /// A file subtable's `path` attribute was not a string.
    #[error(
        "values file subtable's \"path\" attribute must be a string, got \
         {value}"
    )]
    PathNotString {
        value: String,
    },
    /// A file subtable selector was not a string.
    #[error(
        "values file subtable's selector {selector:?} must be a string, got \
         {value}"
    )]
    SelectorNotString {
        selector: &'static str,
        value: String,
    },
    /// A selector key was specified on bare string entries.
    #[error(
        "configures selector {selector:?} but entries contain bare string \
         values"
    )]
    SelectorOnBareEntries {
        selector: &'static str,
    },
    /// An entry object was missing a key named by a selector.
    #[error(
        "configures selector {selector:?} = {key:?}, but an entry is missing \
         this key"
    )]
    SelectorMissingKey {
        selector: &'static str,
        key: String,
    },
    /// A selected entry key was not a string.
    #[error(
        "configures selector {selector:?} = {key:?}, but that entry value \
         must be a string, got {value}"
    )]
    SelectedValueNotString {
        selector: &'static str,
        key: String,
        value: String,
    },
    /// A selected order key was not numeric.
    #[error(
        "configures selector \"order\" = {key:?}, but that entry value must \
         be a number, got {value}"
    )]
    OrderNotNumber {
        key: String,
        value: String,
    },
    /// An entry includes a passthrough key reserved by rendered select output.
    #[error(
        "entry key {key:?} is reserved for rendered select output; choose \
         another source key or selector"
    )]
    ReservedOutputKey {
        key: String,
    },
}

/// Errors encountered while loading or parsing an external values file.
#[derive(Debug, Error)]
pub(crate) enum SelectValuesFileError {
    /// The values file path escaped the schema directory or contained unsafe
    /// components.
    #[error(transparent)]
    Confinement(#[from] PathError),

    /// An I/O error occurred while reading the values file.
    #[error("failed to read values file: {0}")]
    Io(#[from] std::io::Error),

    /// A TOML values file failed to parse.
    #[error("failed to parse TOML values file: {0}")]
    ParseToml(#[source] Box<toml::de::Error>),

    /// A JSON values file failed to parse.
    #[error("failed to parse JSON values file: {0}")]
    ParseJson(#[source] Box<serde_json::Error>),

    /// The values file path has an unsupported extension.
    #[error("unsupported values file extension {0:?} (must be .toml or .json)")]
    BadExtension(String),

    /// The values file is missing the top-level `entries` list.
    #[error("values file missing top-level 'entries' list")]
    MissingEntries,

    /// The values file's `entries` list mixed incompatible entry shapes.
    #[error(
        "entries must be a list of strings or a list of value objects, got {0}"
    )]
    MixedEntries(String),

    #[error(transparent)]
    Entry(Box<SelectValuesError>),
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
            SchemaFieldParserError::NumberConstraint {
                address,
                key,
                value,
                expected,
            } => Self::InvalidNumberOverride {
                address,
                key,
                value,
                expected,
            },
            SchemaFieldParserError::NumberRange {
                address,
                min,
                max,
            } => Self::InvalidNumberRangeOverride {
                address,
                min,
                max,
            },
            SchemaFieldParserError::SelectValues {
                address,
                source,
            } => Self::SelectValuesOverrideDegraded {
                address,
                error: source.to_string(),
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

        #[test]
        fn select_values_formats_display_message() {
            let error = SchemaFieldParserError::SelectValues {
                address: FieldAddress::try_from("#book/status")
                    .expect("valid ref"),
                source: SelectValuesError::SelectorMissingKey {
                    selector: "label",
                    key: "name".to_owned(),
                },
            };

            assert_display(
                &error,
                "field #book/status configures selector \"label\" = \"name\", \
                 but an entry is missing this key",
            );
        }

        #[test]
        fn values_file_entry_formats_display_message_with_path() {
            let error = SchemaFieldParserError::SelectValues {
                address: FieldAddress::try_from("#book/status")
                    .expect("valid ref"),
                source: SelectValuesError::ValuesFile {
                    path: "values/statuses.json".to_owned(),
                    source: SelectValuesFileError::Entry(Box::new(
                        SelectValuesError::ReservedOutputKey {
                            key: "value".to_owned(),
                        },
                    )),
                },
            };

            assert_display(
                &error,
                "field #book/status values file \"values/statuses.json\" is \
                 invalid: entry key \"value\" is reserved for rendered select \
                 output; choose another source key or selector",
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
            let error = SchemaFieldParserError::SelectValues {
                address: FieldAddress::try_from("#sci_fi/status")
                    .expect("valid ref"),
                source: SelectValuesError::SelectorMissingKey {
                    selector: "label",
                    key: "label".to_owned(),
                },
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
