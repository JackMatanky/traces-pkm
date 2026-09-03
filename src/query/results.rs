//! Query result row representations and common table expression (CTE) result
//! sets.
//!
//! Defines [`QueryRow`] and [`QuerySet`], which encapsulate evaluated index
//! rows and provide post-fetch CTE transformation pipelines.
//!
//! # Common Table Expression (CTE) Semantics
//!
//! [`QuerySet`] acts as a branchable, memoized common table expression (CTE)
//! table:
//! - **`O(1)` Appends**: Chained transform calls ([`filter`](QuerySet::filter),
//!   [`sort`](QuerySet::sort), [`limit`](QuerySet::limit),
//!   [`group_by`](QuerySet::group_by), [`flatten`](QuerySet::flatten)) append
//!   steps to an internal [`QueryPlan`](super::QueryPlan) in `O(1)` time.
//! - **Lazy Materialization**: Computes transformations lazily on first access
//!   ([`len`](QuerySet::len), [`get`](QuerySet::get), [`iter`](QuerySet::iter),
//!   or terminal renderers), memoizing the resulting rows.
//! - **Branching Support**: Multiple derived `QuerySet` instances can branch
//!   off one base set without mutating base state or re-evaluating shared
//!   prefixes.
//!
//! # Main Types
//!
//! - [`QueryRow`] - Individual page or task row paired with indexed file
//!   metadata.
//! - [`QuerySet`] - Lazily evaluated result set with chained transform methods
//!   and terminal renderers.
//! # Examples
//!
//! ```rust
//! use traces_pkm::QuerySet;
//! let set = QuerySet::default();
//! assert!(set.is_empty());
//! assert_eq!(set.len(), 0);
//! ```
//!
//! [`FileBase`]: crate::FileBase
//! [`Note`]: crate::Note
use std::{path::PathBuf, sync::Arc};

use super::{
    QueryPlan, QueryResult, QueryTransform,
    format::{QueryDisplayFormat, TaskPathStyle},
    grammar::{FieldPath, FileField, TaskField},
    value::{QueryFieldValueRef, QueryListValueRef},
};
use crate::{
    TaskStatus,
    file::FileBase,
    index::{FileEntry, FileIndex, RowIndex},
    note::{ListItem, ListItemType, Note, NoteFieldValue},
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

    /// Resolves this row's indexed [`FileEntry`].
    fn entry(&self) -> &FileEntry {
        self.index.entry_at(self.position)
    }

    /// Promotes this row to task level.
    ///
    /// No-ops for a `Plain` or `Checkbox` item; only [`ListItemType::Task`]
    /// items carry a resolved status to promote.
    pub(super) fn with_task_item(mut self, item: &ListItem) -> Self {
        let ListItemType::Task(task) = item.kind() else {
            return self;
        };
        self.kind = RowKind::Task(TaskRow {
            status: task.status().clone(),
            text: item.clean_text().to_owned(),
        });
        self
    }

    /// Returns task completion state if this row represents a task item, or
    /// `None`.
    ///
    /// Returns `Some(true)` for completed tasks, `Some(false)` for incomplete
    /// tasks, and `None` for page-level rows or cancelled tasks.
    #[inline]
    #[must_use]
    pub fn task_completed(&self) -> Option<bool> {
        match &self.kind {
            RowKind::Page => None,
            RowKind::Task(task) => task.status.kind().completed(),
        }
    }

    /// Returns the task item's text if this is a task-level row, or `None`
    /// for page-level rows.
    #[inline]
    #[must_use]
    pub fn task_text(&self) -> Option<&str> {
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
    /// row's Note, or an empty slice if no Notes link to it.
    #[inline]
    #[must_use]
    pub(crate) fn inlinks(&self) -> &[PathBuf] {
        self.entry().inlinks()
    }

    /// Resolves a field path string against this row's metadata.
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

    /// Returns a copy of this row with `path` overridden to `value`.
    ///
    /// Used by [`QuerySet::flatten`] to set the resolved value for exploded
    /// list rows. If `path` already has an override, the value is updated in
    /// place.
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
/// [`flatten`](Self::flatten)) append transformations in `O(1)` time to a
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
    base: Arc<Vec<QueryRow>>,
    plan: QueryPlan,
    cache: std::sync::OnceLock<Arc<Vec<QueryRow>>>,
}

impl QuerySet {
    /// Wraps `rows` into a new [`QuerySet`] with no pending transforms.
    pub(super) fn new(rows: Vec<QueryRow>) -> Self {
        Self {
            base: rows.into(),
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
    fn rows(&self) -> &Arc<Vec<QueryRow>> {
        self.cache.get_or_init(|| {
            if self.plan.is_empty() {
                Arc::clone(&self.base)
            } else {
                Arc::new(self.plan.clone().run(self.base.to_vec()))
            }
        })
    }

    /// Returns the number of [`QueryRow`] rows in this query set.
    #[inline]
    #[must_use]
    pub fn len(&self) -> usize {
        self.rows().len()
    }

    /// Returns `true` if this query set contains no [`QueryRow`] rows.
    #[inline]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rows().is_empty()
    }

    /// Returns a reference to the [`QueryRow`] at `index`, or `None` if
    /// out of bounds.
    #[inline]
    #[must_use]
    pub fn get(&self, index: usize) -> Option<&QueryRow> {
        self.rows().get(index)
    }

    /// Returns an iterator over references to the contained [`QueryRow`]
    /// rows.
    #[inline]
    pub fn iter(&self) -> std::slice::Iter<'_, QueryRow> {
        self.rows().iter()
    }

    /// Appends `transform` to this query set's pending plan, returning a new
    /// [`QuerySet`] over the same base rows. Cheap: moves the `Arc` (no
    /// refcount bump; `self` is consumed) and the short transform-step list;
    /// nothing is evaluated until [`Self::rows`] runs on read.
    fn push(self, transform: QueryTransform) -> Self {
        let mut plan = self.plan;
        plan.push(transform);
        Self {
            base: self.base,
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
    /// - [`Builder`] if `expr` is an invalid filter expression.
    ///
    /// [`Builder`]: super::QueryError::Builder
    #[inline]
    pub(crate) fn filter(self, expr: &str) -> QueryResult<Self> {
        Ok(self.push(QueryTransform::filter(expr)?))
    }

    /// Filters rows matching `expr`, serving as a Rust-side alias for
    /// [`Self::filter`].
    ///
    /// # Errors
    ///
    /// - [`Builder`] if `expr` is an invalid filter expression.
    ///
    /// [`Builder`]: super::QueryError::Builder
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
    /// - [`Builder`] if `path` cannot be parsed as a valid field path.
    ///
    /// [`Builder`]: super::QueryError::Builder
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
    /// - [`Builder`] if `n` is negative or exceeds platform pointer-width
    ///   limits (`usize::MAX`).
    ///
    /// [`Builder`]: super::QueryError::Builder
    #[inline]
    pub(crate) fn limit(self, n: i64) -> QueryResult<Self> {
        Ok(self.push(QueryTransform::limit(n)?))
    }

    /// Groups rows by sorting them ascending on the field at `path`.
    ///
    /// # Errors
    ///
    /// - [`Builder`] if `path` cannot be parsed as a valid field path.
    ///
    /// [`Builder`]: super::QueryError::Builder
    #[inline]
    pub(crate) fn group_by(self, path: &str) -> QueryResult<Self> {
        Ok(self.push(QueryTransform::group_by(path)?))
    }

    /// Explodes rows containing a list at `path` into one row per list element.
    ///
    /// # Errors
    ///
    /// - [`Builder`] if `path` cannot be parsed as a valid field path.
    ///
    /// [`Builder`]: super::QueryError::Builder
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

    /// Renders rows using the given display format.
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
        format.render(self.rows())
    }
}

/// Compares evaluated rows, not the pending plan or cache state. Two query sets
/// that reach the same rows via different transform paths (e.g. two chained
/// `.filter()` calls vs. one combined filter expression) must compare equal
/// once both are materialized.
impl PartialEq for QuerySet {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        self.rows() == other.rows()
    }
}

/// Shows the materialized rows, not the pending plan or cache state. A derived
/// `Debug` would leak `QuerySet`'s internal representation (the pre-transform
/// `base` and the lazily-populated `cache`, which duplicate each other's
/// content once materialized), confusing test-failure diffs. Mirrors
/// [`QueryRow`]'s own hand-rolled [`std::fmt::Debug`].
impl std::fmt::Debug for QuerySet {
    #[inline]
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_tuple("QuerySet").field(&self.rows()).finish()
    }
}

/// Converts the [`QuerySet`] into an iterator over owned [`QueryRow`]
/// rows. Flushes any pending plan first, like every other read.
///
/// Reclaims the materialized rows without cloning when this is the only
/// surviving reference to them (the common case: a chain like
/// `query.from().filter(...)` that was never [`Clone`]d for branching) via
/// [`Arc::try_unwrap`]. Falls back to cloning only when another [`QuerySet`]
/// branch still shares the same cached rows.
impl IntoIterator for QuerySet {
    type IntoIter = std::vec::IntoIter<Self::Item>;
    type Item = QueryRow;

    #[inline]
    #[expect(
        clippy::expect_used,
        reason = "self.rows() on the line above always populates the OnceLock \
                  before this reads it"
    )]
    fn into_iter(self) -> Self::IntoIter {
        let _ = self.rows();
        let Self {
            base,
            cache,
            ..
        } = self;
        drop(base);
        let rows = cache.into_inner().expect("populated by self.rows() above");
        match Arc::try_unwrap(rows) {
            Ok(owned) => owned.into_iter(),
            Err(shared) => (*shared).clone().into_iter(),
        }
    }
}

/// Creates an iterator over borrowed [`QueryRow`] rows from the [`QuerySet`],
/// flushing any pending plan first.
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
    use std::{
        fs,
        path::{Path, PathBuf},
        sync::Arc,
    };

    use super::*;
    use crate::{
        index::IndexerService,
        note::NoteFieldValue,
        query::{
            FieldPathError, QueryBuilder, QueryBuilderError, QueryError,
            QueryService, SourceSelector,
            test_support::{
                find_base, find_entry, outcome_for, outcome_for_files,
            },
        },
    };

    mod memory_layout {
        use super::*;

        #[test]
        fn query_row_stays_within_its_size_budget() {
            let size = std::mem::size_of::<QueryRow>();
            assert!(
                size <= 112,
                "QueryRow grew to {size} bytes, past its ~96-byte target; \
                 check for an accidentally un-boxed field before raising this \
                 bound"
            );
        }
    }

    mod index_record {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn file_accessor_returns_the_bundled_file_record() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("a.md"), "Filed under #tag.")
                .expect("write file");
            let index = Arc::new(
                IndexerService::new(temp.path()).build().expect("build index"),
            );
            let file = find_base(index.entries(), Path::new("a.md"));
            let outcome = QueryService::new("class")
                .execute(&index, QueryBuilder::pages(SourceSelector::All));
            let row = outcome.get(0).expect("row");
            assert_eq!(row.file(), file);
        }

        #[test]
        fn note_accessor_returns_the_bundled_note() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("a.md"), "Filed under #tag.")
                .expect("write file");
            let index = Arc::new(
                IndexerService::new(temp.path()).build().expect("build index"),
            );
            let note = find_entry(index.entries(), Path::new("a.md"))
                .note()
                .expect("note");
            let outcome = QueryService::new("class")
                .execute(&index, QueryBuilder::pages(SourceSelector::All));
            let row = outcome.get(0).expect("row");
            assert_eq!(row.note(), Some(note));
        }

        #[test]
        fn with_task_item_sets_task_completed() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("a.md"), "- [x] Buy milk")
                .expect("write file");
            let index = Arc::new(
                IndexerService::new(temp.path()).build().expect("build index"),
            );
            let outcome = QueryService::new("class")
                .execute(&index, QueryBuilder::tasks(SourceSelector::All));
            let row = outcome.get(0).expect("row");
            assert_eq!(row.task_completed(), Some(true));
        }

        #[test]
        fn with_task_item_sets_task_text() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("a.md"), "- [ ] Buy milk")
                .expect("write file");
            let index = Arc::new(
                IndexerService::new(temp.path()).build().expect("build index"),
            );
            let outcome = QueryService::new("class")
                .execute(&index, QueryBuilder::tasks(SourceSelector::All));
            let row = outcome.get(0).expect("row");
            assert_eq!(row.task_text(), Some("Buy milk"));
        }

        #[test]
        fn task_accessors_return_none_for_page_level_records() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("a.md"), "body").expect("write file");
            let outcome = outcome_for(temp.path(), "body");
            let row = outcome.get(0).expect("row");

            assert_eq!(row.task_completed(), None);
            assert_eq!(row.task_text(), None);
        }

        #[test]
        fn inlinks_accessor_returns_the_bundled_inlinks() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let outcome = outcome_for_files(temp.path(), &[
                ("target.md", "# Target"),
                ("b.md", "[[target]]"),
            ]);
            let row = outcome
                .iter()
                .find(|row| row.file().path() == Path::new("target.md"))
                .expect("target row");

            assert_eq!(row.inlinks(), [PathBuf::from("b.md")]);
        }
    }

    mod field_path {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn resolves_file_path_name_folder_and_size() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::create_dir_all(temp.path().join("notes")).expect("mkdir");
            let outcome =
                outcome_for_files(temp.path(), &[("notes/todo.md", "body")]);
            let row = outcome.get(0).expect("row");

            assert_eq!(
                row.field("file.path"),
                Ok(NoteFieldValue::String("notes/todo.md".to_owned()))
            );
            assert_eq!(
                row.field("file.name"),
                Ok(NoteFieldValue::String("todo".to_owned()))
            );
            assert_eq!(
                row.field("file.folder"),
                Ok(NoteFieldValue::String("notes".to_owned()))
            );
            assert_eq!(row.field("file.size"), Ok(NoteFieldValue::Number(4.0)));
        }

        #[test]
        fn resolves_dataview_style_time_accessors_from_file_record() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let outcome = outcome_for(temp.path(), "body");
            let row = outcome.get(0).expect("row");
            let file = row.file();

            assert_eq!(
                row.field("file.mtime"),
                Ok(NoteFieldValue::Date(
                    file.modified_at().to_datetime_string()
                ))
            );
            assert_eq!(
                row.field("file.mdate"),
                Ok(NoteFieldValue::Date(file.modified_at().to_date_string()))
            );
            assert_eq!(
                row.field("file.ctime"),
                Ok(NoteFieldValue::Date(
                    file.created_at_or_modified().to_datetime_string()
                ))
            );
            assert_eq!(
                row.field("file.cdate"),
                Ok(NoteFieldValue::Date(
                    file.created_at_or_modified().to_date_string()
                ))
            );
            assert_eq!(row.field("file.created_at"), row.field("file.ctime"));
            assert_eq!(row.field("file.modified_at"), row.field("file.mtime"));
        }

        #[test]
        fn resolves_frontmatter_and_inline_fields_by_key() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let outcome =
                outcome_for(temp.path(), "---\nrating: 5\n---\nStatus:: Draft");
            let row = outcome.get(0).expect("row");

            assert_eq!(row.field("rating"), Ok(NoteFieldValue::Number(5.0)));
            assert_eq!(
                row.field("Status"),
                Ok(NoteFieldValue::String("Draft".to_owned()))
            );
        }

        #[test]
        fn frontmatter_field_takes_precedence_over_same_key_inline_field() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let outcome = outcome_for(
                temp.path(),
                "---\nstatus: Approved\n---\nstatus:: Draft",
            );
            let row = outcome.get(0).expect("row");

            assert_eq!(
                row.field("status"),
                Ok(NoteFieldValue::String("Approved".to_owned()))
            );
        }

        #[test]
        fn resolves_tags_as_a_list_of_tag_strings() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let outcome = outcome_for(temp.path(), "Filed under #book #read");
            let row = outcome.get(0).expect("row");

            assert_eq!(
                row.field("tags"),
                Ok(NoteFieldValue::List(vec![
                    NoteFieldValue::String("#book".to_owned()),
                    NoteFieldValue::String("#read".to_owned()),
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
            let row = outcome
                .iter()
                .find(|row| row.file().path() == Path::new("target.md"))
                .expect("target row");

            assert_eq!(
                row.field("inlinks"),
                Ok(NoteFieldValue::List(vec![
                    NoteFieldValue::String("a.md".to_owned()),
                    NoteFieldValue::String("b.md".to_owned()),
                ]))
            );
        }

        #[test]
        fn resolves_inlinks_as_an_empty_list_when_nothing_links_to_the_note() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let outcome = outcome_for(temp.path(), "No inbound links here.");
            let row = outcome.get(0).expect("row");

            assert_eq!(row.field("inlinks"), Ok(NoteFieldValue::List(vec![])));
        }

        #[test]
        fn missing_field_resolves_to_null() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let outcome = outcome_for(temp.path(), "body, no frontmatter");
            let row = outcome.get(0).expect("row");

            assert_eq!(row.field("no_such_field"), Ok(NoteFieldValue::Null));
        }

        #[test]
        fn resolves_task_completed_and_task_text_on_task_rows() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("a.md"), "- [x] Buy milk")
                .expect("write file");
            let index = Arc::new(
                IndexerService::new(temp.path()).build().expect("build index"),
            );
            let outcome = QueryService::new("class")
                .execute(&index, QueryBuilder::tasks(SourceSelector::All));
            let row = outcome.get(0).expect("row");
            assert_eq!(
                row.field("task.completed"),
                Ok(NoteFieldValue::Bool(true))
            );
            assert_eq!(
                row.field("task.text"),
                Ok(NoteFieldValue::String("Buy milk".to_owned()))
            );
        }

        #[test]
        fn task_fields_resolve_to_null_on_page_level_records() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let outcome = outcome_for(temp.path(), "body");
            let row = outcome.get(0).expect("row");

            assert_eq!(row.field("task.completed"), Ok(NoteFieldValue::Null));
            assert_eq!(row.field("task.text"), Ok(NoteFieldValue::Null));
        }
    }

    mod limit {
        use pretty_assertions::assert_eq;

        use super::*;

        fn outcome_of_three(temp: &Path) -> QuerySet {
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
                Err(QueryError::Builder(QueryBuilderError::LimitOutOfRange {
                    value: -1
                }))
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

            let categories: Vec<NoteFieldValue> = grouped
                .iter()
                .map(|row| row.field("category").expect("valid path"))
                .collect();
            assert_eq!(categories, [
                NoteFieldValue::String("article".to_owned()),
                NoteFieldValue::String("book".to_owned()),
                NoteFieldValue::String("book".to_owned()),
            ]);
        }

        #[test]
        fn rejects_malformed_field_path() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let outcome = outcome_for(temp.path(), "body");

            assert_eq!(
                outcome.group_by("file.bogus"),
                Err(QueryError::Builder(QueryBuilderError::FieldPath(
                    FieldPathError::new("file.bogus", None)
                )))
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
            let authors: Vec<NoteFieldValue> = flattened
                .iter()
                .map(|row| row.field("authors").expect("valid path"))
                .collect();
            assert_eq!(authors, [
                NoteFieldValue::String("Alice".to_owned()),
                NoteFieldValue::String("Bob".to_owned()),
            ]);
            // Every other field still resolves from the original row.
            for row in &flattened {
                assert_eq!(
                    row.field("title"),
                    Ok(NoteFieldValue::String("Multi".to_owned()))
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
                flattened.get(0).expect("row").field("rating"),
                Ok(NoteFieldValue::Number(5.0))
            );
        }

        #[test]
        fn flattening_tags_yields_one_row_per_tag() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let outcome = outcome_for(temp.path(), "Filed under #book #read");

            let flattened = outcome.flatten("tags").expect("valid flatten");

            let tags: Vec<NoteFieldValue> = flattened
                .iter()
                .map(|row| row.field("tags").expect("valid path"))
                .collect();
            assert_eq!(tags, [
                NoteFieldValue::String("#book".to_owned()),
                NoteFieldValue::String("#read".to_owned()),
            ]);
        }

        #[test]
        fn rejects_malformed_field_path() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let outcome = outcome_for(temp.path(), "body");

            assert_eq!(
                outcome.flatten("file.bogus"),
                Err(QueryError::Builder(QueryBuilderError::FieldPath(
                    FieldPathError::new("file.bogus", None)
                )))
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
                filtered.get(0).expect("row").field("authors"),
                Ok(NoteFieldValue::String("Bob".to_owned()))
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
            let pairs: Vec<(NoteFieldValue, NoteFieldValue)> = flattened
                .iter()
                .map(|row| {
                    (
                        row.field("authors").expect("valid authors"),
                        row.field("tags").expect("valid tags"),
                    )
                })
                .collect();
            assert_eq!(pairs, [
                (
                    NoteFieldValue::String("Alice".to_owned()),
                    NoteFieldValue::String("#book".to_owned())
                ),
                (
                    NoteFieldValue::String("Alice".to_owned()),
                    NoteFieldValue::String("#read".to_owned())
                ),
                (
                    NoteFieldValue::String("Bob".to_owned()),
                    NoteFieldValue::String("#book".to_owned())
                ),
                (
                    NoteFieldValue::String("Bob".to_owned()),
                    NoteFieldValue::String("#read".to_owned())
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
            let table = QuerySet::default()
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
            let table = QuerySet::default()
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
                Err(QueryError::FieldPath(FieldPathError::new(
                    "file.bogus",
                    None
                )))
            );
        }

        #[test]
        fn rejects_a_headers_columns_length_mismatch() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let outcome = outcome_for(temp.path(), "body");

            assert_eq!(
                outcome.table(&["Name", "Rating"], &["file.name"]),
                Err(QueryError::TableColumnCountMismatch {
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
            let list = QuerySet::default().list("rating").expect("valid list");

            assert_eq!(list, "");
        }

        #[test]
        fn rejects_malformed_field_path() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let outcome = outcome_for(temp.path(), "body");

            assert_eq!(
                outcome.list("file.bogus"),
                Err(QueryError::FieldPath(FieldPathError::new(
                    "file.bogus",
                    None
                )))
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

        #[test]
        fn display_format_matches_list_wrapper() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let outcome = outcome_for(temp.path(), "---\nrating: 5\n---");

            let direct = outcome
                .format(&QueryDisplayFormat::list("rating"))
                .expect("valid display format");

            assert_eq!(direct, outcome.list("rating").expect("valid list"));
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
            let index = Arc::new(
                IndexerService::new(temp.path()).build().expect("build index"),
            );
            let outcome = QueryService::new("class")
                .execute(&index, QueryBuilder::tasks(SourceSelector::All));
            let rendered = outcome
                .task_list(TaskPathStyle::default())
                .expect("valid task_list");

            assert_eq!(rendered, "- [ ] Buy milk\n- [x] Walk dog\n");
        }

        #[test]
        fn renders_a_dash_checkbox_for_a_cancelled_task_without_erroring() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("a.md"), "- [-] Abandoned task\n")
                .expect("write file");
            let index = Arc::new(
                IndexerService::new(temp.path()).build().expect("build index"),
            );
            let outcome = QueryService::new("class")
                .execute(&index, QueryBuilder::tasks(SourceSelector::All));
            let rendered = outcome
                .task_list(TaskPathStyle::default())
                .expect("valid task_list");

            assert_eq!(rendered, "- [-] Abandoned task\n");
        }

        #[test]
        fn renders_an_empty_string_for_an_empty_outcome() {
            let rendered = QuerySet::default()
                .task_list(TaskPathStyle::default())
                .expect("valid task_list");

            assert_eq!(rendered, "");
        }

        #[test]
        fn rejects_page_level_records_with_no_task_fields() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let outcome = outcome_for(temp.path(), "# Just a Note");

            assert_eq!(
                outcome.task_list(TaskPathStyle::default()),
                Err(QueryError::TaskListRequiresTaskRows)
            );
        }
    }

    mod query_outcome {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn len_returns_zero_for_an_empty_outcome() {
            let empty = QuerySet::default();
            assert_eq!(empty.len(), 0);
        }

        #[test]
        fn is_empty_returns_true_for_an_empty_outcome() {
            let empty = QuerySet::default();
            assert!(empty.is_empty());
        }

        #[test]
        fn len_returns_record_count_for_a_non_empty_outcome() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let outcome = outcome_for(temp.path(), "# A");

            assert_eq!(outcome.len(), 1);
        }

        #[test]
        fn is_empty_returns_false_for_a_non_empty_outcome() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let outcome = outcome_for(temp.path(), "# A");

            assert!(!outcome.is_empty());
        }

        #[test]
        fn get_returns_record_or_none() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let outcome = outcome_for(temp.path(), "# A");

            assert!(outcome.get(0).is_some());
            assert_eq!(outcome.get(1), None);
        }

        #[test]
        fn iter_and_into_iterator_yield_records() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let outcome = outcome_for(temp.path(), "# A");

            assert_eq!(outcome.iter().count(), 1);

            assert_eq!((&outcome).into_iter().count(), 1);

            assert_eq!(outcome.into_iter().count(), 1);
        }
    }

    mod cte_chaining {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn chained_filters_match_one_combined_filter_expression() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let base = outcome_for_files(temp.path(), &[
                ("a.md", "---\nrating: 1\n---\n"),
                ("b.md", "---\nrating: 3\n---\n"),
                ("c.md", "---\nrating: 5\n---\n"),
                ("d.md", "---\nrating: 7\n---\n"),
                ("e.md", "---\nrating: 9\n---\n"),
            ]);

            let chained = base
                .clone()
                .filter("rating > 2")
                .expect("valid filter")
                .filter("rating < 8")
                .expect("valid filter");
            let combined =
                base.filter("rating > 2 and rating < 8").expect("valid filter");

            assert_eq!(chained, combined);
        }

        #[test]
        fn chained_sort_then_limit_matches_full_sort_order_for_tied_keys() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let files: Vec<(String, String)> = (0..200)
                .map(|i| {
                    (
                        format!("note-{i:03}.md"),
                        format!("---\nrating: {}\n---\n", i % 4),
                    )
                })
                .collect();
            let file_refs: Vec<(&str, &str)> = files
                .iter()
                .map(|(name, content)| (name.as_str(), content.as_str()))
                .collect();
            let base = outcome_for_files(temp.path(), &file_refs);

            for n in [5_usize, 50, 100] {
                let chained = base
                    .clone()
                    .sort("rating", false)
                    .expect("valid sort")
                    .limit(i64::try_from(n).expect("limit fits i64"))
                    .expect("valid limit");
                let chained_paths: Vec<_> = (0..chained.len())
                    .map(|i| {
                        chained.get(i).expect("row").file().path().to_path_buf()
                    })
                    .collect();

                let full_sorted =
                    base.clone().sort("rating", false).expect("valid sort");
                let full_first_n: Vec<_> = (0..n)
                    .map(|i| {
                        full_sorted
                            .get(i)
                            .expect("row")
                            .file()
                            .path()
                            .to_path_buf()
                    })
                    .collect();

                assert_eq!(
                    chained_paths, full_first_n,
                    "chained .sort().limit(n={n}) must match a full stable \
                     sort's first {n} rows, including tie order"
                );
            }
        }

        #[test]
        fn branching_from_the_same_base_does_not_cross_contaminate() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let base = outcome_for_files(temp.path(), &[
                ("a.md", "---\nrating: 1\n---\n"),
                ("b.md", "---\nrating: 5\n---\n"),
                ("c.md", "---\nrating: 9\n---\n"),
            ]);

            let low = base.clone().filter("rating < 5").expect("valid filter");
            let high = base.clone().filter("rating > 5").expect("valid filter");

            assert_eq!(base.len(), 3, "branching must not mutate the base");
            assert_eq!(low.len(), 1);
            assert_eq!(high.len(), 1);
        }
    }
}
