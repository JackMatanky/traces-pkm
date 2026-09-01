//! Resolved field definitions, type-specific options, and `$ref` building.
//!
//! Each raw field's `type` and `options` bag are validated and merged into a
//! [`SchemaFieldType`] by the per-type `parse` functions in submodules.
//!
//! Two severities back the same validation:
//! - **Hard failure** for `Direct` fields and `$ref` with a `type` override.
//! - **Degraded warning** for bare `$ref` overrides (offending key dropped,
//!   every other valid key kept).
//!
//! Submodules partition the per-type parsing:
//!
//! - [`parser`] provides [`SchemaFieldParser`](parser::SchemaFieldParser) for
//!   shared attribute-table validation.
//! - [`select`] parses `values` into [`SchemaSelectFieldEntry`]s.
//! - [`number`] parses `min`/`max`/`step`.
//! - [`date`] parses `format`.
//! - [`mod@file`] parses `folders`/`ext`/`class` and provides
//!   [`SchemaFileFieldRef`] for `FileIndex` queries.

use std::sync::Arc;

use super::RawSchemaFieldType;

mod address;
mod builder;
mod date;
mod error;
mod file;
mod number;
mod parser;
mod select;

pub(crate) use address::{FieldAddress, FieldAddressRef};
pub(crate) use builder::{SchemaFieldBuildContext, SchemaFieldBuilder};
pub(crate) use date::SchemaDateField;
pub(crate) use error::SchemaFieldBuilderError;
// Needed by schema::error's tests (which can't reach the private
// fields::error module directly).
#[cfg(test)]
pub(crate) use error::SchemaFieldParserError;
pub(crate) use file::{SchemaFileField, SchemaFileFieldRef};
pub(crate) use number::SchemaNumberField;
pub(crate) use select::{SchemaSelectField, SchemaSelectFieldEntry};

/// A resolved field definition after inheritance and `$ref` application.
///
/// Carries the effective type, whether the field is required, and whether it
/// accepts multiple values.
#[derive(Clone, Debug, PartialEq)]
pub struct SchemaFieldDef {
    kind: SchemaFieldType,
    required: bool,
    multi: bool,
}

impl SchemaFieldDef {
    const fn new(kind: SchemaFieldType, required: bool, multi: bool) -> Self {
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
    pub(super) const fn for_test(
        kind: SchemaFieldType,
        required: bool,
        multi: bool,
    ) -> Self {
        Self::new(kind, required, multi)
    }

    /// Return this field's type.
    ///
    /// Use [`select_values`] and [`file_filter`] for type-specific accessors.
    ///
    /// [`select_values`]: Self::select_values
    /// [`file_filter`]: Self::file_filter
    #[inline]
    #[must_use]
    pub(super) const fn kind(&self) -> &SchemaFieldType {
        &self.kind
    }

    /// Return the static selectable entries for a `select` or `multi` field.
    ///
    /// Returns `None` for every other field type.
    #[inline]
    #[must_use]
    pub(crate) fn select_values(&self) -> Option<&[SchemaSelectFieldEntry]> {
        match &self.kind {
            SchemaFieldType::Select(def) => Some(def.values()),
            SchemaFieldType::Input
            | SchemaFieldType::Boolean
            | SchemaFieldType::Number(_)
            | SchemaFieldType::Date(_)
            | SchemaFieldType::File(_) => None,
        }
    }

    /// Return the borrowed file filter for a `file` field.
    ///
    /// Returns `None` for every other field type.
    #[inline]
    #[must_use]
    pub(crate) fn file_filter(&self) -> Option<file::SchemaFileFieldRef<'_>> {
        match &self.kind {
            SchemaFieldType::File(def) => Some(SchemaFileField::as_ref(def)),
            SchemaFieldType::Input
            | SchemaFieldType::Select(_)
            | SchemaFieldType::Boolean
            | SchemaFieldType::Number(_)
            | SchemaFieldType::Date(_) => None,
        }
    }

    /// Return `true` if this field must be set.
    #[inline]
    #[must_use]
    pub(crate) const fn is_required(&self) -> bool {
        self.required
    }

    /// Return `true` if this field accepts multiple values.
    #[inline]
    #[must_use]
    pub(crate) const fn is_multi(&self) -> bool {
        self.multi
    }
}

/// A field's effective type and its type-specific options.
///
/// Each variant wraps the resolved options for that kind:
/// - [`Select`][SchemaFieldType::Select]: [`SchemaSelectField`]
/// - [`Number`][SchemaFieldType::Number]: [`SchemaNumberField`]
/// - [`Date`][SchemaFieldType::Date]: [`SchemaDateField`]
/// - [`File`][SchemaFieldType::File]: [`SchemaFileField`]
/// - [`Input`][SchemaFieldType::Input] / [`Boolean`][SchemaFieldType::Boolean]:
///   no options
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum SchemaFieldType {
    /// Free-form text input.
    Input,
    /// Boolean value.
    Boolean,
    /// Numeric value with optional bounds ([`SchemaNumberField::min`],
    /// [`SchemaNumberField::max`]) and step ([`SchemaNumberField::step`]).
    Number(SchemaNumberField),
    /// Date value with an optional display format
    /// ([`SchemaDateField::format`]).
    Date(SchemaDateField),
    /// A link to files matched by folder, extension, and class filters.
    ///
    /// Class matching happens at query time, not here.
    File(Arc<SchemaFileField>),
    /// One value from a configured list of [`SchemaSelectFieldEntry`]s.
    Select(Arc<SchemaSelectField>),
}

impl std::fmt::Display for SchemaFieldType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.kind().fmt(f)
    }
}

impl SchemaFieldType {
    /// Return the inner [`SchemaSelectField`] if this is a
    /// [`Select`][Self::Select] variant.
    pub(super) fn as_select(&self) -> Option<&SchemaSelectField> {
        match self {
            Self::Select(inner) => Some(inner.as_ref()),
            _ => None,
        }
    }

    /// Return the inner [`SchemaNumberField`] if this is a
    /// [`Number`][Self::Number] variant.
    pub(super) const fn as_number(&self) -> Option<&SchemaNumberField> {
        match self {
            Self::Number(inner) => Some(inner),
            _ => None,
        }
    }

    /// Return the inner [`SchemaDateField`] if this is a [`Date`][Self::Date]
    /// variant.
    pub(super) const fn as_date(&self) -> Option<&SchemaDateField> {
        match self {
            Self::Date(inner) => Some(inner),
            _ => None,
        }
    }

    /// Return the inner [`SchemaFileField`] if this is a [`File`][Self::File]
    /// variant.
    pub(super) fn as_file(&self) -> Option<&SchemaFileField> {
        match self {
            Self::File(inner) => Some(inner.as_ref()),
            _ => None,
        }
    }

    /// Return this type's tag: which of the six variants it is, without its
    /// payload.
    #[inline]
    #[must_use]
    pub(super) const fn kind(&self) -> SchemaFieldTypeTag {
        match self {
            Self::Input => SchemaFieldTypeTag::Input,
            Self::Select(_) => SchemaFieldTypeTag::Select,
            Self::Boolean => SchemaFieldTypeTag::Boolean,
            Self::Number(_) => SchemaFieldTypeTag::Number,
            Self::Date(_) => SchemaFieldTypeTag::Date,
            Self::File(_) => SchemaFieldTypeTag::File,
        }
    }
}

/// Which of the six field kinds a [`SchemaFieldType`] or
/// [`RawSchemaFieldType`][super::RawSchemaFieldType] is, without either's
/// payload.
///
/// Constructible from a [`RawSchemaFieldType`][super::RawSchemaFieldType] (via
/// [`From`]) or projected from an already-resolved [`SchemaFieldType`] (via
/// [`SchemaFieldType::kind`]), never the reverse. Used by
/// [`SchemaFieldParserError`], [`SchemaWarning`], and
/// [`parser::SchemaFieldParser`] to name a field's kind in diagnostics without
/// carrying its resolved options.
///
/// [`SchemaFieldParserError`]: super::error::SchemaFieldParserError
/// [`SchemaWarning`]: super::error::SchemaWarning
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum SchemaFieldTypeTag {
    Input,
    Select,
    Boolean,
    Number,
    Date,
    File,
}

impl From<RawSchemaFieldType> for SchemaFieldTypeTag {
    fn from(raw: RawSchemaFieldType) -> Self {
        match raw {
            RawSchemaFieldType::Input => Self::Input,
            RawSchemaFieldType::Select => Self::Select,
            RawSchemaFieldType::Boolean => Self::Boolean,
            RawSchemaFieldType::Number => Self::Number,
            RawSchemaFieldType::Date => Self::Date,
            RawSchemaFieldType::File => Self::File,
        }
    }
}

impl std::fmt::Display for SchemaFieldTypeTag {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Input => "input",
            Self::Select => "select",
            Self::Boolean => "boolean",
            Self::Number => "number",
            Self::Date => "date",
            Self::File => "file",
        })
    }
}

#[cfg(test)]
mod tests {

    mod field_def {
        use super::super::*;

        #[test]
        fn kind_returns_the_field_type() {
            let def = SchemaFieldDef::new(SchemaFieldType::Input, false, false);
            assert_eq!(def.kind(), &SchemaFieldType::Input);
        }

        #[test]
        fn select_values_returns_entries_for_select_type() {
            let entries =
                vec![SchemaSelectFieldEntry::literal("draft".to_owned())];
            let def = SchemaFieldDef::new(
                SchemaFieldType::Select(Arc::new(SchemaSelectField::for_test(
                    entries,
                ))),
                false,
                false,
            );
            assert_eq!(def.select_values().unwrap().len(), 1);
        }

        #[test]
        fn select_values_returns_none_for_non_select_type() {
            let def = SchemaFieldDef::new(SchemaFieldType::Input, false, false);
            assert!(def.select_values().is_none());
        }

        #[test]
        fn file_filter_returns_some_for_file_type() {
            let def = SchemaFieldDef::new(
                SchemaFieldType::File(Arc::new(SchemaFileField::default())),
                false,
                false,
            );
            assert!(def.file_filter().is_some());
        }

        #[test]
        fn file_filter_returns_none_for_non_file_type() {
            let def = SchemaFieldDef::new(SchemaFieldType::Input, false, false);
            assert!(def.file_filter().is_none());
        }

        #[test]
        fn is_required_reflects_construction_value() {
            let required =
                SchemaFieldDef::new(SchemaFieldType::Input, true, false);
            assert!(required.is_required());
            let not_required =
                SchemaFieldDef::new(SchemaFieldType::Input, false, false);
            assert!(!not_required.is_required());
        }

        #[test]
        fn is_multi_reflects_construction_value() {
            let multi =
                SchemaFieldDef::new(SchemaFieldType::Input, false, true);
            assert!(multi.is_multi());
            let not_multi =
                SchemaFieldDef::new(SchemaFieldType::Input, false, false);
            assert!(!not_multi.is_multi());
        }
    }

    mod field_type_display {
        use super::super::*;

        #[test]
        fn display_returns_lowercase_type_name() {
            let cases = vec![
                (SchemaFieldType::Input, "input"),
                (
                    SchemaFieldType::Select(Arc::new(
                        SchemaSelectField::default(),
                    )),
                    "select",
                ),
                (SchemaFieldType::Boolean, "boolean"),
                (
                    SchemaFieldType::Number(SchemaNumberField::default()),
                    "number",
                ),
                (SchemaFieldType::Date(SchemaDateField::default()), "date"),
                (
                    SchemaFieldType::File(Arc::new(SchemaFileField::default())),
                    "file",
                ),
            ];
            for (kind, expected) in cases {
                assert_eq!(kind.to_string(), expected, "Display for {kind:?}");
            }
        }
    }

    mod field_type_accessors {
        use super::super::*;

        #[test]
        fn as_select_returns_some_for_select_variant() {
            let kind =
                SchemaFieldType::Select(Arc::new(SchemaSelectField::default()));
            assert!(kind.as_select().is_some());
        }

        #[test]
        fn as_select_returns_none_for_non_select_variant() {
            let kind = SchemaFieldType::Input;
            assert!(kind.as_select().is_none());
        }

        #[test]
        fn as_number_returns_some_for_number_variant() {
            let kind = SchemaFieldType::Number(SchemaNumberField::default());
            assert!(kind.as_number().is_some());
        }

        #[test]
        fn as_number_returns_none_for_non_number_variant() {
            let kind = SchemaFieldType::Input;
            assert!(kind.as_number().is_none());
        }

        #[test]
        fn as_date_returns_some_for_date_variant() {
            let kind = SchemaFieldType::Date(SchemaDateField::default());
            assert!(kind.as_date().is_some());
        }

        #[test]
        fn as_date_returns_none_for_non_date_variant() {
            let kind = SchemaFieldType::Input;
            assert!(kind.as_date().is_none());
        }

        #[test]
        fn as_file_returns_some_for_file_variant() {
            let kind =
                SchemaFieldType::File(Arc::new(SchemaFileField::default()));
            assert!(kind.as_file().is_some());
        }

        #[test]
        fn as_file_returns_none_for_non_file_variant() {
            let kind = SchemaFieldType::Input;
            assert!(kind.as_file().is_none());
        }
    }

    mod validation {
        use indexmap::IndexMap;
        use rstest::rstest;

        use super::super::{error::SchemaFieldParserError, *};
        use crate::FieldValue;

        fn address() -> FieldAddress {
            FieldAddress::try_from("#book/field").expect("valid ref")
        }

        fn options(
            pairs: &[(&str, FieldValue)],
        ) -> IndexMap<String, FieldValue> {
            pairs.iter().map(|(k, v)| ((*k).to_owned(), v.clone())).collect()
        }
        #[rstest]
        #[case::input(SchemaFieldTypeTag::Input)]
        #[case::boolean(SchemaFieldTypeTag::Boolean)]
        fn type_with_no_extractor_treats_all_keys_as_unknown(
            #[case] kind: SchemaFieldTypeTag,
        ) {
            let opts = options(&[("min", FieldValue::Int(1))]);

            let addr = address();
            let p = parser::SchemaFieldParser::new(addr.as_ref(), kind);
            let unknowns = p.finish(&opts);

            assert_eq!(unknowns.len(), 1);
            assert!(matches!(
                unknowns.first().expect("expected error"),
                SchemaFieldParserError::UnknownKey { .. }
            ));
        }
    }
}
