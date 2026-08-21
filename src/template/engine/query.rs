//! Register the `query` and `tasks` namespaces for templates.
//!
//! Both namespaces are backed by [`QueryOps`], registered twice by
//! [`super::TemplateEngine::new`]: [`QueryOps::page`] creates the `query`
//! global and [`QueryOps::task`] creates the `tasks` global. Each namespace
//! starts a query with one of four methods, matching [`QuerySource`]'s
//! variants:
//!
//! - `.from()`: every indexed Note.
//! - `.from("#tag")`: Notes with an exact or nested tag.
//! - `.from("folder/")`: Notes under a folder.
//! - `.from("@Class*")`: Notes whose File Class is `Class` or a transitive
//!   descendant.
//!
//! Each call reuses the render's cached [`FileIndex`], refreshing it once per
//! render (see [`cached_refresh`]), and returns a [`QueryOutcome`] wrapped in a
//! [`Value`].
//!
//! # Row Shape
//!
//! `query` returns one row per Note. `tasks` returns one row per task item via
//! [`FileIndex::query_tasks`], exposing `task.completed` and `task.text`
//! alongside the parent Note's `file.*`, frontmatter, inline-field, and tag
//! metadata.
//!
//! # Chaining and Terminal Methods
//!
//! Template callers chain `.where(...)`/`.filter(...)`, `.sort(...)`,
//! `.limit(...)`, `.group_by(...)`, and `.flatten(...)`. The transformation
//! logic lives on [`QueryOutcome`] itself; this module only supplies the
//! minijinja [`Object`] wiring.
//!
//! [`QueryOutcome::table`], [`QueryOutcome::list`],
//! [`QueryOutcome::task_list`], and `count` (an alias for
//! [`QueryOutcome::len`]) are terminal instead: they render final
//! markdown/scalar output and end a chain rather than continue it. Each is
//! reachable both as a `call_method`
//! (`outcome.table(["Name"], ["file.name"])`) and as a pipeline filter,
//! registered once by [`QueryOps::register_terminal_filters`] (`outcome |
//! table(["Name"], ["file.name"])`). Both forms call the same [`QueryOutcome`]
//! method.
//!
//! # Object Wiring
//!
//! [`QueryOutcome`] and [`IndexRecord`] get their [`Object`] impls here instead
//! of in [`crate::index`], keeping that module independent from minijinja so
//! `traces task` can reuse [`FileIndex`], [`QueryOutcome`], and [`IndexRecord`]
//! without pulling in rendering concerns.
//!
//! `record` attributes other than `file` and `task` forward to
//! [`IndexRecord::field`], the same resolver `.where()` and `.sort()` use.
//! `record.file.*` and `record.task.*` use forwarding wrappers ([`FileFields`]
//! and [`TaskFields`]) instead: minijinja resolves a dotted attribute path one
//! segment at a time, so the wrappers call
//! [`FileField::parse`]/[`FileField::resolve`] and
//! [`IndexRecord::task_completed`]/[`IndexRecord::task_text`] directly,
//! skipping the string-prefix handling [`IndexRecord::field`] needs once the
//! `file`/`task` segment is already known.
//!
//! # Errors
//!
//! [`FileIndex::refresh`] and [`QueryError`] failures become
//! [`minijinja::Error`] values with stable messages and the original error
//! preserved as [`std::error::Error::source`], mirroring [`super::ui`]'s
//! `dialog_error` and [`super::error::confine_error`]. Query failures carry
//! template name, line, and column context like every other namespace.

use std::{cmp::Ordering, path::Path, sync::Arc};

use minijinja::{
    Environment, Error, ErrorKind, State,
    value::{DynObject, Enumerator, Object, ObjectRepr, Value, from_args},
};

use crate::{
    index::{FileIndex, FileIndexError},
    note::NoteFieldValue,
    query::{
        self, ClassExpansionMode, FileField, IndexRecord, QueryError,
        QueryOutcome, QuerySource, SourceAtom, resolve_classes,
    },
    schema::SchemaService,
};

/// Method names `query` and `tasks` each expose, for [`QueryOps::enumerate`].
const METHODS: &[&str] = &["from"];

/// The [`State::set_temp`] key used to cache one refreshed [`FileIndex`] for
/// the current render.
///
/// Shared by the `query` and `tasks` namespaces (both dispatch through
/// [`QueryOps::run`]) so a render calling into both pays for one
/// [`FileIndex::refresh`] instead of one per query call. `State`'s temp storage
/// is scoped to one render, including `{% include %}`s, and resets for the
/// next. A cache field on [`QueryOps`] itself would wrongly persist across
/// independent renders on a reused [`Environment`]/[`super::TemplateEngine`].
const INDEX_CACHE_KEY: &str = "query.index_cache";

/// Backs both the `query` and `tasks` minijinja namespace objects: one instance
/// per namespace, differing only in which global it registers as and which
/// [`FileIndex`] method builds the [`QueryOutcome`]. See
/// [`Self::page`]/[`Self::task`].
#[derive(Debug)]
pub(super) struct QueryOps {
    /// The minijinja global this instance registers as.
    name: &'static str,
    root: Arc<Path>,
    /// Frontmatter field naming a Note's File Class(es), from `[schemas]
    /// class_field`. Passed to source matching at execution time.
    class_field: Arc<str>,
    /// Shared with `schema.get()` so both namespaces resolve the same,
    /// already-resolved Schema registry, built once at
    /// [`super::TemplateEngine::new`].
    service: Arc<SchemaService>,
    /// [`FileIndex::query`] for `query`, [`FileIndex::query_tasks`] for
    /// `tasks`.
    query: fn(FileIndex, &QuerySource, &str) -> QueryOutcome,
}

impl QueryOps {
    /// Wraps `root` for page-level dispatch under the `query` global.
    #[inline]
    #[must_use]
    pub(super) const fn page(
        root: Arc<Path>,
        class_field: Arc<str>,
        service: Arc<SchemaService>,
    ) -> Self {
        Self {
            name: "query",
            root,
            class_field,
            service,
            query: query::query,
        }
    }

    /// Wraps `root` for task-level dispatch under the `tasks` global. Each row
    /// is one task item instead of one Note; see the module docs.
    #[inline]
    #[must_use]
    pub(super) const fn task(
        root: Arc<Path>,
        class_field: Arc<str>,
        service: Arc<SchemaService>,
    ) -> Self {
        Self {
            name: "tasks",
            root,
            class_field,
            service,
            query: query::query_tasks,
        }
    }

    /// Registers this object as its `name` global (`query` or `tasks`).
    #[inline]
    pub(super) fn register(self, env: &mut Environment<'static>) {
        let name = self.name;
        env.add_global(name, Value::from_object(self));
    }

    /// Registers `table`, `list`, `task_list`, and `count` as pipeline filters:
    /// `outcome | table(["Name"], ["file.name"])`, mirroring the call-method
    /// form `outcome.table(["Name"], ["file.name"])` documented on
    /// [`Object::call_method`] for [`QueryOutcome`]. Registered once, not per
    /// instance: these filters carry no state and apply to any [`QueryOutcome`]
    /// regardless of which namespace produced it.
    #[inline]
    pub(super) fn register_terminal_filters(env: &mut Environment<'static>) {
        env.add_filter("table", table_filter);
        env.add_filter("list", list_filter);
        env.add_filter("task_list", task_list_filter);
        env.add_filter("count", count_filter);
        env.add_filter("with_children", with_children_filter);
        env.add_filter("with_descendants", with_descendants_filter);
    }

    /// Runs this namespace's query method for `source` against `state`'s
    /// cached [`FileIndex`], refreshing it first if not already cached this
    /// render. See [`INDEX_CACHE_KEY`].
    ///
    /// # Errors
    ///
    /// - [`ErrorKind::InvalidOperation`] via [`index_error`] if refreshing the
    ///   index fails, including I/O errors while scanning `root`, database
    ///   access errors, and TOML (de)serialization errors on stored records.
    fn run(&self, state: &State, source: &QuerySource) -> Result<Value, Error> {
        let index = cached_refresh(state, &self.root).map_err(index_error)?;
        Ok(Value::from_object((self.query)(index, source, &self.class_field)))
    }
}

impl Object for QueryOps {
    fn get_value(self: &Arc<Self>, key: &Value) -> Option<Value> {
        match key.as_str()? {
            "from" => {
                let ops = Arc::clone(self);
                Some(Value::from_function(
                    move |state: &State,
                          expr: Option<Value>|
                          -> Result<Value, Error> {
                        let mut source = resolve_from_arg(expr.as_ref())?;
                        if source.has_classes() {
                            resolve_classes(&mut source, ops.service.as_ref());
                        }
                        ops.run(state, &source)
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

/// Returns the render's cached [`FileIndex`], refreshing and caching it in
/// `state`'s temp storage first if not already cached this render.
///
/// # Errors
///
/// - Any error [`FileIndex::refresh`] returns.
pub(super) fn cached_refresh(
    state: &State,
    root: &Path,
) -> Result<FileIndex, FileIndexError> {
    super::cache::cached(state, INDEX_CACHE_KEY, || FileIndex::refresh(root))
}

/// Maps a [`FileIndexError`] into a [`minijinja::Error`].
///
/// Keeps the original error as [`source`], matching [`super::ui`]'s
/// `dialog_error`.
///
/// [`source`]: std::error::Error::source
pub(super) fn index_error(source: FileIndexError) -> Error {
    super::error::invalid_operation("failed to refresh the file index", source)
}

/// Maps a [`QueryError`] into a [`minijinja::Error`].
///
/// Keeps the original error as [`source`].
///
/// [`source`]: std::error::Error::source
fn query_error(source: QueryError) -> Error {
    super::error::invalid_operation("query failed", source)
}

/// Resolves `.from()`'s optional argument into a [`QuerySource`]: `None`
/// selects every indexed file; a bound `QuerySource` `Value` (a Schema `file`
/// field, possibly widened by `with_children`/`with_descendants`) is used
/// as-is; any other value must be a DSL source-expression string.
///
/// # Errors
///
/// - [`ErrorKind::InvalidOperation`] if `expr` is neither a `QuerySource` nor a
///   string.
/// - Propagates [`QueryError::Syntax`] (via [`query_error`]) if `expr` is a
///   string that fails to parse as a source expression.
fn resolve_from_arg(expr: Option<&Value>) -> Result<QuerySource, Error> {
    let Some(value) = expr else {
        return Ok(QuerySource::All);
    };
    if let Some(source) = value.downcast_object_ref::<QuerySource>() {
        return Ok(source.clone());
    }
    let text = value.as_str().ok_or_else(from_arg_type_error)?;
    QuerySource::parse(text).map_err(query_error)
}

/// Builds the error for `.from()`'s argument being neither a `QuerySource`
/// nor a string.
fn from_arg_type_error() -> Error {
    Error::new(
        ErrorKind::InvalidOperation,
        "from() expects a source expression string or a Schema file field",
    )
}

/// Lets `.field()` hand a [`QuerySource`] filter across the minijinja boundary:
/// `.from()` and the `with_children`/`with_descendants` filters downcast it
/// back via [`Value::downcast_object_ref`]. No method overrides — mirrors
/// `cache.rs`'s `Cached<T>`, this crate's other bare `impl Object` used purely
/// to smuggle a typed value through a `Value`.
impl Object for QuerySource {}

impl Object for QueryOutcome {
    #[inline]
    fn repr(self: &Arc<Self>) -> ObjectRepr {
        ObjectRepr::Seq
    }

    #[inline]
    fn get_value(self: &Arc<Self>, key: &Value) -> Option<Value> {
        self.get(key.as_usize()?).cloned().map(Value::from_object)
    }

    #[inline]
    fn enumerate(self: &Arc<Self>) -> Enumerator {
        Enumerator::Seq(self.len())
    }

    /// Dispatches [`QueryOutcome`] methods by template name.
    ///
    /// The terminal methods `table`, `list`, `task_list`, and `count` render
    /// final output and return early, without touching the non-terminal chain
    /// below:
    ///
    /// - `table(headers, columns)` and `list(path)` render field path strings
    ///   (or, for `table`'s `headers`, display labels), not further
    ///   [`QueryOutcome`] arguments.
    /// - `task_list()` takes no arguments.
    /// - `count()` takes no arguments and returns [`QueryOutcome::len`]
    ///   directly; it cannot fail, unlike the other three.
    ///
    /// Every other name falls through to the non-terminal chain: `.where`/
    /// `.filter`, `.sort`, `.limit`, `.group_by`, and `.flatten`. Each of those
    /// calls consumes a clone of the current outcome and wraps the transformed
    /// result in a [`Value`] for further chaining:
    ///
    /// - `where` and `filter` both call [`QueryOutcome::filter`]. The Rust-side
    ///   `r#where` alias exists only for Rust callers.
    /// - `sort` defaults to ascending order when the optional `descending`
    ///   argument is omitted.
    ///
    /// # Errors
    ///
    /// - [`ErrorKind::UnknownMethod`] for any other method name.
    /// - [`ErrorKind::TooManyArguments`]/[`ErrorKind::MissingArgument`] if a
    ///   method's arguments don't match its expected shape.
    /// - [`ErrorKind::InvalidOperation`] via `query_error` if a field path or
    ///   filter expression is unparsable, `.limit(...)` is negative, or
    ///   `.task_list()` runs on records with no `task.*` fields.
    ///
    /// [`ErrorKind::InvalidOperation`]: minijinja::ErrorKind::InvalidOperation
    /// [`ErrorKind::MissingArgument`]: minijinja::ErrorKind::MissingArgument
    /// [`ErrorKind::TooManyArguments`]: minijinja::ErrorKind::TooManyArguments
    /// [`ErrorKind::UnknownMethod`]: minijinja::ErrorKind::UnknownMethod
    #[inline]
    fn call_method(
        self: &Arc<Self>,
        _state: &State<'_, '_>,
        method: &str,
        args: &[Value],
    ) -> Result<Value, Error> {
        match method {
            "table" => {
                let (headers, columns): (Vec<String>, Vec<String>) =
                    from_args(args)?;
                return self
                    .table(&as_str_slice(&headers), &as_str_slice(&columns))
                    .map(Value::from)
                    .map_err(query_error);
            }
            "list" => {
                let (path,): (&str,) = from_args(args)?;
                return self.list(path).map(Value::from).map_err(query_error);
            }
            "task_list" => {
                from_args::<()>(args)?;
                return self.task_list().map(Value::from).map_err(query_error);
            }
            "count" => {
                from_args::<()>(args)?;
                return Ok(Value::from(self.len()));
            }
            _ => {}
        }
        let outcome = self.as_ref().clone();
        let transformed = match method {
            "filter" | "where" => {
                let (expr,): (&str,) = from_args(args)?;
                outcome.filter(expr)
            }
            "sort" => {
                let (path, descending): (&str, Option<bool>) = from_args(args)?;
                outcome.sort(path, descending.unwrap_or(false))
            }
            "limit" => {
                let (n,): (i64,) = from_args(args)?;
                outcome.limit(n)
            }
            "group_by" => {
                let (path,): (&str,) = from_args(args)?;
                outcome.group_by(path)
            }
            "flatten" => {
                let (path,): (&str,) = from_args(args)?;
                outcome.flatten(path)
            }
            _ => return Err(Error::from(ErrorKind::UnknownMethod)),
        };
        transformed.map(Value::from_object).map_err(query_error)
    }
}

/// `outcome | table(...)` filter body. See [`QueryOutcome::table`].
///
/// Takes owned `Vec<String>` for `headers`/`columns` rather than borrowed
/// `Vec<&str>`. Two reasons stack here:
///
/// - minijinja's `Function` trait (backing filters) requires each parameter
///   type to implement `ArgType` for every lifetime, which a borrowed
///   `Vec<&str>` cannot satisfy; only its owned `String` form can.
/// - Even [`Object::call_method`]'s [`from_args`], which has no such
///   constraint, cannot borrow a `Vec<&str>` out of a list-literal argument
///   value: minijinja reports "type conversion is not legal in this situation
///   (implicit borrow)". Both entry points build owned `Vec<String>` first and
///   borrow from that.
#[expect(
    clippy::needless_pass_by_value,
    reason = "minijinja's Function trait dictates the by-value Vec<String> \
              signature; the body only needs to borrow each entry"
)]
fn table_filter(
    outcome: &QueryOutcome,
    headers: Vec<String>,
    columns: Vec<String>,
) -> Result<String, Error> {
    let headers = as_str_slice(&headers);
    let columns = as_str_slice(&columns);
    outcome.table(&headers, &columns).map_err(query_error)
}

/// Borrows each entry of `values` as `&str`. Shared by
/// [`Object::call_method`]'s `"table"` branch and [`table_filter`]: both must
/// build an owned `Vec<String>` first (see [`table_filter`]'s docs for why),
/// then borrow a `&[&str]` slice from it to call [`QueryOutcome::table`].
fn as_str_slice(values: &[String]) -> Vec<&str> {
    values.iter().map(String::as_str).collect()
}

/// `outcome | list(path)` filter body. See [`QueryOutcome::list`].
fn list_filter(outcome: &QueryOutcome, path: &str) -> Result<String, Error> {
    outcome.list(path).map_err(query_error)
}

/// `outcome | task_list` filter body. See [`QueryOutcome::task_list`].
fn task_list_filter(outcome: &QueryOutcome) -> Result<String, Error> {
    outcome.task_list().map_err(query_error)
}

/// `outcome | count` filter body: the number of records in `outcome`.
const fn count_filter(outcome: &QueryOutcome) -> usize {
    outcome.len()
}

/// `field | with_children` filter body: widens a `file` field's `Class` atom
/// (if any) to direct-children depth.
fn with_children_filter(source: &QuerySource) -> Value {
    Value::from_object(set_class_depth(
        source.clone(),
        ClassExpansionMode::Children,
    ))
}

/// `field | with_descendants` filter body: widens a `file` field's `Class` atom
/// (if any) to transitive-descendants depth.
fn with_descendants_filter(source: &QuerySource) -> Value {
    Value::from_object(set_class_depth(
        source.clone(),
        ClassExpansionMode::Descendants,
    ))
}

/// Replaces every `Class` atom's [`ClassExpansionMode`] in `source`, keeping
/// the match set empty (still unresolved; [`resolve_classes`] fills it in at
/// `.from()` dispatch time, same as DSL-parsed sources).
fn set_class_depth(
    mut source: QuerySource,
    mode: impl Fn(std::collections::BTreeSet<String>) -> ClassExpansionMode,
) -> QuerySource {
    if let QuerySource::Expr(expr) = &mut source {
        expr.visit_atoms_mut(&mut |atom| {
            if let SourceAtom::Class {
                mode: existing,
                ..
            } = atom
            {
                *existing = mode(std::collections::BTreeSet::new());
            }
        });
    }
    source
}

impl Object for IndexRecord {
    /// Resolves `record.<key>` or `record["<key>"]`.
    ///
    /// `"file"` and `"task"` return forwarding wrappers for `record.file.*` and
    /// `record.task.*`. Every other key resolves through [`IndexRecord`]'s
    /// field lookup, the same frontmatter, inline-field, and tag lookup used by
    /// `.where()` and `.sort()`.
    ///
    /// A rejected key, such as a dotted, empty, or unknown `file.*`/`task.*`
    /// accessor, resolves to `None` like any other missing attribute instead of
    /// surfacing [`QueryError::FieldPath`] as a render error.
    #[inline]
    fn get_value(self: &Arc<Self>, key: &Value) -> Option<Value> {
        let key = key.as_str()?;
        match key {
            "file" => Some(Value::from_object(FileFields(Arc::clone(self)))),
            "task" => Some(Value::from_object(TaskFields(Arc::clone(self)))),
            _ => self.field(key).ok().map(field_value),
        }
    }

    fn custom_cmp(self: &Arc<Self>, other: &DynObject) -> Option<Ordering> {
        other.downcast_ref::<IndexRecord>().map(|other| {
            if **self == *other {
                Ordering::Equal
            } else {
                Ordering::Less
            }
        })
    }
}

/// Forwards `record.file.<field>` to
/// [`FileField::parse`]/[`FileField::resolve`].
///
/// A thin wrapper rather than a second lookup path, needed only because
/// minijinja resolves a dotted attribute path one segment at a time:
/// `record.file` must itself resolve to *something* before `.name` can be
/// looked up on it. Calls the same [`FileField`] accessor pair
/// [`IndexRecord::field`] uses for its `file.*` branch, skipping that method's
/// string-based `file.` prefix handling, which doesn't apply here: `key` is
/// already a single attribute segment, never a dotted path.
#[derive(Debug)]
struct FileFields(Arc<IndexRecord>);

impl Object for FileFields {
    fn get_value(self: &Arc<Self>, key: &Value) -> Option<Value> {
        let field = FileField::parse(key.as_str()?)?;
        Some(field_value(field.resolve(self.0.file())))
    }

    fn enumerate(self: &Arc<Self>) -> Enumerator {
        Enumerator::Str(FileField::ACCESSOR_NAMES)
    }
}

/// Forwards `record.task.<field>` to
/// [`IndexRecord::task_completed`]/[`IndexRecord::task_text`].
///
/// Mirrors [`FileFields`]: minijinja resolves a dotted attribute path one
/// segment at a time, so `record.task` must itself resolve to something before
/// `.completed`/`.text` can be looked up.
///
/// On a page-level record (not built by [`FileIndex::query_tasks`]) both
/// accessors resolve to minijinja's `none`, a defined empty value rather than a
/// missing attribute, matching [`field_value`]'s handling of
/// [`NoteFieldValue::Null`].
#[derive(Debug)]
struct TaskFields(Arc<IndexRecord>);

impl Object for TaskFields {
    fn get_value(self: &Arc<Self>, key: &Value) -> Option<Value> {
        match key.as_str()? {
            "completed" => Some(
                self.0
                    .task_completed()
                    .map_or_else(|| Value::from(()), Value::from),
            ),
            "text" => Some(
                self.0.task_text().map_or_else(|| Value::from(()), Value::from),
            ),
            _ => None,
        }
    }

    fn enumerate(self: &Arc<Self>) -> Enumerator {
        Enumerator::Str(&["completed", "text"])
    }
}

/// Converts a resolved [`NoteFieldValue`] into a minijinja [`Value`].
///
/// - [`NoteFieldValue::Null`] becomes minijinja's `none` rather than
///   `undefined`: [`IndexRecord::field`]'s own docs note that a well-formed
///   path with no value resolves to `Null`, not an error. That's a defined
///   empty value, not a missing attribute.
/// - [`NoteFieldValue::Link`] renders as its target path; Traces has no
///   minijinja-facing link type yet.
fn field_value(value: NoteFieldValue) -> Value {
    match value {
        NoteFieldValue::Null => Value::from(()),
        NoteFieldValue::Bool(b) => Value::from(b),
        NoteFieldValue::Number(n) => Value::from(n),
        NoteFieldValue::String(s)
        | NoteFieldValue::Date(s)
        | NoteFieldValue::Duration(s) => Value::from(s),
        NoteFieldValue::Link(link) => Value::from(link.target().to_owned()),
        NoteFieldValue::List(items) => {
            Value::from(items.into_iter().map(field_value).collect::<Vec<_>>())
        }
        NoteFieldValue::Object(fields) => fields
            .into_iter()
            .map(|(key, field)| (key, field_value(field)))
            .collect::<Value>(),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use minijinja::Environment;

    use super::*;
    use crate::{
        DialogProvider, PresetDialogProvider, config::SchemaConfigSpec,
    };

    /// Builds a shared [`SchemaService`] for `root`, backing both
    /// [`page_ops`] and [`task_ops`] so both namespaces resolve the same
    /// Schema registry directory (`root/.traces/schemas`), mirroring
    /// [`super::super::TemplateEngine::new`]'s wiring.
    fn schema_service(root: &Path) -> Arc<SchemaService> {
        let (service, _, _) = SchemaService::new(&SchemaConfigSpec::for_test(
            root,
            &root.join(".traces/schemas"),
        ))
        .expect("valid test schema directory");
        Arc::new(service)
    }

    /// Builds a `query` [`QueryOps`] for `root` with the default class field
    /// (`class`) and Schema registry directory (`root/.traces/schemas`).
    fn page_ops(root: &Path) -> QueryOps {
        QueryOps::page(
            Arc::from(root),
            Arc::from("class"),
            schema_service(root),
        )
    }

    /// Builds a `tasks` [`QueryOps`], the [`page_ops`] counterpart.
    fn task_ops(root: &Path) -> QueryOps {
        QueryOps::task(
            Arc::from(root),
            Arc::from("class"),
            schema_service(root),
        )
    }

    /// A minimal [`Environment`] with `query` and `tasks` registered against
    /// `root`, plus the `table`/`list`/`task_list`/`count` pipeline filters.
    fn env(root: &Path) -> Environment<'static> {
        let mut env = Environment::new();
        page_ops(root).register(&mut env);
        task_ops(root).register(&mut env);
        QueryOps::register_terminal_filters(&mut env);
        env
    }

    fn render(root: &Path, source: &str) -> Result<String, Error> {
        env(root).render_str(source, minijinja::context!())
    }

    /// Renders `source` against `root` with the `query`/`tasks` namespaces
    /// bound to a non-default File Class frontmatter field, exercising the
    /// `[schemas] class_field` wiring end-to-end.
    fn render_with_class_field(
        root: &Path,
        class_field: &str,
        source: &str,
    ) -> Result<String, Error> {
        let field: Arc<str> = Arc::from(class_field);
        let service = schema_service(root);
        let mut env = Environment::new();
        QueryOps::page(
            Arc::from(root),
            Arc::clone(&field),
            Arc::clone(&service),
        )
        .register(&mut env);
        QueryOps::task(Arc::from(root), field, service).register(&mut env);
        QueryOps::register_terminal_filters(&mut env);
        env.render_str(source, minijinja::context!())
    }

    mod fixtures {
        use std::{fs, path::Path};

        /// Writes `content` as `name` under `root`.
        pub(super) fn write_note(root: &Path, name: &str, content: &str) {
            fs::write(root.join(name), content).expect("write note");
        }
    }
    use fixtures::write_note;

    mod get_value {
        use super::*;

        #[test]
        fn returns_none_for_an_unknown_key() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let ops = Arc::new(page_ops(temp.path()));

            assert!(ops.get_value(&Value::from("unknown")).is_none());
        }

        #[test]
        fn returns_none_for_a_non_string_key() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let ops = Arc::new(page_ops(temp.path()));

            assert!(ops.get_value(&Value::from(1)).is_none());
        }
    }

    mod enumerate {
        use super::*;

        #[test]
        fn lists_every_method() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let ops = Arc::new(page_ops(temp.path()));

            assert!(matches!(ops.enumerate(), Enumerator::Str(METHODS)));
        }

        #[test]
        fn every_enumerated_method_resolves_via_get_value() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let ops = Arc::new(page_ops(temp.path()));

            for method in METHODS {
                assert!(
                    ops.get_value(&Value::from(*method)).is_some(),
                    "{method:?} is enumerated but get_value has no matching \
                     arm"
                );
            }
        }
    }

    mod register {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn register_makes_query_reachable_through_a_real_environment() {
            let temp = tempfile::tempdir().expect("create temp dir");
            write_note(temp.path(), "a.md", "# A");

            let rendered = render(temp.path(), "{{ query.from() | length }}")
                .expect("render succeeds");

            assert_eq!(rendered, "1");
        }

        #[test]
        fn register_makes_tasks_reachable_through_a_real_environment() {
            let temp = tempfile::tempdir().expect("create temp dir");
            write_note(temp.path(), "todo.md", "- [ ] buy milk\n");

            let rendered = render(temp.path(), "{{ tasks.from() | length }}")
                .expect("render succeeds");

            assert_eq!(rendered, "1");
        }
    }

    mod source_selection {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn all_returns_every_indexed_note() {
            let temp = tempfile::tempdir().expect("create temp dir");
            write_note(temp.path(), "a.md", "# A");
            write_note(temp.path(), "b.md", "# B");

            let rendered = render(temp.path(), "{{ query.from() | length }}")
                .expect("render succeeds");

            assert_eq!(rendered, "2");
        }

        #[test]
        fn from_tags_keeps_only_matching_notes() {
            let temp = tempfile::tempdir().expect("create temp dir");
            write_note(temp.path(), "book.md", "Reading this. #book");
            write_note(temp.path(), "other.md", "Nothing tagged here.");

            let rendered = render(
                temp.path(),
                r##"{% for n in query.from("#book") %}{{ n.file.name }}{% endfor %}"##,
            )
            .expect("render succeeds");

            assert_eq!(rendered, "book");
        }

        #[test]
        fn from_folder_keeps_only_notes_under_the_folder() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::create_dir_all(temp.path().join("books")).expect("mkdir");
            write_note(temp.path(), "books/dune.md", "# Dune");
            write_note(temp.path(), "other.md", "# Other");

            let rendered = render(
                temp.path(),
                r#"{% for n in query.from("books/") %}{{ n.file.name }}{% endfor %}"#,
            )
            .expect("render succeeds");

            assert_eq!(rendered, "dune");
        }

        #[test]
        fn from_tags_keeps_only_matching_notes_tasks() {
            let temp = tempfile::tempdir().expect("create temp dir");
            write_note(temp.path(), "a.md", "#projects\n- [ ] project task\n");
            write_note(temp.path(), "b.md", "#books\n- [ ] book task\n");

            let rendered = render(
                temp.path(),
                r##"{% for t in tasks.from("#projects") %}{{ t.task.text }}{% endfor %}"##,
            )
            .expect("render succeeds");

            assert_eq!(rendered, "project task");
        }

        #[test]
        fn from_folder_keeps_only_notes_under_the_folder_tasks() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::create_dir_all(temp.path().join("projects")).expect("mkdir");
            write_note(temp.path(), "projects/a.md", "- [ ] project task\n");
            write_note(temp.path(), "other.md", "- [ ] other task\n");

            let rendered = render(
                temp.path(),
                r#"{% for t in tasks.from("projects/") %}{{ t.task.text }}{% endfor %}"#,
            )
            .expect("render succeeds");

            assert_eq!(rendered, "project task");
        }
    }

    mod method_chaining {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn where_filters_by_frontmatter_field() {
            let temp = tempfile::tempdir().expect("create temp dir");
            write_note(temp.path(), "high.md", "---\nrating: 9\n---\n#book");
            write_note(temp.path(), "low.md", "---\nrating: 2\n---\n#book");

            let rendered = render(
                temp.path(),
                r##"{% for n in query.from("#book").where("rating > 5") %}{{ n.file.name }}{% endfor %}"##,
            )
            .expect("render succeeds");

            assert_eq!(rendered, "high");
        }

        #[test]
        fn sort_orders_records_and_limit_caps_them() {
            let temp = tempfile::tempdir().expect("create temp dir");
            write_note(temp.path(), "a.md", "---\nrating: 3\n---\n#book");
            write_note(temp.path(), "b.md", "---\nrating: 9\n---\n#book");
            write_note(temp.path(), "c.md", "---\nrating: 6\n---\n#book");

            let rendered = render(
                temp.path(),
                r##"{% for n in query.from("#book").sort("rating", true).limit(2) %}{{ n.file.name }} {% endfor %}"##,
            )
            .expect("render succeeds");

            assert_eq!(rendered, "b c ");
        }

        #[test]
        fn group_by_clusters_equal_values_ascending() {
            let temp = tempfile::tempdir().expect("create temp dir");
            write_note(temp.path(), "a.md", "---\nrating: 9\n---\n#book");
            write_note(temp.path(), "b.md", "---\nrating: 2\n---\n#book");

            let rendered = render(
                temp.path(),
                r##"{% for n in query.from("#book").group_by("rating") %}{{ n.file.name }} {% endfor %}"##,
            )
            .expect("render succeeds");

            assert_eq!(rendered, "b a ");
        }

        #[test]
        fn flatten_explodes_a_list_field_into_one_row_per_element() {
            let temp = tempfile::tempdir().expect("create temp dir");
            write_note(temp.path(), "note.md", "Tagged #a and #b.");

            let rendered = render(
                temp.path(),
                "{{ query.from().flatten(\"tags\") | length }}",
            )
            .expect("render succeeds");

            assert_eq!(rendered, "2");
        }

        #[test]
        fn where_filters_by_task_completion_not_by_note() {
            let temp = tempfile::tempdir().expect("create temp dir");
            write_note(
                temp.path(),
                "todo.md",
                "- [ ] buy milk\n- [x] pay rent\n",
            );

            let rendered = render(
                temp.path(),
                r#"{% for t in tasks.from().where("task.completed == true") %}{{ t.task.text }}{% endfor %}"#,
            )
            .expect("render succeeds");

            // The Note has one complete and one incomplete task: filtering
            // must keep only the matching task row, not both of the one
            // Note that has at least one match.
            assert_eq!(rendered, "pay rent");
        }
    }

    mod terminal_rendering {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn table_call_method_and_filter_forms_render_identically() {
            let temp = tempfile::tempdir().expect("create temp dir");
            write_note(temp.path(), "a.md", "---\nrating: 9\n---");
            write_note(temp.path(), "b.md", "---\nrating: 2\n---");

            let via_method = render(
                temp.path(),
                r#"{{ query.from().sort("file.name", false).table(["Name", "Rating"], ["file.name", "rating"]) }}"#,
            )
            .expect("render succeeds");
            let via_filter = render(
                temp.path(),
                r#"{{ query.from().sort("file.name", false) | table(["Name", "Rating"], ["file.name", "rating"]) }}"#,
            )
            .expect("render succeeds");

            assert_eq!(via_method, via_filter);
            assert_eq!(
                via_method,
                "| Name | Rating |\n|------|--------|\n| a    | 9      |\n| b    | 2      |\n"
            );
        }

        #[test]
        fn list_call_method_and_filter_forms_render_identically() {
            let temp = tempfile::tempdir().expect("create temp dir");
            write_note(temp.path(), "a.md", "---\nrating: 9\n---");
            write_note(temp.path(), "b.md", "---\nrating: 2\n---");

            let via_method = render(
                temp.path(),
                r#"{{ query.from().sort("file.name", false).list("rating") }}"#,
            )
            .expect("render succeeds");
            let via_filter = render(
                temp.path(),
                r#"{{ query.from().sort("file.name", false) | list("rating") }}"#,
            )
            .expect("render succeeds");

            assert_eq!(via_method, via_filter);
            assert_eq!(via_method, "- 9\n- 2\n");
        }

        #[test]
        fn task_list_call_method_and_filter_forms_render_identically() {
            let temp = tempfile::tempdir().expect("create temp dir");
            write_note(
                temp.path(),
                "todo.md",
                "- [ ] buy milk\n- [x] pay rent\n",
            );

            let via_method =
                render(temp.path(), "{{ tasks.from().task_list() }}")
                    .expect("render succeeds");
            let via_filter =
                render(temp.path(), "{{ tasks.from() | task_list }}")
                    .expect("render succeeds");

            assert_eq!(via_method, via_filter);
            assert_eq!(via_method, "- [ ] buy milk\n- [x] pay rent\n");
        }

        #[test]
        fn count_call_method_and_filter_forms_render_identically() {
            let temp = tempfile::tempdir().expect("create temp dir");
            write_note(temp.path(), "a.md", "# A");
            write_note(temp.path(), "b.md", "# B");

            let via_method = render(temp.path(), "{{ query.from().count() }}")
                .expect("render succeeds");
            let via_filter = render(temp.path(), "{{ query.from() | count }}")
                .expect("render succeeds");

            assert_eq!(via_method, via_filter);
            assert_eq!(via_method, "2");
        }

        #[test]
        fn table_takes_empty_lists_as_a_header_with_no_data_columns() {
            let temp = tempfile::tempdir().expect("create temp dir");
            write_note(temp.path(), "note.md", "# Note");

            // Chaining `{% for %}` after a terminal renderer is meaningless;
            // this only proves empty headers/columns lists do not panic.
            let rendered =
                render(temp.path(), "{{ query.from().table([], []) }}")
                    .expect("render succeeds");
            assert_eq!(rendered, "||\n");
        }
    }

    mod for_loop_escape_hatch {
        use pretty_assertions::assert_eq;

        use super::*;

        /// `table`/`list` accept field path strings, not minijinja
        /// expressions (spec: "Terminal table/list helpers accept field path
        /// strings, not arbitrary minijinja expression strings"), so
        /// `file.name | upper` has no terminal-renderer form. A `{% for %}`
        /// loop remains the escape hatch that gives template authors the
        /// full filter pipeline per value.
        #[test]
        fn for_loop_applies_a_minijinja_filter_terminal_renderers_cannot_express()
         {
            let temp = tempfile::tempdir().expect("create temp dir");
            write_note(temp.path(), "note.md", "# Note");

            let rendered = render(
                temp.path(),
                "{% for n in query.from() %}{{ n.file.name | upper }}{% \
                 endfor %}",
            )
            .expect("render succeeds");

            assert_eq!(rendered, "NOTE");
        }
    }

    mod attribute_resolution {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn yields_file_frontmatter_inline_field_and_tags_attributes() {
            let temp = tempfile::tempdir().expect("create temp dir");
            write_note(
                temp.path(),
                "note.md",
                "---\nrating: 9\n---\nStatus:: Draft\n\nTracked with #book.\n",
            );

            let rendered = render(
                temp.path(),
                "{% for n in query.from() %}{{ n.file.name }}|{{ n.rating \
                 }}|{{ n.Status }}|{{ n.tags | length }}{% endfor %}",
            )
            .expect("render succeeds");

            assert_eq!(rendered, "note|9.0|Draft|1");
        }

        #[test]
        fn indexes_by_integer_position() {
            let temp = tempfile::tempdir().expect("create temp dir");
            write_note(temp.path(), "only.md", "# Only");

            let rendered =
                render(temp.path(), "{{ query.from()[0].file.name }}")
                    .expect("render succeeds");

            assert_eq!(rendered, "only");
        }

        #[test]
        fn missing_frontmatter_field_resolves_to_none_not_undefined() {
            let temp = tempfile::tempdir().expect("create temp dir");
            write_note(temp.path(), "note.md", "# No frontmatter");

            let rendered = render(
                temp.path(),
                "{{ query.from()[0].bogus_field is none }}",
            )
            .expect("render succeeds");

            assert_eq!(rendered, "true");
        }

        #[test]
        fn file_enumerates_every_name_from_file_field() {
            let temp = tempfile::tempdir().expect("create temp dir");
            write_note(temp.path(), "only.md", "# Only");

            let rendered = render(
                temp.path(),
                "{% for key in query.from()[0].file %}{{ key }},{% endfor %}",
            )
            .expect("render succeeds");

            let expected = format!("{},", FileField::ACCESSOR_NAMES.join(","));
            assert_eq!(rendered, expected);
        }

        #[test]
        fn task_completed_and_task_text_resolve_per_row() {
            let temp = tempfile::tempdir().expect("create temp dir");
            write_note(
                temp.path(),
                "todo.md",
                "- [ ] buy milk\n- [x] pay rent\n",
            );

            let rendered = render(
                temp.path(),
                "{% for t in tasks.from() %}{{ t.task.completed }}:{{ \
                 t.task.text }} {% endfor %}",
            )
            .expect("render succeeds");

            assert_eq!(rendered, "false:buy milk true:pay rent ");
        }

        #[test]
        fn task_rows_retain_parent_note_metadata_for_filtering_and_display() {
            let temp = tempfile::tempdir().expect("create temp dir");
            write_note(
                temp.path(),
                "project.md",
                "---\ntitle: Launch\n---\nFiled under #projects.\n\n- [ ] \
                 ship it\n",
            );

            let rendered = render(
                temp.path(),
                "{% for t in tasks.from() %}{{ t.file.name }}|{{ t.title \
                 }}|{{ t.tags | length }}{% endfor %}",
            )
            .expect("render succeeds");

            assert_eq!(rendered, "project|Launch|1");
        }

        #[test]
        fn task_completed_and_task_text_are_none_on_a_page_level_record() {
            let temp = tempfile::tempdir().expect("create temp dir");
            write_note(temp.path(), "note.md", "# No tasks here");

            let rendered = render(
                temp.path(),
                "{{ query.from()[0].task.completed is none }}:{{ \
                 query.from()[0].task.text is none }}",
            )
            .expect("render succeeds");

            assert_eq!(rendered, "true:true");
        }
    }

    mod ui_select_integration {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn a_query_outcome_can_be_passed_to_ui_select_with_attribute() {
            let temp = tempfile::tempdir().expect("create temp dir");
            write_note(temp.path(), "a.md", "# A");
            write_note(temp.path(), "b.md", "# B");

            let mut env = env(temp.path());
            let provider: Arc<dyn DialogProvider> =
                Arc::new(PresetDialogProvider::new().with_select(1));
            crate::template::engine::ui::UiOps::new(provider)
                .register(&mut env);

            let rendered = env
                .render_str(
                    r#"{% set picked = ui.select("pick", query.from(), attribute="file.name") %}{{ picked.file.name }}"#,
                    minijinja::context!(),
                )
                .expect("render succeeds");

            assert_eq!(rendered, "b");
        }

        #[test]
        fn a_query_outcome_can_be_passed_to_ui_multi_select_with_attribute() {
            let temp = tempfile::tempdir().expect("create temp dir");
            write_note(temp.path(), "a.md", "# A");
            write_note(temp.path(), "b.md", "# B");

            let mut env = env(temp.path());
            let provider: Arc<dyn DialogProvider> = Arc::new(
                PresetDialogProvider::new().with_multi_select(vec![0, 1]),
            );
            crate::template::engine::ui::UiOps::new(provider)
                .register(&mut env);

            let rendered = env
                .render_str(
                    r#"{% set picked = ui.multi_select("pick", query.from(), attribute="file.name") %}{{ picked | map(attribute="file.name") | join(",") }}"#,
                    minijinja::context!(),
                )
                .expect("render succeeds");

            assert_eq!(rendered, "a,b");
        }
    }

    mod errors {
        use super::*;

        #[test]
        fn unparsable_filter_expression_surfaces_as_a_render_error_not_a_panic()
        {
            let temp = tempfile::tempdir().expect("create temp dir");
            write_note(temp.path(), "note.md", "# Note");

            let error =
                render(temp.path(), r#"{{ query.from().filter("rating >") }}"#)
                    .expect_err("malformed filter expression should error");

            assert!(error.to_string().contains("query failed"));
        }

        #[test]
        fn unknown_field_path_in_sort_surfaces_as_a_render_error() {
            let temp = tempfile::tempdir().expect("create temp dir");
            write_note(temp.path(), "note.md", "# Note");

            let error = render(
                temp.path(),
                r#"{{ query.from().sort("a.b.c", true) }}"#,
            )
            .expect_err("malformed field path should error");

            assert!(error.to_string().contains("query failed"));
        }

        #[test]
        fn negative_limit_surfaces_as_a_render_error() {
            let temp = tempfile::tempdir().expect("create temp dir");
            write_note(temp.path(), "note.md", "# Note");

            let error = render(temp.path(), "{{ query.from().limit(-1) }}")
                .expect_err("negative limit should error");

            assert!(error.to_string().contains("query failed"));
        }

        #[test]
        fn unparsable_filter_expression_on_tasks_surfaces_as_a_render_error() {
            let temp = tempfile::tempdir().expect("create temp dir");
            write_note(temp.path(), "todo.md", "- [ ] buy milk\n");

            let error = render(
                temp.path(),
                r#"{{ tasks.from().filter("task.completed >") }}"#,
            )
            .expect_err("malformed filter expression should error");

            assert!(error.to_string().contains("query failed"));
        }

        #[test]
        fn task_list_on_page_level_records_surfaces_as_a_render_error() {
            let temp = tempfile::tempdir().expect("create temp dir");
            write_note(temp.path(), "note.md", "# Note");

            let error = render(temp.path(), "{{ query.from().task_list() }}")
                .expect_err("task_list on page-level records should error");

            assert!(error.to_string().contains("query failed"));
        }

        #[test]
        fn table_headers_columns_length_mismatch_surfaces_as_a_render_error() {
            let temp = tempfile::tempdir().expect("create temp dir");
            write_note(temp.path(), "note.md", "# Note");

            let error = render(
                temp.path(),
                r#"{{ query.from().table(["Name", "Rating"], ["file.name"]) }}"#,
            )
            .expect_err("mismatched headers/columns length should error");

            assert!(error.to_string().contains("query failed"));
        }

        #[cfg(unix)]
        #[test]
        fn refresh_failure_surfaces_as_a_render_error_not_a_panic() {
            use std::{os::unix::fs::PermissionsExt as _, path::Path};

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

            let error = render(temp.path(), "{{ query.from() | length }}")
                .expect_err("unreadable subdirectory should fail the refresh");

            assert!(error.to_string().contains("failed to refresh"));
        }
    }

    mod refresh {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn each_query_reflects_the_current_filesystem_state() {
            let temp = tempfile::tempdir().expect("create temp dir");
            write_note(temp.path(), "a.md", "# A");
            let e = env(temp.path());

            let first = e
                .render_str(
                    "{{ query.from() | length }}",
                    minijinja::context!(),
                )
                .expect("render succeeds");
            assert_eq!(first, "1");

            write_note(temp.path(), "b.md", "# B");
            let second = e
                .render_str(
                    "{{ query.from() | length }}",
                    minijinja::context!(),
                )
                .expect("render succeeds");
            assert_eq!(second, "2");
        }
    }

    mod caching {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn tasks_reuses_the_index_query_cached_in_the_same_render() {
            let temp = tempfile::tempdir().expect("create temp dir");
            write_note(temp.path(), "a.md", "# A");
            let page_ops = Arc::new(page_ops(temp.path()));
            let task_ops = Arc::new(task_ops(temp.path()));
            let page_from = page_ops
                .get_value(&Value::from("from"))
                .expect("from is a known method");
            let task_from = task_ops
                .get_value(&Value::from("from"))
                .expect("from is a known method");
            let env = Environment::new();
            let state = env.empty_state();

            // Populates state's cached FileIndex; both namespaces dispatch
            // through the same INDEX_CACHE_KEY.
            page_from.call(&state, &[]).expect("query.from succeeds");
            // Written after the index was cached: a cache-sharing tasks.from()
            // call must not observe this new task.
            write_note(temp.path(), "todo.md", "- [ ] buy milk\n");
            let tasks =
                task_from.call(&state, &[]).expect("tasks.from succeeds");

            let count = tasks
                .downcast_object_ref::<QueryOutcome>()
                .expect("value wraps a QueryOutcome")
                .len();
            assert_eq!(count, 0);
        }
    }

    mod task_expansion {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn expands_one_note_with_two_tasks_into_two_rows_not_one() {
            let temp = tempfile::tempdir().expect("create temp dir");
            write_note(
                temp.path(),
                "todo.md",
                "- [ ] buy milk\n- [x] pay rent\n",
            );

            // A page-level query over the same Note returns exactly one
            // row; the task-level query must not collapse back to it.
            let pages = render(temp.path(), "{{ query.from() | length }}")
                .expect("render succeeds");
            let tasks = render(temp.path(), "{{ tasks.from() | length }}")
                .expect("render succeeds");

            assert_eq!(pages, "1");
            assert_eq!(tasks, "2");
        }
    }

    mod class_sources {
        use pretty_assertions::assert_eq;

        use super::*;

        /// Writes `toml` as `<name>.toml` under `root`'s Schema registry
        /// directory, creating it if needed.
        fn write_schema(root: &Path, name: &str, toml: &str) {
            let dir = root.join(".traces/schemas");
            fs::create_dir_all(&dir).expect("create schema dir");
            fs::write(dir.join(format!("{name}.toml")), toml)
                .expect("write schema");
        }

        /// Writes a Note carrying `frontmatter` between YAML delimiters.
        fn write_class_note(root: &Path, name: &str, frontmatter: &str) {
            write_note(
                root,
                name,
                &format!("---\n{frontmatter}\n---\n# {name}"),
            );
        }

        #[test]
        fn selects_notes_of_a_single_class() {
            let temp = tempfile::tempdir().expect("create temp dir");
            write_schema(temp.path(), "book", "");
            write_class_note(temp.path(), "dune.md", "class: book");
            write_class_note(temp.path(), "diary.md", "class: journal");

            let rendered =
                render(temp.path(), r#"{{ query.from("@book") | length }}"#)
                    .expect("render succeeds");

            assert_eq!(rendered, "1");
        }

        #[test]
        fn matches_any_of_several_classes() {
            let temp = tempfile::tempdir().expect("create temp dir");
            write_schema(temp.path(), "book", "");
            write_schema(temp.path(), "movie", "");
            write_class_note(temp.path(), "dune.md", "class: book");
            write_class_note(temp.path(), "alien.md", "class: movie");
            write_class_note(temp.path(), "diary.md", "class: journal");

            let rendered = render(
                temp.path(),
                r#"{{ query.from("@book or @movie") | length }}"#,
            )
            .expect("render succeeds");

            assert_eq!(rendered, "2");
        }

        #[test]
        fn matches_a_subclass_transitively() {
            let temp = tempfile::tempdir().expect("create temp dir");
            write_schema(temp.path(), "book", "");
            write_schema(temp.path(), "sci_fi", "extends = [\"book\"]\n");
            write_class_note(temp.path(), "dune.md", "class: sci_fi");

            let rendered =
                render(temp.path(), r#"{{ query.from("@book*") | length }}"#)
                    .expect("render succeeds");

            assert_eq!(rendered, "1");
        }

        #[test]
        fn degrades_to_exact_match_when_the_class_has_no_schema() {
            let temp = tempfile::tempdir().expect("create temp dir");
            // No Schema registry: `book` cannot resolve subclasses, so only
            // a Note whose class is literally `book` matches.
            write_class_note(temp.path(), "dune.md", "class: book");
            write_class_note(temp.path(), "unknown.md", "class: sci_fi");

            let rendered =
                render(temp.path(), r#"{{ query.from("@book*") | length }}"#)
                    .expect("render succeeds");

            assert_eq!(rendered, "1");
        }

        #[test]
        fn tasks_from_class_source_selects_task_rows() {
            let temp = tempfile::tempdir().expect("create temp dir");
            write_schema(temp.path(), "book", "");
            write_note(
                temp.path(),
                "dune.md",
                "---\nclass: book\n---\n# Dune\n- [ ] read part two\n",
            );

            let rendered =
                render(temp.path(), r#"{{ tasks.from("@book") | length }}"#)
                    .expect("render succeeds");

            assert_eq!(rendered, "1");
        }

        #[test]
        fn reads_the_file_class_from_the_configured_field() {
            let temp = tempfile::tempdir().expect("create temp dir");
            write_schema(temp.path(), "book", "");
            write_class_note(temp.path(), "dune.md", "kind: book");

            let configured = render_with_class_field(
                temp.path(),
                "kind",
                r#"{{ query.from("@book") | length }}"#,
            )
            .expect("render succeeds");
            // Under the default `class` field the `kind: book` Note carries
            // no File Class, so the same query must select nothing.
            let default =
                render(temp.path(), r#"{{ query.from("@book") | length }}"#)
                    .expect("render succeeds");

            assert_eq!(configured, "1");
            assert_eq!(default, "0");
        }

        #[test]
        fn rejects_a_non_string_source_argument() {
            let temp = tempfile::tempdir().expect("create temp dir");
            write_class_note(temp.path(), "dune.md", "class: book");

            let error = render(temp.path(), "{{ query.from(5) | length }}")
                .expect_err("a non-string source argument is rejected");

            assert_eq!(error.kind(), ErrorKind::InvalidOperation);
        }

        #[test]
        fn rejects_a_non_string_source_list() {
            let temp = tempfile::tempdir().expect("create temp dir");
            write_class_note(temp.path(), "dune.md", "class: book");

            let error = render(temp.path(), "{{ query.from([5]) | length }}")
                .expect_err("a source list is rejected");

            assert_eq!(error.kind(), ErrorKind::InvalidOperation);
        }
    }
}
