//! `$ref` resolution and raw-to-resolved field building.

use std::collections::{BTreeMap, BTreeSet};

use super::{
    super::{
        GLOBAL_SCHEMA_NAME,
        error::{SchemaError, SchemaFieldBuilderError, SchemaWarning},
        model::Schema,
        name::SchemaName,
        raw::{RawFieldSource, RawSchemaFieldDef, RawSchemaFieldType},
    },
    SchemaDateField, SchemaFieldDef, SchemaFieldType, SchemaFileField,
    SchemaNumberField, SchemaSelectField,
    address::{FieldAddress, FieldAddressRef},
    parser,
};

/// Build one [`SchemaFieldDef`] from its raw declaration, resolving `$ref`
/// targets against already-resolved Schemas.
pub(crate) struct SchemaFieldBuilder<'a> {
    pub(crate) refs: &'a RefResolver<'a>,
    pub(crate) warnings: &'a mut Vec<SchemaWarning>,
}

impl SchemaFieldBuilder<'_> {
    /// Build `address`'s effective [`SchemaFieldDef`] from `raw`.
    ///
    /// - `Direct(kind)` or `Ref` with a `type` override: builds fresh from
    ///   `raw.options` against the resolved kind.
    /// - Bare `Ref` (no override): resolves the base via [`RefResolver`], then
    ///   merges `raw.options` on top. Unrecognized or wrongly-shaped keys
    ///   degrade to [`SchemaWarning`] rather than failing.
    ///
    /// # Errors
    ///
    /// - [`SchemaError::FieldBuilder`]: `$ref` resolution failure, unrecognized
    ///   attribute key, or wrongly-shaped attribute value.
    pub(crate) fn build(
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
                let (field_type, errors) = parse_field(
                    address,
                    raw_to_schema_kind(*override_type),
                    &raw.options,
                    Some(base.kind()),
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
                let base_kind = base.kind().clone();
                let (field_type, errors) = parse_field(
                    address,
                    base_kind,
                    &raw.options,
                    Some(base.kind()),
                );
                self.warnings.extend(errors.into_iter().map(Into::into));
                (
                    field_type,
                    raw.required.unwrap_or(base.is_required()),
                    raw.multi.unwrap_or(base.is_multi()),
                )
            }
            RawFieldSource::Direct(raw_type) => {
                let (field_type, errors) = parse_field(
                    address,
                    raw_to_schema_kind(*raw_type),
                    &raw.options,
                    None,
                );
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
            self.apply_global_degrade(address, required),
            multi,
        ))
    }

    /// Degrade `required` to `false` for Global Schema fields, which can never
    /// be required.
    fn apply_global_degrade(
        &mut self,
        address: FieldAddressRef<'_>,
        required: bool,
    ) -> bool {
        if address.schema().as_str() == GLOBAL_SCHEMA_NAME && required {
            self.warnings.push(SchemaWarning::StrayGlobalRequired {
                field: address.field().as_str().to_owned(),
            });
            false
        } else {
            required
        }
    }
}

/// Convert a [`RawSchemaFieldType`] tag to a minimal [`SchemaFieldType`]
/// variant for parser dispatch. The inner options are default-constructed;
/// they exist only so the parser carries the correct `Display` tag in error
/// messages.
fn raw_to_schema_kind(raw: RawSchemaFieldType) -> SchemaFieldType {
    match raw {
        RawSchemaFieldType::Input => SchemaFieldType::Input,
        RawSchemaFieldType::Select => {
            SchemaFieldType::Select(SchemaSelectField::default())
        }
        RawSchemaFieldType::Boolean => SchemaFieldType::Boolean,
        RawSchemaFieldType::Number => {
            SchemaFieldType::Number(SchemaNumberField::default())
        }
        RawSchemaFieldType::Date => {
            SchemaFieldType::Date(SchemaDateField::default())
        }
        RawSchemaFieldType::File => {
            SchemaFieldType::File(SchemaFileField::default())
        }
    }
}

/// Dispatch `options` parsing to the appropriate per-type `parse` function.
fn parse_field(
    address: FieldAddressRef<'_>,
    kind: SchemaFieldType,
    options: &std::collections::BTreeMap<String, crate::field::FieldValue>,
    base: Option<&SchemaFieldType>,
) -> (SchemaFieldType, Vec<super::error::SchemaFieldParserError>) {
    use super::{date, file, number, select};
    match &kind {
        SchemaFieldType::Input => {
            let unknowns = parser::parse_simple(address, kind, options);
            (SchemaFieldType::Input, unknowns)
        }
        SchemaFieldType::Boolean => {
            let unknowns = parser::parse_simple(address, kind, options);
            (SchemaFieldType::Boolean, unknowns)
        }
        SchemaFieldType::Select(_) => {
            select::SchemaSelectField::parse(address, options, base)
        }
        SchemaFieldType::Number(_) => {
            number::SchemaNumberField::parse(address, options, base)
        }
        SchemaFieldType::Date(_) => {
            date::SchemaDateField::parse(address, options, base)
        }
        SchemaFieldType::File(_) => {
            file::SchemaFileField::parse(address, options, base)
        }
    }
}

/// Resolve `$ref` values to their base [`SchemaFieldDef`]s, bounded to the
/// Global Schema or the referencing Schema's transitive `extends` ancestors.
pub(crate) struct RefResolver<'a> {
    pub(crate) ancestors: &'a BTreeSet<SchemaName>,
    pub(crate) resolved: &'a BTreeMap<SchemaName, Schema>,
}

impl<'a> RefResolver<'a> {
    /// Resolve `base_address` to its [`SchemaFieldDef`].
    ///
    /// # Errors
    ///
    /// - [`SchemaFieldBuilderError::RefOutOfBounds`] if the named Schema is
    ///   neither the Global Schema nor a transitive `extends` ancestor of
    ///   `address.schema()`.
    /// - [`SchemaFieldBuilderError::RefFieldNotFound`] if the named Schema is
    ///   in bounds but has no such field.
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
    use super::{super::SchemaSelectFieldEntry, *};
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

            assert_eq!(field.kind(), &SchemaFieldType::Input);
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
