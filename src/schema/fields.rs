//! Resolved field definitions, type-specific options, and `$ref` building.
//!
//! Each raw field's `type` and `options` bag are validated and merged into a
//! [`SchemaFieldType`] by [`SchemaFieldType::try_parse`]. The same validation
//! backs two severities: a hard failure for `Direct` fields and `$ref` fields
//! with a local `type` override, or a degraded warning for bare `$ref`
//! overrides that drops the offending key and keeps every other valid key.
//!
//! Submodules partition the per-type parsing:
//!
//! - [`select`] parses `values` into [`SchemaSelectFieldEntry`]s.
//! - [`number`] parses `min`/`max`/`step`.
//! - [`date`] parses `format`.
//! - [`mod@file`] parses `folders`/`ext`/`class` and provides
//!   [`SchemaFileFieldDefRef`] for `FileIndex` queries.
//! - [`builder`] resolves `$ref` targets and builds [`SchemaFieldDef`]s.

mod address;
pub(crate) use address::{FieldAddress, FieldAddressRef};

mod builder;
mod error;
pub(crate) use builder::{RefResolver, SchemaFieldBuilder};

mod date;
mod file;
pub(crate) use file::SchemaFileFieldDefRef;
mod number;
mod select;

use std::collections::BTreeMap;

use error::AttributeError;

use super::raw::RawSchemaFieldType;
use crate::field::FieldValue;

/// One resolved field definition after inheritance and `$ref` application.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SchemaFieldDef {
    kind: SchemaFieldType,
    required: bool,
    multi: bool,
}

impl SchemaFieldDef {
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

    /// Return this field's effective [`SchemaFieldType`].
    #[inline]
    #[must_use]
    pub(crate) fn kind(&self) -> &SchemaFieldType {
        &self.kind
    }

    /// Return the static selectable entries for a `select` or `multi` field,
    /// or `None` for every other field type.
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

    /// Return the [`file::SchemaFileFieldDefRef`] for a `file` field, or
    /// `None` for every other field type.
    #[inline]
    #[must_use]
    pub(crate) fn file_filter(
        &self,
    ) -> Option<file::SchemaFileFieldDefRef<'_>> {
        match &self.kind {
            SchemaFieldType::File {
                folders,
                ext,
                class,
            } => Some(file::SchemaFileFieldDefRef {
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

    /// Return `true` if this field must be set.
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

/// A field's effective type and its type-specific options.
///
/// Each variant carries only the options relevant to that kind: `select`
/// carries `values`, `number` carries `min`/`max`/`step`, `date` carries
/// `format`, and `file` carries `folders`/`ext`/`class`. [`Input`] and
/// [`Boolean`] are unit variants with no options.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum SchemaFieldType {
    /// Free-form text input.
    Input,
    /// One value from a configured list.
    Select {
        values: Vec<SchemaSelectFieldEntry>,
    },
    /// Boolean value.
    Boolean,
    /// Numeric value with optional bounds and step.
    Number {
        /// Inclusive minimum bound.
        min: Option<f64>,
        /// Inclusive maximum bound.
        max: Option<f64>,
        /// Increment step.
        step: Option<f64>,
    },
    /// Date value with an optional display format.
    Date {
        /// Display/parse format (strftime).
        format: Option<String>,
    },
    /// A link to files matched by folder, extension, and class filters.
    ///
    /// Class matching happens at query time via
    /// [`super::SchemaService::matches`], not here.
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

    /// Parse `options` for a field of `kind`, falling back to `base` for any
    /// key `options` leaves unset. Returns the effective type and every per-key
    /// validation failure.
    ///
    /// `base` is only consulted when it matches `kind`. A `$ref` that switches
    /// type starts from empty options. `Input`/`Boolean` have no type-specific
    /// keys, so every key in `options` becomes an
    /// [`AttributeError::UnknownKey`].
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

/// One selectable entry a `select`/`multi` field resolves its `values` to.
///
/// Rendered as a plain string when `label == value` and `extra` is empty,
/// otherwise as `{value, label, ...extra}`.
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
