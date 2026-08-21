//! Query rows and result set types for [`QueryRecordSet`].
//!
//! This module implements [`QueryRecord`], which pairs a [`FileRecord`] with
//! its parsed [`Note`] and resolves field paths for template rendering and CLI
//! output. Each record resolves `file.*`, `task.*`, frontmatter, inline fields,
//! `tags`, and derived inlinks.
//!
//! # Main Types
//!
//! - [`QueryRecord`] is the primary query row, produced by
//!   [`super::QueryService::query`] or [`super::QueryService::query_tasks`].
//! - [`QueryRecordSet`] stores result rows and provides chained transformation
//!   methods (`filter`, `sort`, `limit`, `group_by`, `flatten`) and terminal
//!   rendering methods (`table`, `list`, `task_list`).
//! - [`TaskInfo`] carries per-task fields layered onto a [`QueryRecord`] by
//!   [`super::QueryService::query_tasks`].
//!
//! # Examples
//!
//! ```ignore
//! use std::path::Path;
//!
//! use traces_pkm::{file::FileRecord, note::Note, query::QueryRecord};
//!
//! let note = Note::default();
//! let record = QueryRecord::new(file, Some(note));
//!
//! assert_eq!(record.file().path().to_str(), Some("note.md"));
//! ```
//!
//! [`FileRecord`]: crate::file::FileRecord
//! [`Note`]: crate::note::Note

use std::{path::PathBuf, sync::Arc};

use super::{
    QueryError,
    field::{FieldPath, TaskField},
    filter::FilterExpr,
    sort::SortKey,
};
use crate::{
    file::FileRecord,
    index::InlinkMap,
    note::{Note, NoteFieldValue},
};

/// A query row pairing a [`FileRecord`] with parsed [`Note`] metadata.
///
/// Each record resolves `file.*`, `task.*`, frontmatter, inline fields, `tags`,
/// and derived inlinks for template rendering and CLI output.
///
/// The [`Note`] is reference-counted via [`Arc`] to share data efficiently when
/// expanding one note into multiple task-level rows.
///
/// # Examples
///
/// ```ignore
/// use std::path::Path;
///
/// use traces_pkm::{file::FileRecord, note::Note, query::QueryRecord};
///
/// let note = Note::default();
/// let record = QueryRecord::new(file, Some(note));
/// ```
#[derive(Clone, Debug, PartialEq)]
pub struct QueryRecord {
    file: FileRecord,
    /// Reference-counted, not owned outright: exploding one Note into several
    /// rows (see [`super::QueryService::query_tasks`] and
    /// [`QueryRecordSet::flatten`]) shares this field across every row
    /// instead of deep-cloning frontmatter, links, tags, and lists per row.
    ///
    /// `None` when the underlying file has no parsed [`Note`] (for example, an
    /// image or PDF referenced by a `file`-typed Schema field).
    note: Option<Arc<Note>>,
    /// Overrides field resolution for exploded rows produced by
    /// [`QueryRecordSet::flatten`].
    flattened: Vec<(FieldPath, NoteFieldValue)>,
    /// Stores per-task fields set by [`super::QueryService::query_tasks`], or
    /// `None` for page-level records.
    task: Option<TaskInfo>,
    /// Stores project-relative paths of Notes whose outlinks resolve to this
    /// row's Note, set by [`super::QueryService::query`] and
    /// [`super::QueryService::query_tasks`].
    inlinks: Vec<PathBuf>,
}

impl QueryRecord {
    /// Creates a new [`QueryRecord`] pairing `file` with its parsed `note`.
    ///
    /// `note` is `None` for files with no parsed [`Note`] (non-Markdown files
    /// matched by a `file`-typed Schema field).
    pub(super) fn new(file: FileRecord, note: Option<Note>) -> Self {
        Self {
            file,
            note: note.map(Arc::new),
            flattened: Vec::new(),
            task: None,
            inlinks: Vec::new(),
        }
    }

    /// Constructs a new [`QueryRecord`] pairing `file` with `note`, with
    /// inbound link paths populated from `inlinks` (removing `file`'s entry).
    ///
    /// Consolidates every construction path used while assembling a query
    /// pass — see [`super::QueryService::query`]/`query_tasks`.
    pub(super) fn from_parts(
        file: FileRecord,
        note: Option<Note>,
        inlinks: &mut InlinkMap,
    ) -> Self {
        let links = inlinks.remove(file.path()).unwrap_or_default();
        Self::new(file, note).with_inlinks(links)
    }

    /// Converts this record into a task-level row.
    ///
    /// Attaches task completion state and text, used by
    /// [`super::QueryService::query_tasks`] to expand a page-level record
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

    /// Attaches project-relative paths of Notes whose wikilinks resolve to
    /// this record's Note.
    pub(super) fn with_inlinks(mut self, inlinks: Vec<PathBuf>) -> Self {
        self.inlinks = inlinks;
        self
    }

    /// Returns task completion state if this is a task-level record, or
    /// `None` for page-level records.
    ///
    /// Returns `true` for `- [x]` and `false` for `- [ ]`.
    #[inline]
    #[must_use]
    pub fn task_completed(&self) -> Option<bool> {
        self.task.as_ref().map(|task| task.completed)
    }

    /// Returns the task item's text if this is a task-level record, or
    /// `None` for page-level records.
    #[inline]
    #[must_use]
    pub(crate) fn task_text(&self) -> Option<&str> {
        self.task.as_ref().map(|task| task.text.as_str())
    }

    /// Returns general metadata for the indexed file.
    #[inline]
    #[must_use]
    pub const fn file(&self) -> &FileRecord {
        &self.file
    }

    /// Returns parsed [`Note`] metadata for the indexed file, or `None` if the
    /// file has no parsed Note (a non-Markdown file matched by a `file`-typed
    /// Schema field).
    ///
    /// The returned reference shares the underlying [`Arc`] allocation with
    /// any task-level rows derived from the same Note.
    #[inline]
    #[must_use]
    pub(crate) fn note(&self) -> Option<&Note> {
        self.note.as_deref()
    }

    /// Returns project-relative paths of Notes whose wikilinks resolve to
    /// this record's Note, or an empty slice if no Notes link to it.
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
    ///   accessor on a page-level record) resolve to [`NoteFieldValue::Null`].
    ///
    /// # Errors
    ///
    /// - [`QueryError::FieldPath`] if `path` cannot be parsed as a valid field
    ///   path.
    #[inline]
    pub(crate) fn field(
        &self,
        path: &str,
    ) -> Result<NoteFieldValue, QueryError> {
        Ok(self.resolve(&FieldPath::parse(path)?))
    }

    /// Resolves a pre-parsed field path against this record, applying
    /// overrides.
    pub(super) fn resolve(&self, path: &FieldPath) -> NoteFieldValue {
        if let Some((_, value)) = self.flattened.iter().find(|(p, _)| p == path)
        {
            return value.clone();
        }
        match path {
            FieldPath::File(field) => field.resolve(&self.file),
            FieldPath::Task(field) => {
                self.task.as_ref().map_or(NoteFieldValue::Null, |task| {
                    match field {
                        TaskField::Completed => {
                            NoteFieldValue::Bool(task.completed)
                        }
                        TaskField::Text => {
                            NoteFieldValue::String(task.text.clone())
                        }
                    }
                })
            }
            FieldPath::Tags => NoteFieldValue::List(
                self.note
                    .iter()
                    .flat_map(|note| note.tags())
                    .map(|tag| NoteFieldValue::String(tag.as_str().to_owned()))
                    .collect(),
            ),
            FieldPath::Inlinks => NoteFieldValue::List(
                self.inlinks
                    .iter()
                    .map(|linking_note| {
                        NoteFieldValue::String(
                            linking_note.to_string_lossy().into_owned(),
                        )
                    })
                    .collect(),
            ),
            FieldPath::Metadata(key) => self
                .note
                .as_deref()
                .and_then(|note| {
                    note.fields()
                        .find(|(k, _)| k.is_match(key.as_str()))
                        .map(|(_, v)| v.clone())
                })
                .unwrap_or(NoteFieldValue::Null),
        }
    }

    /// Returns a copy of this record with `path` overridden to `value`.
    ///
    /// Used by [`QueryRecordSet::flatten`] to set the resolved value for
    /// exploded list rows. If `path` already has an override, the value is
    /// updated in place.
    pub(super) fn with_flattened(
        mut self,
        path: FieldPath,
        value: NoteFieldValue,
    ) -> Self {
        if let Some(entry) = self.flattened.iter_mut().find(|(p, _)| p == &path)
        {
            entry.1 = value;
        } else {
            self.flattened.push((path, value));
        }
        self
    }
}

/// Per-task fields layered onto a [`QueryRecord`] by
/// [`super::QueryService::query_tasks`].
///
/// Task-level rows retain parent [`Note`] file and metadata fields for
/// filtering and display while attaching task completion and text. This is
/// distinct from [`QueryRecord::flattened`], which overrides existing field
/// paths rather than adding new ones.
#[derive(Clone, Debug, PartialEq)]
struct TaskInfo {
    completed: bool,
    text: String,
}

/// An ordered collection of [`QueryRecord`] rows produced by an index query.
///
/// Page-level outcomes contain one row per Note, while task-level outcomes
/// contain one row per task item. Transformation methods consume and return
/// a [`QueryRecordSet`], enabling method chaining.
///
/// # Examples
///
/// ```ignore
/// use traces_pkm::query::QueryRecordSet;
///
/// let outcome = QueryRecordSet::default();
/// assert!(outcome.is_empty());
/// ```
#[must_use]
#[derive(Clone, Debug, Default, PartialEq)]
pub struct QueryRecordSet {
    records: Vec<QueryRecord>,
}

impl QueryRecordSet {
    /// Wraps `records` into a new [`QueryRecordSet`].
    pub(super) const fn new(records: Vec<QueryRecord>) -> Self {
        Self {
            records,
        }
    }

    /// Returns the number of [`QueryRecord`] rows in this record set.
    #[inline]
    #[must_use]
    pub const fn len(&self) -> usize {
        self.records.len()
    }

    /// Returns `true` if this record set contains no [`QueryRecord`] rows.
    #[inline]
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// Returns a reference to the [`QueryRecord`] at `index`, or `None` if out
    /// of bounds.
    #[inline]
    #[must_use]
    pub fn get(&self, index: usize) -> Option<&QueryRecord> {
        self.records.get(index)
    }

    /// Returns an iterator over references to the contained [`QueryRecord`]
    /// rows.
    #[inline]
    pub(crate) fn iter(&self) -> std::slice::Iter<'_, QueryRecord> {
        self.records.iter()
    }

    /// Retains only records matching the filter expression.
    ///
    /// # Errors
    ///
    /// - [`QueryError::Syntax`] if the expression is invalid.
    /// - [`QueryError::FieldPath`] if a field path is malformed.
    #[inline]
    pub fn filter(mut self, expr: &str) -> Result<Self, QueryError> {
        let expr = FilterExpr::parse(expr)?;
        self.records.retain(|record| expr.matches(record));
        Ok(self)
    }

    /// Filters records matching `expr`, serving as an alias for
    /// [`Self::filter`].
    ///
    /// # Errors
    ///
    /// - [`QueryError::Syntax`] if the expression is invalid.
    /// - [`QueryError::FieldPath`] if a field path is malformed.
    #[inline]
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "no current caller outside tests; Rust-side alias for \
                      direct callers of this crate's Rust API"
        )
    )]
    pub(crate) fn r#where(self, expr: &str) -> Result<Self, QueryError> {
        self.filter(expr)
    }

    /// Sorts records by the field at `path` in ascending or descending order.
    ///
    /// # Errors
    ///
    /// - [`QueryError::FieldPath`] if `path` cannot be parsed as a valid field
    ///   path.
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
    /// - [`QueryError::LimitOutOfRange`] if `n` is negative or exceeds platform
    ///   pointer-width limits.
    #[inline]
    pub fn limit(mut self, n: i64) -> Result<Self, QueryError> {
        let n = usize::try_from(n).map_err(|_source| {
            QueryError::LimitOutOfRange {
                value: n,
            }
        })?;
        self.records.truncate(n);
        Ok(self)
    }

    /// Groups records by sorting them ascending on the field at `path`.
    ///
    /// # Errors
    ///
    /// - [`QueryError::FieldPath`] if `path` cannot be parsed as a valid field
    ///   path.
    #[inline]
    pub(crate) fn group_by(self, path: &str) -> Result<Self, QueryError> {
        self.sort_by_field(path, false)
    }

    /// Explodes records containing a list at `path` into one row per list
    /// element.
    ///
    /// # Errors
    ///
    /// - [`QueryError::FieldPath`] if `path` cannot be parsed as a valid field
    ///   path.
    pub(crate) fn flatten(self, path: &str) -> Result<Self, QueryError> {
        let field_path = FieldPath::parse(path)?;
        let mut records = Vec::with_capacity(self.records.len());
        for record in self.records {
            let NoteFieldValue::List(mut items) = record.resolve(&field_path)
            else {
                records.push(record);
                continue;
            };
            let Some(last) = items.pop() else {
                continue;
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
    /// # Errors
    ///
    /// - [`QueryError::TableColumnCountMismatch`] if `headers` and `columns`
    ///   slices differ in length.
    /// - [`QueryError::FieldPath`] if any field path string in `columns` is
    ///   malformed.
    pub(crate) fn table(
        &self,
        headers: &[&str],
        columns: &[&str],
    ) -> Result<String, QueryError> {
        if headers.len() != columns.len() {
            return Err(QueryError::TableColumnCountMismatch {
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
    /// - [`QueryError::FieldPath`] if `path` cannot be parsed as a valid field
    ///   path.
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
    /// # Errors
    ///
    /// - [`QueryError::TaskListRequiresTaskRows`] if any record lacks task
    ///   fields.
    pub(crate) fn task_list(&self) -> Result<String, QueryError> {
        let mut out = String::new();
        for record in &self.records {
            let Some(completed) = record.task_completed() else {
                return Err(QueryError::TaskListRequiresTaskRows);
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

/// Converts the [`QueryRecordSet`] into an iterator over owned [`QueryRecord`]
/// rows.
impl IntoIterator for QueryRecordSet {
    type IntoIter = std::vec::IntoIter<Self::Item>;
    type Item = QueryRecord;

    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        self.records.into_iter()
    }
}

/// Creates an iterator over borrowed [`QueryRecord`] rows from the
/// [`QueryRecordSet`].
impl<'a> IntoIterator for &'a QueryRecordSet {
    type IntoIter = std::slice::Iter<'a, QueryRecord>;
    type Item = &'a QueryRecord;

    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        self.records.iter()
    }
}

/// Converts a resolved [`NoteFieldValue`] to plain text for list and table
/// rendering.
fn field_text(value: &NoteFieldValue) -> String {
    match value {
        NoteFieldValue::Null => String::new(),
        NoteFieldValue::Bool(b) => b.to_string(),
        NoteFieldValue::Number(n) => n.to_string(),
        NoteFieldValue::String(s)
        | NoteFieldValue::Date(s)
        | NoteFieldValue::Duration(s) => s.clone(),
        NoteFieldValue::Link(link) => link.target().to_owned(),
        NoteFieldValue::List(items) => {
            items.iter().map(field_text).collect::<Vec<_>>().join(", ")
        }
        NoteFieldValue::Object(fields) => fields
            .iter()
            .map(|(key, field)| format!("{key}: {}", field_text(field)))
            .collect::<Vec<_>>()
            .join(", "),
    }
}

/// Escapes pipes (`|`) and collapses newlines to spaces to preserve table
/// formatting.
fn escape_table_text(text: &str) -> String {
    text.replace('\n', " ").replace('|', "\\|")
}

/// Formats a [`NoteFieldValue`] into plain text suitable for Markdown table
/// cells.
fn table_cell_text(value: &NoteFieldValue) -> String {
    escape_table_text(&field_text(value))
}
