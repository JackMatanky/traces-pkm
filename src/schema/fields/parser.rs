//! Shared attribute-table parser for per-type field parsing.
//!
//! Each per-type `parse` function receives a [`SchemaFieldParser`], claims keys
//! via typed extractors, and calls [`SchemaFieldParser::finish`] to detect
//! unknown keys. This replaces the duplicated loop-and-match that previously
//! existed in `select`, `number`, `date`, and `file`.

use std::collections::{BTreeMap, BTreeSet};

use super::{
    address::FieldAddressRef,
    error::{AttributeError, type_mismatch, unknown_key},
};
use crate::{field::FieldValue, schema::raw::RawSchemaFieldType};

/// Parse `options` for a simple type (no type-specific keys) and return every
/// key as an unknown-key error.
pub(super) fn parse_simple(
    address: FieldAddressRef<'_>,
    kind: RawSchemaFieldType,
    options: &BTreeMap<String, FieldValue>,
) -> Vec<AttributeError> {
    let parser = SchemaFieldParser::new(address, kind);
    parser.finish(options)
}

/// Attribute-table parser that tracks claimed keys and emits unknown-key errors
/// for anything left over.
///
/// Per-type `parse` functions create one of these, call typed extractors for
/// each known key, then call [`finish`](Self::finish) to get the residual
/// unknown-key errors.
pub(super) struct SchemaFieldParser<'a> {
    address: FieldAddressRef<'a>,
    kind: RawSchemaFieldType,
    claimed: BTreeSet<&'static str>,
}

impl<'a> SchemaFieldParser<'a> {
    /// Create a new parser for a field at `address` of type `kind`.
    pub(super) fn new(
        address: FieldAddressRef<'a>,
        kind: RawSchemaFieldType,
    ) -> Self {
        Self {
            address,
            kind,
            claimed: BTreeSet::new(),
        }
    }

    /// Extract a `String` value for `key`, falling back to `fallback` when
    /// `options` does not contain the key.
    ///
    /// Returns `None` when the key is present but its value is not a string
    /// (a type-mismatch error is pushed to `errors`).
    pub(super) fn string(
        &mut self,
        options: &BTreeMap<String, FieldValue>,
        key: &'static str,
        fallback: Option<String>,
        errors: &mut Vec<AttributeError>,
    ) -> Option<String> {
        self.claimed.insert(key);
        match options.get(key) {
            Some(value) => match value {
                FieldValue::String(s) => Some(s.clone()),
                _ => {
                    errors.push(type_mismatch(
                        self.address,
                        self.kind,
                        key,
                        value,
                        "a string",
                    ));
                    None
                }
            },
            None => fallback,
        }
    }

    /// Extract a `Vec<String>` value for `key`, falling back to `fallback`
    /// when `options` does not contain the key.
    ///
    /// Returns `None` when the key is present but its value is not a list of
    /// strings (a type-mismatch error is pushed to `errors`).
    pub(super) fn string_list(
        &mut self,
        options: &BTreeMap<String, FieldValue>,
        key: &'static str,
        fallback: Vec<String>,
        errors: &mut Vec<AttributeError>,
    ) -> Vec<String> {
        self.claimed.insert(key);
        match options.get(key) {
            Some(value) => match value {
                FieldValue::List(items) => {
                    let all_strings = items
                        .iter()
                        .map(|item| match item {
                            FieldValue::String(s) => Some(s.clone()),
                            _ => None,
                        })
                        .collect::<Option<Vec<_>>>();
                    match all_strings {
                        Some(list) => list,
                        None => {
                            errors.push(type_mismatch(
                                self.address,
                                self.kind,
                                key,
                                value,
                                "an array of strings",
                            ));
                            fallback
                        }
                    }
                }
                _ => {
                    errors.push(type_mismatch(
                        self.address,
                        self.kind,
                        key,
                        value,
                        "an array of strings",
                    ));
                    fallback
                }
            },
            None => fallback,
        }
    }

    /// Extract an `f64` value for `key`, falling back to `fallback` when
    /// `options` does not contain the key.
    ///
    /// Returns `None` when the key is present but its value is not numeric (a
    /// type-mismatch error is pushed to `errors`).
    pub(super) fn f64(
        &mut self,
        options: &BTreeMap<String, FieldValue>,
        key: &'static str,
        fallback: Option<f64>,
        errors: &mut Vec<AttributeError>,
    ) -> Option<f64> {
        self.claimed.insert(key);
        match options.get(key) {
            Some(value) => match value.as_f64() {
                Some(number) => Some(number),
                None => {
                    errors.push(type_mismatch(
                        self.address,
                        self.kind,
                        key,
                        value,
                        "a number",
                    ));
                    None
                }
            },
            None => fallback,
        }
    }

    /// Consume the parser and return unknown-key errors for every key in
    /// `options` that was not claimed by a typed extractor.
    pub(super) fn finish(
        self,
        options: &BTreeMap<String, FieldValue>,
    ) -> Vec<AttributeError> {
        options
            .keys()
            .filter(|key| !self.claimed.contains(key.as_str()))
            .map(|key| unknown_key(self.address, self.kind, key))
            .collect()
    }
}
