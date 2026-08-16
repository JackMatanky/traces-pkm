//! Per-key field-attribute validation errors.

use super::{
    super::{
        error::{SchemaFieldBuilderError, SchemaWarning},
        raw::RawSchemaFieldType,
    },
    address::{FieldAddress, FieldAddressRef},
};

/// One per-key validation failure from parsing a field type's `options`.
///
/// Converts into:
/// - [`SchemaFieldBuilderError`] (hard failure) for `Direct` fields and `$ref`
///   with a `type` override.
/// - [`SchemaWarning`] (degraded) for bare `$ref` overrides.
pub(crate) enum AttributeError {
    UnknownKey {
        address: FieldAddress,
        kind: RawSchemaFieldType,
        key: String,
    },
    TypeMismatch {
        address: FieldAddress,
        kind: RawSchemaFieldType,
        key: String,
        value: String,
        expected: &'static str,
    },
}

impl From<AttributeError> for SchemaFieldBuilderError {
    fn from(error: AttributeError) -> Self {
        match error {
            AttributeError::UnknownKey {
                address,
                kind,
                key,
            } => Self::UnknownAttributeKey {
                address,
                kind,
                key,
            },
            AttributeError::TypeMismatch {
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

impl From<AttributeError> for SchemaWarning {
    fn from(error: AttributeError) -> Self {
        match error {
            AttributeError::UnknownKey {
                address,
                kind,
                key,
            } => Self::UnknownOverrideKey {
                address,
                kind,
                key,
            },
            AttributeError::TypeMismatch {
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

/// Build an [`AttributeError::UnknownKey`] for an unrecognized attribute key.
pub(super) fn unknown_key(
    address: FieldAddressRef<'_>,
    kind: RawSchemaFieldType,
    key: &str,
) -> AttributeError {
    AttributeError::UnknownKey {
        address: FieldAddress::from(address),
        kind,
        key: key.to_owned(),
    }
}

/// Build an [`AttributeError::TypeMismatch`] for a wrongly-shaped `value`.
pub(super) fn type_mismatch(
    address: FieldAddressRef<'_>,
    kind: RawSchemaFieldType,
    key: &str,
    value: &crate::field::FieldValue,
    expected: &'static str,
) -> AttributeError {
    AttributeError::TypeMismatch {
        address: FieldAddress::from(address),
        kind,
        key: key.to_owned(),
        value: format!("{value:?}"),
        expected,
    }
}

/// Return `value` as an owned list of strings, or `None` if it is not a list
/// of strings.
pub(super) fn expect_string_list(
    value: &crate::field::FieldValue,
) -> Option<Vec<String>> {
    let crate::field::FieldValue::List(items) = value else {
        return None;
    };
    items
        .iter()
        .map(|item| match item {
            crate::field::FieldValue::String(s) => Some(s.clone()),
            _ => None,
        })
        .collect()
}

/// Return `value` as an owned string, or `None` if it is not.
pub(super) fn expect_string(
    value: &crate::field::FieldValue,
) -> Option<String> {
    match value {
        crate::field::FieldValue::String(s) => Some(s.clone()),
        _ => None,
    }
}
