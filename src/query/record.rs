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
//!   [`super::QueryService::execute`].
//! - [`QueryRecordSet`] stores result rows and provides chained transformation
//!   methods (`filter`, `sort`, `limit`, `group_by`, `flatten`) and terminal
//!   rendering methods (`table`, `list`, `task_list`).
//! - Task rows carry a small overlay with `task.completed` and `task.text`.
//!
//! # Examples
//!
//! ```ignore
//! use traces_pkm::{IndexerService, QueryRequest, QueryService, SourceSelector};
//!
//! let index = IndexerService::new(".").build().unwrap();
//! let records = QueryService::new("class")
//!     .execute(&index, QueryRequest::pages(SourceSelector::All));
//! ```
//!
//! [`FileRecord`]: crate::file::FileRecord
//! [`Note`]: crate::note::Note

use std::{path::PathBuf, sync::Arc};

use super::{
    QueryError, QueryTransform,
    format::QueryDisplayFormat,
    grammar::{FieldPath, FileField, FilterExpr, TaskField},
    sort::SortKey,
};
use crate::{
    file::FileRecord,
    index::FileIndexEntry,
    note::{Link, ListItem, Note, NoteFieldValue, Tag, TaskStatus},
};

/// A query row pairing a [`FileRecord`] with parsed [`Note`] metadata.
///
/// Each record resolves `file.*`, `task.*`, frontmatter, inline fields, `tags`,
/// and derived inlinks for template rendering and CLI output.
#[derive(Clone, Debug, PartialEq)]
pub struct QueryRecord {
    base: Arc<RecordBase>,
    /// Overrides field resolution for exploded rows produced by
    /// [`QueryRecordSet::flatten`].
    flattened: Vec<(FieldPath, NoteFieldValue)>,
    kind: RowKind,
}

#[derive(Clone, Debug, PartialEq)]
struct RecordBase {
    file: FileRecord,
    note: Option<Arc<Note>>,
    inlinks: Arc<[PathBuf]>,
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
    /// Constructs a new [`QueryRecord`] from an index entry.
    pub(super) fn from_entry(entry: FileIndexEntry<'_>) -> Self {
        Self {
            base: Arc::new(RecordBase {
                file: entry.file().clone(),
                note: entry.note().cloned().map(Arc::new),
                inlinks: Arc::<[PathBuf]>::from(entry.inlinks()),
            }),
            flattened: Vec::new(),
            kind: RowKind::Page,
        }
    }

    /// Converts this record into a task-level row.
    pub(super) fn with_task_item(mut self, item: &ListItem) -> Self {
        let Some(status) = item.task_status() else {
            return self;
        };
        self.kind = RowKind::Task(TaskRow {
            status,
            text: item.text().to_owned(),
        });
        self
    }

    /// Returns task completion state if this is a task-level record, or
    /// `None` for page-level records.
    ///
    /// Returns `true` for `- [x]` and `false` for `- [ ]`.
    #[inline]
    #[must_use]
    pub fn task_completed(&self) -> Option<bool> {
        match &self.kind {
            RowKind::Page => None,
            RowKind::Task(task) => {
                Some(task.status == crate::note::TaskStatus::Complete)
            }
        }
    }

    /// Returns the task item's text if this is a task-level record, or
    /// `None` for page-level records.
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
    pub fn file(&self) -> &FileRecord {
        &self.base.file
    }

    /// Returns parsed [`Note`] metadata for the indexed file, or `None` if the
    /// file has no parsed Note (a non-Markdown file matched by a `file`-typed
    /// Schema field).
    #[inline]
    #[must_use]
    pub(crate) fn note(&self) -> Option<&Note> {
        self.base.note.as_deref()
    }

    /// Returns project-relative paths of Notes whose wikilinks resolve to
    /// this record's Note, or an empty slice if no Notes link to it.
    #[inline]
    #[must_use]
    pub(crate) fn inlinks(&self) -> &[PathBuf] {
        &self.base.inlinks
    }

    /// Resolves a field path string against this record's metadata.
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
                    note.fields()
                        .find(|(k, _)| k.is_match(key.as_str()))
                        .map(|(_, value)| QueryFieldValueRef::from(value))
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
        let file = self.file();
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
            TaskField::Completed => {
                QueryFieldValueRef::Bool(task.status == TaskStatus::Complete)
            }
            TaskField::Text => QueryFieldValueRef::Text(&task.text),
        }
    }
}

/// Borrowed field value resolved from a [`QueryRecord`].
pub(super) enum QueryFieldValueRef<'a> {
    Null,
    Bool(bool),
    Number(f64),
    Text(&'a str),
    Link(&'a Link),
    Date(&'a str),
    Duration(&'a str),
    Object(&'a indexmap::IndexMap<String, NoteFieldValue>),
    List(QueryListValueRef<'a>),
    Owned(NoteFieldValue),
}

impl QueryFieldValueRef<'_> {
    pub(super) fn to_owned_value(&self) -> NoteFieldValue {
        match self {
            Self::Null => NoteFieldValue::Null,
            Self::Bool(value) => NoteFieldValue::Bool(*value),
            Self::Number(value) => NoteFieldValue::Number(*value),
            Self::Text(value) => NoteFieldValue::String((*value).to_owned()),
            Self::Link(value) => NoteFieldValue::Link((*value).clone()),
            Self::Date(value) => NoteFieldValue::Date((*value).to_owned()),
            Self::Duration(value) => {
                NoteFieldValue::Duration((*value).to_owned())
            }
            Self::Object(value) => NoteFieldValue::Object((*value).clone()),
            Self::List(value) => value.to_owned_value(),
            Self::Owned(value) => value.clone(),
        }
    }

    pub(super) fn as_str(&self) -> Option<&str> {
        match self {
            Self::Text(value) | Self::Date(value) | Self::Duration(value) => {
                Some(value)
            }
            Self::Owned(value) => value.as_str(),
            _ => None,
        }
    }
}

impl<'a> From<&'a NoteFieldValue> for QueryFieldValueRef<'a> {
    fn from(value: &'a NoteFieldValue) -> Self {
        match value {
            NoteFieldValue::Null => Self::Null,
            NoteFieldValue::Bool(value) => Self::Bool(*value),
            NoteFieldValue::Number(value) => Self::Number(*value),
            NoteFieldValue::String(value) => Self::Text(value),
            NoteFieldValue::Date(value) => Self::Date(value),
            NoteFieldValue::Duration(value) => Self::Duration(value),
            NoteFieldValue::Link(value) => Self::Link(value),
            NoteFieldValue::List(value) => {
                Self::List(QueryListValueRef::Values(value))
            }
            NoteFieldValue::Object(value) => Self::Object(value),
        }
    }
}

/// Borrowed list value resolved from a [`QueryRecord`].
pub(super) enum QueryListValueRef<'a> {
    Values(&'a [NoteFieldValue]),
    Tags(&'a [Tag]),
    Inlinks(&'a [PathBuf]),
}

impl QueryListValueRef<'_> {
    fn to_owned_value(&self) -> NoteFieldValue {
        match self {
            Self::Values(values) => NoteFieldValue::List((*values).to_vec()),
            Self::Tags(tags) => NoteFieldValue::List(
                tags.iter()
                    .map(|tag| NoteFieldValue::String(tag.as_str().to_owned()))
                    .collect(),
            ),
            Self::Inlinks(inlinks) => NoteFieldValue::List(
                inlinks
                    .iter()
                    .map(|path| {
                        NoteFieldValue::String(
                            path.to_string_lossy().into_owned(),
                        )
                    })
                    .collect(),
            ),
        }
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
    pub(crate) fn filter(self, expr: &str) -> Result<Self, QueryError> {
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
    pub(super) fn r#where(self, expr: &str) -> Result<Self, QueryError> {
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
    ) -> Result<Self, QueryError> {
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
    pub(crate) fn limit(self, n: i64) -> Result<Self, QueryError> {
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
    pub(crate) fn group_by(self, path: &str) -> Result<Self, QueryError> {
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
    pub(crate) fn flatten(self, path: &str) -> Result<Self, QueryError> {
        let transform = QueryTransform::flatten(path)?;
        Ok(self.apply_transform(&transform))
    }

    pub(super) fn apply_transform(self, transform: &QueryTransform) -> Self {
        match transform {
            QueryTransform::Filter(expr) => self.apply_filter(expr),
            QueryTransform::Sort {
                field,
                descending,
            } => self.sort_by_field(field, *descending),
            QueryTransform::Limit(n) => self.limit_to(*n),
            QueryTransform::GroupBy(field) => self.sort_by_field(field, false),
            QueryTransform::Flatten(field) => self.flatten_field(field),
        }
    }

    fn apply_filter(mut self, expr: &FilterExpr) -> Self {
        self.records.retain(|record| expr.is_matching(record));
        self
    }

    fn limit_to(mut self, n: usize) -> Self {
        self.records.truncate(n);
        self
    }

    fn flatten_field(self, field_path: &FieldPath) -> Self {
        let mut records = Vec::with_capacity(self.records.len());
        for record in self.records {
            let NoteFieldValue::List(mut items) =
                record.resolve_owned(field_path)
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
        Self::new(records)
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
        self.format(&QueryDisplayFormat::table(headers, columns))
    }

    /// Renders records as a Markdown bullet list formatting the resolved field
    /// value at `path`.
    ///
    /// # Errors
    ///
    /// - [`QueryError::FieldPath`] if `path` cannot be parsed as a valid field
    ///   path.
    pub(crate) fn list(&self, path: &str) -> Result<String, QueryError> {
        self.format(&QueryDisplayFormat::list(path))
    }

    /// Renders task-level records as a Markdown task list (`- [ ]` or `- [x]`).
    ///
    /// # Errors
    ///
    /// - [`QueryError::TaskListRequiresTaskRows`] if any record lacks task
    ///   fields.
    pub(crate) fn task_list(&self) -> Result<String, QueryError> {
        self.format(&QueryDisplayFormat::task_list())
    }

    /// Sorts records stably by the resolved value of `path`.
    fn sort_by_field(self, field_path: &FieldPath, descending: bool) -> Self {
        let mut records = self.records;
        records.sort_by_cached_key(|record| SortKey {
            value: record.resolve_owned(field_path),
            descending,
        });
        Self::new(records)
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
