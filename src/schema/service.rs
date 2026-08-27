//! Schema loading, resolution, and query facade.
//!
//! [`SchemaService`] loads `.traces/schemas/*.toml` files, linearizes the
//! `extends` DAG, and resolves it once at construction, exposing read-side
//! queries over the resolved Schemas for its whole lifetime.

use std::{ffi::OsStr, fs, path::Path, sync::Arc};

use indexmap::{IndexMap, IndexSet};

use super::{
    RawSchema, SchemaName,
    builder::{SchemaBuilder, SchemaFailure},
    error::{SchemaError, SchemaWarning},
    fields::SelectValuesFileCache,
    model::Schema,
};
use crate::{BaseNameRef, DirTree, DirTreeError};

/// Schema loading, resolution, and hierarchy/class query facade.
///
/// Resolves every Schema once at construction ([`SchemaService::new`]); every
/// query method reads the already-resolved Schemas directly, with no separate
/// registry type or re-resolution.
#[derive(Debug)]
pub struct SchemaService {
    schemas: IndexMap<SchemaName, Arc<Schema>>,
}

/// The triple [`SchemaService::new`] returns: the constructed service,
/// accumulated resolution warnings, and per-Schema build failures.
type SchemaConstruction =
    (SchemaService, Vec<SchemaWarning>, Vec<SchemaFailure>);

impl SchemaService {
    /// Load every Schema TOML file under `directory` and resolve their
    /// effective fields, building a single read-only [`SchemaService`].
    ///
    /// The `extends` DAG is linearized and each Schema's fields are merged
    /// from its parents. A missing directory resolves to an empty registry.
    /// Per-Schema [`SchemaError`]s exclude that Schema from the result;
    /// its dependents still resolve without its fields
    /// ([`ParentFailedToResolve`]).
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
        directory: &Path,
    ) -> Result<SchemaConstruction, SchemaError> {
        let raw = read_raw_schemas(directory)?;
        let values_cache = SelectValuesFileCache::new(directory);
        let resolved = SchemaBuilder::new(&raw, &values_cache).build()?;
        let schemas = resolved
            .schemas
            .into_iter()
            .map(|(name, schema)| (name, Arc::new(schema)))
            .collect();
        Ok((
            Self {
                schemas,
            },
            resolved.warnings,
            resolved.failures,
        ))
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

    /// Borrowed names of every Schema that directly extends `name`.
    ///
    /// Empty iterator, not an error, if `name` has no Schema or nothing extends
    /// it.
    pub(crate) fn children_names_of<'a>(
        &'a self,
        name: &str,
    ) -> impl Iterator<Item = &'a str> {
        self.schemas
            .get(name)
            .into_iter()
            .flat_map(|schema| schema.children().iter().map(SchemaName::as_str))
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
    pub(crate) fn matches(&self, classes: &[String]) -> IndexSet<String> {
        self.warn_unknown_classes(classes);
        let mut matches: IndexSet<String> = classes.iter().cloned().collect();
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

    /// Warn once per unknown class name in `classes`.
    ///
    /// Shared by [`Self::matches`] and `file_class_expander`'s
    /// [`FileClassExpander`](crate::query::FileClassExpander) impl.
    pub(crate) fn warn_unknown_classes(&self, classes: &[String]) {
        for class in classes {
            if self.get(class).is_none() {
                tracing::warn!(
                    class,
                    "query source names an unknown File Class; matching it \
                     exactly"
                );
            }
        }
    }

    // ------------- `test-utils` Public Surface ------------- //

    // Integration tests (`tests/integration/`) and benchmarks (`benches/`) can
    // only call `pub` methods. These thin wrappers expose the `pub(crate)`
    // internals under the same feature gate that re-exports `SchemaService`
    // from `lib.rs`.

    /// Load and resolve every Schema under `directory`.
    ///
    /// Integration-test and bench entry point; production code should use
    /// [`SchemaService::new`] directly. Warnings and per-Schema build failures
    /// are discarded — tests that need them should use [`SchemaService::new`]
    /// from within the crate.
    ///
    /// # Errors
    ///
    /// Returns `Err(String)` if the schema directory cannot be read, a schema
    /// file cannot be parsed, or the `extends` DAG contains a cycle.
    #[cfg(any(test, feature = "test-utils"))]
    #[inline]
    pub fn resolve(directory: &Path) -> Result<Self, String> {
        let (service, _warnings, _failures) =
            Self::new(directory).map_err(|e| e.to_string())?;
        Ok(service)
    }

    /// Look up a Schema by name.
    #[cfg(any(test, feature = "test-utils"))]
    #[inline]
    #[must_use]
    pub fn schema_get(&self, name: &str) -> Option<&Arc<Schema>> {
        self.get(name)
    }

    /// Every Schema that directly extends `name`.
    #[cfg(any(test, feature = "test-utils"))]
    #[inline]
    #[must_use]
    pub fn schema_children_of(&self, name: &str) -> Vec<Arc<Schema>> {
        self.children_of(name)
    }

    /// Every Schema that transitively extends `name`.
    #[cfg(any(test, feature = "test-utils"))]
    #[inline]
    #[must_use]
    pub fn schema_descendants_of(&self, name: &str) -> Vec<Arc<Schema>> {
        self.descendants_of(name)
    }

    /// Set of Schema names matching `classes`, including transitive
    /// descendants.
    #[cfg(any(test, feature = "test-utils"))]
    #[inline]
    #[must_use]
    pub fn schema_matches(&self, classes: &[String]) -> IndexSet<String> {
        self.matches(classes)
    }
}

/// Read and parse every `*.toml` file directly under `dir` into a [`RawSchema`]
/// keyed by filename stem.
///
/// Walks only `dir`'s immediate entries (`min_depth(1).max_depth(1)`):
/// Schemas do not nest. A `dir` that does not exist yields an empty map
/// rather than [`SchemaError::ReadDirectory`].
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
) -> Result<IndexMap<SchemaName, RawSchema>, SchemaError> {
    let mut schemas = IndexMap::new();
    for node in DirTree::children(dir) {
        let node = match node {
            Ok(node) => node,
            Err(DirTreeError::MissingRoot {
                ..
            }) => return Ok(IndexMap::new()),
            Err(error) => {
                let (directory, source) = error.into_parts();
                return Err(SchemaError::ReadDirectory {
                    directory,
                    source,
                });
            }
        };
        let path = node.path();
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

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    type ResolveResult = Result<SchemaConstruction, SchemaError>;

    /// Resolves every Schema TOML file directly under `dir`, mirroring the
    /// pre-refactor `SchemaRegistry::load(dir)` call shape: `dir` is used
    /// directly as the Schema directory, `root` is unused by resolution itself.
    fn resolve_dir(dir: &Path) -> ResolveResult {
        SchemaService::new(dir)
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

            /// Restores a locked directory's permissions on drop, even if the
            /// test panics. Otherwise, a `0o000` directory blocks the tempdir's
            /// own cleanup.
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
            let (service, _, _) =
                SchemaService::new(&schemas_dir).expect("registry loads");

            fs::remove_dir_all(&schemas_dir).expect("remove schemas dir");

            assert!(
                service.get("book").is_some(),
                "a Schema resolved at construction must not need to reread a \
                 now-missing directory"
            );
        }
    }

    mod matches {
        use indexmap::IndexSet;
        use pretty_assertions::assert_eq;

        use super::*;

        fn set(names: &[&str]) -> IndexSet<String> {
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

    mod children_of {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn returns_only_direct_extenders() {
            let temp = tempfile::tempdir().expect("create temp dir");
            write_schema(temp.path(), "thing", "");
            write_schema(temp.path(), "book", r#"extends = ["thing"]"#);
            write_schema(temp.path(), "sci_fi", r#"extends = ["book"]"#);

            let (service, _, _) =
                resolve_dir(temp.path()).expect("registry loads");

            let children = service.children_of("thing");
            let names: Vec<&str> =
                children.iter().map(|schema| schema.name()).collect();

            assert_eq!(names, vec!["book"]);
        }
    }

    mod children_names_of {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn returns_only_direct_extenders_as_borrowed_names() {
            let temp = tempfile::tempdir().expect("create temp dir");
            write_schema(temp.path(), "thing", "");
            write_schema(temp.path(), "book", r#"extends = ["thing"]"#);
            write_schema(temp.path(), "sci_fi", r#"extends = ["book"]"#);

            let (service, _, _) =
                resolve_dir(temp.path()).expect("registry loads");

            let names: Vec<&str> = service.children_names_of("thing").collect();

            assert_eq!(names, vec!["book"]);
        }

        #[test]
        fn returns_no_names_for_a_leaf_schema() {
            let temp = tempfile::tempdir().expect("create temp dir");
            write_schema(temp.path(), "book", "");

            let (service, _, _) =
                resolve_dir(temp.path()).expect("registry loads");

            assert_eq!(service.children_names_of("book").next(), None);
        }

        #[test]
        fn returns_no_names_for_an_unknown_schema() {
            let temp = tempfile::tempdir().expect("create temp dir");
            write_schema(temp.path(), "book", "");

            let (service, _, _) =
                resolve_dir(temp.path()).expect("registry loads");

            assert_eq!(service.children_names_of("missing").next(), None);
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

    mod warn_unknown_classes {
        use super::*;

        #[test]
        fn does_not_panic_for_unknown_classes() {
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

            service
                .warn_unknown_classes(&["book".to_owned(), "ghost".to_owned()]);
        }

        struct EventCapture {
            events: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
        }

        impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for EventCapture {
            fn on_event(
                &self,
                event: &tracing::Event<'_>,
                _ctx: tracing_subscriber::layer::Context<'_, S>,
            ) {
                if *event.metadata().level() != tracing::Level::WARN {
                    return;
                }
                let mut visitor = EventVisitor(String::new());
                event.record(&mut visitor);
                self.events.lock().unwrap().push(visitor.0);
            }
        }

        struct EventVisitor(String);

        impl tracing::field::Visit for EventVisitor {
            fn record_debug(
                &mut self,
                field: &tracing::field::Field,
                value: &dyn std::fmt::Debug,
            ) {
                use std::fmt::Write;
                let _ = write!(self.0, "{}={:?} ", field.name(), value);
            }
        }

        #[test]
        fn emits_a_warning_for_each_unknown_class() {
            use std::sync::{Arc, Mutex};

            use tracing_subscriber::prelude::*;
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

            let events = Arc::new(Mutex::new(Vec::new()));
            let capture = EventCapture {
                events: events.clone(),
            };
            let subscriber = tracing_subscriber::registry().with(capture);
            let guard = tracing::subscriber::set_default(subscriber);

            service.warn_unknown_classes(&[
                "book".to_owned(),
                "ghost".to_owned(),
                "phantom".to_owned(),
            ]);

            drop(guard);
            let events = events.lock().unwrap();
            assert!(
                events.iter().any(|e| e.contains("ghost")),
                "expected warning for unknown class 'ghost', got: {events:?}"
            );
            assert!(
                events.iter().any(|e| e.contains("phantom")),
                "expected warning for unknown class 'phantom', got: {events:?}"
            );
            assert!(
                !events.iter().any(|e| e.contains("book")),
                "must not warn for known class 'book', got: {events:?}"
            );
        }
    }
}
