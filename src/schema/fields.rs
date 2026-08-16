//! Build resolved Field Definitions from raw Schema TOML and `$ref` bases.
//!
//! [`SchemaFieldType`] absorbs what used to be a separate `FieldType` tag plus
//! `FieldOptions`: no field type without its own options can exist, and no
//! separate kind-only type shadows [`RawFieldType`], which already serves that
//! role at both the wire layer and here.
//!
//! [`SchemaFieldBuilder`] is the one seam that resolves a raw field's
//! type-specific `options` bag (a [`std::collections::BTreeMap<String,
//! FieldValue>`](crate::field::FieldValue)) into a [`SchemaFieldType`],
//! validating that every declared key belongs to the field's resolved type and
//! every declared value is shaped correctly for that key. The same validation
//! ([`parse_field_type`]) backs two severities: [`SchemaFieldBuilder::build`]
//! hard-fails a `Direct` field or a `$ref` with a local `type` override
//! ([`SchemaFieldBuilderError::UnknownAttributeKey`]/
//! [`SchemaFieldBuilderError::AttributeValueTypeMismatch`]), while a bare
//! `$ref` override (no local `type` override) degrades the same failure to a
//! warning ([`SchemaWarning::UnknownOverrideKey`]/
//! [`SchemaWarning::OverrideValueTypeMismatch`]), drops the offending key, and
//! keeps every other valid key.

use std::collections::{BTreeMap, BTreeSet};

use super::{
    GLOBAL_SCHEMA_NAME,
    address::{FieldAddress, FieldAddressRef},
    error::{SchemaError, SchemaFieldBuilderError, SchemaWarning},
    model::Schema,
    name::SchemaName,
    raw::{RawFieldSource, RawFieldType, RawSchemaFieldDef},
};
use crate::field::FieldValue;

/// Store one resolved field definition.
///
/// `required` and `multi` are currently inert; reserved for future LSP/MCP
/// guardrails.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SchemaFieldDef {
    field_type: SchemaFieldType,
    required: bool,
    multi: bool,
}

impl SchemaFieldDef {
    /// Build a resolved field definition from already-merged parts.
    fn new(field_type: SchemaFieldType, required: bool, multi: bool) -> Self {
        Self {
            field_type,
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
        field_type: SchemaFieldType,
        required: bool,
        multi: bool,
    ) -> Self {
        Self::new(field_type, required, multi)
    }

    /// Return this field's type-specific effective type.
    #[inline]
    #[must_use]
    pub(crate) fn field_type(&self) -> &SchemaFieldType {
        &self.field_type
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
        match &self.field_type {
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

    /// Return this file field's `FileIndex` filter parts, or `None` for
    /// every non-`file` field type.
    #[inline]
    #[must_use]
    pub(crate) fn file_filter(&self) -> Option<SchemaFileFieldFilter<'_>> {
        match &self.field_type {
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
/// variants. Replaces a separate `FieldType` tag: [`RawFieldType`] already
/// names every kind at the wire layer, so [`Self::kind`] returns that instead
/// of a second, schema-domain-only tag type.
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
    /// Return the [`RawFieldType`] this variant represents.
    #[inline]
    #[must_use]
    fn kind(&self) -> RawFieldType {
        match self {
            Self::Input => RawFieldType::Input,
            Self::Select {
                ..
            } => RawFieldType::Select,
            Self::Boolean => RawFieldType::Boolean,
            Self::Number {
                ..
            } => RawFieldType::Number,
            Self::Date {
                ..
            } => RawFieldType::Date,
            Self::File {
                ..
            } => RawFieldType::File,
        }
    }
}

/// One selectable entry a `select`/`multi` field's `values` resolves to.
///
/// No memory of source: literal today (every entry built by
/// [`SchemaSelectFieldEntry::literal`]); an inline object or values-file entry
/// once ticket 08 lands. `template/engine/schema.rs` renders an entry as a
/// plain string when `label == value` and `extra` is empty (always true under
/// this ticket), else as `{value, label, ...extra}`.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SchemaSelectFieldEntry {
    value: FieldValue,
    label: FieldValue,
    extra: BTreeMap<String, FieldValue>,
}

impl SchemaSelectFieldEntry {
    /// Builds a flat entry from a plain declared string: `label` defaults to
    /// `value`, `extra` is empty. The only shape a literal `values = [...]`
    /// array produces.
    pub(super) fn literal(value: String) -> Self {
        Self {
            value: FieldValue::String(value.clone()),
            label: FieldValue::String(value),
            extra: BTreeMap::new(),
        }
    }

    /// Return this entry's value.
    #[inline]
    #[must_use]
    pub(crate) fn value(&self) -> &FieldValue {
        &self.value
    }

    /// Return this entry's display label.
    #[inline]
    #[must_use]
    pub(crate) fn label(&self) -> &FieldValue {
        &self.label
    }

    /// Return this entry's passthrough keys beyond `value`/`label`.
    #[inline]
    #[must_use]
    pub(crate) fn extra(&self) -> &BTreeMap<String, FieldValue> {
        &self.extra
    }
}

/// One field-attribute key/value validation failure from parsing a field
/// type's `options` bag: either the key doesn't belong to the field's
/// resolved type, or its value isn't shaped like the key expects.
///
/// Converts into a hard [`SchemaFieldBuilderError`] for a `Direct`/`$ref`
/// `type`-override field ([`SchemaFieldBuilder::build`]'s strict path), or a
/// soft [`SchemaWarning`] for a bare `$ref` override (its lenient path) — see
/// the module docs.
enum AttributeError {
    UnknownKey {
        address: FieldAddress,
        kind: RawFieldType,
        key: String,
    },
    TypeMismatch {
        address: FieldAddress,
        kind: RawFieldType,
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
fn unknown_key(
    address: FieldAddressRef<'_>,
    kind: RawFieldType,
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
fn type_mismatch(
    address: FieldAddressRef<'_>,
    kind: RawFieldType,
    key: &str,
    value: &FieldValue,
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

/// Returns `value` as an owned list of strings, or `None` if it isn't a list
/// of nothing but strings.
fn expect_string_list(value: &FieldValue) -> Option<Vec<String>> {
    let FieldValue::List(items) = value else {
        return None;
    };
    items
        .iter()
        .map(|item| match item {
            FieldValue::String(s) => Some(s.clone()),
            _ => None,
        })
        .collect()
}

/// Returns `value` as an owned string, or `None` if it isn't one.
fn expect_string(value: &FieldValue) -> Option<String> {
    match value {
        FieldValue::String(s) => Some(s.clone()),
        _ => None,
    }
}

/// Own declaration of a `select` field's type-specific options, one level
/// more `Option` than [`SchemaFieldType::Select`]: not yet merged with an
/// inherited `$ref` base.
#[derive(Default)]
struct SchemaSelectFieldDef {
    values: Option<Vec<SchemaSelectFieldEntry>>,
}

impl SchemaSelectFieldDef {
    /// Parses every key in `options` against `select`'s one valid attribute
    /// (`values`), returning the best-effort parsed definition alongside every
    /// per-key failure encountered — an unrecognized key or a wrongly-shaped
    /// value does not stop parsing the rest.
    fn parse(
        address: FieldAddressRef<'_>,
        options: &BTreeMap<String, FieldValue>,
    ) -> (Self, Vec<AttributeError>) {
        let mut def = Self::default();
        let mut errors = Vec::new();
        for (key, value) in options {
            match key.as_str() {
                "values" => match expect_string_list(value) {
                    Some(values) => {
                        def.values = Some(
                            values
                                .into_iter()
                                .map(SchemaSelectFieldEntry::literal)
                                .collect(),
                        );
                    }
                    None => errors.push(type_mismatch(
                        address,
                        RawFieldType::Select,
                        key,
                        value,
                        "an array of strings",
                    )),
                },
                _ => {
                    errors.push(unknown_key(
                        address,
                        RawFieldType::Select,
                        key,
                    ));
                }
            }
        }
        (def, errors)
    }
}

/// Own declaration of a `file` field's type-specific options. See
/// [`SchemaSelectFieldDef`]'s docs for why this is one level more `Option`
/// than [`SchemaFieldType::File`].
#[derive(Default)]
struct SchemaFileFieldDef {
    folders: Option<Vec<String>>,
    ext: Option<String>,
    class: Option<Vec<String>>,
}

impl SchemaFileFieldDef {
    fn parse(
        address: FieldAddressRef<'_>,
        options: &BTreeMap<String, FieldValue>,
    ) -> (Self, Vec<AttributeError>) {
        let mut def = Self::default();
        let mut errors = Vec::new();
        for (key, value) in options {
            match key.as_str() {
                "folders" => match expect_string_list(value) {
                    Some(folders) => def.folders = Some(folders),
                    None => errors.push(type_mismatch(
                        address,
                        RawFieldType::File,
                        key,
                        value,
                        "an array of strings",
                    )),
                },
                "ext" => match expect_string(value) {
                    Some(ext) => def.ext = Some(ext),
                    None => errors.push(type_mismatch(
                        address,
                        RawFieldType::File,
                        key,
                        value,
                        "a string",
                    )),
                },
                "class" => match expect_string_list(value) {
                    Some(class) => def.class = Some(class),
                    None => errors.push(type_mismatch(
                        address,
                        RawFieldType::File,
                        key,
                        value,
                        "an array of strings",
                    )),
                },
                _ => {
                    errors.push(unknown_key(address, RawFieldType::File, key));
                }
            }
        }
        (def, errors)
    }
}

/// Own declaration of a `number` field's type-specific options. See
/// [`SchemaSelectFieldDef`]'s docs for why this is one level more `Option`
/// than [`SchemaFieldType::Number`].
#[derive(Default)]
struct SchemaNumberFieldDef {
    min: Option<f64>,
    max: Option<f64>,
    step: Option<f64>,
}

impl SchemaNumberFieldDef {
    fn parse(
        address: FieldAddressRef<'_>,
        options: &BTreeMap<String, FieldValue>,
    ) -> (Self, Vec<AttributeError>) {
        let mut def = Self::default();
        let mut errors = Vec::new();
        for (key, value) in options {
            let slot = match key.as_str() {
                "min" => &mut def.min,
                "max" => &mut def.max,
                "step" => &mut def.step,
                _ => {
                    errors.push(unknown_key(
                        address,
                        RawFieldType::Number,
                        key,
                    ));
                    continue;
                }
            };
            match value.as_f64() {
                Some(number) => *slot = Some(number),
                None => errors.push(type_mismatch(
                    address,
                    RawFieldType::Number,
                    key,
                    value,
                    "a number",
                )),
            }
        }
        (def, errors)
    }
}

/// Own declaration of a `date` field's type-specific options. See
/// [`SchemaSelectFieldDef`]'s docs for why this is one level more `Option`
/// than [`SchemaFieldType::Date`].
#[derive(Default)]
struct SchemaDateFieldDef {
    format: Option<String>,
}

impl SchemaDateFieldDef {
    fn parse(
        address: FieldAddressRef<'_>,
        options: &BTreeMap<String, FieldValue>,
    ) -> (Self, Vec<AttributeError>) {
        let mut def = Self::default();
        let mut errors = Vec::new();
        for (key, value) in options {
            match key.as_str() {
                "format" => match expect_string(value) {
                    Some(format) => def.format = Some(format),
                    None => errors.push(type_mismatch(
                        address,
                        RawFieldType::Date,
                        key,
                        value,
                        "a string",
                    )),
                },
                _ => {
                    errors.push(unknown_key(address, RawFieldType::Date, key));
                }
            }
        }
        (def, errors)
    }
}

/// Parses every key in `options` for a field of `kind`, falling back to
/// `base`'s options for any key `options` leaves unset, and returns the
/// resulting effective type alongside every per-key validation failure.
///
/// `base` is only consulted when it is `Some` of the same [`RawFieldType`]
/// kind; a `$ref` that switches type, or a field with no base at all, starts
/// from empty options instead of reusing a mismatched base. For example, a
/// `select`'s `values` never leaks into an overriding `file` field.
///
/// `Input`/`Boolean` have no type-specific keys at all: every key in
/// `options` is unrecognized for them, so each becomes its own
/// [`AttributeError::UnknownKey`] rather than routing through a dedicated
/// (empty) own-declaration struct.
fn parse_field_type(
    address: FieldAddressRef<'_>,
    kind: RawFieldType,
    options: &BTreeMap<String, FieldValue>,
    base: Option<&SchemaFieldType>,
) -> (SchemaFieldType, Vec<AttributeError>) {
    match kind {
        RawFieldType::Input => (
            SchemaFieldType::Input,
            options.keys().map(|key| unknown_key(address, kind, key)).collect(),
        ),
        RawFieldType::Boolean => (
            SchemaFieldType::Boolean,
            options.keys().map(|key| unknown_key(address, kind, key)).collect(),
        ),
        RawFieldType::Select => {
            let (def, errors) = SchemaSelectFieldDef::parse(address, options);
            let values = def.values.unwrap_or_else(|| match base {
                Some(SchemaFieldType::Select {
                    values,
                }) => values.clone(),
                _ => Vec::new(),
            });
            (
                SchemaFieldType::Select {
                    values,
                },
                errors,
            )
        }
        RawFieldType::Number => {
            let (def, errors) = SchemaNumberFieldDef::parse(address, options);
            let (base_min, base_max, base_step) = match base {
                Some(SchemaFieldType::Number {
                    min,
                    max,
                    step,
                }) => (*min, *max, *step),
                _ => (None, None, None),
            };
            (
                SchemaFieldType::Number {
                    min: def.min.or(base_min),
                    max: def.max.or(base_max),
                    step: def.step.or(base_step),
                },
                errors,
            )
        }
        RawFieldType::Date => {
            let (def, errors) = SchemaDateFieldDef::parse(address, options);
            let format = def.format.or_else(|| match base {
                Some(SchemaFieldType::Date {
                    format,
                }) => format.clone(),
                _ => None,
            });
            (
                SchemaFieldType::Date {
                    format,
                },
                errors,
            )
        }
        RawFieldType::File => {
            let (def, errors) = SchemaFileFieldDef::parse(address, options);
            let folders = def.folders.unwrap_or_else(|| match base {
                Some(SchemaFieldType::File {
                    folders,
                    ..
                }) => folders.clone(),
                _ => Vec::new(),
            });
            let ext = def.ext.or_else(|| match base {
                Some(SchemaFieldType::File {
                    ext,
                    ..
                }) => ext.clone(),
                _ => None,
            });
            let class = def.class.unwrap_or_else(|| match base {
                Some(SchemaFieldType::File {
                    class,
                    ..
                }) => class.clone(),
                _ => Vec::new(),
            });
            (
                SchemaFieldType::File {
                    folders,
                    ext,
                    class,
                },
                errors,
            )
        }
    }
}

/// Resolve a Schema's `$ref` values to their base [`SchemaFieldDef`]s, bounded
/// to the Global Schema or the referencing Schema's transitive `extends`
/// ancestors.
///
/// `$ref`s point up the `extends` DAG or to the Global Schema, so they are
/// acyclic by construction.
pub(super) struct RefResolver<'a> {
    pub(super) ancestors: &'a BTreeSet<SchemaName>,
    pub(super) resolved: &'a BTreeMap<SchemaName, Schema>,
}

impl<'a> RefResolver<'a> {
    /// Resolve `base_address`, `address`'s own already-parsed `$ref` value, to
    /// its base [`SchemaFieldDef`].
    ///
    /// # Errors
    ///
    /// - [`SchemaError::RefOutOfBounds`] if the named Schema is neither the
    ///   Global Schema nor a transitive `extends` ancestor of
    ///   `address.schema()`.
    /// - [`SchemaError::RefFieldNotFound`] if the named Schema is in bounds but
    ///   has no such field.
    fn resolve(
        &self,
        address: FieldAddressRef<'_>,
        base_address: &FieldAddress,
    ) -> Result<&'a SchemaFieldDef, SchemaError> {
        // Rejecting an out-of-bounds target here, rather than only checking
        // whether it happens to be resolved yet, keeps the bound spec-accurate
        // and independent of Kahn's tie-breaking order among unrelated Schemas.
        if base_address.schema().as_str() != GLOBAL_SCHEMA_NAME
            && !self.ancestors.contains(base_address.schema().as_str())
        {
            return Err(SchemaFieldBuilderError::RefOutOfBounds {
                own: Box::new(FieldAddress::from(address)),
                reference: Box::new(base_address.clone()),
            }
            .into());
        }
        self.resolved
            .get(base_address.schema().as_str())
            .and_then(|schema| schema.field(base_address.field().as_str()))
            .ok_or_else(|| {
                SchemaFieldBuilderError::RefFieldNotFound {
                    own: Box::new(FieldAddress::from(address)),
                    reference: Box::new(base_address.clone()),
                }
                .into()
            })
    }
}

/// Force `required` to `false` and record a
/// [`SchemaWarning::StrayGlobalRequired`] when `address.schema()` is the
/// Global Schema and it declared `required = true`.
///
/// Global Schema fields can never be required.
fn apply_global_degrade(
    address: FieldAddressRef<'_>,
    required: bool,
    warnings: &mut Vec<SchemaWarning>,
) -> bool {
    if address.schema().as_str() == GLOBAL_SCHEMA_NAME && required {
        warnings.push(SchemaWarning::StrayGlobalRequired {
            field: address.field().as_str().to_owned(),
        });
        false
    } else {
        required
    }
}

/// Builds one resolved [`SchemaFieldDef`] from its raw declaration, resolving
/// a `$ref` (if any) against already-resolved Schemas.
pub(super) struct SchemaFieldBuilder<'a> {
    pub(super) refs: &'a RefResolver<'a>,
    pub(super) warnings: &'a mut Vec<SchemaWarning>,
}

impl SchemaFieldBuilder<'_> {
    /// Build `address`'s effective [`SchemaFieldDef`] from `raw`.
    ///
    /// - `Direct(kind)`: builds fresh from `raw.options`, no base to merge.
    /// - `Ref` with a local `type` override: builds fresh from `raw.options`
    ///   against the override kind, ignoring the base's own type-specific
    ///   options (see [`parse_field_type`]'s docs on type-switching refs).
    /// - Bare `Ref` (no override): resolves the base field via `refs`, then
    ///   merges `raw.options` on top of the base's own options at the base's
    ///   kind. An unrecognized key or wrongly-shaped value here degrades to a
    ///   [`SchemaWarning`] and is dropped rather than failing the build.
    ///
    /// # Errors
    ///
    /// - Any error [`RefResolver::resolve`] returns while resolving a `$ref`.
    /// - [`SchemaError::FieldBuilder`] if a `Direct` field or a `$ref` with a
    ///   local `type` override declares an unrecognized attribute key or a
    ///   wrongly-shaped attribute value.
    pub(super) fn build(
        &mut self,
        address: FieldAddressRef<'_>,
        raw: &RawSchemaFieldDef,
    ) -> Result<SchemaFieldDef, SchemaError> {
        let (field_type, required, multi) = match &raw.source {
            RawFieldSource::Ref {
                address: base_address,
                override_type: Some(override_type),
            } => {
                let base = self.refs.resolve(address, base_address)?;
                let (field_type, errors) = parse_field_type(
                    address,
                    *override_type,
                    &raw.options,
                    Some(base.field_type()),
                );
                if let Some(error) = errors.into_iter().next() {
                    return Err(SchemaFieldBuilderError::from(error).into());
                }
                (
                    field_type,
                    raw.required.unwrap_or(base.is_required()),
                    raw.multi.unwrap_or(base.is_multi()),
                )
            }
            RawFieldSource::Ref {
                address: base_address,
                override_type: None,
            } => {
                let base = self.refs.resolve(address, base_address)?;
                let (field_type, errors) = parse_field_type(
                    address,
                    base.field_type().kind(),
                    &raw.options,
                    Some(base.field_type()),
                );
                self.warnings.extend(errors.into_iter().map(Into::into));
                (
                    field_type,
                    raw.required.unwrap_or(base.is_required()),
                    raw.multi.unwrap_or(base.is_multi()),
                )
            }
            RawFieldSource::Direct(raw_type) => {
                let (field_type, errors) =
                    parse_field_type(address, *raw_type, &raw.options, None);
                if let Some(error) = errors.into_iter().next() {
                    return Err(SchemaFieldBuilderError::from(error).into());
                }
                (
                    field_type,
                    raw.required.unwrap_or(false),
                    raw.multi.unwrap_or(false),
                )
            }
        };

        Ok(SchemaFieldDef::new(
            field_type,
            apply_global_degrade(address, required, self.warnings),
            multi,
        ))
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
    /// with the given `field_type`, keyed by `name` for a `resolved` map.
    fn schema_with_field(
        name: &str,
        field: &str,
        field_type: SchemaFieldType,
    ) -> (SchemaName, Schema) {
        let mut fields = BTreeMap::new();
        fields.insert(
            crate::field::FieldName::try_from(field)
                .expect("valid test field name"),
            SchemaFieldDef::new(field_type, false, false),
        );
        (
            SchemaName::from(name),
            Schema::new(SchemaName::from(name), fields, BTreeSet::new()),
        )
    }

    mod schema_field_type {
        mod kind {
            use pretty_assertions::assert_eq;
            use rstest::rstest;

            use super::super::super::*;

            #[rstest]
            #[case::input(SchemaFieldType::Input, RawFieldType::Input)]
            #[case::select(
                SchemaFieldType::Select { values: Vec::new() },
                RawFieldType::Select
            )]
            #[case::boolean(SchemaFieldType::Boolean, RawFieldType::Boolean)]
            #[case::number(
                SchemaFieldType::Number { min: None, max: None, step: None },
                RawFieldType::Number
            )]
            #[case::date(
                SchemaFieldType::Date { format: None },
                RawFieldType::Date
            )]
            #[case::file(
                SchemaFieldType::File {
                    folders: Vec::new(),
                    ext: None,
                    class: Vec::new(),
                },
                RawFieldType::File
            )]
            fn returns_the_raw_field_type_matching_the_variant(
                #[case] field_type: SchemaFieldType,
                #[case] expected: RawFieldType,
            ) {
                assert_eq!(field_type.kind(), expected);
            }
        }
    }

    mod parse_field_type {
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

                let (field_type, errors) = parse_field_type(
                    address().as_ref(),
                    RawFieldType::Select,
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

                let (field_type, errors) = parse_field_type(
                    address().as_ref(),
                    RawFieldType::Select,
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

                let (_, errors) = parse_field_type(
                    address().as_ref(),
                    RawFieldType::Select,
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

                let (_, errors) = parse_field_type(
                    address().as_ref(),
                    RawFieldType::Date,
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

                let (_, errors) = parse_field_type(
                    address().as_ref(),
                    RawFieldType::Number,
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

                let (field_type, errors) = parse_field_type(
                    address().as_ref(),
                    RawFieldType::Number,
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

                let (field_type, errors) = parse_field_type(
                    address().as_ref(),
                    RawFieldType::File,
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

                let (field_type, errors) = parse_field_type(
                    address().as_ref(),
                    RawFieldType::Input,
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

                let (field_type, errors) = parse_field_type(
                    address().as_ref(),
                    RawFieldType::Boolean,
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

                let (field_type, errors) = parse_field_type(
                    address().as_ref(),
                    RawFieldType::Select,
                    &BTreeMap::new(),
                    Some(&base),
                );

                assert!(errors.is_empty());
                assert_eq!(field_type, base);
            }

            #[test]
            fn select_ignores_a_mismatched_base_type() {
                let base = SchemaFieldType::Input;

                let (field_type, errors) = parse_field_type(
                    address().as_ref(),
                    RawFieldType::Select,
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

                let (field_type, errors) = parse_field_type(
                    address().as_ref(),
                    RawFieldType::File,
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

    mod ref_resolver {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn resolves_a_field_from_an_ancestor_schema() {
            let (name, book) =
                schema_with_field("book", "status", SchemaFieldType::Input);
            let mut resolved = BTreeMap::new();
            resolved.insert(name.clone(), book);
            let mut ancestors = BTreeSet::new();
            ancestors.insert(name);
            let refs = RefResolver {
                ancestors: &ancestors,
                resolved: &resolved,
            };
            let address = FieldAddressRef::new(
                SchemaNameRef::from("sci_fi"),
                crate::field::FieldNameRef::try_from("status")
                    .expect("valid field name"),
            );

            let field = refs
                .resolve(address, &field_address("#book/status"))
                .expect("resolves");

            assert_eq!(field.field_type(), &SchemaFieldType::Input);
        }

        #[test]
        fn resolves_a_field_from_the_global_schema() {
            let (name, global) = schema_with_field(
                GLOBAL_SCHEMA_NAME,
                "priority",
                SchemaFieldType::Input,
            );
            let mut resolved = BTreeMap::new();
            resolved.insert(name, global);
            let ancestors = BTreeSet::new();
            let refs = RefResolver {
                ancestors: &ancestors,
                resolved: &resolved,
            };
            let address = FieldAddressRef::new(
                SchemaNameRef::from("task"),
                crate::field::FieldNameRef::try_from("priority")
                    .expect("valid field name"),
            );

            let field = refs
                .resolve(address, &field_address("#global/priority"))
                .expect("resolves");

            assert_eq!(field.field_type(), &SchemaFieldType::Input);
        }

        #[test]
        fn rejects_a_reference_outside_the_bound() {
            let (name, movie) =
                schema_with_field("movie", "status", SchemaFieldType::Input);
            let mut resolved = BTreeMap::new();
            resolved.insert(name, movie);
            let ancestors = BTreeSet::new();
            let refs = RefResolver {
                ancestors: &ancestors,
                resolved: &resolved,
            };
            let address = FieldAddressRef::new(
                SchemaNameRef::from("book"),
                crate::field::FieldNameRef::try_from("status")
                    .expect("valid field name"),
            );

            let err = refs
                .resolve(address, &field_address("#movie/status"))
                .expect_err("out-of-bounds rejected");

            assert!(matches!(
                err,
                SchemaError::FieldBuilder(inner)
                    if matches!(*inner, SchemaFieldBuilderError::RefOutOfBounds { .. })
            ));
        }

        #[test]
        fn rejects_a_reference_to_a_field_that_does_not_exist() {
            let name = SchemaName::from("book");
            let book =
                Schema::new(name.clone(), BTreeMap::new(), BTreeSet::new());
            let mut resolved = BTreeMap::new();
            resolved.insert(name.clone(), book);
            let mut ancestors = BTreeSet::new();
            ancestors.insert(name);
            let refs = RefResolver {
                ancestors: &ancestors,
                resolved: &resolved,
            };
            let address = FieldAddressRef::new(
                SchemaNameRef::from("sci_fi"),
                crate::field::FieldNameRef::try_from("status")
                    .expect("valid field name"),
            );

            let err = refs
                .resolve(address, &field_address("#book/status"))
                .expect_err("missing field rejected");

            assert!(matches!(
                err,
                SchemaError::FieldBuilder(inner)
                    if matches!(*inner, SchemaFieldBuilderError::RefFieldNotFound { .. })
            ));
        }
    }
}
