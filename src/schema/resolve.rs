//! Resolve raw Schema sets into effective Field Definitions via Kahn's
//! topological sort.
//!
//! No filesystem or minijinja access: [`resolve`] is a pure function over an
//! already-parsed Schema set. Reads top-down: the pipeline ([`resolve`] to
//! [`build_schema`] to [`build_field`]) comes first; [`RefResolver`] (`$ref`
//! bounds-checking) follows below. DAG bookkeeping lives in [`SchemaGraph`].
//!
//! # Main Type
//!
//! - [`resolve`]: linearizes the `extends` DAG into [`super::model::Schema`]s.
//!   See [`super::model`] for the resolved shapes ([`Schema`],
//!   [`FieldDefinition`], [`FieldOptions`]) this produces.

use std::collections::{BTreeMap, BTreeSet};

use super::{
    GLOBAL_SCHEMA_NAME,
    address::{FieldAddress, FieldAddressRef},
    error::{SchemaError, SchemaWarning},
    graph::SchemaGraph,
    model::{FieldDefinition, FieldOptions, FieldType, Schema},
    name::{SchemaName, SchemaNameRef},
    raw::{RawFieldDef, RawFieldSource, RawSchema},
};
use crate::field::FieldName;

/// Return every Schema resolved by [`resolve`], keyed by name, alongside the
/// [`SchemaWarning`]s degraded resolution accumulated along the way.
type ResolveOutput = (BTreeMap<SchemaName, Schema>, Vec<SchemaWarning>);

/// Resolve `raw_schemas` into effective Field Definitions per Schema.
///
/// Linearizes the `extends` DAG with Kahn's topological sort so every Schema is
/// resolved only after all of its valid parents. For each Schema, in order:
///
/// - Parent fields are merged first-listed-wins.
/// - `excludes` drops named fields from that merge.
/// - The Schema's own fields (`$ref`-resolved against already-resolved Schemas)
///   override the result.
///
/// # Errors
///
/// - [`SchemaError::Cycle`] if the `extends` DAG contains a cycle.
/// - [`SchemaError::RefOutOfBounds`] if a `$ref` names a Schema outside its
///   bound (the Global Schema or a transitive `extends` ancestor).
/// - [`SchemaError::RefFieldNotFound`] if a `$ref` names an in-bounds Schema
///   that has no such field.
/// - [`SchemaError::AmbiguousFieldName`] if two of a Schema's effective fields
///   share a [`FieldKey`](crate::field::FieldKey) canonical form.
pub(crate) fn resolve(
    raw_schemas: &BTreeMap<SchemaName, RawSchema>,
) -> Result<ResolveOutput, SchemaError> {
    let mut warnings = Vec::new();
    let mut graph = SchemaGraph::new(raw_schemas, &mut warnings);
    let mut resolved: BTreeMap<SchemaName, Schema> = BTreeMap::new();

    while let Some(name) = graph.next_ready() {
        let Some(raw) = raw_schemas.get(name.as_str()) else {
            continue;
        };
        let schema = build_schema(
            name,
            raw,
            graph.parents_of(name),
            &resolved,
            &mut warnings,
        )?;
        resolved.insert(SchemaName::from(name), schema);
        graph.mark_resolved(name);
    }

    match graph.cyclic_remainder(raw_schemas) {
        Some(schemas) => Err(SchemaError::Cycle {
            schemas,
        }),
        None => Ok((resolved, warnings)),
    }
}

/// Resolve one Schema's effective fields and transitive ancestors.
///
/// Merges `parents`' fields first-listed-wins, applies `raw.excludes`, then
/// overrides the result with `raw`'s own (`$ref`-resolved) fields.
///
/// `parents` must already be resolved in `resolved`: [`resolve`] guarantees
/// this by calling in Kahn topological order.
///
/// # Arguments
///
/// * `name` - The Schema being resolved (its filename stem).
/// * `raw` - `name`'s own parsed TOML: `extends`, `excludes`, and fields.
/// * `parents` - `raw.extends`, filtered to targets that resolved.
/// * `resolved` - Schemas already resolved earlier in Kahn order, keyed by
///   name.
/// * `warnings` - Accumulates degraded-resolution warnings raised while
///   building `name`'s own fields.
///
/// # Errors
///
/// Propagates any [`SchemaError`] that [`build_field`] returns while resolving
/// `raw`'s own fields, or [`SchemaError::AmbiguousFieldName`] if two of the
/// resolved fields share a [`FieldKey`](crate::field::FieldKey) canonical form.
fn build_schema(
    name: SchemaNameRef<'_>,
    raw: &RawSchema,
    parents: &[SchemaNameRef<'_>],
    resolved: &BTreeMap<SchemaName, Schema>,
    warnings: &mut Vec<SchemaWarning>,
) -> Result<Schema, SchemaError> {
    let mut fields = BTreeMap::new();
    let mut ancestors = BTreeSet::new();
    for &parent in parents {
        let Some(parent_schema) = resolved.get(parent.as_str()) else {
            continue;
        };
        for (field_name, field) in parent_schema.fields() {
            fields.entry(field_name.clone()).or_insert_with(|| field.clone());
        }
        ancestors.insert(SchemaName::from(parent));
        ancestors.extend(parent_schema.ancestors().iter().cloned());
    }
    for excluded in &raw.excludes {
        fields.remove(excluded);
    }

    // Own fields resolve last (so they override inherited fields above) but
    // need `ancestors` computed above to validate a `$ref`'s bounded target:
    // `#global/<field>` or `#<ancestor-schema>/<field>` only.
    let refs = RefResolver {
        ancestors: &ancestors,
        resolved,
    };
    let mut own_fields = BTreeMap::new();
    for (field_name, raw_field) in &raw.fields {
        let address = FieldAddressRef::new(name, field_name.as_ref());
        let field = build_field(address, raw_field, &refs, warnings)?;
        own_fields.insert(field_name.clone(), field);
    }
    fields.extend(own_fields);

    reject_ambiguous_canonical_names(name, &fields)?;

    Ok(Schema::new(SchemaName::from(name), fields, ancestors))
}

/// Build one resolved [`FieldDefinition`] for `address`, resolving its `$ref`
/// (if any) via `refs` and applying the Global Schema's `required` degrade.
///
/// # Errors
///
/// Any error that [`RefResolver::resolve`] returns while resolving a `$ref`.
fn build_field(
    address: FieldAddressRef<'_>,
    raw: &RawFieldDef,
    refs: &RefResolver<'_>,
    warnings: &mut Vec<SchemaWarning>,
) -> Result<FieldDefinition, SchemaError> {
    let (options, required, multi) = match &raw.source {
        RawFieldSource::Ref {
            address: base_address,
            override_type,
        } => {
            let base = refs.resolve(address, base_address)?;
            let field_type = (*override_type)
                .map_or_else(|| base.options().kind(), FieldType::from);
            (
                FieldOptions::build(field_type, raw, Some(base.options())),
                raw.required.unwrap_or(base.is_required()),
                raw.multi.unwrap_or(base.is_multi()),
            )
        }
        RawFieldSource::Direct(raw_type) => {
            let field_type = FieldType::from(*raw_type);
            (
                FieldOptions::build(field_type, raw, None),
                raw.required.unwrap_or(false),
                raw.multi.unwrap_or(false),
            )
        }
    };

    Ok(FieldDefinition::new(
        options,
        apply_global_degrade(address, required, warnings),
        multi,
    ))
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

/// Reject `fields` if two entries share a [`FieldKey`](crate::field::FieldKey)
/// canonical form: ambiguous field identities would make later note-vs-schema
/// field matching and unknown-field suggestions unreliable.
///
/// # Errors
///
/// Returns [`SchemaError::AmbiguousFieldName`] naming the first two
/// (name-sorted) colliding field names.
fn reject_ambiguous_canonical_names(
    name: SchemaNameRef<'_>,
    fields: &BTreeMap<FieldName, FieldDefinition>,
) -> Result<(), SchemaError> {
    let mut seen: BTreeMap<String, FieldName> = BTreeMap::new();
    for field_name in fields.keys() {
        let canonical = field_name.to_key().canonical().to_owned();
        if let Some(first) = seen.get(&canonical) {
            return Err(SchemaError::AmbiguousFieldName {
                schema: SchemaName::from(name),
                first: first.clone(),
                second: Box::new(field_name.clone()),
            });
        }
        seen.insert(canonical, field_name.clone());
    }
    Ok(())
}

/// Resolve a Schema's `$ref` values to their base [`FieldDefinition`]s, bounded
/// to the Global Schema or the referencing Schema's transitive `extends`
/// ancestors.
///
/// `$ref`s point up the `extends` DAG or to the Global Schema, so they are
/// acyclic by construction.
struct RefResolver<'a> {
    ancestors: &'a BTreeSet<SchemaName>,
    resolved: &'a BTreeMap<SchemaName, Schema>,
}

impl<'a> RefResolver<'a> {
    /// Resolve `base_address`, `address`'s own already-parsed `$ref` value, to
    /// its base [`FieldDefinition`].
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
    ) -> Result<&'a FieldDefinition, SchemaError> {
        // Rejecting an out-of-bounds target here, rather than only checking
        // whether it happens to be resolved yet, keeps the bound spec-accurate
        // and independent of Kahn's tie-breaking order among unrelated Schemas.
        if base_address.schema().as_str() != GLOBAL_SCHEMA_NAME
            && !self.ancestors.contains(base_address.schema().as_str())
        {
            return Err(SchemaError::RefOutOfBounds {
                own: Box::new(FieldAddress::from(address)),
                reference: Box::new(base_address.clone()),
            });
        }
        self.resolved
            .get(base_address.schema().as_str())
            .and_then(|schema| schema.field(base_address.field().as_str()))
            .ok_or_else(|| SchemaError::RefFieldNotFound {
                own: Box::new(FieldAddress::from(address)),
                reference: Box::new(base_address.clone()),
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{field::FieldNameRef, schema::raw::RawFieldType};

    /// Parses `name` into a [`FieldName`], panicking on an invalid test
    /// fixture.
    fn field_name(name: &str) -> FieldName {
        FieldName::try_from(name).expect("valid test field name")
    }

    /// Parses `reference` into a [`FieldAddress`], panicking on an invalid test
    /// fixture.
    fn field_address(reference: &str) -> FieldAddress {
        FieldAddress::try_from(reference).expect("valid test $ref")
    }

    /// Builds a `select`-type [`RawFieldDef`] with the given `values`.
    fn select_field(values: &[&str]) -> RawFieldDef {
        RawFieldDef {
            values: Some(values.iter().map(|&v| v.to_owned()).collect()),
            ..RawFieldDef::direct(RawFieldType::Select)
        }
    }

    /// Builds an `input`-type [`RawFieldDef`].
    fn input_field() -> RawFieldDef {
        RawFieldDef::direct(RawFieldType::Input)
    }

    /// Builds a `file`-type [`RawFieldDef`] with the given filter.
    fn file_field(
        folders: &[&str],
        ext: Option<&str>,
        class: &[&str],
    ) -> RawFieldDef {
        RawFieldDef {
            folders: Some(folders.iter().map(|&f| f.to_owned()).collect()),
            ext: ext.map(str::to_owned),
            class: Some(class.iter().map(|&c| c.to_owned()).collect()),
            ..RawFieldDef::direct(RawFieldType::File)
        }
    }

    /// Builds a `$ref`-only [`RawFieldDef`] with an optional local
    /// `required` override.
    fn ref_field(reference: &str, required: Option<bool>) -> RawFieldDef {
        RawFieldDef {
            required,
            ..RawFieldDef::reference(field_address(reference))
        }
    }

    fn schema(extends: &[&str], fields: &[(&str, RawFieldDef)]) -> RawSchema {
        RawSchema {
            extends: extends.iter().map(|&s| SchemaName::from(s)).collect(),
            excludes: Vec::new(),
            fields: fields
                .iter()
                .cloned()
                .map(|(name, def)| (field_name(name), def))
                .collect(),
        }
    }

    fn schema_with_excludes(
        extends: &[&str],
        excludes: &[&str],
        fields: &[(&str, RawFieldDef)],
    ) -> RawSchema {
        RawSchema {
            excludes: excludes.iter().map(|&s| field_name(s)).collect(),
            ..schema(extends, fields)
        }
    }

    mod resolve {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn resolves_a_schema_with_no_extends() {
            let mut raw = BTreeMap::new();
            raw.insert(
                SchemaName::from("book"),
                schema(&[], &[("status", select_field(&["draft", "done"]))]),
            );

            let (resolved, warnings) = resolve(&raw).expect("resolves");

            assert!(warnings.is_empty());
            let book = resolved.get("book").expect("book resolved");
            assert_eq!(book.name(), "book");
            let status = book.field("status").expect("status field");
            assert_eq!(status.options(), &FieldOptions::Select {
                values: vec!["draft".to_owned(), "done".to_owned()]
            });
            assert!(!status.is_required());
            assert!(!status.is_multi());
        }

        #[test]
        fn own_fields_override_parent_fields() {
            let mut raw = BTreeMap::new();
            raw.insert(
                SchemaName::from("book"),
                schema(&[], &[("status", select_field(&["draft", "done"]))]),
            );
            raw.insert(
                SchemaName::from("sci_fi"),
                schema(&["book"], &[(
                    "status",
                    select_field(&["outline", "shipped"]),
                )]),
            );

            let (resolved, _) = resolve(&raw).expect("resolves");

            let status = resolved
                .get("sci_fi")
                .and_then(|s| s.field("status"))
                .expect("status field");
            assert_eq!(status.options(), &FieldOptions::Select {
                values: vec!["outline".to_owned(), "shipped".to_owned()]
            });
        }

        #[test]
        fn first_listed_parent_wins_a_shared_field() {
            let mut raw = BTreeMap::new();
            raw.insert(
                SchemaName::from("a"),
                schema(&[], &[("shared", select_field(&["from-a"]))]),
            );
            raw.insert(
                SchemaName::from("b"),
                schema(&[], &[("shared", select_field(&["from-b"]))]),
            );
            raw.insert(SchemaName::from("child"), schema(&["a", "b"], &[]));

            let (resolved, _) = resolve(&raw).expect("resolves");

            let shared = resolved
                .get("child")
                .and_then(|s| s.field("shared"))
                .expect("shared field");
            assert_eq!(shared.options(), &FieldOptions::Select {
                values: vec!["from-a".to_owned()]
            });
        }

        #[test]
        fn excludes_drops_an_inherited_field_by_name() {
            let mut raw = BTreeMap::new();
            raw.insert(
                SchemaName::from("book"),
                schema(&[], &[
                    ("status", select_field(&["draft"])),
                    ("author", input_field()),
                ]),
            );
            raw.insert(
                SchemaName::from("sci_fi"),
                schema_with_excludes(&["book"], &["status"], &[]),
            );

            let (resolved, _) = resolve(&raw).expect("resolves");

            let sci_fi = resolved.get("sci_fi").expect("sci_fi resolved");
            assert!(sci_fi.field("status").is_none());
            assert!(sci_fi.field("author").is_some());
            let field_names: Vec<&str> =
                sci_fi.fields().keys().map(FieldName::as_str).collect();
            assert_eq!(field_names, vec!["author"]);
        }

        #[test]
        fn excludes_is_exact_and_does_not_drop_a_different_case_field() {
            // `excludes` uses exact `FieldName` identity, not canonical
            // `FieldKey` matching: `excludes = ["Status"]` must not remove an
            // inherited `status`.
            let mut raw = BTreeMap::new();
            raw.insert(
                SchemaName::from("book"),
                schema(&[], &[("status", select_field(&["draft"]))]),
            );
            raw.insert(
                SchemaName::from("sci_fi"),
                schema_with_excludes(&["book"], &["Status"], &[]),
            );

            let (resolved, _) = resolve(&raw).expect("resolves");

            let sci_fi = resolved.get("sci_fi").expect("sci_fi resolved");
            assert!(sci_fi.field("status").is_some());
        }

        #[test]
        fn a_missing_extends_target_degrades_with_a_warning() {
            let mut raw = BTreeMap::new();
            raw.insert(
                SchemaName::from("sci_fi"),
                schema(&["ghost"], &[("title", input_field())]),
            );

            let (resolved, warnings) = resolve(&raw).expect("resolves");

            assert_eq!(warnings, vec![SchemaWarning::MissingExtendsTarget {
                schema: SchemaName::from("sci_fi"),
                target: SchemaName::from("ghost"),
            }]);
            let sci_fi =
                resolved.get("sci_fi").expect("own fields still render");
            assert!(sci_fi.field("title").is_some());
            assert!(!sci_fi.is_a("ghost"));
        }

        #[test]
        fn a_cycle_is_a_hard_error() {
            let mut raw = BTreeMap::new();
            raw.insert(SchemaName::from("a"), schema(&["b"], &[]));
            raw.insert(SchemaName::from("b"), schema(&["a"], &[]));

            let err = resolve(&raw).expect_err("cycle rejected");
            assert!(
                matches!(
                    &err,
                    SchemaError::Cycle { schemas }
                        if schemas == &vec![SchemaName::from("a"), SchemaName::from("b")]
                ),
                "expected Cycle over [a, b], got {err:?}"
            );
        }

        #[test]
        fn is_a_matches_transitively_through_extends() {
            let mut raw = BTreeMap::new();
            raw.insert(SchemaName::from("thing"), schema(&[], &[]));
            raw.insert(SchemaName::from("book"), schema(&["thing"], &[]));
            raw.insert(SchemaName::from("sci_fi"), schema(&["book"], &[]));

            let (resolved, _) = resolve(&raw).expect("resolves");

            let sci_fi = resolved.get("sci_fi").expect("sci_fi resolved");
            assert!(sci_fi.is_a("sci_fi"));
            assert!(sci_fi.is_a("book"));
            assert!(sci_fi.is_a("thing"));
            assert!(!sci_fi.is_a("movie"));
        }

        #[test]
        fn a_ref_to_an_ancestor_resolves_with_local_overrides() {
            let mut raw = BTreeMap::new();
            raw.insert(
                SchemaName::from("book"),
                schema(&[], &[("status", select_field(&["draft", "done"]))]),
            );
            raw.insert(
                SchemaName::from("sci_fi"),
                schema(&["book"], &[(
                    "status",
                    ref_field("#book/status", Some(true)),
                )]),
            );

            let (resolved, _) = resolve(&raw).expect("resolves");

            let status = resolved
                .get("sci_fi")
                .and_then(|s| s.field("status"))
                .expect("status field");
            assert_eq!(status.options(), &FieldOptions::Select {
                values: vec!["draft".to_owned(), "done".to_owned()]
            });
            assert!(status.is_required());
        }

        #[test]
        fn a_ref_to_global_resolves_with_local_overrides() {
            let mut raw = BTreeMap::new();
            raw.insert(
                SchemaName::from(GLOBAL_SCHEMA_NAME),
                schema(&[], &[("priority", select_field(&["low", "high"]))]),
            );
            raw.insert(
                SchemaName::from("task"),
                schema(&[], &[(
                    "priority",
                    ref_field("#global/priority", Some(true)),
                )]),
            );

            let (resolved, _) = resolve(&raw).expect("resolves");

            let priority = resolved
                .get("task")
                .and_then(|s| s.field("priority"))
                .expect("priority field");
            assert_eq!(priority.options(), &FieldOptions::Select {
                values: vec!["low".to_owned(), "high".to_owned()]
            });
            assert!(priority.is_required());
        }

        #[test]
        fn a_stray_required_on_global_is_ignored_with_a_warning() {
            let mut raw = BTreeMap::new();
            raw.insert(
                SchemaName::from(GLOBAL_SCHEMA_NAME),
                schema(&[], &[("priority", RawFieldDef {
                    required: Some(true),
                    ..select_field(&["low", "high"])
                })]),
            );

            let (resolved, warnings) = resolve(&raw).expect("resolves");

            let priority = resolved
                .get(GLOBAL_SCHEMA_NAME)
                .and_then(|s| s.field("priority"))
                .expect("priority field");
            assert!(!priority.is_required());
            assert_eq!(warnings, vec![SchemaWarning::StrayGlobalRequired {
                field: "priority".to_owned()
            }]);
        }

        #[test]
        fn a_ref_to_an_unknown_field_is_a_hard_error() {
            let mut raw = BTreeMap::new();
            raw.insert(SchemaName::from("book"), schema(&[], &[]));
            raw.insert(
                SchemaName::from("sci_fi"),
                schema(&["book"], &[(
                    "status",
                    ref_field("#book/status", None),
                )]),
            );

            let err = resolve(&raw).expect_err("unresolved ref rejected");
            assert!(matches!(err, SchemaError::RefFieldNotFound { .. }));
        }

        #[test]
        fn a_ref_to_a_non_ancestor_sibling_is_a_hard_error() {
            // `movie` and `book` share no `extends` relationship: a `$ref` from
            // one to the other is out of bounds even though both happen to
            // resolve in the same Kahn tier (spec: "$ref is deliberately
            // bounded to global + ancestors").
            let mut raw = BTreeMap::new();
            raw.insert(
                SchemaName::from("book"),
                schema(&[], &[("status", select_field(&["draft"]))]),
            );
            raw.insert(
                SchemaName::from("movie"),
                schema(&[], &[("status", ref_field("#book/status", None))]),
            );

            let err = resolve(&raw).expect_err("out-of-bounds ref rejected");
            assert!(matches!(err, SchemaError::RefOutOfBounds { .. }));
        }

        #[test]
        fn defining_both_status_and_status_cased_differently_is_a_hard_error() {
            let mut raw = BTreeMap::new();
            raw.insert(
                SchemaName::from("book"),
                schema(&[], &[
                    ("status", input_field()),
                    ("Status", input_field()),
                ]),
            );

            let err = resolve(&raw).expect_err("ambiguous field name rejected");
            assert!(matches!(err, SchemaError::AmbiguousFieldName { .. }));
        }

        #[test]
        fn an_own_field_colliding_with_an_inherited_field_is_a_hard_error() {
            let mut raw = BTreeMap::new();
            raw.insert(
                SchemaName::from("book"),
                schema(&[], &[("Due Date", input_field())]),
            );
            raw.insert(
                SchemaName::from("sci_fi"),
                schema(&["book"], &[("due-date", input_field())]),
            );

            let err = resolve(&raw).expect_err("ambiguous field name rejected");
            assert!(matches!(err, SchemaError::AmbiguousFieldName { .. }));
        }

        #[test]
        fn every_field_type_resolves_its_own_options() {
            let mut raw = BTreeMap::new();
            raw.insert(
                SchemaName::from("book"),
                schema(&[], &[
                    ("title", input_field()),
                    ("status", select_field(&["draft", "done"])),
                    ("archived", RawFieldDef::direct(RawFieldType::Boolean)),
                    ("rating", RawFieldDef::direct(RawFieldType::Number)),
                    ("published", RawFieldDef::direct(RawFieldType::Date)),
                    (
                        "cover",
                        file_field(&["assets/covers"], Some("png"), &["image"]),
                    ),
                ]),
            );

            let (resolved, _) = resolve(&raw).expect("resolves");
            let book = resolved.get("book").expect("book resolved");

            assert_eq!(
                book.field("title").map(FieldDefinition::options),
                Some(&FieldOptions::Input)
            );
            assert_eq!(
                book.field("status").map(FieldDefinition::options),
                Some(&FieldOptions::Select {
                    values: vec!["draft".to_owned(), "done".to_owned()]
                })
            );
            assert_eq!(
                book.field("archived").map(FieldDefinition::options),
                Some(&FieldOptions::Boolean)
            );
            assert_eq!(
                book.field("rating").map(FieldDefinition::options),
                Some(&FieldOptions::Number {
                    step: None,
                    min: None,
                    max: None,
                })
            );
            assert_eq!(
                book.field("published").map(FieldDefinition::options),
                Some(&FieldOptions::Date {
                    format: None
                })
            );
            assert_eq!(
                book.field("cover").map(FieldDefinition::options),
                Some(&FieldOptions::File {
                    folders: vec!["assets/covers".to_owned()],
                    ext: Some("png".to_owned()),
                    class: vec!["image".to_owned()],
                })
            );
        }

        #[test]
        fn multi_defaults_to_false_and_honors_a_local_override() {
            let mut raw = BTreeMap::new();
            raw.insert(
                SchemaName::from("book"),
                schema(&[], &[
                    ("status", select_field(&["draft"])),
                    ("authors", RawFieldDef {
                        multi: Some(true),
                        ..select_field(&["ann", "bo"])
                    }),
                ]),
            );

            let (resolved, _) = resolve(&raw).expect("resolves");
            let book = resolved.get("book").expect("book resolved");

            assert!(!book.field("status").expect("status").is_multi());
            assert!(book.field("authors").expect("authors").is_multi());
        }

        #[test]
        fn a_ref_to_a_file_field_merges_the_filter_with_local_overrides() {
            let mut raw = BTreeMap::new();
            raw.insert(
                SchemaName::from(GLOBAL_SCHEMA_NAME),
                schema(&[], &[(
                    "cover",
                    file_field(&["assets"], Some("png"), &["image"]),
                )]),
            );
            raw.insert(
                SchemaName::from("book"),
                schema(&[], &[("cover", RawFieldDef {
                    folders: Some(vec!["assets/covers".to_owned()]),
                    ..RawFieldDef::reference(field_address("#global/cover"))
                })]),
            );

            let (resolved, _) = resolve(&raw).expect("resolves");
            let cover = resolved
                .get("book")
                .and_then(|s| s.field("cover"))
                .expect("cover field");

            assert_eq!(cover.options(), &FieldOptions::File {
                folders: vec!["assets/covers".to_owned()],
                ext: Some("png".to_owned()),
                class: vec!["image".to_owned()],
            });
        }

        #[test]
        fn global_does_not_inherit_fields_from_its_own_declared_extends() {
            // Global is a flat reference pool, not a link in the `extends`
            // chain: a declared `extends` on `global.toml` itself must not be
            // honored, and not even warned about (`book` is a real Schema, so
            // this isn't a `MissingExtendsTarget`).
            let mut raw = BTreeMap::new();
            raw.insert(
                SchemaName::from("book"),
                schema(&[], &[("title", input_field())]),
            );
            raw.insert(
                SchemaName::from(GLOBAL_SCHEMA_NAME),
                schema(&["book"], &[(
                    "priority",
                    select_field(&["low", "high"]),
                )]),
            );

            let (resolved, warnings) = resolve(&raw).expect("resolves");

            assert!(warnings.is_empty());
            let global =
                resolved.get(GLOBAL_SCHEMA_NAME).expect("global resolved");
            assert!(
                global.field("title").is_none(),
                "global must not inherit from its own declared extends"
            );
        }

        #[test]
        fn global_resolves_before_a_sibling_that_refs_it_despite_declaring_extends()
         {
            // `book` is `global`'s declared (and ignored) `extends` parent, so
            // a naive Kahn in-degree would place `global` one tier after
            // `book` - after `poem`, which shares `book`'s tier-0 in-degree.
            // `build_dag` forces `global` to in-degree zero unconditionally,
            // so it must still resolve before `poem` needs it via `$ref`.
            let mut raw = BTreeMap::new();
            raw.insert(
                SchemaName::from("book"),
                schema(&[], &[("title", input_field())]),
            );
            raw.insert(
                SchemaName::from(GLOBAL_SCHEMA_NAME),
                schema(&["book"], &[(
                    "priority",
                    select_field(&["low", "high"]),
                )]),
            );
            raw.insert(
                SchemaName::from("poem"),
                schema(&[], &[(
                    "priority",
                    ref_field("#global/priority", None),
                )]),
            );

            let (resolved, _) = resolve(&raw).expect("resolves");

            let priority = resolved
                .get("poem")
                .and_then(|s| s.field("priority"))
                .expect("priority field resolves via $ref to global");
            assert_eq!(priority.options(), &FieldOptions::Select {
                values: vec!["low".to_owned(), "high".to_owned()]
            });
        }

        #[test]
        fn a_ref_to_global_resolves_when_the_referrer_sorts_before_it_alphabetically()
         {
            // Both Schemas have no `extends`, so both start at Kahn in-degree
            // zero. `"author"` sorts before `"global"` in the name-ordered
            // `BTreeMap` `resolve` iterates, so without the explicit
            // Global-first queue reorder, `author` would be popped (and
            // resolved) before `global`, and its `$ref` would fail.
            let mut raw = BTreeMap::new();
            raw.insert(
                SchemaName::from(GLOBAL_SCHEMA_NAME),
                schema(&[], &[("name", select_field(&["anon"]))]),
            );
            raw.insert(
                SchemaName::from("author"),
                schema(&[], &[("name", ref_field("#global/name", None))]),
            );

            let (resolved, _) = resolve(&raw).expect("resolves");

            let name = resolved
                .get("author")
                .and_then(|s| s.field("name"))
                .expect("name field resolves via $ref to global");
            assert_eq!(name.options(), &FieldOptions::Select {
                values: vec!["anon".to_owned()]
            });
        }

        #[test]
        fn a_ref_that_switches_field_type_starts_from_empty_base_options() {
            // Per `FieldOptions::merged`'s doc comment: a `$ref` that
            // switches `type` starts from empty options rather than reusing a
            // mismatched base, so a `select`'s `values` can never leak into
            // an overriding `file` field.
            let mut raw = BTreeMap::new();
            raw.insert(
                SchemaName::from("book"),
                schema(&[], &[("status", select_field(&["draft", "done"]))]),
            );
            raw.insert(
                SchemaName::from("sci_fi"),
                schema(&["book"], &[("status", RawFieldDef {
                    source: RawFieldSource::Ref {
                        address: field_address("#book/status"),
                        override_type: Some(RawFieldType::File),
                    },
                    folders: Some(vec!["assets".to_owned()]),
                    ..RawFieldDef::direct(RawFieldType::Input)
                })]),
            );

            let (resolved, _) = resolve(&raw).expect("resolves");

            let status = resolved
                .get("sci_fi")
                .and_then(|s| s.field("status"))
                .expect("status field");
            assert_eq!(status.options(), &FieldOptions::File {
                folders: vec!["assets".to_owned()],
                ext: None,
                class: Vec::new(),
            });
        }
    }

    mod ref_resolver {
        use pretty_assertions::assert_eq;

        use super::*;

        /// Builds a resolved [`Schema`] named `name` with one `field` field
        /// of the given `options`, keyed by `name` for a `resolved` map.
        fn schema_with_field(
            name: &str,
            field: &str,
            options: FieldOptions,
        ) -> (SchemaName, Schema) {
            let mut fields = BTreeMap::new();
            fields.insert(
                field_name(field),
                FieldDefinition::new(options, false, false),
            );
            (
                SchemaName::from(name),
                Schema::new(SchemaName::from(name), fields, BTreeSet::new()),
            )
        }

        #[test]
        fn resolves_a_field_from_an_ancestor_schema() {
            let (name, book) =
                schema_with_field("book", "status", FieldOptions::Input);
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
                FieldNameRef::try_from("status").expect("valid field name"),
            );

            let field = refs
                .resolve(address, &field_address("#book/status"))
                .expect("resolves");

            assert_eq!(field.options(), &FieldOptions::Input);
        }

        #[test]
        fn resolves_a_field_from_the_global_schema() {
            let (name, global) = schema_with_field(
                GLOBAL_SCHEMA_NAME,
                "priority",
                FieldOptions::Input,
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
                FieldNameRef::try_from("priority").expect("valid field name"),
            );

            let field = refs
                .resolve(address, &field_address("#global/priority"))
                .expect("resolves");

            assert_eq!(field.options(), &FieldOptions::Input);
        }

        #[test]
        fn rejects_a_reference_outside_the_bound() {
            let (name, movie) =
                schema_with_field("movie", "status", FieldOptions::Input);
            let mut resolved = BTreeMap::new();
            resolved.insert(name, movie);
            let ancestors = BTreeSet::new();
            let refs = RefResolver {
                ancestors: &ancestors,
                resolved: &resolved,
            };
            let address = FieldAddressRef::new(
                SchemaNameRef::from("book"),
                FieldNameRef::try_from("status").expect("valid field name"),
            );

            let err = refs
                .resolve(address, &field_address("#movie/status"))
                .expect_err("out-of-bounds rejected");

            assert!(matches!(err, SchemaError::RefOutOfBounds { .. }));
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
                FieldNameRef::try_from("status").expect("valid field name"),
            );

            let err = refs
                .resolve(address, &field_address("#book/status"))
                .expect_err("missing field rejected");

            assert!(matches!(err, SchemaError::RefFieldNotFound { .. }));
        }
    }
}
