//! Register the `schema` namespace for templates.
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
//!   a `select` field, label/value objects for a `file` field, or `none` for
//!   every other type. `file` fields resolve live from the render-scoped
//!   `FileIndex`: labels use the configured `[frontmatter]` aliases key,
//!   falling back to the configured title key, then the filename stem; values
//!   are paths.
//! - `.descendants()`: every Schema that is-a this one transitively (extends it
//!   directly or via an ancestor), each itself a [`SchemaBinding`] so a
//!   Template can walk the whole subtree (`.name`, `.field(...)`, and
//!   `.descendants()` again). Empty, not an error, when nothing extends this
//!   Schema.
//!
//! `schema.get` and `.field` are structural references: a typo in either name
//! surfaces as a render error carrying template context, not a panic, mirroring
//! [`super::query`]'s `errors` module. Class-based predicate references
//! (`from_class`, `file`-field filters) degrade missing class targets to exact
//! matching with a warning; structural Schema and field names still hard-error.
//!
//! # Registry Loading and Caching
//!
//! No Schema TOML is read or resolved until a template actually calls
//! `schema.get(...)`: a Template that never touches the `schema` namespace
//! never reads the registry directory, so a broken Schema file elsewhere in it
//! only breaks the Template that reaches into `schema`. Once loaded, the
//! resolved [`SchemaRegistry`] is cached in [`State`]'s temp storage for the
//! remainder of the render, mirroring [`super::query::cached_refresh`], so a
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

use crate::{
    field,
    field::FieldKey,
    query::{FileOption, FileOptionFilter, FrontmatterFieldKeys},
    schema::{Schema, SchemaError, SchemaFileFieldFilter, SchemaRegistry},
};

/// Method names `schema` exposes, for [`SchemaOps::enumerate`].
const METHODS: &[&str] = &["get"];

/// Keys a bound [`SchemaBinding`] exposes: `field`/`descendants` are called as
/// methods, `name` is a plain attribute. Backs [`Object::enumerate`].
const SCHEMA_METHODS: &[&str] = &["field", "name", "descendants"];

/// Shared runtime state for the `schema` namespace: the project root and Schema
/// registry directory used to load/refresh render-scoped data, plus the
/// frontmatter keys used by file-field label/class resolution. Held once in
/// [`SchemaOps`] and cloned as a single `Arc` into every [`SchemaBinding`]
/// instead of threading separate fields through both types.
///
/// `Arc`, not `Rc`: minijinja `Object` values are reference-counted through
/// `Arc<Self>`, and the existing namespace objects already use `Arc` to support
/// cached object values without tying the engine to a single-thread-only type.
#[derive(Debug)]
pub(super) struct SchemaContext {
    /// Project root used to refresh the render-scoped `FileIndex`.
    root: Arc<Path>,
    /// The Schema registry directory, resolved against the project root.
    directory: Arc<Path>,
    /// Frontmatter keys used by file-field class filtering and label
    /// resolution.
    keys: FrontmatterFieldKeys,
}

impl SchemaContext {
    /// Wraps the project `root`, resolved Schema registry `directory`, and
    /// configured frontmatter keys used by file-field option resolution.
    #[inline]
    #[must_use]
    pub(super) const fn new(
        root: Arc<Path>,
        directory: Arc<Path>,
        keys: FrontmatterFieldKeys,
    ) -> Self {
        Self {
            root,
            directory,
            keys,
        }
    }

    /// Returns the resolved Schema registry directory.
    #[inline]
    #[must_use]
    pub(super) fn directory(&self) -> &Path {
        &self.directory
    }
}

/// Backs the `schema` namespace object.
#[derive(Debug)]
pub(super) struct SchemaOps {
    ctx: Arc<SchemaContext>,
}

impl SchemaOps {
    /// Wraps the shared [`SchemaContext`] used to load the Schema registry and
    /// resolve file-field options.
    #[inline]
    #[must_use]
    pub(super) fn new(ctx: Arc<SchemaContext>) -> Self {
        Self {
            ctx,
        }
    }

    /// Registers this object as the `schema` global.
    #[inline]
    pub(super) fn register(self, env: &mut Environment<'static>) {
        env.add_global("schema", Value::from_object(self));
    }

    /// Returns the render's [`SchemaRegistry`] cached via [`super::cache`],
    /// shared with the `query`/`tasks` namespaces so a render touching both
    /// pays for one [`SchemaRegistry::load`]. Logs each recovered
    /// `SchemaWarning` once, at load time.
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
        let directory = self.ctx.directory();
        super::cache::cached(
            state,
            super::cache::SCHEMA_REGISTRY_CACHE_KEY,
            || {
                let (registry, warnings) =
                    SchemaRegistry::load(directory).map_err(registry_error)?;
                for warning in &warnings {
                    tracing::warn!(
                        %warning,
                        "Schema registry resolved with a warning"
                    );
                }
                Ok(Arc::new(registry))
            },
        )
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
                                        ctx: Arc::clone(&ops.ctx),
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

/// Pairs a bound [`Schema`] with the [`SchemaRegistry`] it resolved from, so
/// `.descendants()` can look up other Schemas by is-a relationship. [`Schema`]
/// itself stays registry-unaware (see the module docs): this wrapper, not
/// [`crate::schema`], is where minijinja-facing tree-walking lives.
///
/// Gets its [`Object`] impl here instead of in [`crate::schema`], mirroring how
/// [`super::query`] wires up [`crate::query::QueryOutcome`].
#[derive(Debug)]
struct SchemaBinding {
    schema: Arc<Schema>,
    registry: Arc<SchemaRegistry>,
    ctx: Arc<SchemaContext>,
}

impl SchemaBinding {
    /// Resolves a file-typed field against the render-scoped `FileIndex`.
    fn file_field_values(
        &self,
        state: &State,
        folders: &[String],
        ext: Option<&str>,
        classes: &[String],
    ) -> Result<Value, Error> {
        let index = super::query::cached_refresh(state, &self.ctx.root)
            .map_err(super::query::index_error)?;
        let class_matches = if classes.is_empty() {
            None
        } else {
            for class in classes {
                if self.registry.get(class).is_none() {
                    tracing::warn!(
                        class = %class,
                        "file field references a class with no Schema; \
                         degrading to exact match"
                    );
                }
            }
            Some(self.registry.matches(classes))
        };
        let options = index.file_options(FileOptionFilter::new(
            folders,
            ext,
            class_matches.as_ref(),
            &self.ctx.keys,
        ));
        Ok(Value::from(
            options.into_iter().map(file_option_value).collect::<Vec<_>>(),
        ))
    }
}

impl Object for SchemaBinding {
    fn get_value(self: &Arc<Self>, key: &Value) -> Option<Value> {
        match key.as_str()? {
            "name" => Some(Value::from(self.schema.name())),
            "field" => {
                let binding = Arc::clone(self);
                Some(Value::from_function(
                    move |state: &State, name: &str| -> Result<Value, Error> {
                        let field =
                            binding.schema.field(name).ok_or_else(|| {
                                unknown_field_error(&binding.schema, name)
                            })?;
                        if let Some(SchemaFileFieldFilter {
                            folders,
                            ext,
                            class: classes,
                        }) = field.file_filter()
                        {
                            return binding.file_field_values(
                                state, folders, ext, classes,
                            );
                        }
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
                                        ctx: Arc::clone(&binding.ctx),
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

/// Converts an index-derived file option into the label/value object shape
/// `ui.select` expects by default.
fn file_option_value(option: FileOption) -> Value {
    let (label, value) = option.into_parts();
    minijinja::context! {
        label => label,
        value => value,
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
///
/// Appends a `; did you mean "..."?` hint when [`closest_field_suggestion`]
/// finds exactly one plausible candidate; omits the hint otherwise. Never
/// changes whether the lookup itself succeeds: suggestions are diagnostic text
/// only.
fn unknown_field_error(schema: &Schema, field: &str) -> Error {
    let base = format!("schema {:?} has no field {field:?}", schema.name());
    let message = closest_field_suggestion(schema, field).map_or_else(
        || base.clone(),
        |name| format!("{base}; did you mean {name:?}?"),
    );
    Error::new(ErrorKind::InvalidOperation, message)
}

/// Finds the single field name in `schema` that best matches `field`, for a
/// `did you mean` suggestion on the unknown-field error path only. Never
/// consulted on a successful `.field(name)` lookup, which stays exact: this
/// makes the namespace more user-friendly on typos without silently forgiving
/// `.field("Status")` to match `status`.
///
/// Prefers a canonical (`FieldKey`) match over an edit-distance match. Suggests
/// nothing when `field` itself fails `FieldKey` validation, when no field
/// canonically or approximately matches, or when more than one field
/// canonically matches (schema resolution already rejects two fields sharing a
/// canonical form within one Schema, so this last case is defensive).
fn closest_field_suggestion<'a>(
    schema: &'a Schema,
    field: &str,
) -> Option<&'a str> {
    let input_key = FieldKey::try_from(field).ok()?;
    let mut canonical_matches =
        schema.fields().keys().filter(|name| input_key.is_match(name.as_str()));
    match (canonical_matches.next(), canonical_matches.next()) {
        (Some(only), None) => Some(only.as_str()),
        (Some(_), Some(_)) => None,
        (None, _) => closest_field_name(schema, field),
    }
}

/// Finds the field name in `schema` with the smallest edit distance to `field`,
/// for a `did you mean` suggestion when no field canonically matches.
///
/// Thin wrapper over [`crate::field::closest_match`]: see its doc for the
/// matching threshold.
fn closest_field_name<'a>(schema: &'a Schema, field: &str) -> Option<&'a str> {
    field::closest_match(
        schema.fields().keys().map(|name| (name.as_str(), name.as_str())),
        field,
    )
}

#[cfg(test)]
mod tests {
    use minijinja::Environment;

    use super::{super::query::QueryOps, *};

    /// A minimal [`Environment`] with `schema` registered against `directory`.
    fn env(directory: &Path) -> Environment<'static> {
        let mut env = Environment::new();
        schema_ops(directory).register(&mut env);
        env
    }

    fn schema_ops(directory: &Path) -> SchemaOps {
        let root =
            directory.parent().and_then(Path::parent).unwrap_or(directory);
        let keys = FrontmatterFieldKeys::new(
            FieldKey::try_new("class").expect("valid field key"),
            FieldKey::try_new("title").expect("valid field key"),
            FieldKey::try_new("aliases").expect("valid field key"),
        );
        SchemaOps::new(Arc::new(SchemaContext::new(
            Arc::from(root),
            Arc::from(directory),
            keys,
        )))
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

        /// Writes `content` as a project-relative Markdown file under `root`,
        /// creating parent directories if needed.
        pub(super) fn write_note(root: &Path, path: &str, content: &str) {
            let path = root.join(path);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).expect("create note parent dir");
            }
            fs::write(path, content).expect("write note");
        }
    }
    use fixtures::{write_note, write_schema};

    mod get_value {
        use super::*;

        #[test]
        fn returns_none_for_an_unknown_key() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let ops = Arc::new(schema_ops(temp.path()));

            assert!(ops.get_value(&Value::from("unknown")).is_none());
        }

        #[test]
        fn returns_none_for_a_non_string_key() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let ops = Arc::new(schema_ops(temp.path()));

            assert!(ops.get_value(&Value::from(1)).is_none());
        }
    }

    mod enumerate {
        use super::*;

        #[test]
        fn lists_every_method() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let ops = Arc::new(schema_ops(temp.path()));

            assert!(matches!(ops.enumerate(), Enumerator::Str(METHODS)));
        }

        #[test]
        fn every_enumerated_method_resolves_via_get_value() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let ops = Arc::new(schema_ops(temp.path()));

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

        use pretty_assertions::assert_eq;

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
            let ops = Arc::new(schema_ops(&schemas_dir));
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

        #[test]
        fn a_query_call_reuses_the_registry_schema_already_cached() {
            // `sci_fi` transitively is-a `book` via `extends`. An empty,
            // degraded registry (`SchemaRegistry::load` on a missing
            // directory - see the sibling test above) has no Schemas at
            // all, so `from("@book*")` would fall back to exact-match and
            // miss a Note whose File Class is `sci_fi`. Asserting the Note
            // still matches after the directory is gone proves `query`
            // reused the registry `schema.get` already cached in this render,
            // not a fresh (post-deletion) load of its own.
            let temp = tempfile::tempdir().expect("create temp dir");
            let schemas_dir = temp.path().join(".traces/schemas");
            write_schema(temp.path(), "book", "");
            write_schema(temp.path(), "sci_fi", "extends = [\"book\"]\n");
            write_note(
                temp.path(),
                "dune.md",
                "---\nclass: sci_fi\n---\n# dune",
            );
            let ctx = Arc::new(SchemaContext::new(
                Arc::from(temp.path()),
                Arc::from(schemas_dir.as_path()),
                FrontmatterFieldKeys::new(
                    FieldKey::try_new("class").expect("valid field key"),
                    FieldKey::try_new("title").expect("valid field key"),
                    FieldKey::try_new("aliases").expect("valid field key"),
                ),
            ));
            let schema_ops = Arc::new(SchemaOps::new(Arc::clone(&ctx)));
            let get = schema_ops
                .get_value(&Value::from("get"))
                .expect("get is a known method");
            let query_ops = Arc::new(QueryOps::page(
                Arc::from(temp.path()),
                Arc::from("class"),
                Arc::clone(&ctx),
            ));
            let from = query_ops
                .get_value(&Value::from("from"))
                .expect("from is a known method");
            let env = Environment::new();
            let state = env.empty_state();

            // Populates state's cached SchemaRegistry under `schema`.
            get.call(&state, &[Value::from("book")])
                .expect("schema.get loads and caches the registry");
            fs::remove_dir_all(&schemas_dir).expect("remove schemas dir");

            let matched = from.call(&state, &[Value::from("@book*")]).expect(
                "query.from must reuse the cached registry, not reread the \
                 now-missing directory",
            );

            assert_eq!(
                matched.len().expect("from returns a sized sequence"),
                1,
                "sci_fi's is-a relationship to book only resolves through the \
                 registry schema.get already cached"
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
        fn file_field_returns_index_derived_label_value_pairs() {
            let temp = tempfile::tempdir().expect("create temp dir");
            write_schema(
                temp.path(),
                "book",
                r#"
                [fields.cover]
                type = "file"
                folders = ["covers"]
                ext = "md"
                class = ["book"]
                "#,
            );
            write_schema(temp.path(), "sci_fi", r#"extends = ["book"]"#);
            write_note(
                temp.path(),
                "covers/dune.md",
                "---\naliases:\n  - Friendly Dune\nclass: sci_fi\n---\n",
            );
            write_note(
                temp.path(),
                "covers/plain.md",
                "---\nclass: book\n---\n",
            );
            write_note(
                temp.path(),
                "covers/titled.md",
                "---\ntitle: Titled Cover\nclass: book\n---\n",
            );
            write_note(
                temp.path(),
                "misc/ignored.md",
                "---\nclass: book\n---\n",
            );
            write_note(temp.path(), "covers/ignored.txt", "");

            let rendered = render(
                &temp.path().join(".traces/schemas"),
                "{% for item in schema.get('book').field('cover') %}{{ \
                 item.label }}={{ item.value }}{% if not loop.last %}|{% \
                 endif %}{% endfor %}",
            )
            .expect("render succeeds");

            assert_eq!(
                rendered,
                "Friendly Dune=covers/dune.md|plain=covers/plain.md|Titled \
                 Cover=covers/titled.md"
            );
        }

        #[test]
        fn file_field_options_refresh_between_renders() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let schemas_dir = temp.path().join(".traces/schemas");
            write_schema(
                temp.path(),
                "book",
                r#"
                [fields.cover]
                type = "file"
                folders = ["covers"]
                ext = "md"
                "#,
            );
            let env = env(&schemas_dir);
            let source = "{{ schema.get('book').field('cover') | \
                          map(attribute='value') | join(',') }}";
            write_note(temp.path(), "covers/first.md", "");

            let first = env
                .render_str(source, minijinja::context!())
                .expect("first render succeeds");
            write_note(temp.path(), "covers/second.md", "");
            let second = env
                .render_str(source, minijinja::context!())
                .expect("second render succeeds");

            assert_eq!(first, "covers/first.md");
            assert_eq!(second, "covers/first.md,covers/second.md");
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
            schema_ops(&temp.path().join(".traces/schemas")).register(&mut env);

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
        fn unknown_field_with_a_canonical_typo_suggests_the_exact_field() {
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

            let error = render(
                &temp.path().join(".traces/schemas"),
                "{{ schema.get('book').field('Status') }}",
            )
            .expect_err("unknown field should error");

            assert!(error.to_string().contains(r#"did you mean "status"?"#));
        }

        #[test]
        fn unknown_field_with_an_edit_distance_typo_suggests_the_closest_field()
        {
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

            let error = render(
                &temp.path().join(".traces/schemas"),
                "{{ schema.get('book').field('statu') }}",
            )
            .expect_err("unknown field should error");

            assert!(error.to_string().contains(r#"did you mean "status"?"#));
        }

        #[test]
        fn unknown_field_with_no_close_candidate_has_no_suggestion() {
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

            let error = render(
                &temp.path().join(".traces/schemas"),
                "{{ schema.get('book').field('completely_unrelated') }}",
            )
            .expect_err("unknown field should error");

            let message = error.to_string();
            assert!(message.contains("has no field"));
            assert!(!message.contains("did you mean"));
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
