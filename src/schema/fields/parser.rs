//! Shared attribute-table parser for per-type field parsing.
//!
//! Each per-type `parse` function receives a [`SchemaFieldParser`], claims keys
//! via typed extractors, and calls [`SchemaFieldParser::finish`] to detect
//! unknown keys. This replaces the duplicated loop-and-match that previously
//! existed in `select`, `number`, `date`, and `file`.

use std::collections::{BTreeMap, BTreeSet};

use super::{
    super::error::SchemaFieldParserError,
    SchemaFieldType,
    address::{FieldAddress, FieldAddressRef},
};
use crate::field::FieldValue;

/// Attribute-table parser that tracks claimed keys and emits unknown-key errors
/// for anything left over.
///
/// Per-type `parse` functions create one of these, call typed extractors for
/// each known key, then call [`finish`](Self::finish) to get the residual
/// unknown-key errors.
pub(super) struct SchemaFieldParser<'a> {
    address: FieldAddressRef<'a>,
    kind: SchemaFieldType,
    claimed: BTreeSet<&'static str>,
    errors: Vec<SchemaFieldParserError>,
}

impl<'a> SchemaFieldParser<'a> {
    /// Create a new parser for a field at `address` of type `kind`.
    pub(super) fn new(
        address: FieldAddressRef<'a>,
        kind: SchemaFieldType,
    ) -> Self {
        Self {
            address,
            kind,
            claimed: BTreeSet::new(),
            errors: Vec::new(),
        }
    }

    /// Extract a `String` value for `key`, falling back to `fallback` when
    /// `options` does not contain the key.
    ///
    /// Returns `None` when the key is present but its value is not a string
    /// (a type-mismatch error is pushed to `self.errors`).
    pub(super) fn string(
        &mut self,
        options: &BTreeMap<String, FieldValue>,
        key: &'static str,
        fallback: Option<String>,
    ) -> Option<String> {
        self.claimed.insert(key);
        match options.get(key) {
            Some(value) => match value {
                FieldValue::String(s) => Some(s.clone()),
                _ => {
                    self.errors.push(
                        self.type_mismatch(&self.kind, key, value, "a string"),
                    );
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
    /// strings (a type-mismatch error is pushed to `self.errors`).
    pub(super) fn string_list(
        &mut self,
        options: &BTreeMap<String, FieldValue>,
        key: &'static str,
        fallback: Vec<String>,
    ) -> Vec<String> {
        self.claimed.insert(key);
        match options.get(key) {
            Some(value) => match value {
                FieldValue::List(items) => {
                    // Validate first, collect only on success.
                    if items
                        .iter()
                        .all(|item| matches!(item, FieldValue::String(_)))
                    {
                        items
                            .iter()
                            .map(|item| match item {
                                FieldValue::String(s) => s.clone(),
                                _ => unreachable!(),
                            })
                            .collect()
                    } else {
                        self.errors.push(self.type_mismatch(
                            &self.kind,
                            key,
                            value,
                            "an array of strings",
                        ));
                        fallback
                    }
                }
                _ => {
                    self.errors.push(self.type_mismatch(
                        &self.kind,
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
    /// type-mismatch error is pushed to `self.errors`).
    pub(super) fn f64(
        &mut self,
        options: &BTreeMap<String, FieldValue>,
        key: &'static str,
        fallback: Option<f64>,
    ) -> Option<f64> {
        self.claimed.insert(key);
        match options.get(key) {
            Some(value) => match value.as_f64() {
                Some(number) => Some(number),
                None => {
                    self.errors.push(
                        self.type_mismatch(&self.kind, key, value, "a number"),
                    );
                    None
                }
            },
            None => fallback,
        }
    }

    /// Consume the parser and return accumulated errors.
    ///
    /// Returns type-mismatch errors from extraction, plus unknown-key errors
    /// for every key in `options` not claimed by a typed extractor.
    pub(super) fn finish(
        mut self,
        options: &BTreeMap<String, FieldValue>,
    ) -> Vec<SchemaFieldParserError> {
        let mut errors = std::mem::take(&mut self.errors);
        let mut unknowns: Vec<_> = options
            .keys()
            .filter(|key| !self.claimed.contains(key.as_str()))
            .collect();
        if let Some(last) = unknowns.pop() {
            // Move kind into the last error; clone for the rest.
            let kind = self.kind.clone();
            for key in unknowns {
                errors.push(SchemaFieldParserError::UnknownKey {
                    address: FieldAddress::from(self.address),
                    kind: kind.clone(),
                    key: key.to_owned(),
                });
            }
            errors.push(SchemaFieldParserError::UnknownKey {
                address: FieldAddress::from(self.address),
                kind: self.kind,
                key: last.to_owned(),
            });
        }
        errors
    }

    /// Build a [`SchemaFieldParserError::TypeMismatch`] for `key`'s `value`.
    fn type_mismatch(
        &self,
        kind: &SchemaFieldType,
        key: &str,
        value: &FieldValue,
        expected: &'static str,
    ) -> SchemaFieldParserError {
        SchemaFieldParserError::TypeMismatch {
            address: FieldAddress::from(self.address),
            kind: kind.clone(),
            key: key.to_owned(),
            value: format!("{value:?}"),
            expected,
        }
    }
}
