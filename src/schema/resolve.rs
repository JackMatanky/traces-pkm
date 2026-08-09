//! Resolves raw Schema sets into effective Field Definitions via Kahn's
//! topological sort.
//!
//! No filesystem or minijinja access: [`resolve`] is a pure function over an
//! already-parsed Schema set. Reads top-down: the pipeline ([`resolve`] ->
//! [`resolve_one`] -> [`build_field`]) comes first; [`SchemaGraph`] (DAG
//! bookkeeping), [`RefResolver`] (`$ref` bounds-checking), and `FieldPath`
//! (address type threaded through all three) follow below.
//!
//! # Main Type
//!
//! - [`resolve`]: linearizes the `extends` DAG into [`super::model::Schema`]s.
//!   See [`super::model`] for the resolved domain shapes ([`Schema`],
//!   [`super::model::FieldDefinition`], [`super::model::FieldOptions`]) this
//!   produces.
//!
//! [`Schema`]: super::model::Schema

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use super::{
    GLOBAL_SCHEMA_NAME,
    error::{SchemaError, SchemaWarning},
    model::{FieldDefinition, FieldOptions, FieldType, Schema},
    name::{SchemaName, SchemaNameRef},
    raw::{FieldRef, FieldSource, RawFieldDef, RawSchema},
};
use crate::field::{FieldName, FieldNameRef};

/// Every Schema resolved by [`resolve`], keyed by name, alongside the
/// [`SchemaWarning`]s degraded resolution accumulated along the way.
type ResolveOutput = (BTreeMap<SchemaName, Schema>, Vec<SchemaWarning>);

/// Resolves `raw_schemas` into effective Field Definitions per Schema.
///
/// Linearizes the `extends` DAG with Kahn's topological sort, so every Schema
/// is resolved only after all of its (valid) parents. For each Schema, in
/// order: parent fields are merged first-listed-wins, `excludes` drops named
/// fields from that merge, then the Schema's own fields (`$ref`-resolved
/// against already-resolved Schemas) override the result.
///
/// # Errors
///
/// - [`SchemaError::RefOutOfBounds`] if a `$ref` names a Schema outside its
///   bound (the Global Schema or a transitive `extends` ancestor).
/// - [`SchemaError::RefFieldNotFound`] if a `$ref` names an in-bounds Schema
///   that has no such field.
/// - [`SchemaError::AmbiguousFieldName`] if two of a Schema's effective fields
///   share a [`FieldKey`] canonical form.
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
        let schema = resolve_one(
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

/// Resolves one Schema's effective fields and transitive ancestors: merges
/// `parents`' fields first-listed-wins, applies `raw.excludes`, then overrides
/// the result with `raw`'s own (`$ref`-resolved) fields.
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
/// Propagates any [`SchemaError`] [`build_field`] returns while resolving
/// `raw`'s own fields, or [`SchemaError::AmbiguousFieldName`] if two of the
/// resolved fields share a [`FieldKey`] canonical form.
fn resolve_one(
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
        let at = FieldPath {
            schema: name,
            field: field_name.as_ref(),
        };
        let field = build_field(at, raw_field, &refs, warnings)?;
        own_fields.insert(field_name.clone(), field);
    }
    fields.extend(own_fields);

    reject_ambiguous_canonical_names(name, &fields)?;

    Ok(Schema::new(SchemaName::from(name), fields, ancestors))
}

/// Rejects `fields` if two entries share a [`FieldKey`] canonical form:
/// ambiguous field identities would make later note-vs-schema field matching
/// and unknown-field suggestions unreliable.
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
                second: field_name.clone(),
            });
        }
        seen.insert(canonical, field_name.clone());
    }
    Ok(())
}

/// Builds one resolved [`FieldDefinition`] for `at`, resolving its `$ref`
/// (if any) via `refs` and applying the Global Schema's `required` degrade.
///
/// # Errors
///
/// Any error [`RefResolver::resolve`] returns while resolving a `$ref`.
fn build_field(
    at: FieldPath<'_>,
    raw: &RawFieldDef,
    refs: &RefResolver<'_>,
    warnings: &mut Vec<SchemaWarning>,
) -> Result<FieldDefinition, SchemaError> {
    let (options, required, multi) = match &raw.source {
        FieldSource::Ref {
            reference,
            override_type,
        } => {
            let base = refs.resolve(at, reference)?;
            let field_type = (*override_type)
                .map_or_else(|| base.options().kind(), FieldType::from);
            (
                FieldOptions::merged(base.options(), field_type, raw),
                raw.required.unwrap_or(base.is_required()),
                raw.multi.unwrap_or(base.is_multi()),
            )
        }
        FieldSource::Direct(raw_type) => {
            let field_type = FieldType::from(*raw_type);
            (
                FieldOptions::from_raw(field_type, raw),
                raw.required.unwrap_or(false),
                raw.multi.unwrap_or(false),
            )
        }
    };

    Ok(FieldDefinition::new(
        options,
        apply_global_degrade(at, required, warnings),
        multi,
    ))
}

/// Global Schema fields can never be required: forces `required` to `false` and
/// records a [`SchemaWarning::StrayGlobalRequired`] when `at.schema` is the
/// Global Schema and it declared `required = true`.
fn apply_global_degrade(
    at: FieldPath<'_>,
    required: bool,
    warnings: &mut Vec<SchemaWarning>,
) -> bool {
    if at.schema.as_str() == GLOBAL_SCHEMA_NAME && required {
        warnings.push(SchemaWarning::StrayGlobalRequired {
            field: at.field.as_str().to_owned(),
        });
        false
    } else {
        required
    }
}

/// Kahn's-algorithm bookkeeping for linearizing the `extends` DAG: adjacency,
/// in-degrees, the ready queue, and which Schemas have been popped.
/// Isolates the Global-first tie-break from the field-merge logic in
/// [`resolve_one`].
struct SchemaGraph<'a> {
    parents_by_name: BTreeMap<SchemaNameRef<'a>, Vec<SchemaNameRef<'a>>>,
    in_degree: BTreeMap<SchemaNameRef<'a>, usize>,
    children_by_name: BTreeMap<SchemaNameRef<'a>, Vec<SchemaNameRef<'a>>>,
    queue: VecDeque<SchemaNameRef<'a>>,
    visited: BTreeSet<SchemaNameRef<'a>>,
}

impl<'a> SchemaGraph<'a> {
    /// Builds `raw_schemas`' `extends` adjacency (parent -> children) and Kahn
    /// in-degrees, seeding the ready queue with every in-degree-zero Schema.
    ///
    /// Filtering and tie-breaking:
    ///
    /// - Each Schema's `extends` list is filtered to targets `raw_schemas`
    ///   actually contains; a missing target emits
    ///   [`SchemaWarning::MissingExtendsTarget`].
    /// - The reserved Global Schema is forced to in-degree zero and stripped of
    ///   any declared `extends`. It is a flat `$ref`-able reference pool, not a
    ///   link in the `extends` chain.
    /// - Kahn's sort only guarantees parent-before-child ordering along
    ///   `extends` edges, so several Schemas can tie at in-degree zero. The
    ///   ready queue reorders Global to the front of that tier so it resolves
    ///   before any sibling that might `$ref` it.
    fn new(
        raw_schemas: &'a BTreeMap<SchemaName, RawSchema>,
        warnings: &mut Vec<SchemaWarning>,
    ) -> Self {
        // `BTreeMap` iteration is name-sorted, so the parent order for a given
        // schema matches declaration order in its own `extends` list while the
        // overall processing order stays deterministic.
        let mut parents_by_name: BTreeMap<
            SchemaNameRef<'_>,
            Vec<SchemaNameRef<'_>>,
        > = BTreeMap::new();
        for (name, raw) in raw_schemas {
            let mut parents = Vec::with_capacity(raw.extends.len());
            for target in &raw.extends {
                if raw_schemas.contains_key(target) {
                    parents.push(target.as_ref());
                } else {
                    warnings.push(SchemaWarning::MissingExtendsTarget {
                        schema: name.clone(),
                        target: target.clone(),
                    });
                }
            }
            parents_by_name.insert(name.as_ref(), parents);
        }

        // An edge runs parent -> child, so a child's in-degree is its
        // (filtered) parent count.
        let mut in_degree: BTreeMap<SchemaNameRef<'_>, usize> = BTreeMap::new();
        let mut children_by_name: BTreeMap<
            SchemaNameRef<'_>,
            Vec<SchemaNameRef<'_>>,
        > = BTreeMap::new();
        for (&name, parents) in &parents_by_name {
            in_degree.insert(name, parents.len());
            for &parent in parents {
                children_by_name.entry(parent).or_default().push(name);
            }
        }

        if let Some(parents) = parents_by_name.get_mut(GLOBAL_SCHEMA_NAME) {
            parents.clear();
        }
        if let Some(degree) = in_degree.get_mut(GLOBAL_SCHEMA_NAME) {
            *degree = 0;
        }

        let mut queue: VecDeque<SchemaNameRef<'_>> = in_degree
            .iter()
            .filter(|&(_, &degree)| degree == 0)
            .map(|(&name, _)| name)
            .collect();
        if let Some(position) =
            queue.iter().position(|&name| name.as_str() == GLOBAL_SCHEMA_NAME)
            && let Some(global) = queue.remove(position)
        {
            queue.push_front(global);
        }

        Self {
            parents_by_name,
            in_degree,
            children_by_name,
            queue,
            visited: BTreeSet::new(),
        }
    }

    /// Pops the next Schema whose in-degree reached zero, marking it visited,
    /// or `None` once the ready queue drains.
    fn next_ready(&mut self) -> Option<SchemaNameRef<'a>> {
        let name = self.queue.pop_front()?;
        self.visited.insert(name);
        Some(name)
    }

    /// Borrows `name`'s (filtered) parent list.
    fn parents_of(&self, name: SchemaNameRef<'_>) -> &[SchemaNameRef<'a>] {
        self.parents_by_name.get(name.as_str()).map_or(&[], Vec::as_slice)
    }

    /// Records `name` as resolved, releasing any children whose in-degree just
    /// hit zero into the ready queue.
    fn mark_resolved(&mut self, name: SchemaNameRef<'_>) {
        for &child in
            self.children_by_name.get(name.as_str()).into_iter().flatten()
        {
            if let Some(degree) = self.in_degree.get_mut(&child) {
                *degree = degree.saturating_sub(1);
                if *degree == 0 {
                    self.queue.push_back(child);
                }
            }
        }
    }

    /// Returns every Schema name that never reached in-degree zero (a cycle
    /// membership), or `None` if every Schema in `raw_schemas` was visited.
    fn cyclic_remainder(
        &self,
        raw_schemas: &BTreeMap<SchemaName, RawSchema>,
    ) -> Option<Vec<SchemaName>> {
        if self.visited.len() == raw_schemas.len() {
            return None;
        }
        Some(
            raw_schemas
                .keys()
                .filter(|name| !self.visited.contains(name.as_str()))
                .cloned()
                .collect(),
        )
    }
}

/// Resolves a Schema's `$ref` values to their base [`FieldDefinition`]s,
/// bounded to the Global Schema or the referencing Schema's transitive
/// `extends` ancestors: `$ref`s point up the `extends` DAG or to the Global
/// Schema, so they are acyclic by construction.
struct RefResolver<'a> {
    ancestors: &'a BTreeSet<SchemaName>,
    resolved: &'a BTreeMap<SchemaName, Schema>,
}

impl<'a> RefResolver<'a> {
    /// Resolves `reference` (`at`'s own already-parsed `$ref` value) to its
    /// base [`FieldDefinition`].
    ///
    /// # Errors
    ///
    /// - [`SchemaError::RefOutOfBounds`] if the named Schema is neither the
    ///   Global Schema nor a transitive `extends` ancestor of `at.schema`.
    /// - [`SchemaError::RefFieldNotFound`] if the named Schema is in bounds but
    ///   has no such field.
    fn resolve(
        &self,
        at: FieldPath<'_>,
        reference: &FieldRef,
    ) -> Result<&'a FieldDefinition, SchemaError> {
        // Rejecting an out-of-bounds target here, rather than only checking
        // whether it happens to be resolved yet, keeps the bound spec-accurate
        // and independent of Kahn's tie-breaking order among unrelated Schemas.
        if reference.schema().as_str() != GLOBAL_SCHEMA_NAME
            && !self.ancestors.contains(reference.schema().as_str())
        {
            return Err(SchemaError::RefOutOfBounds {
                schema: SchemaName::from(at.schema),
                field: FieldName::from(at.field),
                reference: Box::new(reference.clone()),
            });
        }
        self.resolved
            .get(reference.schema().as_str())
            .and_then(|schema| schema.field(reference.field().as_str()))
            .ok_or_else(|| SchemaError::RefFieldNotFound {
                schema: SchemaName::from(at.schema),
                field: FieldName::from(at.field),
                reference: Box::new(reference.clone()),
            })
    }
}

/// A field's address within a Schema: `<schema>.<field>`, mirroring what a
/// `$ref` value (`#<schema>/<field>`) names. `schema` and `field` are distinct
/// types (not just distinct names), so a construction site like `FieldPath {
/// schema: field_name, field: name }` is a compile error, not a silent
/// transposition.
#[derive(Copy, Clone, Debug)]
struct FieldPath<'a> {
    schema: SchemaNameRef<'a>,
    field: FieldNameRef<'a>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::raw::RawFieldType;

    /// Parses `name` into a [`FieldName`], panicking on an invalid test
    /// fixture.
    fn field_name(name: &str) -> FieldName {
        FieldName::try_from(name).expect("valid test field name")
    }

    /// Parses `reference` into a [`FieldRef`], panicking on an invalid test
    /// fixture.
    fn field_ref(reference: &str) -> FieldRef {
        FieldRef::try_from(reference).expect("valid test $ref")
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
            ..RawFieldDef::reference(field_ref(reference))
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
                Some(&FieldOptions::Number)
            );
            assert_eq!(
                book.field("published").map(FieldDefinition::options),
                Some(&FieldOptions::Date)
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
                    ..RawFieldDef::reference(field_ref("#global/cover"))
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
                    source: FieldSource::Ref {
                        reference: field_ref("#book/status"),
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

    mod schema_graph {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn schema_graph_pops_global_before_an_alphabetically_earlier_sibling() {
            // Isolates the Global-first reorder at the graph level, without
            // going through field resolution: `"author"` sorts before
            // `"global"` in name order, so this fails without the reorder.
            let mut raw = BTreeMap::new();
            raw.insert(SchemaName::from(GLOBAL_SCHEMA_NAME), schema(&[], &[]));
            raw.insert(SchemaName::from("author"), schema(&[], &[]));
            let mut warnings = Vec::new();

            let mut graph = SchemaGraph::new(&raw, &mut warnings);

            assert_eq!(
                graph.next_ready(),
                Some(SchemaNameRef::from(GLOBAL_SCHEMA_NAME))
            );
            assert_eq!(graph.next_ready(), Some(SchemaNameRef::from("author")));
            assert_eq!(graph.next_ready(), None);
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
            let at = FieldPath {
                schema: SchemaNameRef::from("sci_fi"),
                field: FieldNameRef::try_from("status")
                    .expect("valid field name"),
            };

            let field =
                refs.resolve(at, &field_ref("#book/status")).expect("resolves");

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
            let at = FieldPath {
                schema: SchemaNameRef::from("task"),
                field: FieldNameRef::try_from("priority")
                    .expect("valid field name"),
            };

            let field = refs
                .resolve(at, &field_ref("#global/priority"))
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
            let at = FieldPath {
                schema: SchemaNameRef::from("book"),
                field: FieldNameRef::try_from("status")
                    .expect("valid field name"),
            };

            let err = refs
                .resolve(at, &field_ref("#movie/status"))
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
            let at = FieldPath {
                schema: SchemaNameRef::from("sci_fi"),
                field: FieldNameRef::try_from("status")
                    .expect("valid field name"),
            };

            let err = refs
                .resolve(at, &field_ref("#book/status"))
                .expect_err("missing field rejected");

            assert!(matches!(err, SchemaError::RefFieldNotFound { .. }));
        }
    }
}
