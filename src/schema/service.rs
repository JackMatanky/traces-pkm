//! Load, resolve, and query Schemas: the `schema` domain's public facade.
//!
//! [`SchemaService`] is the one type the rest of the crate constructs to work
//! with Schemas. It wraps an owned [`SchemaConfigSpec`] (so `new` is trivial
//! and does no I/O) and exposes:
//!
//! - [`SchemaService::resolve`]: the impure edge — reads every
//!   `.traces/schemas/*.toml` file under the configured directory
//!   ([`read_raw_schemas`]), then linearizes the `extends` DAG with Kahn's
//!   topological sort ([`resolve_all`], driven by
//!   [`super::graph::SchemaGraph`]) into a [`SchemaRegistry`].
//! - [`SchemaService::get`]/[`SchemaService::children`]/
//!   [`SchemaService::descendants`]/[`SchemaService::matches`]/
//!   [`SchemaService::expand_classes`]: read-side queries over an already
//!   [`SchemaService::resolve`]d [`SchemaRegistry`].
//!
//! [`SchemaRegistry`] itself is a pure lookup table (name to resolved
//! [`super::model::Schema`]); every hierarchy/class-matching query lives on
//! [`SchemaService`] instead, so a caller reaches one facade for both loading
//! and querying.

use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::OsStr,
    fs, io,
    path::Path,
    sync::Arc,
};

use walkdir::WalkDir;

use super::{
    address::FieldAddressRef,
    error::{SchemaError, SchemaWarning},
    fields::{RefResolver, SchemaFieldBuilder},
    graph::SchemaGraph,
    model::Schema,
    name::{SchemaName, SchemaNameRef},
    raw::RawSchema,
};
use crate::{
    BaseNameRef,
    config::SchemaConfigSpec,
    field::FieldName,
    query::{ClassExpansionMode, QuerySource, SourceAtom},
};

/// Store every Schema [`SchemaService::resolve`] resolved, keyed by name.
///
/// A pure lookup table: reference-counted per Schema, not owned outright, so
/// [`SchemaService`]'s hierarchy queries share one Schema's field map across
/// every caller in a render instead of deep-cloning it per lookup, mirroring
/// [`crate::query::IndexRecord`]'s `Arc<Note>`.
#[derive(Clone, Debug)]
pub(crate) struct SchemaRegistry {
    schemas: BTreeMap<SchemaName, Arc<Schema>>,
}

impl SchemaRegistry {
    fn new(schemas: BTreeMap<SchemaName, Arc<Schema>>) -> Self {
        Self {
            schemas,
        }
    }

    /// Return a reference to the named Schema, or `None` if no Schema by that
    /// name resolved.
    #[inline]
    #[must_use]
    pub(crate) fn get(&self, name: &str) -> Option<&Arc<Schema>> {
        self.schemas.get(name)
    }
}

/// Facade over Schema loading, resolution, and hierarchy/class queries.
///
/// Wraps an owned [`SchemaConfigSpec`]; `new` is trivial and does no I/O.
/// [`Self::resolve`] does the actual filesystem read and DAG linearization,
/// returning a [`SchemaRegistry`] the rest of this type's methods query.
#[derive(Debug)]
pub struct SchemaService {
    spec: SchemaConfigSpec,
}

/// The pair [`SchemaService::resolve`] returns: the resolved registry and
/// any warnings degraded resolution accumulated along the way.
type SchemaResolution = (Arc<SchemaRegistry>, Vec<SchemaWarning>);

impl SchemaService {
    /// Wraps `spec`. Does no I/O; call [`Self::resolve`] to actually load
    /// Schemas.
    ///
    /// `pub(crate)`, not `pub`, despite [`SchemaService`] itself being `pub`
    /// (gated) at the crate root: [`SchemaConfigSpec`] stays `pub(crate)`
    /// (see its own docs), so a constructor taking one by value can never be
    /// more public than that without leaking a private type through a public
    /// signature. The `pub` on the type itself exists so external code can
    /// *name* and pass around a [`SchemaService`]/[`Schema`] it obtained some
    /// other way, not to construct one directly.
    #[inline]
    #[must_use]
    pub(crate) fn new(spec: SchemaConfigSpec) -> Self {
        Self {
            spec,
        }
    }

    /// Returns the config projection this service was built from.
    ///
    /// `template/engine/schema.rs`'s only route to `root()` (`FileIndex`
    /// refresh) and `class_field()`/`title_field()`/`aliases_field()`
    /// (building a `FrontmatterFieldKeys` on the fly for `file`-field label
    /// resolution).
    #[inline]
    #[must_use]
    pub(crate) fn spec(&self) -> &SchemaConfigSpec {
        &self.spec
    }

    /// Load every Schema TOML file directly under
    /// [`SchemaConfigSpec::directory`] and resolve the `extends` DAG.
    ///
    /// Reads every `*.toml` file directly under the configured directory
    /// (non-recursive), parses each as a Schema keyed by its filename stem,
    /// resolves inheritance, and populates each Schema's `children`/
    /// `descendants` from the whole DAG in one pass.
    ///
    /// A missing directory resolves to an empty registry rather than an
    /// error: an unconfigured or not-yet-created Schema directory is absence,
    /// not corruption.
    ///
    /// # Errors
    ///
    /// - [`SchemaError::ReadDirectory`] if the directory exists but its entries
    ///   cannot be listed.
    /// - [`SchemaError::ReadFile`] if a `.toml` file cannot be read.
    /// - [`SchemaError::Parse`] if a Schema file's TOML is malformed, contains
    ///   an unknown key, has a malformed `$ref`, or defines a field with
    ///   neither `type` nor `$ref`.
    /// - [`SchemaError::Cycle`] if the `extends` DAG contains a cycle.
    /// - [`SchemaError::FieldBuilder`] if a `Direct` field or a `$ref` with a
    ///   local `type` override declares an attribute key that doesn't belong to
    ///   its type, a wrongly-shaped attribute value, an out-of-bounds `$ref`,
    ///   or a `$ref` to a field that doesn't exist.
    /// - [`SchemaError::AmbiguousFieldName`] if two effective fields share the
    ///   same canonical metadata key.
    ///
    /// `pub(crate)`, not `pub`, for the same reason as [`Self::new`]:
    /// [`SchemaRegistry`], [`SchemaError`], and [`SchemaWarning`] all stay
    /// `pub(crate)`, so this method's return type can't be any more public
    /// without leaking one of them.
    pub(crate) fn resolve(&self) -> Result<SchemaResolution, SchemaError> {
        let raw = read_raw_schemas(self.spec.directory())?;
        let (schemas, warnings) = resolve_all(&raw)?;
        let schemas = schemas
            .into_iter()
            .map(|(name, schema)| (name, Arc::new(schema)))
            .collect();
        Ok((Arc::new(SchemaRegistry::new(schemas)), warnings))
    }

    /// Return a reference to the named Schema in `registry`, or `None` if no
    /// Schema by that name resolved.
    #[inline]
    #[must_use]
    #[expect(
        clippy::unused_self,
        reason = "kept as &self for the uniform SchemaService facade call \
                  convention (get/children/descendants/matches all take &self \
                  + &SchemaRegistry), even though this particular method \
                  never reads spec"
    )]
    pub(crate) fn get<'a>(
        &self,
        registry: &'a SchemaRegistry,
        name: &str,
    ) -> Option<&'a Arc<Schema>> {
        registry.get(name)
    }

    /// Return every Schema in `registry` that directly extends `name`.
    ///
    /// Excludes `name` itself and every transitive descendant. Empty, not an
    /// error, if `name` has no Schema or nothing extends it.
    #[must_use]
    #[expect(
        clippy::unused_self,
        reason = "kept as &self for the uniform SchemaService facade call \
                  convention; see get()'s doc"
    )]
    pub(crate) fn children(
        &self,
        registry: &SchemaRegistry,
        name: &str,
    ) -> Vec<Arc<Schema>> {
        let Some(schema) = registry.get(name) else {
            return Vec::new();
        };
        schema
            .children()
            .iter()
            .filter_map(|child| registry.get(child.as_str()))
            .cloned()
            .collect()
    }

    /// Return every Schema in `registry` that directly or transitively
    /// extends `name`.
    ///
    /// Excludes `name` itself. Empty, not an error, if `name` has no Schema
    /// or nothing extends it.
    #[must_use]
    #[expect(
        clippy::unused_self,
        reason = "kept as &self for the uniform SchemaService facade call \
                  convention; see get()'s doc"
    )]
    pub(crate) fn descendants(
        &self,
        registry: &SchemaRegistry,
        name: &str,
    ) -> Vec<Arc<Schema>> {
        let Some(schema) = registry.get(name) else {
            return Vec::new();
        };
        schema
            .descendants()
            .iter()
            .filter_map(|descendant| registry.get(descendant.as_str()))
            .cloned()
            .collect()
    }

    /// Return the set of Schema names in `registry` that match `classes`.
    ///
    /// The set includes:
    ///
    /// - Every name in `classes` itself (so a class with no Schema still
    ///   matches itself).
    /// - Every resolved Schema that is-a one of the class names.
    ///
    /// A File Class source query tests each Note's File Class against this set:
    /// a Note matches when any of its class values is in the returned set.
    /// Transitive `extends` is folded in here (via each named class's
    /// precomputed [`Schema::descendants`]) so the caller compares plain
    /// strings without consulting the registry per Note.
    ///
    /// Warns once per name in `classes` with no corresponding Schema, so every
    /// caller (a `from_class` query source, a `file`-field `class` filter)
    /// gets the same degrade-to-exact-match diagnostic without each
    /// implementing its own warning loop.
    ///
    /// # Examples
    ///
    /// Given `sci_fi` extending `book`, and `movie` unrelated:
    ///
    /// - `matches(&["book"])` returns `{"book", "sci_fi"}`.
    /// - `matches(&["movie"])` returns `{"movie"}`.
    /// - `matches(&["ghost"])` returns `{"ghost"}` (no Schema, still matches)
    #[must_use]
    #[expect(
        clippy::unused_self,
        reason = "kept as &self for the uniform SchemaService facade call \
                  convention; see get()'s doc"
    )]
    pub(crate) fn matches(
        &self,
        registry: &SchemaRegistry,
        classes: &[String],
    ) -> BTreeSet<String> {
        warn_unknown_classes(registry, classes);
        let mut matches: BTreeSet<String> = classes.iter().cloned().collect();
        for class in classes {
            if let Some(schema) = registry.get(class) {
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
    /// Unknown class names remain in the set so a Note may still use them even
    /// without a corresponding Schema.
    pub(crate) fn expand_classes(
        &self,
        registry: &SchemaRegistry,
        classes: &[String],
        mode: &mut ClassExpansionMode,
    ) {
        let mut expanded: BTreeSet<String> = classes.iter().cloned().collect();
        match mode {
            ClassExpansionMode::Exact(_) => {
                warn_unknown_classes(registry, classes);
            }
            ClassExpansionMode::Children(_) => {
                warn_unknown_classes(registry, classes);
                for class in classes {
                    expanded.extend(
                        self.children(registry, class)
                            .iter()
                            .map(|schema| schema.name().to_owned()),
                    );
                }
            }
            ClassExpansionMode::Descendants(_) => {
                // `matches` warns internally, so this branch alone would
                // otherwise skip the warning the other two branches emit
                // directly above.
                expanded = self.matches(registry, classes);
            }
        }
        mode.set_classes(expanded);
    }
}

/// Warns once per name in `classes` with no corresponding Schema in
/// `registry`. Shared by [`SchemaService::matches`] and
/// [`SchemaService::expand_classes`]'s `Exact`/`Children` branches so every
/// class-matching entry point emits the same diagnostic exactly once per
/// call, regardless of which one a caller reaches.
fn warn_unknown_classes(registry: &SchemaRegistry, classes: &[String]) {
    for class in classes {
        if registry.get(class).is_none() {
            tracing::warn!(
                class,
                "query source names an unknown File Class; matching it exactly"
            );
        }
    }
}

/// Resolve every File Class leaf in `source` against `registry`.
///
/// This caller-side pre-pass keeps query parsing and matching independent of
/// the Schema registry.
pub(crate) fn resolve_sources(
    source: &mut QuerySource,
    service: &SchemaService,
    registry: &SchemaRegistry,
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
            service.expand_classes(registry, names, mode);
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
/// - [`SchemaError::ReadDirectory`] if `dir` exists but its entries cannot be
///   listed.
/// - [`SchemaError::ReadFile`] if a `.toml` file cannot be read.
/// - [`SchemaError::Parse`] if a Schema file's TOML is malformed, contains an
///   unknown key, has a malformed `$ref`, or omits both `type` and `$ref` for a
///   field.
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

/// Return `true` if `error` reports that the walk's root itself does not
/// exist, so [`read_raw_schemas`] can degrade to an empty registry.
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

/// Return every Schema resolved by [`resolve_all`], keyed by name, alongside
/// the [`SchemaWarning`]s degraded resolution accumulated along the way.
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
/// Once every Schema resolved (confirmed acyclic), populates each Schema's
/// `children`/`descendants` in one pass over the whole DAG via
/// [`SchemaGraph::children_by_name`]/[`SchemaGraph::descendants_by_name`].
///
/// # Errors
///
/// - [`SchemaError::Cycle`] if the `extends` DAG contains a cycle.
/// - Any [`SchemaError`] [`SchemaFieldBuilder::build`] returns while resolving
///   a Schema's own fields.
/// - [`SchemaError::AmbiguousFieldName`] if two of a Schema's effective fields
///   share a [`crate::field::FieldKey`] canonical form.
fn resolve_all(
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

    if let Some(schemas) = graph.cyclic_remainder(raw_schemas) {
        return Err(SchemaError::Cycle {
            schemas,
        });
    }

    let children_by_name = graph.children_by_name();
    let descendants_by_name = graph.descendants_by_name();
    for (name, schema) in &mut resolved {
        schema.set_hierarchy(
            children_by_name.get(name).cloned().unwrap_or_default(),
            descendants_by_name.get(name).cloned().unwrap_or_default(),
        );
    }

    Ok((resolved, warnings))
}

/// Resolve one Schema's effective fields and transitive ancestors.
///
/// Merges `parents`' fields first-listed-wins, applies `raw.excludes`, then
/// overrides the result with `raw`'s own (`$ref`-resolved) fields.
///
/// `parents` must already be resolved in `resolved`: [`resolve_all`]
/// guarantees this by calling in Kahn topological order.
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
/// Propagates any [`SchemaError`] that [`SchemaFieldBuilder::build`] returns
/// while resolving `raw`'s own fields, or [`SchemaError::AmbiguousFieldName`]
/// if two of the resolved fields share a [`crate::field::FieldKey`] canonical
/// form.
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
    let mut builder = SchemaFieldBuilder {
        refs: &refs,
        warnings,
    };
    let mut own_fields = BTreeMap::new();
    for (field_name, raw_field) in &raw.fields {
        let address = FieldAddressRef::new(name, field_name.as_ref());
        let field = builder.build(address, raw_field)?;
        own_fields.insert(field_name.clone(), field);
    }
    fields.extend(own_fields);

    reject_ambiguous_canonical_names(name, &fields)?;

    Ok(Schema::new(SchemaName::from(name), fields, ancestors))
}

/// Reject `fields` if two entries share a
/// [`FieldKey`](crate::field::FieldKey) canonical form: ambiguous field
/// identities would make later note-vs-schema field matching and
/// unknown-field suggestions unreliable.
///
/// # Errors
///
/// Returns [`SchemaError::AmbiguousFieldName`] naming the first two
/// (name-sorted) colliding field names.
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
        error::SchemaFieldBuilderError,
        fields::{SchemaFieldType, SchemaSelectFieldEntry},
        raw::{RawFieldSource, RawSchemaFieldDef, RawSchemaFieldType},
    };

    /// Resolves every Schema TOML file directly under `dir`, mirroring the
    /// pre-refactor `SchemaRegistry::load(dir)` call shape: `dir` is used
    /// directly as the Schema directory, `root` is unused by resolution
    /// itself.
    fn resolve_dir(
        dir: &Path,
    ) -> Result<(Arc<SchemaRegistry>, Vec<SchemaWarning>), SchemaError> {
        SchemaService::new(SchemaConfigSpec::for_test(dir, dir)).resolve()
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

            let (registry, warnings) =
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

            let (registry, _) =
                resolve_dir(temp.path()).expect("registry loads");

            assert!(registry.get("README").is_none());
        }

        #[test]
        fn resolves_to_an_empty_registry_when_the_directory_is_missing() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let missing = temp.path().join("does-not-exist");

            let (registry, warnings) =
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
                r#"
                [fields.status]
                required = true
                "#,
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

            let (registry, _) =
                resolve_dir(temp.path()).expect("registry loads");

            assert!(registry.get("hidden").is_none());
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
            let (registry, _) =
                resolve_dir(temp.path()).expect("registry loads");
            let service = SchemaService::new(SchemaConfigSpec::for_test(
                temp.path(),
                temp.path(),
            ));

            let first = service.get(&registry, "book").expect("book resolved");
            let second = service.get(&registry, "book").expect("book resolved");

            assert!(
                Arc::ptr_eq(first, second),
                "repeated lookups must share one Arc-backed Schema, not clone \
                 a fresh one per call"
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

        fn service(dir: &Path) -> SchemaService {
            SchemaService::new(SchemaConfigSpec::for_test(dir, dir))
        }

        #[test]
        fn includes_a_class_with_no_schema() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let (registry, _) =
                resolve_dir(temp.path()).expect("registry loads");

            let matches =
                service(temp.path()).matches(&registry, &["ghost".to_owned()]);

            assert_eq!(matches, set(&["ghost"]));
        }

        #[test]
        fn includes_transitive_subclasses_of_a_class() {
            let temp = tempfile::tempdir().expect("create temp dir");
            write_schema(temp.path(), "book", "");
            write_schema(temp.path(), "sci_fi", r#"extends = ["book"]"#);

            let (registry, _) =
                resolve_dir(temp.path()).expect("registry loads");

            let matches =
                service(temp.path()).matches(&registry, &["book".to_owned()]);

            assert_eq!(matches, set(&["book", "sci_fi"]));
        }

        #[test]
        fn excludes_classes_unrelated_to_the_class() {
            let temp = tempfile::tempdir().expect("create temp dir");
            write_schema(temp.path(), "book", "");
            write_schema(temp.path(), "movie", "");

            let (registry, _) =
                resolve_dir(temp.path()).expect("registry loads");

            let matches =
                service(temp.path()).matches(&registry, &["book".to_owned()]);

            assert_eq!(matches, set(&["book"]));
        }

        #[test]
        fn unions_the_matches_of_every_class() {
            let temp = tempfile::tempdir().expect("create temp dir");
            write_schema(temp.path(), "book", "");
            write_schema(temp.path(), "sci_fi", r#"extends = ["book"]"#);
            write_schema(temp.path(), "movie", "");

            let (registry, _) =
                resolve_dir(temp.path()).expect("registry loads");

            let matches = service(temp.path())
                .matches(&registry, &["book".to_owned(), "movie".to_owned()]);

            assert_eq!(matches, set(&["book", "movie", "sci_fi"]));
        }

        #[test]
        fn returns_an_empty_set_for_no_classes() {
            let temp = tempfile::tempdir().expect("create temp dir");
            write_schema(temp.path(), "book", "");

            let (registry, _) =
                resolve_dir(temp.path()).expect("registry loads");

            let matches = service(temp.path()).matches(&registry, &[]);

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

        fn registry_and_service(
            temp: &tempfile::TempDir,
        ) -> (Arc<SchemaRegistry>, SchemaService) {
            write_schema(temp.path(), "thing", "");
            write_schema(temp.path(), "book", r#"extends = ["thing"]"#);
            write_schema(temp.path(), "sci_fi", r#"extends = ["book"]"#);
            write_schema(temp.path(), "space_opera", r#"extends = ["sci_fi"]"#);
            let (registry, _) =
                resolve_dir(temp.path()).expect("registry loads");
            let service = SchemaService::new(SchemaConfigSpec::for_test(
                temp.path(),
                temp.path(),
            ));
            (registry, service)
        }

        #[test]
        fn children_returns_only_direct_extenders() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let (registry, service) = registry_and_service(&temp);

            let names: Vec<String> = service
                .children(&registry, "thing")
                .into_iter()
                .map(|schema| schema.name().to_owned())
                .collect();

            assert_eq!(names, vec!["book".to_owned()]);
        }

        #[test]
        fn expansion_modes_are_incremental() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let (registry, service) = registry_and_service(&temp);
            let names = vec!["thing".to_owned()];
            let mut exact = ClassExpansionMode::Exact(BTreeSet::new());
            let mut children = ClassExpansionMode::Children(BTreeSet::new());
            let mut descendants =
                ClassExpansionMode::Descendants(BTreeSet::new());

            service.expand_classes(&registry, &names, &mut exact);
            service.expand_classes(&registry, &names, &mut children);
            service.expand_classes(&registry, &names, &mut descendants);

            assert_eq!(exact.classes(), &set(&["thing"]));
            assert_eq!(children.classes(), &set(&["book", "thing"]));
            assert_eq!(
                descendants.classes(),
                &set(&["book", "sci_fi", "space_opera", "thing"])
            );
        }

        #[test]
        fn resolve_sources_walks_nested_expressions_and_preserves_unknowns() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let (registry, service) = registry_and_service(&temp);
            let mut source = QuerySource::parse("@thing+ and not @ghost*")
                .expect("source parses");

            resolve_sources(&mut source, &service, &registry);

            let QuerySource::Expr(expression) = &mut source else {
                panic!("expected expression source");
            };
            let mut classes = Vec::new();
            expression.visit_atoms_mut(&mut |atom| {
                if let SourceAtom::Class {
                    names,
                    mode,
                } = atom
                {
                    classes.push((names.clone(), mode.classes().clone()));
                }
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

        fn service(dir: &Path) -> SchemaService {
            SchemaService::new(SchemaConfigSpec::for_test(dir, dir))
        }

        #[test]
        fn returns_a_direct_extender() {
            let temp = tempfile::tempdir().expect("create temp dir");
            write_schema(temp.path(), "book", "");
            write_schema(temp.path(), "sci_fi", r#"extends = ["book"]"#);

            let (registry, _) =
                resolve_dir(temp.path()).expect("registry loads");

            let descendants =
                service(temp.path()).descendants(&registry, "book");
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

            let (registry, _) =
                resolve_dir(temp.path()).expect("registry loads");

            let descendants =
                service(temp.path()).descendants(&registry, "thing");
            let names: Vec<&str> =
                descendants.iter().map(|schema| schema.name()).collect();

            assert_eq!(names, vec!["book", "sci_fi"]);
        }

        #[test]
        fn returns_empty_for_a_leaf_schema() {
            let temp = tempfile::tempdir().expect("create temp dir");
            write_schema(temp.path(), "book", "");
            write_schema(temp.path(), "sci_fi", r#"extends = ["book"]"#);

            let (registry, _) =
                resolve_dir(temp.path()).expect("registry loads");

            assert!(
                service(temp.path())
                    .descendants(&registry, "sci_fi")
                    .is_empty()
            );
        }

        #[test]
        fn returns_empty_for_a_name_with_no_schema() {
            let temp = tempfile::tempdir().expect("create temp dir");
            write_schema(temp.path(), "book", "");

            let (registry, _) =
                resolve_dir(temp.path()).expect("registry loads");

            assert!(
                service(temp.path()).descendants(&registry, "ghost").is_empty()
            );
        }
    }

    /// Parses `name` into a [`FieldName`], panicking on an invalid test
    /// fixture.
    fn field_name(name: &str) -> FieldName {
        FieldName::try_from(name).expect("valid test field name")
    }

    /// Parses `reference` into a [`super::super::address::FieldAddress`],
    /// panicking on an invalid test fixture.
    fn field_address(reference: &str) -> super::super::address::FieldAddress {
        super::super::address::FieldAddress::try_from(reference)
            .expect("valid test $ref")
    }

    /// Builds a `BTreeMap<String, FieldValue>` options bag from `pairs`.
    fn options(
        pairs: &[(&str, crate::field::FieldValue)],
    ) -> BTreeMap<String, crate::field::FieldValue> {
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

            let (resolved, warnings) = resolve_all(&raw).expect("resolves");

            assert!(warnings.is_empty());
            let book = resolved.get("book").expect("book resolved");
            assert_eq!(book.name(), "book");
            let status = book.field("status").expect("status field");
            assert_eq!(status.kind(), &SchemaFieldType::Select {
                values: select_entries(&["draft", "done"])
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

            let (resolved, _) = resolve_all(&raw).expect("resolves");

            let status = resolved
                .get("sci_fi")
                .and_then(|s| s.field("status"))
                .expect("status field");
            assert_eq!(status.kind(), &SchemaFieldType::Select {
                values: select_entries(&["outline", "shipped"])
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

            let (resolved, _) = resolve_all(&raw).expect("resolves");

            let shared = resolved
                .get("child")
                .and_then(|s| s.field("shared"))
                .expect("shared field");
            assert_eq!(shared.kind(), &SchemaFieldType::Select {
                values: select_entries(&["from-a"])
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

            let (resolved, _) = resolve_all(&raw).expect("resolves");

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

            let (resolved, _) = resolve_all(&raw).expect("resolves");

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

            let (resolved, warnings) = resolve_all(&raw).expect("resolves");

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

            let (resolved, _) = resolve_all(&raw).expect("resolves");

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

            let (resolved, _) = resolve_all(&raw).expect("resolves");

            let status = resolved
                .get("sci_fi")
                .and_then(|s| s.field("status"))
                .expect("status field");
            assert_eq!(status.kind(), &SchemaFieldType::Select {
                values: select_entries(&["draft", "done"])
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

            let (resolved, _) = resolve_all(&raw).expect("resolves");

            let priority = resolved
                .get("task")
                .and_then(|s| s.field("priority"))
                .expect("priority field");
            assert_eq!(priority.kind(), &SchemaFieldType::Select {
                values: select_entries(&["low", "high"])
            });
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

            let (resolved, warnings) = resolve_all(&raw).expect("resolves");

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

            let err = resolve_all(&raw).expect_err("unresolved ref rejected");
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

            let err =
                resolve_all(&raw).expect_err("out-of-bounds ref rejected");
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
        fn defining_both_status_and_status_cased_differently_is_a_hard_error() {
            let mut raw = BTreeMap::new();
            raw.insert(
                SchemaName::from("book"),
                schema(&[], &[
                    ("status", input_field()),
                    ("Status", input_field()),
                ]),
            );

            let err =
                resolve_all(&raw).expect_err("ambiguous field name rejected");
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

            let err =
                resolve_all(&raw).expect_err("ambiguous field name rejected");
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

            let (resolved, _) = resolve_all(&raw).expect("resolves");
            let book = resolved.get("book").expect("book resolved");

            assert_eq!(
                book.field("title").map(|f| f.kind()),
                Some(&SchemaFieldType::Input)
            );
            assert_eq!(
                book.field("status").map(|f| f.kind()),
                Some(&SchemaFieldType::Select {
                    values: select_entries(&["draft", "done"])
                })
            );
            assert_eq!(
                book.field("archived").map(|f| f.kind()),
                Some(&SchemaFieldType::Boolean)
            );
            assert_eq!(
                book.field("rating").map(|f| f.kind()),
                Some(&SchemaFieldType::Number {
                    step: None,
                    min: None,
                    max: None,
                })
            );
            assert_eq!(
                book.field("published").map(|f| f.kind()),
                Some(&SchemaFieldType::Date {
                    format: None
                })
            );
            assert_eq!(
                book.field("cover").map(|f| f.kind()),
                Some(&SchemaFieldType::File {
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
                    ("authors", RawSchemaFieldDef {
                        multi: Some(true),
                        ..select_field(&["ann", "bo"])
                    }),
                ]),
            );

            let (resolved, _) = resolve_all(&raw).expect("resolves");
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

            let (resolved, _) = resolve_all(&raw).expect("resolves");
            let cover = resolved
                .get("book")
                .and_then(|s| s.field("cover"))
                .expect("cover field");

            assert_eq!(cover.kind(), &SchemaFieldType::File {
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

            let (resolved, warnings) = resolve_all(&raw).expect("resolves");

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

            let (resolved, _) = resolve_all(&raw).expect("resolves");

            let priority = resolved
                .get("poem")
                .and_then(|s| s.field("priority"))
                .expect("priority field resolves via $ref to global");
            assert_eq!(priority.kind(), &SchemaFieldType::Select {
                values: select_entries(&["low", "high"])
            });
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

            let (resolved, _) = resolve_all(&raw).expect("resolves");

            let name = resolved
                .get("author")
                .and_then(|s| s.field("name"))
                .expect("name field resolves via $ref to global");
            assert_eq!(name.kind(), &SchemaFieldType::Select {
                values: select_entries(&["anon"])
            });
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
                    source: RawFieldSource::Ref {
                        address: field_address("#book/status"),
                        override_type: Some(RawSchemaFieldType::File),
                    },
                    options: options(&[("folders", string_list(&["assets"]))]),
                    ..RawSchemaFieldDef::direct(RawSchemaFieldType::Input)
                })]),
            );

            let (resolved, _) = resolve_all(&raw).expect("resolves");

            let status = resolved
                .get("sci_fi")
                .and_then(|s| s.field("status"))
                .expect("status field");
            assert_eq!(status.kind(), &SchemaFieldType::File {
                folders: vec!["assets".to_owned()],
                ext: None,
                class: Vec::new(),
            });
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

            let (resolved, warnings) = resolve_all(&raw).expect("resolves");

            let status = resolved
                .get("sci_fi")
                .and_then(|s| s.field("status"))
                .expect("status field still resolves from the base");
            assert_eq!(status.kind(), &SchemaFieldType::Select {
                values: select_entries(&["draft", "done"])
            });
            assert_eq!(warnings.len(), 1);
            assert!(matches!(
                warnings[0],
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

            let (resolved, warnings) = resolve_all(&raw).expect("resolves");

            let rating = resolved
                .get("sci_fi")
                .and_then(|s| s.field("rating"))
                .expect("rating field still resolves from the base");
            assert_eq!(rating.kind(), &SchemaFieldType::Number {
                min: None,
                max: None,
                step: None,
            });
            assert_eq!(warnings.len(), 1);
            assert!(matches!(
                warnings[0],
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

            let (resolved, warnings) = resolve_all(&raw).expect("resolves");

            let cover = resolved
                .get("book")
                .and_then(|s| s.field("cover"))
                .expect("cover field still resolves from the base");
            assert_eq!(cover.kind(), &SchemaFieldType::File {
                // The valid override key applied...
                folders: vec!["assets/covers".to_owned()],
                // ...while the dropped key's own subfields fall back to the
                // base, untouched.
                ext: Some("png".to_owned()),
                class: vec!["image".to_owned()],
            });
            assert_eq!(warnings.len(), 1);
            assert!(matches!(
                &warnings[0],
                SchemaWarning::UnknownOverrideKey { key, .. } if key == "bogus"
            ));
        }

        #[test]
        fn a_ref_with_a_type_override_and_an_unknown_key_is_a_hard_error() {
            let mut raw = BTreeMap::new();
            raw.insert(
                SchemaName::from("book"),
                schema(&[], &[("status", select_field(&["draft", "done"]))]),
            );
            raw.insert(
                SchemaName::from("sci_fi"),
                schema(&["book"], &[("status", RawSchemaFieldDef {
                    source: RawFieldSource::Ref {
                        address: field_address("#book/status"),
                        override_type: Some(RawSchemaFieldType::Date),
                    },
                    options: options(&[("values", string_list(&["draft"]))]),
                    ..RawSchemaFieldDef::direct(RawSchemaFieldType::Input)
                })]),
            );

            let err = resolve_all(&raw).expect_err(
                "unknown attribute key on a type-overriding $ref rejected",
            );

            assert!(matches!(
                err,
                SchemaError::FieldBuilder(inner)
                    if matches!(
                        *inner,
                        SchemaFieldBuilderError::UnknownAttributeKey { .. }
                    )
            ));
        }

        #[test]
        fn a_direct_field_with_an_unknown_key_is_a_hard_error() {
            let mut raw = BTreeMap::new();
            raw.insert(
                SchemaName::from("book"),
                schema(&[], &[("published", RawSchemaFieldDef {
                    options: options(&[("values", string_list(&["draft"]))]),
                    ..RawSchemaFieldDef::direct(RawSchemaFieldType::Date)
                })]),
            );

            let err =
                resolve_all(&raw).expect_err("unknown attribute key rejected");

            assert!(matches!(
                err,
                SchemaError::FieldBuilder(inner)
                    if matches!(
                        *inner,
                        SchemaFieldBuilderError::UnknownAttributeKey { .. }
                    )
            ));
        }

        #[test]
        fn a_direct_field_with_a_type_mismatched_value_is_a_hard_error() {
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

            let err = resolve_all(&raw)
                .expect_err("attribute value type mismatch rejected");

            assert!(matches!(
                err,
                SchemaError::FieldBuilder(inner)
                    if matches!(
                        *inner,
                        SchemaFieldBuilderError::AttributeValueTypeMismatch { .. }
                    )
            ));
        }
    }
}
