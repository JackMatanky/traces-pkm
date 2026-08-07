//! Pure Field Resolution: linearizes the `extends` DAG and merges Field
//! Definitions.
//!
//! No filesystem or minijinja access: [`resolve`] is a pure function over an
//! already-parsed Schema set, mirroring how `index::query`'s `filter`/
//! `operators` submodules unit-test their expression machinery.
//!
//! Reads top-down: the pipeline ([`resolve`] -> [`resolve_one`] ->
//! [`build_field`]) comes first; the [`SchemaGraph`] (DAG bookkeeping) and
//! [`RefResolver`] (`$ref` bounds-checking) types backing it, plus the
//! `FieldPath` address type threaded through all three, follow below.
//!
//! # Main Type
//!
//! - [`resolve`]: Resolves an entire Schema set via Kahn's topological sort
//!   into [`super::model::Schema`]s. See [`super::model`] for the resolved
//!   domain shapes ([`Schema`], `FieldDefinition`, `FieldOptions`) this
//!   produces.
//!
//! [`Schema`]: super::model::Schema

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use super::{
    GLOBAL_SCHEMA_NAME,
    error::{SchemaError, SchemaWarning},
    model::{FieldDefinition, FieldOptions, FieldType, Schema},
    name::{SchemaName, SchemaNameRef},
    raw::{RawFieldDef, RawSchema},
};

/// Every Schema resolved by [`resolve`], keyed by name, alongside the
/// [`SchemaWarning`]s degraded resolution accumulated along the way.
type ResolveOutput = (BTreeMap<SchemaName, Schema>, Vec<SchemaWarning>);

/// Resolves `raw_schemas` into effective Field Definitions per Schema.
///
/// Linearizes the `extends` DAG with Kahn's topological sort, so every
/// Schema is resolved only after all of its (valid) parents. For each
/// Schema, in order: parent fields are merged first-listed-wins, `excludes`
/// drops named fields from that merge, then the Schema's own fields
/// (`$ref`-resolved against already-resolved Schemas) override the result.
///
/// # Errors
///
/// - [`SchemaError::MalformedRef`] if a `$ref` is not shaped
///   `#<schema>/<field>`.
/// - [`SchemaError::RefOutOfBounds`] if a `$ref` names a Schema outside its
///   bound (the Global Schema or a transitive `extends` ancestor).
/// - [`SchemaError::RefFieldNotFound`] if a `$ref` names an in-bounds Schema
///   that has no such field.
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "declared by the schema-registry ticket; consumed by the \
                  schema-namespace ticket \
                  (.scratch/metadata-schemas/issues/\
                  03-schema-minijinja-namespace.md)"
    )
)]
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
/// `parents`' fields first-listed-wins, applies `raw.excludes`, then
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
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "declared by the schema-registry ticket; consumed by the \
                  schema-namespace ticket \
                  (.scratch/metadata-schemas/issues/\
                  03-schema-minijinja-namespace.md)"
    )
)]
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
            field: field_name,
        };
        let field = build_field(at, raw_field, &refs, warnings)?;
        own_fields.insert(field_name.clone(), field);
    }
    fields.extend(own_fields);

    Ok(Schema::new(SchemaName::from(name), fields, ancestors))
}

/// Builds one resolved [`FieldDefinition`] for `at`, resolving its `$ref`
/// (if any) via `refs` and applying the Global Schema's `required` degrade.
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "declared by the schema-registry ticket; consumed by the \
                  schema-namespace ticket \
                  (.scratch/metadata-schemas/issues/\
                  03-schema-minijinja-namespace.md)"
    )
)]
fn build_field(
    at: FieldPath<'_>,
    raw: &RawFieldDef,
    refs: &RefResolver<'_>,
    warnings: &mut Vec<SchemaWarning>,
) -> Result<FieldDefinition, SchemaError> {
    let (options, required, multi) = if let Some(reference) = &raw.reference {
        let base = refs.resolve(at, reference)?;
        let field_type = raw
            .field_type
            .map_or_else(|| base.options().field_type(), FieldType::from);
        (
            FieldOptions::merged(base.options(), field_type, raw),
            raw.required.unwrap_or(base.is_required()),
            raw.multi.unwrap_or(base.is_multi()),
        )
    } else {
        let field_type =
            raw.field_type.map(FieldType::from).ok_or_else(|| {
                SchemaError::MissingFieldType {
                    schema: SchemaName::from(at.schema),
                    field: at.field.to_owned(),
                }
            })?;
        (
            FieldOptions::from_raw(field_type, raw),
            raw.required.unwrap_or(false),
            raw.multi.unwrap_or(false),
        )
    };

    Ok(FieldDefinition::new(
        options,
        apply_global_degrade(at, required, warnings),
        multi,
    ))
}

/// Global Schema fields can never be required: forces `required` to `false`
/// and records a [`SchemaWarning::StrayGlobalRequired`] when `at.schema` is
/// the Global Schema and it declared `required = true`.
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "declared by the schema-registry ticket; consumed by the \
                  schema-namespace ticket \
                  (.scratch/metadata-schemas/issues/\
                  03-schema-minijinja-namespace.md)"
    )
)]
fn apply_global_degrade(
    at: FieldPath<'_>,
    required: bool,
    warnings: &mut Vec<SchemaWarning>,
) -> bool {
    if at.schema.as_str() == GLOBAL_SCHEMA_NAME && required {
        warnings.push(SchemaWarning::StrayGlobalRequired {
            field: at.field.to_owned(),
        });
        false
    } else {
        required
    }
}

/// Kahn's-algorithm bookkeeping for linearizing the `extends` DAG: adjacency,
/// in-degrees, the ready queue, and which Schemas have been popped.
/// Isolates the Global-first tie-break (see [`SchemaGraph::new`]'s doc) from
/// the field-merge logic in [`resolve_one`].
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
    /// Filters each Schema's `extends` list to targets `raw_schemas` actually
    /// contains, pushing a [`SchemaWarning::MissingExtendsTarget`] for each
    /// miss. The reserved Global Schema is forced to in-degree zero and
    /// stripped of any declared `extends` — it is a flat,
    /// `$ref`-able-from-anywhere reference pool (ADR-7: "refs point up the
    /// extends DAG or to the Global Schema"), not a link in the `extends` chain
    /// itself.
    ///
    /// Kahn's sort only guarantees parent-before-child ordering along `extends`
    /// edges, so several Schemas can tie at in-degree zero with no defined
    /// order between them; the ready queue reorders Global to the front of that
    /// tier so it is popped (and resolved) before any sibling that might `$ref`
    /// it.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "declared by the schema-registry ticket; consumed by the \
                      schema-namespace ticket \
                      (.scratch/metadata-schemas/issues/\
                      03-schema-minijinja-namespace.md)"
        )
    )]
    fn new(
        raw_schemas: &'a BTreeMap<SchemaName, RawSchema>,
        warnings: &mut Vec<SchemaWarning>,
    ) -> Self {
        // `BTreeMap` iteration is name-sorted, so the parent order for a
        // given schema matches declaration order in its own `extends` list
        // while the overall processing order stays deterministic.
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

    /// Pops the next Schema whose in-degree reached zero, marking it
    /// visited, or `None` once the ready queue drains.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "declared by the schema-registry ticket; consumed by the \
                      schema-namespace ticket \
                      (.scratch/metadata-schemas/issues/\
                      03-schema-minijinja-namespace.md)"
        )
    )]
    fn next_ready(&mut self) -> Option<SchemaNameRef<'a>> {
        let name = self.queue.pop_front()?;
        self.visited.insert(name);
        Some(name)
    }

    /// Borrows `name`'s (filtered) parent list.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "declared by the schema-registry ticket; consumed by the \
                      schema-namespace ticket \
                      (.scratch/metadata-schemas/issues/\
                      03-schema-minijinja-namespace.md)"
        )
    )]
    fn parents_of(&self, name: SchemaNameRef<'_>) -> &[SchemaNameRef<'a>] {
        self.parents_by_name.get(name.as_str()).map_or(&[], Vec::as_slice)
    }

    /// Records `name` as resolved, releasing any children whose in-degree
    /// just hit zero into the ready queue.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "declared by the schema-registry ticket; consumed by the \
                      schema-namespace ticket \
                      (.scratch/metadata-schemas/issues/\
                      03-schema-minijinja-namespace.md)"
        )
    )]
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
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "declared by the schema-registry ticket; consumed by the \
                      schema-namespace ticket \
                      (.scratch/metadata-schemas/issues/\
                      03-schema-minijinja-namespace.md)"
        )
    )]
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
/// `extends` ancestors (spec: "refs point up the extends DAG or to the
/// Global Schema, so they are acyclic by construction").
struct RefResolver<'a> {
    ancestors: &'a BTreeSet<SchemaName>,
    resolved: &'a BTreeMap<SchemaName, Schema>,
}

impl<'a> RefResolver<'a> {
    /// Resolves `reference` (`at`'s own `$ref` value) to its base
    /// [`FieldDefinition`].
    ///
    /// # Errors
    ///
    /// - [`SchemaError::MalformedRef`] if `reference` is not shaped
    ///   `#<schema>/<field>`.
    /// - [`SchemaError::RefOutOfBounds`] if the named Schema is neither the
    ///   Global Schema nor a transitive `extends` ancestor of `at.schema`.
    /// - [`SchemaError::RefFieldNotFound`] if the named Schema is in bounds but
    ///   has no such field.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "declared by the schema-registry ticket; consumed by the \
                      schema-namespace ticket \
                      (.scratch/metadata-schemas/issues/\
                      03-schema-minijinja-namespace.md)"
        )
    )]
    fn resolve(
        &self,
        at: FieldPath<'_>,
        reference: &str,
    ) -> Result<&'a FieldDefinition, SchemaError> {
        let target = FieldPath::parse(reference).map_err(|ParseRefError| {
            SchemaError::MalformedRef {
                schema: SchemaName::from(at.schema),
                field: at.field.to_owned(),
                reference: reference.to_owned(),
            }
        })?;
        // Rejecting an out-of-bounds target here, rather than only checking
        // whether it happens to be resolved yet, keeps the bound
        // spec-accurate and independent of Kahn's tie-breaking order among
        // unrelated Schemas.
        if target.schema.as_str() != GLOBAL_SCHEMA_NAME
            && !self.ancestors.contains(target.schema.as_str())
        {
            return Err(SchemaError::RefOutOfBounds {
                schema: SchemaName::from(at.schema),
                field: at.field.to_owned(),
                reference: reference.to_owned(),
            });
        }
        self.resolved
            .get(target.schema.as_str())
            .and_then(|schema| schema.field(target.field))
            .ok_or_else(|| SchemaError::RefFieldNotFound {
                schema: SchemaName::from(at.schema),
                field: at.field.to_owned(),
                reference: reference.to_owned(),
            })
    }
}

/// A field's address within a Schema: `<schema>.<field>`, mirroring what a
/// `$ref` value (`#<schema>/<field>`) names. `schema` and `field` are
/// distinct types (not just distinct names), so a construction site like
/// `FieldPath { schema: field_name, field: name }` is a compile error, not
/// a silent transposition.
#[derive(Copy, Clone, Debug)]
struct FieldPath<'a> {
    schema: SchemaNameRef<'a>,
    field: &'a str,
}

impl<'a> FieldPath<'a> {
    /// Parses a `$ref` value (`#<schema>/<field>`) into its target
    /// `FieldPath`. Context-free — the caller attaches its own address on
    /// failure (see [`RefResolver::resolve`]), mirroring
    /// `index::query::field::FieldPath::parse`, this crate's established
    /// `Type::parse(&str) -> Result<Self, Error>` idiom.
    ///
    /// # Errors
    ///
    /// [`ParseRefError`] if `reference` is not shaped `#<schema>/<field>`
    /// with both segments non-empty.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "declared by the schema-registry ticket; consumed by the \
                      schema-namespace ticket \
                      (.scratch/metadata-schemas/issues/\
                      03-schema-minijinja-namespace.md)"
        )
    )]
    fn parse(reference: &'a str) -> Result<Self, ParseRefError> {
        let stripped = reference.strip_prefix('#').ok_or(ParseRefError)?;
        let (schema, field) = stripped.split_once('/').ok_or(ParseRefError)?;
        if schema.is_empty() || field.is_empty() {
            return Err(ParseRefError);
        }
        Ok(Self {
            schema: SchemaNameRef::from(schema),
            field,
        })
    }
}

/// `$ref` wasn't shaped `#<schema>/<field>` with both segments non-empty.
/// Discarded by the one caller, which rebuilds
/// [`SchemaError::MalformedRef`] with the referencing field's context
/// (mirrors [`crate::file_name::MissingFileName`]).
#[derive(Debug)]
struct ParseRefError;

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::schema::raw::RawFieldType;

    /// Builds a `select`-type [`RawFieldDef`] with the given `values`.
    fn select_field(values: &[&str]) -> RawFieldDef {
        RawFieldDef {
            field_type: Some(RawFieldType::Select),
            values: Some(values.iter().map(|&v| v.to_owned()).collect()),
            ..RawFieldDef::default()
        }
    }

    /// Builds an `input`-type [`RawFieldDef`].
    fn input_field() -> RawFieldDef {
        RawFieldDef {
            field_type: Some(RawFieldType::Input),
            ..RawFieldDef::default()
        }
    }

    /// Builds a `file`-type [`RawFieldDef`] with the given filter.
    fn file_field(
        folders: &[&str],
        ext: Option<&str>,
        class: &[&str],
    ) -> RawFieldDef {
        RawFieldDef {
            field_type: Some(RawFieldType::File),
            folders: Some(folders.iter().map(|&f| f.to_owned()).collect()),
            ext: ext.map(str::to_owned),
            class: Some(class.iter().map(|&c| c.to_owned()).collect()),
            ..RawFieldDef::default()
        }
    }

    /// Builds a `$ref`-only [`RawFieldDef`] with an optional local
    /// `required` override.
    fn ref_field(reference: &str, required: Option<bool>) -> RawFieldDef {
        RawFieldDef {
            reference: Some(reference.to_owned()),
            required,
            ..RawFieldDef::default()
        }
    }

    fn schema(extends: &[&str], fields: &[(&str, RawFieldDef)]) -> RawSchema {
        RawSchema {
            extends: extends.iter().map(|&s| SchemaName::from(s)).collect(),
            excludes: Vec::new(),
            fields: fields
                .iter()
                .cloned()
                .map(|(name, def)| (name.to_owned(), def))
                .collect(),
        }
    }

    fn schema_with_excludes(
        extends: &[&str],
        excludes: &[&str],
        fields: &[(&str, RawFieldDef)],
    ) -> RawSchema {
        RawSchema {
            excludes: excludes.iter().map(|&s| s.to_owned()).collect(),
            ..schema(extends, fields)
        }
    }

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
            sci_fi.fields().keys().map(String::as_str).collect();
        assert_eq!(field_names, vec!["author"]);
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
        let sci_fi = resolved.get("sci_fi").expect("own fields still render");
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
    fn a_malformed_ref_is_a_hard_error() {
        let mut raw = BTreeMap::new();
        raw.insert(
            SchemaName::from("book"),
            schema(&[], &[("status", ref_field("book/status", None))]),
        );

        let err = resolve(&raw).expect_err("malformed ref rejected");
        assert!(matches!(err, SchemaError::MalformedRef { .. }));
    }

    #[test]
    fn a_ref_to_an_unknown_field_is_a_hard_error() {
        let mut raw = BTreeMap::new();
        raw.insert(SchemaName::from("book"), schema(&[], &[]));
        raw.insert(
            SchemaName::from("sci_fi"),
            schema(&["book"], &[("status", ref_field("#book/status", None))]),
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
    fn schema_error_stays_small() {
        // Regression guard (mem-assert-type-size): `UnresolvedRef` used to
        // carry 5 owned Strings (120 bytes) because `ref_schema`/`ref_field`
        // duplicated what `reference` already shows verbatim. Keep every
        // variant's payload small enough that `Result<_, SchemaError>`
        // stays cheap to move through the resolution call chain.
        assert!(
            std::mem::size_of::<SchemaError>() <= 80,
            "SchemaError grew to {} bytes; box or trim the offending variant",
            std::mem::size_of::<SchemaError>()
        );
    }

    #[test]
    fn a_field_with_neither_type_nor_ref_is_a_hard_error() {
        let mut raw = BTreeMap::new();
        raw.insert(
            SchemaName::from("book"),
            schema(&[], &[("status", RawFieldDef::default())]),
        );

        let err = resolve(&raw).expect_err("missing type rejected");
        assert!(matches!(err, SchemaError::MissingFieldType { .. }));
    }

    #[test]
    fn every_field_type_resolves_its_own_options() {
        let mut raw = BTreeMap::new();
        raw.insert(
            SchemaName::from("book"),
            schema(&[], &[
                ("title", input_field()),
                ("status", select_field(&["draft", "done"])),
                ("archived", RawFieldDef {
                    field_type: Some(RawFieldType::Boolean),
                    ..RawFieldDef::default()
                }),
                ("rating", RawFieldDef {
                    field_type: Some(RawFieldType::Number),
                    ..RawFieldDef::default()
                }),
                ("published", RawFieldDef {
                    field_type: Some(RawFieldType::Date),
                    ..RawFieldDef::default()
                }),
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
                reference: Some("#global/cover".to_owned()),
                folders: Some(vec!["assets/covers".to_owned()]),
                ..RawFieldDef::default()
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
        // Global is a flat reference pool (ADR-7), not a link in the
        // `extends` chain: a declared `extends` on `global.toml` itself
        // must not be honored, and not even warned about (`book` is a
        // real Schema, so this isn't a `MissingExtendsTarget`).
        let mut raw = BTreeMap::new();
        raw.insert(
            SchemaName::from("book"),
            schema(&[], &[("title", input_field())]),
        );
        raw.insert(
            SchemaName::from(GLOBAL_SCHEMA_NAME),
            schema(&["book"], &[("priority", select_field(&["low", "high"]))]),
        );

        let (resolved, warnings) = resolve(&raw).expect("resolves");

        assert!(warnings.is_empty());
        let global = resolved.get(GLOBAL_SCHEMA_NAME).expect("global resolved");
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
            schema(&["book"], &[("priority", select_field(&["low", "high"]))]),
        );
        raw.insert(
            SchemaName::from("poem"),
            schema(&[], &[("priority", ref_field("#global/priority", None))]),
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
                reference: Some("#book/status".to_owned()),
                field_type: Some(RawFieldType::File),
                folders: Some(vec!["assets".to_owned()]),
                ..RawFieldDef::default()
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
