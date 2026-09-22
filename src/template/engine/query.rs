//! Register the `query`, `lists`, and `tasks` namespaces for templates.
//!
//! All three namespaces are backed by [`QueryOps`], registered by
//! [`super::TemplateEngine::new`]:
//! - [`QueryOps::page`] creates the `query` global
//! - [`QueryOps::list`] creates the `lists` global
//! - [`QueryOps::task`] creates the `tasks` global.
//!
//! Each namespace starts a query with `.from([expr])`, whose expression
//! forms mirror [`SourceSelector`]'s variants:
//!
//! - `.from()`: every indexed Note.
//! - `.from("#tag")`: Notes with an exact or nested tag.
//! - `.from("folder/")`: Notes under a folder.
//! - `.from("@Class*")`: Notes whose File Class is `Class` or a transitive
//!   descendant.
//!
//! Each call reuses the render's cached [`WorkspaceIndex`], refreshing it once
//! per render (see [`QueryOps::cached_index`]), and returns a [`QuerySet`]
//! wrapped in a [`Value`].
//!
//! # Row Shape
//!
//! `query` returns one row per Note. `lists` returns one row per list item
//! (plain bullets, checkboxes, and tasks). `tasks` returns one row per task
//! item. Universal and task-specific properties are exposed via the canonical
//! `record.list.<field>` namespace wrapped by [`ListFields`], alongside the
//! parent Note's `file.*`, frontmatter, inline-field, and tag metadata.
//!
//! # Chaining and Terminal Methods
//!
//! Template callers chain `.where(...)`/`.filter(...)`, `.sort(...)`,
//! `.limit(...)`, `.group_by(...)`, and `.flatten(...)`. The transformation
//! logic lives on [`QuerySet`] itself; this module only supplies the minijinja
//! [`Object`] wiring.
//!
//! [`QuerySet::table`], [`QuerySet::list`], [`QuerySet::task_list`], and
//! `count` (an alias for [`QuerySet::len`]) are terminal instead: they render
//! final markdown/scalar output and end a chain rather than continue it. Each
//! is reachable both as a `call_method` (`outcome.table(["Name"],
//! ["file.name"])`) and as a pipeline filter, registered once by
//! [`QueryOps::register_terminal_filters`] (`outcome | table(["Name"],
//! ["file.name"])`). Both forms call the same [`QuerySet`] method.
//!
//! # Object Wiring
//!
//! [`QuerySet`] and [`QueryRow`] get their [`Object`] impls here instead of in
//! [`crate::index`], keeping that module independent from minijinja so `traces
//! task` can reuse [`WorkspaceIndex`], [`QuerySet`], and [`QueryRow`] without
//! pulling in rendering concerns.
//!
//! `record` attributes other than `file` and `list` forward to
//! [`QueryRow::field`], the same resolver `.where()` and `.sort()` use.
//! `record.file.*` and `record.list.*` use forwarding wrappers ([`FileFields`]
//! and [`ListFields`]) instead: minijinja resolves a dotted attribute path one
//! segment at a time, so the wrappers call [`FileField::parse`] and
//! [`ListField::parse`] directly, skipping the string-prefix handling
//! [`QueryRow::field`] needs once the `file`/`list` segment is already known.
//!
//! # Errors
//!
//! [`IndexerService::refresh`] and [`QueryError`] failures become
//! [`minijinja::Error`] values with stable messages and the original error
//! preserved as [`std::error::Error::source`], mirroring [`super::ui`]'s
//! `dialog_error` and [`super::error::confine_error`]. Query failures carry
//! template name, line, and column context like every other namespace.

use std::{cmp::Ordering, collections::BTreeSet, path::Path, sync::Arc};

use minijinja::{
    Environment, Error, ErrorKind, State,
    value::{DynObject, Enumerator, Object, ObjectRepr, Value, from_args},
};

use super::error::TemplateEngineResult;
use crate::{
    NoteFieldValue,
    index::{IndexerService, WorkspaceIndex},
    query::{
        ClassExpansionMode, FieldPath, FileField, ListField, QueryBuilder,
        QueryError, QueryMode, QueryRow, QueryService, QuerySet, SortDirection,
        SourceAtom, SourceSelector, TaskPathStyle,
    },
    schema::SchemaService,
};

/// Method names `query`, `lists`, and `tasks` each expose, for
/// [`QueryOps::enumerate`].
const METHODS: &[&str] = &["from"];

/// The [`State::set_temp`] key used to cache one refreshed [`WorkspaceIndex`]
/// for the current render.
///
/// Shared by the `query`, `lists`, and `tasks` namespaces (all dispatch
/// through [`QueryOps::run`]) so a render calling into multiple pays for one
/// [`IndexerService::refresh`] instead of one per query call. `State`'s temp
/// storage is scoped to one render, including `{% include %}`s, and resets for
/// the next. A cache field on [`QueryOps`] itself would wrongly persist across
/// independent renders on a reused [`Environment`]/[`super::TemplateEngine`].
const INDEX_CACHE_KEY: &str = "query.index_cache";

/// Backs the `query`, `lists`, and `tasks` minijinja namespace objects: one
/// instance per namespace, differing only in which global it registers as and
/// which [`QueryBuilder`] mode it executes.
#[derive(Debug)]
pub(super) struct QueryOps {
    /// The minijinja global this instance registers as.
    name: &'static str,
    root: Arc<Path>,
    /// Pre-configured once at construction instead of being rebuilt on every
    /// `.from()` call.
    service: QueryService,
    /// Row granularity this namespace dispatches to [`QueryBuilder`].
    mode: QueryMode,
}

impl QueryOps {
    /// Wires the shared pipeline every namespace dispatches through: one
    /// [`QueryService`] pre-configured with `class_field` and the File Class
    /// `schema` expander, for registration as the `name` global at `mode`'s
    /// row granularity.
    fn new(
        name: &'static str,
        mode: QueryMode,
        root: Arc<Path>,
        class_field: &str,
        schema: Arc<SchemaService>,
    ) -> Self {
        Self {
            name,
            root,
            service: QueryService::new(class_field).with_class_expander(schema),
            mode,
        }
    }

    /// Wraps `root` for page-level dispatch under the `query` global.
    #[inline]
    #[must_use]
    pub(super) fn page(
        root: Arc<Path>,
        class_field: &str,
        schema: Arc<SchemaService>,
    ) -> Self {
        Self::new("query", QueryMode::Pages, root, class_field, schema)
    }

    /// Wraps `root` for list-level dispatch under the `lists` global. Each row
    /// is one list item (plain bullets, checkboxes, and tasks) instead of one
    /// Note.
    #[inline]
    #[must_use]
    pub(super) fn list(
        root: Arc<Path>,
        class_field: &str,
        schema: Arc<SchemaService>,
    ) -> Self {
        Self::new("lists", QueryMode::Lists, root, class_field, schema)
    }

    /// Wraps `root` for task-level dispatch under the `tasks` global. Each row
    /// is one task item instead of one Note; see the module docs.
    #[inline]
    #[must_use]
    pub(super) fn task(
        root: Arc<Path>,
        class_field: &str,
        schema: Arc<SchemaService>,
    ) -> Self {
        Self::new("tasks", QueryMode::Tasks, root, class_field, schema)
    }

    /// Registers this object as its `name` global (`query`, `lists`, or
    /// `tasks`).
    #[inline]
    pub(super) fn register(self, env: &mut Environment<'static>) {
        let name = self.name;
        env.add_global(name, Value::from_object(self));
    }

    /// Registers `table`, `list`, `task_list`, and `count` as pipeline filters:
    /// `outcome | table(["Name"], ["file.name"])`, mirroring the call-method
    /// form `outcome.table(["Name"], ["file.name"])` documented on
    /// [`Object::call_method`] for [`QuerySet`]. Registered once, not per
    /// instance: these filters carry no state and apply to any [`QuerySet`]
    /// regardless of which namespace produced it.
    pub(super) fn register_terminal_filters(env: &mut Environment<'static>) {
        env.add_filter("table", table_filter);
        env.add_filter("list", list_filter);
        env.add_filter("task_list", task_list_filter);
        env.add_filter("count", count_filter);
        env.add_filter("with_children", with_children_filter);
        env.add_filter("with_descendants", with_descendants_filter);
    }

    /// Runs this namespace's query method for `source` against `state`'s
    /// cached [`WorkspaceIndex`], refreshing it first if not already cached
    /// this render. See [`INDEX_CACHE_KEY`].
    ///
    /// # Errors
    ///
    /// - [`ErrorKind::InvalidOperation`] if refreshing the index fails, as
    ///   documented on [`Self::cached_index`].
    fn run(
        &self,
        state: &State,
        source: SourceSelector,
    ) -> TemplateEngineResult<Value> {
        let index = self.cached_index(state)?;
        let builder = QueryBuilder::from_mode(self.mode, source);
        Ok(Value::from_object(self.service.run(&index, builder)))
    }

    /// Returns this render's cached [`WorkspaceIndex`] for `self.root`,
    /// refreshing and caching it first if not already cached this render.
    /// See [`INDEX_CACHE_KEY`] and [`super::cache::cached`].
    ///
    /// # Errors
    ///
    /// - [`ErrorKind::InvalidOperation`] if refreshing the index fails,
    ///   including I/O errors while scanning `root`, database access errors,
    ///   and TOML (de)serialization errors on stored records. The original
    ///   error is preserved as [`source`], matching [`super::ui`]'s
    ///   `dialog_error`.
    ///
    /// [`source`]: std::error::Error::source
    fn cached_index(
        &self,
        state: &State,
    ) -> TemplateEngineResult<Arc<WorkspaceIndex>> {
        super::cache::cached(state, INDEX_CACHE_KEY, || {
            IndexerService::new(self.root.as_ref())
                .refresh()
                .map(Arc::new)
                .map_err(|source| {
                    super::error::invalid_operation(
                        "failed to refresh the file index",
                        source,
                    )
                })
        })
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
                          -> TemplateEngineResult<Value> {
                        let source = resolve_from_arg(expr.as_ref())?;
                        ops.run(state, source)
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

/// Maps a [`QueryError`] into a [`minijinja::Error`].
///
/// Keeps the original error as [`source`].
///
/// [`source`]: std::error::Error::source
fn query_error(source: QueryError) -> Error {
    super::error::invalid_operation("query failed", source)
}

/// Resolves `.from()`'s optional argument into a [`SourceSelector`]: `None`
/// selects every indexed file; a bound `SourceSelector` `Value` (a Schema
/// `file` field, possibly widened by `with_children`/`with_descendants`) is
/// used as-is; any other value must be a DSL source-expression string.
///
/// # Errors
///
/// - [`ErrorKind::InvalidOperation`] if `expr` is neither a `SourceSelector`
///   nor a string.
/// - Propagates [`QueryError::Syntax`] (via [`query_error`]) if `expr` is a
///   string that fails to parse as a source expression.
fn resolve_from_arg(
    expr: Option<&Value>,
) -> TemplateEngineResult<SourceSelector> {
    let Some(value) = expr else {
        return Ok(SourceSelector::All);
    };
    if let Some(source) = value.downcast_object_ref::<SourceSelector>() {
        return Ok(source.clone());
    }
    let text = value.as_str().ok_or_else(|| {
        Error::new(
            ErrorKind::InvalidOperation,
            "from() expects a source expression string or a Schema file field",
        )
    })?;
    SourceSelector::parse(text).map_err(query_error)
}

/// Lets `.field()` hand a [`SourceSelector`] filter across the minijinja
/// boundary: `.from()` and the `with_children`/`with_descendants` filters
/// downcast it back via [`Value::downcast_object_ref`]. No method overrides;
/// mirrors `cache.rs`'s `Cached<T>`, this crate's other bare `impl Object` used
/// purely to smuggle a typed value through a `Value`.
impl Object for SourceSelector {}

impl Object for QuerySet {
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

    /// Dispatches [`QuerySet`] methods by template name.
    ///
    /// The terminal methods `table`, `list`, `task_list`, and `count` render
    /// final output and return early, without touching the non-terminal chain
    /// below:
    ///
    /// - `table(headers, columns)` and `list(path)` render field path strings
    ///   (or, for `table`'s `headers`, display labels), not further
    ///   [`QuerySet`] arguments.
    /// - `task_list()` takes no arguments.
    /// - `count()` takes no arguments and returns [`QuerySet::len`] directly;
    ///   it cannot fail, unlike the other three.
    ///
    /// Every other name falls through to the non-terminal chain: `.where`/
    /// `.filter`, `.sort`, `.limit`, `.group_by`, and `.flatten`. Each of those
    /// calls consumes a clone of the current outcome and wraps the transformed
    /// result in a [`Value`] for further chaining:
    ///
    /// - `where` and `filter` both call `QuerySet::filter`. The Rust-side
    ///   `r#where` alias exists only for Rust callers.
    /// - `sort` defaults to `SortDirection::default` (descending) when the
    ///   optional `descending` argument is omitted.
    ///
    /// # Errors
    ///
    /// - [`ErrorKind::UnknownMethod`] for any other method name.
    /// - [`ErrorKind::TooManyArguments`]/[`ErrorKind::MissingArgument`] if a
    ///   method's arguments don't match its expected shape.
    /// - [`ErrorKind::InvalidOperation`] via `query_error` if a field path or
    ///   filter expression is unparsable, `.limit(...)` is negative, or
    ///   `.task_list()` runs on records with no `list.*` fields.
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
    ) -> TemplateEngineResult<Value> {
        match method {
            "table" => {
                let (headers, columns): (Vec<String>, Vec<String>) =
                    from_args(args)?;
                let headers_slice: Vec<&str> =
                    headers.iter().map(String::as_str).collect();
                let columns_slice: Vec<&str> =
                    columns.iter().map(String::as_str).collect();
                return self
                    .table(&headers_slice, &columns_slice)
                    .map(Value::from)
                    .map_err(query_error);
            }
            "list" => {
                let (path,): (&str,) = from_args(args)?;
                return self.list(path).map(Value::from).map_err(query_error);
            }
            "task_list" => {
                from_args::<()>(args)?;
                return self
                    .task_list(TaskPathStyle::default())
                    .map(Value::from)
                    .map_err(query_error);
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
                let (field, descending): (&str, Option<bool>) =
                    from_args(args)?;
                let direction = if descending.unwrap_or(true) {
                    SortDirection::default()
                } else {
                    SortDirection::Ascending
                };
                outcome.sort_field(field, direction)
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

/// `outcome | table(...)` filter body. See [`QuerySet::table`].
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
    outcome: &QuerySet,
    headers: Vec<String>,
    columns: Vec<String>,
) -> TemplateEngineResult<String> {
    let headers_slice: Vec<&str> = headers.iter().map(String::as_str).collect();
    let columns_slice: Vec<&str> = columns.iter().map(String::as_str).collect();
    outcome.table(&headers_slice, &columns_slice).map_err(query_error)
}

/// `outcome | list(path)` filter body. See [`QuerySet::list`].
fn list_filter(outcome: &QuerySet, path: &str) -> TemplateEngineResult<String> {
    outcome.list(path).map_err(query_error)
}

/// `outcome | task_list` filter body. See [`QuerySet::task_list`].
fn task_list_filter(outcome: &QuerySet) -> TemplateEngineResult<String> {
    outcome.task_list(TaskPathStyle::default()).map_err(query_error)
}

/// `outcome | count` filter body: the number of records in `outcome`.
fn count_filter(outcome: &QuerySet) -> usize {
    outcome.len()
}

/// `field | with_children` filter body: widens a `file` field's `Class` atom
/// (if any) to direct-children depth.
fn with_children_filter(source: &SourceSelector) -> Value {
    Value::from_object(set_class_depth(
        source.clone(),
        ClassExpansionMode::Children,
    ))
}

/// `field | with_descendants` filter body: widens a `file` field's `Class` atom
/// (if any) to transitive-descendants depth.
fn with_descendants_filter(source: &SourceSelector) -> Value {
    Value::from_object(set_class_depth(
        source.clone(),
        ClassExpansionMode::Descendants,
    ))
}

/// Replaces every `Class` atom's [`ClassExpansionMode`] in `source` with
/// `mode`, keeping each atom's match set empty: the selector stays
/// unresolved, and `resolve_classes` fills the match sets in at query time.
fn set_class_depth(
    mut source: SourceSelector,
    mode: impl Fn(BTreeSet<String>) -> ClassExpansionMode,
) -> SourceSelector {
    if let SourceSelector::Expr(expr) = &mut source {
        expr.visit_atoms_mut(&mut |atom| {
            if let SourceAtom::Class {
                mode: existing,
                ..
            } = atom
            {
                *existing = mode(BTreeSet::new());
            }
        });
    }
    source
}

impl Object for QueryRow {
    /// Resolves `record.<key>` or `record["<key>"]`.
    ///
    /// `"file"` and `"list"` return forwarding wrappers for `record.file.*` and
    /// `record.list.*`. Every other key resolves through [`QueryRow`]'s field
    /// lookup, the same frontmatter, inline-field, and tag lookup used by
    /// `.where()` and `.sort()`.
    ///
    /// A rejected key, such as a dotted, empty, or unknown `file.*`/`list.*`
    /// accessor, resolves to `None` like any other missing attribute instead of
    /// surfacing `QueryError::FieldPath` as a render error.
    #[inline]
    fn get_value(self: &Arc<Self>, key: &Value) -> Option<Value> {
        let key = key.as_str()?;
        match key {
            "file" => Some(Value::from_object(FileFields(Arc::clone(self)))),
            "list" => Some(Value::from_object(ListFields(Arc::clone(self)))),
            _ => self.field(key).ok().map(field_value),
        }
    }

    #[inline]
    fn custom_cmp(self: &Arc<Self>, other: &DynObject) -> Option<Ordering> {
        other.downcast_ref::<Self>().map(|other| self.cmp_document_order(other))
    }
}

/// Forwards `record.file.<field>` to [`FileField::parse`].
///
/// A thin wrapper rather than a second lookup path, needed only because
/// minijinja resolves a dotted attribute path one segment at a time:
/// `record.file` must itself resolve to *something* before `.name` can be
/// looked up on it. Calls the same [`FileField`] accessor pair
/// [`QueryRow::field`] uses for its `file.*` branch, skipping that method's
/// string-based `file.` prefix handling, which doesn't apply here: `key` is
/// already a single attribute segment, never a dotted path.
#[derive(Debug)]
struct FileFields(Arc<QueryRow>);

impl Object for FileFields {
    fn get_value(self: &Arc<Self>, key: &Value) -> Option<Value> {
        let field = FileField::parse(key.as_str()?)?;
        Some(field_value(self.0.resolve_owned(&FieldPath::File(field))))
    }

    fn enumerate(self: &Arc<Self>) -> Enumerator {
        Enumerator::Str(FileField::ACCESSOR_NAMES)
    }
}

/// Forwards `record.list.<field>` to [`ListField::parse`].
///
/// A thin wrapper rather than a second lookup path, needed only because
/// minijinja resolves a dotted attribute path one segment at a time:
/// `record.list` must itself resolve to *something* before `.<field>` can be
/// looked up on it. Calls the same [`ListField`] accessor pair
/// [`QueryRow::field`] uses for its `list.*` branch, skipping that method's
/// string-based `list.` prefix handling, which doesn't apply here: `key` is
/// already a single attribute segment, never a dotted path.
#[derive(Debug)]
struct ListFields(Arc<QueryRow>);

impl Object for ListFields {
    fn get_value(self: &Arc<Self>, key: &Value) -> Option<Value> {
        let field = ListField::parse(key.as_str()?)?;
        Some(field_value(self.0.resolve_owned(&FieldPath::List(field))))
    }

    fn enumerate(self: &Arc<Self>) -> Enumerator {
        Enumerator::Str(ListField::ACCESSOR_NAMES)
    }
}

/// Converts a resolved [`NoteFieldValue`] into a minijinja [`Value`].
///
/// - [`NoteFieldValue::Null`] becomes minijinja's `none` rather than
///   `undefined`: [`QueryRow`]'s own docs note that a well-formed path with no
///   value resolves to `Null`, not an error. That's a defined empty value, not
///   a missing attribute.
/// - [`NoteFieldValue::Link`] renders as its target path; Traces has no
///   minijinja-facing link type yet.
fn field_value(value: NoteFieldValue) -> Value {
    match value {
        NoteFieldValue::Null => Value::from(()),
        NoteFieldValue::Bool(b) => Value::from(b),
        NoteFieldValue::Number(n) => Value::from(n),
        NoteFieldValue::String(s) => Value::from(s),
        NoteFieldValue::Duration(dv) => Value::from(dv.as_str()),
        NoteFieldValue::Date(value) => Value::from(value.to_date_string()),
        NoteFieldValue::DateTime(value) => {
            Value::from(value.to_datetime_string())
        }
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
    use crate::{DialogProvider, PresetDialogProvider};

    /// Builds a shared [`SchemaService`] for `root`, backing [`page_ops`],
    /// [`list_ops`], and [`task_ops`] so all namespaces resolve the same Schema
    /// registry directory (`root/.traces/schemas`), mirroring
    /// [`super::super::TemplateEngine::new`]'s wiring.
    fn schema_service(root: &Path) -> Arc<SchemaService> {
        Arc::new(
            SchemaService::new(&root.join(".traces/schemas"))
                .expect("valid test schema directory"),
        )
    }

    /// Builds a `query` [`QueryOps`] for `root` with the default class field
    /// (`class`) and Schema registry directory (`root/.traces/schemas`).
    fn page_ops(root: &Path) -> QueryOps {
        QueryOps::page(Arc::from(root), "class", schema_service(root))
    }

    /// Builds a `lists` [`QueryOps`], the [`page_ops`] counterpart.
    fn list_ops(root: &Path) -> QueryOps {
        QueryOps::list(Arc::from(root), "class", schema_service(root))
    }

    /// Builds a `tasks` [`QueryOps`], the [`page_ops`] counterpart.
    fn task_ops(root: &Path) -> QueryOps {
        QueryOps::task(Arc::from(root), "class", schema_service(root))
    }

    /// A minimal [`Environment`] with `query`, `lists`, and `tasks` registered
    /// against `root`, plus terminal query filters.
    fn env(root: &Path) -> Environment<'static> {
        let mut env = Environment::new();
        page_ops(root).register(&mut env);
        list_ops(root).register(&mut env);
        task_ops(root).register(&mut env);
        QueryOps::register_terminal_filters(&mut env);
        env
    }

    fn render(root: &Path, source: &str) -> TemplateEngineResult<String> {
        env(root).render_str(source, minijinja::context!())
    }

    /// Renders `source` against `root` with the `query`/`tasks` namespaces
    /// bound to a non-default File Class frontmatter field, exercising the
    /// `[schemas] class_field` wiring end-to-end.
    fn render_with_class_field(
        root: &Path,
        class_field: &str,
        source: &str,
    ) -> TemplateEngineResult<String> {
        let service = schema_service(root);
        let mut env = Environment::new();
        QueryOps::page(Arc::from(root), class_field, Arc::clone(&service))
            .register(&mut env);
        QueryOps::list(Arc::from(root), class_field, Arc::clone(&service))
            .register(&mut env);
        QueryOps::task(Arc::from(root), class_field, service)
            .register(&mut env);
        QueryOps::register_terminal_filters(&mut env);
        env.render_str(source, minijinja::context!())
    }

    use crate::write_note;

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

        #[test]
        fn register_makes_lists_reachable_through_a_real_environment() {
            let temp = tempfile::tempdir().expect("create temp dir");
            write_note(temp.path(), "notes.md", "- item 1\n- item 2\n");

            let rendered = render(temp.path(), "{{ lists.from() | length }}")
                .expect("render succeeds");

            assert_eq!(rendered, "2");
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
                r##"{% for t in tasks.from("#projects") %}{{ t.list.text }}{% endfor %}"##,
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
                r#"{% for t in tasks.from("projects/") %}{{ t.list.text }}{% endfor %}"#,
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
                r#"{% for t in tasks.from().where("list.completed == true") %}{{ t.list.text }}{% endfor %}"#,
            )
            .expect("render succeeds");

            // The Note has one complete and one incomplete task: filtering
            // must keep only the matching task row, not both of the one
            // Note that has at least one match.
            assert_eq!(rendered, "pay rent");
        }

        #[test]
        fn where_filters_lists_by_is_task() {
            let temp = tempfile::tempdir().expect("create temp dir");
            write_note(temp.path(), "note.md", "- bullet\n- [x] task\n");

            let rendered = render(
                temp.path(),
                r#"{% for l in lists.from().where("list.is_task == false") %}{{ l.list.text }}{% endfor %}"#,
            )
            .expect("render succeeds");

            assert_eq!(rendered, "bullet");
        }

        #[test]
        fn where_filters_tasks_by_status() {
            let temp = tempfile::tempdir().expect("create temp dir");
            write_note(temp.path(), "todo.md", "- [ ] todo\n- [x] done\n");

            let rendered = render(
                temp.path(),
                r#"{% for t in tasks.from().where("list.status == \"Done\"") %}{{ t.list.text }}{% endfor %}"#,
            )
            .expect("render succeeds");

            assert_eq!(rendered, "done");
        }

        #[test]
        fn sort_orders_tasks_by_due_date() {
            let temp = tempfile::tempdir().expect("create temp dir");
            write_note(
                temp.path(),
                "tasks.md",
                "- [ ] later 📅 2026-05-01\n- [ ] earlier 📅 2026-01-01\n",
            );

            let rendered = render(
                temp.path(),
                r#"{% for t in tasks.from().sort("list.due", true) %}{{ t.list.text }} {% endfor %}"#,
            )
            .expect("render succeeds");

            assert_eq!(rendered, "later earlier ");
        }
    }

    mod terminal_rendering {
        use pretty_assertions::assert_eq;
        use rstest::rstest;

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

        #[rstest]
        #[case::true_value("true", "true")]
        #[case::false_value("false", "false")]
        fn list_renders_boolean_field_values_as_true_or_false(
            #[case] frontmatter_value: &str,
            #[case] expected_line: &str,
        ) {
            let temp = tempfile::tempdir().expect("create temp dir");
            write_note(
                temp.path(),
                "note.md",
                &format!("---\nactive: {frontmatter_value}\n---"),
            );

            let rendered =
                render(temp.path(), r#"{{ query.from().list("active") }}"#)
                    .expect("render succeeds");

            assert_eq!(rendered, format!("- {expected_line}\n"));
        }

        #[test]
        fn table_renders_list_and_task_fields() {
            let temp = tempfile::tempdir().expect("create temp dir");
            write_note(temp.path(), "todo.md", "- [ ] task A\n- [x] task B\n");

            let rendered = render(
                temp.path(),
                r#"{{ tasks.from().table(["Task", "Done"], ["list.text", "list.completed"]) }}"#,
            )
            .expect("render succeeds");

            assert_eq!(
                rendered,
                "| Task   | Done  |\n|--------|-------|\n| task A | false \
                 |\n| task B | true  |\n",
            );
        }

        #[test]
        fn list_renders_list_items_text() {
            let temp = tempfile::tempdir().expect("create temp dir");
            write_note(temp.path(), "items.md", "- first\n- second\n");

            let rendered =
                render(temp.path(), r#"{{ lists.from().list("list.text") }}"#)
                    .expect("render succeeds");

            assert_eq!(rendered, "- first\n- second\n");
        }

        #[test]
        fn task_list_renders_matching_tasks_from_filtered_lists_query() {
            let temp = tempfile::tempdir().expect("create temp dir");
            write_note(
                temp.path(),
                "notes.md",
                "- plain bullet\n- [x] done task\n",
            );

            let rendered = render(
                temp.path(),
                r#"{{ lists.from().where("list.is_task == true").task_list() }}"#,
            )
            .expect("render succeeds");

            assert_eq!(rendered, "- [x] done task\n");
        }

        #[test]
        fn count_renders_list_item_count() {
            let temp = tempfile::tempdir().expect("create temp dir");
            write_note(temp.path(), "notes.md", "- a\n- b\n- c\n");

            let rendered = render(temp.path(), "{{ lists.from().count() }}")
                .expect("render succeeds");

            assert_eq!(rendered, "3");
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
        use rstest::rstest;

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

            assert_eq!(rendered, "True");
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
        fn list_completed_and_list_text_resolve_per_row() {
            let temp = tempfile::tempdir().expect("create temp dir");
            write_note(
                temp.path(),
                "todo.md",
                "- [ ] buy milk\n- [x] pay rent\n",
            );

            let rendered = render(
                temp.path(),
                "{% for t in tasks.from() %}{{ t.list.completed }}:{{ \
                 t.list.text }} {% endfor %}",
            )
            .expect("render succeeds");

            assert_eq!(rendered, "False:buy milk True:pay rent ");
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
                 }}|{{ t.file.tags | length }}{% endfor %}",
            )
            .expect("render succeeds");

            assert_eq!(rendered, "project|Launch|1");
        }

        #[test]
        fn list_completed_and_list_text_are_none_on_a_page_level_record() {
            let temp = tempfile::tempdir().expect("create temp dir");
            write_note(temp.path(), "note.md", "# No tasks here");

            let rendered = render(
                temp.path(),
                "{{ query.from()[0].list.completed is none }}:{{ \
                 query.from()[0].list.text is none }}",
            )
            .expect("render succeeds");

            assert_eq!(rendered, "True:True");
        }

        #[rstest]
        #[case::status("status")]
        #[case::status_type("status_type")]
        #[case::status_symbol("status_symbol")]
        #[case::completed("completed")]
        #[case::priority("priority")]
        #[case::due("due")]
        #[case::done("done")]
        #[case::created("created")]
        #[case::start("start")]
        #[case::scheduled("scheduled")]
        #[case::cancelled("cancelled")]
        #[case::fully_complete("fully_complete")]
        fn task_fields_are_none_on_plain_bullets(#[case] field: &str) {
            let temp = tempfile::tempdir().expect("create temp dir");
            write_note(temp.path(), "note.md", "- a plain bullet\n");

            let rendered = render(
                temp.path(),
                &format!("{{{{ lists.from()[0].list.{field} is none }}}}"),
            )
            .expect("render succeeds");

            assert_eq!(rendered, "True");
        }

        #[test]
        fn universal_fields_resolve_correctly_on_plain_bullets() {
            let temp = tempfile::tempdir().expect("create temp dir");
            write_note(
                temp.path(),
                "note.md",
                "1. first ordered item\n  - nested plain #tag1\n",
            );

            let rendered = render(
                temp.path(),
                "{% for item in lists.from() %}{{ item.list.text }}|{{ \
                 item.list.raw_text }}|{{ item.list.depth }}|{{ \
                 item.list.line }}|{{ item.list.parent is none }}|{{ \
                 item.list.is_task }}|{{ item.list.kind }}|{{ \
                 item.list.is_ordered }};{% endfor %}",
            )
            .expect("render succeeds");

            assert_eq!(
                rendered,
                "first ordered item|first ordered \
                 item|0.0|1.0|True|False|plain|True;nested plain #tag1|nested \
                 plain #tag1|0.0|2.0|True|False|plain|False;",
            );
        }

        #[test]
        fn universal_fields_resolve_and_task_fields_are_none_on_checkboxes() {
            let temp = tempfile::tempdir().expect("create temp dir");
            write_note(temp.path(), "note.md", "- [ ] untagged checkbox\n");
            let config = crate::Config::test_default(temp.path().to_path_buf())
                .with_tasks(crate::TaskConfig::from_tags(&["#task"]));
            let index = Arc::new(
                IndexerService::new(temp.path())
                    .with_config(&config)
                    .build()
                    .expect("build index"),
            );
            let row = QueryService::new("class")
                .run(&index, QueryBuilder::lists(SourceSelector::All))
                .into_iter()
                .next()
                .expect("row");

            let rendered = env(temp.path())
                .render_str(
                    "{{ item.list.text }}|{{ item.list.depth }}|{{ \
                     item.list.line }}|{{ item.list.is_task }}|{{ \
                     item.list.kind }}|{{ item.list.due is none }}|{{ \
                     item.list.completed is none }}",
                    minijinja::context! { item => Value::from_object(row) },
                )
                .expect("render succeeds");

            assert_eq!(
                rendered,
                "untagged checkbox|0.0|1.0|False|checkbox|True|True",
            );
        }

        #[test]
        fn universal_and_task_fields_resolve_correctly_on_tasks() {
            let temp = tempfile::tempdir().expect("create temp dir");
            write_note(
                temp.path(),
                "note.md",
                "- [ ] buy milk 📅 2026-05-01 #errand\n",
            );

            let rendered = render(
                temp.path(),
                "{% set t = tasks.from()[0] %}{{ t.list.text }}|{{ \
                 t.list.is_task }}|{{ t.list.kind }}|{{ t.list.status }}|{{ \
                 t.list.status_symbol }}|{{ t.list.status_type }}|{{ \
                 t.list.completed }}|{{ t.list.due }}|{{ t.list.tags[0] }}",
            )
            .expect("render succeeds");

            // The default Todo status symbol is a bare space, hence the
            // empty-looking segment between `Todo|` and `|todo`.
            assert_eq!(
                rendered,
                "buy milk #errand|True|task|Todo| \
                 |todo|False|2026-05-01|#errand",
            );
        }
    }

    mod row_ordering {
        use pretty_assertions::assert_eq;

        use super::*;

        /// `>` and `<` on rows must follow document order: a total order
        /// where each distinct pair compares one way, never both.
        #[test]
        fn row_comparisons_follow_document_order() {
            let temp = tempfile::tempdir().expect("create temp dir");
            write_note(temp.path(), "a.md", "# A");
            write_note(temp.path(), "b.md", "# B");

            let rendered = render(
                temp.path(),
                r"{% set rows = query.from() %}{{ rows[0] > rows[1] }} {{ rows[1] > rows[0] }}",
            )
            .expect("render succeeds");

            assert_eq!(rendered, "False True");
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
                r#"{{ tasks.from().filter("list.completed >") }}"#,
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

            // Populates state's cached WorkspaceIndex; both namespaces dispatch
            // through the same INDEX_CACHE_KEY.
            page_from.call(&state, &[]).expect("query.from succeeds");
            // Written after the index was cached: a cache-sharing tasks.from()
            // call must not observe this new task.
            write_note(temp.path(), "todo.md", "- [ ] buy milk\n");
            let tasks =
                task_from.call(&state, &[]).expect("tasks.from succeeds");

            let count = tasks
                .downcast_object_ref::<QuerySet>()
                .expect("value wraps a QuerySet")
                .len();
            assert_eq!(count, 0);
        }

        #[test]
        fn one_render_can_query_pages_and_tasks_from_one_cached_index() {
            let temp = tempfile::tempdir().expect("create temp dir");
            write_note(
                temp.path(),
                "note.md",
                "---\nrating: 9\n---\n#book\n- [x] done\n",
            );

            let rendered = render(
                temp.path(),
                r##"{{ query.from("#book").list("file.name") }}{{ tasks.from("#book").task_list() }}"##,
            )
            .expect("render succeeds");

            assert_eq!(rendered, "- note\n- [x] done\n");
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
        fn lists_from_class_source_selects_list_rows() {
            let temp = tempfile::tempdir().expect("create temp dir");
            write_schema(temp.path(), "book", "");
            write_note(
                temp.path(),
                "dune.md",
                "---\nclass: book\n---\n# Dune\n- outline point\n- [ ] read \
                 part two\n",
            );

            let rendered =
                render(temp.path(), r#"{{ lists.from("@book") | length }}"#)
                    .expect("render succeeds");

            // Both list items match, where `tasks.from` would yield only the
            // task row.
            assert_eq!(rendered, "2");
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
