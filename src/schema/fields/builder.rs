//! Build validated field definitions from raw declarations.
//!
//! Constructs a [`SchemaFieldDef`] from its raw declaration by resolving `$ref`
//! targets, merging inherited options, and validating against the resolved
//! field type.
//!
//! The main type is [`SchemaFieldBuilder`], which builds a field definition
//! from raw input, resolving `$ref` targets against ancestor schemas.

use indexmap::{IndexMap, IndexSet};

use super::{
    SchemaFieldDef, SchemaFieldType, SchemaFieldTypeTag,
    address::{FieldAddress, FieldAddressRef},
    error::{SchemaFieldBuilderError, SchemaFieldParserError},
};
use crate::{
    field::FieldValue,
    schema::{
        GLOBAL_SCHEMA_NAME, RawSchemaFieldDef, RawSchemaFieldSource,
        SchemaName,
        error::{SchemaError, SchemaWarning},
        model::Schema,
    },
};

/// [`SchemaFieldBuilder::resolve_from_raw`]'s result: the inherited base field
/// (if any), the effective type tag to parse against, and whether parser errors
/// on this field degrade to warnings rather than failing.
struct ResolvedBase<'a> {
    field: Option<&'a SchemaFieldDef>,
    tag: SchemaFieldTypeTag,
    degrade_on_error: bool,
}

/// Build one [`SchemaFieldDef`] from its raw declaration, resolving `$ref`
/// targets against already-resolved Schemas.
///
/// `$ref` targets are bounded to the Global Schema or the referencing Schema's
/// transitive `extends` ancestors.
pub(crate) struct SchemaFieldBuilder<'a> {
    ancestors: &'a IndexSet<SchemaName>,
    resolved: &'a IndexMap<SchemaName, Schema>,
}

impl<'a> SchemaFieldBuilder<'a> {
    pub(crate) const fn new(
        ancestors: &'a IndexSet<SchemaName>,
        resolved: &'a IndexMap<SchemaName, Schema>,
    ) -> Self {
        Self {
            ancestors,
            resolved,
        }
    }

    /// Build `address`'s effective [`SchemaFieldDef`] from `raw`, alongside any
    /// warnings a bare `$ref` override's degraded validation raised.
    ///
    /// - `Direct(kind)` or `Ref` with a `type` override: builds fresh from
    ///   `raw.options` against the resolved kind.
    /// - Bare `Ref` (no override): resolves the base via [`Self::resolve_ref`],
    ///   then merges `raw.options` on top. Unrecognized or wrongly-shaped keys
    ///   degrade to [`SchemaWarning`] rather than failing.
    ///
    /// # Arguments
    ///
    /// * `address` - path identifying the field within the schema hierarchy.
    /// * `raw` - the raw field declaration to build from.
    ///
    /// # Errors
    ///
    /// - [`RefOutOfBounds`] if the `$ref` target is neither the Global Schema
    ///   nor a transitive `extends` ancestor.
    /// - [`RefFieldNotFound`] if the `$ref` target Schema exists but lacks the
    ///   named field.
    /// - [`Parser`] if an option key does not belong to the resolved field
    ///   type, or is valid but its value has the wrong shape (hard failure for
    ///   `Direct` fields and `$ref` with `type` override).
    ///
    /// [`RefOutOfBounds`]: SchemaFieldBuilderError::RefOutOfBounds
    /// [`RefFieldNotFound`]: SchemaFieldBuilderError::RefFieldNotFound
    /// [`Parser`]: SchemaFieldBuilderError::Parser
    pub(crate) fn build(
        &self,
        address: FieldAddressRef<'_>,
        raw: &RawSchemaFieldDef,
    ) -> Result<(SchemaFieldDef, Vec<SchemaWarning>), SchemaError> {
        let mut warnings = Vec::new();
        let ResolvedBase {
            field: base,
            tag,
            degrade_on_error,
        } = self.resolve_from_raw(address, raw)?;
        let (field_type, errors) = Self::parse_options(
            address,
            tag,
            &raw.options,
            base.map(SchemaFieldDef::kind),
        );
        if !errors.is_empty() {
            if degrade_on_error {
                warnings.extend(errors.into_iter().map(Into::into));
            } else {
                return Err(SchemaFieldBuilderError::Parser(errors).into());
            }
        }
        let required = raw
            .required
            .unwrap_or_else(|| base.is_some_and(SchemaFieldDef::is_required));
        let multi = raw
            .multi
            .unwrap_or_else(|| base.is_some_and(SchemaFieldDef::is_multi));
        let (required, degrade) =
            Self::handle_global_required_value(address, required);
        warnings.extend(degrade);
        Ok((SchemaFieldDef::new(field_type, required, multi), warnings))
    }

    /// Resolve `raw` to its base [`SchemaFieldDef`], the effective
    /// [`SchemaFieldTypeTag`], and whether parser errors should degrade to
    /// warnings.
    ///
    /// # Errors
    ///
    /// - [`RefOutOfBounds`] if the `$ref` target is neither the Global Schema
    ///   nor a transitive `extends` ancestor.
    /// - [`RefFieldNotFound`] if the `$ref` target Schema exists but lacks the
    ///   named field.
    ///
    /// [`RefOutOfBounds`]: SchemaFieldBuilderError::RefOutOfBounds
    /// [`RefFieldNotFound`]: SchemaFieldBuilderError::RefFieldNotFound
    fn resolve_from_raw(
        &self,
        address: FieldAddressRef<'_>,
        raw: &RawSchemaFieldDef,
    ) -> Result<ResolvedBase<'a>, SchemaError> {
        let (base, degrade_on_error) = match &raw.source {
            RawSchemaFieldSource::Ref {
                address: base_address,
                override_type,
            } => (
                Some(self.resolve_ref(address, base_address)?),
                override_type.is_none(),
            ),
            RawSchemaFieldSource::Direct(_) => (None, false),
        };
        let tag = match &raw.source {
            RawSchemaFieldSource::Direct(kind)
            | RawSchemaFieldSource::Ref {
                override_type: Some(kind),
                ..
            } => SchemaFieldTypeTag::from(*kind),
            RawSchemaFieldSource::Ref {
                override_type: None,
                ..
            } => {
                // `base` is always `Some` here: the `Ref` arms above only reach
                // this point after `self.resolve_ref` succeeded.
                #[expect(
                    clippy::expect_used,
                    reason = "the Ref match arm above already resolved base \
                              via self.resolve_ref; this expect only covers \
                              the impossible None case, not a recoverable \
                              caller error"
                )]
                let base = base.expect("bare $ref always resolves a base");
                base.kind().kind()
            }
        };
        Ok(ResolvedBase {
            field: base,
            tag,
            degrade_on_error,
        })
    }

    /// Parse `options` into a [`SchemaFieldType`], returning the resolved type
    /// and any [`SchemaFieldParserError`]s for unknown keys or type mismatches.
    ///
    /// # Arguments
    ///
    /// * `address` - path identifying the field within the schema hierarchy.
    /// * `kind` - the resolved field type tag to parse against.
    /// * `options` - the raw key-value options to validate and extract from.
    /// * `base` - inherited field type to merge defaults from, if any.
    fn parse_options(
        address: FieldAddressRef<'_>,
        kind: SchemaFieldTypeTag,
        options: &IndexMap<String, FieldValue>,
        base: Option<&SchemaFieldType>,
    ) -> (SchemaFieldType, Vec<SchemaFieldParserError>) {
        use super::{date, file, number, parser::SchemaFieldParser, select};
        let mut parser = SchemaFieldParser::new(address, kind);
        let field_type = match kind {
            SchemaFieldTypeTag::Input => SchemaFieldType::Input,
            SchemaFieldTypeTag::Boolean => SchemaFieldType::Boolean,
            SchemaFieldTypeTag::Select => select::SchemaSelectField::parse(
                &mut parser,
                options,
                base.and_then(SchemaFieldType::as_select),
            ),
            SchemaFieldTypeTag::Number => number::SchemaNumberField::parse(
                &mut parser,
                options,
                base.and_then(SchemaFieldType::as_number),
            ),
            SchemaFieldTypeTag::Date => date::SchemaDateField::parse(
                &mut parser,
                options,
                base.and_then(SchemaFieldType::as_date),
            ),
            SchemaFieldTypeTag::File => file::SchemaFileField::parse(
                &mut parser,
                options,
                base.and_then(SchemaFieldType::as_file),
            ),
        };
        let errors = parser.finish(options);
        (field_type, errors)
    }

    /// Degrade `required` to `false` for Global Schema fields, which can never
    /// be required.
    fn handle_global_required_value(
        address: FieldAddressRef<'_>,
        required: bool,
    ) -> (bool, Option<SchemaWarning>) {
        if address.schema().as_str() == GLOBAL_SCHEMA_NAME && required {
            (
                false,
                Some(SchemaWarning::StrayGlobalRequired {
                    field: address.field().as_str().to_owned(),
                }),
            )
        } else {
            (required, None)
        }
    }

    /// Resolve `base_address` to its [`SchemaFieldDef`].
    ///
    /// # Errors
    ///
    /// - [`RefOutOfBounds`] if the named Schema is neither the Global Schema
    ///   nor a transitive `extends` ancestor of `address.schema()`.
    /// - [`RefFieldNotFound`] if the named Schema is in bounds but has no such
    ///   field.
    ///
    /// [`RefOutOfBounds`]: SchemaFieldBuilderError::RefOutOfBounds
    /// [`RefFieldNotFound`]: SchemaFieldBuilderError::RefFieldNotFound
    fn resolve_ref(
        &self,
        address: FieldAddressRef<'_>,
        base_address: &FieldAddress,
    ) -> Result<&'a SchemaFieldDef, SchemaError> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{
        RawSchemaFieldSource, RawSchemaFieldType, SchemaNameRef,
    };

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
    ) -> (SchemaName, Schema) {
        let mut fields = IndexMap::new();
        fields.insert(
            crate::field::FieldName::try_from(field)
                .expect("valid test field name"),
            SchemaFieldDef::new(kind, false, false),
        );
        (
            SchemaName::from(name),
            Schema::new(SchemaName::from(name), fields, IndexSet::new()),
        )
    }

    /// Builds `(ancestors, resolved)` fixture data for a
    /// [`SchemaFieldBuilder`]: each schema in `schemas` gets one field named
    /// `field_name` with the given `kind`.
    fn fixture(
        schemas: &[(&str, &str, SchemaFieldType)],
        ancestors: &[&str],
    ) -> (IndexSet<SchemaName>, IndexMap<SchemaName, Schema>) {
        let resolved: IndexMap<SchemaName, Schema> = schemas
            .iter()
            .map(|(name, field_name, kind)| {
                schema_with_field(name, field_name, kind.clone())
            })
            .collect();
        let ancestors: IndexSet<SchemaName> =
            ancestors.iter().map(|name| SchemaName::from(*name)).collect();
        (ancestors, resolved)
    }

    /// Builds `(ancestors, resolved)` fixture data for a
    /// [`SchemaFieldBuilder`] where every schema gets one field named
    /// `"field"`.
    fn field_fixture(
        schemas: &[(&str, SchemaFieldType)],
        ancestors: &[&str],
    ) -> (IndexSet<SchemaName>, IndexMap<SchemaName, Schema>) {
        let resolved: IndexMap<SchemaName, Schema> = schemas
            .iter()
            .map(|(name, kind)| schema_with_field(name, "field", kind.clone()))
            .collect();
        let ancestors: IndexSet<SchemaName> =
            ancestors.iter().map(|name| SchemaName::from(*name)).collect();
        (ancestors, resolved)
    }

    /// Builds a [`RawSchemaFieldDef`] with sensible defaults.
    fn raw_field(
        source: RawSchemaFieldSource,
        required: Option<bool>,
        multi: Option<bool>,
        options: IndexMap<String, FieldValue>,
    ) -> RawSchemaFieldDef {
        RawSchemaFieldDef {
            source,
            required,
            multi,
            options,
        }
    }

    mod resolve_ref {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn resolves_a_field_from_an_ancestor_schema() {
            let (ancestors, resolved) =
                fixture(&[("book", "status", SchemaFieldType::Input)], &[
                    "book",
                ]);
            let field_builder = SchemaFieldBuilder::new(&ancestors, &resolved);
            let address = FieldAddressRef::new(
                SchemaNameRef::from("sci_fi"),
                crate::field::FieldNameRef::try_from("status")
                    .expect("valid field name"),
            );

            let field = field_builder
                .resolve_ref(address, &field_address("#book/status"))
                .expect("resolves");

            assert_eq!(field.kind(), &SchemaFieldType::Input);
        }

        #[test]
        fn resolves_a_field_from_the_global_schema() {
            let (ancestors, resolved) = fixture(
                &[(GLOBAL_SCHEMA_NAME, "priority", SchemaFieldType::Input)],
                &[],
            );
            let field_builder = SchemaFieldBuilder::new(&ancestors, &resolved);
            let address = FieldAddressRef::new(
                SchemaNameRef::from("task"),
                crate::field::FieldNameRef::try_from("priority")
                    .expect("valid field name"),
            );

            let field = field_builder
                .resolve_ref(address, &field_address("#global/priority"))
                .expect("resolves");

            assert_eq!(field.kind(), &SchemaFieldType::Input);
        }

        #[test]
        fn rejects_a_reference_outside_the_bound() {
            let (ancestors, resolved) =
                fixture(&[("movie", "status", SchemaFieldType::Input)], &[]);
            let field_builder = SchemaFieldBuilder::new(&ancestors, &resolved);
            let address = FieldAddressRef::new(
                SchemaNameRef::from("book"),
                crate::field::FieldNameRef::try_from("status")
                    .expect("valid field name"),
            );

            let err = field_builder
                .resolve_ref(address, &field_address("#movie/status"))
                .expect_err("out-of-bounds rejected");

            assert!(matches!(
                err,
                SchemaError::FieldBuilder(inner)
                    if matches!(*inner, SchemaFieldBuilderError::RefOutOfBounds { .. })
            ));
        }

        #[test]
        fn rejects_a_reference_to_a_field_that_does_not_exist() {
            let (ancestors, resolved) =
                fixture(&[("book", "status", SchemaFieldType::Input)], &[
                    "book",
                ]);
            let field_builder = SchemaFieldBuilder::new(&ancestors, &resolved);
            let address = FieldAddressRef::new(
                SchemaNameRef::from("sci_fi"),
                crate::field::FieldNameRef::try_from("missing")
                    .expect("valid field name"),
            );

            let err = field_builder
                .resolve_ref(address, &field_address("#book/missing"))
                .expect_err("missing field rejected");

            assert!(matches!(
                err,
                SchemaError::FieldBuilder(inner)
                    if matches!(*inner, SchemaFieldBuilderError::RefFieldNotFound { .. })
            ));
        }
    }

    mod build {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn returns_direct_field_with_no_options() {
            let (ancestors, resolved) = field_fixture(&[], &[]);
            let b = SchemaFieldBuilder::new(&ancestors, &resolved);
            let address = FieldAddressRef::new(
                SchemaNameRef::from("sci_fi"),
                crate::field::FieldNameRef::try_from("title")
                    .expect("valid field name"),
            );
            let raw = raw_field(
                RawSchemaFieldSource::Direct(RawSchemaFieldType::Input),
                Some(true),
                Some(false),
                IndexMap::new(),
            );

            let (field, warnings) = b.build(address, &raw).expect("builds");

            assert_eq!(field.kind(), &SchemaFieldType::Input);
            assert!(field.is_required());
            assert!(!field.is_multi());
            assert_eq!(warnings, Vec::new());
        }

        #[test]
        fn merges_bare_ref_with_base() {
            let (ancestors, resolved) =
                field_fixture(&[("book", SchemaFieldType::Input)], &["book"]);
            let b = SchemaFieldBuilder::new(&ancestors, &resolved);
            let address = FieldAddressRef::new(
                SchemaNameRef::from("sci_fi"),
                crate::field::FieldNameRef::try_from("field")
                    .expect("valid field name"),
            );
            let raw = raw_field(
                RawSchemaFieldSource::Ref {
                    address: field_address("#book/field"),
                    override_type: None,
                },
                None,
                None,
                IndexMap::new(),
            );

            let (field, _warnings) = b.build(address, &raw).expect("builds");

            assert_eq!(field.kind(), &SchemaFieldType::Input);
        }

        #[test]
        fn applies_type_override_on_ref() {
            let (ancestors, resolved) =
                field_fixture(&[("book", SchemaFieldType::Input)], &["book"]);
            let b = SchemaFieldBuilder::new(&ancestors, &resolved);
            let address = FieldAddressRef::new(
                SchemaNameRef::from("sci_fi"),
                crate::field::FieldNameRef::try_from("field")
                    .expect("valid field name"),
            );
            let raw = raw_field(
                RawSchemaFieldSource::Ref {
                    address: field_address("#book/field"),
                    override_type: Some(RawSchemaFieldType::Boolean),
                },
                None,
                None,
                IndexMap::new(),
            );

            let (field, _warnings) = b.build(address, &raw).expect("builds");

            assert_eq!(field.kind(), &SchemaFieldType::Boolean);
        }

        #[test]
        fn returns_parser_error_for_direct_field_with_bad_option() {
            let (ancestors, resolved) = field_fixture(&[], &[]);
            let b = SchemaFieldBuilder::new(&ancestors, &resolved);
            let address = FieldAddressRef::new(
                SchemaNameRef::from("sci_fi"),
                crate::field::FieldNameRef::try_from("title")
                    .expect("valid field name"),
            );
            let mut options = IndexMap::new();
            options.insert(
                "unknown_key".to_owned(),
                FieldValue::String("value".to_owned()),
            );
            let raw = raw_field(
                RawSchemaFieldSource::Direct(RawSchemaFieldType::Input),
                None,
                None,
                options,
            );

            let err = b.build(address, &raw).expect_err("rejects bad option");

            assert!(matches!(
                &err,
                SchemaError::FieldBuilder(inner)
                    if matches!(**inner, SchemaFieldBuilderError::Parser(_))
            ));
        }

        #[test]
        fn degrades_bare_ref_bad_option_to_warning() {
            let (ancestors, resolved) =
                field_fixture(&[("book", SchemaFieldType::Input)], &["book"]);
            let b = SchemaFieldBuilder::new(&ancestors, &resolved);
            let address = FieldAddressRef::new(
                SchemaNameRef::from("sci_fi"),
                crate::field::FieldNameRef::try_from("field")
                    .expect("valid field name"),
            );
            let mut options = IndexMap::new();
            options.insert(
                "unknown_key".to_owned(),
                FieldValue::String("value".to_owned()),
            );
            let raw = raw_field(
                RawSchemaFieldSource::Ref {
                    address: field_address("#book/field"),
                    override_type: None,
                },
                None,
                None,
                options,
            );

            let (field, warnings) = b.build(address, &raw).expect("builds");

            assert_eq!(field.kind(), &SchemaFieldType::Input);
            assert_ne!(warnings, Vec::new());
        }

        #[test]
        fn rejects_ref_to_out_of_bounds_target() {
            let (ancestors, resolved) =
                field_fixture(&[("movie", SchemaFieldType::Input)], &[]);
            let b = SchemaFieldBuilder::new(&ancestors, &resolved);
            let address = FieldAddressRef::new(
                SchemaNameRef::from("book"),
                crate::field::FieldNameRef::try_from("status")
                    .expect("valid field name"),
            );
            let raw = raw_field(
                RawSchemaFieldSource::Ref {
                    address: field_address("#movie/field"),
                    override_type: None,
                },
                None,
                None,
                IndexMap::new(),
            );

            let err = b.build(address, &raw).expect_err("out of bounds");

            assert!(matches!(
                &err,
                SchemaError::FieldBuilder(inner)
                    if matches!(**inner, SchemaFieldBuilderError::RefOutOfBounds { .. })
            ));
        }

        #[test]
        fn rejects_ref_to_missing_field() {
            let (ancestors, resolved) =
                field_fixture(&[("book", SchemaFieldType::Input)], &["book"]);
            let b = SchemaFieldBuilder::new(&ancestors, &resolved);
            let address = FieldAddressRef::new(
                SchemaNameRef::from("sci_fi"),
                crate::field::FieldNameRef::try_from("missing")
                    .expect("valid field name"),
            );
            let raw = raw_field(
                RawSchemaFieldSource::Ref {
                    address: field_address("#book/missing"),
                    override_type: None,
                },
                None,
                None,
                IndexMap::new(),
            );

            let err = b.build(address, &raw).expect_err("missing field");

            assert!(matches!(
                &err,
                SchemaError::FieldBuilder(inner)
                    if matches!(**inner, SchemaFieldBuilderError::RefFieldNotFound { .. })
            ));
        }

        #[test]
        fn strips_required_on_global_schema_with_warning() {
            let (ancestors, resolved) = field_fixture(
                &[(GLOBAL_SCHEMA_NAME, SchemaFieldType::Input)],
                &[],
            );
            let b = SchemaFieldBuilder::new(&ancestors, &resolved);
            let address = FieldAddressRef::new(
                SchemaNameRef::from(GLOBAL_SCHEMA_NAME),
                crate::field::FieldNameRef::try_from("title")
                    .expect("valid field name"),
            );
            let raw = raw_field(
                RawSchemaFieldSource::Direct(RawSchemaFieldType::Input),
                Some(true),
                None,
                IndexMap::new(),
            );

            let (field, warnings) = b.build(address, &raw).expect("builds");

            assert!(!field.is_required());
            assert!(warnings.iter().any(|w| matches!(
                w,
                SchemaWarning::StrayGlobalRequired { field: name }
                    if name == "title"
            )));
        }
    }
}
