//! Per-key field-attribute validation errors.

use super::{
    super::error::{SchemaFieldBuilderError, SchemaWarning},
    SchemaFieldType,
    address::{FieldAddress, FieldAddressRef},
};

/// One per-key validation failure from parsing a field type's `options`.
///
/// Converts into:
/// - [`SchemaFieldBuilderError`] (hard failure) for `Direct` fields and `$ref`
///   with a `type` override.
/// - [`SchemaWarning`] (degraded) for bare `$ref` overrides.
pub(crate) enum SchemaFieldParserError {
    UnknownKey {
        address: FieldAddress,
        kind: SchemaFieldType,
        key: String,
    },
    TypeMismatch {
        address: FieldAddress,
        kind: SchemaFieldType,
        key: String,
        value: String,
        expected: &'static str,
    },
}

impl From<SchemaFieldParserError> for SchemaFieldBuilderError {
    fn from(error: SchemaFieldParserError) -> Self {
        match error {
            SchemaFieldParserError::UnknownKey {
                address,
                kind,
                key,
            } => Self::UnknownAttributeKey {
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
            } => Self::AttributeValueTypeMismatch {
                address,
                kind,
                key,
                value,
                expected,
            },
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
        }
    }
}

/// Build a [`SchemaFieldParserError::UnknownKey`] for an unrecognized attribute
/// key.
pub(super) fn unknown_key(
    address: FieldAddressRef<'_>,
    kind: SchemaFieldType,
    key: &str,
) -> SchemaFieldParserError {
    SchemaFieldParserError::UnknownKey {
        address: FieldAddress::from(address),
        kind,
        key: key.to_owned(),
    }
}

/// Build a [`SchemaFieldParserError::TypeMismatch`] for a wrongly-shaped
/// `value`.
pub(super) fn type_mismatch(
    address: FieldAddressRef<'_>,
    kind: SchemaFieldType,
    key: &str,
    value: &crate::field::FieldValue,
    expected: &'static str,
) -> SchemaFieldParserError {
    SchemaFieldParserError::TypeMismatch {
        address: FieldAddress::from(address),
        kind,
        key: key.to_owned(),
        value: format!("{value:?}"),
        expected,
    }
}
