//! `$ref` resolution and raw-to-resolved field building.

use std::collections::{BTreeMap, BTreeSet};

use super::{
    super::{
        GLOBAL_SCHEMA_NAME,
        error::{SchemaError, SchemaFieldBuilderError, SchemaWarning},
        model::Schema,
        name::SchemaName,
        raw::{RawFieldSource, RawSchemaFieldDef},
    },
    SchemaFieldDef, SchemaFieldType,
    address::{FieldAddress, FieldAddressRef},
};

/// Builds one resolved [`SchemaFieldDef`] from its raw declaration, resolving
/// a `$ref` (if any) against already-resolved Schemas.
pub(crate) struct SchemaFieldBuilder<'a> {
    pub(crate) refs: &'a RefResolver<'a>,
    pub(crate) warnings: &'a mut Vec<SchemaWarning>,
}

impl SchemaFieldBuilder<'_> {
    /// Build `address`'s effective [`SchemaFieldDef`] from `raw`.
    ///
    /// - `Direct(kind)`: builds fresh from `raw.options`, no base to merge.
    /// - `Ref` with a local `type` override: builds fresh from `raw.options`
    ///   against the override kind, ignoring the base's own type-specific
    ///   options (see [`SchemaFieldType::try_parse`]'s docs on type-switching
    ///   refs).
    /// - Bare `Ref` (no override): resolves the base field via [`RefResolver`],
    ///   then merges `raw.options` on top of the base's own options at the
    ///   base's kind. An unrecognized key or wrongly-shaped value here degrades
    ///   to a [`SchemaWarning`] and is dropped rather than failing the build.
    ///
    /// # Errors
    ///
    /// - Any error [`RefResolver::resolve`] returns while resolving a `$ref`
    ///   ([`SchemaError::FieldBuilder`] wrapping
    ///   [`SchemaFieldBuilderError::RefOutOfBounds`] or
    ///   [`SchemaFieldBuilderError::RefFieldNotFound`]).
    /// - [`SchemaError::FieldBuilder`] if a `Direct` field or a `$ref` with a
    ///   local `type` override declares an unrecognized attribute key
    ///   ([`SchemaFieldBuilderError::UnknownAttributeKey`]) or a wrongly-shaped
    ///   attribute value
    ///   ([`SchemaFieldBuilderError::AttributeValueTypeMismatch`]).
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
                let (field_type, errors) = SchemaFieldType::try_parse(
                    address,
                    *override_type,
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
                let (field_type, errors) = SchemaFieldType::try_parse(
                    address,
                    base.kind().raw_kind(),
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
                let (field_type, errors) = SchemaFieldType::try_parse(
                    address,
                    *raw_type,
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

    /// Force `required` to `false` and record a
    /// [`SchemaWarning::StrayGlobalRequired`] when `address.schema()` is the
    /// Global Schema and it declared `required = true`.
    ///
    /// Global Schema fields can never be required.
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

/// Resolve a Schema's `$ref` values to their base [`SchemaFieldDef`]s, bounded
/// to the Global Schema or the referencing Schema's transitive `extends`
/// ancestors.
///
/// `$ref`s point up the `extends` DAG or to the Global Schema, so they are
/// acyclic by construction.
pub(crate) struct RefResolver<'a> {
    pub(crate) ancestors: &'a BTreeSet<SchemaName>,
    pub(crate) resolved: &'a BTreeMap<SchemaName, Schema>,
}

impl<'a> RefResolver<'a> {
    /// Resolve `base_address`, `address`'s own already-parsed `$ref` value, to
    /// its base [`SchemaFieldDef`].
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
