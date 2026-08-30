//! Field-construction error types:
//! [`SchemaFieldBuilder`](super::SchemaFieldBuilder) and
//! [`SchemaFieldParser`](super::parser::SchemaFieldParser)'s own failure modes.
//!
//! [`SchemaFieldParserError`] is the single per-key validation vocabulary
//! every field type's parser pushes through: `number`'s
//! [`SchemaFieldParserError::NumberConstraint`]/
//! [`SchemaFieldParserError::NumberRange`] and `select`'s
//! [`SchemaFieldParserError::SelectValue`] are field-specific variants on this
//! one enum, not a parallel error tree.
//!
//! Both types are only ever constructed and matched within [`super`]; the only
//! outside touch is [`SchemaFieldBuilderError`] wrapping into
//! [`SchemaError::FieldBuilder`](super::super::error::SchemaError::FieldBuilder)
//! and [`SchemaFieldParserError`] converting into
//! [`SchemaWarning`](super::super::error::SchemaWarning) for a degraded bare
//! `$ref` override.

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
/// Every field type pushes through this one vocabulary — `select`'s
/// [`Self::SelectValue`] wraps [`SchemaSelectFieldValueError`] the same way
/// `number`'s [`Self::NumberConstraint`]/[`Self::NumberRange`] are flat
/// siblings here, not a second parser-error hierarchy.
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
    SelectValue {
        address: FieldAddress,
        #[source]
        source: SchemaSelectFieldValueError,
    },
}

/// A validation failure inside a `select`/`multi` values definition.
///
/// Reuses [`SchemaSelectFieldEntryError::ShapeMismatch`] for every "this must
/// be {expected}, got {value}" failure (a file's `path`, a selector name, a
/// selected entry value, an `order` value) instead of a bespoke variant per
/// selector — the shape is always the same, only the description of what was
/// checked varies.
#[derive(Debug, Error)]
pub(crate) enum SchemaSelectFieldValueError {
    /// A file subtable omitted its required `path` attribute.
    #[error("values file subtable is missing required attribute \"path\"")]
    MissingPath,
    /// An entry has a shape or selector problem, whether declared inline in
    /// the field's own `values` list or loaded from an external file —
    /// `path` names which (`None` for inline).
    #[error(
        "{}",
        path.as_deref().map_or_else(
            || source.to_string(),
            |path| format!("values file {path:?}: {source}"),
        )
    )]
    Entry {
        path: Option<String>,
        #[source]
        source: SchemaSelectFieldEntryError,
    },
    /// The referenced values file itself could not be confined, read,
    /// parsed, or has the wrong top-level shape.
    #[error("values file {path:?} is invalid: {source}")]
    ValuesFile {
        path: String,
        #[source]
        source: SchemaSelectFieldFileError,
    },
}

/// One entry's shape or selector failure.
/// [`SchemaSelectFieldValueError::Entry`] carries whether the entry was inline
/// or loaded from a file — this type doesn't need to know.
#[derive(Debug, Error)]
pub(crate) enum SchemaSelectFieldEntryError {
    /// A selector's or entry key's value has the wrong shape.
    #[error("{context} must be {expected}, got {value}")]
    ShapeMismatch {
        context: String,
        value: String,
        expected: &'static str,
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
    /// An entry includes a passthrough key reserved by rendered select output.
    #[error(
        "entry key {key:?} is reserved for rendered select output; choose \
         another source key or selector"
    )]
    ReservedOutputKey {
        key: String,
    },
}

/// Why the file backing a `select`/`multi` values subtable could not be
/// loaded.
#[derive(Debug, Error)]
pub(crate) enum SchemaSelectFieldFileError {
    /// The values file path escaped the schema directory or contained unsafe
    /// components.
    #[error(transparent)]
    Confinement(#[from] PathError),
    /// An I/O error occurred while reading the values file.
    #[error("failed to read values file: {0}")]
    Io(#[from] std::io::Error),
    /// A TOML values file failed to parse.
    #[error("failed to parse TOML values file: {0}")]
    ParseToml(#[from] Box<toml::de::Error>),
    /// A JSON values file failed to parse.
    #[error("failed to parse JSON values file: {0}")]
    ParseJson(#[from] Box<serde_json::Error>),
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
}

impl From<SchemaFieldParserError> for SchemaWarning {
    /// Stringifies the wrapped error rather than re-deriving a parallel
    /// `SchemaWarning` variant per `SchemaFieldParserError` variant: a
    /// degraded `$ref` override reports the exact same failure a hard error
    /// would, plus "using the base value instead".
    fn from(error: SchemaFieldParserError) -> Self {
        Self::DegradedOverride {
            message: error.to_string(),
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
        fn select_value_formats_display_message_for_an_inline_entry() {
            let error = SchemaFieldParserError::SelectValue {
                address: FieldAddress::try_from("#book/status")
                    .expect("valid ref"),
                source: SchemaSelectFieldValueError::Entry {
                    path: None,
                    source: SchemaSelectFieldEntryError::SelectorMissingKey {
                        selector: "label",
                        key: "name".to_owned(),
                    },
                },
            };

            assert_display(
                &error,
                "field #book/status configures selector \"label\" = \"name\", \
                 but an entry is missing this key",
            );
        }

        #[test]
        fn select_value_formats_display_message_for_a_file_entry_with_path() {
            let error = SchemaFieldParserError::SelectValue {
                address: FieldAddress::try_from("#book/status")
                    .expect("valid ref"),
                source: SchemaSelectFieldValueError::Entry {
                    path: Some("values/statuses.json".to_owned()),
                    source: SchemaSelectFieldEntryError::ReservedOutputKey {
                        key: "value".to_owned(),
                    },
                },
            };

            assert_display(
                &error,
                "field #book/status values file \"values/statuses.json\": \
                 entry key \"value\" is reserved for rendered select output; \
                 choose another source key or selector",
            );
        }

        #[test]
        fn select_value_reuses_shape_mismatch_for_every_wrong_typed_selector() {
            // One variant covers what used to be four (`PathNotString`,
            // `SelectorNotString`, `SelectedValueNotString`, `OrderNotNumber`).
            let error = SchemaFieldParserError::SelectValue {
                address: FieldAddress::try_from("#book/status")
                    .expect("valid ref"),
                source: SchemaSelectFieldValueError::Entry {
                    path: None,
                    source: SchemaSelectFieldEntryError::ShapeMismatch {
                        context: "selector \"order\" = \"rank\"".to_owned(),
                        value: "\"first\"".to_owned(),
                        expected: "a number",
                    },
                },
            };

            assert_display(
                &error,
                "field #book/status selector \"order\" = \"rank\" must be a \
                 number, got \"first\"",
            );
        }

        #[test]
        fn number_constraint_formats_display_message() {
            let error = SchemaFieldParserError::NumberConstraint {
                address: FieldAddress::try_from("#book/rating")
                    .expect("valid ref"),
                key: "step".to_owned(),
                value: "0".to_owned(),
                expected: "positive",
            };

            assert_display(
                &error,
                "field #book/rating of type number's \"step\" attribute must \
                 be positive, got 0",
            );
        }

        #[test]
        fn number_range_formats_display_message() {
            let error = SchemaFieldParserError::NumberRange {
                address: FieldAddress::try_from("#book/rating")
                    .expect("valid ref"),
                min: "10".to_owned(),
                max: "1".to_owned(),
            };

            assert_display(
                &error,
                "field #book/rating of type number's \"min\" attribute must \
                 be <= \"max\", got min 10 and max 1",
            );
        }

        #[test]
        fn select_value_formats_display_message_for_a_missing_path() {
            let error = SchemaFieldParserError::SelectValue {
                address: FieldAddress::try_from("#book/status")
                    .expect("valid ref"),
                source: SchemaSelectFieldValueError::MissingPath,
            };

            assert_display(
                &error,
                "field #book/status values file subtable is missing required \
                 attribute \"path\"",
            );
        }

        #[test]
        fn select_value_formats_display_message_for_a_bad_values_file() {
            let error = SchemaFieldParserError::SelectValue {
                address: FieldAddress::try_from("#book/status")
                    .expect("valid ref"),
                source: SchemaSelectFieldValueError::ValuesFile {
                    path: "values/statuses.json".to_owned(),
                    source: SchemaSelectFieldFileError::MissingEntries,
                },
            };

            assert_display(
                &error,
                "field #book/status values file \"values/statuses.json\" is \
                 invalid: values file missing top-level 'entries' list",
            );
        }
    }

    mod schema_select_field_entry_error {
        use super::{super::*, assert_display};

        #[test]
        fn selector_on_bare_entries_formats_display_message() {
            let error = SchemaSelectFieldEntryError::SelectorOnBareEntries {
                selector: "label",
            };

            assert_display(
                &error,
                "configures selector \"label\" but entries contain bare \
                 string values",
            );
        }
    }

    mod schema_select_field_file_error {
        use super::{super::*, assert_display};

        #[test]
        fn confinement_delegates_display_to_the_path_error() {
            let error =
                SchemaSelectFieldFileError::Confinement(PathError::Absolute);

            assert_display(
                &error,
                "path is absolute, expected a relative path",
            );
        }

        #[test]
        fn io_formats_display_message() {
            let error =
                SchemaSelectFieldFileError::Io(std::io::Error::other("denied"));

            assert_display(&error, "failed to read values file: denied");
        }

        #[test]
        fn parse_toml_formats_display_message() {
            let source = "not valid toml".parse::<toml::Value>().unwrap_err();
            let error = SchemaSelectFieldFileError::ParseToml(Box::new(source));

            let message = error.to_string();
            assert!(
                message.starts_with("failed to parse TOML values file: "),
                "expected message to open with the TOML context, got: \
                 {message:?}"
            );
        }

        #[test]
        fn parse_json_formats_display_message() {
            let source = serde_json::from_str::<serde_json::Value>("not json")
                .unwrap_err();
            let error = SchemaSelectFieldFileError::ParseJson(Box::new(source));

            let message = error.to_string();
            assert!(
                message.starts_with("failed to parse JSON values file: "),
                "expected message to open with the JSON context, got: \
                 {message:?}"
            );
        }

        #[test]
        fn bad_extension_formats_display_message() {
            let error = SchemaSelectFieldFileError::BadExtension(
                "values.yaml".to_owned(),
            );

            assert_display(
                &error,
                "unsupported values file extension \"values.yaml\" (must be \
                 .toml or .json)",
            );
        }

        #[test]
        fn missing_entries_formats_display_message() {
            let error = SchemaSelectFieldFileError::MissingEntries;

            assert_display(
                &error,
                "values file missing top-level 'entries' list",
            );
        }

        #[test]
        fn mixed_entries_formats_display_message() {
            let error = SchemaSelectFieldFileError::MixedEntries(
                "a mix of strings and objects".to_owned(),
            );

            assert_display(
                &error,
                "entries must be a list of strings or a list of value \
                 objects, got a mix of strings and objects",
            );
        }
    }

    mod conversions {
        use super::super::*;

        #[test]
        fn stringifies_the_wrapped_error_instead_of_re_deriving_a_variant() {
            let error = SchemaFieldParserError::UnknownKey {
                address: FieldAddress::try_from("#sci_fi/cover")
                    .expect("valid ref"),
                kind: SchemaFieldTypeTag::Date,
                key: "values".to_owned(),
            };
            let expected_message = error.to_string();

            let warning = SchemaWarning::from(error);

            assert!(matches!(
                warning,
                SchemaWarning::DegradedOverride { ref message }
                    if *message == expected_message
            ));
        }

        #[test]
        fn select_value_error_converts_through_the_same_path_as_any_other() {
            let error = SchemaFieldParserError::SelectValue {
                address: FieldAddress::try_from("#sci_fi/status")
                    .expect("valid ref"),
                source: SchemaSelectFieldValueError::Entry {
                    path: None,
                    source: SchemaSelectFieldEntryError::SelectorMissingKey {
                        selector: "label",
                        key: "label".to_owned(),
                    },
                },
            };

            let warning = SchemaWarning::from(error);

            assert!(matches!(
                warning,
                SchemaWarning::DegradedOverride { ref message }
                    if message.contains("selector \"label\"")
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
