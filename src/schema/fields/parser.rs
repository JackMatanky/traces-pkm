//! Shared attribute-table parser for per-type field parsing.
//!
//! Each per-type [`parse`][super::select::SchemaSelectField::parse] function
//! receives a [`SchemaFieldParser`], claims keys via typed extractors, and
//! calls [`SchemaFieldParser::finish`] to detect unknown keys.

use std::collections::{BTreeMap, BTreeSet};

use super::{
    super::error::SchemaFieldParserError,
    SchemaFieldType,
    address::{FieldAddress, FieldAddressRef},
};
use crate::field::FieldValue;

/// Attribute-table parser that tracks claimed keys and emits errors.
///
/// Per-type [`parse`][super::select::SchemaSelectField::parse] functions create
/// one of these, call typed extractors ([`string`](Self::string),
/// [`string_list`](Self::string_list), [`f64`](Self::f64)) for each known key,
/// then call [`finish`](Self::finish) to get residual unknown-key errors.
pub(super) struct SchemaFieldParser<'a> {
    address: FieldAddressRef<'a>,
    kind: SchemaFieldType,
    claimed: BTreeSet<&'static str>,
    errors: Vec<SchemaFieldParserError>,
}

impl<'a> SchemaFieldParser<'a> {
    /// Creates a parser that tracks claimed keys for the field at `address`.
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

    /// Extracts a [`String`] for `key`, using `fallback` when the key is
    /// absent.
    ///
    /// A type-mismatch error is pushed to `self.errors` when the key is present
    /// but its value is not a string.
    pub(super) fn string(
        &mut self,
        options: &BTreeMap<String, FieldValue>,
        key: &'static str,
        fallback: Option<String>,
    ) -> Option<String> {
        self.claimed.insert(key);
        match options.get(key) {
            Some(value) => {
                if let FieldValue::String(s) = value {
                    Some(s.clone())
                } else {
                    self.errors.push(
                        self.type_mismatch(&self.kind, key, value, "a string"),
                    );
                    None
                }
            }
            None => fallback,
        }
    }

    /// Extracts a [`Vec<String>`] for `key`, using `fallback` when the key is
    /// absent.
    ///
    /// A type-mismatch error is pushed to `self.errors` when the key is present
    /// but its value is not an array of strings.
    pub(super) fn string_list(
        &mut self,
        options: &BTreeMap<String, FieldValue>,
        key: &'static str,
        fallback: Vec<String>,
    ) -> Vec<String> {
        self.claimed.insert(key);
        match options.get(key) {
            Some(value) => {
                if let FieldValue::List(items) = value {
                    if items
                        .iter()
                        .all(|item| matches!(item, FieldValue::String(_)))
                    {
                        return items
                            .iter()
                            .filter_map(|item| match item {
                                FieldValue::String(s) => Some(s.clone()),
                                _ => None,
                            })
                            .collect();
                    }
                }
                self.errors.push(self.type_mismatch(
                    &self.kind,
                    key,
                    value,
                    "an array of strings",
                ));
                fallback
            }
            None => fallback,
        }
    }

    /// Extracts an `f64` for `key`, using `fallback` when the key is absent.
    ///
    /// A type-mismatch error is pushed to `self.errors` when the key is present
    /// but its value is not numeric.
    pub(super) fn f64(
        &mut self,
        options: &BTreeMap<String, FieldValue>,
        key: &'static str,
        fallback: Option<f64>,
    ) -> Option<f64> {
        self.claimed.insert(key);
        match options.get(key) {
            Some(value) => {
                if let Some(number) = value.as_f64() {
                    Some(number)
                } else {
                    self.errors.push(
                        self.type_mismatch(&self.kind, key, value, "a number"),
                    );
                    None
                }
            }
            None => fallback,
        }
    }

    /// Collects accumulated errors and emits an
    /// [`UnknownKey`](SchemaFieldParserError::UnknownKey) for every key in
    /// `options` that was not claimed by a typed extractor.
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

    /// Builds a [`TypeMismatch`](SchemaFieldParserError::TypeMismatch) error
    /// for an unexpected `value`.
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
