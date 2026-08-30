//! Attribute-table parsing utilities for schema field definitions.
//!
//! When deserializing schema field options, field parsers inspect an underlying
//! key-value map. This module provides [`SchemaFieldParser`], a tracker that
//! validates expected field types, registers accessed keys, and detects
//! unrecognized or extraneous configuration options.

use std::collections::HashSet;

use indexmap::IndexMap;

use super::{
    SchemaFieldTypeTag,
    address::{FieldAddress, FieldAddressRef},
    error::{SchemaFieldParserError, SchemaSelectFieldValueError},
};
use crate::field::FieldValue;

/// Key extractor and error collector for schema field option tables.
///
/// `SchemaFieldParser` coordinates field validation by recording which keys
/// were explicitly requested via typed extractor methods
/// ([`string`](Self::string), [`string_list`](Self::string_list), and
/// [`f64`](Self::f64)). When extraction completes, invoking
/// [`finish`](Self::finish) checks the remaining entries in the source map to
/// flag unknown keys alongside any type mismatch errors accumulated during
/// extraction.
pub(super) struct SchemaFieldParser<'a> {
    address: FieldAddressRef<'a>,
    kind: SchemaFieldTypeTag,
    claimed: HashSet<&'static str>,
    errors: Vec<SchemaFieldParserError>,
}

impl<'a> SchemaFieldParser<'a> {
    /// Creates a new field parser scoped to a specific field address and tag.
    ///
    /// # Arguments
    ///
    /// * `address`: Path reference identifying the field location within the
    ///   schema hierarchy.
    /// * `kind`: The expected schema field tag being processed.
    pub(super) fn new(
        address: FieldAddressRef<'a>,
        kind: SchemaFieldTypeTag,
    ) -> Self {
        Self {
            address,
            kind,
            claimed: HashSet::new(),
            errors: Vec::new(),
        }
    }

    /// Claims `key` and returns the unparsed [`FieldValue`], if present.
    pub(super) fn raw_value<'m>(
        &mut self,
        options: &'m IndexMap<String, FieldValue>,
        key: &'static str,
    ) -> Option<&'m FieldValue> {
        self.claimed.insert(key);
        options.get(key)
    }

    /// Marks the current parser error count for later fallback decisions.
    pub(super) fn mark_errors(&self) -> usize {
        self.errors.len()
    }

    /// Returns whether new errors were recorded after `mark`.
    pub(super) fn has_errors_since(&self, mark: usize) -> bool {
        self.errors.len() > mark
    }

    /// Pushes a typed mismatch error for `key`.
    pub(super) fn push_type_mismatch<T: std::fmt::Debug + ?Sized>(
        &mut self,
        key: &str,
        value: &T,
        expected: &'static str,
    ) {
        let error = self.type_mismatch(key, value, expected);
        self.errors.push(error);
    }

    /// Pushes an unknown option-key error for `key`.
    pub(super) fn push_unknown_key(&mut self, key: &str) {
        self.errors.push(SchemaFieldParserError::UnknownKey {
            address: FieldAddress::from(self.address),
            kind: self.kind,
            key: key.to_owned(),
        });
    }

    /// Pushes a `select`/`multi` values-specific parser error.
    pub(super) fn push_select_value_error(
        &mut self,
        source: SchemaSelectFieldValueError,
    ) {
        self.errors.push(SchemaFieldParserError::SelectValue {
            address: FieldAddress::from(self.address),
            source,
        });
    }

    /// Pushes a `number` field invariant violation for one attribute.
    pub(super) fn push_number_constraint(
        &mut self,
        key: &'static str,
        value: f64,
        expected: &'static str,
    ) {
        self.errors.push(SchemaFieldParserError::NumberConstraint {
            address: FieldAddress::from(self.address),
            key: key.to_owned(),
            value: value.to_string(),
            expected,
        });
    }

    /// Pushes a `number` field range invariant violation.
    pub(super) fn push_number_range(&mut self, min: f64, max: f64) {
        self.errors.push(SchemaFieldParserError::NumberRange {
            address: FieldAddress::from(self.address),
            min: min.to_string(),
            max: max.to_string(),
        });
    }

    /// Extracts a string value associated with a specified key.
    ///
    /// Marks `key` as claimed. Returns `fallback` if `key` is not present in
    /// `options`.
    ///
    /// # Arguments
    ///
    /// * `options` - the raw key-value options to extract from.
    /// * `key` - the attribute key to look up.
    /// * `fallback` - value to return when `key` is absent.
    ///
    /// # Errors
    ///
    /// - [`TypeMismatch`] if `key` is present but the corresponding
    ///   [`FieldValue`] is not a [`FieldValue::String`].
    ///
    /// [`FieldValue`]: crate::field::FieldValue
    /// [`TypeMismatch`]: SchemaFieldParserError::TypeMismatch
    pub(super) fn string(
        &mut self,
        options: &IndexMap<String, FieldValue>,
        key: &'static str,
        fallback: Option<String>,
    ) -> Option<String> {
        self.claimed.insert(key);
        match options.get(key) {
            Some(value) => {
                if let FieldValue::String(s) = value {
                    Some(s.clone())
                } else {
                    self.errors
                        .push(self.type_mismatch(key, value, "a string"));
                    None
                }
            }
            None => fallback,
        }
    }

    /// Extracts a list of strings associated with a specified key.
    ///
    /// Marks `key` as claimed. Returns `fallback` if `key` is not present in
    /// `options`.
    ///
    /// # Arguments
    ///
    /// * `options` - the raw key-value options to extract from.
    /// * `key` - the attribute key to look up.
    /// * `fallback` - value to return when `key` is absent.
    ///
    /// # Errors
    ///
    /// - [`TypeMismatch`] if `key` is present but the corresponding
    ///   [`FieldValue`] is not a [`FieldValue::List`] containing exclusively
    ///   string values.
    ///
    /// [`FieldValue`]: crate::field::FieldValue
    /// [`TypeMismatch`]: SchemaFieldParserError::TypeMismatch
    pub(super) fn string_list(
        &mut self,
        options: &IndexMap<String, FieldValue>,
        key: &'static str,
        fallback: Vec<String>,
    ) -> Vec<String> {
        self.claimed.insert(key);
        match options.get(key) {
            Some(value) => {
                if let FieldValue::List(items) = value
                    && items
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
                self.errors.push(self.type_mismatch(
                    key,
                    value,
                    "an array of strings",
                ));
                fallback
            }
            None => fallback,
        }
    }

    /// Extracts a floating-point numeric value associated with a specified key.
    ///
    /// Marks `key` as claimed. Returns `fallback` if `key` is not present in
    /// `options`.
    ///
    /// # Arguments
    ///
    /// * `options` - the raw key-value options to extract from.
    /// * `key` - the attribute key to look up.
    /// * `fallback` - value to return when `key` is absent.
    ///
    /// # Errors
    ///
    /// - [`TypeMismatch`] if `key` is present but the corresponding
    ///   [`FieldValue`] cannot be converted to an `f64`.
    ///
    /// [`FieldValue`]: crate::field::FieldValue
    /// [`TypeMismatch`]: SchemaFieldParserError::TypeMismatch
    pub(super) fn f64(
        &mut self,
        options: &IndexMap<String, FieldValue>,
        key: &'static str,
        fallback: Option<f64>,
    ) -> Option<f64> {
        self.claimed.insert(key);
        match options.get(key) {
            Some(value) => {
                if let Some(number) = value.as_f64() {
                    Some(number)
                } else {
                    self.errors
                        .push(self.type_mismatch(key, value, "a number"));
                    None
                }
            }
            None => fallback,
        }
    }

    /// Finalizes parsing by detecting unclaimed keys and returning all
    /// accumulated errors.
    ///
    /// Any key present in `options` that was not accessed via a typed
    /// extractor is treated as invalid and converted into a
    /// [`SchemaFieldParserError::UnknownKey`]. Returns a [`Vec`] containing
    /// all accumulated type mismatch errors and unknown key violations.
    /// An empty list means all options were valid and recognized.
    pub(super) fn finish(
        self,
        options: &IndexMap<String, FieldValue>,
    ) -> Vec<SchemaFieldParserError> {
        let mut errors = self.errors;
        errors.extend(
            options
                .keys()
                .filter(|key| !self.claimed.contains(key.as_str()))
                .map(|key| SchemaFieldParserError::UnknownKey {
                    address: FieldAddress::from(self.address),
                    kind: self.kind,
                    key: key.to_owned(),
                }),
        );
        errors
    }

    /// Constructs a [`SchemaFieldParserError::TypeMismatch`] for an unexpected
    /// value.
    ///
    /// # Arguments
    ///
    /// * `key` - the attribute key whose value was unexpected.
    /// * `value` - the actual value encountered.
    /// * `expected` - a human-readable description of the expected shape.
    fn type_mismatch<T: std::fmt::Debug + ?Sized>(
        &self,
        key: &str,
        value: &T,
        expected: &'static str,
    ) -> SchemaFieldParserError {
        SchemaFieldParserError::TypeMismatch {
            address: FieldAddress::from(self.address),
            kind: self.kind,
            key: key.to_owned(),
            value: format!("{value:?}"),
            expected,
        }
    }
}

#[cfg(test)]
mod tests {
    use indexmap::IndexMap;

    use super::*;
    use crate::schema::fields::{
        address::FieldAddress, error::SchemaFieldParserError,
        parser::SchemaFieldParser,
    };

    fn address() -> FieldAddress {
        FieldAddress::try_from("#book/field").expect("valid ref")
    }

    fn options(pairs: &[(&str, FieldValue)]) -> IndexMap<String, FieldValue> {
        pairs.iter().map(|(k, v)| ((*k).to_owned(), v.clone())).collect()
    }

    mod string_extractor {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn returns_cloned_value_when_key_is_a_string() {
            let opts =
                options(&[("name", FieldValue::String("alice".to_owned()))]);
            let addr = address();
            let mut parser = SchemaFieldParser::new(
                addr.as_ref(),
                SchemaFieldTypeTag::Input,
            );

            let result = parser.string(&opts, "name", None);

            assert_eq!(result, Some("alice".to_owned()));
            assert!(parser.finish(&opts).is_empty());
        }

        #[test]
        fn returns_fallback_when_key_is_absent() {
            let opts = IndexMap::new();
            let addr = address();
            let mut parser = SchemaFieldParser::new(
                addr.as_ref(),
                SchemaFieldTypeTag::Input,
            );

            let result =
                parser.string(&opts, "name", Some("default".to_owned()));

            assert_eq!(result, Some("default".to_owned()));
            assert!(parser.finish(&opts).is_empty());
        }

        #[test]
        fn returns_none_and_records_type_mismatch_when_value_is_not_a_string() {
            let opts = options(&[("name", FieldValue::Int(42))]);
            let addr = address();
            let mut parser = SchemaFieldParser::new(
                addr.as_ref(),
                SchemaFieldTypeTag::Input,
            );

            let result = parser.string(&opts, "name", None);

            assert_eq!(result, None);
            let errors = parser.finish(&opts);
            assert_eq!(errors.len(), 1);
            assert!(matches!(
                errors.first().expect("expected error"),
                SchemaFieldParserError::TypeMismatch { .. }
            ));
        }
    }

    mod string_list_extractor {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn collects_all_strings_from_a_list() {
            let opts = options(&[(
                "tags",
                FieldValue::List(vec![
                    FieldValue::String("a".to_owned()),
                    FieldValue::String("b".to_owned()),
                ]),
            )]);
            let addr = address();
            let mut parser = SchemaFieldParser::new(
                addr.as_ref(),
                SchemaFieldTypeTag::Input,
            );

            let result = parser.string_list(&opts, "tags", Vec::new());

            assert_eq!(result, vec!["a".to_owned(), "b".to_owned()]);
            assert!(parser.finish(&opts).is_empty());
        }

        #[test]
        fn returns_fallback_when_key_is_absent() {
            let opts = IndexMap::new();
            let addr = address();
            let mut parser = SchemaFieldParser::new(
                addr.as_ref(),
                SchemaFieldTypeTag::Input,
            );

            let result =
                parser.string_list(&opts, "tags", vec!["default".to_owned()]);

            assert_eq!(result, vec!["default".to_owned()]);
            assert!(parser.finish(&opts).is_empty());
        }

        #[test]
        fn returns_fallback_and_records_type_mismatch_when_value_is_not_a_list()
        {
            let opts = options(&[(
                "tags",
                FieldValue::String("not-a-list".to_owned()),
            )]);
            let addr = address();
            let mut parser = SchemaFieldParser::new(
                addr.as_ref(),
                SchemaFieldTypeTag::Input,
            );

            let result =
                parser.string_list(&opts, "tags", vec!["fb".to_owned()]);

            assert_eq!(result, vec!["fb".to_owned()]);
            let errors = parser.finish(&opts);
            assert_eq!(errors.len(), 1);
            assert!(matches!(
                errors.first().expect("expected error"),
                SchemaFieldParserError::TypeMismatch { .. }
            ));
        }

        #[test]
        fn returns_fallback_and_records_type_mismatch_when_list_contains_non_strings()
         {
            let opts = options(&[(
                "tags",
                FieldValue::List(vec![FieldValue::Int(1), FieldValue::Int(2)]),
            )]);
            let addr = address();
            let mut parser = SchemaFieldParser::new(
                addr.as_ref(),
                SchemaFieldTypeTag::Input,
            );

            let result = parser.string_list(&opts, "tags", Vec::new());

            assert!(result.is_empty());
            let errors = parser.finish(&opts);
            assert_eq!(errors.len(), 1);
            assert!(matches!(
                errors.first().expect("expected error"),
                SchemaFieldParserError::TypeMismatch { .. }
            ));
        }

        #[test]
        fn returns_an_empty_vec_for_an_empty_list_value() {
            let opts = options(&[("tags", FieldValue::List(Vec::new()))]);
            let addr = address();
            let mut parser = SchemaFieldParser::new(
                addr.as_ref(),
                SchemaFieldTypeTag::Input,
            );

            let result =
                parser.string_list(&opts, "tags", vec!["fb".to_owned()]);

            assert!(result.is_empty());
            assert!(parser.finish(&opts).is_empty());
        }
    }

    mod f64_extractor {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn converts_an_integer_value_to_f64() {
            let opts = options(&[("min", FieldValue::Int(5))]);
            let addr = address();
            let mut parser = SchemaFieldParser::new(
                addr.as_ref(),
                SchemaFieldTypeTag::Input,
            );

            let result = parser.f64(&opts, "min", None);

            assert_eq!(result, Some(5.0));
            assert!(parser.finish(&opts).is_empty());
        }

        #[test]
        fn returns_a_float_value_as_is() {
            let opts = options(&[("min", FieldValue::Float(2.5))]);
            let addr = address();
            let mut parser = SchemaFieldParser::new(
                addr.as_ref(),
                SchemaFieldTypeTag::Input,
            );

            let result = parser.f64(&opts, "min", None);

            assert_eq!(result, Some(2.5));
            assert!(parser.finish(&opts).is_empty());
        }

        #[test]
        fn returns_fallback_when_key_is_absent() {
            let opts = IndexMap::new();
            let addr = address();
            let mut parser = SchemaFieldParser::new(
                addr.as_ref(),
                SchemaFieldTypeTag::Input,
            );

            let result = parser.f64(&opts, "min", Some(0.0));

            assert_eq!(result, Some(0.0));
            assert!(parser.finish(&opts).is_empty());
        }

        #[test]
        fn returns_none_and_records_type_mismatch_when_value_is_not_numeric() {
            let opts =
                options(&[("min", FieldValue::String("abc".to_owned()))]);
            let addr = address();
            let mut parser = SchemaFieldParser::new(
                addr.as_ref(),
                SchemaFieldTypeTag::Input,
            );

            let result = parser.f64(&opts, "min", None);

            assert_eq!(result, None);
            let errors = parser.finish(&opts);
            assert_eq!(errors.len(), 1);
            assert!(matches!(
                errors.first().expect("expected error"),
                SchemaFieldParserError::TypeMismatch { .. }
            ));
        }
    }

    mod finish {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn returns_empty_when_no_unknowns_and_no_accumulated_errors() {
            let opts = options(&[("name", FieldValue::String("x".to_owned()))]);
            let addr = address();
            let mut parser = SchemaFieldParser::new(
                addr.as_ref(),
                SchemaFieldTypeTag::Input,
            );
            let _ = parser.string(&opts, "name", None);

            let errors = parser.finish(&opts);

            assert!(errors.is_empty());
        }

        #[test]
        fn returns_accumulated_type_mismatch_errors() {
            let opts =
                options(&[("min", FieldValue::String("bad".to_owned()))]);
            let addr = address();
            let mut parser = SchemaFieldParser::new(
                addr.as_ref(),
                SchemaFieldTypeTag::Input,
            );
            let _ = parser.f64(&opts, "min", None);

            let errors = parser.finish(&opts);

            assert_eq!(errors.len(), 1);
            assert!(matches!(
                errors.first().expect("expected error"),
                SchemaFieldParserError::TypeMismatch { .. }
            ));
        }

        #[test]
        fn returns_unknown_key_error_for_each_unclaimed_key() {
            let opts = options(&[
                ("a", FieldValue::Int(1)),
                ("b", FieldValue::Int(2)),
                ("c", FieldValue::Int(3)),
            ]);
            let addr = address();
            let parser = SchemaFieldParser::new(
                addr.as_ref(),
                SchemaFieldTypeTag::Input,
            );

            let errors = parser.finish(&opts);

            assert_eq!(errors.len(), 3);
            assert!(errors.iter().all(|e| matches!(
                e,
                SchemaFieldParserError::UnknownKey { .. }
            )));
        }

        #[test]
        fn returns_single_unknown_key_error_for_one_unclaimed_key() {
            let opts = options(&[("only", FieldValue::Int(1))]);
            let addr = address();
            let parser = SchemaFieldParser::new(
                addr.as_ref(),
                SchemaFieldTypeTag::Input,
            );

            let errors = parser.finish(&opts);

            assert_eq!(errors.len(), 1);
            assert!(matches!(
                errors.first().expect("expected error"),
                SchemaFieldParserError::UnknownKey { key, .. } if key == "only"
            ));
        }

        #[test]
        fn combines_accumulated_errors_with_unknown_key_errors() {
            let opts = options(&[
                ("min", FieldValue::String("bad".to_owned())),
                ("bogus", FieldValue::Int(1)),
            ]);
            let addr = address();
            let mut parser = SchemaFieldParser::new(
                addr.as_ref(),
                SchemaFieldTypeTag::Input,
            );
            let _ = parser.f64(&opts, "min", None);

            let errors = parser.finish(&opts);

            assert_eq!(errors.len(), 2);
            let has_type_mismatch = errors.iter().any(|e| {
                matches!(e, SchemaFieldParserError::TypeMismatch { .. })
            });
            let has_unknown_key = errors.iter().any(|e| {
                matches!(e, SchemaFieldParserError::UnknownKey { .. })
            });
            assert!(has_type_mismatch, "expected a TypeMismatch error");
            assert!(has_unknown_key, "expected an UnknownKey error");
        }
    }

    mod claimed_keys {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn claimed_keys_are_not_reported_as_unknown() {
            let opts = options(&[("a", FieldValue::String("x".to_owned()))]);
            let addr = address();
            let mut parser = SchemaFieldParser::new(
                addr.as_ref(),
                SchemaFieldTypeTag::Input,
            );
            let _ = parser.string(&opts, "a", None);

            let errors = parser.finish(&opts);

            assert!(errors.is_empty());
        }

        #[test]
        fn multiple_extractors_each_claim_their_own_key() {
            let opts = options(&[
                ("name", FieldValue::String("alice".to_owned())),
                ("min", FieldValue::Int(0)),
                (
                    "tags",
                    FieldValue::List(vec![FieldValue::String("x".to_owned())]),
                ),
            ]);
            let addr = address();
            let mut parser = SchemaFieldParser::new(
                addr.as_ref(),
                SchemaFieldTypeTag::Input,
            );
            let _ = parser.string(&opts, "name", None);
            let _ = parser.f64(&opts, "min", None);
            let _ = parser.string_list(&opts, "tags", Vec::new());

            let errors = parser.finish(&opts);

            assert!(errors.is_empty());
        }

        #[test]
        fn unclaimed_keys_become_unknown_errors_even_when_others_are_claimed() {
            let opts = options(&[
                ("valid", FieldValue::String("ok".to_owned())),
                ("extra", FieldValue::Int(99)),
            ]);
            let addr = address();
            let mut parser = SchemaFieldParser::new(
                addr.as_ref(),
                SchemaFieldTypeTag::Input,
            );
            let _ = parser.string(&opts, "valid", None);

            let errors = parser.finish(&opts);

            assert_eq!(errors.len(), 1);
            assert!(matches!(
                errors.first().expect("expected error"),
                SchemaFieldParserError::UnknownKey { key, .. } if key == "extra"
            ));
        }
    }

    mod raw_value {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn returns_the_raw_unparsed_value_for_a_present_key() {
            let opts = options(&[("name", FieldValue::Int(5))]);
            let addr = address();
            let mut parser = SchemaFieldParser::new(
                addr.as_ref(),
                SchemaFieldTypeTag::Input,
            );

            let result = parser.raw_value(&opts, "name");

            assert_eq!(result, Some(&FieldValue::Int(5)));
        }

        #[test]
        fn returns_none_for_an_absent_key() {
            let opts = IndexMap::new();
            let addr = address();
            let mut parser = SchemaFieldParser::new(
                addr.as_ref(),
                SchemaFieldTypeTag::Input,
            );

            assert_eq!(parser.raw_value(&opts, "missing"), None);
        }

        #[test]
        fn claims_the_key_so_finish_does_not_flag_it_as_unknown() {
            let opts = options(&[("name", FieldValue::Int(5))]);
            let addr = address();
            let mut parser = SchemaFieldParser::new(
                addr.as_ref(),
                SchemaFieldTypeTag::Input,
            );
            let _ = parser.raw_value(&opts, "name");

            assert!(parser.finish(&opts).is_empty());
        }
    }

    mod error_tracking {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn mark_errors_starts_at_zero() {
            let addr = address();
            let parser = SchemaFieldParser::new(
                addr.as_ref(),
                SchemaFieldTypeTag::Input,
            );

            assert_eq!(parser.mark_errors(), 0);
        }

        #[test]
        fn has_errors_since_is_false_when_nothing_was_pushed_after_the_mark() {
            let addr = address();
            let mut parser = SchemaFieldParser::new(
                addr.as_ref(),
                SchemaFieldTypeTag::Input,
            );
            parser.push_unknown_key("first");
            let mark = parser.mark_errors();

            assert!(!parser.has_errors_since(mark));
        }

        #[test]
        fn has_errors_since_is_true_after_a_push_following_the_mark() {
            let addr = address();
            let mut parser = SchemaFieldParser::new(
                addr.as_ref(),
                SchemaFieldTypeTag::Input,
            );
            let mark = parser.mark_errors();
            parser.push_unknown_key("late");

            assert!(parser.has_errors_since(mark));
        }
    }

    mod error_pushes {
        use super::*;

        #[test]
        fn push_unknown_key_records_the_key_and_field_tag() {
            let addr = address();
            let mut parser = SchemaFieldParser::new(
                addr.as_ref(),
                SchemaFieldTypeTag::Input,
            );
            parser.push_unknown_key("bogus");

            let errors = parser.finish(&IndexMap::new());
            assert!(matches!(
                errors.first().expect("expected error"),
                SchemaFieldParserError::UnknownKey { key, .. }
                    if key == "bogus"
            ));
        }

        #[test]
        fn push_type_mismatch_records_the_expected_shape() {
            let addr = address();
            let mut parser = SchemaFieldParser::new(
                addr.as_ref(),
                SchemaFieldTypeTag::Input,
            );
            parser.push_type_mismatch("min", &"abc", "a number");

            let errors = parser.finish(&IndexMap::new());
            assert!(matches!(
                errors.first().expect("expected error"),
                SchemaFieldParserError::TypeMismatch { expected, .. }
                    if *expected == "a number"
            ));
        }

        #[test]
        fn push_number_constraint_records_the_key_and_expected_shape() {
            let addr = address();
            let mut parser = SchemaFieldParser::new(
                addr.as_ref(),
                SchemaFieldTypeTag::Number,
            );
            parser.push_number_constraint("step", 0.0, "positive");

            let errors = parser.finish(&IndexMap::new());
            assert!(matches!(
                errors.first().expect("expected error"),
                SchemaFieldParserError::NumberConstraint { key, expected, .. }
                    if key == "step" && *expected == "positive"
            ));
        }

        #[test]
        fn push_number_range_records_min_and_max() {
            let addr = address();
            let mut parser = SchemaFieldParser::new(
                addr.as_ref(),
                SchemaFieldTypeTag::Number,
            );
            parser.push_number_range(10.0, 1.0);

            let errors = parser.finish(&IndexMap::new());
            assert!(matches!(
                errors.first().expect("expected error"),
                SchemaFieldParserError::NumberRange { min, max, .. }
                    if min == "10" && max == "1"
            ));
        }

        #[test]
        fn push_select_value_error_wraps_the_source_error() {
            let addr = address();
            let mut parser = SchemaFieldParser::new(
                addr.as_ref(),
                SchemaFieldTypeTag::Select,
            );
            parser.push_select_value_error(
                SchemaSelectFieldValueError::MissingPath,
            );

            let errors = parser.finish(&IndexMap::new());
            assert!(matches!(
                errors.first().expect("expected error"),
                SchemaFieldParserError::SelectValue {
                    source: SchemaSelectFieldValueError::MissingPath,
                    ..
                }
            ));
        }
    }
}
