//! Query source selection, field resolution, and outcome transformations.
//!
//! This module powers page-level results from [`super::FileIndex::query`] and
//! task-level rows from [`super::FileIndex::query_tasks`].
//!
//! # Main Types
//!
//! - [`Source`] - Selects which Notes a query includes.
//! - [`IndexRecord`] - Pairs a [`FileRecord`] with its parsed [`Note`] and
//!   resolves `file.*`, `task.*`, metadata, and tag fields.
//! - [`QueryOutcome`] - Stores result rows, applies chained transformations
//!   ([`QueryOutcome::filter`], [`QueryOutcome::sort`],
//!   [`QueryOutcome::limit`], [`QueryOutcome::group_by`],
//!   [`QueryOutcome::flatten`]), and renders terminal markdown output
//!   ([`QueryOutcome::table`], [`QueryOutcome::list`],
//!   [`QueryOutcome::task_list`]). Terminal renderers are plain Rust methods
//!   with no minijinja dependency, so both the `query`/`tasks` template
//!   namespaces and future CLI query commands can reuse them.
//! - [`QueryError`] - Reports malformed field paths and query expressions.

mod error;
mod field;
mod filter;
mod operators;
mod sort;

use std::path::PathBuf;

pub(crate) use error::QueryError;
pub(crate) use field::FileField;
use filter::FilterExpr;
use sort::sort_key_cmp;

use super::file::FileRecord;
use crate::note::{FieldValue, Note};

/// Ordered collection of [`IndexRecord`] rows produced by an index query.
///
/// Page-level outcomes contain one row per Note. Task-level outcomes contain
/// one row per task item. Consumers should not assume which shape they have
/// unless they control the query source.
///
/// Transformation methods consume and return [`QueryOutcome`], so calls chain
/// naturally: `outcome.filter("rating > 7")?.sort("rating", true)?.limit(10)?`.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct QueryOutcome {
    records: Vec<IndexRecord>,
}

impl QueryOutcome {
    /// Wraps `records` as a query result.
    pub(super) fn new(records: Vec<IndexRecord>) -> Self {
        Self {
            records,
        }
    }

    /// The number of [`IndexRecord`]s in this outcome.
    #[inline]
    #[must_use]
    pub(crate) fn len(&self) -> usize {
        self.records.len()
    }

    /// Whether this outcome has no [`IndexRecord`]s.
    #[inline]
    #[must_use]
    pub(crate) fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// The [`IndexRecord`] at `index`, if present.
    #[inline]
    #[must_use]
    pub(crate) fn get(&self, index: usize) -> Option<&IndexRecord> {
        self.records.get(index)
    }

    /// Iterates over the contained [`IndexRecord`]s by reference.
    #[inline]
    pub(crate) fn iter(&self) -> std::slice::Iter<'_, IndexRecord> {
        self.records.iter()
    }

    /// Keeps only records matching the filter expression `expr`.
    ///
    /// Supported forms:
    /// - Comparisons: `<field> <op> <value>` with `==`, `!=`, `>=`, `<=`, `>`,
    ///   or `<`.
    /// - Functions: `contains(field, value)` checks list membership, tag prefix
    ///   matches like `#book` matching `#book/fiction`, or string substring
    ///   containment.
    /// - Boolean logic: `AND` / `and` / `&&`, `OR` / `or` / `||`, and `NOT` /
    ///   `not` / `!`.
    /// - Parentheses: `( ... )` overrides standard operator precedence.
    /// - Literals: double-quoted strings with `\"` escape support, numbers,
    ///   `true`/`false`, or `null`/`Null`.
    /// - Text normalization: `==` and `!=` treat `String`, `Date`, and
    ///   `Duration` values as textually comparable. For example, `"2026-07-29"`
    ///   matches a `Date` field with equal text.
    /// - Type mismatches: other cross-kind comparisons, such as comparing a
    ///   number to a string, never match under any operator except `!=`.
    /// - Null values: records missing the field (`Null`) never match `==` or
    ///   ordering operators, but do match `!=`.
    ///
    /// # Errors
    ///
    /// - [`QueryError::UnparsableFilterExpression`] if `expr` cannot be parsed.
    /// - [`QueryError::UnknownFieldPath`] if its field path is malformed.
    pub(crate) fn filter(self, expr: &str) -> Result<Self, QueryError> {
        let expr = FilterExpr::parse(expr)?;
        let records = self
            .records
            .into_iter()
            .filter(|record| expr.matches(record))
            .collect();
        Ok(Self::new(records))
    }

    /// Keeps only records matching the filter expression `expr`.
    ///
    /// Alias for [`Self::filter`] using raw identifier syntax (`r#where`).
    /// See [`Self::filter`] for full syntax and matching rules.
    ///
    /// # Errors
    ///
    /// - [`QueryError::UnparsableFilterExpression`] if `expr` cannot be parsed
    /// - [`QueryError::UnknownFieldPath`] if its field path is malformed
    #[inline]
    pub(crate) fn r#where(self, expr: &str) -> Result<Self, QueryError> {
        self.filter(expr)
    }

    /// Orders records by `path`, ascending unless `descending` is set.
    ///
    /// Matches Dataview's sort semantics:
    /// - Null values: records missing `path` ([`FieldValue::Null`]) sort as
    ///   minimum values, so they lead ascending and trail descending.
    /// - Stability: equal or incomparable records keep their relative order.
    ///
    /// # Errors
    ///
    /// - [`QueryError::UnknownFieldPath`] if `path` is malformed.
    #[inline]
    pub(crate) fn sort(
        self,
        path: &str,
        descending: bool,
    ) -> Result<Self, QueryError> {
        self.sort_by_field(path, descending)
    }

    /// Keeps at most `n` leading records.
    ///
    /// # Errors
    ///
    /// - [`QueryError::NegativeLimit`] if `n` is negative or does not fit in a
    ///   [`usize`] on this platform.
    pub(crate) fn limit(self, n: i64) -> Result<Self, QueryError> {
        let n = usize::try_from(n).map_err(|_source| {
            QueryError::NegativeLimit {
                n,
            }
        })?;
        Ok(Self::new(self.records.into_iter().take(n).collect()))
    }

    /// Orders records by `path` to cluster equal values for grouping.
    ///
    /// Sorts ascending so consumers (such as template loops or terminal
    /// renderers) can detect group boundaries by comparing each record's
    /// resolved `path` value to the previous record.
    ///
    /// # Errors
    ///
    /// - [`QueryError::UnknownFieldPath`] if `path` is malformed.
    #[inline]
    pub(crate) fn group_by(self, path: &str) -> Result<Self, QueryError> {
        self.sort_by_field(path, false)
    }

    /// Explodes each record's `path` field into one row per list element.
    ///
    /// Behavioral details:
    /// - Target fields: applies to fields resolving to [`FieldValue::List`],
    ///   including frontmatter lists, inline list fields, and `tags`.
    /// - Non-list fields: records with scalar values pass through unchanged.
    /// - Empty lists: records with empty list values contribute no rows.
    /// - Row resolution: on exploded rows, `path` resolves to that row's single
    ///   element while all other fields resolve from the original record.
    ///
    /// # Errors
    ///
    /// - [`QueryError::UnknownFieldPath`] if `path` is malformed.
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

    /// Renders these records as a GitHub-flavored markdown table with one
    /// column per entry, pairing each `headers` label with the field path at
    /// the same position in `columns`.
    ///
    /// Cell values render like [`Self::list`], except pipe characters (`|`)
    /// are escaped and newlines are collapsed to spaces so no cell value can
    /// corrupt the table's row structure.
    ///
    /// # Errors
    ///
    /// - [`QueryError::TableColumnMismatch`] if `headers` and `columns` have
    ///   different lengths.
    /// - [`QueryError::UnknownFieldPath`] if any entry of `columns` is
    ///   malformed.
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
        let mut out = markdown_row(headers.iter().copied());
        out.push_str(&markdown_row(headers.iter().map(|_| "---")));
        for record in &self.records {
            out.push_str(&markdown_row(
                paths.iter().map(|path| table_cell_text(&record.resolve(path))),
            ));
        }
        Ok(out)
    }

    /// Renders these records as a markdown bullet list, one item per record,
    /// using the resolved value of `path`.
    ///
    /// # Errors
    ///
    /// - [`QueryError::UnknownFieldPath`] if `path` is malformed.
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

    /// Renders these records as a markdown task list (`- [ ]`/`- [x]`), one
    /// item per task-level record's `task.completed`/`task.text`.
    ///
    /// # Errors
    ///
    /// - [`QueryError::TaskListOnPageRecords`] if any record has no task fields
    ///   — built by [`super::FileIndex::query`] rather than
    ///   [`super::FileIndex::query_tasks`].
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

    /// Sorts records by the resolved value of `path`.
    ///
    /// Shared implementation for [`Self::sort`] and [`Self::group_by`]: a
    /// stable sort by `path`'s resolved value, treating [`FieldValue::Null`]
    /// as the minimum value.
    ///
    /// # Errors
    ///
    /// - [`QueryError::UnknownFieldPath`] if `path` is malformed.
    fn sort_by_field(
        self,
        path: &str,
        descending: bool,
    ) -> Result<Self, QueryError> {
        let field_path = FieldPath::parse(path)?;
        let mut records = self.records;
        records.sort_by(|a, b| {
            sort_key_cmp(
                &a.resolve(&field_path),
                &b.resolve(&field_path),
                descending,
            )
        });
        Ok(Self::new(records))
    }
}

/// Joins `cells` into one markdown table row: `| c1 | c2 | ... |`, newline
/// included. Shared by [`QueryOutcome::table`]'s header, separator, and data
/// rows.
fn markdown_row(cells: impl IntoIterator<Item = impl AsRef<str>>) -> String {
    let mut row = String::from("|");
    for cell in cells {
        row.push(' ');
        row.push_str(cell.as_ref());
        row.push_str(" |");
    }
    row.push('\n');
    row
}

/// Converts a resolved [`FieldValue`] to plain text for [`QueryOutcome::list`]
/// and [`QueryOutcome::table`] cells.
///
/// [`FieldValue::Null`] renders as an empty string. [`FieldValue::Link`]
/// renders as its target path; Traces has no separate link display yet.
/// [`FieldValue::List`] and [`FieldValue::Object`] flatten recursively,
/// joined by `", "`.
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

/// [`field_text`], with pipe characters escaped and newlines collapsed to
/// spaces so a cell value cannot corrupt [`QueryOutcome::table`]'s row
/// structure.
fn table_cell_text(value: &FieldValue) -> String {
    field_text(value).replace('\n', " ").replace('|', "\\|")
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

/// A `task.<field>` accessor, valid on task-level rows built by
/// [`super::FileIndex::query_tasks`].
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(super) enum TaskField {
    /// Task completion state (`- [ ]` vs `- [x]`).
    Completed,
    /// Task item text.
    Text,
}

impl TaskField {
    /// Parses a `task.<field>` accessor name (the part after `"task."`).
    ///
    /// Returns `None` if `name` is not a known accessor. Mirrors
    /// [`FileField::parse`]'s single failure mode; the caller building
    /// [`QueryError::UnknownFieldPath`] already has the full `task.<field>`
    /// path.
    pub(super) fn parse(name: &str) -> Option<Self> {
        match name {
            "completed" => Some(Self::Completed),
            "text" => Some(Self::Text),
            _ => None,
        }
    }
}

/// A query field path, resolved once per [`QueryOutcome`] transformation
/// and then applied to every [`IndexRecord`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum FieldPath {
    /// A `file.<field>` accessor.
    File(FileField),
    /// A `task.<field>` accessor, resolving to [`FieldValue::Null`] on
    /// page-level records.
    Task(TaskField),
    /// A frontmatter or inline field, looked up by key.
    Metadata(String),
    /// The Note's markdown tags, as a [`FieldValue::List`] of tag strings.
    Tags,
}

impl FieldPath {
    /// Parses a query field path string into a [`FieldPath`].
    ///
    /// Resolves `file.<field>` accessors, `task.<field>` accessors, `tags`,
    /// or frontmatter/inline field keys.
    ///
    /// # Errors
    ///
    /// - [`QueryError::UnknownFieldPath`] if `path` is empty, uses an unknown
    ///   `file.*`/`task.*` accessor, or has unexpected `.` structure.
    pub(super) fn parse(path: &str) -> Result<Self, QueryError> {
        let path = path.trim();
        let invalid = || QueryError::UnknownFieldPath {
            path: path.to_owned(),
        };
        if let Some(field) = path.strip_prefix("file.") {
            return if field.is_empty() || field.contains('.') {
                Err(invalid())
            } else {
                FileField::parse(field).map(Self::File).ok_or_else(invalid)
            };
        }
        if let Some(field) = path.strip_prefix("task.") {
            return if field.is_empty() || field.contains('.') {
                Err(invalid())
            } else {
                TaskField::parse(field).map(Self::Task).ok_or_else(invalid)
            };
        }
        if path.is_empty()
            || path == "file"
            || path == "task"
            || path.contains('.')
        {
            return Err(invalid());
        }
        if path == "tags" {
            return Ok(Self::Tags);
        }
        Ok(Self::Metadata(path.to_owned()))
    }
}

/// Query row pairing a [`FileRecord`] with parsed [`Note`] metadata.
///
/// Task-level rows also carry one task item's fields. Each row resolves
/// `file.*`, `task.*`, frontmatter, inline fields, and tags for Template and
/// CLI callers.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct IndexRecord {
    file: FileRecord,
    note: Note,
    /// Overrides field resolution for exploded rows produced by
    /// [`QueryOutcome::flatten`].
    flattened: Vec<(FieldPath, FieldValue)>,
    /// This row's task fields, set by [`super::FileIndex::query_tasks`].
    /// `None` for page-level records.
    task: Option<TaskInfo>,
}

/// Per-task fields layered onto an [`IndexRecord`] by
/// [`super::FileIndex::query_tasks`]. A task-level row keeps its parent Note's
/// `file`/`note` fields for filtering and display, adding only these two.
///
/// Distinct from [`IndexRecord::flattened`], which overrides an *existing*
/// field path rather than adding new ones.
#[derive(Clone, Debug, PartialEq)]
struct TaskInfo {
    completed: bool,
    text: String,
}

impl IndexRecord {
    /// Pairs `file` with its parsed `note`.
    pub(super) fn new(file: FileRecord, note: Note) -> Self {
        Self {
            file,
            note,
            flattened: Vec::new(),
            task: None,
        }
    }

    /// Returns this record as one task row, with `task.completed` and
    /// `task.text` set to `completed`/`text`.
    ///
    /// Used by [`super::FileIndex::query_tasks`] to turn one page-level
    /// record into one row per task item, retaining the parent Note's
    /// `file.*`, frontmatter, inline-field, and tag metadata for filtering
    /// and display via [`Self::field`].
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

    /// This row's task completion state, if it is a task-level row built by
    /// [`super::FileIndex::query_tasks`]. `None` for page-level records.
    #[inline]
    #[must_use]
    pub(crate) fn task_completed(&self) -> Option<bool> {
        self.task.as_ref().map(|task| task.completed)
    }

    /// This row's task text, if it is a task-level row built by
    /// [`super::FileIndex::query_tasks`]. `None` for page-level records.
    #[inline]
    #[must_use]
    pub(crate) fn task_text(&self) -> Option<&str> {
        self.task.as_ref().map(|task| task.text.as_str())
    }

    /// The indexed file's general metadata.
    #[inline]
    #[must_use]
    pub(crate) fn file(&self) -> &FileRecord {
        &self.file
    }

    /// The indexed file's parsed Note Metadata.
    #[inline]
    #[must_use]
    pub(crate) fn note(&self) -> &Note {
        &self.note
    }

    /// Resolves `path` against this record's file and note metadata.
    ///
    /// Resolves `file.*` accessors, `task.*` accessors, frontmatter fields,
    /// inline fields, and `tags`:
    /// - Frontmatter fields take precedence over inline fields with the same
    ///   key. See [`Note::fields`].
    /// - A well-formed path this record has no value for, such as a missing
    ///   frontmatter key or a `task.*` accessor on a page-level record,
    ///   resolves to [`FieldValue::Null`] instead of erroring.
    ///
    /// # Errors
    ///
    /// - [`QueryError::UnknownFieldPath`] if `path` is malformed. See
    ///   [`FieldPath::parse`].
    #[inline]
    pub(crate) fn field(&self, path: &str) -> Result<FieldValue, QueryError> {
        Ok(self.resolve(&FieldPath::parse(path)?))
    }

    /// Resolves an already-parsed `path`, applying any [`Self::flattened`]
    /// override.
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
            FieldPath::Metadata(key) => self
                .note
                .fields()
                .find(|field| field.key() == key.as_str())
                .map_or(FieldValue::Null, |field| field.value().clone()),
        }
    }

    /// Returns a copy of this record with `path` overridden to `value`.
    ///
    /// Used for exploded rows produced by [`QueryOutcome::flatten`].
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

/// Selects which markdown Notes a page-level or task-level query includes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Source {
    /// Every indexed markdown Note.
    All,
    /// Notes tagged with a markdown tag, or a sub-tag nested under it. For
    /// example, `#projects` also matches `#projects/active`.
    Tag(String),
    /// Notes located in `folder` or a directory nested under it.
    Folder(PathBuf),
}

impl Source {
    /// Whether `file` and its parsed `note` belong to this source.
    #[inline]
    #[must_use]
    pub(super) fn is_match(&self, file: &FileRecord, note: &Note) -> bool {
        match self {
            Self::All => true,
            Self::Tag(tag) => {
                note.tags().iter().any(|t| t.is_nested_under(tag))
            }
            Self::Folder(folder) => file.folder().starts_with(folder),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, path::Path};

    use super::*;
    use crate::index::FileIndex;

    mod fixtures {
        use std::{fs, path::Path};

        use super::*;

        /// Builds a [`QueryOutcome`] over every markdown Note in `files`
        /// (`(name, content)` pairs), written under `temp`.
        pub(super) fn outcome_for_files(
            temp: &Path,
            files: &[(&str, &str)],
        ) -> QueryOutcome {
            for (name, content) in files {
                fs::write(temp.join(name), content).expect("write note");
            }
            FileIndex::build(temp).expect("build index").query(&Source::All)
        }

        /// Builds a single-record [`QueryOutcome`] from one markdown Note's
        /// `content`.
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

            assert!(Source::All.is_match(record, note));
        }

        #[test]
        fn returns_true_when_note_has_matching_or_sub_tag() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("a.md"), "Tracked in #projects/active.")
                .expect("write file");
            let index = FileIndex::build(temp.path()).expect("build index");
            let record = index.record(Path::new("a.md")).expect("record");
            let note = index.note(Path::new("a.md")).expect("note");

            assert!(Source::Tag("#projects".to_owned()).is_match(record, note));
            assert!(!Source::Tag("#books".to_owned()).is_match(record, note));
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
                Source::Folder(PathBuf::from("projects"))
                    .is_match(record, note)
            );
            assert!(
                !Source::Folder(PathBuf::from("archive"))
                    .is_match(record, note)
            );
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
                Err(QueryError::UnknownFieldPath {
                    path: path.to_owned()
                })
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
                Err(QueryError::UnknownFieldPath {
                    path: "file.bogus".to_owned()
                })
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
                Err(QueryError::UnknownFieldPath {
                    path: "file.bogus".to_owned()
                })
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
            assert_eq!(lines.get(1), Some(&"| --- | --- |"));
            assert!(lines.iter().skip(2).any(|line| line.contains('5')));
            assert!(lines.iter().skip(2).any(|line| line.contains('3')));
        }

        #[test]
        fn renders_no_data_rows_for_an_empty_outcome() {
            let table = QueryOutcome::default()
                .table(&["Name"], &["file.name"])
                .expect("valid table");

            assert_eq!(table, "| Name |\n| --- |\n");
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
                Err(QueryError::UnknownFieldPath {
                    path: "file.bogus".to_owned()
                })
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
                Err(QueryError::UnknownFieldPath {
                    path: "file.bogus".to_owned()
                })
            );
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
                .query_tasks(&Source::All);

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
            let outcome = index.query(&Source::All);

            assert!(!outcome.is_empty());
            assert_eq!(outcome.len(), 1);
        }

        #[test]
        fn get_returns_record_or_none() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("a.md"), "# A").expect("write file");
            let index = FileIndex::build(temp.path()).expect("build index");
            let outcome = index.query(&Source::All);

            assert!(outcome.get(0).is_some());
            assert_eq!(outcome.get(1), None);
        }

        #[test]
        fn iter_and_into_iterator_yield_records() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("a.md"), "# A").expect("write file");
            let index = FileIndex::build(temp.path()).expect("build index");
            let outcome = index.query(&Source::All);

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
