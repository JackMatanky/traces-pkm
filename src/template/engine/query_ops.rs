//! The `query` minijinja namespace for querying the [`FileIndex`] from
//! templates.
//!
//! [`QueryOps`] backs the `query` global registered by
//! [`super::TemplateEngine`]. A template starts a page-level query with one of
//! three methods, mirroring [`Source`]'s three variants one-to-one:
//! - `query.all()`: every indexed Note.
//! - `query.from_tags(tag)`: Notes tagged `tag`.
//! - `query.from_folder(folder)`: Notes under `folder`.
//!
//! Each refreshes a fresh [`FileIndex`] for the render's project root and
//! returns a [`QueryOutcome`] wrapped in a [`Value`]. The result chains with
//! `.where(...)`/`.filter(...)`, `.sort(...)`, `.limit(...)`, `.group_by(...)`,
//! and `.flatten(...)`, all implemented on [`QueryOutcome`] itself. This
//! module only adds the minijinja [`Object`] wiring, not the transformation
//! logic.
//!
//! # Why the `Object` impls live here
//!
//! [`QueryOutcome`] and [`IndexRecord`] gain their [`Object`] impls in this
//! module rather than in [`crate::index`], keeping the index module free of
//! any rendering-framework dependency, so a future CLI query command can
//! reuse the same [`FileIndex`]/[`QueryOutcome`]/[`IndexRecord`] types
//! without pulling minijinja along.
//!
//! [`IndexRecord`] attribute resolution (`record.rating`, `record.file.name`)
//! forwards to [`IndexRecord::field`], the same field resolver
//! `.where()`/`.sort()` use, rather than a second lookup path. Only
//! `record.file.*` needs a forwarding [`FileFields`] wrapper, since minijinja
//! resolves a dotted attribute path one segment at a time.
//!
//! # Error handling
//!
//! Every [`FileIndex::refresh`]/[`QueryError`] failure surfaces as a
//! [`minijinja::Error`] with a stable message and the original error
//! preserved as [`std::error::Error::source`]. This mirrors the conversion
//! pattern [`super::ui_ops`]'s `dialog_error` and [`super::file_ops`]'s
//! `confine_error` use, so query failures carry template name/line/column
//! like every other namespace's errors.

use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use minijinja::{
    Environment, Error, ErrorKind, State,
    value::{Enumerator, Object, ObjectRepr, Value, from_args},
};

use crate::{
    index::{
        FileIndex, FileIndexError, FileRecord, IndexRecord, QueryError,
        QueryOutcome, Source,
    },
    note::FieldValue,
};

/// Method names `query` exposes, for [`QueryOps::enumerate`].
const METHODS: &[&str] = &["all", "from_tags", "from_folder"];

/// Backs the `query` minijinja namespace object.
///
/// Holds the trusted project root that every method refreshes a fresh
/// [`FileIndex`] against before running its query. See
/// [`super::TemplateEngine::new`] for where `root` comes from.
#[derive(Debug)]
pub(super) struct QueryOps {
    root: Arc<Path>,
}

impl QueryOps {
    /// Wraps `root` for template-facing dispatch.
    #[inline]
    #[must_use]
    pub(super) const fn new(root: Arc<Path>) -> Self {
        Self {
            root,
        }
    }

    /// Registers this object as the `query` global.
    #[inline]
    pub(super) fn register(self, env: &mut Environment<'static>) {
        env.add_global("query", Value::from_object(self));
    }
}

impl Object for QueryOps {
    fn get_value(self: &Arc<Self>, key: &Value) -> Option<Value> {
        match key.as_str()? {
            "all" => {
                let root = Arc::clone(&self.root);
                Some(Value::from_function(move || -> Result<Value, Error> {
                    query(&root, &Source::All)
                }))
            }
            "from_tags" => {
                let root = Arc::clone(&self.root);
                Some(Value::from_function(
                    move |tag: &str| -> Result<Value, Error> {
                        query(&root, &Source::Tag(tag.to_owned()))
                    },
                ))
            }
            "from_folder" => {
                let root = Arc::clone(&self.root);
                Some(Value::from_function(
                    move |folder: &str| -> Result<Value, Error> {
                        query(&root, &Source::Folder(PathBuf::from(folder)))
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

/// Refreshes the [`FileIndex`] for `root`, then runs `source` against it.
///
/// Shared by every `query.*` method.
///
/// # Errors
///
/// [`ErrorKind::InvalidOperation`] (via [`index_error`]) if refreshing the
/// index fails: an I/O error scanning `root`, a redb error accessing the
/// index database, or a TOML (de)serialization error on a stored record.
fn query(root: &Path, source: &Source) -> Result<Value, Error> {
    let index = FileIndex::refresh(root).map_err(index_error)?;
    Ok(Value::from_object(index.query(source)))
}

/// Maps a [`FileIndexError`] into a [`minijinja::Error`].
///
/// Keeps the original as [`source`](std::error::Error::source), mirroring
/// [`super::ui_ops`]'s `dialog_error`.
fn index_error(source: FileIndexError) -> Error {
    Error::new(ErrorKind::InvalidOperation, "failed to refresh the file index")
        .with_source(source)
}

/// Maps a [`QueryError`] into a [`minijinja::Error`].
///
/// Mirrors [`index_error`].
fn query_error(source: QueryError) -> Error {
    Error::new(ErrorKind::InvalidOperation, "query failed").with_source(source)
}

impl Object for QueryOutcome {
    fn repr(self: &Arc<Self>) -> ObjectRepr {
        ObjectRepr::Seq
    }

    fn get_value(self: &Arc<Self>, key: &Value) -> Option<Value> {
        self.get(key.as_usize()?).cloned().map(Value::from_object)
    }

    fn enumerate(self: &Arc<Self>) -> Enumerator {
        Enumerator::Seq(self.len())
    }

    /// Dispatches every [`QueryOutcome`] transformation method by name.
    ///
    /// Handles `.where`/`.filter`, `.sort`, `.limit`, `.group_by`, and
    /// `.flatten`. Each consumes a clone of the current outcome and wraps the
    /// result back into a [`Value`] for further chaining:
    /// - `where`/`filter` both call [`QueryOutcome::filter`] directly; the
    ///   Rust-side `r#where` alias exists only for Rust callers, not template
    ///   ones.
    /// - `sort`'s `descending` argument defaults to `false` (ascending) when
    ///   omitted.
    ///
    /// # Errors
    ///
    /// - [`ErrorKind::UnknownMethod`] for any other method name.
    /// - [`ErrorKind::InvalidOperation`] (via [`query_error`]) if the field
    ///   path or filter expression is unparsable, or `.limit(...)` is negative.
    fn call_method(
        self: &Arc<Self>,
        _state: &State<'_, '_>,
        method: &str,
        args: &[Value],
    ) -> Result<Value, Error> {
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

impl Object for IndexRecord {
    /// Resolves `record.<key>` (or `record["<key>"]`).
    ///
    /// `"file"` returns a [`FileFields`] forwarding wrapper for
    /// `record.file.*`; every other key resolves through
    /// [`IndexRecord::field`], the same frontmatter/inline-field/tags lookup
    /// `.where()`/`.sort()` use. A `key` that `field` rejects (dotted, empty,
    /// or an unknown `file.*` accessor) resolves to `None` here, the same as
    /// any other missing attribute, rather than surfacing
    /// [`QueryError::UnknownFieldPath`] as a render error.
    fn get_value(self: &Arc<Self>, key: &Value) -> Option<Value> {
        let key = key.as_str()?;
        if key == "file" {
            return Some(Value::from_object(FileFields(Arc::clone(self))));
        }
        self.field(key).ok().map(field_value)
    }
}

/// Forwards `record.file.<field>` to [`IndexRecord::field`].
///
/// A thin wrapper rather than a second lookup path, needed only because
/// minijinja resolves a dotted attribute path one segment at a time:
/// `record.file` must itself resolve to *something* before `.name` can be
/// looked up on it. Rewrites the key to `"file.<field>"` before delegating.
#[derive(Debug)]
struct FileFields(Arc<IndexRecord>);

impl Object for FileFields {
    fn get_value(self: &Arc<Self>, key: &Value) -> Option<Value> {
        let key = key.as_str()?;
        self.0.field(&format!("file.{key}")).ok().map(field_value)
    }

    fn enumerate(self: &Arc<Self>) -> Enumerator {
        Enumerator::Iter(Box::new(FileRecord::field_names().map(Value::from)))
    }
}

/// Converts a resolved [`FieldValue`] into a minijinja [`Value`].
///
/// - [`FieldValue::Null`] becomes minijinja's `none` rather than `undefined`:
///   [`IndexRecord::field`]'s own docs note that a well-formed path with no
///   value resolves to `Null`, not an error. That's a defined empty value, not
///   a missing attribute.
/// - [`FieldValue::Link`] renders as its target path; Traces has no
///   minijinja-facing link type yet.
fn field_value(value: FieldValue) -> Value {
    match value {
        FieldValue::Null => Value::from(()),
        FieldValue::Bool(b) => Value::from(b),
        FieldValue::Number(n) => Value::from(n),
        FieldValue::String(s)
        | FieldValue::Date(s)
        | FieldValue::Duration(s) => Value::from(s),
        FieldValue::Link(link) => Value::from(link.target().to_owned()),
        FieldValue::List(items) => {
            Value::from(items.into_iter().map(field_value).collect::<Vec<_>>())
        }
        FieldValue::Object(fields) => fields
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

    /// A minimal [`Environment`] with `query` registered against `root`.
    fn env(root: &Path) -> Environment<'static> {
        let mut env = Environment::new();
        QueryOps::new(Arc::from(root)).register(&mut env);
        env
    }

    fn render(root: &Path, source: &str) -> Result<String, Error> {
        env(root).render_str(source, minijinja::context!())
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
            let ops = Arc::new(QueryOps::new(Arc::from(temp.path())));

            assert!(ops.get_value(&Value::from("unknown")).is_none());
        }

        #[test]
        fn returns_none_for_a_non_string_key() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let ops = Arc::new(QueryOps::new(Arc::from(temp.path())));

            assert!(ops.get_value(&Value::from(1)).is_none());
        }
    }

    mod enumerate {
        use super::*;

        #[test]
        fn lists_every_method() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let ops = Arc::new(QueryOps::new(Arc::from(temp.path())));

            assert!(matches!(ops.enumerate(), Enumerator::Str(METHODS)));
        }

        #[test]
        fn every_enumerated_method_resolves_via_get_value() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let ops = Arc::new(QueryOps::new(Arc::from(temp.path())));

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

            let rendered = render(temp.path(), "{{ query.all() | length }}")
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

            let rendered = render(temp.path(), "{{ query.all() | length }}")
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
                r##"{% for n in query.from_tags("#book") %}{{ n.file.name }}{% endfor %}"##,
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
                r#"{% for n in query.from_folder("books") %}{{ n.file.name }}{% endfor %}"#,
            )
            .expect("render succeeds");

            assert_eq!(rendered, "dune");
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
                r##"{% for n in query.from_tags("#book").where("rating > 5") %}{{ n.file.name }}{% endfor %}"##,
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
                r##"{% for n in query.from_tags("#book").sort("rating", true).limit(2) %}{{ n.file.name }} {% endfor %}"##,
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
                r##"{% for n in query.from_tags("#book").group_by("rating") %}{{ n.file.name }} {% endfor %}"##,
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
                "{{ query.all().flatten(\"tags\") | length }}",
            )
            .expect("render succeeds");

            assert_eq!(rendered, "2");
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
                "{% for n in query.all() %}{{ n.file.name }}|{{ n.rating \
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
                render(temp.path(), "{{ query.all()[0].file.name }}")
                    .expect("render succeeds");

            assert_eq!(rendered, "only");
        }

        #[test]
        fn missing_frontmatter_field_resolves_to_none_not_undefined() {
            let temp = tempfile::tempdir().expect("create temp dir");
            write_note(temp.path(), "note.md", "# No frontmatter");

            let rendered =
                render(temp.path(), "{{ query.all()[0].bogus_field is none }}")
                    .expect("render succeeds");

            assert_eq!(rendered, "true");
        }

        #[test]
        fn file_enumerates_every_name_from_file_record() {
            let temp = tempfile::tempdir().expect("create temp dir");
            write_note(temp.path(), "only.md", "# Only");

            let rendered = render(
                temp.path(),
                "{% for key in query.all()[0].file %}{{ key }},{% endfor %}",
            )
            .expect("render succeeds");

            let expected: String = FileRecord::field_names()
                .map(|name| format!("{name},"))
                .collect();
            assert_eq!(rendered, expected);
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
            crate::template::engine::ui_ops::UiOps::new(provider)
                .register(&mut env);

            let rendered = env
                .render_str(
                    r#"{% set picked = ui.select("pick", query.all(), attribute="file.name") %}{{ picked.file.name }}"#,
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
            crate::template::engine::ui_ops::UiOps::new(provider)
                .register(&mut env);

            let rendered = env
                .render_str(
                    r#"{% set picked = ui.multi_select("pick", query.all(), attribute="file.name") %}{{ picked | map(attribute="file.name") | join(",") }}"#,
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
                render(temp.path(), r#"{{ query.all().filter("rating >") }}"#)
                    .expect_err("malformed filter expression should error");

            assert!(error.to_string().contains("query failed"));
        }

        #[test]
        fn unknown_field_path_in_sort_surfaces_as_a_render_error() {
            let temp = tempfile::tempdir().expect("create temp dir");
            write_note(temp.path(), "note.md", "# Note");

            let error =
                render(temp.path(), r#"{{ query.all().sort("a.b.c", true) }}"#)
                    .expect_err("malformed field path should error");

            assert!(error.to_string().contains("query failed"));
        }

        #[test]
        fn negative_limit_surfaces_as_a_render_error() {
            let temp = tempfile::tempdir().expect("create temp dir");
            write_note(temp.path(), "note.md", "# Note");

            let error = render(temp.path(), "{{ query.all().limit(-1) }}")
                .expect_err("negative limit should error");

            assert!(error.to_string().contains("query failed"));
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
                .render_str("{{ query.all() | length }}", minijinja::context!())
                .expect("render succeeds");
            assert_eq!(first, "1");

            write_note(temp.path(), "b.md", "# B");
            let second = e
                .render_str("{{ query.all() | length }}", minijinja::context!())
                .expect("render succeeds");
            assert_eq!(second, "2");
        }
    }
}
