//! Field-attribute validation errors produced during type-specific option
//! parsing.

use super::{
    super::{
        error::{SchemaFieldBuilderError, SchemaWarning},
        raw::RawSchemaFieldType,
    },
    address::{FieldAddress, FieldAddressRef},
};

/// One field-attribute key/value validation failure from parsing a field type's
/// `options` bag: either the key doesn't belong to the field's resolved type,
/// or its value isn't shaped like the key expects.
///
/// Converts into a hard [`SchemaFieldBuilderError`] for a `Direct`/`$ref`
/// `type`-override field ([`super::builder::SchemaFieldBuilder::build`]'s
/// strict path), or a soft [`SchemaWarning`] for a bare `$ref` override (its
/// lenient path) — see the module docs.
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

/// Builds an [`AttributeError::UnknownKey`] for `key` on `kind`.
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

/// Builds an [`AttributeError::TypeMismatch`] for `key`'s wrongly-shaped
/// `value` on `kind`, rendering `value` via [`std::fmt::Debug`] for the error
/// message.
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

/// Returns `value` as an owned list of strings, or `None` if it isn't a list of
/// nothing but strings.
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

/// Returns `value` as an owned string, or `None` if it isn't one.
pub(super) fn expect_string(
    value: &crate::field::FieldValue,
) -> Option<String> {
    match value {
        crate::field::FieldValue::String(s) => Some(s.clone()),
        _ => None,
    }
}
