//! Build resolved Field Definitions from raw Schema TOML and `$ref` bases.
//!
//! [`SchemaFieldType`] absorbs what used to be a separate `FieldType` tag plus
//! `FieldOptions`: no field type without its own options can exist, and no
//! separate kind-only type shadows [`RawSchemaFieldType`], which already serves
//! that role at both the wire layer and here.
//!
//! [`SchemaFieldType::try_parse`] is the one seam that resolves a raw field's
//! type-specific `options` bag (a [`std::collections::BTreeMap<String,
//! FieldValue>`](crate::field::FieldValue)) into a [`SchemaFieldType`],
//! validating that every declared key belongs to the field's resolved type and
//! every declared value is shaped correctly for that key. The same validation
//! backs two severities: [`super::builder::SchemaFieldBuilder::build`]
//! hard-fails a `Direct` field or a `$ref` with a local `type` override
//! ([`SchemaFieldBuilderError::UnknownAttributeKey`]/
//! [`SchemaFieldBuilderError::AttributeValueTypeMismatch`]), while a bare
//! `$ref` override (no local `type` override) degrades the same failure to a
//! warning ([`SchemaWarning::UnknownOverrideKey`]/
//! [`SchemaWarning::OverrideValueTypeMismatch`]), drops the offending key, and
//! keeps every other valid key.

mod address;
pub(crate) use address::{FieldAddress, FieldAddressRef};

mod builder;
mod error;
pub(crate) use builder::{RefResolver, SchemaFieldBuilder};

mod date;
mod file;
mod number;
mod select;

use std::collections::BTreeMap;

use error::AttributeError;

use super::raw::RawSchemaFieldType;
use crate::field::FieldValue;

/// Store one resolved field definition.
///
/// `required` and `multi` are currently inert; reserved for future LSP/MCP
/// guardrails.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SchemaFieldDef {
    kind: SchemaFieldType,
    required: bool,
    multi: bool,
}

impl SchemaFieldDef {
    /// Build a resolved field definition from already-merged parts.
    fn new(kind: SchemaFieldType, required: bool, multi: bool) -> Self {
        Self {
            kind,
            required,
            multi,
        }
    }

    /// Build a resolved field definition directly, for tests outside this
    /// module that need a [`SchemaFieldDef`] without going through
    /// [`SchemaFieldBuilder`].
    #[cfg(test)]
    #[must_use]
    pub(super) fn for_test(
        kind: SchemaFieldType,
        required: bool,
        multi: bool,
    ) -> Self {
        Self::new(kind, required, multi)
    }

    /// Return this field's type-specific effective type.
    #[inline]
    #[must_use]
    pub(crate) fn kind(&self) -> &SchemaFieldType {
        &self.kind
    }

    /// Return this field's static selectable entries for the `schema`
    /// minijinja namespace's `.field()` method, or `None` if this field type
    /// has none to offer without consulting the file index.
    ///
    /// Only `select` fields have entries here; `file` resolves live from the
    /// `FileIndex` (see [`Self::file_filter`]), and every other type is not
    /// list-bearing.
    #[inline]
    #[must_use]
    pub(crate) fn select_values(&self) -> Option<&[SchemaSelectFieldEntry]> {
        match &self.kind {
            SchemaFieldType::Select {
                values,
            } => Some(values),
            SchemaFieldType::Input
            | SchemaFieldType::Boolean
            | SchemaFieldType::Number {
                ..
            }
            | SchemaFieldType::Date {
                ..
            }
            | SchemaFieldType::File {
                ..
            } => None,
        }
    }

    /// Return this file field's `FileIndex` filter parts, or `None` for every
    /// non-`file` field type.
    #[inline]
    #[must_use]
    pub(crate) fn file_filter(&self) -> Option<SchemaFileFieldFilter<'_>> {
        match &self.kind {
            SchemaFieldType::File {
                folders,
                ext,
                class,
            } => Some(SchemaFileFieldFilter {
                folders,
                ext: ext.as_deref(),
                class,
            }),
            SchemaFieldType::Input
            | SchemaFieldType::Select {
                ..
            }
            | SchemaFieldType::Boolean
            | SchemaFieldType::Number {
                ..
            }
            | SchemaFieldType::Date {
                ..
            } => None,
        }
    }

    /// Return `true` if this field must be set. Always `false` on the reserved
    /// Global Schema, regardless of its own TOML.
    #[inline]
    #[must_use]
    pub(crate) fn is_required(&self) -> bool {
        self.required
    }

    /// Return `true` if this field accepts multiple values.
    #[inline]
    #[must_use]
    pub(crate) fn is_multi(&self) -> bool {
        self.multi
    }
}

/// Borrow a `file` field's `FileIndex` filter parts.
pub(crate) struct SchemaFileFieldFilter<'a> {
    pub(crate) folders: &'a [String],
    pub(crate) ext: Option<&'a str>,
    pub(crate) class: &'a [String],
}

/// Represent a field's effective type and type-specific options.
///
/// Pairs each kind with the options only that kind carries: a `select` field
/// without `values`, or a `date` field with a stray `folders` list, cannot be
/// represented. `select` and `file` are the only list-bearing kinds; `number`
/// carries `step`/`min`/`max` and `date` a `format`; the rest are unit
/// variants. Replaces a separate `FieldType` tag: [`RawSchemaFieldType`]
/// already names every kind at the wire layer, so [`Self::raw_kind`] returns
/// that instead of a second, schema-domain-only tag type.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum SchemaFieldType {
    /// Accept free-form text input.
    Input,
    /// Accept one value from a configured list.
    Select {
        values: Vec<SchemaSelectFieldEntry>,
    },
    /// Accept a boolean value.
    Boolean,
    /// Accept a numeric value.
    Number {
        /// Inclusive minimum; `None` when unset.
        min: Option<f64>,
        /// Inclusive maximum; `None` when unset.
        max: Option<f64>,
        /// Increment step for the numeric value; `None` when unset.
        step: Option<f64>,
    },
    /// Accept a date value.
    Date {
        /// Display/parse format (strftime); `None` when unset.
        format: Option<String>,
    },
    /// Accept a link to files matched by folder, extension, and class filters.
    ///
    /// `class` stays the declared string list, unmatched against is-a
    /// expansion here: [`super::SchemaService::matches`] applies is-a
    /// expansion live, at render/query time, same as every other class
    /// filter in this crate.
    File {
        folders: Vec<String>,
        ext: Option<String>,
        class: Vec<String>,
    },
}

impl SchemaFieldType {
    /// Return the [`RawSchemaFieldType`] this variant represents.
    #[inline]
    #[must_use]
    fn raw_kind(&self) -> RawSchemaFieldType {
        match self {
            Self::Input => RawSchemaFieldType::Input,
            Self::Select {
                ..
            } => RawSchemaFieldType::Select,
            Self::Boolean => RawSchemaFieldType::Boolean,
            Self::Number {
                ..
            } => RawSchemaFieldType::Number,
            Self::Date {
                ..
            } => RawSchemaFieldType::Date,
            Self::File {
                ..
            } => RawSchemaFieldType::File,
        }
    }

    /// Parses every key in `options` for a field of `kind`, falling back to
    /// `base`'s options for any key `options` leaves unset, and returns the
    /// resulting effective type alongside every per-key validation failure.
    ///
    /// `base` is only consulted when it is `Some` of the same
    /// [`RawSchemaFieldType`] kind; a `$ref` that switches type, or a field
    /// with no base at all, starts from empty options instead of reusing a
    /// mismatched base. For example, a `select`'s `values` never leaks into an
    /// overriding `file` field.
    ///
    /// `Input`/`Boolean` have no type-specific keys at all: every key in
    /// `options` is unrecognized for them, so each becomes its own
    /// [`AttributeError::UnknownKey`] rather than routing through a dedicated
    /// (empty) own-declaration struct.
    pub(super) fn try_parse(
        address: address::FieldAddressRef<'_>,
        kind: RawSchemaFieldType,
        options: &BTreeMap<String, FieldValue>,
        base: Option<&SchemaFieldType>,
    ) -> (SchemaFieldType, Vec<AttributeError>) {
        match kind {
            RawSchemaFieldType::Input => (
                SchemaFieldType::Input,
                options
                    .keys()
                    .map(|key| error::unknown_key(address, kind, key))
                    .collect(),
            ),
            RawSchemaFieldType::Boolean => (
                SchemaFieldType::Boolean,
                options
                    .keys()
                    .map(|key| error::unknown_key(address, kind, key))
                    .collect(),
            ),
            RawSchemaFieldType::Select => {
                select::SchemaSelectFieldDef::parse(address, options, base)
            }
            RawSchemaFieldType::Number => {
                number::SchemaNumberFieldDef::parse(address, options, base)
            }
            RawSchemaFieldType::Date => {
                date::SchemaDateFieldDef::parse(address, options, base)
            }
            RawSchemaFieldType::File => {
                file::SchemaFileFieldDef::parse(address, options, base)
            }
        }
    }
}

/// One selectable entry a `select`/`multi` field's `values` resolves to.
///
/// No memory of source: literal today (every entry built by
/// [`SchemaSelectFieldEntry::literal`]); an inline object or values-file
/// entry once ticket 08 lands. `template/engine/schema.rs` renders an
/// entry as a plain string when `label == value` and `extra` is empty
/// (always true under this ticket), else as `{value, label, ...extra}`.
pub(crate) use select::SchemaSelectFieldEntry;

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;
    use crate::schema::name::SchemaNameRef;

    /// Parses `reference` into a [`FieldAddress`], panicking on an invalid
    /// test fixture.
    fn field_address(reference: &str) -> FieldAddress {
        FieldAddress::try_from(reference).expect("valid test $ref")
    }

    /// Builds a resolved [`Schema`] named `name` with one field named `field`
    /// with the given `kind`, keyed by `name` for a `resolved` map.
    fn schema_with_field(
        name: &str,
        field: &str,
        kind: SchemaFieldType,
    ) -> (crate::schema::name::SchemaName, crate::schema::model::Schema) {
        let mut fields = BTreeMap::new();
        fields.insert(
            crate::field::FieldName::try_from(field)
                .expect("valid test field name"),
            SchemaFieldDef::new(kind, false, false),
        );
        (
            crate::schema::name::SchemaName::from(name),
            crate::schema::model::Schema::new(
                crate::schema::name::SchemaName::from(name),
                fields,
                BTreeSet::new(),
            ),
        )
    }

    mod schema_field_type {
        mod kind {
            use pretty_assertions::assert_eq;
            use rstest::rstest;

            use super::super::super::*;

            #[rstest]
            #[case::input(SchemaFieldType::Input, RawSchemaFieldType::Input)]
            #[case::select(
                SchemaFieldType::Select { values: Vec::new() },
                RawSchemaFieldType::Select
            )]
            #[case::boolean(
                SchemaFieldType::Boolean,
                RawSchemaFieldType::Boolean
            )]
            #[case::number(
                SchemaFieldType::Number { min: None, max: None, step: None },
                RawSchemaFieldType::Number
            )]
            #[case::date(
                SchemaFieldType::Date { format: None },
                RawSchemaFieldType::Date
            )]
            #[case::file(
                SchemaFieldType::File {
                    folders: Vec::new(),
                    ext: None,
                    class: Vec::new(),
                },
                RawSchemaFieldType::File
            )]
            fn returns_the_raw_field_type_matching_the_variant(
                #[case] field_type: SchemaFieldType,
                #[case] expected: RawSchemaFieldType,
            ) {
                assert_eq!(field_type.raw_kind(), expected);
            }
        }
    }

    mod try_parse {
        mod without_base {
            use pretty_assertions::assert_eq;

            use super::super::super::*;

            fn address() -> FieldAddress {
                FieldAddress::try_from("#book/field").expect("valid ref")
            }

            fn options(
                pairs: &[(&str, FieldValue)],
            ) -> BTreeMap<String, FieldValue> {
                pairs
                    .iter()
                    .map(|(k, v)| ((*k).to_owned(), v.clone()))
                    .collect()
            }

            #[test]
            fn select_collects_declared_values_as_literal_entries() {
                let opts = options(&[(
                    "values",
                    FieldValue::List(vec![
                        FieldValue::String("draft".to_owned()),
                        FieldValue::String("done".to_owned()),
                    ]),
                )]);

                let (field_type, errors) = SchemaFieldType::try_parse(
                    address().as_ref(),
                    RawSchemaFieldType::Select,
                    &opts,
                    None,
                );

                assert!(errors.is_empty());
                let SchemaFieldType::Select {
                    values,
                } = field_type
                else {
                    panic!("expected Select");
                };
                assert_eq!(values.len(), 2);
                assert_eq!(
                    values[0].value(),
                    &FieldValue::String("draft".to_owned())
                );
                assert_eq!(
                    values[0].label(),
                    &FieldValue::String("draft".to_owned())
                );
                assert!(values[0].extra().is_empty());
            }

            #[test]
            fn select_defaults_to_empty_values_when_options_omit_them() {
                let opts = options(&[]);

                let (field_type, errors) = SchemaFieldType::try_parse(
                    address().as_ref(),
                    RawSchemaFieldType::Select,
                    &opts,
                    None,
                );

                assert!(errors.is_empty());
                assert_eq!(field_type, SchemaFieldType::Select {
                    values: Vec::new()
                });
            }

            #[test]
            fn select_with_a_non_list_values_key_is_a_type_mismatch() {
                let opts = options(&[(
                    "values",
                    FieldValue::String("draft".to_owned()),
                )]);

                let (_, errors) = SchemaFieldType::try_parse(
                    address().as_ref(),
                    RawSchemaFieldType::Select,
                    &opts,
                    None,
                );

                assert_eq!(errors.len(), 1);
                assert!(matches!(
                    errors[0],
                    AttributeError::TypeMismatch { .. }
                ));
            }

            #[test]
            fn date_declaring_values_is_an_unknown_key() {
                let opts = options(&[(
                    "values",
                    FieldValue::List(vec![FieldValue::String("x".to_owned())]),
                )]);

                let (_, errors) = SchemaFieldType::try_parse(
                    address().as_ref(),
                    RawSchemaFieldType::Date,
                    &opts,
                    None,
                );

                assert_eq!(errors.len(), 1);
                assert!(matches!(errors[0], AttributeError::UnknownKey { .. }));
            }

            #[test]
            fn number_with_a_string_min_is_a_type_mismatch() {
                let opts =
                    options(&[("min", FieldValue::String("abc".to_owned()))]);

                let (_, errors) = SchemaFieldType::try_parse(
                    address().as_ref(),
                    RawSchemaFieldType::Number,
                    &opts,
                    None,
                );

                assert_eq!(errors.len(), 1);
                assert!(matches!(
                    errors[0],
                    AttributeError::TypeMismatch { .. }
                ));
            }

            #[test]
            fn number_accepts_an_integer_min_as_a_float() {
                let opts = options(&[("min", FieldValue::Int(0))]);

                let (field_type, errors) = SchemaFieldType::try_parse(
                    address().as_ref(),
                    RawSchemaFieldType::Number,
                    &opts,
                    None,
                );

                assert!(errors.is_empty());
                assert_eq!(field_type, SchemaFieldType::Number {
                    min: Some(0.0),
                    max: None,
                    step: None,
                });
            }

            #[test]
            fn file_collects_folders_ext_and_class() {
                let opts = options(&[
                    (
                        "folders",
                        FieldValue::List(vec![FieldValue::String(
                            "assets".to_owned(),
                        )]),
                    ),
                    ("ext", FieldValue::String("png".to_owned())),
                    (
                        "class",
                        FieldValue::List(vec![FieldValue::String(
                            "image".to_owned(),
                        )]),
                    ),
                ]);

                let (field_type, errors) = SchemaFieldType::try_parse(
                    address().as_ref(),
                    RawSchemaFieldType::File,
                    &opts,
                    None,
                );

                assert!(errors.is_empty());
                assert_eq!(field_type, SchemaFieldType::File {
                    folders: vec!["assets".to_owned()],
                    ext: Some("png".to_owned()),
                    class: vec!["image".to_owned()],
                });
            }

            #[test]
            fn input_declaring_any_key_is_an_unknown_key() {
                let opts = options(&[("min", FieldValue::Int(1))]);

                let (field_type, errors) = SchemaFieldType::try_parse(
                    address().as_ref(),
                    RawSchemaFieldType::Input,
                    &opts,
                    None,
                );

                assert_eq!(field_type, SchemaFieldType::Input);
                assert_eq!(errors.len(), 1);
                assert!(matches!(errors[0], AttributeError::UnknownKey { .. }));
            }

            #[test]
            fn boolean_declaring_any_key_is_an_unknown_key() {
                let opts =
                    options(&[("ext", FieldValue::String("x".to_owned()))]);

                let (field_type, errors) = SchemaFieldType::try_parse(
                    address().as_ref(),
                    RawSchemaFieldType::Boolean,
                    &opts,
                    None,
                );

                assert_eq!(field_type, SchemaFieldType::Boolean);
                assert_eq!(errors.len(), 1);
                assert!(matches!(errors[0], AttributeError::UnknownKey { .. }));
            }
        }

        mod with_base {
            use pretty_assertions::assert_eq;

            use super::super::super::*;

            fn address() -> FieldAddress {
                FieldAddress::try_from("#sci_fi/field").expect("valid ref")
            }

            #[test]
            fn select_falls_back_to_bases_values_when_options_omit_them() {
                let base = SchemaFieldType::Select {
                    values: vec![SchemaSelectFieldEntry::literal(
                        "old".to_owned(),
                    )],
                };

                let (field_type, errors) = SchemaFieldType::try_parse(
                    address().as_ref(),
                    RawSchemaFieldType::Select,
                    &BTreeMap::new(),
                    Some(&base),
                );

                assert!(errors.is_empty());
                assert_eq!(field_type, base);
            }

            #[test]
            fn select_ignores_a_mismatched_base_type() {
                let base = SchemaFieldType::Input;

                let (field_type, errors) = SchemaFieldType::try_parse(
                    address().as_ref(),
                    RawSchemaFieldType::Select,
                    &BTreeMap::new(),
                    Some(&base),
                );

                assert!(errors.is_empty());
                assert_eq!(field_type, SchemaFieldType::Select {
                    values: Vec::new()
                });
            }

            #[test]
            fn file_falls_back_independently_per_subfield() {
                let base = SchemaFieldType::File {
                    folders: vec!["base-folder".to_owned()],
                    ext: Some("base-ext".to_owned()),
                    class: vec!["base-class".to_owned()],
                };
                let mut options = BTreeMap::new();
                options.insert(
                    "folders".to_owned(),
                    FieldValue::List(vec![FieldValue::String(
                        "raw-folder".to_owned(),
                    )]),
                );

                let (field_type, errors) = SchemaFieldType::try_parse(
                    address().as_ref(),
                    RawSchemaFieldType::File,
                    &options,
                    Some(&base),
                );

                assert!(errors.is_empty());
                assert_eq!(field_type, SchemaFieldType::File {
                    folders: vec!["raw-folder".to_owned()],
                    ext: Some("base-ext".to_owned()),
                    class: vec!["base-class".to_owned()],
                });
            }
        }
    }
}
