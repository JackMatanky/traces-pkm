//! Query rows and result set types for query execution.
//!
//! This module implements [`QueryRow`], which pairs a [`FileBase`] with its
//! parsed [`Note`] and resolves field paths for template rendering and CLI
//! output, and [`QuerySet`], a memoized CTE table supporting chained
//! transformations and terminal rendering.
//!
//! # Main Types
//!
//! - [`QueryRow`] is the primary query row produced by
//!   [`super::QueryService::execute`].
//! - [`QuerySet`] stores result rows and provides chained transformation
//!   methods ([`filter`](QuerySet::filter), [`sort`](QuerySet::sort),
//!   [`limit`](QuerySet::limit), [`group_by`](QuerySet::group_by),
//!   [`flatten`](QuerySet::flatten)) and terminal rendering methods
//!   ([`table`](QuerySet::table), [`list`](QuerySet::list),
//!   [`task_list`](QuerySet::task_list)).
//!
//! # Examples
//!
//! ```rust
//! use traces_pkm::QuerySet;
//! let set = QuerySet::default();
//! assert!(set.is_empty());
//! assert_eq!(set.len(), 0);
//! ```
//!
//! [`FileBase`]: crate::file::FileBase
//! [`Note`]: crate::note::Note
use std::{path::PathBuf, sync::Arc};

use super::{
    QueryPlan, QueryResult, QueryTransform,
    format::{QueryDisplayFormat, TaskPathStyle},
    grammar::{FieldPath, FileField, TaskField},
    value::{QueryFieldValueRef, QueryListValueRef},
};
use crate::{
    file::FileBase,
    index::{FileEntry, FileIndex, RowIndex},
    note::{ListItem, ListItemType, Note, NoteFieldValue},
    task::TaskStatus,
};

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

/// A query row over one indexed [`FileEntry`].
///
/// `QueryRow` pairs an indexed file entry with optional task item metadata or
/// exploded field overrides. It resolves `file.*`, `task.*`, frontmatter,
/// inline fields, `tags`, and derived inlinks for template rendering and CLI
/// output.
#[derive(Clone)]
pub struct QueryRow {
    index: Arc<FileIndex>,
    position: RowIndex,
    /// Overrides field resolution for exploded rows produced by
    /// [`QuerySet::flatten`].
    flattened: Vec<(FieldPath, NoteFieldValue)>,
    kind: RowKind,
}

impl QueryRow {
    /// Constructs a new [`QueryRow`] at `position` in `index`.
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

    /// Returns task completion state if this row represents a task item, or
    /// `None`.
    ///
    /// Returns `Some(true)` for completed tasks, `Some(false)` for incomplete
    /// tasks, and `None` for page-level records or cancelled tasks.
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

    /// Returns general file metadata for the underlying indexed note.
    #[inline]
    #[must_use]
    pub fn file(&self) -> &FileBase {
        self.entry().file()
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
    /// - [`FieldPath`] if `path` cannot be parsed as a valid field path.
    ///
    /// [`FieldPath`]: super::QueryError::FieldPath
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
    /// Used by [`QuerySet::flatten`] to set the resolved value for
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
            TaskField::Completed => match task.status.kind().completed() {
                Some(completed) => QueryFieldValueRef::Bool(completed),
                None => QueryFieldValueRef::Null,
            },
            TaskField::Text => QueryFieldValueRef::Text(&task.text),
        }
    }
}

impl PartialEq for QueryRow {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        self.entry() == other.entry()
            && self.flattened == other.flattened
            && self.kind == other.kind
    }
}

impl std::fmt::Debug for QueryRow {
    #[inline]
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("QueryRow")
            .field("position", &self.position)
            .field("entry", self.entry())
            .field("flattened", &self.flattened)
            .field("kind", &self.kind)
            .finish_non_exhaustive()
    }
}

/// An ordered, memoized collection of [`QueryRow`] rows produced by an index
/// query.
///
/// `QuerySet` acts as a common table expression (CTE) result set:
/// transformation methods ([`filter`](Self::filter), [`sort`](Self::sort),
/// [`limit`](Self::limit), [`group_by`](Self::group_by),
/// [`flatten`](Self::flatten)) append transformations in $O(1)$ time to a
/// pending plan. Execution occurs lazily on first read ([`len`](Self::len),
/// [`get`](Self::get), [`iter`](Self::iter), or any terminal renderer),
/// memoizing the result for all subsequent reads and branch evaluations.
///
/// # Examples
///
/// ```rust
/// use traces_pkm::QuerySet;
/// let set = QuerySet::default();
/// assert!(set.is_empty());
/// ```
#[must_use]
#[derive(Clone, Default)]
pub struct QuerySet {
    records: Arc<[QueryRow]>,
    plan: QueryPlan,
    cache: std::sync::OnceLock<Arc<[QueryRow]>>,
}

impl QuerySet {
    /// Wraps `records` into a new [`QuerySet`] with no pending
    /// transforms.
    pub(super) fn new(records: Vec<QueryRow>) -> Self {
        Self {
            records: records.into(),
            plan: QueryPlan::default(),
            cache: std::sync::OnceLock::new(),
        }
    }

    /// Returns this query set's rows with every pending transform applied,
    /// computing and memoizing the result on first access. Every read (`len`,
    /// `get`, `iter`, the minijinja `Object` iteration methods, and the
    /// terminal renderers) goes through this, so a chain of
    /// `.where()/.sort()/.limit()/...` calls pays for [`QueryPlan::run`] at
    /// most once, however many times the resulting rows are read.
    fn materialized(&self) -> &Arc<[QueryRow]> {
        self.cache.get_or_init(|| {
            if self.plan.is_empty() {
                Arc::clone(&self.records)
            } else {
                Arc::from(self.plan.clone().run(self.records.to_vec()))
            }
        })
    }

    /// Returns the number of [`QueryRow`] rows in this query set.
    #[inline]
    #[must_use]
    pub fn len(&self) -> usize {
        self.materialized().len()
    }

    /// Returns `true` if this query set contains no [`QueryRow`] rows.
    #[inline]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.materialized().is_empty()
    }

    /// Returns a reference to the [`QueryRow`] at `index`, or `None` if
    /// out of bounds.
    #[inline]
    #[must_use]
    pub fn get(&self, index: usize) -> Option<&QueryRow> {
        self.materialized().get(index)
    }

    /// Returns an iterator over references to the contained [`QueryRow`]
    /// rows.
    #[inline]
    pub fn iter(&self) -> std::slice::Iter<'_, QueryRow> {
        self.materialized().iter()
    }

    /// Appends `transform` to this query set's pending plan, returning a
    /// new [`QuerySet`] over the same base rows. Cheap: moves the
    /// `Arc` (no refcount bump; `self` is consumed) and the short
    /// transform-step list — nothing is evaluated until [`Self::materialized`]
    /// runs on read.
    fn push(self, transform: QueryTransform) -> Self {
        let mut plan = self.plan;
        plan.push(transform);
        Self {
            records: self.records,
            plan,
            cache: std::sync::OnceLock::new(),
        }
    }

    /// Retains only rows matching the filter expression `expr`.
    ///
    /// Appends a filter step to the pending transformation plan.
    ///
    /// # Errors
    ///
    /// - [`Request`] if `expr` is an invalid filter expression.
    ///
    /// [`Request`]: super::QueryError::Request
    #[inline]
    pub(crate) fn filter(self, expr: &str) -> QueryResult<Self> {
        Ok(self.push(QueryTransform::filter(expr)?))
    }

    /// Filters rows matching `expr`, serving as a Rust-side alias for
    /// [`Self::filter`].
    ///
    /// # Errors
    ///
    /// - [`Request`] if `expr` is an invalid filter expression.
    ///
    /// [`Request`]: super::QueryError::Request
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

    /// Sorts rows by the field at `path` in ascending or descending order.
    ///
    /// # Errors
    ///
    /// - [`Request`] if `path` cannot be parsed as a valid field path.
    ///
    /// [`Request`]: super::QueryError::Request
    #[inline]
    pub(crate) fn sort(
        self,
        path: &str,
        descending: bool,
    ) -> QueryResult<Self> {
        Ok(self.push(QueryTransform::sort(path, descending)?))
    }

    /// Retains at most `n` leading rows from the result set.
    ///
    /// # Errors
    ///
    /// - [`Request`] if `n` is negative or exceeds platform pointer-width
    ///   limits (`usize::MAX`).
    ///
    /// [`Request`]: super::QueryError::Request
    #[inline]
    pub(crate) fn limit(self, n: i64) -> QueryResult<Self> {
        Ok(self.push(QueryTransform::limit(n)?))
    }

    /// Groups rows by sorting them ascending on the field at `path`.
    ///
    /// # Errors
    ///
    /// - [`Request`] if `path` cannot be parsed as a valid field path.
    ///
    /// [`Request`]: super::QueryError::Request
    #[inline]
    pub(crate) fn group_by(self, path: &str) -> QueryResult<Self> {
        Ok(self.push(QueryTransform::group_by(path)?))
    }

    /// Explodes rows containing a list at `path` into one row per list element.
    ///
    /// # Errors
    ///
    /// - [`Request`] if `path` cannot be parsed as a valid field path.
    ///
    /// [`Request`]: super::QueryError::Request
    pub(crate) fn flatten(self, path: &str) -> QueryResult<Self> {
        Ok(self.push(QueryTransform::flatten(path)?))
    }

    /// Renders rows as a Markdown table matching headers to corresponding
    /// column field paths.
    ///
    /// # Errors
    ///
    /// - [`TableColumnCountMismatch`] if `headers` and `columns` differ in
    ///   length.
    /// - [`FieldPath`] if any field path in `columns` is malformed.
    ///
    /// [`TableColumnCountMismatch`]: super::QueryError::TableColumnCountMismatch
    /// [`FieldPath`]: super::QueryError::FieldPath
    pub(crate) fn table(
        &self,
        headers: &[&str],
        columns: &[&str],
    ) -> QueryResult<String> {
        self.format(&QueryDisplayFormat::table(headers, columns))
    }

    /// Renders rows as a Markdown bullet list formatting the resolved field
    /// value at `path`.
    ///
    /// # Errors
    ///
    /// - [`FieldPath`] if `path` cannot be parsed as a valid field path.
    ///
    /// [`FieldPath`]: super::QueryError::FieldPath
    pub(crate) fn list(&self, path: &str) -> QueryResult<String> {
        self.format(&QueryDisplayFormat::list(path))
    }

    /// Renders task-level rows as a Markdown task list (`- [ ]`/`- [x]`/`-
    /// [-]`).
    ///
    /// `path_style` controls whether each row's file path is appended in
    /// parentheses: [`TaskPathStyle::None`] for templates,
    /// [`TaskPathStyle::Suffix`] for `traces task`.
    ///
    /// - [`TaskListRequiresTaskRows`] if any row is a page-level row rather
    ///   than a task row.
    ///
    /// [`TaskListRequiresTaskRows`]: super::QueryError::TaskListRequiresTaskRows
    pub(crate) fn task_list(
        &self,
        path_style: TaskPathStyle,
    ) -> QueryResult<String> {
        self.format(&QueryDisplayFormat::task_list(path_style))
    }

    /// Renders records using the given display format.
    ///
    /// # Errors
    ///
    /// - [`TableColumnCountMismatch`] if table headers and columns differ in
    ///   length.
    /// - [`FieldPath`] if a field path cannot be resolved.
    /// - [`TaskListRequiresTaskRows`] if task-list formatting runs on
    ///   page-level rows.
    ///
    /// [`TableColumnCountMismatch`]: super::QueryError::TableColumnCountMismatch
    /// [`FieldPath`]: super::QueryError::FieldPath
    /// [`TaskListRequiresTaskRows`]: super::QueryError::TaskListRequiresTaskRows
    pub(super) fn format(
        &self,
        format: &QueryDisplayFormat,
    ) -> QueryResult<String> {
        format.render(self.materialized())
    }
}

/// Compares evaluated rows, not the pending plan or cache state — two
/// query sets that reach the same rows via different transform paths
/// (e.g. two chained `.filter()` calls vs. one combined filter expression)
/// must compare equal once both are materialized.
impl PartialEq for QuerySet {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        self.materialized() == other.materialized()
    }
}

/// Shows the materialized rows, not the pending plan or cache state — a
/// derived `Debug` would leak `QuerySet`'s internal representation
/// (the pre-transform `records` and the lazily-populated `cache`, which
/// duplicate each other's content once materialized), confusing test-failure
/// diffs. Mirrors [`QueryRow`]'s own hand-rolled [`std::fmt::Debug`].
impl std::fmt::Debug for QuerySet {
    #[inline]
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_tuple("QuerySet").field(&self.materialized()).finish()
    }
}

/// Converts the [`QuerySet`] into an iterator over owned [`QueryRow`]
/// rows. Flushes any pending plan first, like every other read; clones each row
/// out of the materialized `Arc<[QueryRow]>` since an `Arc<[T]>` has no
/// owned `into_iter`.
impl IntoIterator for QuerySet {
    type IntoIter = std::vec::IntoIter<Self::Item>;
    type Item = QueryRow;

    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        self.materialized().to_vec().into_iter()
    }
}

/// Creates an iterator over borrowed [`QueryRow`] rows from the
/// [`QuerySet`], flushing any pending plan first.
impl<'a> IntoIterator for &'a QuerySet {
    type IntoIter = std::slice::Iter<'a, QueryRow>;
    type Item = &'a QueryRow;

    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}
#[cfg(test)]
mod tests {
    use super::QueryRow;

    #[test]
    fn query_row_stays_within_its_size_budget() {
        let size = std::mem::size_of::<QueryRow>();
        assert!(
            size <= 112,
            "QueryRow grew to {size} bytes, past its ~96-byte target — check \
             for an accidentally un-boxed field before raising this bound"
        );
    }
}
