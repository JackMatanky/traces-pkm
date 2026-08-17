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
//! - [`builder`] resolves `$ref` targets and builds [`SchemaFieldDef`]s.

mod address;
pub(crate) use address::{FieldAddress, FieldAddressRef};

mod builder;
pub(crate) use builder::{RefAddressResolver, SchemaFieldBuilder};

mod date;
pub(crate) use date::SchemaDateField;
mod file;
pub(crate) use file::{SchemaFileField, SchemaFileFieldRef};
mod number;
pub(crate) use number::SchemaNumberField;
mod parser;
mod select;

use std::collections::BTreeMap;

pub(crate) use select::{SchemaSelectField, SchemaSelectFieldEntry};

use super::error::SchemaFieldParserError;
use crate::field::FieldValue;

/// A resolved field definition after inheritance and `$ref` application.
///
/// Carries the effective type, whether the field is required, and whether it
/// accepts multiple values.
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
    ///
    /// Use [`select_values`] and [`file_filter`] for type-specific accessors.
    ///
    /// [`select_values`]: Self::select_values
    /// [`file_filter`]: Self::file_filter
    #[inline]
    #[must_use]
    pub(super) fn kind(&self) -> &SchemaFieldType {
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
            SchemaFieldType::File(def) => Some(def.as_ref()),
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
    /// One value from a configured list of [`SchemaSelectFieldEntry`]s.
    Select(SchemaSelectField),
    /// Boolean value.
    Boolean,
    /// Numeric value with optional bounds and step.
    Number(SchemaNumberField),
    /// Date value with an optional display format.
    Date(SchemaDateField),
    /// A link to files matched by folder, extension, and class filters.
    ///
    /// Class matching happens at query time via
    /// [`super::service::SchemaService::matches`], not here.
    File(SchemaFileField),
}

impl std::fmt::Display for SchemaFieldType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Input => "input",
            Self::Select(_) => "select",
            Self::Boolean => "boolean",
            Self::Number(_) => "number",
            Self::Date(_) => "date",
            Self::File(_) => "file",
        })
    }
}

impl SchemaFieldType {
    /// Return the inner [`SchemaSelectField`] if this is a
    /// [`Select`][Self::Select] variant.
    pub(super) fn as_select(&self) -> Option<&SchemaSelectField> {
        match self {
            Self::Select(inner) => Some(inner),
            _ => None,
        }
    }

    /// Return the inner [`SchemaNumberField`] if this is a
    /// [`Number`][Self::Number] variant.
    pub(super) fn as_number(&self) -> Option<&SchemaNumberField> {
        match self {
            Self::Number(inner) => Some(inner),
            _ => None,
        }
    }

    /// Return the inner [`SchemaDateField`] if this is a [`Date`][Self::Date]
    /// variant.
    pub(super) fn as_date(&self) -> Option<&SchemaDateField> {
        match self {
            Self::Date(inner) => Some(inner),
            _ => None,
        }
    }

    /// Return the inner [`SchemaFileField`] if this is a [`File`][Self::File]
    /// variant.
    pub(super) fn as_file(&self) -> Option<&SchemaFileField> {
        match self {
            Self::File(inner) => Some(inner),
            _ => None,
        }
    }
}

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

    mod select_parse {
        use pretty_assertions::assert_eq;

        use super::super::*;

        fn address() -> FieldAddress {
            FieldAddress::try_from("#book/field").expect("valid ref")
        }

        fn options(
            pairs: &[(&str, FieldValue)],
        ) -> BTreeMap<String, FieldValue> {
            pairs.iter().map(|(k, v)| ((*k).to_owned(), v.clone())).collect()
        }

        #[test]
        fn collects_declared_values_as_literal_entries() {
            let opts = options(&[(
                "values",
                FieldValue::List(vec![
                    FieldValue::String("draft".to_owned()),
                    FieldValue::String("done".to_owned()),
                ]),
            )]);

            let addr = address();
            let mut parser = parser::SchemaFieldParser::new(
                addr.as_ref(),
                SchemaFieldType::Select(SchemaSelectField::default()),
            );
            let field_type =
                select::SchemaSelectField::parse(&mut parser, &opts, None);
            let errors = parser.finish(&opts);

            assert!(errors.is_empty());
            let SchemaFieldType::Select(def) = field_type else {
                panic!("expected Select");
            };
            assert_eq!(def.values().len(), 2);
            assert_eq!(
                def.values()[0].value(),
                &FieldValue::String("draft".to_owned())
            );
            assert_eq!(
                def.values()[0].label(),
                &FieldValue::String("draft".to_owned())
            );
            assert!(def.values()[0].extra().is_empty());
        }

        #[test]
        fn defaults_to_empty_values_when_options_omit_them() {
            let opts = options(&[]);

            let addr = address();
            let mut parser = parser::SchemaFieldParser::new(
                addr.as_ref(),
                SchemaFieldType::Select(SchemaSelectField::default()),
            );
            let field_type =
                select::SchemaSelectField::parse(&mut parser, &opts, None);
            let errors = parser.finish(&opts);

            assert!(errors.is_empty());
            assert_eq!(
                field_type,
                SchemaFieldType::Select(SchemaSelectField::default())
            );
        }

        #[test]
        fn a_non_list_values_key_is_a_type_mismatch() {
            let opts =
                options(&[("values", FieldValue::String("draft".to_owned()))]);

            let addr = address();
            let mut parser = parser::SchemaFieldParser::new(
                addr.as_ref(),
                SchemaFieldType::Select(SchemaSelectField::default()),
            );
            let _ = select::SchemaSelectField::parse(&mut parser, &opts, None);
            let errors = parser.finish(&opts);

            assert_eq!(errors.len(), 1);
            assert!(matches!(
                errors[0],
                SchemaFieldParserError::TypeMismatch { .. }
            ));
        }

        #[test]
        fn falls_back_to_bases_values_when_options_omit_them() {
            let base =
                SchemaFieldType::Select(SchemaSelectField::for_test(vec![
                    SchemaSelectFieldEntry::literal("old".to_owned()),
                ]));

            let addr = address();
            let mut parser = parser::SchemaFieldParser::new(
                addr.as_ref(),
                SchemaFieldType::Select(SchemaSelectField::default()),
            );
            let field_type = select::SchemaSelectField::parse(
                &mut parser,
                &BTreeMap::new(),
                Some(&base),
            );
            let errors = parser.finish(&BTreeMap::new());

            assert!(errors.is_empty());
            assert_eq!(field_type, base);
        }

        #[test]
        fn ignores_a_mismatched_base_type() {
            let base = SchemaFieldType::Input;

            let addr = address();
            let mut parser = parser::SchemaFieldParser::new(
                addr.as_ref(),
                SchemaFieldType::Select(SchemaSelectField::default()),
            );
            let field_type = select::SchemaSelectField::parse(
                &mut parser,
                &BTreeMap::new(),
                Some(&base),
            );
            let errors = parser.finish(&BTreeMap::new());

            assert!(errors.is_empty());
            assert_eq!(
                field_type,
                SchemaFieldType::Select(SchemaSelectField::default())
            );
        }
    }

    mod number_parse {
        use pretty_assertions::assert_eq;

        use super::super::*;

        fn address() -> FieldAddress {
            FieldAddress::try_from("#book/field").expect("valid ref")
        }

        fn options(
            pairs: &[(&str, FieldValue)],
        ) -> BTreeMap<String, FieldValue> {
            pairs.iter().map(|(k, v)| ((*k).to_owned(), v.clone())).collect()
        }

        #[test]
        fn a_string_min_is_a_type_mismatch() {
            let opts =
                options(&[("min", FieldValue::String("abc".to_owned()))]);

            let addr = address();
            let mut parser = parser::SchemaFieldParser::new(
                addr.as_ref(),
                SchemaFieldType::Number(SchemaNumberField::default()),
            );
            let _ = number::SchemaNumberField::parse(&mut parser, &opts, None);
            let errors = parser.finish(&opts);

            assert_eq!(errors.len(), 1);
            assert!(matches!(
                errors[0],
                SchemaFieldParserError::TypeMismatch { .. }
            ));
        }

        #[test]
        fn accepts_an_integer_min_as_a_float() {
            let opts = options(&[("min", FieldValue::Int(0))]);

            let addr = address();
            let mut parser = parser::SchemaFieldParser::new(
                addr.as_ref(),
                SchemaFieldType::Number(SchemaNumberField::default()),
            );
            let field_type =
                number::SchemaNumberField::parse(&mut parser, &opts, None);
            let errors = parser.finish(&opts);

            assert!(errors.is_empty());
            assert_eq!(
                field_type,
                SchemaFieldType::Number(SchemaNumberField::for_test(
                    Some(0.0),
                    None,
                    None
                ))
            );
        }

        #[test]
        fn an_unknown_key_is_rejected() {
            let opts = options(&[("values", FieldValue::Int(1))]);

            let addr = address();
            let mut parser = parser::SchemaFieldParser::new(
                addr.as_ref(),
                SchemaFieldType::Number(SchemaNumberField::default()),
            );
            let _ = number::SchemaNumberField::parse(&mut parser, &opts, None);
            let errors = parser.finish(&opts);

            assert_eq!(errors.len(), 1);
            assert!(matches!(
                errors[0],
                SchemaFieldParserError::UnknownKey { .. }
            ));
        }
    }

    mod date_parse {
        use super::super::*;

        fn address() -> FieldAddress {
            FieldAddress::try_from("#book/field").expect("valid ref")
        }

        fn options(
            pairs: &[(&str, FieldValue)],
        ) -> BTreeMap<String, FieldValue> {
            pairs.iter().map(|(k, v)| ((*k).to_owned(), v.clone())).collect()
        }

        #[test]
        fn declaring_values_is_an_unknown_key() {
            let opts = options(&[(
                "values",
                FieldValue::List(vec![FieldValue::String("x".to_owned())]),
            )]);

            let addr = address();
            let mut parser = parser::SchemaFieldParser::new(
                addr.as_ref(),
                SchemaFieldType::Date(SchemaDateField::default()),
            );
            let _ = date::SchemaDateField::parse(&mut parser, &opts, None);
            let errors = parser.finish(&opts);

            assert_eq!(errors.len(), 1);
            assert!(matches!(
                errors[0],
                SchemaFieldParserError::UnknownKey { .. }
            ));
        }
    }

    mod file_parse {
        use pretty_assertions::assert_eq;

        use super::super::*;

        fn address() -> FieldAddress {
            FieldAddress::try_from("#book/field").expect("valid ref")
        }

        fn options(
            pairs: &[(&str, FieldValue)],
        ) -> BTreeMap<String, FieldValue> {
            pairs.iter().map(|(k, v)| ((*k).to_owned(), v.clone())).collect()
        }

        #[test]
        fn collects_folders_ext_and_class() {
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

            let addr = address();
            let mut parser = parser::SchemaFieldParser::new(
                addr.as_ref(),
                SchemaFieldType::File(SchemaFileField::default()),
            );
            let field_type =
                file::SchemaFileField::parse(&mut parser, &opts, None);
            let errors = parser.finish(&opts);

            assert!(errors.is_empty());
            assert_eq!(
                field_type,
                SchemaFieldType::File(SchemaFileField::for_test(
                    vec!["assets".to_owned()],
                    Some("png".to_owned()),
                    vec!["image".to_owned()],
                ))
            );
        }

        #[test]
        fn falls_back_independently_per_subfield() {
            let base = SchemaFieldType::File(SchemaFileField::for_test(
                vec!["base-folder".to_owned()],
                Some("base-ext".to_owned()),
                vec!["base-class".to_owned()],
            ));
            let mut options = BTreeMap::new();
            options.insert(
                "folders".to_owned(),
                FieldValue::List(vec![FieldValue::String(
                    "raw-folder".to_owned(),
                )]),
            );

            let addr = address();
            let mut parser = parser::SchemaFieldParser::new(
                addr.as_ref(),
                SchemaFieldType::File(SchemaFileField::default()),
            );
            let field_type = file::SchemaFileField::parse(
                &mut parser,
                &options,
                Some(&base),
            );
            let errors = parser.finish(&options);

            assert!(errors.is_empty());
            assert_eq!(
                field_type,
                SchemaFieldType::File(SchemaFileField::for_test(
                    vec!["raw-folder".to_owned()],
                    Some("base-ext".to_owned()),
                    vec!["base-class".to_owned()],
                ))
            );
        }
    }

    mod simple_parse {
        use super::super::*;

        fn address() -> FieldAddress {
            FieldAddress::try_from("#book/field").expect("valid ref")
        }

        fn options(
            pairs: &[(&str, FieldValue)],
        ) -> BTreeMap<String, FieldValue> {
            pairs.iter().map(|(k, v)| ((*k).to_owned(), v.clone())).collect()
        }
        #[test]
        fn input_declaring_any_key_is_an_unknown_key() {
            let opts = options(&[("min", FieldValue::Int(1))]);

            let addr = address();
            let p = parser::SchemaFieldParser::new(
                addr.as_ref(),
                SchemaFieldType::Input,
            );
            let unknowns = p.finish(&opts);

            assert_eq!(unknowns.len(), 1);
            assert!(matches!(
                unknowns[0],
                SchemaFieldParserError::UnknownKey { .. }
            ));
        }

        #[test]
        fn boolean_declaring_any_key_is_an_unknown_key() {
            let opts = options(&[("ext", FieldValue::String("x".to_owned()))]);

            let addr = address();
            let p = parser::SchemaFieldParser::new(
                addr.as_ref(),
                SchemaFieldType::Boolean,
            );
            let unknowns = p.finish(&opts);

            assert_eq!(unknowns.len(), 1);
            assert!(matches!(
                unknowns[0],
                SchemaFieldParserError::UnknownKey { .. }
            ));
        }
    }
}
