//! Build validated field definitions from raw declarations.
//!
//! Constructs a [`SchemaFieldDef`] from its raw declaration by resolving `$ref`
//! targets, merging inherited options, and validating against the resolved
//! field type.
//!
//! # Main types:
//!
//! - [`SchemaFieldBuilder`]: builds a field definition from raw input.
//! - [`RefAddressResolver`]: resolves `$ref` targets against ancestor schemas.

use std::collections::{BTreeMap, BTreeSet};

use super::{
    super::{
        GLOBAL_SCHEMA_NAME,
        error::{SchemaError, SchemaWarning},
        model::Schema,
        name::SchemaName,
        raw::{RawFieldSource, RawSchemaFieldDef},
    },
    SchemaFieldDef, SchemaFieldType, SchemaFieldTypeTag,
    address::{FieldAddress, FieldAddressRef},
    error::SchemaFieldBuilderError,
};

/// Base field, resolved tag, and degrade flag from
/// [`SchemaFieldBuilder::resolve_from_raw`].
type ResolvedRawField<'a> =
    (Option<&'a SchemaFieldDef>, SchemaFieldTypeTag, bool);

/// Build one [`SchemaFieldDef`] from its raw declaration, resolving `$ref`
/// targets against already-resolved Schemas.
pub(crate) struct SchemaFieldBuilder<'a> {
    pub(crate) refs: &'a RefAddressResolver<'a>,
}

impl<'a> SchemaFieldBuilder<'a> {
    /// Build `address`'s effective [`SchemaFieldDef`] from `raw`, alongside
    /// any warnings a bare `$ref` override's degraded validation raised.
    ///
    /// - `Direct(kind)` or `Ref` with a `type` override: builds fresh from
    ///   `raw.options` against the resolved kind.
    /// - Bare `Ref` (no override): resolves the base via
    ///   [`RefAddressResolver`], then merges `raw.options` on top. Unrecognized
    ///   or wrongly-shaped keys degrade to [`SchemaWarning`] rather than
    ///   failing.
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
        let (base, tag, degrade_on_error) =
            self.resolve_from_raw(address, raw)?;
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
    ) -> Result<ResolvedRawField<'a>, SchemaError> {
        let (base, degrade_on_error) = match &raw.source {
            RawFieldSource::Ref {
                address: base_address,
                override_type,
            } => (
                Some(self.refs.resolve(address, base_address)?),
                override_type.is_none(),
            ),
            RawFieldSource::Direct(_) => (None, false),
        };
        let tag = match &raw.source {
            RawFieldSource::Direct(kind)
            | RawFieldSource::Ref {
                override_type: Some(kind),
                ..
            } => SchemaFieldTypeTag::from(*kind),
            RawFieldSource::Ref {
                override_type: None,
                ..
            } => {
                // `base` is always `Some` here: the `Ref` arms above only
                // reach this point after `self.refs.resolve` succeeded.
                #[expect(
                    clippy::expect_used,
                    reason = "the Ref match arm above already resolved base \
                              via self.refs.resolve; this expect only covers \
                              the impossible None case, not a recoverable \
                              caller error"
                )]
                let base = base.expect("bare $ref always resolves a base");
                base.kind().kind()
            }
        };
        Ok((base, tag, degrade_on_error))
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
        options: &std::collections::BTreeMap<String, crate::field::FieldValue>,
        base: Option<&SchemaFieldType>,
    ) -> (SchemaFieldType, Vec<super::error::SchemaFieldParserError>) {
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
}

/// Resolve `$ref` values to their base [`SchemaFieldDef`]s.
///
/// Bounded to the Global Schema or the referencing Schema's transitive
/// `extends` ancestors.
pub(crate) struct RefAddressResolver<'a> {
    pub(crate) ancestors: &'a BTreeSet<SchemaName>,
    pub(crate) resolved: &'a BTreeMap<SchemaName, Schema>,
}

impl<'a> RefAddressResolver<'a> {
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
    fn resolve(
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
    ) -> (SchemaName, Schema) {
        let mut fields = BTreeMap::new();
        fields.insert(
            crate::field::FieldName::try_from(field)
                .expect("valid test field name"),
            SchemaFieldDef::new(kind, false, false),
        );
        (
            SchemaName::from(name),
            Schema::new(SchemaName::from(name), fields, BTreeSet::new()),
        )
    }

    mod ref_address_resolver {
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
            let refs = RefAddressResolver {
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

            assert_eq!(field.kind(), &SchemaFieldType::Input);
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
            let refs = RefAddressResolver {
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

            assert_eq!(field.kind(), &SchemaFieldType::Input);
        }

        #[test]
        fn rejects_a_reference_outside_the_bound() {
            let (name, movie) =
                schema_with_field("movie", "status", SchemaFieldType::Input);
            let mut resolved = BTreeMap::new();
            resolved.insert(name, movie);
            let ancestors = BTreeSet::new();
            let refs = RefAddressResolver {
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
            let refs = RefAddressResolver {
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
