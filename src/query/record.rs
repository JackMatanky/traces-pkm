//! Query rows and result set types for query execution.
//!
//! This module implements [`QueryRecord`], which pairs a [`FileBase`] with its
//! parsed [`Note`] and resolves field paths for template rendering and CLI
//! output. Each record resolves `file.*`, `task.*`, frontmatter, inline fields,
//! `tags`, and derived inlinks.
//!
//! # Main Types
//!
//! - [`QueryRecord`] is the primary query row, produced by
//!   [`super::QueryService::execute`].
//! - [`QueryRecordSet`] stores result rows and provides chained transformation
//!   methods (`filter`, `sort`, `limit`, `group_by`, `flatten`) and terminal
//!   rendering methods (`table`, `list`, `task_list`).
//! - Task rows carry a small overlay with `task.completed` and `task.text`.
//!
//! # Examples
//!
//! ```ignore
//! use std::sync::Arc;
//!
//! use traces_pkm::{IndexerService, QueryRequest, QueryService, SourceSelector};
//!
//! let index = Arc::new(IndexerService::new(".").build().unwrap());
//! let records = QueryService::new("class")
//!     .execute(&index, QueryRequest::pages(SourceSelector::All));
//! ```
//!
//! [`FileBase`]: crate::file::FileBase
//! [`Note`]: crate::note::Note

use std::{path::PathBuf, sync::Arc};

use super::{
    QueryResult, QueryTransform,
    format::QueryDisplayFormat,
    grammar::{FieldPath, FileField, TaskField},
    value::{QueryFieldValueRef, QueryListValueRef},
};
use crate::{
    file::FileBase,
    index::{FileEntry, FileIndex, RowIndex},
    note::{ListItem, ListItemType, Note, NoteFieldValue},
    task::TaskStatus,
};

/// A query row over one indexed [`FileEntry`].
///
/// Each record resolves `file.*`, `task.*`, frontmatter, inline fields, `tags`,
/// and derived inlinks for template rendering and CLI output.
#[derive(Clone)]
pub struct QueryRecord {
    index: Arc<FileIndex>,
    position: RowIndex,
    /// Overrides field resolution for exploded rows produced by
    /// [`QueryRecordSet::flatten`].
    flattened: Vec<(FieldPath, NoteFieldValue)>,
    kind: RowKind,
}

#[derive(Clone, Debug, PartialEq)]
enum RowKind {
    Page,
    Task(TaskRow),
}

#[derive(Clone, Debug, PartialEq)]
struct TaskRow {
    status: TaskStatus,
    text: String,
}

impl QueryRecord {
    /// Constructs a new [`QueryRecord`] at `position` in `index`.
    pub(super) fn from_row(index: Arc<FileIndex>, position: RowIndex) -> Self {
        Self {
            index,
            position,
            flattened: Vec::new(),
            kind: RowKind::Page,
        }
    }

    /// Resolves this record's indexed [`FileEntry`].
    fn entry(&self) -> &FileEntry {
        self.index.entry_at(self.position)
    }

    /// Converts this record into a task-level row.
    ///
    /// No-ops for a `Plain` or `Checkbox` item; only [`ListItemType::Task`]
    /// items carry a resolved status to promote.
    pub(super) fn with_task_item(mut self, item: &ListItem) -> Self {
        let ListItemType::Task(status) = item.item_type() else {
            return self;
        };
        self.kind = RowKind::Task(TaskRow {
            status: status.clone(),
            text: item.text().to_owned(),
        });
        self
    }

    /// Returns task completion state if this is a task-level record, or `None`
    /// for page-level records or a cancelled task.
    ///
    /// `Some(true)` for a done task, `Some(false)` for an incomplete one,
    /// `None` for a cancelled task or a page-level record.
    #[inline]
    #[must_use]
    pub fn task_completed(&self) -> Option<bool> {
        match &self.kind {
            RowKind::Page => None,
            RowKind::Task(task) => task.status.kind().completed(),
        }
    }

    /// Returns the task item's text if this is a task-level record, or `None`
    /// for page-level records.
    #[inline]
    #[must_use]
    pub(crate) fn task_text(&self) -> Option<&str> {
        match &self.kind {
            RowKind::Page => None,
            RowKind::Task(task) => Some(task.text.as_str()),
        }
    }

    /// Returns general metadata for the indexed file.
    #[inline]
    #[must_use]
    pub fn base(&self) -> &FileBase {
        self.entry().base()
    }

    /// Returns parsed [`Note`] metadata for the indexed file, or `None` if the
    /// file has no parsed Note (a non-Markdown file matched by a `file`-typed
    /// Schema field).
    #[inline]
    #[must_use]
    pub(crate) fn note(&self) -> Option<&Note> {
        self.entry().note()
    }

    /// Returns project-relative paths of Notes whose wikilinks resolve to this
    /// record's Note, or an empty slice if no Notes link to it.
    #[inline]
    #[must_use]
    pub(crate) fn inlinks(&self) -> &[PathBuf] {
        self.entry().inlinks()
    }

    /// Resolves a field path string against this record's metadata.
    ///
    /// # Errors
    ///
    /// - [`QueryError::FieldPath`] if `path` cannot be parsed as a valid field
    ///   path.
    #[inline]
    pub(crate) fn field(&self, path: &str) -> QueryResult<NoteFieldValue> {
        Ok(self.resolve_owned(&FieldPath::parse(path)?))
    }

    /// Resolves a pre-parsed field path into a borrowed value where possible.
    pub(super) fn resolve_ref(
        &self,
        path: &FieldPath,
    ) -> QueryFieldValueRef<'_> {
        if let Some((_, value)) = self.flattened.iter().find(|(p, _)| p == path)
        {
            return QueryFieldValueRef::from(value);
        }
        match path {
            FieldPath::File(field) => self.resolve_file_ref(*field),
            FieldPath::Task(field) => self.resolve_task_ref(*field),
            FieldPath::Tags => {
                let tags = self.note().map_or(&[][..], Note::tags);
                QueryFieldValueRef::List(QueryListValueRef::Tags(tags))
            }
            FieldPath::Inlinks => QueryFieldValueRef::List(
                QueryListValueRef::Inlinks(self.inlinks()),
            ),
            FieldPath::Metadata(key) => self
                .note()
                .and_then(|note| {
                    note.get(key.as_str()).map(QueryFieldValueRef::from)
                })
                .unwrap_or(QueryFieldValueRef::Null),
        }
    }

    /// Resolves a pre-parsed field path into the public owned value type.
    pub(crate) fn resolve_owned(&self, path: &FieldPath) -> NoteFieldValue {
        self.resolve_ref(path).to_owned_value()
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

    fn resolve_file_ref(&self, field: FileField) -> QueryFieldValueRef<'_> {
        let file = self.base();
        match field {
            FileField::Path => file.path().to_str().map_or_else(
                || {
                    QueryFieldValueRef::Owned(NoteFieldValue::String(
                        file.path().to_string_lossy().into_owned(),
                    ))
                },
                QueryFieldValueRef::Text,
            ),
            FileField::Name => QueryFieldValueRef::Text(file.name().as_str()),
            FileField::Folder => file.folder().to_str().map_or_else(
                || {
                    QueryFieldValueRef::Owned(NoteFieldValue::String(
                        file.folder().to_string_lossy().into_owned(),
                    ))
                },
                QueryFieldValueRef::Text,
            ),
            #[expect(
                clippy::as_conversions,
                clippy::cast_precision_loss,
                reason = "file sizes stay well under 2^53 bytes for PKM-scale \
                          projects, so f64 keeps exact byte counts"
            )]
            FileField::Size => QueryFieldValueRef::Number(file.size() as f64),
            FileField::CreatedDateTime => {
                QueryFieldValueRef::Owned(NoteFieldValue::Date(
                    file.created_at_or_modified().to_datetime_string(),
                ))
            }
            FileField::CreatedDate => {
                QueryFieldValueRef::Owned(NoteFieldValue::Date(
                    file.created_at_or_modified().to_date_string(),
                ))
            }
            FileField::ModifiedDateTime => QueryFieldValueRef::Owned(
                NoteFieldValue::Date(file.modified_at().to_datetime_string()),
            ),
            FileField::ModifiedDate => QueryFieldValueRef::Owned(
                NoteFieldValue::Date(file.modified_at().to_date_string()),
            ),
        }
    }

    fn resolve_task_ref(&self, field: TaskField) -> QueryFieldValueRef<'_> {
        let RowKind::Task(task) = &self.kind else {
            return QueryFieldValueRef::Null;
        };
        match field {
            TaskField::Completed => match task.status.kind().completed() {
                Some(completed) => QueryFieldValueRef::Bool(completed),
                None => QueryFieldValueRef::Null,
            },
            TaskField::Text => QueryFieldValueRef::Text(&task.text),
        }
    }
}

impl PartialEq for QueryRecord {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        self.entry() == other.entry()
            && self.flattened == other.flattened
            && self.kind == other.kind
    }
}

impl std::fmt::Debug for QueryRecord {
    #[inline]
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("QueryRecord")
            .field("position", &self.position)
            .field("entry", self.entry())
            .field("flattened", &self.flattened)
            .field("kind", &self.kind)
            .finish_non_exhaustive()
    }
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
    /// - [`QueryError::Request`] if the expression is invalid.
    #[inline]
    pub(crate) fn filter(self, expr: &str) -> QueryResult<Self> {
        let transform = QueryTransform::filter(expr)?;
        Ok(self.apply_transform(&transform))
    }

    /// Filters records matching `expr`, serving as an alias for
    /// [`Self::filter`].
    ///
    /// # Errors
    ///
    /// - [`QueryError::Request`] if the expression is invalid.
    #[inline]
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "no current caller outside tests; Rust-side alias for \
                      direct callers of this crate's Rust API"
        )
    )]
    pub(super) fn r#where(self, expr: &str) -> QueryResult<Self> {
        self.filter(expr)
    }

    /// Sorts records by the field at `path` in ascending or descending order.
    ///
    /// # Errors
    ///
    /// - [`QueryError::Request`] if `path` cannot be parsed as a valid field
    ///   path.
    #[inline]
    pub(crate) fn sort(
        self,
        path: &str,
        descending: bool,
    ) -> QueryResult<Self> {
        let transform = QueryTransform::sort(path, descending)?;
        Ok(self.apply_transform(&transform))
    }

    /// Truncates the outcome to retain at most `n` leading records.
    ///
    /// # Errors
    ///
    /// - [`QueryError::Request`] if `n` is negative or exceeds platform
    ///   pointer-width limits.
    #[inline]
    pub(crate) fn limit(self, n: i64) -> QueryResult<Self> {
        let transform = QueryTransform::limit(n)?;
        Ok(self.apply_transform(&transform))
    }

    /// Groups records by sorting them ascending on the field at `path`.
    ///
    /// # Errors
    ///
    /// - [`QueryError::Request`] if `path` cannot be parsed as a valid field
    ///   path.
    #[inline]
    pub(crate) fn group_by(self, path: &str) -> QueryResult<Self> {
        let transform = QueryTransform::group_by(path)?;
        Ok(self.apply_transform(&transform))
    }

    /// Explodes records containing a list at `path` into one row per list
    /// element.
    ///
    /// # Errors
    ///
    /// - [`QueryError::Request`] if `path` cannot be parsed as a valid field
    ///   path.
    pub(crate) fn flatten(self, path: &str) -> QueryResult<Self> {
        let transform = QueryTransform::flatten(path)?;
        Ok(self.apply_transform(&transform))
    }

    /// Applies one already-parsed transform. Used by [`Self::filter`]/
    /// [`Self::sort`]/[`Self::limit`]/[`Self::group_by`]/[`Self::flatten`]
    /// (Minijinja's eager per-call chaining) and, via a whole
    /// [`super::QueryPlan`], by [`super::QueryService::execute`]. All
    /// per-step logic lives on [`QueryTransform::apply`].
    pub(super) fn apply_transform(self, transform: &QueryTransform) -> Self {
        Self::new(transform.apply(self.records))
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
    ) -> QueryResult<String> {
        self.format(&QueryDisplayFormat::table(headers, columns))
    }

    /// Renders records as a Markdown bullet list formatting the resolved field
    /// value at `path`.
    ///
    /// # Errors
    ///
    /// - [`QueryError::FieldPath`] if `path` cannot be parsed as a valid field
    ///   path.
    pub(crate) fn list(&self, path: &str) -> QueryResult<String> {
        self.format(&QueryDisplayFormat::list(path))
    }

    /// Renders task-level records as a Markdown task list (`- [ ]` or `- [x]`).
    ///
    /// # Errors
    ///
    /// - [`QueryError::TaskListRequiresTaskRows`] if any record lacks task
    ///   fields.
    pub(crate) fn task_list(&self) -> QueryResult<String> {
        self.format(&QueryDisplayFormat::task_list())
    }

    /// Renders records using the given display format.
    ///
    /// # Errors
    ///
    /// Returns existing query errors for malformed field paths, table column
    /// mismatches, or task-list rendering on page rows.
    pub(super) fn format(
        &self,
        format: &QueryDisplayFormat,
    ) -> QueryResult<String> {
        format.render(&self.records)
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
