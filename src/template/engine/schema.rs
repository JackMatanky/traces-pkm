//! Registers the `schema` namespace for templates.
//!
//! [`SchemaOps`] is the `schema` namespace object registered as a minijinja
//! global by [`super::TemplateEngine`]. It exposes one method:
//!
//! - `schema.get(name)`: binds the resolved [`Schema`] named `name` as a
//!   [`SchemaBinding`], hard-erroring if no Schema by that name resolves.
//!
//! The bound [`SchemaBinding`] is itself a minijinja object exposing one plain
//! attribute and two methods:
//!
//! - `.name`: the Schema's own name (its source file's stem).
//! - `.field(name)`: the named field's selectable values, as plain strings for
//!   a `select` field, or `none` for every other type. `file` fields currently
//!   always resolve to `none` here; their selectable options come from the
//!   `FileIndex`, which this namespace does not yet consult. An unknown field
//!   name hard-errors, mirroring `.get`.
//! - `.descendants()`: every Schema that is-a this one transitively (extends it
//!   directly or via an ancestor), each itself a [`SchemaBinding`] so a
//!   Template can walk the whole subtree (`.name`, `.field(...)`, and
//!   `.descendants()` again). Empty, not an error, when nothing extends this
//!   Schema.
//!
//! `schema.get` and `.field` are structural references: a typo in either name
//! surfaces as a render error carrying template context, not a panic, mirroring
//! [`super::query`]'s `errors` module. Class-based predicate references
//! (`from_class`, `file`-field filters) are not supported by this namespace;
//! the Schema supplies values only, and the template author still picks the
//! interactive `ui.*` function.
//!
//! # Registry Loading and Caching
//!
//! No Schema TOML is read or resolved until a template actually calls
//! `schema.get(...)`: a Template that never touches the `schema` namespace
//! never reads the registry directory, so a broken Schema file elsewhere in it
//! only breaks the Template that reaches into `schema`. Once loaded, the
//! resolved [`SchemaRegistry`] is cached in [`State`]'s temp storage for the
//! remainder of the render, mirroring [`super::query`]'s `cached_refresh`, so a
//! Template calling `schema.get` several times pays for one registry load.
//! [`SchemaRegistry`] itself stores each Schema behind an `Arc`, so binding one
//! via [`SchemaBinding`] shares that Schema's field map instead of deep-cloning
//! it per call. `.descendants()` itself is *not* memoized across calls within
//! a render: each call re-scans the registry (`O(n)` in Schema count),
//! including nested `.descendants().descendants()` chains. Fine at the small
//! Schema counts this module assumes; revisit if that assumption changes.

use std::{path::Path, sync::Arc};

use minijinja::{
    Environment, Error, ErrorKind, State,
    value::{Enumerator, Object, Value},
};

use crate::schema::{Schema, SchemaError, SchemaRegistry};

/// Method names `schema` exposes, for [`SchemaOps::enumerate`].
const METHODS: &[&str] = &["get"];

/// Keys a bound [`SchemaBinding`] exposes: `field`/`descendants` are called as
/// methods, `name` is a plain attribute. Backs [`Object::enumerate`].
const SCHEMA_METHODS: &[&str] = &["field", "name", "descendants"];

/// The [`State::set_temp`] key used to cache one loaded [`SchemaRegistry`] for
/// the current render. See the module docs' "Registry Loading and Caching"
/// section.
const REGISTRY_CACHE_KEY: &str = "schema.registry_cache";

/// Backs the `schema` namespace object.
#[derive(Debug)]
pub(super) struct SchemaOps {
    /// The Schema registry directory, resolved against the render's project
    /// root.
    directory: Arc<Path>,
}

impl SchemaOps {
    /// Wraps `directory`, the resolved Schema registry directory.
    #[inline]
    #[must_use]
    pub(super) const fn new(directory: Arc<Path>) -> Self {
        Self {
            directory,
        }
    }

    /// Registers this object as the `schema` global.
    #[inline]
    pub(super) fn register(self, env: &mut Environment<'static>) {
        env.add_global("schema", Value::from_object(self));
    }

    /// Returns the render's cached [`SchemaRegistry`], loading and caching it
    /// in `state`'s temp storage first if not already cached this render.
    /// Logs each accumulated [`SchemaWarning`](crate::schema::SchemaWarning)
    /// once, at load time.
    ///
    /// # Errors
    ///
    /// - [`ErrorKind::InvalidOperation`] via [`registry_error`] if
    ///   [`SchemaRegistry::load`] fails: a malformed Schema file, a resolution
    ///   cycle, or an out-of-bounds `$ref`.
    fn cached_registry(
        &self,
        state: &State,
    ) -> Result<Arc<SchemaRegistry>, Error> {
        if let Some(registry) =
            state.get_temp(REGISTRY_CACHE_KEY).and_then(|value| {
                value
                    .downcast_object_ref::<CachedRegistry>()
                    .map(|cached| Arc::clone(&cached.0))
            })
        {
            return Ok(registry);
        }
        let (registry, warnings) =
            SchemaRegistry::load(&self.directory).map_err(registry_error)?;
        for warning in &warnings {
            tracing::warn!(%warning, "Schema registry resolved with a warning");
        }
        let registry = Arc::new(registry);
        state.set_temp(
            REGISTRY_CACHE_KEY,
            Value::from_object(CachedRegistry(Arc::clone(&registry))),
        );
        Ok(registry)
    }
}

impl Object for SchemaOps {
    fn get_value(self: &Arc<Self>, key: &Value) -> Option<Value> {
        match key.as_str()? {
            "get" => {
                let ops = Arc::clone(self);
                Some(Value::from_function(
                    move |state: &State, name: &str| -> Result<Value, Error> {
                        let registry = ops.cached_registry(state)?;
                        registry
                            .get(name)
                            .cloned()
                            .map(|schema| {
                                Value::from_dyn_object(Arc::new(
                                    SchemaBinding {
                                        schema,
                                        registry: Arc::clone(&registry),
                                    },
                                ))
                            })
                            .ok_or_else(|| unknown_schema_error(name))
                    },
                ))
            }
            _ => None,
        }
    }

    fn enumerate(self: &Arc<Self>) -> Enumerator {
        Enumerator::Str(METHODS)
    }
}

/// Wraps [`SchemaRegistry`] only so it can round-trip through [`State`]'s temp
/// storage via [`Value::from_object`]/[`Value::downcast_object_ref`]. Never
/// exposed to templates: no global registers it, unlike [`SchemaBinding`].
#[derive(Debug)]
struct CachedRegistry(Arc<SchemaRegistry>);

impl Object for CachedRegistry {}

/// Pairs a bound [`Schema`] with the [`SchemaRegistry`] it resolved from, so
/// `.descendants()` can look up other Schemas by is-a relationship. [`Schema`]
/// itself stays registry-unaware (see the module docs): this wrapper, not
/// [`crate::schema`], is where minijinja-facing tree-walking lives.
///
/// Gets its [`Object`] impl here instead of in [`crate::schema`], mirroring how
/// [`super::query`] wires up [`crate::index::QueryOutcome`].
#[derive(Debug)]
struct SchemaBinding {
    schema: Arc<Schema>,
    registry: Arc<SchemaRegistry>,
}

impl Object for SchemaBinding {
    fn get_value(self: &Arc<Self>, key: &Value) -> Option<Value> {
        match key.as_str()? {
            "name" => Some(Value::from(self.schema.name())),
            "field" => {
                let schema = Arc::clone(&self.schema);
                Some(Value::from_function(
                    move |name: &str| -> Result<Value, Error> {
                        let field = schema.field(name).ok_or_else(|| {
                            unknown_field_error(schema.name(), name)
                        })?;
                        Ok(field.selectable_values().map_or_else(
                            || Value::from(()),
                            |values| Value::from(values.to_vec()),
                        ))
                    },
                ))
            }
            "descendants" => {
                let binding = Arc::clone(self);
                Some(Value::from_function(move || -> Value {
                    let descendants =
                        binding.registry.descendants_of(binding.schema.name());
                    Value::from(
                        descendants
                            .into_iter()
                            .map(|schema| {
                                Value::from_dyn_object(Arc::new(
                                    SchemaBinding {
                                        schema,
                                        registry: Arc::clone(&binding.registry),
                                    },
                                ))
                            })
                            .collect::<Vec<_>>(),
                    )
                }))
            }
            _ => None,
        }
    }

    fn enumerate(self: &Arc<Self>) -> Enumerator {
        Enumerator::Str(SCHEMA_METHODS)
    }
}

/// Maps a directory-wide [`SchemaError`] into a [`minijinja::Error`].
///
/// Keeps the original error as [`source`](std::error::Error::source).
fn registry_error(source: SchemaError) -> Error {
    super::error::invalid_operation(
        "failed to load the Schema registry",
        source,
    )
}

/// Builds the error for `schema.get(name)` naming a Schema that did not
/// resolve.
fn unknown_schema_error(name: &str) -> Error {
    Error::new(ErrorKind::InvalidOperation, format!("unknown Schema {name:?}"))
}

/// Builds the error for `.field(name)` naming a field absent from `schema`'s
/// resolved fields.
fn unknown_field_error(schema: &str, field: &str) -> Error {
    Error::new(
        ErrorKind::InvalidOperation,
        format!("Schema {schema:?} has no field {field:?}"),
    )
}

#[cfg(test)]
mod tests {
    use minijinja::Environment;

    use super::*;

    /// A minimal [`Environment`] with `schema` registered against `directory`.
    fn env(directory: &Path) -> Environment<'static> {
        let mut env = Environment::new();
        SchemaOps::new(Arc::from(directory)).register(&mut env);
        env
    }

    fn render(directory: &Path, source: &str) -> Result<String, Error> {
        env(directory).render_str(source, minijinja::context!())
    }

    mod fixtures {
        use std::{fs, path::Path};

        /// Writes `content` as a Schema TOML file named `name.toml` under
        /// `root/.traces/schemas/`, creating the directory if needed.
        pub(super) fn write_schema(root: &Path, name: &str, content: &str) {
            let dir = root.join(".traces/schemas");
            fs::create_dir_all(&dir).expect("create schemas dir");
            fs::write(dir.join(format!("{name}.toml")), content)
                .expect("write schema");
        }
    }
    use fixtures::write_schema;

    mod get_value {
        use super::*;

        #[test]
        fn returns_none_for_an_unknown_key() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let ops = Arc::new(SchemaOps::new(Arc::from(temp.path())));

            assert!(ops.get_value(&Value::from("unknown")).is_none());
        }

        #[test]
        fn returns_none_for_a_non_string_key() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let ops = Arc::new(SchemaOps::new(Arc::from(temp.path())));

            assert!(ops.get_value(&Value::from(1)).is_none());
        }
    }

    mod enumerate {
        use super::*;

        #[test]
        fn lists_every_method() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let ops = Arc::new(SchemaOps::new(Arc::from(temp.path())));

            assert!(matches!(ops.enumerate(), Enumerator::Str(METHODS)));
        }

        #[test]
        fn every_enumerated_method_resolves_via_get_value() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let ops = Arc::new(SchemaOps::new(Arc::from(temp.path())));

            for method in METHODS {
                assert!(
                    ops.get_value(&Value::from(*method)).is_some(),
                    "{method:?} is enumerated but get_value has no matching \
                     arm"
                );
            }
        }
    }

    mod get {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn binds_a_resolved_schema_by_name() {
            let temp = tempfile::tempdir().expect("create temp dir");
            write_schema(
                temp.path(),
                "book",
                r#"
                [fields.status]
                type = "select"
                values = ["reading", "read"]
                "#,
            );

            let rendered = render(
                &temp.path().join(".traces/schemas"),
                "{{ schema.get('book').field('status') | join(',') }}",
            )
            .expect("render succeeds");

            assert_eq!(rendered, "reading,read");
        }

        #[test]
        fn resolves_inheritance_through_extends() {
            let temp = tempfile::tempdir().expect("create temp dir");
            write_schema(
                temp.path(),
                "book",
                r#"
                [fields.status]
                type = "select"
                values = ["reading", "read"]
                "#,
            );
            write_schema(
                temp.path(),
                "sci_fi",
                r#"
                extends = ["book"]
                "#,
            );

            let rendered = render(
                &temp.path().join(".traces/schemas"),
                "{{ schema.get('sci_fi').field('status') | join(',') }}",
            )
            .expect("render succeeds");

            assert_eq!(rendered, "reading,read");
        }

        #[test]
        fn two_calls_in_the_same_render_both_resolve_the_same_schema() {
            let temp = tempfile::tempdir().expect("create temp dir");
            write_schema(
                temp.path(),
                "book",
                r#"
                [fields.status]
                type = "select"
                values = ["reading"]
                "#,
            );

            let rendered = render(
                &temp.path().join(".traces/schemas"),
                "{{ schema.get('book').field('status') | join(',') }}-{{ \
                 schema.get('book').field('status') | join(',') }}",
            )
            .expect("render succeeds");

            assert_eq!(rendered, "reading-reading");
        }
    }

    mod caching {
        use std::fs;

        use super::*;

        #[test]
        fn a_second_call_reuses_the_registry_cached_by_the_first() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let schemas_dir = temp.path().join(".traces/schemas");
            write_schema(
                temp.path(),
                "book",
                r#"
                [fields.status]
                type = "select"
                values = ["reading"]
                "#,
            );
            let ops =
                Arc::new(SchemaOps::new(Arc::from(schemas_dir.as_path())));
            let get = ops
                .get_value(&Value::from("get"))
                .expect("get is a known method");
            let env = Environment::new();
            let state = env.empty_state();

            // Populates state's cached SchemaRegistry.
            get.call(&state, &[Value::from("book")])
                .expect("first call loads and caches the registry");
            // A missing registry directory degrades to an empty registry
            // (SchemaRegistry::load), not an error: if the second call
            // below re-read the directory instead of reusing the cache, it
            // would find no `book` Schema and hard-error.
            fs::remove_dir_all(&schemas_dir).expect("remove schemas dir");

            let second = get.call(&state, &[Value::from("book")]);

            assert!(
                second.is_ok(),
                "a cached registry must not need to reread a now-missing \
                 directory"
            );
        }
    }

    mod warnings {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn a_missing_extends_target_degrades_with_a_warning_instead_of_failing_the_render()
         {
            // `sci_fi` extends an unresolvable Schema name: ticket 02's
            // resolve() degrades this to a `SchemaWarning`, not a
            // `SchemaError` — `sci_fi`'s own fields must still render.
            let temp = tempfile::tempdir().expect("create temp dir");
            write_schema(
                temp.path(),
                "sci_fi",
                r#"
                extends = ["does_not_exist"]

                [fields.status]
                type = "select"
                values = ["reading"]
                "#,
            );

            let rendered = render(
                &temp.path().join(".traces/schemas"),
                "{{ schema.get('sci_fi').field('status') | join(',') }}",
            )
            .expect(
                "a degraded extends target should warn, not fail the render",
            );

            assert_eq!(rendered, "reading");
        }
    }

    mod field {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn returns_none_for_a_non_list_field_type() {
            let temp = tempfile::tempdir().expect("create temp dir");
            write_schema(
                temp.path(),
                "book",
                r#"
                [fields.title]
                type = "input"
                "#,
            );

            let rendered = render(
                &temp.path().join(".traces/schemas"),
                "{{ schema.get('book').field('title') is none }}",
            )
            .expect("render succeeds");

            assert_eq!(rendered, "true");
        }

        #[test]
        fn returns_none_for_a_file_field_type_pending_the_fileindex_ticket() {
            let temp = tempfile::tempdir().expect("create temp dir");
            write_schema(
                temp.path(),
                "book",
                r#"
                [fields.cover]
                type = "file"
                "#,
            );

            let rendered = render(
                &temp.path().join(".traces/schemas"),
                "{{ schema.get('book').field('cover') is none }}",
            )
            .expect("render succeeds");

            assert_eq!(rendered, "true");
        }
    }

    mod name {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn returns_the_schemas_own_name() {
            let temp = tempfile::tempdir().expect("create temp dir");
            write_schema(temp.path(), "book", "");

            let rendered = render(
                &temp.path().join(".traces/schemas"),
                "{{ schema.get('book').name }}",
            )
            .expect("render succeeds");

            assert_eq!(rendered, "book");
        }
    }

    mod descendants {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn returns_a_direct_extender_as_a_bound_schema() {
            let temp = tempfile::tempdir().expect("create temp dir");
            write_schema(temp.path(), "book", "");
            write_schema(temp.path(), "sci_fi", r#"extends = ["book"]"#);

            let rendered = render(
                &temp.path().join(".traces/schemas"),
                "{{ schema.get('book').descendants() | map(attribute='name') \
                 | join(',') }}",
            )
            .expect("render succeeds");

            assert_eq!(rendered, "sci_fi");
        }

        #[test]
        fn returns_a_transitive_descendant_through_an_intermediate_schema() {
            let temp = tempfile::tempdir().expect("create temp dir");
            write_schema(temp.path(), "thing", "");
            write_schema(temp.path(), "book", r#"extends = ["thing"]"#);
            write_schema(temp.path(), "sci_fi", r#"extends = ["book"]"#);

            let rendered = render(
                &temp.path().join(".traces/schemas"),
                "{{ schema.get('thing').descendants() | map(attribute='name') \
                 | join(',') }}",
            )
            .expect("render succeeds");

            assert_eq!(rendered, "book,sci_fi");
        }

        #[test]
        fn returns_an_empty_list_for_a_leaf_schema_not_none_or_an_error() {
            let temp = tempfile::tempdir().expect("create temp dir");
            write_schema(temp.path(), "book", "");
            write_schema(temp.path(), "sci_fi", r#"extends = ["book"]"#);

            let rendered = render(
                &temp.path().join(".traces/schemas"),
                "{{ schema.get('sci_fi').descendants() | length }}",
            )
            .expect("render succeeds");

            assert_eq!(rendered, "0");
        }

        #[test]
        fn a_descendant_still_resolves_its_own_fields() {
            let temp = tempfile::tempdir().expect("create temp dir");
            write_schema(
                temp.path(),
                "book",
                r#"
                [fields.status]
                type = "select"
                values = ["reading"]
                "#,
            );
            write_schema(temp.path(), "sci_fi", r#"extends = ["book"]"#);

            let rendered = render(
                &temp.path().join(".traces/schemas"),
                "{{ schema.get('book').descendants()[0].field('status') | \
                 join(',') }}",
            )
            .expect("render succeeds");

            assert_eq!(rendered, "reading");
        }

        #[test]
        fn a_descendants_own_descendants_are_also_reachable() {
            // Proves the "descendants" branch threads the same registry
            // into each returned SchemaBinding (schema.rs's `.descendants`
            // arm clones `binding.registry`, not a fresh/empty one) — the
            // registry-level `descendants_of` test already proves the
            // underlying data is transitively correct; this proves the
            // render-facing chain (`.descendants().descendants()`) the
            // module docs promise actually works.
            let temp = tempfile::tempdir().expect("create temp dir");
            write_schema(temp.path(), "thing", "");
            write_schema(temp.path(), "book", r#"extends = ["thing"]"#);
            write_schema(temp.path(), "sci_fi", r#"extends = ["book"]"#);

            let rendered = render(
                &temp.path().join(".traces/schemas"),
                "{{ schema.get('thing').descendants()[0].descendants() | \
                 map(attribute='name') | join(',') }}",
            )
            .expect("render succeeds");

            assert_eq!(rendered, "sci_fi");
        }
    }

    mod errors {
        use super::*;

        #[test]
        fn schema_get_of_an_unknown_schema_surfaces_as_a_render_error_not_a_panic()
         {
            let temp = tempfile::tempdir().expect("create temp dir");

            let error = render(
                &temp.path().join(".traces/schemas"),
                "{{ schema.get('missing') }}",
            )
            .expect_err("unknown Schema should error");

            assert!(error.to_string().contains("unknown Schema"));
        }

        #[test]
        fn unknown_schema_error_carries_the_template_name_and_line() {
            // AC5 promises template *context* (name/line/column), not just
            // a message: the shared `render()` helper below never asserts
            // on either, so no existing test proves minijinja actually
            // attaches them to a `schema.*` error. `.name()`/`.line()` are
            // populated unconditionally (verified: still `Some` with
            // `set_debug(false)`, in both debug and release profiles) —
            // only the byte-accurate column info `crate::cli::error` needs
            // requires `set_debug(true)`. Calling it here anyway mirrors
            // `TemplateEngine::new`'s production wiring exactly, so this
            // test can't drift from it unnoticed.
            let temp = tempfile::tempdir().expect("create temp dir");
            let mut env = Environment::new();
            env.set_debug(true);
            SchemaOps::new(Arc::from(temp.path().join(".traces/schemas")))
                .register(&mut env);

            let error = env
                .render_named_str(
                    "note.md",
                    "line one\n{{ schema.get('missing') }}\n",
                    minijinja::context!(),
                )
                .expect_err("unknown Schema should error");

            assert_eq!(error.name(), Some("note.md"));
            assert_eq!(error.line(), Some(2));
        }

        #[test]
        fn field_of_an_unknown_field_surfaces_as_a_render_error_not_a_panic() {
            let temp = tempfile::tempdir().expect("create temp dir");
            write_schema(
                temp.path(),
                "book",
                r#"
                [fields.title]
                type = "input"
                "#,
            );

            let error = render(
                &temp.path().join(".traces/schemas"),
                "{{ schema.get('book').field('missing') }}",
            )
            .expect_err("unknown field should error");

            assert!(error.to_string().contains("has no field"));
        }

        #[test]
        fn a_malformed_schema_file_surfaces_as_a_render_error_not_a_panic() {
            let temp = tempfile::tempdir().expect("create temp dir");
            write_schema(temp.path(), "book", "not valid toml [[[");

            let error = render(
                &temp.path().join(".traces/schemas"),
                "{{ schema.get('book') }}",
            )
            .expect_err("malformed Schema TOML should error");

            assert!(
                error
                    .to_string()
                    .contains("failed to load the Schema registry")
            );
        }

        #[test]
        fn a_broken_sibling_schema_still_breaks_a_template_that_touches_schema()
        {
            // Directory-wide load failure is documented, not hidden: a
            // template that reaches into `schema` at all pays for every
            // Schema file in the registry resolving, per ticket 02's
            // `resolve()` contract. Lazy loading only isolates templates
            // that never call `schema.get` in the first place (see the
            // `register` module below).
            let temp = tempfile::tempdir().expect("create temp dir");
            write_schema(
                temp.path(),
                "book",
                r#"
                [fields.status]
                type = "select"
                values = ["reading"]
                "#,
            );
            write_schema(temp.path(), "broken", "not valid toml [[[");

            let error = render(
                &temp.path().join(".traces/schemas"),
                "{{ schema.get('book').field('status') }}",
            )
            .expect_err(
                "a broken sibling Schema fails the whole registry load",
            );

            assert!(
                error
                    .to_string()
                    .contains("failed to load the Schema registry")
            );
        }
    }

    mod register {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn register_makes_schema_reachable_through_a_real_environment() {
            let temp = tempfile::tempdir().expect("create temp dir");
            write_schema(
                temp.path(),
                "book",
                r#"
                [fields.status]
                type = "select"
                values = ["reading"]
                "#,
            );

            let rendered = render(
                &temp.path().join(".traces/schemas"),
                "{{ schema.get('book').field('status') | join(',') }}",
            )
            .expect("render succeeds");

            assert_eq!(rendered, "reading");
        }

        #[test]
        fn a_broken_schema_never_breaks_a_template_that_never_touches_schema() {
            let temp = tempfile::tempdir().expect("create temp dir");
            write_schema(temp.path(), "broken", "not valid toml [[[");

            let rendered =
                render(&temp.path().join(".traces/schemas"), "hello")
                    .expect("a template never calling schema.* never loads it");

            assert_eq!(rendered, "hello");
        }
    }
}
