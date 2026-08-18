//! Register the `schema` namespace for templates, and wire
//! [`crate::schema::Schema`] into minijinja directly.
//!
//! [`SchemaOps`] is the `schema` namespace object registered as a minijinja
//! global by [`super::TemplateEngine`]. It exposes one method:
//!
//! - `schema.get(name)`: binds the resolved [`Schema`] named `name` directly
//!   (`Schema` implements minijinja's [`Object`] itself, mirroring
//!   [`super::query`]'s `impl Object for QueryOutcome`), hard-erroring if no
//!   Schema by that name resolves.
//!
//! The bound [`Schema`] exposes one plain attribute and three methods:
//!
//! - `.name`: the Schema's own name (its source file's stem).
//! - `.field(name)`: the named field's selectable values, as plain strings for
//!   a `select` field, label/value objects for a `file` field, or `none` for
//!   every other type. `file` fields resolve live from the render-scoped
//!   `FileIndex`: labels use the configured `[frontmatter]` aliases key,
//!   falling back to the configured title key, then the filename stem; values
//!   are paths.
//! - `.children()`: every Schema that directly `extends` this one, each itself
//!   a bound `Schema`. Empty, not an error, when nothing directly extends this
//!   Schema.
//! - `.descendants()`: every Schema that is-a this one transitively (extends it
//!   directly or via an ancestor), each itself a bound `Schema` so a Template
//!   can walk the whole subtree (`.name`, `.field(...)`, `.children()`, and
//!   `.descendants()` again). Empty, not an error, when nothing extends this
//!   Schema.
//!
//! `schema.get` and `.field` are structural references: a typo in either name
//! surfaces as a render error carrying template context, not a panic, mirroring
//! [`super::query`]'s `errors` module. Class-based predicate references
//! (`from_class`, `file`-field filters) degrade missing class targets to exact
//! matching with a warning; structural Schema and field names still hard-error.
//!
//! # Resolution Timing
//!
//! Every Schema TOML file is read and resolved once, at
//! [`super::TemplateEngine`]
//! construction (`SchemaService::new`) — not lazily on the first
//! `schema.get(...)` call. Every render for that engine's whole lifetime
//! shares the identical already-resolved `Arc<SchemaService>`: no
//! render-scoped registry cache or re-resolution.
//!
//! [`Schema`] itself stores each field behind the resolved Schema's shared
//! [`Arc`], so `schema.get` shares that Schema's field map instead of
//! deep-cloning it per call.
//!
//! [`Schema`]'s own `Object` impl carries no context fields (no per-instance
//! registry/config bundle, unlike a wrapper type): `.field()`/`.children()`/
//! `.descendants()` instead re-fetch the render's `Arc<SchemaService>` from
//! `State`'s temp storage on demand, seeded once by `schema.get` (see
//! [`cached_service`]). `.descendants()`/`.children()` themselves are *not*
//! memoized across calls within a render: each call re-fetches the
//! (`State`-cached) registry and reads the target Schema's already-precomputed
//! `children`/`descendants` set — no per-call registry scan.

use std::sync::Arc;

use indexmap::IndexMap;
use minijinja::{
    Environment, Error, ErrorKind, State,
    value::{Enumerator, Object, Value},
};

use crate::{
    field::FieldValue,
    query::{FileOption, FileOptionFilter, FrontmatterFieldKeys},
    schema::{
        Schema, SchemaFileFieldRef, SchemaSelectFieldEntry, SchemaService,
    },
};

/// Method names `schema` exposes, for [`SchemaOps::enumerate`].
const METHODS: &[&str] = &["get"];

/// Keys a bound [`Schema`] exposes: `field`/`children`/`descendants` are
/// called as methods, `name` is a plain attribute. Backs
/// [`Object::enumerate`] on [`Schema`]'s own `impl Object` below.
const SCHEMA_METHODS: &[&str] = &["children", "descendants", "field", "name"];

/// The [`State::set_temp`] key seeding the render's `Arc<SchemaService>`, so
/// [`Schema`]'s own [`Object`] impl can reach [`SchemaService::spec`]/
/// [`SchemaService::matches`]/[`SchemaService::children`]/
/// [`SchemaService::descendants`] without holding a context field itself.
/// Seeded by [`SchemaOps::get_value`]'s `"get"` branch before it hands out
/// any `Schema`-backed value, so every live `Schema` [`Value`] in a render is
/// guaranteed to have this already stashed by the time `.field()`/
/// `.children()`/`.descendants()` runs.
const SCHEMA_SERVICE_CACHE_KEY: &str = "schema.service_cache";

/// Backs the `schema` namespace object.
#[derive(Debug)]
pub(super) struct SchemaOps {
    service: Arc<SchemaService>,
}

impl SchemaOps {
    /// Wraps the shared [`SchemaService`] used to load Schemas and resolve
    /// file-field options.
    #[inline]
    #[must_use]
    pub(super) fn new(service: Arc<SchemaService>) -> Self {
        Self {
            service,
        }
    }

    /// Registers this object as the `schema` global.
    #[inline]
    pub(super) fn register(self, env: &mut Environment<'static>) {
        env.add_global("schema", Value::from_object(self));
    }
}

impl Object for SchemaOps {
    fn get_value(self: &Arc<Self>, key: &Value) -> Option<Value> {
        match key.as_str()? {
            "get" => {
                let ops = Arc::clone(self);
                Some(Value::from_function(
                    move |state: &State, name: &str| -> Result<Value, Error> {
                        super::cache::set_temp(
                            state,
                            SCHEMA_SERVICE_CACHE_KEY,
                            Arc::clone(&ops.service),
                        );
                        ops.service
                            .get(name)
                            .cloned()
                            .map(Value::from_dyn_object)
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

/// Returns the render's `Arc<SchemaService>`, seeded by
/// [`SchemaOps::get_value`]'s `"get"` branch before it hands out any
/// `Schema`-backed value.
///
/// # Errors
///
/// [`ErrorKind::InvalidOperation`] if no `Schema` value produced this render
/// ever went through `schema.get` — cannot happen for a `Schema` bound the
/// documented way, since that is the only route to one.
fn cached_service(state: &State) -> Result<Arc<SchemaService>, Error> {
    super::cache::get_temp(state, SCHEMA_SERVICE_CACHE_KEY).ok_or_else(|| {
        Error::new(
            ErrorKind::InvalidOperation,
            "internal error: no Schema service cached for this render",
        )
    })
}

impl Object for Schema {
    #[inline]
    fn get_value(self: &Arc<Self>, key: &Value) -> Option<Value> {
        match key.as_str()? {
            "name" => Some(Value::from(self.name())),
            "field" => {
                let schema = Arc::clone(self);
                Some(Value::from_function(
                    move |state: &State, name: &str| -> Result<Value, Error> {
                        let field = schema.field(name).ok_or_else(|| {
                            unknown_field_error(&schema, name)
                        })?;
                        if let Some(SchemaFileFieldRef {
                            folders,
                            ext,
                            class: classes,
                        }) = field.file_filter()
                        {
                            return file_field_values(
                                state, folders, ext, classes,
                            );
                        }
                        Ok(field.select_values().map_or_else(
                            || Value::from(()),
                            |values| {
                                Value::from(
                                    values
                                        .iter()
                                        .map(select_entry_value)
                                        .collect::<Vec<_>>(),
                                )
                            },
                        ))
                    },
                ))
            }
            "children" => {
                let schema = Arc::clone(self);
                Some(Value::from_function(
                    move |state: &State| -> Result<Value, Error> {
                        bind_related(state, &schema, SchemaService::children_of)
                    },
                ))
            }
            "descendants" => {
                let schema = Arc::clone(self);
                Some(Value::from_function(
                    move |state: &State| -> Result<Value, Error> {
                        bind_related(
                            state,
                            &schema,
                            SchemaService::descendants_of,
                        )
                    },
                ))
            }
            _ => None,
        }
    }

    #[inline]
    fn enumerate(self: &Arc<Self>) -> Enumerator {
        Enumerator::Str(SCHEMA_METHODS)
    }
}

/// A [`SchemaService`] hierarchy query (`children`/`descendants`), shared by
/// [`bind_related`].
type RelateFn = fn(&SchemaService, &str) -> Vec<Arc<Schema>>;

/// Binds every Schema `relate` returns for `schema` as a `Value` list of
/// bound `Schema` objects. Shared by `.children()`/`.descendants()`, which
/// differ only in which [`SchemaService`] method they call.
fn bind_related(
    state: &State,
    schema: &Arc<Schema>,
    relate: RelateFn,
) -> Result<Value, Error> {
    let service = cached_service(state)?;
    let related = relate(&service, schema.name());
    Ok(Value::from(
        related.into_iter().map(Value::from_dyn_object).collect::<Vec<_>>(),
    ))
}

/// Converts a resolved `select`-field entry into the minijinja `Value` shape
/// `.field()` returns: a plain string when `label == value` and `extra` is
/// empty (always true under this ticket — see
/// [`SchemaSelectFieldEntry`](crate::schema::SchemaSelectFieldEntry)'s docs),
/// else a `{value, label, ...extra}` object for a future structured source.
fn select_entry_value(entry: &SchemaSelectFieldEntry) -> Value {
    if entry.label() == entry.value() && entry.extra().is_empty() {
        return Value::from_serialize(entry.value());
    }
    let mut object: IndexMap<String, FieldValue> = entry.extra().clone();
    object.insert("value".to_owned(), entry.value().clone());
    object.insert("label".to_owned(), entry.label().clone());
    Value::from_serialize(&object)
}

/// Resolves a file-typed field against the render-scoped `FileIndex`.
fn file_field_values(
    state: &State,
    folders: &[String],
    ext: Option<&str>,
    classes: &[String],
) -> Result<Value, Error> {
    let service = cached_service(state)?;
    let index = super::query::cached_refresh(state, service.spec().root())
        .map_err(super::query::index_error)?;
    let class_matches = if classes.is_empty() {
        None
    } else {
        Some(service.matches(classes))
    };
    let keys = FrontmatterFieldKeys::new(
        service.spec().class_field().clone(),
        service.spec().title_field().clone(),
        service.spec().aliases_field().clone(),
    );
    let options = index.file_options(FileOptionFilter::new(
        folders,
        ext,
        class_matches.as_ref(),
        &keys,
    ));
    Ok(Value::from(
        options.into_iter().map(file_option_value).collect::<Vec<_>>(),
    ))
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

/// Builds the error for `schema.get(name)` naming a Schema that did not
/// resolve.
fn unknown_schema_error(name: &str) -> Error {
    Error::new(ErrorKind::InvalidOperation, format!("unknown Schema {name:?}"))
}

/// Builds the error for `.field(name)` naming a field absent from `schema`'s
/// resolved fields.
///
/// Appends a `; did you mean "..."?` hint when
/// [`Schema::suggest_field`] finds exactly one plausible candidate; omits the
/// hint otherwise. Never changes whether the lookup itself succeeds:
/// suggestions are diagnostic text only.
fn unknown_field_error(schema: &Schema, field: &str) -> Error {
    let base = format!("schema {:?} has no field {field:?}", schema.name());
    let message = schema.suggest_field(field).map_or_else(
        || base.clone(),
        |name| format!("{base}; did you mean {name:?}?"),
    );
    Error::new(ErrorKind::InvalidOperation, message)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use minijinja::Environment;

    use super::*;
    use crate::{config::SchemaConfigSpec, schema::SchemaError};

    /// A minimal [`Environment`] with `schema` registered against `directory`.
    fn env(directory: &Path) -> Environment<'static> {
        let mut env = Environment::new();
        schema_ops(directory).register(&mut env);
        env
    }

    fn schema_ops(directory: &Path) -> SchemaOps {
        let root =
            directory.parent().and_then(Path::parent).unwrap_or(directory);
        let (service, _, _) =
            SchemaService::new(SchemaConfigSpec::for_test(root, directory))
                .expect("valid test schema directory");
        SchemaOps::new(Arc::new(service))
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

    mod warnings {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn a_missing_extends_target_degrades_with_a_warning_instead_of_failing_the_render()
         {
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

    mod children {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn returns_a_direct_extender_as_a_bound_schema() {
            let temp = tempfile::tempdir().expect("create temp dir");
            write_schema(temp.path(), "thing", "");
            write_schema(temp.path(), "book", r#"extends = ["thing"]"#);
            write_schema(temp.path(), "sci_fi", r#"extends = ["book"]"#);

            let rendered = render(
                &temp.path().join(".traces/schemas"),
                "{{ schema.get('thing').children() | map(attribute='name') | \
                 join(',') }}",
            )
            .expect("render succeeds");

            assert_eq!(rendered, "book");
        }

        #[test]
        fn excludes_a_transitive_descendant() {
            let temp = tempfile::tempdir().expect("create temp dir");
            write_schema(temp.path(), "thing", "");
            write_schema(temp.path(), "book", r#"extends = ["thing"]"#);
            write_schema(temp.path(), "sci_fi", r#"extends = ["book"]"#);

            let rendered = render(
                &temp.path().join(".traces/schemas"),
                "{{ schema.get('thing').children() | length }}",
            )
            .expect("render succeeds");

            assert_eq!(rendered, "1");
        }

        #[test]
        fn returns_an_empty_list_for_a_leaf_schema_not_none_or_an_error() {
            let temp = tempfile::tempdir().expect("create temp dir");
            write_schema(temp.path(), "book", "");
            write_schema(temp.path(), "sci_fi", r#"extends = ["book"]"#);

            let rendered = render(
                &temp.path().join(".traces/schemas"),
                "{{ schema.get('sci_fi').children() | length }}",
            )
            .expect("render succeeds");

            assert_eq!(rendered, "0");
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
            // Proves the "descendants" branch threads the render-cached
            // `SchemaService`/registry into each returned bound `Schema`
            // (via `bind_related`'s own `cached_service`/`cached_schema_set`
            // calls, not a fresh/empty one) — the service-level
            // `descendants` test already proves the underlying data is
            // transitively correct; this proves the render-facing chain
            // (`.descendants().descendants()`) the module docs promise
            // actually works.
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
        use pretty_assertions::assert_eq;

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
        fn a_malformed_schema_file_fails_construction_not_a_panic() {
            // Resolution happens once at `SchemaService::new` (construction
            // time), not lazily on the first `schema.get` call, so a
            // malformed Schema TOML now fails construction rather than
            // surfacing as a render error.
            let temp = tempfile::tempdir().expect("create temp dir");
            write_schema(temp.path(), "book", "not valid toml [[[");

            let error = SchemaService::new(SchemaConfigSpec::for_test(
                temp.path(),
                &temp.path().join(".traces/schemas"),
            ))
            .expect_err(
                "malformed Schema TOML fails construction, not a panic",
            );

            assert!(matches!(error, SchemaError::Parse { .. }));
        }

        #[test]
        fn a_broken_sibling_schema_still_breaks_construction() {
            // Directory-wide load failure is documented, not hidden:
            // resolution happens once at construction, so one malformed
            // sibling Schema fails the whole registry — and therefore every
            // render, regardless of whether the template ever reaches into
            // `schema` (see the `register` module below).
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

            let error = SchemaService::new(SchemaConfigSpec::for_test(
                temp.path(),
                &temp.path().join(".traces/schemas"),
            ))
            .expect_err(
                "a broken sibling Schema fails the whole registry load",
            );

            assert!(matches!(error, SchemaError::Parse { .. }));
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
        fn a_broken_schema_now_breaks_construction_even_when_the_template_never_touches_schema()
         {
            // Accepted behavior change: resolution happens once at
            // construction (`SchemaService::new`), not lazily on the first
            // `schema.get` call, so a malformed Schema TOML fails
            // construction even for a template that never references
            // `schema.*`.
            let temp = tempfile::tempdir().expect("create temp dir");
            write_schema(temp.path(), "broken", "not valid toml [[[");

            let error = SchemaService::new(SchemaConfigSpec::for_test(
                temp.path(),
                &temp.path().join(".traces/schemas"),
            ))
            .expect_err(
                "a malformed Schema now fails construction unconditionally",
            );

            assert!(matches!(error, SchemaError::Parse { .. }));
        }
    }
}
