//! Selects query sources, resolves record fields, and transforms query
//! outcomes.
//!
//! This module powers page-level results from [`super::FileIndex::query`] and
//! task-level rows from [`super::FileIndex::query_tasks`].
//!
//! # Main Types
//!
//! - [`QuerySource`]: Selects which Notes a query includes.
//! - [`IndexRecord`]: Pairs a [`FileRecord`] with its parsed [`Note`] and
//!   resolves `file.*`, `task.*`, frontmatter, tag, and inlinks fields.
//! - [`QueryOutcome`]: Stores result rows, applies chained transformations
//!   ([`QueryOutcome::filter`], [`QueryOutcome::sort`],
//!   [`QueryOutcome::limit`], [`QueryOutcome::group_by`],
//!   [`QueryOutcome::flatten`]), and renders terminal Markdown output
//!   ([`QueryOutcome::table`], [`QueryOutcome::list`],
//!   [`QueryOutcome::task_list`]). Terminal renderers are plain Rust methods
//!   with no minijinja dependency, enabling reuse across template namespaces
//!   and CLI query commands.
//! - [`QueryError`]: Reports malformed field paths and query expressions.

mod error;
mod field;
mod filter;
mod operators;
mod sort;

use std::{collections::BTreeSet, path::PathBuf, sync::Arc};

pub(crate) use error::QueryError;
pub(crate) use field::FileField;
use field::{FieldPath, TaskField};
use filter::FilterExpr;
use sort::SortKey;
pub(crate) use sort::SortOrder;

use super::file::FileRecord;
use crate::note::{FieldValue, Note};

/// Selects which Markdown Notes a page-level or task-level query includes.
///
/// Each variant defines its own matching behavior applied to every indexed
/// Note. Passing `None` from CLI flags selects [`Self::All`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum QuerySource {
    /// Matches every indexed Markdown Note regardless of tags, folder, or
    /// class.
    All,
    /// Matches Notes whose tags include `tag` exactly, or a sub-tag nested
    /// under it (for example, `#projects` also matches `#projects/active`).
    Tag(String),
    /// Matches Notes whose project-relative path starts with `folder`,
    /// including the folder itself and every directory nested under it.
    Folder(PathBuf),
    /// Matches Notes whose File Class(es) overlap the resolved `classes` set.
    /// A Note's File Class(es) are read from the frontmatter field named
    /// `class_field`; the Note matches when any of those values is in
    /// `classes`, the resolved is-a match set built by the schema registry.
    Class {
        /// Frontmatter field naming the Note's File Class(es).
        class_field: Arc<str>,
        /// Resolved match set: the queried class names plus every Schema
        /// that transitively `extends` one of them.
        classes: BTreeSet<String>,
    },
}

impl QuerySource {
    /// Builds a [`QuerySource`] from a `--from`-style CLI flag value.
    ///
    /// Passing `None` selects [`Self::All`]. A string starting with `#` selects
    /// [`Self::Tag`] (matching nested sub-tags such as `#book/fiction` for
    /// `#book`), while any other string selects [`Self::Folder`].
    #[must_use]
    pub(crate) fn from_flag(flag: Option<&str>) -> Self {
        match flag {
            None => Self::All,
            Some(value) if value.starts_with('#') => {
                Self::Tag(value.to_owned())
            }
            Some(value) => Self::Folder(PathBuf::from(value)),
        }
    }

    /// Returns `true` if `file` and its parsed `note` belong to this source.
    #[inline]
    #[must_use]
    pub(super) fn is_match(&self, file: &FileRecord, note: &Note) -> bool {
        match self {
            Self::All => true,
            Self::Tag(tag) => {
                note.tags().iter().any(|t| t.is_nested_under(tag))
            }
            Self::Folder(folder) => file.folder().starts_with(folder),
            Self::Class {
                class_field,
                classes,
            } => class_values(note, class_field)
                .any(|value| classes.contains(value)),
        }
    }
}

/// Yields a Note's File Class values: the strings held by the frontmatter
/// field named `class_field`.
///
/// - A single string yields one element.
/// - A list of strings yields each string element.
/// - A missing field, a non-string scalar, or non-string list elements yield
///   nothing.
pub(super) fn class_values<'a>(
    note: &'a Note,
    class_field: &str,
) -> impl Iterator<Item = &'a str> {
    let value = note.frontmatter().and_then(|frontmatter| {
        let field = frontmatter
            .fields()
            .iter()
            .find(|field| field.key().is_match(class_field))?;
        Some(field.value())
    });
    let list = match value {
        Some(FieldValue::List(items)) => items.as_slice(),
        _ => &[],
    };
    let scalar = match value {
        Some(FieldValue::List(_)) | None => None,
        Some(other) => other.as_str(),
    };
    list.iter().filter_map(FieldValue::as_str).chain(scalar)
}

/// Represents a query row pairing a [`FileRecord`] with parsed [`Note`]
/// metadata.
///
/// Task-level rows also carry fields for a single task item. Each row resolves
/// `file.*`, `task.*`, frontmatter, inline fields, tags, and derived inlinks
/// for template rendering and CLI output.
#[derive(Clone, Debug, PartialEq)]
pub struct IndexRecord {
    file: FileRecord,
    /// Reference-counted, not owned outright: exploding one Note into
    /// several rows (see [`super::FileIndex::query_tasks`] and
    /// [`QueryOutcome::flatten`]) shares this field across every row instead
    /// of deep-cloning frontmatter, links, tags, and lists per row.
    note: Arc<Note>,
    /// Overrides field resolution for exploded rows produced by
    /// [`QueryOutcome::flatten`].
    flattened: Vec<(FieldPath, FieldValue)>,
    /// Stores per-task fields set by [`super::FileIndex::query_tasks`], or
    /// `None` for page-level records.
    task: Option<TaskInfo>,
    /// Stores project-relative paths of Notes whose outlinks resolve to this
    /// row's Note, set by [`super::FileIndex::query`] and
    /// [`super::FileIndex::query_tasks`].
    inlinks: Vec<PathBuf>,
}

impl IndexRecord {
    /// Creates a new [`IndexRecord`] pairing `file` with its parsed `note`.
    pub(super) fn new(file: FileRecord, note: Note) -> Self {
        Self {
            file,
            note: Arc::new(note),
            flattened: Vec::new(),
            task: None,
            inlinks: Vec::new(),
        }
    }

    /// Converts this record into a task-level row with specified completion
    /// state and text.
    ///
    /// Used by [`super::FileIndex::query_tasks`] to turn a page-level record
    /// into one row per task item while retaining parent Note metadata for
    /// filtering and display via [`Self::field`].
    pub(super) fn with_task(
        mut self,
        completed: bool,
        text: impl Into<String>,
    ) -> Self {
        self.task = Some(TaskInfo {
            completed,
            text: text.into(),
        });
        self
    }

    /// Attaches project-relative paths of Notes whose outlinks resolve to this
    /// record's Note.
    pub(super) fn with_inlinks(mut self, inlinks: Vec<PathBuf>) -> Self {
        self.inlinks = inlinks;
        self
    }

    /// Returns task completion state (`true` for `- [x]`, `false` for
    /// `- [ ]`) if this is a task-level record, or `None` for page-level
    /// records.
    #[inline]
    #[must_use]
    pub fn task_completed(&self) -> Option<bool> {
        self.task.as_ref().map(|task| task.completed)
    }

    /// Returns the task item's text if this is a task-level record, or `None`
    /// for page-level records.
    #[inline]
    #[must_use]
    pub(crate) fn task_text(&self) -> Option<&str> {
        self.task.as_ref().map(|task| task.text.as_str())
    }

    /// Returns general metadata for the indexed file.
    #[inline]
    #[must_use]
    pub fn file(&self) -> &FileRecord {
        &self.file
    }

    /// Returns parsed [`Note`] metadata for the indexed file.
    #[inline]
    #[must_use]
    pub(crate) fn note(&self) -> &Note {
        self.note.as_ref()
    }

    /// Returns project-relative paths of Notes whose outlinks resolve to this
    /// record's Note, or an empty slice if no Notes link to it.
    #[inline]
    #[must_use]
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "no current caller outside tests; documented deliberate \
                      API in index-query#10's derived-inlinks design \
                      (Templates/CLI select or filter inlinks via \
                      field(\"inlinks\") today; this direct accessor for \
                      display output is not yet wired to a CLI/Template \
                      renderer)"
        )
    )]
    pub(crate) fn inlinks(&self) -> &[PathBuf] {
        &self.inlinks
    }

    /// Resolves a field path string against this record's metadata.
    ///
    /// Resolves `file.*` accessors, `task.*` accessors, frontmatter fields,
    /// inline fields, `tags`, and `inlinks`. Resolution rules include:
    /// - Frontmatter fields take precedence over inline fields sharing the same
    ///   key (see [`Note::fields`]).
    /// - Well-formed paths without values (such as a missing key or a `task.*`
    ///   accessor on a page-level record) resolve to [`FieldValue::Null`].
    ///
    /// # Errors
    ///
    /// - [`UnknownFieldPath`] if `path` cannot be parsed as a valid field path.
    ///
    /// [`UnknownFieldPath`]: QueryError::UnknownFieldPath
    #[inline]
    pub(crate) fn field(&self, path: &str) -> Result<FieldValue, QueryError> {
        Ok(self.resolve(&FieldPath::parse(path)?))
    }

    /// Resolves a parsed [`FieldPath`], applying any [`Self::flattened`]
    /// overrides.
    fn resolve(&self, path: &FieldPath) -> FieldValue {
        if let Some((_, value)) = self.flattened.iter().find(|(p, _)| p == path)
        {
            return value.clone();
        }
        match path {
            FieldPath::File(field) => field.resolve(&self.file),
            FieldPath::Task(field) => self.task.as_ref().map_or(
                FieldValue::Null,
                |task| match field {
                    TaskField::Completed => FieldValue::Bool(task.completed),
                    TaskField::Text => FieldValue::String(task.text.clone()),
                },
            ),
            FieldPath::Tags => FieldValue::List(
                self.note
                    .tags()
                    .iter()
                    .map(|tag| FieldValue::String(tag.as_str().to_owned()))
                    .collect(),
            ),
            FieldPath::Inlinks => FieldValue::List(
                self.inlinks
                    .iter()
                    .map(|linking_note| {
                        FieldValue::String(
                            linking_note.to_string_lossy().into_owned(),
                        )
                    })
                    .collect(),
            ),
            FieldPath::Metadata(key) => self
                .note
                .fields()
                .find(|field| field.key().is_match(key.as_str()))
                .map_or(FieldValue::Null, |field| field.value().clone()),
        }
    }

    /// Returns a copy of this record with `path` overridden to `value` for
    /// exploded [`QueryOutcome::flatten`] rows.
    fn with_flattened(mut self, path: FieldPath, value: FieldValue) -> Self {
        if let Some(entry) = self.flattened.iter_mut().find(|(p, _)| p == &path)
        {
            entry.1 = value;
        } else {
            self.flattened.push((path, value));
        }
        self
    }
}

/// Represents per-task fields layered onto an [`IndexRecord`] by
/// [`super::FileIndex::query_tasks`].
///
/// Task-level rows retain parent [`Note`] file and metadata fields for
/// filtering and display while attaching task completion and text attributes.
/// This is distinct from [`IndexRecord::flattened`], which overrides existing
/// field paths rather than adding new ones.
#[derive(Clone, Debug, PartialEq)]
struct TaskInfo {
    completed: bool,
    text: String,
}

/// Represents an ordered collection of [`IndexRecord`] rows produced by an
/// index query.
///
/// Page-level outcomes contain one row per Note, while task-level outcomes
/// contain one row per task item. Transformation methods consume and return a
/// [`QueryOutcome`], enabling method chaining such as `outcome.filter("rating >
/// 7")?.sort("rating", true)?.limit(10)?`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct QueryOutcome {
    records: Vec<IndexRecord>,
}

impl QueryOutcome {
    /// Wraps `records` into a new [`QueryOutcome`].
    pub(super) fn new(records: Vec<IndexRecord>) -> Self {
        Self {
            records,
        }
    }

    /// Returns the number of [`IndexRecord`] rows in this outcome.
    #[inline]
    #[must_use]
    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// Returns `true` if this outcome contains no [`IndexRecord`] rows.
    #[inline]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// Returns a reference to the [`IndexRecord`] at `index`, or `None` if out
    /// of bounds.
    #[inline]
    #[must_use]
    pub fn get(&self, index: usize) -> Option<&IndexRecord> {
        self.records.get(index)
    }

    /// Returns an iterator over references to the contained [`IndexRecord`]
    /// rows.
    #[inline]
    pub(crate) fn iter(&self) -> std::slice::Iter<'_, IndexRecord> {
        self.records.iter()
    }

    /// Retains only records matching the filter expression `expr`.
    ///
    /// Supported syntax:
    ///
    /// - **Comparisons:** `<field> <op> <value>` with `==`, `!=`, `>=`, `<=`,
    ///   `>`, or `<`.
    /// - **Functions:** `contains(field, value)` checks list membership, tag
    ///   hierarchy (for example `#book` matching `#book/fiction`), or substring
    ///   containment.
    /// - **Logical operators:** `AND` / `and` / `&&`, `OR` / `or` / `||`, and
    ///   `NOT` / `not` / `!`.
    /// - **Grouping:** `( ... )` overrides default operator precedence.
    /// - **Literals:** quoted strings with `\` escapes, numbers, booleans
    ///   (`true`/`false`), and `null`/`Null`.
    ///
    /// Matching rules:
    ///
    /// - `==` and `!=` compare [`String`], `Date`, and `Duration` values by
    ///   text.
    /// - Mismatched data types never match except under `!=`.
    /// - Records missing a field (`Null`) fail equality and ordering checks,
    ///   but match `!=`.
    ///
    /// # Errors
    ///
    /// - `UnparsableFilterExpression` if `expr` cannot be parsed.
    /// - `UnknownFieldPath` if a field path referenced in `expr` is malformed.
    #[inline]
    pub fn filter(self, expr: &str) -> Result<Self, QueryError> {
        let expr = FilterExpr::parse(expr)?;
        let records = self
            .records
            .into_iter()
            .filter(|record| expr.matches(record))
            .collect();
        Ok(Self::new(records))
    }

    /// Filters records matching `expr`, serving as an alias for
    /// [`Self::filter`].
    ///
    /// Uses Rust raw identifier syntax (`r#where`) because `where` is a
    /// reserved keyword; `where` is this query API's name for the same
    /// operation as `filter`. Refer to [`Self::filter`] for full syntax
    /// details and matching behavior.
    ///
    /// # Errors
    ///
    /// - [`UnparsableFilterExpression`] if `expr` cannot be parsed.
    /// - [`UnknownFieldPath`] if a field path referenced in `expr` is
    ///   malformed.
    ///
    /// [`UnparsableFilterExpression`]: QueryError::UnparsableFilterExpression
    /// [`UnknownFieldPath`]: QueryError::UnknownFieldPath
    #[inline]
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "no current caller outside tests; Rust-side alias for \
                      direct callers of this crate's Rust API (Templates and \
                      the CLI both call filter() directly, see \
                      template/engine/query.rs's \"filter\" | \"where\" \
                      dispatch, which maps the Template-facing `where` method \
                      name onto filter(), not r#where())"
        )
    )]
    pub(crate) fn r#where(self, expr: &str) -> Result<Self, QueryError> {
        self.filter(expr)
    }

    /// Sorts records by the field at `path` in ascending or descending order.
    ///
    /// Sort semantics:
    /// - Null handling: records with [`FieldValue::Null`] at `path` sort as
    ///   minimum values (leading in ascending order, trailing in descending
    ///   order).
    /// - Stability: records with equal or incomparable keys preserve their
    ///   original relative order.
    ///
    /// # Errors
    ///
    /// - `UnknownFieldPath` if `path` cannot be parsed as a valid field path.
    #[inline]
    pub fn sort(
        self,
        path: &str,
        descending: bool,
    ) -> Result<Self, QueryError> {
        self.sort_by_field(path, descending)
    }

    /// Truncates the outcome to retain at most `n` leading records.
    ///
    /// # Errors
    ///
    /// - `NegativeLimit` if `n` is negative or exceeds platform pointer width
    ///   limits.
    #[inline]
    pub fn limit(self, n: i64) -> Result<Self, QueryError> {
        let n = usize::try_from(n).map_err(|_source| {
            QueryError::NegativeLimit {
                n,
            }
        })?;
        Ok(Self::new(self.records.into_iter().take(n).collect()))
    }

    /// Groups records by sorting them ascending on the field at `path`, so
    /// template loops or terminal renderers can detect group transitions by
    /// comparing adjacent records.
    ///
    /// # Errors
    ///
    /// - [`UnknownFieldPath`] if `path` cannot be parsed as a valid field path.
    ///
    /// [`UnknownFieldPath`]: QueryError::UnknownFieldPath
    #[inline]
    pub(crate) fn group_by(self, path: &str) -> Result<Self, QueryError> {
        self.sort_by_field(path, false)
    }

    /// Explodes records containing a list at `path` into one row per list
    /// element.
    ///
    /// Behavior:
    ///
    /// - **List fields:** applies to fields resolving to [`FieldValue::List`]
    ///   (such as frontmatter lists, inline list fields, or `tags`).
    /// - **Non-list fields:** records with scalar values pass through
    ///   unmodified.
    /// - **Empty lists:** records with empty list values yield no rows.
    /// - **Field resolution:** exploded rows resolve `path` to the individual
    ///   list element, retaining all other fields from the source record.
    ///
    /// # Errors
    ///
    /// - [`UnknownFieldPath`] if `path` cannot be parsed as a valid field path.
    ///
    /// [`UnknownFieldPath`]: QueryError::UnknownFieldPath
    pub(crate) fn flatten(self, path: &str) -> Result<Self, QueryError> {
        let field_path = FieldPath::parse(path)?;
        let mut records = Vec::with_capacity(self.records.len());
        for record in self.records {
            let FieldValue::List(mut items) = record.resolve(&field_path)
            else {
                records.push(record);
                continue;
            };
            let Some(last) = items.pop() else {
                continue; // Empty list: this record contributes no rows.
            };
            records.extend(items.into_iter().map(|item| {
                record.clone().with_flattened(field_path.clone(), item)
            }));
            records.push(record.with_flattened(field_path.clone(), last));
        }
        Ok(Self::new(records))
    }

    /// Renders records as a Markdown table matching headers to corresponding
    /// column field paths.
    ///
    /// Pairs each `headers` label with the field path at the identical index in
    /// `columns`. Table formatting uses `comfy-table`'s [`ASCII_MARKDOWN`]
    /// preset to align column widths. Pipe characters (`|`) are escaped and
    /// newlines collapse into spaces to prevent table layout corruption.
    ///
    /// # Errors
    ///
    /// - [`TableColumnMismatch`] if `headers` and `columns` slices differ in
    ///   length.
    /// - [`UnknownFieldPath`] if any field path string in `columns` is
    ///   malformed.
    ///
    /// [`ASCII_MARKDOWN`]: comfy_table::presets::ASCII_MARKDOWN
    /// [`TableColumnMismatch`]: QueryError::TableColumnMismatch
    /// [`UnknownFieldPath`]: QueryError::UnknownFieldPath
    pub(crate) fn table(
        &self,
        headers: &[&str],
        columns: &[&str],
    ) -> Result<String, QueryError> {
        if headers.len() != columns.len() {
            return Err(QueryError::TableColumnMismatch {
                headers: headers.len(),
                columns: columns.len(),
            });
        }
        let paths = columns
            .iter()
            .map(|column| FieldPath::parse(column))
            .collect::<Result<Vec<_>, _>>()?;
        let mut table = comfy_table::Table::new();
        table.load_preset(comfy_table::presets::ASCII_MARKDOWN);
        table
            .set_header(headers.iter().map(|header| escape_table_text(header)));
        for record in &self.records {
            table.add_row(
                paths.iter().map(|path| table_cell_text(&record.resolve(path))),
            );
        }
        let mut out = table.to_string();
        out.push('\n');
        Ok(out)
    }

    /// Renders records as a Markdown bullet list formatting the resolved field
    /// value at `path`.
    ///
    /// # Errors
    ///
    /// - [`UnknownFieldPath`] if `path` cannot be parsed as a valid field path.
    ///
    /// [`UnknownFieldPath`]: QueryError::UnknownFieldPath
    pub(crate) fn list(&self, path: &str) -> Result<String, QueryError> {
        let field_path = FieldPath::parse(path)?;
        let mut out = String::new();
        for record in &self.records {
            out.push_str("- ");
            out.push_str(&field_text(&record.resolve(&field_path)));
            out.push('\n');
        }
        Ok(out)
    }

    /// Renders task-level records as a Markdown task list (`- [ ]` or `- [x]`).
    ///
    /// Renders each record using its task completion state and text fields.
    ///
    /// # Errors
    ///
    /// - [`TaskListOnPageRecords`] if any record lacks task fields (built by
    ///   [`super::FileIndex::query`] instead of
    ///   [`super::FileIndex::query_tasks`]).
    ///
    /// [`TaskListOnPageRecords`]: QueryError::TaskListOnPageRecords
    pub(crate) fn task_list(&self) -> Result<String, QueryError> {
        let mut out = String::new();
        for record in &self.records {
            let Some(completed) = record.task_completed() else {
                return Err(QueryError::TaskListOnPageRecords);
            };
            out.push_str(if completed {
                "- [x] "
            } else {
                "- [ ] "
            });
            out.push_str(record.task_text().unwrap_or_default());
            out.push('\n');
        }
        Ok(out)
    }

    /// Sorts records stably by the resolved value of `path`.
    ///
    /// Provides shared sorting logic for [`Self::sort`] and [`Self::group_by`],
    /// treating [`FieldValue::Null`] as the minimum value.
    ///
    /// # Performance
    ///
    /// Runs key resolution in `O(n)` time using [`slice::sort_by_cached_key`],
    /// resolving `path` once per record rather than on every comparison made
    /// by a standard `sort_by` closure.
    ///
    /// # Errors
    ///
    /// - [`UnknownFieldPath`] if `path` cannot be parsed as a valid field path.
    ///
    /// [`UnknownFieldPath`]: QueryError::UnknownFieldPath
    fn sort_by_field(
        self,
        path: &str,
        descending: bool,
    ) -> Result<Self, QueryError> {
        let field_path = FieldPath::parse(path)?;
        let mut records = self.records;
        records.sort_by_cached_key(|record| SortKey {
            value: record.resolve(&field_path),
            descending,
        });
        Ok(Self::new(records))
    }
}

impl IntoIterator for QueryOutcome {
    type IntoIter = std::vec::IntoIter<Self::Item>;
    type Item = IndexRecord;

    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        self.records.into_iter()
    }
}

impl<'a> IntoIterator for &'a QueryOutcome {
    type IntoIter = std::slice::Iter<'a, IndexRecord>;
    type Item = &'a IndexRecord;

    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        self.records.iter()
    }
}

/// Converts a resolved [`FieldValue`] to plain text for list and table
/// rendering.
///
/// Conversion rules:
/// - [`FieldValue::Null`] renders as an empty string.
/// - [`FieldValue::Link`] renders as its target path.
/// - [`FieldValue::List`] and [`FieldValue::Object`] flatten recursively with
///   elements joined by `", "`.
fn field_text(value: &FieldValue) -> String {
    match value {
        FieldValue::Null => String::new(),
        FieldValue::Bool(b) => b.to_string(),
        FieldValue::Number(n) => n.to_string(),
        FieldValue::String(s)
        | FieldValue::Date(s)
        | FieldValue::Duration(s) => s.clone(),
        FieldValue::Link(link) => link.target().to_owned(),
        FieldValue::List(items) => {
            items.iter().map(field_text).collect::<Vec<_>>().join(", ")
        }
        FieldValue::Object(fields) => fields
            .iter()
            .map(|(key, field)| format!("{key}: {}", field_text(field)))
            .collect::<Vec<_>>()
            .join(", "),
    }
}

/// Escapes pipes (`|`) and collapses newlines to spaces to preserve table row
/// formatting.
fn escape_table_text(text: &str) -> String {
    text.replace('\n', " ").replace('|', "\\|")
}

/// Converts a [`FieldValue`] to plain text and escapes table formatting
/// characters.
fn table_cell_text(value: &FieldValue) -> String {
    escape_table_text(&field_text(value))
}

#[cfg(test)]
mod tests {
    use std::{fs, path::Path};

    use super::*;
    use crate::index::FileIndex;

    mod fixtures {
        use std::{fs, path::Path};

        use super::*;

        /// Builds a [`QueryOutcome`] over every Markdown Note in `files`
        /// written under `temp`.
        pub(super) fn outcome_for_files(
            temp: &Path,
            files: &[(&str, &str)],
        ) -> QueryOutcome {
            for (name, content) in files {
                fs::write(temp.join(name), content).expect("write note");
            }
            FileIndex::build(temp)
                .expect("build index")
                .query(&QuerySource::All)
        }

        /// Builds a single-record [`QueryOutcome`] from a single Markdown
        /// Note's content.
        pub(super) fn outcome_for(temp: &Path, content: &str) -> QueryOutcome {
            outcome_for_files(temp, &[("note.md", content)])
        }
    }
    use fixtures::*;

    mod source_is_match {

        use super::*;

        #[test]
        fn returns_true_for_all_source() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("a.md"), "# A").expect("write file");
            let index = FileIndex::build(temp.path()).expect("build index");
            let record = index.record(Path::new("a.md")).expect("record");
            let note = index.note(Path::new("a.md")).expect("note");

            assert!(QuerySource::All.is_match(record, note));
        }

        #[test]
        fn returns_true_when_note_has_matching_or_sub_tag() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("a.md"), "Tracked in #projects/active.")
                .expect("write file");
            let index = FileIndex::build(temp.path()).expect("build index");
            let record = index.record(Path::new("a.md")).expect("record");
            let note = index.note(Path::new("a.md")).expect("note");

            assert!(
                QuerySource::Tag("#projects".to_owned()).is_match(record, note)
            );
            assert!(
                !QuerySource::Tag("#books".to_owned()).is_match(record, note)
            );
        }

        #[test]
        fn returns_true_when_file_is_under_folder_source() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::create_dir_all(temp.path().join("projects/active"))
                .expect("mkdir");
            fs::write(temp.path().join("projects/active/task.md"), "# Task")
                .expect("write file");
            let index = FileIndex::build(temp.path()).expect("build index");
            let record = index
                .record(Path::new("projects/active/task.md"))
                .expect("record");
            let note =
                index.note(Path::new("projects/active/task.md")).expect("note");

            assert!(
                QuerySource::Folder(PathBuf::from("projects"))
                    .is_match(record, note)
            );
            assert!(
                !QuerySource::Folder(PathBuf::from("archive"))
                    .is_match(record, note)
            );
        }

        #[test]
        fn returns_true_when_note_class_is_in_the_match_set() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("a.md"), "---\nclass: book\n---\n# A")
                .expect("write file");
            let index = FileIndex::build(temp.path()).expect("build index");
            let record = index.record(Path::new("a.md")).expect("record");
            let note = index.note(Path::new("a.md")).expect("note");

            assert!(
                QuerySource::Class {
                    class_field: Arc::from("class"),
                    classes: BTreeSet::from(["book".to_owned()]),
                }
                .is_match(record, note)
            );
            assert!(
                !QuerySource::Class {
                    class_field: Arc::from("class"),
                    classes: BTreeSet::from(["movie".to_owned()]),
                }
                .is_match(record, note)
            );
        }

        #[test]
        fn matches_any_class_of_a_multi_class_note() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(
                temp.path().join("a.md"),
                "---\nclass: [book, movie]\n---\n# A",
            )
            .expect("write file");
            let index = FileIndex::build(temp.path()).expect("build index");
            let record = index.record(Path::new("a.md")).expect("record");
            let note = index.note(Path::new("a.md")).expect("note");

            assert!(
                QuerySource::Class {
                    class_field: Arc::from("class"),
                    classes: BTreeSet::from(["movie".to_owned()]),
                }
                .is_match(record, note)
            );
        }

        #[test]
        fn reads_the_class_from_the_configured_field() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("a.md"), "---\nkind: book\n---\n# A")
                .expect("write file");
            let index = FileIndex::build(temp.path()).expect("build index");
            let record = index.record(Path::new("a.md")).expect("record");
            let note = index.note(Path::new("a.md")).expect("note");

            assert!(
                QuerySource::Class {
                    class_field: Arc::from("kind"),
                    classes: BTreeSet::from(["book".to_owned()]),
                }
                .is_match(record, note)
            );
            assert!(
                !QuerySource::Class {
                    class_field: Arc::from("class"),
                    classes: BTreeSet::from(["book".to_owned()]),
                }
                .is_match(record, note)
            );
        }

        #[test]
        fn returns_false_when_note_has_no_class_field() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("a.md"), "# A").expect("write file");
            let index = FileIndex::build(temp.path()).expect("build index");
            let record = index.record(Path::new("a.md")).expect("record");
            let note = index.note(Path::new("a.md")).expect("note");

            assert!(
                !QuerySource::Class {
                    class_field: Arc::from("class"),
                    classes: BTreeSet::from(["book".to_owned()]),
                }
                .is_match(record, note)
            );
        }

        #[test]
        fn returns_false_when_class_value_is_not_a_string() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("a.md"), "---\nclass: 5\n---\n# A")
                .expect("write file");
            let index = FileIndex::build(temp.path()).expect("build index");
            let record = index.record(Path::new("a.md")).expect("record");
            let note = index.note(Path::new("a.md")).expect("note");

            assert!(
                !QuerySource::Class {
                    class_field: Arc::from("class"),
                    classes: BTreeSet::from(["5".to_owned()]),
                }
                .is_match(record, note)
            );
        }
    }

    mod source_from_flag {
        use pretty_assertions::assert_eq;
        use rstest::rstest;

        use super::*;

        #[rstest]
        #[case::none_selects_all(None, QuerySource::All)]
        #[case::hash_prefix_selects_tag(
            Some("#projects"),
            QuerySource::Tag("#projects".to_owned())
        )]
        #[case::other_value_selects_folder(
            Some("books"),
            QuerySource::Folder(PathBuf::from("books"))
        )]
        fn selects_the_expected_source_variant(
            #[case] flag: Option<&str>,
            #[case] expected: QuerySource,
        ) {
            assert_eq!(QuerySource::from_flag(flag), expected);
        }
    }

    mod index_record {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn accessors_return_file_record_and_note() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("a.md"), "Filed under #tag.")
                .expect("write file");
            let index = FileIndex::build(temp.path()).expect("build index");
            let file = index.record(Path::new("a.md")).expect("record").clone();
            let note = index.note(Path::new("a.md")).expect("note").clone();

            let record = IndexRecord::new(file.clone(), note.clone());

            assert_eq!(record.file(), &file);
            assert_eq!(record.note(), &note);
        }

        #[test]
        fn with_task_sets_task_completed_and_task_text() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("a.md"), "body").expect("write file");
            let index = FileIndex::build(temp.path()).expect("build index");
            let file = index.record(Path::new("a.md")).expect("record").clone();
            let note = index.note(Path::new("a.md")).expect("note").clone();

            let record =
                IndexRecord::new(file, note).with_task(true, "Buy milk");

            assert_eq!(record.task_completed(), Some(true));
            assert_eq!(record.task_text(), Some("Buy milk"));
        }

        #[test]
        fn task_accessors_return_none_for_page_level_records() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("a.md"), "body").expect("write file");
            let index = FileIndex::build(temp.path()).expect("build index");
            let file = index.record(Path::new("a.md")).expect("record").clone();
            let note = index.note(Path::new("a.md")).expect("note").clone();

            let record = IndexRecord::new(file, note);

            assert_eq!(record.task_completed(), None);
            assert_eq!(record.task_text(), None);
        }

        #[test]
        fn with_inlinks_sets_the_inlinks_accessor() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("a.md"), "body").expect("write file");
            let index = FileIndex::build(temp.path()).expect("build index");
            let file = index.record(Path::new("a.md")).expect("record").clone();
            let note = index.note(Path::new("a.md")).expect("note").clone();

            let record = IndexRecord::new(file, note)
                .with_inlinks(vec![PathBuf::from("b.md")]);

            assert_eq!(record.inlinks(), [PathBuf::from("b.md")]);
        }
    }

    mod field_path {
        use pretty_assertions::assert_eq;
        use rstest::rstest;

        use super::*;

        #[test]
        fn resolves_file_path_name_folder_and_size() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::create_dir_all(temp.path().join("notes")).expect("mkdir");
            let outcome =
                outcome_for_files(temp.path(), &[("notes/todo.md", "body")]);
            let record = outcome.get(0).expect("record");

            assert_eq!(
                record.field("file.path"),
                Ok(FieldValue::String("notes/todo.md".to_owned()))
            );
            assert_eq!(
                record.field("file.name"),
                Ok(FieldValue::String("todo".to_owned()))
            );
            assert_eq!(
                record.field("file.folder"),
                Ok(FieldValue::String("notes".to_owned()))
            );
            assert_eq!(record.field("file.size"), Ok(FieldValue::Number(4.0)));
        }

        #[test]
        fn resolves_dataview_style_time_accessors_from_file_record() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let outcome = outcome_for(temp.path(), "body");
            let record = outcome.get(0).expect("record");
            let file = record.file();

            assert_eq!(
                record.field("file.mtime"),
                Ok(FieldValue::Date(file.modified_at().to_datetime_string()))
            );
            assert_eq!(
                record.field("file.mdate"),
                Ok(FieldValue::Date(file.modified_at().to_date_string()))
            );
            assert_eq!(
                record.field("file.ctime"),
                Ok(FieldValue::Date(
                    file.created_at_or_modified().to_datetime_string()
                ))
            );
            assert_eq!(
                record.field("file.cdate"),
                Ok(FieldValue::Date(
                    file.created_at_or_modified().to_date_string()
                ))
            );
            assert_eq!(
                record.field("file.created_at"),
                record.field("file.ctime")
            );
            assert_eq!(
                record.field("file.modified_at"),
                record.field("file.mtime")
            );
        }

        #[test]
        fn resolves_frontmatter_and_inline_fields_by_key() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let outcome =
                outcome_for(temp.path(), "---\nrating: 5\n---\nStatus:: Draft");
            let record = outcome.get(0).expect("record");

            assert_eq!(record.field("rating"), Ok(FieldValue::Number(5.0)));
            assert_eq!(
                record.field("Status"),
                Ok(FieldValue::String("Draft".to_owned()))
            );
        }

        #[test]
        fn frontmatter_field_takes_precedence_over_same_key_inline_field() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let outcome = outcome_for(
                temp.path(),
                "---\nstatus: Approved\n---\nstatus:: Draft",
            );
            let record = outcome.get(0).expect("record");

            assert_eq!(
                record.field("status"),
                Ok(FieldValue::String("Approved".to_owned()))
            );
        }

        #[test]
        fn resolves_tags_as_a_list_of_tag_strings() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let outcome = outcome_for(temp.path(), "Filed under #book #read");
            let record = outcome.get(0).expect("record");

            assert_eq!(
                record.field("tags"),
                Ok(FieldValue::List(vec![
                    FieldValue::String("#book".to_owned()),
                    FieldValue::String("#read".to_owned()),
                ]))
            );
        }

        #[test]
        fn resolves_inlinks_as_a_list_of_linking_note_paths() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let outcome = outcome_for_files(temp.path(), &[
                ("target.md", "# Target"),
                ("a.md", "[[target]]"),
                ("b.md", "[[target]]"),
            ]);
            let record = outcome
                .iter()
                .find(|record| record.file().path() == Path::new("target.md"))
                .expect("target record");

            assert_eq!(
                record.field("inlinks"),
                Ok(FieldValue::List(vec![
                    FieldValue::String("a.md".to_owned()),
                    FieldValue::String("b.md".to_owned()),
                ]))
            );
        }

        #[test]
        fn resolves_inlinks_as_an_empty_list_when_nothing_links_to_the_note() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let outcome = outcome_for(temp.path(), "No inbound links here.");
            let record = outcome.get(0).expect("record");

            assert_eq!(record.field("inlinks"), Ok(FieldValue::List(vec![])));
        }

        #[test]
        fn missing_field_resolves_to_null() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let outcome = outcome_for(temp.path(), "body, no frontmatter");
            let record = outcome.get(0).expect("record");

            assert_eq!(record.field("no_such_field"), Ok(FieldValue::Null));
        }

        #[rstest]
        #[case::empty("")]
        #[case::bare_file("file")]
        #[case::trailing_dot("file.")]
        #[case::unknown_file_accessor("file.bogus")]
        #[case::extra_file_segment("file.name.extra")]
        #[case::bare_task("task")]
        #[case::trailing_dot_task("task.")]
        #[case::unknown_task_accessor("task.bogus")]
        #[case::extra_task_segment("task.completed.extra")]
        #[case::dotted_metadata_path("a.b")]
        fn rejects_malformed_field_paths(#[case] path: &str) {
            let temp = tempfile::tempdir().expect("create temp dir");
            let outcome = outcome_for(temp.path(), "body");
            let record = outcome.get(0).expect("record");

            assert_eq!(
                record.field(path),
                Err(QueryError::unknown_field_path(path, None))
            );
        }

        #[test]
        fn resolves_task_completed_and_task_text_on_task_rows() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let record = outcome_for(temp.path(), "body")
                .into_iter()
                .next()
                .expect("record")
                .with_task(true, "Buy milk");

            assert_eq!(
                record.field("task.completed"),
                Ok(FieldValue::Bool(true))
            );
            assert_eq!(
                record.field("task.text"),
                Ok(FieldValue::String("Buy milk".to_owned()))
            );
        }

        #[test]
        fn task_fields_resolve_to_null_on_page_level_records() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let outcome = outcome_for(temp.path(), "body");
            let record = outcome.get(0).expect("record");

            assert_eq!(record.field("task.completed"), Ok(FieldValue::Null));
            assert_eq!(record.field("task.text"), Ok(FieldValue::Null));
        }
    }

    mod limit {
        use pretty_assertions::assert_eq;

        use super::*;

        fn outcome_of_three(temp: &Path) -> QueryOutcome {
            outcome_for_files(temp, &[
                ("a.md", "# A"),
                ("b.md", "# B"),
                ("c.md", "# C"),
            ])
        }

        #[test]
        fn keeps_at_most_n_leading_records() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let outcome = outcome_of_three(temp.path());

            assert_eq!(outcome.limit(2).expect("valid limit").len(), 2);
        }

        #[test]
        fn n_at_or_above_length_keeps_every_record() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let outcome = outcome_of_three(temp.path());

            assert_eq!(outcome.limit(10).expect("valid limit").len(), 3);
        }

        #[test]
        fn zero_yields_an_empty_outcome() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let outcome = outcome_of_three(temp.path());

            assert!(outcome.limit(0).expect("valid limit").is_empty());
        }

        #[test]
        fn rejects_a_negative_limit() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let outcome = outcome_of_three(temp.path());

            assert_eq!(
                outcome.limit(-1),
                Err(QueryError::NegativeLimit {
                    n: -1
                })
            );
        }
    }

    mod group_by {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn clusters_records_with_equal_values_in_ascending_key_order() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let outcome = outcome_for_files(temp.path(), &[
                ("a.md", "---\ncategory: book\n---"),
                ("b.md", "---\ncategory: article\n---"),
                ("c.md", "---\ncategory: book\n---"),
            ]);

            let grouped = outcome.group_by("category").expect("valid group_by");

            let categories: Vec<FieldValue> = grouped
                .iter()
                .map(|record| record.field("category").expect("valid path"))
                .collect();
            assert_eq!(categories, [
                FieldValue::String("article".to_owned()),
                FieldValue::String("book".to_owned()),
                FieldValue::String("book".to_owned()),
            ]);
        }

        #[test]
        fn rejects_malformed_field_path() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let outcome = outcome_for(temp.path(), "body");

            assert_eq!(
                outcome.group_by("file.bogus"),
                Err(QueryError::unknown_field_path("file.bogus", None))
            );
        }
    }

    mod flatten {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn explodes_a_list_field_into_one_row_per_element() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let outcome = outcome_for(
                temp.path(),
                "---\ntitle: Multi\nauthors:\n  - Alice\n  - Bob\n---",
            );

            let flattened = outcome.flatten("authors").expect("valid flatten");

            assert_eq!(flattened.len(), 2);
            let authors: Vec<FieldValue> = flattened
                .iter()
                .map(|record| record.field("authors").expect("valid path"))
                .collect();
            assert_eq!(authors, [
                FieldValue::String("Alice".to_owned()),
                FieldValue::String("Bob".to_owned()),
            ]);
            // Every other field still resolves from the original record.
            for record in &flattened {
                assert_eq!(
                    record.field("title"),
                    Ok(FieldValue::String("Multi".to_owned()))
                );
            }
        }

        #[test]
        fn empty_list_drops_the_record() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let outcome = outcome_for(temp.path(), "---\nauthors: []\n---");

            let flattened = outcome.flatten("authors").expect("valid flatten");

            assert!(flattened.is_empty());
        }

        #[test]
        fn non_list_field_passes_through_unchanged() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let outcome = outcome_for(temp.path(), "---\nrating: 5\n---");

            let flattened = outcome.flatten("rating").expect("valid flatten");

            assert_eq!(flattened.len(), 1);
            assert_eq!(
                flattened.get(0).expect("record").field("rating"),
                Ok(FieldValue::Number(5.0))
            );
        }

        #[test]
        fn flattening_tags_yields_one_row_per_tag() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let outcome = outcome_for(temp.path(), "Filed under #book #read");

            let flattened = outcome.flatten("tags").expect("valid flatten");

            let tags: Vec<FieldValue> = flattened
                .iter()
                .map(|record| record.field("tags").expect("valid path"))
                .collect();
            assert_eq!(tags, [
                FieldValue::String("#book".to_owned()),
                FieldValue::String("#read".to_owned()),
            ]);
        }

        #[test]
        fn rejects_malformed_field_path() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let outcome = outcome_for(temp.path(), "body");

            assert_eq!(
                outcome.flatten("file.bogus"),
                Err(QueryError::unknown_field_path("file.bogus", None))
            );
        }

        #[test]
        fn chains_into_a_filter_over_the_flattened_value() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let outcome = outcome_for(
                temp.path(),
                "---\nauthors:\n  - Alice\n  - Bob\n---",
            );

            let filtered = outcome
                .flatten("authors")
                .expect("valid flatten")
                .filter("authors == \"Bob\"")
                .expect("valid filter");

            assert_eq!(filtered.len(), 1);
            assert_eq!(
                filtered.get(0).expect("record").field("authors"),
                Ok(FieldValue::String("Bob".to_owned()))
            );
        }

        #[test]
        fn chains_multiple_flatten_calls_without_overwriting_prior_overrides() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let outcome = outcome_for(
                temp.path(),
                "---\nauthors:\n  - Alice\n  - Bob\n---\nFiled under #book \
                 #read",
            );

            let flattened = outcome
                .flatten("authors")
                .expect("valid flatten")
                .flatten("tags")
                .expect("valid flatten");

            // 2 authors * 2 tags = 4 rows
            assert_eq!(flattened.len(), 4);
            let pairs: Vec<(FieldValue, FieldValue)> = flattened
                .iter()
                .map(|record| {
                    (
                        record.field("authors").expect("valid authors"),
                        record.field("tags").expect("valid tags"),
                    )
                })
                .collect();
            assert_eq!(pairs, [
                (
                    FieldValue::String("Alice".to_owned()),
                    FieldValue::String("#book".to_owned())
                ),
                (
                    FieldValue::String("Alice".to_owned()),
                    FieldValue::String("#read".to_owned())
                ),
                (
                    FieldValue::String("Bob".to_owned()),
                    FieldValue::String("#book".to_owned())
                ),
                (
                    FieldValue::String("Bob".to_owned()),
                    FieldValue::String("#read".to_owned())
                ),
            ]);
        }
    }

    mod table {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn renders_header_separator_and_one_row_per_record() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let outcome = outcome_for_files(temp.path(), &[
                ("a.md", "---\nrating: 5\n---"),
                ("b.md", "---\nrating: 3\n---"),
            ]);

            let table = outcome
                .table(&["Name", "Rating"], &["file.name", "rating"])
                .expect("valid table");

            let lines: Vec<&str> = table.lines().collect();
            assert_eq!(lines.len(), 4); // header + separator + 2 rows
            assert_eq!(lines.first(), Some(&"| Name | Rating |"));
            assert_eq!(lines.get(1), Some(&"|------|--------|"));
            assert!(lines.iter().skip(2).any(|line| line.contains('5')));
            assert!(lines.iter().skip(2).any(|line| line.contains('3')));
        }

        #[test]
        fn renders_no_data_rows_for_an_empty_outcome() {
            let table = QueryOutcome::default()
                .table(&["Name"], &["file.name"])
                .expect("valid table");

            assert_eq!(table, "| Name |\n|------|\n");
        }

        #[test]
        fn escapes_pipe_characters_so_cell_values_cannot_break_table_rows() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let outcome =
                outcome_for(temp.path(), "---\ntitle: \"A | B\"\n---");

            let table =
                outcome.table(&["Title"], &["title"]).expect("valid table");

            assert_eq!(table.lines().count(), 3);
            assert!(table.contains("A \\| B"));
        }

        #[test]
        fn escapes_pipe_characters_in_headers_the_same_way_as_cell_values() {
            let table = QueryOutcome::default()
                .table(&["A|B"], &["file.name"])
                .expect("valid table");

            assert_eq!(table, "| A\\|B |\n|------|\n");
        }

        #[test]
        fn collapses_newlines_in_cell_values_to_keep_one_row_per_record() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let outcome = outcome_for(
                temp.path(),
                "---\nnotes: |\n  line one\n  line two\n---",
            );

            let table =
                outcome.table(&["Notes"], &["notes"]).expect("valid table");

            // A literal newline inside the cell value must not split into a
            // second table row: header + separator + exactly one data row.
            assert_eq!(table.lines().count(), 3);
        }

        #[test]
        fn rejects_malformed_field_path() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let outcome = outcome_for(temp.path(), "body");

            assert_eq!(
                outcome.table(&["Name"], &["file.bogus"]),
                Err(QueryError::unknown_field_path("file.bogus", None))
            );
        }

        #[test]
        fn rejects_a_headers_columns_length_mismatch() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let outcome = outcome_for(temp.path(), "body");

            assert_eq!(
                outcome.table(&["Name", "Rating"], &["file.name"]),
                Err(QueryError::TableColumnMismatch {
                    headers: 2,
                    columns: 1,
                })
            );
        }
    }

    mod list {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn renders_one_bullet_per_record() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let outcome = outcome_for_files(temp.path(), &[
                ("a.md", "---\nrating: 5\n---"),
                ("b.md", "---\nrating: 3\n---"),
            ]);

            let list = outcome.list("rating").expect("valid list");

            assert_eq!(list.lines().count(), 2);
            assert!(list.lines().all(|line| line.starts_with("- ")));
            assert!(list.contains('5'));
            assert!(list.contains('3'));
        }

        #[test]
        fn renders_an_empty_string_for_an_empty_outcome() {
            let list =
                QueryOutcome::default().list("rating").expect("valid list");

            assert_eq!(list, "");
        }

        #[test]
        fn rejects_malformed_field_path() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let outcome = outcome_for(temp.path(), "body");

            assert_eq!(
                outcome.list("file.bogus"),
                Err(QueryError::unknown_field_path("file.bogus", None))
            );
        }

        #[test]
        fn renders_a_bool_field_as_true_or_false() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let outcome = outcome_for(temp.path(), "---\nactive: true\n---");

            let list = outcome.list("active").expect("valid list");

            assert_eq!(list, "- true\n");
        }

        #[test]
        fn renders_a_missing_field_as_an_empty_bullet() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let outcome = outcome_for(temp.path(), "body, no frontmatter");

            let list = outcome.list("no_such_field").expect("valid list");

            assert_eq!(list, "- \n");
        }

        #[test]
        fn renders_a_wikilink_field_as_its_target_path() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let outcome = outcome_for(
                temp.path(),
                "---\nlink: \"[[Project Alpha|Alpha]]\"\n---",
            );

            let list = outcome.list("link").expect("valid list");

            assert_eq!(list, "- Project Alpha\n");
        }

        #[test]
        fn renders_an_unflattened_list_field_joined_by_comma_space() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let outcome = outcome_for(
                temp.path(),
                "---\nauthors:\n  - Alice\n  - Bob\n---",
            );

            let list = outcome.list("authors").expect("valid list");

            assert_eq!(list, "- Alice, Bob\n");
        }

        #[test]
        fn renders_an_object_field_as_key_value_pairs_joined_by_comma_space() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let outcome = outcome_for(
                temp.path(),
                "---\nmeta:\n  city: NYC\n  zip: 10001\n---",
            );

            let list = outcome.list("meta").expect("valid list");

            assert_eq!(list, "- city: NYC, zip: 10001\n");
        }
    }

    mod task_list {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn renders_a_checkbox_per_task_matching_completion_state() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(
                temp.path().join("a.md"),
                "- [ ] Buy milk\n- [x] Walk dog\n",
            )
            .expect("write file");
            let outcome = FileIndex::build(temp.path())
                .expect("build index")
                .query_tasks(&QuerySource::All);

            let rendered = outcome.task_list().expect("valid task_list");

            assert_eq!(rendered, "- [ ] Buy milk\n- [x] Walk dog\n");
        }

        #[test]
        fn renders_an_empty_string_for_an_empty_outcome() {
            let rendered =
                QueryOutcome::default().task_list().expect("valid task_list");

            assert_eq!(rendered, "");
        }

        #[test]
        fn rejects_page_level_records_with_no_task_fields() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let outcome = outcome_for(temp.path(), "# Just a Note");

            assert_eq!(
                outcome.task_list(),
                Err(QueryError::TaskListOnPageRecords)
            );
        }
    }

    mod query_outcome {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn reports_len_and_is_empty() {
            let empty = QueryOutcome::default();
            assert!(empty.is_empty());
            assert_eq!(empty.len(), 0);

            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("a.md"), "# A").expect("write file");
            let index = FileIndex::build(temp.path()).expect("build index");
            let outcome = index.query(&QuerySource::All);

            assert!(!outcome.is_empty());
            assert_eq!(outcome.len(), 1);
        }

        #[test]
        fn get_returns_record_or_none() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("a.md"), "# A").expect("write file");
            let index = FileIndex::build(temp.path()).expect("build index");
            let outcome = index.query(&QuerySource::All);

            assert!(outcome.get(0).is_some());
            assert_eq!(outcome.get(1), None);
        }

        #[test]
        fn iter_and_into_iterator_yield_records() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("a.md"), "# A").expect("write file");
            let index = FileIndex::build(temp.path()).expect("build index");
            let outcome = index.query(&QuerySource::All);

            let via_iter: Vec<&IndexRecord> = outcome.iter().collect();
            assert_eq!(via_iter.len(), 1);

            let via_ref_into: Vec<&IndexRecord> =
                (&outcome).into_iter().collect();
            assert_eq!(via_ref_into.len(), 1);

            let via_into: Vec<IndexRecord> = outcome.into_iter().collect();
            assert_eq!(via_into.len(), 1);
        }
    }
}
