//! Register the `schema` namespace for templates, and wire
//! [`crate::schema::Schema`] into minijinja directly.
//!
//! [`SchemaOps`] is the `schema` namespace object registered as a minijinja
//! global by [`super::TemplateEngine`]. It exposes one method:
//!
//! - `schema.get(name)`: binds the resolved [`Schema`] named `name` directly
//!   (`Schema` implements minijinja's [`Object`] itself, mirroring
//!   [`super::query`]'s `impl Object for QueryRecordSet`), hard-erroring if no
//!   Schema by that name resolves.
//!
//! The bound [`Schema`] exposes one plain attribute and three methods:
//!
//! - `.name`: the Schema's own name (its source file's stem).
//! - `.field(name)`: the named field's selectable values. For a `select` field,
//!   plain strings (for simple string lists) or resolved `{value, label,
//!   ...extra}` objects (for structured inline objects and file sources). For a
//!   `file` field, a Query Source filter, declarative data built without
//!   executing any query itself, composable with `query.from(...)` and `|
//!   with_children`/`| with_descendants`. `none` for every other type.
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
//! [`super::TemplateEngine`] construction (`SchemaService::new`), not lazily on
//! the first `schema.get(...)` call. Every render for that engine's whole
//! lifetime shares the identical already-resolved `Arc<SchemaService>`: no
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
//! `children`/`descendants` set, with no per-call registry scan.

use std::{collections::BTreeSet, sync::Arc};

use indexmap::IndexMap;
use minijinja::{
    Environment, Error, ErrorKind, State,
    value::{Enumerator, Object, Value},
};

use super::error::TemplateEngineResult;
use crate::{
    field::FieldValue,
    query::{ClassExpansionMode, SourceAtom, SourceExpr, SourceSelector},
    schema::{
        Schema, SchemaFileFieldRef, SchemaSelectFieldEntry, SchemaService,
    },
};

/// Method names `schema` exposes, for [`SchemaOps::enumerate`].
const METHODS: &[&str] = &["get"];

/// Keys a bound [`Schema`] exposes: `field`/`children`/`descendants` are called
/// as methods, `name` is a plain attribute. Backs [`Object::enumerate`] on
/// [`Schema`]'s own `impl Object` below.
const SCHEMA_METHODS: &[&str] = &["children", "descendants", "field", "name"];

/// The [`State::set_temp`] key seeding the render's `Arc<SchemaService>`, so
/// [`Schema`]'s own [`Object`] impl can reach [`SchemaService::matches`]/
/// [`SchemaService::children_of`]/[`SchemaService::descendants_of`] without
/// holding a context field itself.
/// Seeded by [`SchemaOps::get_value`]'s `"get"` branch before it hands out any
/// `Schema`-backed value, so every live `Schema` [`Value`] in a render is
/// guaranteed to have this already stashed by the time
/// `.field()`/`.children()`/`.descendants()` runs.
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
    pub(super) const fn new(service: Arc<SchemaService>) -> Self {
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
                    move |state: &State,
                          name: &str|
                          -> TemplateEngineResult<Value> {
                        super::cache::set_temp(
                            state,
                            SCHEMA_SERVICE_CACHE_KEY,
                            Arc::clone(&ops.service),
                        );
                        ops.service
                            .get(name)
                            .cloned()
                            .map(Value::from_dyn_object)
                            .ok_or_else(|| {
                                Error::new(
                                    ErrorKind::InvalidOperation,
                                    format!("unknown Schema {name:?}"),
                                )
                            })
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
/// ever went through `schema.get`; cannot happen for a `Schema` bound the
/// documented way, since that is the only route to one.
fn cached_service(state: &State) -> TemplateEngineResult<Arc<SchemaService>> {
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
                    move |name: &str| -> TemplateEngineResult<Value> {
                        let field = schema.field(name).ok_or_else(|| {
                            unknown_field_error(&schema, name)
                        })?;
                        if let Some(filter) = field.file_filter() {
                            let source = file_field_source(&filter)
                                .map_err(glob_compile_error)?;
                            return Ok(Value::from_object(source));
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
                    move |state: &State| -> TemplateEngineResult<Value> {
                        bind_related(state, &schema, SchemaService::children_of)
                    },
                ))
            }
            "descendants" => {
                let schema = Arc::clone(self);
                Some(Value::from_function(
                    move |state: &State| -> TemplateEngineResult<Value> {
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

/// Binds every Schema `relate` returns for `schema` as a `Value` list of bound
/// `Schema` objects. Shared by `.children()`/`.descendants()`, which differ
/// only in which [`SchemaService`] method they call.
fn bind_related(
    state: &State,
    schema: &Arc<Schema>,
    relate: RelateFn,
) -> TemplateEngineResult<Value> {
    let service = cached_service(state)?;
    let related = relate(&service, schema.name());
    Ok(Value::from(
        related.into_iter().map(Value::from_dyn_object).collect::<Vec<_>>(),
    ))
}

/// Converts a resolved `select`-field entry into the minijinja `Value` shape
/// `.field()` returns: a plain string when `label == value` and `extra` is
/// empty, otherwise a `{value, label, ...extra}` object for structured inline
/// value objects or external values-file entries.
fn select_entry_value(entry: &SchemaSelectFieldEntry) -> Value {
    if entry.label() == entry.value() && entry.extra().is_empty() {
        return Value::from_serialize(entry.value());
    }
    let mut object: IndexMap<String, FieldValue> = entry.extra().clone();
    object.insert("value".to_owned(), entry.value().clone());
    object.insert("label".to_owned(), entry.label().clone());
    Value::from_serialize(&object)
}

/// Builds a [`SourceSelector`] filter from a `file` field's declaration.
///
/// Empty `folders`/`class` are omitted from the built expression rather than
/// defaulted to always-true, so a class-only field still narrows to Notes of
/// that class instead of matching every non-Note file. The `Class` atom's match
/// set stays empty here, the same unresolved shape DSL parsing produces for
/// `@Book`/`class(Name)`, and is populated later by
/// [`query::resolve_classes`](crate::query::resolve_classes), the same pass
/// that resolves DSL-parsed class atoms.
///
/// # Errors
///
/// Returns a [`regex::Error`] if a folder/extension glob fails to compile
/// (see [`compile_glob`]'s docs: not expected in practice).
fn file_field_source(
    filter: &SchemaFileFieldRef<'_>,
) -> Result<SourceSelector, regex::Error> {
    let SchemaFileFieldRef {
        folders,
        ext,
        class,
    } = *filter;
    let mut terms = Vec::new();
    if let [first, rest @ ..] = folders {
        let first_glob = glob_for(first, ext)?;
        let rest_globs = rest
            .iter()
            .map(|folder| glob_for(folder, ext))
            .collect::<Result<Vec<_>, _>>()?;
        terms.push(SourceExpr::disjunction(first_glob, rest_globs));
    } else if ext.is_some() {
        terms.push(SourceExpr::atom(glob_for("", ext)?));
    } else {
        // Neither folders nor ext restrict the match: no glob term to add.
    }
    if !class.is_empty() {
        terms.push(SourceExpr::atom(SourceAtom::Class {
            names: class.to_vec(),
            mode: ClassExpansionMode::Exact(BTreeSet::new()),
        }));
    }
    let mut terms = terms.into_iter();
    Ok(terms.next().map_or_else(
        || SourceSelector::All,
        |first| {
            SourceSelector::Expr(SourceExpr::conjunction(
                first,
                terms.collect(),
            ))
        },
    ))
}

/// Builds the glob matching every file under `folder` (or, when `folder` is
/// empty, every file anywhere in the project) with the given `ext`.
///
/// Mirrors the DSL's own glob compilation ([`compile_glob`]) so a `file`
/// field's filter composes identically to hand-written source text.
///
/// # Errors
///
/// Returns a [`regex::Error`] if the built glob fails to compile.
fn glob_for(
    folder: &str,
    ext: Option<&str>,
) -> Result<SourceAtom, regex::Error> {
    let suffix = ext.map_or_else(String::new, |ext| format!(".{ext}"));
    let glob = if folder.is_empty() {
        format!("**{suffix}")
    } else {
        format!("{folder}/**{suffix}")
    };
    SourceAtom::path(&glob)
}

/// Maps a [`regex::Error`] from a `file` field's glob compilation into a
/// [`minijinja::Error`].
///
/// Keeps the original error as [`source`].
///
/// [`source`]: std::error::Error::source
fn glob_compile_error(source: regex::Error) -> Error {
    super::error::invalid_operation("failed to compile file field glob", source)
}

/// Builds the error for `.field(name)` naming a field absent from `schema`'s
/// resolved fields.
///
/// Appends a `; did you mean "..."?` hint when [`Schema::suggest_field`] finds
/// exactly one plausible candidate; omits the hint otherwise. Never changes
/// whether the lookup itself succeeds: suggestions are diagnostic text only.
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
    use crate::schema::{SchemaError, SchemaFileError};

    /// A minimal [`Environment`] with `schema` registered against `directory`.
    fn env(directory: &Path) -> Environment<'static> {
        let mut env = Environment::new();
        schema_ops(directory).register(&mut env);
        env
    }

    fn schema_ops(directory: &Path) -> SchemaOps {
        SchemaOps::new(Arc::new(
            SchemaService::new(directory).expect("valid test schema directory"),
        ))
    }

    fn render(directory: &Path, source: &str) -> Result<String, Error> {
        env(directory).render_str(source, minijinja::context!())
    }

    /// A fuller [`Environment`] than [`env`]: also registers `query`/`tasks`
    /// and their terminal filters (`with_children`/`with_descendants` among
    /// them) alongside `schema`, against `directory`'s schema directory.
    /// Needed only by tests proving a `file` field's `SourceSelector` output
    /// composes with `query.from(...)`.
    fn full_env(directory: &Path) -> Environment<'static> {
        let root: Arc<Path> = Arc::from(
            directory.parent().and_then(Path::parent).unwrap_or(directory),
        );
        let service = Arc::new(
            SchemaService::new(directory).expect("valid test schema directory"),
        );
        let class_field: Arc<str> = Arc::from("class");
        let mut env = Environment::new();
        crate::template::engine::QueryOps::page(
            Arc::clone(&root),
            class_field,
            Arc::clone(&service),
        )
        .register(&mut env);
        crate::template::engine::QueryOps::register_terminal_filters(&mut env);
        SchemaOps::new(service).register(&mut env);
        env
    }

    fn render_full(directory: &Path, source: &str) -> Result<String, Error> {
        full_env(directory).render_str(source, minijinja::context!())
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
        fn file_field_composes_with_query_from_to_select_matching_files() {
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

            // `class = ["book"]` matches exactly, the same default depth a
            // bare `@Book`/`class(Book)` DSL atom gets: `covers/dune.md`
            // (class `sci_fi`, a `book` descendant) is excluded here, not
            // widened, proving `.field()` builds an unresolved `Exact` atom
            // rather than eagerly expanding classes itself. `misc/ignored.md`
            // (wrong folder) and `covers/ignored.txt` (wrong extension) are
            // excluded by the glob.
            let rendered = render_full(
                &temp.path().join(".traces/schemas"),
                "{% for item in query.from(schema.get('book').field('cover')) \
                 %}{{ item.file.name }}{% if not loop.last %}|{% endif %}{% \
                 endfor %}",
            )
            .expect("render succeeds");

            assert_eq!(rendered, "plain|titled");
        }

        #[test]
        fn file_field_reaches_a_non_markdown_file_with_no_note() {
            // The acceptance case this refactor exists for: a `file` field
            // matches non-Markdown files (no parsed Note at all), and
            // `.field()` selects it without executing any query itself —
            // only `query.from(...)` does, via the shared resolution path.
            let temp = tempfile::tempdir().expect("create temp dir");
            write_schema(
                temp.path(),
                "book",
                r#"
                [fields.cover]
                type = "file"
                folders = ["covers"]
                ext = "jpg"
                "#,
            );
            write_note(temp.path(), "covers/a.jpg", "");
            write_note(temp.path(), "notes/b.md", "# Unrelated Note");

            let rendered = render_full(
                &temp.path().join(".traces/schemas"),
                "{% for item in query.from(schema.get('book').field('cover')) \
                 %}{{ item.file.path }}{% endfor %}",
            )
            .expect("render succeeds");

            assert_eq!(rendered, "covers/a.jpg");
        }

        #[test]
        fn file_field_widens_to_descendants_via_with_descendants_filter() {
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
                "---\nclass: sci_fi\n---\n",
            );
            write_note(
                temp.path(),
                "covers/plain.md",
                "---\nclass: book\n---\n",
            );

            let rendered = render_full(
                &temp.path().join(".traces/schemas"),
                "{% for item in query.from(schema.get('book').field('cover') \
                 | with_descendants) %}{{ item.file.name }}{% if not \
                 loop.last %}|{% endif %}{% endfor %}",
            )
            .expect("render succeeds");

            assert_eq!(rendered, "dune|plain");
        }

        #[test]
        fn file_field_refreshes_between_renders() {
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
            let env = full_env(&schemas_dir);
            let source = "{% for item in \
                          query.from(schema.get('book').field('cover')) %}{{ \
                          item.file.path }}{% if not loop.last %},{% endif \
                          %}{% endfor %}";
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

            let error =
                SchemaService::new(&temp.path().join(".traces/schemas"))
                    .expect_err(
                        "malformed Schema TOML fails construction, not a panic",
                    );

            assert!(matches!(error.as_error(), SchemaError::File {
                source: SchemaFileError::Parse(_),
                ..
            }));
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

            let error =
                SchemaService::new(&temp.path().join(".traces/schemas"))
                    .expect_err(
                        "a broken sibling Schema fails the whole registry load",
                    );

            assert!(matches!(error.as_error(), SchemaError::File {
                source: SchemaFileError::Parse(_),
                ..
            }));
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
        fn renders_structured_file_sourced_select_field_as_objects() {
            use pretty_assertions::assert_eq;

            let temp = tempfile::tempdir().expect("create temp dir");
            let schemas_dir = temp.path().join(".traces/schemas");
            std::fs::create_dir_all(schemas_dir.join("values"))
                .expect("values dir");

            std::fs::write(
                schemas_dir.join("values/countries.toml"),
                "[[entries]]\nslug = \"us\"\nname = \"United \
                 States\"\ncontinent = \"North America\"\n",
            )
            .expect("write values file");

            write_schema(
                temp.path(),
                "book",
                r#"
                [fields.country]
                type = "select"
                values = { path = "values/countries.toml", value = "slug", label = "name" }
                "#,
            );

            let rendered = render(
                &schemas_dir,
                "{{ schema.get('book').field('country')[0].label }} ({{ \
                 schema.get('book').field('country')[0].value }}, {{ \
                 schema.get('book').field('country')[0].continent }})",
            )
            .expect("render succeeds");

            assert_eq!(rendered, "United States (us, North America)");
        }

        #[test]
        fn renders_structured_inline_select_field_as_objects() {
            use pretty_assertions::assert_eq;

            let temp = tempfile::tempdir().expect("create temp dir");
            let schemas_dir = temp.path().join(".traces/schemas");
            write_schema(
                temp.path(),
                "calendar",
                r#"
                [fields.month]
                type = "select"
                values = [
                    { value = "jan", label = "January", quarter = 1 },
                ]
                "#,
            );

            let rendered = render(
                &schemas_dir,
                "{{ schema.get('calendar').field('month')[0].label }}:Q{{ \
                 schema.get('calendar').field('month')[0].quarter }}",
            )
            .expect("render succeeds");

            assert_eq!(rendered, "January:Q1");
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

            let error =
                SchemaService::new(&temp.path().join(".traces/schemas"))
                    .expect_err(
                        "a malformed Schema now fails construction \
                         unconditionally",
                    );

            assert!(matches!(error.as_error(), SchemaError::File {
                source: SchemaFileError::Parse(_),
                ..
            }));
        }
    }

    mod select_entry_value {
        use pretty_assertions::assert_eq;
        use rstest::rstest;

        use super::*;

        #[test]
        fn literal_entry_returns_plain_string_value() {
            let entry = SchemaSelectFieldEntry::literal("draft".to_owned());

            let value = select_entry_value(&entry);

            assert_eq!(value.as_str(), Some("draft"));
        }

        #[rstest]
        #[case::different_label("draft", "Draft", IndexMap::new())]
        #[case::with_extra(
            "draft",
            "draft",
            IndexMap::from([("color".to_owned(), FieldValue::String("blue".to_owned()))])
        )]
        fn non_literal_entry_returns_object(
            #[case] val: &str,
            #[case] lbl: &str,
            #[case] extra: IndexMap<String, FieldValue>,
        ) {
            let entry = SchemaSelectFieldEntry::with_label(
                val.to_owned(),
                lbl.to_owned(),
                extra,
            );

            let value = select_entry_value(&entry);

            assert!(
                matches!(value.kind(), minijinja::value::ValueKind::Map),
                "expected map, got {:?}",
                value.kind()
            );
        }
    }
}
