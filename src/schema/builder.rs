//! Schema resolution: linearize the `extends` DAG, merge inherited fields, and
//! compute hierarchy sets.
//!
//! [`SchemaBuilder`] is the single-method entry point. Internally it:
//!
//! 1. Resolves the Global Schema first (a `$ref` target, not an `extends`
//!    target) so later `$ref` lookups find it.
//! 2. Drives [`SchemaGraphBuilder`]/[`SchemaGraph`]: Kahn's topological sort
//!    yields schemas in dependency order, with the Global Schema excluded from
//!    the graph entirely.
//! 3. Delegates per-Schema field merging to [`SchemaMerger`], which applies
//!    first-listed-wins inheritance, `excludes`, and `$ref` resolution.
//! 4. Filters hierarchy sets against resolution failures so broken links do not
//!    propagate downstream.
//!
//! [`SchemaGraphBuilder`]: super::graph::SchemaGraphBuilder
//! [`SchemaGraph`]: super::graph::SchemaGraph

use std::collections::HashMap;

use indexmap::{IndexMap, IndexSet};

use super::{
    GLOBAL_SCHEMA_NAME, RawSchema, SchemaName, SchemaNameRef,
    error::{SchemaError, SchemaWarning},
    fields::{FieldAddressRef, SchemaFieldBuilder, SelectValuesFileCache},
    graph::{SchemaGraph, SchemaGraphBuilder},
    model::Schema,
};
use crate::field::{FieldKey, FieldName};

/// A Schema that failed to resolve, with its [`SchemaError`].
#[derive(Debug)]
pub(crate) struct SchemaFailure {
    pub(crate) schema: SchemaName,
    pub(crate) error: SchemaError,
}

/// Output of a successful resolution pass.
///
/// Contains every Schema that resolved without error, alongside warnings for
/// recoverable defects and failures for hard errors.
#[derive(Debug)]
pub(super) struct ResolvedSchemas {
    /// Every Schema that resolved without error.
    pub(super) schemas: IndexMap<SchemaName, Schema>,
    /// Recoverable defects encountered across the whole set.
    pub(super) warnings: Vec<SchemaWarning>,
    /// Schemas that failed to resolve, each paired with the error that caused
    /// the failure.
    pub(super) failures: Vec<SchemaFailure>,
}

/// Entry point for resolving a set of raw schemas into [`Schema`] values.
///
/// Build with [`new`](Self::new), then call [`build`](Self::build) to produce
/// [`ResolvedSchemas`].
pub(super) struct SchemaBuilder<'a> {
    /// Raw schemas to resolve, keyed by name.
    raw: &'a IndexMap<SchemaName, RawSchema>,
    /// Confined values file cache for `select`/`multi` fields.
    values_cache: &'a SelectValuesFileCache,
    /// Schemas resolved so far.
    resolved: IndexMap<SchemaName, Schema>,
    /// Recoverable defects accumulated across the whole set.
    warnings: Vec<SchemaWarning>,
    /// Schemas that failed to resolve, accumulated across the whole set.
    failures: Vec<SchemaFailure>,
}

impl<'a> SchemaBuilder<'a> {
    /// Create a builder from a borrowed raw-schema map and values file cache.
    pub(super) fn new(
        raw: &'a IndexMap<SchemaName, RawSchema>,
        values_cache: &'a SelectValuesFileCache,
    ) -> Self {
        Self {
            raw,
            values_cache,
            resolved: IndexMap::new(),
            warnings: Vec::new(),
            failures: Vec::new(),
        }
    }

    /// Resolve every Schema, returning successes, warnings, and failures.
    ///
    /// The Global Schema is resolved first (its `extends` is ignored) so
    /// later `$ref` lookups find it. Remaining schemas are resolved in
    /// Kahn topological order, with the Global Schema excluded from the
    /// graph entirely.
    ///
    /// # Errors
    ///
    /// - [`Cycle`] if the `extends` DAG contains a cycle
    ///
    /// # Warnings
    ///
    /// - [`MissingExtendsTarget`] if an `extends` target has no corresponding
    ///   Schema file
    /// - [`DuplicateExtendsTarget`] if the same `extends` target appears more
    ///   than once
    /// - [`ParentFailedToResolve`] if a declared parent failed to resolve
    /// - [`StrayGlobalRequired`] if the Global Schema declares `required =
    ///   true`
    /// - [`UnknownOverrideKey`] if a `$ref` override has an unrecognized
    ///   attribute key
    /// - [`OverrideValueTypeMismatch`] if a `$ref` override attribute has the
    ///   wrong value type
    ///
    /// [`Cycle`]: SchemaError::Cycle
    /// [`MissingExtendsTarget`]: SchemaWarning::MissingExtendsTarget
    /// [`DuplicateExtendsTarget`]: SchemaWarning::DuplicateExtendsTarget
    /// [`ParentFailedToResolve`]: SchemaWarning::ParentFailedToResolve
    /// [`StrayGlobalRequired`]: SchemaWarning::StrayGlobalRequired
    /// [`UnknownOverrideKey`]: SchemaWarning::UnknownOverrideKey
    /// [`OverrideValueTypeMismatch`]: SchemaWarning::OverrideValueTypeMismatch
    pub(super) fn build(mut self) -> Result<ResolvedSchemas, SchemaError> {
        self.resolve_global()?;
        let (graph_builder, graph_warnings) = SchemaGraphBuilder::new(
            self.raw,
            SchemaNameRef::from(GLOBAL_SCHEMA_NAME),
        );
        self.warnings.extend(graph_warnings);
        let graph =
            graph_builder.build().map_err(|schemas| SchemaError::Cycle {
                schemas,
            })?;
        self.merge_in_topological_order(&graph);
        self.compute_hierarchy(&graph);
        Ok(self.finish())
    }

    /// Merge the Global Schema first, so later `$ref` lookups against it
    /// succeed regardless of raw-map order. Its `extends`, if declared, is
    /// ignored: it is a `$ref` target, never a DAG participant.
    fn resolve_global(&mut self) -> Result<(), SchemaError> {
        let Some(global_raw) = self.raw.get(GLOBAL_SCHEMA_NAME) else {
            return Ok(());
        };
        let (schema, warnings) = SchemaMerger::merge(
            SchemaNameRef::from(GLOBAL_SCHEMA_NAME),
            global_raw,
            &[],
            &self.resolved,
            self.values_cache,
        )?;
        self.warnings.extend(warnings);
        self.resolved.insert(SchemaName::from(GLOBAL_SCHEMA_NAME), schema);
        Ok(())
    }

    /// Merge every non-Global Schema's fields in `graph`'s topological order,
    /// so each Schema's `extends` parents are already in `self.resolved` when
    /// it merges.
    ///
    /// # Panics
    ///
    /// Never in practice: `graph` is built exclusively from `self.raw`'s own
    /// keys (minus the excluded Global Schema), so every name
    /// `graph.topological_order()` yields is present in `self.raw`.
    fn merge_in_topological_order(&mut self, graph: &SchemaGraph<'a>) {
        for name in graph.topological_order() {
            #[expect(
                clippy::expect_used,
                reason = "SchemaGraphBuilder::new/build build every node \
                          exclusively from self.raw's own keys minus the \
                          excluded Global Schema, so topological_order() can \
                          never yield a name absent from self.raw; failure \
                          here means the graph itself is broken, not a \
                          recoverable caller error"
            )]
            let raw_schema = self.raw.get(name.as_str()).expect(
                "SchemaGraph::topological_order only yields names present in \
                 raw",
            );
            match SchemaMerger::merge(
                name,
                raw_schema,
                graph.parents_of(name),
                &self.resolved,
                self.values_cache,
            ) {
                Ok((schema, schema_warnings)) => {
                    self.warnings.extend(schema_warnings);
                    self.resolved.insert(SchemaName::from(name), schema);
                }
                Err(error) => {
                    self.failures.push(SchemaFailure {
                        schema: SchemaName::from(name),
                        error,
                    });
                }
            }
        }
    }

    /// Assign direct children and transitive descendants to each resolved
    /// Schema, filtering out schemas that are not structural descendants due
    /// to failed resolution links.
    ///
    /// Two passes (compute, then apply) avoid needing a full-registry
    /// ancestor-set snapshot: the first pass only reads `self.resolved`,
    /// the second only writes to it.
    fn compute_hierarchy(&mut self, graph: &SchemaGraph<'a>) {
        /// One resolved Schema's recomputed direct children and transitive
        /// descendants, staged before being written back to `self.resolved`.
        type HierarchyUpdate =
            (SchemaName, IndexSet<SchemaName>, IndexSet<SchemaName>);

        let children_by_name = graph.children_by_name();
        let descendants_by_name = graph.descendants_by_name();
        let hierarchy: Vec<HierarchyUpdate> = self
            .resolved
            .keys()
            .map(|name| {
                let children: IndexSet<SchemaName> = children_by_name
                    .get(name.as_str())
                    .into_iter()
                    .flatten()
                    .map(|&child| SchemaName::from(child))
                    .filter(|child| self.still_descends_from(child, name))
                    .collect();
                let descendants: IndexSet<SchemaName> = descendants_by_name
                    .get(name)
                    .into_iter()
                    .flatten()
                    .filter(|descendant| {
                        self.still_descends_from(descendant, name)
                    })
                    .cloned()
                    .collect();
                (name.clone(), children, descendants)
            })
            .collect();
        for (name, children, descendants) in hierarchy {
            if let Some(schema) = self.resolved.get_mut(&name) {
                schema.set_hierarchy(children, descendants);
            }
        }
    }

    /// Whether `candidate` still structurally descends from `name` (its
    /// resolved `ancestors` still contains `name`); a failed parent link can
    /// drop a Schema from a hierarchy set it would otherwise belong to.
    fn still_descends_from(
        &self,
        candidate: &SchemaName,
        name: &SchemaName,
    ) -> bool {
        self.resolved
            .get(candidate)
            .is_some_and(|schema| schema.ancestors().contains(name))
    }

    fn finish(self) -> ResolvedSchemas {
        ResolvedSchemas {
            schemas: self.resolved,
            warnings: self.warnings,
            failures: self.failures,
        }
    }
}

/// Merges one schema's effective field map: parent inheritance
/// (first-listed-wins), `excludes`, and own `$ref`-resolved fields, into a
/// resolved [`Schema`]. Contrast with [`SchemaBuilder`], which orchestrates
/// every schema in the set.
struct SchemaMerger<'a> {
    /// The Schema being merged.
    name: SchemaNameRef<'a>,
    /// Effective field map accumulated so far: inherited fields, then own
    /// fields on top.
    fields: IndexMap<FieldName, super::fields::SchemaFieldDef>,
    /// Direct parents plus their own transitive ancestors.
    ancestors: IndexSet<SchemaName>,
    /// Recoverable defects accumulated across inheritance and field
    /// resolution.
    warnings: Vec<SchemaWarning>,
}

impl<'a> SchemaMerger<'a> {
    /// One-shot entry point: inherit from `parents`, apply `raw.excludes`,
    /// resolve `raw`'s own fields, and yield the merged [`Schema`].
    ///
    /// `parents` must already be resolved in `resolved`: Kahn's topological
    /// sort guarantees this ordering.
    ///
    /// # Arguments
    ///
    /// * `name` - the Schema being merged
    /// * `raw` - the raw Schema definition to resolve
    /// * `parents` - already-resolved parent Schema names in extend order
    /// * `resolved` - all Schema names resolved so far (parents must be
    ///   present)
    ///
    /// # Errors
    ///
    /// - [`FieldBuilder`] if a `$ref` in `raw.fields` fails to resolve or
    ///   validate
    /// - [`AmbiguousFieldName`] if two effective fields canonicalize to the
    ///   same [`FieldKey`]
    ///
    /// [`FieldBuilder`]: SchemaError::FieldBuilder
    /// [`AmbiguousFieldName`]: SchemaError::AmbiguousFieldName
    /// [`FieldKey`]: crate::field::FieldKey
    fn merge(
        name: SchemaNameRef<'a>,
        raw: &RawSchema,
        parents: &[SchemaName],
        resolved: &IndexMap<SchemaName, Schema>,
        values_cache: &'a SelectValuesFileCache,
    ) -> Result<(Schema, Vec<SchemaWarning>), SchemaError> {
        let mut merger = Self::new(name);
        merger.inherit_from(parents, resolved);
        merger.exclude(&raw.excludes);
        merger.resolve_own_fields(raw, resolved, values_cache)?;
        merger.into_schema()
    }

    /// Starts an empty merger for `name` with no inherited fields or ancestors.
    fn new(name: SchemaNameRef<'a>) -> Self {
        Self {
            name,
            fields: IndexMap::new(),
            ancestors: IndexSet::new(),
            warnings: Vec::new(),
        }
    }

    /// First-listed-wins inheritance from `parents`, each already resolved in
    /// `resolved`.
    fn inherit_from(
        &mut self,
        parents: &[SchemaName],
        resolved: &IndexMap<SchemaName, Schema>,
    ) {
        for parent in parents {
            let Some(parent_schema) = resolved.get(parent.as_str()) else {
                self.warnings.push(SchemaWarning::ParentFailedToResolve {
                    schema: SchemaName::from(self.name),
                    parent: parent.clone(),
                });
                continue;
            };
            for (field_name, field) in parent_schema.fields() {
                self.fields
                    .entry(field_name.clone())
                    .or_insert_with(|| field.clone());
            }
            self.ancestors.insert(parent.clone());
            self.ancestors.extend(parent_schema.ancestors().iter().cloned());
        }
    }

    /// Removes `excludes` from the inherited field map.
    fn exclude(&mut self, excludes: &[FieldName]) {
        for excluded in excludes {
            self.fields.shift_remove(excluded);
        }
    }

    /// `$ref`-resolves and validates own fields via [`SchemaFieldBuilder`].
    ///
    /// Must run after [`Self::inherit_from`] (reads `self.ancestors` for `$ref`
    /// bounds-checking).
    ///
    /// # Arguments
    ///
    /// * `raw` - the raw Schema definition whose fields to resolve
    /// * `resolved` - all Schema names resolved so far (for `$ref` lookups)
    ///
    /// # Errors
    ///
    /// - [`FieldBuilder`] if a `$ref` target is out of bounds, missing, or has
    ///   unrecognised attribute keys or type-mismatched values
    ///
    /// [`FieldBuilder`]: SchemaError::FieldBuilder
    fn resolve_own_fields(
        &mut self,
        raw: &RawSchema,
        resolved: &IndexMap<SchemaName, Schema>,
        values_cache: &'a SelectValuesFileCache,
    ) -> Result<(), SchemaError> {
        let field_builder =
            SchemaFieldBuilder::new(&self.ancestors, resolved, values_cache);
        for (field_name, raw_field) in &raw.fields {
            let address = FieldAddressRef::new(self.name, field_name.as_ref());
            let (field, warnings) = field_builder.build(address, raw_field)?;
            self.warnings.extend(warnings);
            self.fields.insert(field_name.clone(), field);
        }
        Ok(())
    }

    /// Rejects a field map where two entries canonicalize to the same
    /// [`FieldKey`] (case-folded, hyphen/underscore-normalized form).
    ///
    /// Ambiguous field identities would make note-vs-schema field matching
    /// unreliable.
    ///
    /// # Errors
    ///
    /// - [`AmbiguousFieldName`] if a collision is found
    ///
    /// [`AmbiguousFieldName`]: SchemaError::AmbiguousFieldName
    fn reject_ambiguous_canonical_names(&self) -> Result<(), SchemaError> {
        let mut seen: HashMap<String, &FieldName> = HashMap::new();
        for field_name in self.fields.keys() {
            let canonical = FieldKey::canonicalize(field_name.as_str());
            if let Some(&first) = seen.get(&canonical) {
                return Err(SchemaError::AmbiguousFieldName {
                    schema: SchemaName::from(self.name),
                    first: first.clone(),
                    second: Box::new(field_name.clone()),
                });
            }
            seen.insert(canonical, field_name);
        }
        Ok(())
    }

    /// Consume the merger, rejecting ambiguous field names and yielding the
    /// resolved [`Schema`].
    ///
    /// # Errors
    ///
    /// - [`AmbiguousFieldName`] if two effective fields canonicalize to the
    ///   same [`FieldKey`]
    ///
    /// [`AmbiguousFieldName`]: SchemaError::AmbiguousFieldName
    /// [`FieldKey`]: crate::field::FieldKey
    fn into_schema(self) -> Result<(Schema, Vec<SchemaWarning>), SchemaError> {
        self.reject_ambiguous_canonical_names()?;
        Ok((
            Schema::new(
                SchemaName::from(self.name),
                self.fields,
                self.ancestors,
            ),
            self.warnings,
        ))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::{super::GLOBAL_SCHEMA_NAME, *};
    use crate::{
        field::FieldValue,
        schema::{
            RawSchemaFieldDef, RawSchemaFieldSource, RawSchemaFieldType,
            fields::{
                FieldAddress, SchemaDateField, SchemaFieldBuilderError,
                SchemaFieldParserError, SchemaFieldType, SchemaFileField,
                SchemaNumberField, SchemaSelectField, SchemaSelectFieldEntry,
            },
        },
    };

    fn resolve(
        raw: &IndexMap<SchemaName, RawSchema>,
    ) -> Result<ResolvedSchemas, SchemaError> {
        let cache = SelectValuesFileCache::for_test();
        SchemaBuilder::new(raw, &cache).build()
    }

    fn field_name(name: &str) -> FieldName {
        FieldName::try_from(name).expect("valid test field name")
    }

    fn field_address(reference: &str) -> FieldAddress {
        FieldAddress::try_from(reference).expect("valid test $ref")
    }

    fn options(
        pairs: &[(&str, FieldValue)],
    ) -> indexmap::IndexMap<String, FieldValue> {
        pairs.iter().map(|(k, v)| ((*k).to_owned(), v.clone())).collect()
    }

    fn string_list(values: &[&str]) -> FieldValue {
        FieldValue::List(
            values.iter().map(|&v| FieldValue::String(v.to_owned())).collect(),
        )
    }

    fn select_field(values: &[&str]) -> RawSchemaFieldDef {
        RawSchemaFieldDef {
            options: options(&[("values", string_list(values))]),
            ..RawSchemaFieldDef::direct(RawSchemaFieldType::Select)
        }
    }

    fn input_field() -> RawSchemaFieldDef {
        RawSchemaFieldDef::direct(RawSchemaFieldType::Input)
    }

    fn file_field(
        folders: &[&str],
        ext: Option<&str>,
        class: &[&str],
    ) -> RawSchemaFieldDef {
        let mut pairs = vec![
            ("folders", string_list(folders)),
            ("class", string_list(class)),
        ];
        if let Some(ext) = ext {
            pairs.push(("ext", FieldValue::String(ext.to_owned())));
        }
        RawSchemaFieldDef {
            options: options(&pairs),
            ..RawSchemaFieldDef::direct(RawSchemaFieldType::File)
        }
    }

    fn ref_field(reference: &str, required: Option<bool>) -> RawSchemaFieldDef {
        RawSchemaFieldDef {
            required,
            ..RawSchemaFieldDef::reference(field_address(reference))
        }
    }

    fn schema(
        extends: &[&str],
        fields: &[(&str, RawSchemaFieldDef)],
    ) -> RawSchema {
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
        fields: &[(&str, RawSchemaFieldDef)],
    ) -> RawSchema {
        RawSchema {
            excludes: excludes.iter().map(|&s| field_name(s)).collect(),
            ..schema(extends, fields)
        }
    }

    fn select_entries(values: &[&str]) -> Vec<SchemaSelectFieldEntry> {
        values
            .iter()
            .map(|&v| SchemaSelectFieldEntry::literal(v.to_owned()))
            .collect()
    }

    mod merge_fields {
        use pretty_assertions::assert_eq;
        use rstest::rstest;

        use super::*;

        #[test]
        fn resolves_a_schema_with_no_extends() {
            let mut raw = IndexMap::new();
            raw.insert(
                SchemaName::from("book"),
                schema(&[], &[("status", select_field(&["draft", "done"]))]),
            );

            let ResolvedSchemas {
                schemas: resolved,
                warnings,
                ..
            } = resolve(&raw).expect("resolves");

            assert!(warnings.is_empty());
            let book = resolved.get("book").expect("book resolved");
            assert_eq!(book.name(), "book");
            let status = book.field("status").expect("status field");
            assert_eq!(
                status.kind(),
                &SchemaFieldType::Select(Arc::new(
                    SchemaSelectField::for_test(select_entries(&[
                        "draft", "done"
                    ]))
                ))
            );
            assert!(!status.is_required());
            assert!(!status.is_multi());
        }

        #[test]
        fn own_fields_override_parent_fields() {
            let mut raw = IndexMap::new();
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

            let ResolvedSchemas {
                schemas: resolved,
                ..
            } = resolve(&raw).expect("resolves");

            let status = resolved
                .get("sci_fi")
                .and_then(|s| s.field("status"))
                .expect("status field");
            assert_eq!(
                status.kind(),
                &SchemaFieldType::Select(Arc::new(
                    SchemaSelectField::for_test(select_entries(&[
                        "outline", "shipped"
                    ]))
                ))
            );
        }

        #[test]
        fn first_listed_parent_wins_a_shared_field() {
            let mut raw = IndexMap::new();
            raw.insert(
                SchemaName::from("a"),
                schema(&[], &[("shared", select_field(&["from-a"]))]),
            );
            raw.insert(
                SchemaName::from("b"),
                schema(&[], &[("shared", select_field(&["from-b"]))]),
            );
            raw.insert(SchemaName::from("child"), schema(&["a", "b"], &[]));

            let ResolvedSchemas {
                schemas: resolved,
                ..
            } = resolve(&raw).expect("resolves");

            let shared = resolved
                .get("child")
                .and_then(|s| s.field("shared"))
                .expect("shared field");
            assert_eq!(
                shared.kind(),
                &SchemaFieldType::Select(Arc::new(
                    SchemaSelectField::for_test(select_entries(&["from-a"]))
                ))
            );
        }

        #[test]
        fn excludes_drops_an_inherited_field_by_name() {
            let mut raw = IndexMap::new();
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

            let ResolvedSchemas {
                schemas: resolved,
                ..
            } = resolve(&raw).expect("resolves");

            let sci_fi = resolved.get("sci_fi").expect("sci_fi resolved");
            assert!(sci_fi.field("status").is_none());
            assert!(sci_fi.field("author").is_some());
            let field_names: Vec<&str> =
                sci_fi.fields().keys().map(FieldName::as_str).collect();
            assert_eq!(field_names, vec!["author"]);
        }

        #[test]
        fn excludes_is_exact_and_does_not_drop_a_different_case_field() {
            let mut raw = IndexMap::new();
            raw.insert(
                SchemaName::from("book"),
                schema(&[], &[("status", select_field(&["draft"]))]),
            );
            raw.insert(
                SchemaName::from("sci_fi"),
                schema_with_excludes(&["book"], &["Status"], &[]),
            );

            let ResolvedSchemas {
                schemas: resolved,
                ..
            } = resolve(&raw).expect("resolves");

            let sci_fi = resolved.get("sci_fi").expect("sci_fi resolved");
            assert!(sci_fi.field("status").is_some());
        }

        #[test]
        fn own_redeclaration_of_an_excluded_field_survives() {
            let mut raw = IndexMap::new();
            raw.insert(
                SchemaName::from("book"),
                schema(&[], &[("status", select_field(&["draft"]))]),
            );
            raw.insert(
                SchemaName::from("sci_fi"),
                schema_with_excludes(&["book"], &["status"], &[(
                    "status",
                    input_field(),
                )]),
            );

            let ResolvedSchemas {
                schemas: resolved,
                ..
            } = resolve(&raw).expect("resolves");

            let sci_fi = resolved.get("sci_fi").expect("sci_fi resolved");
            let status = sci_fi.field("status").expect("own status field");
            assert_eq!(status.kind(), &SchemaFieldType::Input);
        }

        #[rstest]
        #[case::input(input_field(), SchemaFieldType::Input)]
        #[case::select(
            select_field(&["draft", "done"]),
            SchemaFieldType::Select(
                Arc::new(SchemaSelectField::for_test(select_entries(&[
                    "draft", "done",
                ]))),
            ),
        )]
        #[case::boolean(
            RawSchemaFieldDef::direct(RawSchemaFieldType::Boolean),
            SchemaFieldType::Boolean
        )]
        #[case::number(
            RawSchemaFieldDef::direct(RawSchemaFieldType::Number),
            SchemaFieldType::Number(SchemaNumberField::for_test(
                None, None, None
            ))
        )]
        #[case::date(
            RawSchemaFieldDef::direct(RawSchemaFieldType::Date),
            SchemaFieldType::Date(SchemaDateField::for_test(None))
        )]
        #[case::file(
            file_field(&["assets/covers"], Some("png"), &["image"]),
            SchemaFieldType::File(Arc::new(SchemaFileField::for_test(
                vec!["assets/covers".to_owned()],
                Some("png".to_owned()),
                vec!["image".to_owned()],
            )))
        )]
        fn each_field_type_resolves_its_own_options(
            #[case] field_def: RawSchemaFieldDef,
            #[case] expected: SchemaFieldType,
        ) {
            let mut raw = IndexMap::new();
            raw.insert(
                SchemaName::from("book"),
                schema(&[], &[("f", field_def)]),
            );

            let ResolvedSchemas {
                schemas: resolved,
                ..
            } = resolve(&raw).expect("resolves");
            let book = resolved.get("book").expect("book resolved");

            assert_eq!(
                book.field("f").expect("field exists").kind(),
                &expected
            );
        }

        #[test]
        fn multi_defaults_to_false_and_honors_a_local_override() {
            let mut raw = IndexMap::new();
            raw.insert(
                SchemaName::from("book"),
                schema(&[], &[
                    ("status", select_field(&["draft"])),
                    ("authors", RawSchemaFieldDef {
                        multi: Some(true),
                        ..select_field(&["ann", "bo"])
                    }),
                ]),
            );

            let ResolvedSchemas {
                schemas: resolved,
                ..
            } = resolve(&raw).expect("resolves");
            let book = resolved.get("book").expect("book resolved");

            assert!(!book.field("status").expect("status").is_multi());
            assert!(book.field("authors").expect("authors").is_multi());
        }

        #[test]
        fn emits_warning_when_parent_failed_to_resolve() {
            let mut raw = IndexMap::new();
            raw.insert(
                SchemaName::from("broken"),
                schema(&[], &[
                    ("status", input_field()),
                    ("Status", input_field()),
                ]),
            );
            raw.insert(
                SchemaName::from("good"),
                schema(&[], &[("label", select_field(&["a"]))]),
            );
            raw.insert(
                SchemaName::from("child"),
                schema(&["broken", "good"], &[]),
            );

            let ResolvedSchemas {
                schemas: resolved,
                warnings,
                failures,
            } = resolve(&raw).expect("resolves");

            assert_eq!(failures.len(), 1);
            assert_eq!(
                failures.first().expect("one failure").schema,
                SchemaName::from("broken")
            );
            assert!(warnings.contains(&SchemaWarning::ParentFailedToResolve {
                schema: SchemaName::from("child"),
                parent: SchemaName::from("broken"),
            }));
            let child = resolved.get("child").expect("child resolves");
            assert!(
                child.field("label").is_some(),
                "child inherits from the good parent"
            );
            assert!(
                child.field("status").is_none(),
                "child does not inherit from the broken parent"
            );
        }
    }

    mod global {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn does_not_inherit_fields_from_its_own_declared_extends() {
            let mut raw = IndexMap::new();
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

            let ResolvedSchemas {
                schemas: resolved,
                warnings,
                ..
            } = resolve(&raw).expect("resolves");

            assert!(warnings.is_empty());
            let global =
                resolved.get(GLOBAL_SCHEMA_NAME).expect("global resolved");
            assert!(
                global.field("title").is_none(),
                "global must not inherit from its own declared extends"
            );
        }

        #[test]
        fn resolves_before_a_sibling_that_refs_it_despite_declaring_extends() {
            let mut raw = IndexMap::new();
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

            let ResolvedSchemas {
                schemas: resolved,
                ..
            } = resolve(&raw).expect("resolves");

            let priority = resolved
                .get("poem")
                .and_then(|s| s.field("priority"))
                .expect("priority field resolves via $ref to global");
            assert_eq!(
                priority.kind(),
                &SchemaFieldType::Select(Arc::new(
                    SchemaSelectField::for_test(select_entries(&[
                        "low", "high"
                    ]))
                ))
            );
        }

        #[test]
        fn stray_required_on_global_is_ignored_with_a_warning() {
            let mut raw = IndexMap::new();
            raw.insert(
                SchemaName::from(GLOBAL_SCHEMA_NAME),
                schema(&[], &[("priority", RawSchemaFieldDef {
                    required: Some(true),
                    ..select_field(&["low", "high"])
                })]),
            );

            let ResolvedSchemas {
                schemas: resolved,
                warnings,
                ..
            } = resolve(&raw).expect("resolves");

            let priority = resolved
                .get(GLOBAL_SCHEMA_NAME)
                .and_then(|s| s.field("priority"))
                .expect("priority field");
            assert!(!priority.is_required());
            assert_eq!(warnings, vec![SchemaWarning::StrayGlobalRequired {
                field: "priority".to_owned()
            }]);
        }
    }

    mod reference {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn to_ancestor_resolves_with_local_overrides() {
            let mut raw = IndexMap::new();
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

            let ResolvedSchemas {
                schemas: resolved,
                ..
            } = resolve(&raw).expect("resolves");

            let status = resolved
                .get("sci_fi")
                .and_then(|s| s.field("status"))
                .expect("status field");
            assert_eq!(
                status.kind(),
                &SchemaFieldType::Select(Arc::new(
                    SchemaSelectField::for_test(select_entries(&[
                        "draft", "done"
                    ]))
                ))
            );
            assert!(status.is_required());
        }

        #[test]
        fn to_global_resolves_with_local_overrides() {
            let mut raw = IndexMap::new();
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

            let ResolvedSchemas {
                schemas: resolved,
                ..
            } = resolve(&raw).expect("resolves");

            let priority = resolved
                .get("task")
                .and_then(|s| s.field("priority"))
                .expect("priority field");
            assert_eq!(
                priority.kind(),
                &SchemaFieldType::Select(Arc::new(
                    SchemaSelectField::for_test(select_entries(&[
                        "low", "high"
                    ]))
                ))
            );
            assert!(priority.is_required());
        }

        #[test]
        fn to_global_resolves_regardless_of_insertion_order() {
            let mut raw = IndexMap::new();
            raw.insert(
                SchemaName::from(GLOBAL_SCHEMA_NAME),
                schema(&[], &[("name", select_field(&["anon"]))]),
            );
            raw.insert(
                SchemaName::from("author"),
                schema(&[], &[("name", ref_field("#global/name", None))]),
            );

            let ResolvedSchemas {
                schemas: resolved,
                ..
            } = resolve(&raw).expect("resolves");

            let name = resolved
                .get("author")
                .and_then(|s| s.field("name"))
                .expect("name field resolves via $ref to global");
            assert_eq!(
                name.kind(),
                &SchemaFieldType::Select(Arc::new(
                    SchemaSelectField::for_test(select_entries(&["anon"]))
                ))
            );
        }

        #[test]
        fn that_switches_field_type_starts_from_empty_base_options() {
            let mut raw = IndexMap::new();
            raw.insert(
                SchemaName::from("book"),
                schema(&[], &[("status", select_field(&["draft", "done"]))]),
            );
            raw.insert(
                SchemaName::from("sci_fi"),
                schema(&["book"], &[("status", RawSchemaFieldDef {
                    source: RawSchemaFieldSource::Ref {
                        address: field_address("#book/status"),
                        override_type: Some(RawSchemaFieldType::File),
                    },
                    options: options(&[("folders", string_list(&["assets"]))]),
                    ..RawSchemaFieldDef::direct(RawSchemaFieldType::Input)
                })]),
            );

            let ResolvedSchemas {
                schemas: resolved,
                ..
            } = resolve(&raw).expect("resolves");

            let status = resolved
                .get("sci_fi")
                .and_then(|s| s.field("status"))
                .expect("status field");
            assert_eq!(
                status.kind(),
                &SchemaFieldType::File(Arc::new(SchemaFileField::for_test(
                    vec!["assets".to_owned()],
                    None,
                    Vec::new(),
                )))
            );
        }

        #[test]
        fn to_a_file_field_merges_the_filter_with_local_overrides() {
            let mut raw = IndexMap::new();
            raw.insert(
                SchemaName::from(GLOBAL_SCHEMA_NAME),
                schema(&[], &[(
                    "cover",
                    file_field(&["assets"], Some("png"), &["image"]),
                )]),
            );
            raw.insert(
                SchemaName::from("book"),
                schema(&[], &[("cover", RawSchemaFieldDef {
                    options: options(&[(
                        "folders",
                        string_list(&["assets/covers"]),
                    )]),
                    ..RawSchemaFieldDef::reference(field_address(
                        "#global/cover",
                    ))
                })]),
            );

            let ResolvedSchemas {
                schemas: resolved,
                ..
            } = resolve(&raw).expect("resolves");
            let cover = resolved
                .get("book")
                .and_then(|s| s.field("cover"))
                .expect("cover field");

            assert_eq!(
                cover.kind(),
                &SchemaFieldType::File(Arc::new(SchemaFileField::for_test(
                    vec!["assets/covers".to_owned()],
                    Some("png".to_owned()),
                    vec!["image".to_owned()],
                )))
            );
        }

        #[test]
        fn bare_override_with_unknown_key_degrades_with_a_warning() {
            let mut raw = IndexMap::new();
            raw.insert(
                SchemaName::from("book"),
                schema(&[], &[("status", select_field(&["draft", "done"]))]),
            );
            raw.insert(
                SchemaName::from("sci_fi"),
                schema(&["book"], &[("status", RawSchemaFieldDef {
                    options: options(&[("folders", string_list(&["assets"]))]),
                    ..RawSchemaFieldDef::reference(field_address(
                        "#book/status",
                    ))
                })]),
            );

            let ResolvedSchemas {
                schemas: resolved,
                warnings,
                ..
            } = resolve(&raw).expect("resolves");

            let status = resolved
                .get("sci_fi")
                .and_then(|s| s.field("status"))
                .expect("status field still resolves from the base");
            assert_eq!(
                status.kind(),
                &SchemaFieldType::Select(Arc::new(
                    SchemaSelectField::for_test(select_entries(&[
                        "draft", "done"
                    ]))
                ))
            );
            assert_eq!(warnings.len(), 1);
            assert!(matches!(
                warnings.first().expect("expected warning"),
                SchemaWarning::UnknownOverrideKey { .. }
            ));
        }

        #[test]
        fn bare_override_with_type_mismatched_value_degrades_with_a_warning() {
            let mut raw = IndexMap::new();
            raw.insert(
                SchemaName::from("book"),
                schema(&[], &[(
                    "rating",
                    RawSchemaFieldDef::direct(RawSchemaFieldType::Number),
                )]),
            );
            raw.insert(
                SchemaName::from("sci_fi"),
                schema(&["book"], &[("rating", RawSchemaFieldDef {
                    options: options(&[(
                        "min",
                        crate::field::FieldValue::String("abc".to_owned()),
                    )]),
                    ..RawSchemaFieldDef::reference(field_address(
                        "#book/rating",
                    ))
                })]),
            );

            let ResolvedSchemas {
                schemas: resolved,
                warnings,
                ..
            } = resolve(&raw).expect("resolves");

            let rating = resolved
                .get("sci_fi")
                .and_then(|s| s.field("rating"))
                .expect("rating field still resolves from the base");
            assert_eq!(
                rating.kind(),
                &SchemaFieldType::Number(SchemaNumberField::for_test(
                    None, None, None
                ))
            );
            assert_eq!(warnings.len(), 1);
            assert!(matches!(
                warnings.first().expect("expected warning"),
                SchemaWarning::OverrideValueTypeMismatch { .. }
            ));
        }

        #[test]
        fn bare_override_applies_valid_keys_alongside_dropped_unknown() {
            let mut raw = IndexMap::new();
            raw.insert(
                SchemaName::from(GLOBAL_SCHEMA_NAME),
                schema(&[], &[(
                    "cover",
                    file_field(&["assets"], Some("png"), &["image"]),
                )]),
            );
            raw.insert(
                SchemaName::from("book"),
                schema(&[], &[("cover", RawSchemaFieldDef {
                    options: options(&[
                        ("folders", string_list(&["assets/covers"])),
                        ("bogus", string_list(&["x"])),
                    ]),
                    ..RawSchemaFieldDef::reference(field_address(
                        "#global/cover",
                    ))
                })]),
            );

            let ResolvedSchemas {
                schemas: resolved,
                warnings,
                ..
            } = resolve(&raw).expect("resolves");

            let cover = resolved
                .get("book")
                .and_then(|s| s.field("cover"))
                .expect("cover field still resolves from the base");
            assert_eq!(
                cover.kind(),
                &SchemaFieldType::File(Arc::new(SchemaFileField::for_test(
                    vec!["assets/covers".to_owned()],
                    Some("png".to_owned()),
                    vec!["image".to_owned()],
                )))
            );
            assert_eq!(warnings.len(), 1);
            assert!(matches!(
                warnings.first().expect("expected warning"),
                SchemaWarning::UnknownOverrideKey { key, .. } if key == "bogus"
            ));
        }

        #[test]
        fn to_unknown_field_degrades_to_a_failure() {
            let mut raw = IndexMap::new();
            raw.insert(SchemaName::from("book"), schema(&[], &[]));
            raw.insert(
                SchemaName::from("sci_fi"),
                schema(&["book"], &[(
                    "status",
                    ref_field("#book/status", None),
                )]),
            );

            let ResolvedSchemas {
                failures,
                ..
            } = resolve(&raw).expect("unresolved ref degrades, not aborts");
            let err = failures.into_iter().next().expect("one failure").error;
            assert!(matches!(
                err,
                SchemaError::FieldBuilder(inner)
                    if matches!(
                        *inner,
                        SchemaFieldBuilderError::RefFieldNotFound { .. }
                    )
            ));
        }

        #[test]
        fn to_non_ancestor_sibling_degrades_to_a_failure() {
            let mut raw = IndexMap::new();
            raw.insert(
                SchemaName::from("book"),
                schema(&[], &[("status", select_field(&["draft"]))]),
            );
            raw.insert(
                SchemaName::from("movie"),
                schema(&[], &[("status", ref_field("#book/status", None))]),
            );

            let ResolvedSchemas {
                failures,
                ..
            } = resolve(&raw).expect("out-of-bounds ref degrades, not aborts");
            let err = failures.into_iter().next().expect("one failure").error;
            assert!(matches!(
                err,
                SchemaError::FieldBuilder(inner)
                    if matches!(
                        *inner,
                        SchemaFieldBuilderError::RefOutOfBounds { .. }
                    )
            ));
        }

        #[test]
        fn with_type_override_and_unknown_key_degrades_to_a_failure() {
            let mut raw = IndexMap::new();
            raw.insert(
                SchemaName::from("book"),
                schema(&[], &[("status", select_field(&["draft", "done"]))]),
            );
            raw.insert(
                SchemaName::from("sci_fi"),
                schema(&["book"], &[("status", RawSchemaFieldDef {
                    source: RawSchemaFieldSource::Ref {
                        address: field_address("#book/status"),
                        override_type: Some(RawSchemaFieldType::Date),
                    },
                    options: options(&[("values", string_list(&["draft"]))]),
                    ..RawSchemaFieldDef::direct(RawSchemaFieldType::Input)
                })]),
            );

            let ResolvedSchemas {
                failures,
                ..
            } = resolve(&raw).expect(
                "unknown attribute key on a type-overriding $ref degrades, \
                 not aborts",
            );
            let err = failures.into_iter().next().expect("one failure").error;
            assert!(
                matches!(
                    &err,
                    SchemaError::FieldBuilder(inner)
                        if matches!(
                            inner.as_ref(),
                            SchemaFieldBuilderError::Parser(errors)
                                if matches!(
                                    errors.as_slice(),
                                    [SchemaFieldParserError::UnknownKey { .. }]
                                )
                        )
                ),
                "expected Parser(UnknownKey), got {err:?}"
            );
        }

        #[test]
        fn to_nonexistent_ancestor_field_degrades_to_failure() {
            let mut raw = IndexMap::new();
            raw.insert(
                SchemaName::from("ancestor"),
                schema(&[], &[("status", select_field(&["draft"]))]),
            );
            raw.insert(
                SchemaName::from("descendant"),
                schema(&["ancestor"], &[(
                    "ghost",
                    ref_field("#ancestor/ghost", None),
                )]),
            );

            let ResolvedSchemas {
                failures,
                ..
            } = resolve(&raw).expect("ref to nonexistent field degrades");
            let err = failures.into_iter().next().expect("one failure").error;
            assert!(matches!(
                err,
                SchemaError::FieldBuilder(inner)
                    if matches!(
                        *inner,
                        SchemaFieldBuilderError::RefFieldNotFound { .. }
                    )
            ));
        }
    }

    mod validation {
        use super::*;

        #[test]
        fn rejects_ambiguous_field_names_with_different_case() {
            let mut raw = IndexMap::new();
            raw.insert(
                SchemaName::from("book"),
                schema(&[], &[
                    ("status", input_field()),
                    ("Status", input_field()),
                ]),
            );

            let ResolvedSchemas {
                failures,
                ..
            } = resolve(&raw)
                .expect("ambiguous field name degrades, not aborts");
            let err = failures.into_iter().next().expect("one failure").error;
            assert!(matches!(err, SchemaError::AmbiguousFieldName { .. }));
        }

        #[test]
        fn rejects_own_field_colliding_with_inherited_field() {
            let mut raw = IndexMap::new();
            raw.insert(
                SchemaName::from("book"),
                schema(&[], &[("Due Date", input_field())]),
            );
            raw.insert(
                SchemaName::from("sci_fi"),
                schema(&["book"], &[("due-date", input_field())]),
            );

            let ResolvedSchemas {
                failures,
                ..
            } = resolve(&raw)
                .expect("ambiguous field name degrades, not aborts");
            let err = failures.into_iter().next().expect("one failure").error;
            assert!(matches!(err, SchemaError::AmbiguousFieldName { .. }));
        }

        #[test]
        fn rejects_direct_field_with_unknown_key() {
            let mut raw = IndexMap::new();
            raw.insert(
                SchemaName::from("book"),
                schema(&[], &[("published", RawSchemaFieldDef {
                    options: options(&[("values", string_list(&["draft"]))]),
                    ..RawSchemaFieldDef::direct(RawSchemaFieldType::Date)
                })]),
            );

            let ResolvedSchemas {
                failures,
                ..
            } = resolve(&raw)
                .expect("unknown attribute key degrades, not aborts");
            let err = failures.into_iter().next().expect("one failure").error;
            assert!(
                matches!(
                    &err,
                    SchemaError::FieldBuilder(inner)
                        if matches!(
                            inner.as_ref(),
                            SchemaFieldBuilderError::Parser(errors)
                                if matches!(
                                    errors.as_slice(),
                                    [SchemaFieldParserError::UnknownKey { .. }]
                                )
                        )
                ),
                "expected Parser(UnknownKey), got {err:?}"
            );
        }

        #[test]
        fn rejects_direct_field_with_type_mismatched_value() {
            let mut raw = IndexMap::new();
            raw.insert(
                SchemaName::from("book"),
                schema(&[], &[("rating", RawSchemaFieldDef {
                    options: options(&[(
                        "min",
                        crate::field::FieldValue::String("abc".to_owned()),
                    )]),
                    ..RawSchemaFieldDef::direct(RawSchemaFieldType::Number)
                })]),
            );

            let ResolvedSchemas {
                failures,
                ..
            } = resolve(&raw)
                .expect("attribute value type mismatch degrades, not aborts");
            let err = failures.into_iter().next().expect("one failure").error;
            assert!(
                matches!(
                    &err,
                    SchemaError::FieldBuilder(inner)
                        if matches!(
                            inner.as_ref(),
                            SchemaFieldBuilderError::Parser(errors)
                                if matches!(
                                    errors.as_slice(),
                                    [SchemaFieldParserError::TypeMismatch { .. }]
                                )
                        )
                ),
                "expected Parser(TypeMismatch), got {err:?}"
            );
        }
    }

    mod hierarchy {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn is_a_matches_transitively_through_extends() {
            let mut raw = IndexMap::new();
            raw.insert(SchemaName::from("thing"), schema(&[], &[]));
            raw.insert(SchemaName::from("book"), schema(&["thing"], &[]));
            raw.insert(SchemaName::from("sci_fi"), schema(&["book"], &[]));

            let ResolvedSchemas {
                schemas: resolved,
                ..
            } = resolve(&raw).expect("resolves");

            let sci_fi = resolved.get("sci_fi").expect("sci_fi resolved");
            assert!(sci_fi.is_a("sci_fi"));
            assert!(sci_fi.is_a("book"));
            assert!(sci_fi.is_a("thing"));
            assert!(!sci_fi.is_a("movie"));
        }

        #[test]
        fn failed_link_breaks_hierarchy_downstream() {
            let mut raw = IndexMap::new();
            raw.insert(
                SchemaName::from("book"),
                schema(&[], &[("status", select_field(&["draft"]))]),
            );
            raw.insert(
                SchemaName::from("broken"),
                schema(&["book"], &[
                    ("dup", input_field()),
                    ("Dup", input_field()),
                ]),
            );
            raw.insert(
                SchemaName::from("sci_fi"),
                schema(&["broken"], &[("subgenre", input_field())]),
            );

            let ResolvedSchemas {
                schemas: resolved,
                failures,
                ..
            } = resolve(&raw).expect("resolves");

            assert_eq!(
                failures.len(),
                1,
                "expected broken to fail: {failures:?}"
            );
            let sci_fi = resolved.get("sci_fi").expect("sci_fi still resolves");
            assert!(sci_fi.field("subgenre").is_some());
            assert!(
                sci_fi.field("status").is_none(),
                "sci_fi must not inherit book's status field"
            );
            assert!(!sci_fi.is_a("book"), "the chain broke at broken");
            assert!(!sci_fi.is_a("broken"), "broken never resolved");

            let book = resolved.get("book").expect("book resolves");
            assert!(
                !book.descendants().contains(&SchemaName::from("sci_fi")),
                "book.descendants() must not list sci_fi: sci_fi no longer \
                 is-a book"
            );
            assert!(!book.descendants().contains(&SchemaName::from("broken")));
        }
    }

    mod resilience {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn missing_extends_target_degrades_with_a_warning() {
            let mut raw = IndexMap::new();
            raw.insert(
                SchemaName::from("sci_fi"),
                schema(&["ghost"], &[("title", input_field())]),
            );

            let ResolvedSchemas {
                schemas: resolved,
                warnings,
                ..
            } = resolve(&raw).expect("resolves");

            assert!(warnings.contains(&SchemaWarning::MissingExtendsTarget {
                schema: SchemaName::from("sci_fi"),
                target: SchemaName::from("ghost"),
            }));
            assert!(warnings.contains(&SchemaWarning::ParentFailedToResolve {
                schema: SchemaName::from("sci_fi"),
                parent: SchemaName::from("ghost"),
            }));
            let sci_fi =
                resolved.get("sci_fi").expect("own fields still render");
            assert!(sci_fi.field("title").is_some());
            assert!(!sci_fi.is_a("ghost"));
        }

        #[test]
        fn malformed_field_does_not_block_unrelated_sibling() {
            let mut raw = IndexMap::new();
            raw.insert(
                SchemaName::from("book"),
                schema(&[], &[("status", select_field(&["draft"]))]),
            );
            raw.insert(
                SchemaName::from("broken"),
                schema(&[], &[
                    ("status", input_field()),
                    ("Status", input_field()),
                ]),
            );

            let ResolvedSchemas {
                schemas: resolved,
                failures,
                ..
            } = resolve(&raw).expect("unrelated failure does not abort");

            assert!(resolved.contains_key("book"));
            assert!(!resolved.contains_key("broken"));
            assert_eq!(failures.len(), 1);
            let failure = failures.first().expect("one failure");
            assert_eq!(failure.schema, SchemaName::from("broken"));
        }

        #[test]
        fn child_of_failed_parent_resolves_own_fields() {
            let mut raw = IndexMap::new();
            raw.insert(
                SchemaName::from("broken"),
                schema(&[], &[
                    ("status", input_field()),
                    ("Status", input_field()),
                ]),
            );
            raw.insert(
                SchemaName::from("child"),
                schema(&["broken"], &[("title", input_field())]),
            );

            let ResolvedSchemas {
                schemas: resolved,
                warnings,
                failures,
            } = resolve(&raw).expect("child still resolves");

            assert_eq!(failures.len(), 1);
            let failure = failures.first().expect("one failure");
            assert_eq!(failure.schema, SchemaName::from("broken"));
            assert!(warnings.contains(&SchemaWarning::ParentFailedToResolve {
                schema: SchemaName::from("child"),
                parent: SchemaName::from("broken"),
            }));
            let child = resolved.get("child").expect("child still resolves");
            assert!(child.field("title").is_some());
        }

        #[test]
        fn rejects_cycle_as_hard_error() {
            let mut raw = IndexMap::new();
            raw.insert(SchemaName::from("a"), schema(&["b"], &[]));
            raw.insert(SchemaName::from("b"), schema(&["a"], &[]));

            let err = resolve(&raw).expect_err("cycle rejected");
            assert!(
                matches!(
                    &err,
                    SchemaError::Cycle { schemas }
                        if schemas
                            == &vec![
                                SchemaName::from("a"),
                                SchemaName::from("b")
                            ]
                ),
                "expected Cycle over [a, b], got {err:?}"
            );
        }

        #[test]
        fn cycle_error_excludes_a_non_cyclic_dependent() {
            let mut raw = IndexMap::new();
            raw.insert(SchemaName::from("a"), schema(&["b"], &[]));
            raw.insert(SchemaName::from("b"), schema(&["a"], &[]));
            raw.insert(SchemaName::from("c"), schema(&["a"], &[]));

            let err = resolve(&raw).expect_err("cycle rejected");
            assert!(
                matches!(
                    &err,
                    SchemaError::Cycle { schemas }
                        if schemas
                            == &vec![
                                SchemaName::from("a"),
                                SchemaName::from("b")
                            ]
                ),
                "expected Cycle over [a, b] only, excluding non-cyclic \
                 dependent c; got {err:?}"
            );
        }

        #[test]
        fn multiple_schemas_fail_independently() {
            let mut raw = IndexMap::new();
            raw.insert(
                SchemaName::from("alpha"),
                schema(&[], &[
                    ("status", input_field()),
                    ("Status", input_field()),
                ]),
            );
            raw.insert(
                SchemaName::from("beta"),
                schema(&[], &[
                    ("name", input_field()),
                    ("Name", input_field()),
                ]),
            );

            let ResolvedSchemas {
                failures,
                ..
            } = resolve(&raw).expect("both fail independently");

            assert_eq!(
                failures.len(),
                2,
                "expected two failures: {failures:?}"
            );
            let failed_schemas: Vec<&SchemaName> =
                failures.iter().map(|f| &f.schema).collect();
            assert!(failed_schemas.contains(&&SchemaName::from("alpha")));
            assert!(failed_schemas.contains(&&SchemaName::from("beta")));
        }
    }
}
