//! Schema loading, resolution, and query facade.
//!
//! [`SchemaService`] loads `.traces/schemas/*.toml` files, linearizes the
//! `extends` DAG, and resolves it once at construction, exposing read-side
//! queries over the resolved Schemas for its whole lifetime.

use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::OsStr,
    fs, io,
    path::Path,
    sync::Arc,
};

use walkdir::WalkDir;

use super::{
    RawSchema, SchemaName, SchemaNameRef,
    error::{SchemaError, SchemaWarning},
    fields::{FieldAddressRef, RefAddressResolver, SchemaFieldBuilder},
    graph::SchemaGraph,
    model::Schema,
};
use crate::{
    BaseNameRef,
    config::SchemaConfigSpec,
    field::FieldName,
    query::{ClassExpansionMode, QuerySource, SourceAtom},
};

/// Schema loading, resolution, and hierarchy/class query facade.
///
/// Resolves every Schema once at construction ([`SchemaService::new`]); every
/// query method reads the already-resolved Schemas directly, with no separate
/// registry type or re-resolution.
#[derive(Debug)]
pub struct SchemaService {
    spec: SchemaConfigSpec,
    schemas: BTreeMap<SchemaName, Arc<Schema>>,
}

/// One Schema whose own fields failed to build during resolution, alongside the
/// [`SchemaError`] it failed with.
///
/// Excluded from [`SchemaService`]'s resolved Schemas; any Schema naming it as
/// a parent inherits none of its fields (see
/// [`SchemaWarning::ParentFailedToResolve`]).
#[derive(Debug)]
pub(crate) struct SchemaFailure {
    pub(crate) schema: SchemaName,
    pub(crate) error: SchemaError,
}

/// The triple [`SchemaService::new`] returns: the constructed service, any
/// warnings degraded resolution accumulated along the way, and every Schema
/// whose own build failed alongside the error it failed with.
type SchemaConstruction =
    (SchemaService, Vec<SchemaWarning>, Vec<SchemaFailure>);

impl SchemaService {
    /// Load every Schema TOML file under [`SchemaConfigSpec::directory`],
    /// linearize the `extends` DAG, and resolve every Schema's effective
    /// fields, alongside any [`SchemaWarning`]s degraded resolution accumulated
    /// and any per-Schema [`SchemaError`] failures that excluded that Schema
    /// from the result (see [`ParentFailedToResolve`]: dependents of a failed
    /// Schema still resolve, without its fields).
    ///
    /// A missing directory resolves to an empty registry.
    ///
    /// # Errors
    ///
    /// - [`ReadDirectory`] if the registry directory exists but cannot be
    ///   listed.
    /// - [`ReadFile`] if a `.toml` file cannot be read.
    /// - [`Parse`] if a Schema file's TOML is malformed.
    /// - [`Cycle`] if the `extends` DAG contains a cycle.
    ///
    /// [`ReadDirectory`]: SchemaError::ReadDirectory
    /// [`ReadFile`]: SchemaError::ReadFile
    /// [`Parse`]: SchemaError::Parse
    /// [`Cycle`]: SchemaError::Cycle
    /// [`ParentFailedToResolve`]: SchemaWarning::ParentFailedToResolve
    pub(crate) fn new(
        spec: SchemaConfigSpec,
    ) -> Result<SchemaConstruction, SchemaError> {
        let raw = read_raw_schemas(spec.directory())?;
        let (schemas, warnings, failures) = resolve_all(&raw)?;
        let schemas = schemas
            .into_iter()
            .map(|(name, schema)| (name, Arc::new(schema)))
            .collect();
        Ok((
            Self {
                spec,
                schemas,
            },
            warnings,
            failures,
        ))
    }

    /// Return the config projection this service was built from.
    #[inline]
    #[must_use]
    pub(crate) fn spec(&self) -> &SchemaConfigSpec {
        &self.spec
    }

    /// Return a reference to the named Schema, or `None` if no Schema by that
    /// name resolved.
    #[inline]
    #[must_use]
    pub(crate) fn get(&self, name: &str) -> Option<&Arc<Schema>> {
        self.schemas.get(name)
    }

    /// Return every Schema that directly extends `name`.
    ///
    /// Excludes `name` itself and every transitive descendant. Empty, not an
    /// error, if `name` has no Schema or nothing extends it.
    #[must_use]
    pub(crate) fn children_of(&self, name: &str) -> Vec<Arc<Schema>> {
        let Some(schema) = self.schemas.get(name) else {
            return Vec::new();
        };
        schema
            .children()
            .iter()
            .filter_map(|child| self.schemas.get(child.as_str()))
            .cloned()
            .collect()
    }

    /// Return every Schema that directly or transitively extends `name`.
    ///
    /// Excludes `name` itself. Empty, not an error, if `name` has no Schema or
    /// nothing extends it.
    #[must_use]
    pub(crate) fn descendants_of(&self, name: &str) -> Vec<Arc<Schema>> {
        let Some(schema) = self.schemas.get(name) else {
            return Vec::new();
        };
        schema
            .descendants()
            .iter()
            .filter_map(|descendant| self.schemas.get(descendant.as_str()))
            .cloned()
            .collect()
    }

    /// Return the set of Schema names matching `classes`, including transitive
    /// descendants.
    ///
    /// A class with no corresponding Schema still matches itself. Warns once
    /// per unknown class name.
    ///
    /// # Examples
    ///
    /// Given `sci_fi` extending `book`, and `movie` unrelated:
    ///
    /// - `matches(&["book"])` → `{"book", "sci_fi"}`
    /// - `matches(&["movie"])` → `{"movie"}`
    /// - `matches(&["ghost"])` → `{"ghost"}`
    #[must_use]
    pub(crate) fn matches(&self, classes: &[String]) -> BTreeSet<String> {
        warn_unknown_classes(self, classes);
        let mut matches: BTreeSet<String> = classes.iter().cloned().collect();
        for class in classes {
            if let Some(schema) = self.get(class) {
                matches.extend(
                    schema
                        .descendants()
                        .iter()
                        .map(|name| name.as_str().to_owned()),
                );
            }
        }
        matches
    }

    /// Populate `mode`'s match set from `classes` at its requested depth.
    ///
    /// Unknown class names remain in the set so a [`Note`](crate::note::Note)
    /// may still use them without a corresponding Schema.
    ///
    /// # Arguments
    ///
    /// * `classes`: File Class values to expand.
    /// * `mode`: controls expansion depth (`Exact`, `Children`, `Descendants`).
    pub(crate) fn expand_classes(
        &self,
        classes: &[String],
        mode: &mut ClassExpansionMode,
    ) {
        let mut expanded: BTreeSet<String> = classes.iter().cloned().collect();
        match mode {
            ClassExpansionMode::Exact(_) => {
                warn_unknown_classes(self, classes);
            }
            ClassExpansionMode::Children(_) => {
                warn_unknown_classes(self, classes);
                for class in classes {
                    expanded.extend(
                        self.children_of(class)
                            .iter()
                            .map(|schema| schema.name().to_owned()),
                    );
                }
            }
            ClassExpansionMode::Descendants(_) => {
                // `matches` warns internally, so this branch alone would
                // otherwise skip the warning the other two branches emit
                // directly above.
                expanded = self.matches(classes);
            }
        }
        mode.set_classes(expanded);
    }
}

/// Warn once per unknown class name in `classes`.
fn warn_unknown_classes(service: &SchemaService, classes: &[String]) {
    for class in classes {
        if service.get(class).is_none() {
            tracing::warn!(
                class,
                "query source names an unknown File Class; matching it exactly"
            );
        }
    }
}

/// Resolve every File Class leaf in `source` against `service`.
///
/// This caller-side pre-pass keeps query parsing and matching independent of
/// the Schema registry.
///
/// # Arguments
///
/// * `source`: query source expression to expand class atoms in.
/// * `service`: resolved schema service to match against.
pub(crate) fn resolve_sources(
    source: &mut QuerySource,
    service: &SchemaService,
) {
    let QuerySource::Expr(expression) = source else {
        return;
    };
    expression.visit_atoms_mut(&mut |atom| {
        if let SourceAtom::Class {
            names,
            mode,
        } = atom
        {
            service.expand_classes(names, mode);
        }
    });
}

/// Read and parse every `*.toml` file directly under `dir` into a [`RawSchema`]
/// keyed by filename stem.
///
/// Walks only `dir`'s immediate entries (`min_depth(1).max_depth(1)`): Schemas
/// do not nest. A `dir` that does not exist yields an empty map rather than
/// [`SchemaError::ReadDirectory`].
///
/// # Errors
///
/// - [`ReadDirectory`] if `dir` exists but its entries cannot be listed.
/// - [`ReadFile`] if a `.toml` file cannot be read.
/// - [`Parse`] if a Schema file's TOML is malformed, contains an unknown key,
///   has a malformed `$ref`, or omits both `type` and `$ref` for a field.
///
/// [`ReadDirectory`]: SchemaError::ReadDirectory
/// [`ReadFile`]: SchemaError::ReadFile
/// [`Parse`]: SchemaError::Parse
fn read_raw_schemas(
    dir: &Path,
) -> Result<BTreeMap<SchemaName, RawSchema>, SchemaError> {
    let entries = WalkDir::new(dir).min_depth(1).max_depth(1);
    let mut schemas = BTreeMap::new();
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(source) if is_missing_root(&source) => {
                return Ok(BTreeMap::new());
            }
            Err(source) => return Err(walk_error(dir, source)),
        };
        let path = entry.path();
        if path.extension().and_then(OsStr::to_str) != Some("toml") {
            continue;
        }
        let Some(stem) = BaseNameRef::from_path(path) else {
            continue;
        };
        let stem = SchemaName::from(stem.as_str());
        let contents = fs::read_to_string(path).map_err(|source| {
            SchemaError::ReadFile {
                path: path.to_path_buf(),
                source,
            }
        })?;
        let raw: RawSchema =
            toml::from_str(&contents).map_err(|source| SchemaError::Parse {
                schema: stem.clone(),
                source: Box::new(source),
            })?;
        schemas.insert(stem, raw);
    }
    Ok(schemas)
}

/// Return `true` if `error` reports that the walk's root itself does not exist,
/// so [`read_raw_schemas`] can degrade to an empty registry.
fn is_missing_root(error: &walkdir::Error) -> bool {
    error.depth() == 0
        && error
            .io_error()
            .is_some_and(|source| source.kind() == io::ErrorKind::NotFound)
}

/// Wrap a [`walkdir::Error`] with path context as a
/// [`SchemaError::ReadDirectory`].
///
/// Falls back to `root` if the underlying error carries no path of its own
/// (some I/O errors surface without `DirEntry` context).
fn walk_error(root: &Path, source: walkdir::Error) -> SchemaError {
    let path = source.path().unwrap_or(root).to_path_buf();
    SchemaError::ReadDirectory {
        directory: path,
        source: source.into(),
    }
}

/// Return every Schema resolved by [`resolve_all`], keyed by name, every
/// [`SchemaWarning`] degraded resolution accumulated, and every Schema whose
/// own build failed alongside the [`SchemaError`] it failed with — a failed
/// Schema is excluded from the first map, resolves no descendants, and any
/// Schema that names it as a parent inherits none of its fields (with a
/// [`SchemaWarning::ParentFailedToResolve`]), exactly as an unresolvable
/// `extends` target already degrades today.
type ResolveOutput =
    (BTreeMap<SchemaName, Schema>, Vec<SchemaWarning>, Vec<SchemaFailure>);

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
/// Once every Schema resolved (confirmed acyclic), populates each Schema's
/// `children`/`descendants` in one pass over the whole DAG via
/// [`SchemaGraph::children_by_name`]/[`SchemaGraph::descendants_by_name`].
///
/// A Schema whose own fields fail to build (any [`SchemaError`] that
/// [`SchemaFieldBuilder::build`] returns, or [`AmbiguousFieldName`] if two of
/// its effective fields share a [`FieldKey`] canonical form) is excluded from
/// the returned map and reported in the returned failures list instead of
/// aborting resolution of every other Schema.
///
/// # Errors
///
/// - [`Cycle`] if the `extends` DAG contains a cycle.
///
/// [`Cycle`]: SchemaError::Cycle
/// [`AmbiguousFieldName`]: SchemaError::AmbiguousFieldName
/// [`FieldKey`]: crate::field::FieldKey
fn resolve_all(
    raw_schemas: &BTreeMap<SchemaName, RawSchema>,
) -> Result<ResolveOutput, SchemaError> {
    let (mut graph, mut warnings) = SchemaGraph::new(raw_schemas);
    let mut resolved: BTreeMap<SchemaName, Schema> = BTreeMap::new();
    let mut failures: Vec<SchemaFailure> = Vec::new();

    while let Some(name) = graph.next_ready() {
        #[expect(
            clippy::expect_used,
            reason = "SchemaGraph::new builds parents_by_name/in_degree/ \
                      children_by_name/queue exclusively from raw_schemas's \
                      own keys, so next_ready() can never yield a name absent \
                      from raw_schemas; failure here means the graph itself \
                      is broken, not a recoverable caller error"
        )]
        let raw = raw_schemas.get(name.as_str()).expect(
            "SchemaGraph::next_ready only ever yields names present in \
             raw_schemas",
        );
        match build_schema(name, raw, graph.parents_of(name), &resolved) {
            Ok((schema, schema_warnings)) => {
                warnings.extend(schema_warnings);
                resolved.insert(SchemaName::from(name), schema);
            }
            Err(error) => {
                failures.push(SchemaFailure {
                    schema: SchemaName::from(name),
                    error,
                });
            }
        }
        graph.mark_resolved(name);
    }

    if let Some(schemas) = graph.cyclic_remainder(raw_schemas) {
        return Err(SchemaError::Cycle {
            schemas,
        });
    }

    // `graph.children_by_name`/`descendants_by_name` walk the raw `extends`
    // topology, which does not know a link broke: a Schema downstream of a
    // `ParentFailedToResolve` break (see `build_schema`) is still linked there,
    // even though it no longer semantically `is_a` that ancestor. Each resolved
    // Schema's own `ancestors()` is the authoritative, failure-aware signal, so
    // filter the raw candidate sets against it before publishing them — a
    // raw-graph candidate is kept only if it actually resolved *and* its own
    // ancestors include the schema being populated.
    let children_by_name = graph.children_by_name();
    let descendants_by_name = graph.descendants_by_name();
    let resolved_ancestors: BTreeMap<SchemaName, BTreeSet<SchemaName>> =
        resolved
            .iter()
            .map(|(name, schema)| (name.clone(), schema.ancestors().clone()))
            .collect();
    for (name, schema) in &mut resolved {
        let still_descends_from = |candidate: &SchemaName| {
            resolved_ancestors
                .get(candidate)
                .is_some_and(|ancestors| ancestors.contains(name))
        };
        let children = children_by_name
            .get(name)
            .into_iter()
            .flatten()
            .filter(|child| still_descends_from(child))
            .cloned()
            .collect();
        let descendants = descendants_by_name
            .get(name)
            .into_iter()
            .flatten()
            .filter(|descendant| still_descends_from(descendant))
            .cloned()
            .collect();
        schema.set_hierarchy(children, descendants);
    }

    Ok((resolved, warnings, failures))
}

/// Resolve one Schema's effective fields and transitive ancestors, alongside
/// every warning degraded validation raised while building its own fields.
///
/// Merges `parents`' fields first-listed-wins, applies `raw.excludes`, then
/// overrides the result with `raw`'s own (`$ref`-resolved) fields.
///
/// `parents` must already be resolved in `resolved`: [`resolve_all`] guarantees
/// this by calling in Kahn topological order.
///
/// # Arguments
///
/// * `name`: the Schema being resolved (its filename stem).
/// * `raw`: `name`'s own parsed TOML: `extends`, `excludes`, and fields.
/// * `parents`: `raw.extends`, filtered to targets that resolved.
/// * `resolved`: Schemas already resolved earlier in Kahn order, keyed by name.
///
/// # Errors
///
/// - Any [`SchemaError`] that [`SchemaFieldBuilder::build`] returns while
///   resolving `raw`'s own fields.
/// - [`AmbiguousFieldName`] if two of the resolved fields share a [`FieldKey`]
///   canonical form.
///
/// [`AmbiguousFieldName`]: SchemaError::AmbiguousFieldName
/// [`FieldKey`]: crate::field::FieldKey
fn build_schema(
    name: SchemaNameRef<'_>,
    raw: &RawSchema,
    parents: &[SchemaNameRef<'_>],
    resolved: &BTreeMap<SchemaName, Schema>,
) -> Result<(Schema, Vec<SchemaWarning>), SchemaError> {
    let mut fields = BTreeMap::new();
    let mut ancestors = BTreeSet::new();
    let mut warnings = Vec::new();
    for &parent in parents {
        let Some(parent_schema) = resolved.get(parent.as_str()) else {
            warnings.push(SchemaWarning::ParentFailedToResolve {
                schema: SchemaName::from(name),
                parent: SchemaName::from(parent),
            });
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
    let refs = RefAddressResolver {
        ancestors: &ancestors,
        resolved,
    };
    let builder = SchemaFieldBuilder {
        refs: &refs,
    };
    let mut own_fields = BTreeMap::new();
    for (field_name, raw_field) in &raw.fields {
        let address = FieldAddressRef::new(name, field_name.as_ref());
        let (field, field_warnings) = builder.build(address, raw_field)?;
        warnings.extend(field_warnings);
        own_fields.insert(field_name.clone(), field);
    }
    fields.extend(own_fields);

    reject_ambiguous_canonical_names(name, &fields)?;

    Ok((Schema::new(SchemaName::from(name), fields, ancestors), warnings))
}

/// Reject `fields` if two entries share a [`FieldKey`] canonical form:
/// ambiguous field identities would make later note-vs-schema field matching
/// and unknown-field suggestions unreliable.
///
/// # Errors
///
/// - [`AmbiguousFieldName`] naming the first two (name-sorted) colliding field
///   names.
///
/// [`FieldKey`]: crate::field::FieldKey
/// [`AmbiguousFieldName`]: SchemaError::AmbiguousFieldName
fn reject_ambiguous_canonical_names(
    name: SchemaNameRef<'_>,
    fields: &BTreeMap<FieldName, super::fields::SchemaFieldDef>,
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

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{super::GLOBAL_SCHEMA_NAME, *};
    use crate::schema::{
        RawSchemaFieldDef, RawSchemaFieldSource, RawSchemaFieldType,
        fields::{
            SchemaDateField, SchemaFieldBuilderError, SchemaFieldDef,
            SchemaFieldParserError, SchemaFieldType, SchemaFileField,
            SchemaNumberField, SchemaSelectField, SchemaSelectFieldEntry,
        },
    };

    type ResolveResult = Result<SchemaConstruction, SchemaError>;

    /// Resolves every Schema TOML file directly under `dir`, mirroring the
    /// pre-refactor `SchemaRegistry::load(dir)` call shape: `dir` is used
    /// directly as the Schema directory, `root` is unused by resolution
    /// itself.
    fn resolve_dir(dir: &Path) -> ResolveResult {
        SchemaService::new(SchemaConfigSpec::for_test(dir, dir))
    }

    fn write_schema(dir: &Path, name: &str, toml: &str) {
        fs::write(dir.join(format!("{name}.toml")), toml)
            .expect("write schema fixture");
    }

    mod resolve {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn parses_a_schema_directory_keyed_by_filename_stem() {
            let temp = tempfile::tempdir().expect("create temp dir");
            write_schema(
                temp.path(),
                "book",
                r#"
                [fields.status]
                type = "select"
                values = ["draft", "done"]
                "#,
            );

            let (registry, warnings, _failures) =
                resolve_dir(temp.path()).expect("registry loads");

            assert!(warnings.is_empty());
            let book = registry.get("book").expect("book resolved");
            assert_eq!(book.name(), "book");
            assert!(book.field("status").is_some());
        }

        #[test]
        fn ignores_non_toml_files_in_the_registry_directory() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("README.md"), "not a schema")
                .expect("write non-schema file");

            let (registry, _, _) =
                resolve_dir(temp.path()).expect("registry loads");

            assert!(registry.get("README").is_none());
        }

        #[test]
        fn resolves_to_an_empty_registry_when_the_directory_is_missing() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let missing = temp.path().join("does-not-exist");

            let (registry, warnings, _failures) =
                resolve_dir(&missing).expect("missing dir is not fatal");

            assert!(warnings.is_empty());
            assert!(registry.get("anything").is_none());
        }

        #[test]
        fn rejects_an_unknown_top_level_key_at_parse() {
            let temp = tempfile::tempdir().expect("create temp dir");
            write_schema(temp.path(), "book", "typo_key = true\n");

            let err =
                resolve_dir(temp.path()).expect_err("unknown key rejected");

            assert!(matches!(err, SchemaError::Parse { .. }));
        }

        #[test]
        fn rejects_an_unknown_field_key_at_parse() {
            let temp = tempfile::tempdir().expect("create temp dir");
            write_schema(
                temp.path(),
                "book",
                r#"
                [fields.status]
                type = "select"
                values = ["draft"]
                typo_key = true
                "#,
            );

            let err = resolve_dir(temp.path())
                .expect_err("unknown field key rejected");

            assert!(matches!(err, SchemaError::Parse { .. }));
        }

        #[test]
        fn rejects_a_malformed_ref_at_parse() {
            let temp = tempfile::tempdir().expect("create temp dir");
            write_schema(
                temp.path(),
                "book",
                r#"
                [fields.status]
                "$ref" = "global/status"
                "#,
            );

            let err =
                resolve_dir(temp.path()).expect_err("malformed $ref rejected");

            assert!(matches!(err, SchemaError::Parse { .. }));
        }

        #[test]
        fn rejects_a_field_with_neither_type_nor_ref_at_parse() {
            let temp = tempfile::tempdir().expect("create temp dir");
            write_schema(
                temp.path(),
                "book",
                r"
                [fields.status]
                required = true
                ",
            );

            let err = resolve_dir(temp.path())
                .expect_err("missing type/$ref rejected");

            assert!(matches!(err, SchemaError::Parse { .. }));
        }

        #[test]
        fn propagates_a_cycle_error_when_the_extends_dag_has_a_cycle() {
            let temp = tempfile::tempdir().expect("create temp dir");
            write_schema(temp.path(), "a", r#"extends = ["b"]"#);
            write_schema(temp.path(), "b", r#"extends = ["a"]"#);

            let err = resolve_dir(temp.path()).expect_err("cycle rejected");

            assert!(matches!(err, SchemaError::Cycle { .. }));
        }

        #[test]
        fn ignores_a_nested_subdirectory_of_the_registry() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let nested = temp.path().join("nested");
            fs::create_dir(&nested).expect("create nested dir");
            write_schema(&nested, "hidden", "");

            let (service, _, _) =
                resolve_dir(temp.path()).expect("registry loads");

            assert!(service.get("hidden").is_none());
        }

        #[cfg(unix)]
        #[test]
        fn returns_a_read_directory_error_when_the_registry_directory_is_unreadable()
         {
            use std::os::unix::fs::PermissionsExt as _;

            /// Restores a locked directory's permissions on drop, even if
            /// the test panics. Otherwise, a `0o000` directory blocks the
            /// tempdir's own cleanup.
            struct RestorePermissions<'a>(&'a Path);

            impl Drop for RestorePermissions<'_> {
                fn drop(&mut self) {
                    let _ = fs::set_permissions(
                        self.0,
                        fs::Permissions::from_mode(0o700),
                    );
                }
            }

            let temp = tempfile::tempdir().expect("create temp dir");
            let locked = temp.path().join("locked");
            fs::create_dir(&locked).expect("create locked dir");
            fs::set_permissions(&locked, fs::Permissions::from_mode(0o000))
                .expect("revoke read permission");
            let _restore = RestorePermissions(&locked);

            let err =
                resolve_dir(&locked).expect_err("unreadable directory fails");

            assert!(matches!(err, SchemaError::ReadDirectory { .. }));
        }

        #[cfg(unix)]
        #[test]
        fn returns_a_read_file_error_when_a_schema_file_is_unreadable() {
            use std::os::unix::fs::PermissionsExt as _;

            let temp = tempfile::tempdir().expect("create temp dir");
            write_schema(temp.path(), "book", "");
            let file = temp.path().join("book.toml");
            fs::set_permissions(&file, fs::Permissions::from_mode(0o000))
                .expect("revoke read permission");

            let err =
                resolve_dir(temp.path()).expect_err("unreadable file fails");

            assert!(matches!(err, SchemaError::ReadFile { .. }));
        }
    }

    mod get {
        use super::*;

        #[test]
        fn returns_the_same_arc_backed_schema_on_repeated_calls() {
            let temp = tempfile::tempdir().expect("create temp dir");
            write_schema(
                temp.path(),
                "book",
                r#"
                [fields.status]
                type = "select"
                values = ["draft"]
                "#,
            );
            let (service, _, _) =
                resolve_dir(temp.path()).expect("registry loads");

            let first = service.get("book").expect("book resolved");
            let second = service.get("book").expect("book resolved");

            assert!(
                Arc::ptr_eq(first, second),
                "repeated lookups must share one Arc-backed Schema, not clone \
                 a fresh one per call"
            );
        }

        #[test]
        fn schemas_are_resolved_once_at_construction_not_reread_later() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let schemas_dir = temp.path().join(".traces/schemas");
            fs::create_dir_all(&schemas_dir).expect("create schemas dir");
            write_schema(&schemas_dir, "book", "");
            let (service, _, _) = SchemaService::new(
                SchemaConfigSpec::for_test(temp.path(), &schemas_dir),
            )
            .expect("registry loads");

            fs::remove_dir_all(&schemas_dir).expect("remove schemas dir");

            assert!(
                service.get("book").is_some(),
                "a Schema resolved at construction must not need to reread a \
                 now-missing directory"
            );
        }
    }

    mod matches {
        use std::collections::BTreeSet;

        use pretty_assertions::assert_eq;

        use super::*;

        fn set(names: &[&str]) -> BTreeSet<String> {
            names.iter().map(|name| (*name).to_owned()).collect()
        }

        #[test]
        fn includes_a_class_with_no_schema() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let (service, _, _) =
                resolve_dir(temp.path()).expect("registry loads");

            let matches = service.matches(&["ghost".to_owned()]);

            assert_eq!(matches, set(&["ghost"]));
        }

        #[test]
        fn includes_transitive_subclasses_of_a_class() {
            let temp = tempfile::tempdir().expect("create temp dir");
            write_schema(temp.path(), "book", "");
            write_schema(temp.path(), "sci_fi", r#"extends = ["book"]"#);

            let (service, _, _) =
                resolve_dir(temp.path()).expect("registry loads");

            let matches = service.matches(&["book".to_owned()]);

            assert_eq!(matches, set(&["book", "sci_fi"]));
        }

        #[test]
        fn excludes_classes_unrelated_to_the_class() {
            let temp = tempfile::tempdir().expect("create temp dir");
            write_schema(temp.path(), "book", "");
            write_schema(temp.path(), "movie", "");

            let (service, _, _) =
                resolve_dir(temp.path()).expect("registry loads");

            let matches = service.matches(&["book".to_owned()]);

            assert_eq!(matches, set(&["book"]));
        }

        #[test]
        fn unions_the_matches_of_every_class() {
            let temp = tempfile::tempdir().expect("create temp dir");
            write_schema(temp.path(), "book", "");
            write_schema(temp.path(), "sci_fi", r#"extends = ["book"]"#);
            write_schema(temp.path(), "movie", "");

            let (service, _, _) =
                resolve_dir(temp.path()).expect("registry loads");

            let matches =
                service.matches(&["book".to_owned(), "movie".to_owned()]);

            assert_eq!(matches, set(&["book", "movie", "sci_fi"]));
        }

        #[test]
        fn returns_an_empty_set_for_no_classes() {
            let temp = tempfile::tempdir().expect("create temp dir");
            write_schema(temp.path(), "book", "");

            let (service, _, _) =
                resolve_dir(temp.path()).expect("registry loads");

            let matches = service.matches(&[]);

            assert!(matches.is_empty());
        }
    }

    mod expand_classes {
        use std::collections::BTreeSet;

        use pretty_assertions::assert_eq;

        use super::*;

        fn set(names: &[&str]) -> BTreeSet<String> {
            names.iter().map(|name| (*name).to_owned()).collect()
        }

        fn service(temp: &tempfile::TempDir) -> SchemaService {
            write_schema(temp.path(), "thing", "");
            write_schema(temp.path(), "book", r#"extends = ["thing"]"#);
            write_schema(temp.path(), "sci_fi", r#"extends = ["book"]"#);
            write_schema(temp.path(), "space_opera", r#"extends = ["sci_fi"]"#);
            let (service, _, _) =
                resolve_dir(temp.path()).expect("registry loads");
            service
        }

        #[test]
        fn children_returns_only_direct_extenders() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let service = service(&temp);

            let names: Vec<String> = service
                .children_of("thing")
                .into_iter()
                .map(|schema| schema.name().to_owned())
                .collect();

            assert_eq!(names, vec!["book".to_owned()]);
        }

        #[test]
        fn expansion_modes_are_incremental() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let service = service(&temp);
            let names = vec!["thing".to_owned()];
            let mut exact = ClassExpansionMode::Exact(BTreeSet::new());
            let mut children = ClassExpansionMode::Children(BTreeSet::new());
            let mut descendants =
                ClassExpansionMode::Descendants(BTreeSet::new());

            service.expand_classes(&names, &mut exact);
            service.expand_classes(&names, &mut children);
            service.expand_classes(&names, &mut descendants);

            assert_eq!(exact.classes(), &set(&["thing"]));
            assert_eq!(children.classes(), &set(&["book", "thing"]));
            assert_eq!(
                descendants.classes(),
                &set(&["book", "sci_fi", "space_opera", "thing"])
            );
        }

        #[test]
        #[expect(clippy::panic, reason = "let-else guard on exhaustive match")]
        fn resolve_sources_walks_nested_expressions_and_preserves_unknowns() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let service = service(&temp);
            let mut source = QuerySource::parse("@thing+ and not @ghost*")
                .expect("source parses");

            resolve_sources(&mut source, &service);

            assert!(
                matches!(source, QuerySource::Expr(_)),
                "expected expression source"
            );
            let mut classes = Vec::new();
            let QuerySource::Expr(expression) = &mut source else {
                panic!("expected expression source");
            };
            expression.visit_atoms_mut(&mut |atom| {
                let SourceAtom::Class {
                    names,
                    mode,
                } = atom
                else {
                    return;
                };
                classes.push((names.clone(), mode.classes().clone()));
            });
            assert_eq!(classes, vec![
                (vec!["thing".to_owned()], set(&["book", "thing"]),),
                (vec!["ghost".to_owned()], set(&["ghost"])),
            ]);
        }
    }

    mod descendants {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn returns_a_direct_extender() {
            let temp = tempfile::tempdir().expect("create temp dir");
            write_schema(temp.path(), "book", "");
            write_schema(temp.path(), "sci_fi", r#"extends = ["book"]"#);

            let (service, _, _) =
                resolve_dir(temp.path()).expect("registry loads");

            let descendants = service.descendants_of("book");
            let names: Vec<&str> =
                descendants.iter().map(|schema| schema.name()).collect();

            assert_eq!(names, vec!["sci_fi"]);
        }

        #[test]
        fn returns_a_transitive_descendant_through_an_intermediate_schema() {
            let temp = tempfile::tempdir().expect("create temp dir");
            write_schema(temp.path(), "thing", "");
            write_schema(temp.path(), "book", r#"extends = ["thing"]"#);
            write_schema(temp.path(), "sci_fi", r#"extends = ["book"]"#);

            let (service, _, _) =
                resolve_dir(temp.path()).expect("registry loads");

            let descendants = service.descendants_of("thing");
            let names: Vec<&str> =
                descendants.iter().map(|schema| schema.name()).collect();

            assert_eq!(names, vec!["book", "sci_fi"]);
        }

        #[test]
        fn returns_empty_for_a_leaf_schema() {
            let temp = tempfile::tempdir().expect("create temp dir");
            write_schema(temp.path(), "book", "");
            write_schema(temp.path(), "sci_fi", r#"extends = ["book"]"#);

            let (service, _, _) =
                resolve_dir(temp.path()).expect("registry loads");

            assert!(service.descendants_of("sci_fi").is_empty());
        }

        #[test]
        fn returns_empty_for_a_name_with_no_schema() {
            let temp = tempfile::tempdir().expect("create temp dir");
            write_schema(temp.path(), "book", "");

            let (service, _, _) =
                resolve_dir(temp.path()).expect("registry loads");

            assert!(service.descendants_of("ghost").is_empty());
        }
    }

    /// Parses `name` into a [`FieldName`], panicking on an invalid test
    /// fixture.
    fn field_name(name: &str) -> FieldName {
        FieldName::try_from(name).expect("valid test field name")
    }

    /// Parses `reference` into a [`super::super::fields::FieldAddress`],
    /// panicking on an invalid test fixture.
    fn field_address(reference: &str) -> super::super::fields::FieldAddress {
        super::super::fields::FieldAddress::try_from(reference)
            .expect("valid test $ref")
    }

    /// Builds an `IndexMap<String, FieldValue>` options bag from `pairs`.
    fn options(
        pairs: &[(&str, crate::field::FieldValue)],
    ) -> indexmap::IndexMap<String, crate::field::FieldValue> {
        pairs.iter().map(|(k, v)| ((*k).to_owned(), v.clone())).collect()
    }

    /// Builds a [`FieldValue::List`] of [`FieldValue::String`] entries.
    fn string_list(values: &[&str]) -> crate::field::FieldValue {
        crate::field::FieldValue::List(
            values
                .iter()
                .map(|&v| crate::field::FieldValue::String(v.to_owned()))
                .collect(),
        )
    }

    /// Builds a `select`-type [`RawSchemaFieldDef`] with the given `values`.
    fn select_field(values: &[&str]) -> RawSchemaFieldDef {
        RawSchemaFieldDef {
            options: options(&[("values", string_list(values))]),
            ..RawSchemaFieldDef::direct(RawSchemaFieldType::Select)
        }
    }

    /// Builds an `input`-type [`RawSchemaFieldDef`].
    fn input_field() -> RawSchemaFieldDef {
        RawSchemaFieldDef::direct(RawSchemaFieldType::Input)
    }

    /// Builds a `file`-type [`RawSchemaFieldDef`] with the given filter.
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
            pairs.push((
                "ext",
                crate::field::FieldValue::String(ext.to_owned()),
            ));
        }
        RawSchemaFieldDef {
            options: options(&pairs),
            ..RawSchemaFieldDef::direct(RawSchemaFieldType::File)
        }
    }

    /// Builds a `$ref`-only [`RawSchemaFieldDef`] with an optional local
    /// `required` override.
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

    /// Wraps `values` as literal [`SchemaSelectFieldEntry`]s, matching what
    /// [`select_field`]'s declared array produces after resolution.
    fn select_entries(values: &[&str]) -> Vec<SchemaSelectFieldEntry> {
        values
            .iter()
            .map(|&v| SchemaSelectFieldEntry::literal(v.to_owned()))
            .collect()
    }

    mod resolve_all {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn resolves_a_schema_with_no_extends() {
            let mut raw = BTreeMap::new();
            raw.insert(
                SchemaName::from("book"),
                schema(&[], &[("status", select_field(&["draft", "done"]))]),
            );

            let (resolved, warnings, _failures) =
                resolve_all(&raw).expect("resolves");

            assert!(warnings.is_empty());
            let book = resolved.get("book").expect("book resolved");
            assert_eq!(book.name(), "book");
            let status = book.field("status").expect("status field");
            assert_eq!(
                status.kind(),
                &SchemaFieldType::Select(SchemaSelectField::for_test(
                    select_entries(&["draft", "done"])
                ))
            );
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

            let (resolved, _, _) = resolve_all(&raw).expect("resolves");

            let status = resolved
                .get("sci_fi")
                .and_then(|s| s.field("status"))
                .expect("status field");
            assert_eq!(
                status.kind(),
                &SchemaFieldType::Select(SchemaSelectField::for_test(
                    select_entries(&["outline", "shipped"])
                ))
            );
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

            let (resolved, _, _) = resolve_all(&raw).expect("resolves");

            let shared = resolved
                .get("child")
                .and_then(|s| s.field("shared"))
                .expect("shared field");
            assert_eq!(
                shared.kind(),
                &SchemaFieldType::Select(SchemaSelectField::for_test(
                    select_entries(&["from-a"])
                ))
            );
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

            let (resolved, _, _) = resolve_all(&raw).expect("resolves");

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

            let (resolved, _, _) = resolve_all(&raw).expect("resolves");

            let sci_fi = resolved.get("sci_fi").expect("sci_fi resolved");
            assert!(sci_fi.field("status").is_some());
        }

        #[test]
        fn own_redeclaration_of_an_excluded_field_survives() {
            // `excludes` removes the *inherited* field before the Schema's
            // own fields are merged in, so a Schema that both excludes and
            // redeclares the same field name keeps its own redeclaration.
            let mut raw = BTreeMap::new();
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

            let (resolved, _, _) = resolve_all(&raw).expect("resolves");

            let sci_fi = resolved.get("sci_fi").expect("sci_fi resolved");
            let status = sci_fi.field("status").expect("own status field");
            assert_eq!(status.kind(), &SchemaFieldType::Input);
        }

        #[test]
        fn a_missing_extends_target_degrades_with_a_warning() {
            let mut raw = BTreeMap::new();
            raw.insert(
                SchemaName::from("sci_fi"),
                schema(&["ghost"], &[("title", input_field())]),
            );

            let (resolved, warnings, _failures) =
                resolve_all(&raw).expect("resolves");

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
        fn a_schema_with_a_malformed_field_does_not_block_an_unrelated_sibling()
        {
            let mut raw = BTreeMap::new();
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

            let (resolved, _, failures) =
                resolve_all(&raw).expect("unrelated failure does not abort");

            assert!(resolved.contains_key("book"));
            assert!(!resolved.contains_key("broken"));
            assert_eq!(failures.len(), 1);
            let failure = failures.first().expect("one failure");
            assert_eq!(failure.schema, SchemaName::from("broken"));
        }

        #[test]
        fn a_schema_extending_a_failed_parent_still_resolves_its_own_fields() {
            let mut raw = BTreeMap::new();
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

            let (resolved, warnings, failures) =
                resolve_all(&raw).expect("child still resolves");

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
        fn a_schema_downstream_of_a_failed_link_is_not_a_structural_descendant_of_a_healthy_ancestor()
         {
            // book <- broken (FAILS: ambiguous fields) <- sci_fi (resolves,
            // own field only). `sci_fi` no longer inherits book's fields and
            // must not claim is-a book — so `book`'s own structural
            // children/descendants (and therefore `SchemaService::matches`/
            // `.children()`/`.descendants()`, which read them) must not
            // list `sci_fi` either, even though the raw `extends` topology
            // still structurally links book -> broken -> sci_fi.
            let mut raw = BTreeMap::new();
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

            let (resolved, _, failures) = resolve_all(&raw).expect("resolves");

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

        #[test]
        fn a_cycle_is_a_hard_error() {
            let mut raw = BTreeMap::new();
            raw.insert(SchemaName::from("a"), schema(&["b"], &[]));
            raw.insert(SchemaName::from("b"), schema(&["a"], &[]));

            let err = resolve_all(&raw).expect_err("cycle rejected");
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

            let (resolved, _, _) = resolve_all(&raw).expect("resolves");

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

            let (resolved, _, _) = resolve_all(&raw).expect("resolves");

            let status = resolved
                .get("sci_fi")
                .and_then(|s| s.field("status"))
                .expect("status field");
            assert_eq!(
                status.kind(),
                &SchemaFieldType::Select(SchemaSelectField::for_test(
                    select_entries(&["draft", "done"])
                ))
            );
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

            let (resolved, _, _) = resolve_all(&raw).expect("resolves");

            let priority = resolved
                .get("task")
                .and_then(|s| s.field("priority"))
                .expect("priority field");
            assert_eq!(
                priority.kind(),
                &SchemaFieldType::Select(SchemaSelectField::for_test(
                    select_entries(&["low", "high"])
                ))
            );
            assert!(priority.is_required());
        }

        #[test]
        fn a_stray_required_on_global_is_ignored_with_a_warning() {
            let mut raw = BTreeMap::new();
            raw.insert(
                SchemaName::from(GLOBAL_SCHEMA_NAME),
                schema(&[], &[("priority", RawSchemaFieldDef {
                    required: Some(true),
                    ..select_field(&["low", "high"])
                })]),
            );

            let (resolved, warnings, _failures) =
                resolve_all(&raw).expect("resolves");

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
        fn a_ref_to_an_unknown_field_degrades_to_a_failure() {
            let mut raw = BTreeMap::new();
            raw.insert(SchemaName::from("book"), schema(&[], &[]));
            raw.insert(
                SchemaName::from("sci_fi"),
                schema(&["book"], &[(
                    "status",
                    ref_field("#book/status", None),
                )]),
            );

            let (_, _, failures) =
                resolve_all(&raw).expect("unresolved ref degrades, not aborts");
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
        fn a_ref_to_a_non_ancestor_sibling_degrades_to_a_failure() {
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

            let (_, _, failures) = resolve_all(&raw)
                .expect("out-of-bounds ref degrades, not aborts");
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
        fn defining_both_status_and_status_cased_differently_degrades_to_a_failure()
         {
            let mut raw = BTreeMap::new();
            raw.insert(
                SchemaName::from("book"),
                schema(&[], &[
                    ("status", input_field()),
                    ("Status", input_field()),
                ]),
            );

            let (_, _, failures) = resolve_all(&raw)
                .expect("ambiguous field name degrades, not aborts");
            let err = failures.into_iter().next().expect("one failure").error;
            assert!(matches!(err, SchemaError::AmbiguousFieldName { .. }));
        }

        #[test]
        fn an_own_field_colliding_with_an_inherited_field_degrades_to_a_failure()
         {
            let mut raw = BTreeMap::new();
            raw.insert(
                SchemaName::from("book"),
                schema(&[], &[("Due Date", input_field())]),
            );
            raw.insert(
                SchemaName::from("sci_fi"),
                schema(&["book"], &[("due-date", input_field())]),
            );

            let (_, _, failures) = resolve_all(&raw)
                .expect("ambiguous field name degrades, not aborts");
            let err = failures.into_iter().next().expect("one failure").error;
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
                    (
                        "archived",
                        RawSchemaFieldDef::direct(RawSchemaFieldType::Boolean),
                    ),
                    (
                        "rating",
                        RawSchemaFieldDef::direct(RawSchemaFieldType::Number),
                    ),
                    (
                        "published",
                        RawSchemaFieldDef::direct(RawSchemaFieldType::Date),
                    ),
                    (
                        "cover",
                        file_field(&["assets/covers"], Some("png"), &["image"]),
                    ),
                ]),
            );

            let (resolved, _, _) = resolve_all(&raw).expect("resolves");
            let book = resolved.get("book").expect("book resolved");

            assert_eq!(
                book.field("title").map(SchemaFieldDef::kind),
                Some(&SchemaFieldType::Input)
            );
            assert_eq!(
                book.field("status").map(SchemaFieldDef::kind),
                Some(&SchemaFieldType::Select(SchemaSelectField::for_test(
                    select_entries(&["draft", "done"])
                )))
            );
            assert_eq!(
                book.field("archived").map(SchemaFieldDef::kind),
                Some(&SchemaFieldType::Boolean)
            );
            assert_eq!(
                book.field("rating").map(SchemaFieldDef::kind),
                Some(&SchemaFieldType::Number(SchemaNumberField::for_test(
                    None, None, None
                )))
            );
            assert_eq!(
                book.field("published").map(SchemaFieldDef::kind),
                Some(&SchemaFieldType::Date(SchemaDateField::for_test(None)))
            );
            assert_eq!(
                book.field("cover").map(SchemaFieldDef::kind),
                Some(&SchemaFieldType::File(SchemaFileField::for_test(
                    vec!["assets/covers".to_owned()],
                    Some("png".to_owned()),
                    vec!["image".to_owned()],
                )))
            );
        }

        #[test]
        fn multi_defaults_to_false_and_honors_a_local_override() {
            let mut raw = BTreeMap::new();
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

            let (resolved, _, _) = resolve_all(&raw).expect("resolves");
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

            let (resolved, _, _) = resolve_all(&raw).expect("resolves");
            let cover = resolved
                .get("book")
                .and_then(|s| s.field("cover"))
                .expect("cover field");

            assert_eq!(
                cover.kind(),
                &SchemaFieldType::File(SchemaFileField::for_test(
                    vec!["assets/covers".to_owned()],
                    Some("png".to_owned()),
                    vec!["image".to_owned()],
                ))
            );
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

            let (resolved, warnings, _failures) =
                resolve_all(&raw).expect("resolves");

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

            let (resolved, _, _) = resolve_all(&raw).expect("resolves");

            let priority = resolved
                .get("poem")
                .and_then(|s| s.field("priority"))
                .expect("priority field resolves via $ref to global");
            assert_eq!(
                priority.kind(),
                &SchemaFieldType::Select(SchemaSelectField::for_test(
                    select_entries(&["low", "high"])
                ))
            );
        }

        #[test]
        fn a_ref_to_global_resolves_when_the_referrer_sorts_before_it_alphabetically()
         {
            // Both Schemas have no `extends`, so both start at Kahn in-degree
            // zero. `"author"` sorts before `"global"` in the name-ordered
            // `BTreeMap` `resolve_all` iterates, so without the explicit
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

            let (resolved, _, _) = resolve_all(&raw).expect("resolves");

            let name = resolved
                .get("author")
                .and_then(|s| s.field("name"))
                .expect("name field resolves via $ref to global");
            assert_eq!(
                name.kind(),
                &SchemaFieldType::Select(SchemaSelectField::for_test(
                    select_entries(&["anon"])
                ))
            );
        }

        #[test]
        fn a_ref_that_switches_field_type_starts_from_empty_base_options() {
            // Per `parse_field_type`'s doc comment: a `$ref` that switches
            // `type` starts from empty options rather than reusing a
            // mismatched base, so a `select`'s `values` can never leak into
            // an overriding `file` field.
            let mut raw = BTreeMap::new();
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

            let (resolved, _, _) = resolve_all(&raw).expect("resolves");

            let status = resolved
                .get("sci_fi")
                .and_then(|s| s.field("status"))
                .expect("status field");
            assert_eq!(
                status.kind(),
                &SchemaFieldType::File(SchemaFileField::for_test(
                    vec!["assets".to_owned()],
                    None,
                    Vec::new(),
                ))
            );
        }

        #[test]
        fn a_bare_ref_override_with_an_unknown_key_degrades_with_a_warning() {
            let mut raw = BTreeMap::new();
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

            let (resolved, warnings, _failures) =
                resolve_all(&raw).expect("resolves");

            let status = resolved
                .get("sci_fi")
                .and_then(|s| s.field("status"))
                .expect("status field still resolves from the base");
            assert_eq!(
                status.kind(),
                &SchemaFieldType::Select(SchemaSelectField::for_test(
                    select_entries(&["draft", "done"])
                ))
            );
            assert_eq!(warnings.len(), 1);
            assert!(matches!(
                warnings.first().expect("expected warning"),
                SchemaWarning::UnknownOverrideKey { .. }
            ));
        }

        #[test]
        fn a_bare_ref_override_with_a_type_mismatched_value_degrades_with_a_warning()
         {
            let mut raw = BTreeMap::new();
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

            let (resolved, warnings, _failures) =
                resolve_all(&raw).expect("resolves");

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
        fn a_bare_ref_override_still_applies_its_other_valid_keys_alongside_a_dropped_unknown_key()
         {
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

            let (resolved, warnings, _failures) =
                resolve_all(&raw).expect("resolves");

            let cover = resolved
                .get("book")
                .and_then(|s| s.field("cover"))
                .expect("cover field still resolves from the base");
            assert_eq!(
                cover.kind(),
                &SchemaFieldType::File(SchemaFileField::for_test(
                    vec!["assets/covers".to_owned()],
                    Some("png".to_owned()),
                    vec!["image".to_owned()],
                ))
            );
            assert_eq!(warnings.len(), 1);
            assert!(matches!(
                warnings.first().expect("expected warning"),
                SchemaWarning::UnknownOverrideKey { key, .. } if key == "bogus"
            ));
        }

        #[test]
        #[expect(
            clippy::unreachable,
            reason = "exhaustive error-match fallback"
        )]
        fn a_ref_with_a_type_override_and_an_unknown_key_degrades_to_a_failure()
        {
            let mut raw = BTreeMap::new();
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

            let (_, _, failures) = resolve_all(&raw).expect(
                "unknown attribute key on a type-overriding $ref degrades, \
                 not aborts",
            );
            let err = failures.into_iter().next().expect("one failure").error;

            if let SchemaError::FieldBuilder(inner) = &err
                && let SchemaFieldBuilderError::Parser(errors) = inner.as_ref()
            {
                assert!(
                    matches!(errors.as_slice(), [
                        SchemaFieldParserError::UnknownKey { .. }
                    ]),
                    "expected a single UnknownKey, got {errors:?}"
                );
            } else {
                unreachable!("expected Parser(UnknownKey), got {err}");
            }
        }

        #[test]
        #[expect(
            clippy::unreachable,
            reason = "exhaustive error-match fallback"
        )]
        fn a_direct_field_with_an_unknown_key_degrades_to_a_failure() {
            let mut raw = BTreeMap::new();
            raw.insert(
                SchemaName::from("book"),
                schema(&[], &[("published", RawSchemaFieldDef {
                    options: options(&[("values", string_list(&["draft"]))]),
                    ..RawSchemaFieldDef::direct(RawSchemaFieldType::Date)
                })]),
            );

            let (_, _, failures) = resolve_all(&raw)
                .expect("unknown attribute key degrades, not aborts");
            let err = failures.into_iter().next().expect("one failure").error;

            if let SchemaError::FieldBuilder(inner) = &err
                && let SchemaFieldBuilderError::Parser(errors) = inner.as_ref()
            {
                assert!(
                    matches!(errors.as_slice(), [
                        SchemaFieldParserError::UnknownKey { .. }
                    ]),
                    "expected a single UnknownKey, got {errors:?}"
                );
            } else {
                unreachable!("expected Parser(UnknownKey), got {err}");
            }
        }

        #[test]
        #[expect(
            clippy::unreachable,
            reason = "exhaustive error-match fallback"
        )]
        fn a_direct_field_with_a_type_mismatched_value_degrades_to_a_failure() {
            let mut raw = BTreeMap::new();
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

            let (_, _, failures) = resolve_all(&raw)
                .expect("attribute value type mismatch degrades, not aborts");
            let err = failures.into_iter().next().expect("one failure").error;

            if let SchemaError::FieldBuilder(inner) = &err
                && let SchemaFieldBuilderError::Parser(errors) = inner.as_ref()
            {
                assert!(
                    matches!(errors.as_slice(), [
                        SchemaFieldParserError::TypeMismatch { .. }
                    ]),
                    "expected a single TypeMismatch, got {errors:?}"
                );
            } else {
                unreachable!("expected Parser(TypeMismatch), got {err}");
            }
        }
    }
}
