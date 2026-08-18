//! Query source selection, field resolution, and result transformation.
//!
//! This module powers page-level results from [`FileIndex::query`] and
//! task-level rows from [`FileIndex::query_tasks`]. It provides a pipeline that
//! selects Notes via [`QuerySource`], pairs each matching Note with its
//! [`FileRecord`] as an [`IndexRecord`], and applies chained transformations
//! through [`QueryOutcome`].
//!
//! # Source Expression Language
//!
//! A source expression is a boolean combination of **leaves** joined by
//! **logical operators** and grouped with **parentheses**.
//!
//! ## Leaves
//!
//! ### Tags
//!
//! A `#`-prefixed identifier matches Notes carrying that tag or any nested
//! sub-tag. Tag names may contain letters, digits, underscores, hyphens, dots,
//! and forward slashes.
//!
//! ```text
//! #book              — matches #book, #book/fiction, #book/science
//! #projects/active   — matches #projects/active, #projects/active/rust
//! ```
//!
//! ### Paths
//!
//! A path leaf matches either an exact file path or every file under a folder
//! prefix. Paths may be bare (unquoted) or enclosed in single or double quotes.
//! Quoted paths support `\` escape sequences.
//!
//! ```text
//! books/             — matches every file under books/
//! books/dune.md      — matches only books/dune.md
//! "books/dune.md"    — same as above (quoted form)
//! ```
//!
//! A trailing `/` signals a folder prefix match. Without it, the path matches
//! only an exact file.
//!
//! ### File Classes
//!
//! File Class leaves match Notes whose frontmatter class field contains the
//! named class. Three syntax forms are available, each offering the same three
//! expansion depths.
//!
//! **Sigil form** (shorthand):
//!
//! ```text
//! @Book              — exact: matches only Book
//! @Book+             — children: matches Book and its direct subclasses
//! @Book*             — descendants: matches Book and all transitive subclasses
//! ```
//!
//! **Function form** (explicit):
//!
//! ```text
//! class(Book)                        — exact (default)
//! class(Book, children)              — children (positional argument)
//! class(Book, descendants)           — descendants (positional argument)
//! class(Book).with_children()        — children (chaining form)
//! class(Book).with_descendants()     — descendants (chaining form)
//! ```
//!
//! The positional argument and chaining form are mutually exclusive; providing
//! both is a syntax error.
//!
//! ## Logical Operators
//!
//! | Operator | Aliases         | Associativity |
//! | -------- | --------------- | ------------- |
//! | `NOT`    | `not`, `!`      | unary prefix  |
//! | `AND`    | `and`, `&&`     | left          |
//! | `OR`     | `or`, `\|\|`    | left          |
//!
//! Precedence from highest to lowest: `NOT` > `AND` > `OR`. Use parentheses to
//! override:
//!
//! ```text
//! #book and books/                   — AND binds tighter than OR
//! (#book or #movie) and !archive/    — parentheses override precedence
//! not not #book                      — repeated negation is allowed
//! ```
//!
//! ## Precedence Examples
//!
//! ```text
//! #book or books/ and not @Archived
//! ```
//! Parses as: `#book OR (books/ AND (NOT @Archived))`
//!
//! ```text
//! #book && !@Archived || @Movie
//! ```
//! Parses as: `(#book AND (NOT @Archived)) OR @Movie`
//!
//! ## Quoted Strings
//!
//! Double-quoted (`"..."`) and single-quoted (`'...'`) strings bypass keyword
//! classification. Backslash escapes are recognized:
//!
//! ```text
//! "path/with spaces"     — literal path with spaces
//! 'path\'s file.md'      — escaped single quote
//! ```
//!
//! ## Token Priority and Collisions
//!
//! Keywords (`class`, `and`, `or`, `not`) take priority over bare identifiers.
//! A file path segment that collides with a keyword (for example, a folder
//! named `class/` or `and/`) must be quoted:
//!
//! ```text
//! class/          — syntax error (lexes as keyword `class` + `/`)
//! "class/"        — lexes as a path
//! ```
//!
//! ## Matching Rules
//!
//! - **Tags:** A Note matches if any of its tags is exactly the leaf tag or is
//!   nested under it (for example, `#book` matches `#book/fiction`).
//! - **Paths:** A Note matches if its file path equals the leaf path exactly,
//!   or if its folder starts with the leaf path (for example, `books/` matches
//!   `books/dune.md`).
//! - **Classes:** A Note matches if any of its File Class values (read from the
//!   configured class field) appears in the resolved match set for the
//!   requested expansion mode.
//! - **Conjunction (`AND`):** All sub-expressions must match.
//! - **Disjunction (`OR`):** At least one sub-expression must match.
//! - **Negation (`NOT`):** The sub-expression must not match.
//!
//! ## Error Recovery
//!
//! Invalid expressions produce a [`QueryError::Syntax`] diagnostic pinpointing
//! the offending token with a repair hint. Common errors:
//!
//! - Missing operand: `#book and`
//! - Unmatched parenthesis: `(#book`
//! - Empty class name: `class()`
//! - Duplicate expansion mode: `class(Book, children).with_descendants()`
//! - Trailing tokens: `class(Book).with_children() extra`
//!
//! # Main Types
//!
//! - [`QuerySource`] is the top-level entry point: either all Notes or a parsed
//!   expression.
//! - [`source::QuerySourceExpr`] wraps the expression AST and exposes parsing,
//!   matching, and class-expansion mutation.
//! - [`SourceAtom`] is the leaf enum (tag, path, class) used by the expression
//!   tree.
//! - [`ClassExpansionMode`] controls the incremental depth model for File Class
//!   matching.
//! - [`IndexRecord`] pairs a [`FileRecord`] with its parsed [`Note`] and
//!   resolves `file.*`, `task.*`, frontmatter, tag, and inlinks fields for
//!   template rendering and CLI output.
//! - [`QueryOutcome`] stores result rows and provides chained transformation
//!   methods: [`filter`][`QueryOutcome::filter`],
//!   [`sort`][`QueryOutcome::sort`], [`limit`][`QueryOutcome::limit`],
//!   [`group_by`][`QueryOutcome::group_by`],
//!   [`flatten`][`QueryOutcome::flatten`]. Terminal rendering methods include
//!   [`table`][`QueryOutcome::table`], [`list`][`QueryOutcome::list`], and
//!   [`task_list`][`QueryOutcome::task_list`].
//! - [`QueryError`] reports malformed field paths, invalid expressions, and
//!   transformation constraint violations.
//!
//! # Submodules
//!
//! - [`choice`] builds selectable file options and borrowed filters.
//! - [`comparison`] implements filter comparison operators and expressions.
//! - [`error`] defines error types for field resolution and query
//!   transformations.
//! - [`field`] parses and resolves query field paths.
//! - [`filter`] parses and evaluates `.filter()`/`.where()` expressions.
//! - [`logic`] provides the shared logical-expression tree and precedence
//!   parser.
//! - [`record`] implements query rows and field resolution.
//! - [`sort`] defines equality and ordering for resolved [`FieldValue`]
//!   instances.
//! - [`source`] parses and evaluates page source expressions.
//!
//! [`FieldValue`]: crate::note::FieldValue
//! [`FileRecord`]: crate::index::FileRecord
//! [`FileIndex::query`]: crate::index::FileIndex::query
//! [`FileIndex::query_tasks`]: crate::index::FileIndex::query_tasks
//! [`Note`]: crate::note::Note

mod choice;
mod comparison;
mod error;
mod field;
mod filter;
mod logic;
mod record;
mod sort;
mod source;

use std::path::PathBuf;

pub(crate) use choice::{FileOption, FileOptionFilter, FrontmatterFieldKeys};
#[cfg(test)]
pub(crate) use error::{FieldPathError, QuerySyntaxError};
pub use error::{QueryDialect, QueryError};
use field::FieldPath;
pub(crate) use field::FileField;
use filter::FilterExpr;
pub use record::IndexRecord;
use sort::SortKey;
pub(crate) use sort::SortOrder;
pub use source::{ClassExpansionMode, QuerySource};
pub(crate) use source::{SourceAtom, class_values};

use crate::{
    index::{FileIndex, FileRecord},
    note::{FieldValue, Note},
};

/// Executes a page-level source query, returning one [`IndexRecord`] per Note
/// matching `source`.
///
/// Consumes the [`FileIndex`] to pair [`FileRecord`] and [`Note`] entries,
/// resolving inlinks from the provided map. Uses `class_field` to read File
/// Class values from each Note's frontmatter when `source` contains class
/// atoms.
///
/// # Examples
///
/// ```ignore
/// # use traces_pkm::query::{query, QuerySource};
/// # use traces_pkm::index::FileIndex;
/// # let index = FileIndex::default();
/// # let source = QuerySource::parse("#book").unwrap();
/// # let outcome = query(index, &source, "class");
/// ```
#[must_use]
pub(crate) fn query(
    index: FileIndex,
    source: &QuerySource,
    class_field: &str,
) -> QueryOutcome {
    let FileIndex {
        records,
        notes,
        mut inlinks,
    } = index;
    let records = matched_pairs(records, notes, source, class_field)
        .map(|(file, note)| record_with_inlinks(file, note, &mut inlinks))
        .collect();
    QueryOutcome::new(records)
}

/// Executes a task-level source query, returning one [`IndexRecord`] per task
/// item across all Notes matching `source`.
///
/// Each matching Note is expanded into multiple task-level rows via
/// [`IndexRecord::with_task`]. Uses `class_field` to read File Class values
/// from each Note's frontmatter when `source` contains class atoms.
///
/// # Examples
///
/// ```ignore
/// # use traces_pkm::query::{query_tasks, QuerySource};
/// # use traces_pkm::index::FileIndex;
/// # let index = FileIndex::default();
/// # let source = QuerySource::parse("#book").unwrap();
/// # let outcome = query_tasks(index, &source, "class");
/// ```
#[must_use]
pub(crate) fn query_tasks(
    index: FileIndex,
    source: &QuerySource,
    class_field: &str,
) -> QueryOutcome {
    let FileIndex {
        records: files,
        notes,
        mut inlinks,
    } = index;
    let mut records = Vec::new();
    for (file, note) in matched_pairs(files, notes, source, class_field) {
        let base = record_with_inlinks(file, note, &mut inlinks);
        let mut tasks = base.note().tasks().peekable();
        while let Some(item) = tasks.next() {
            let completed = item.is_completed();
            let text = item.text().to_owned();
            if tasks.peek().is_some() {
                records.push(base.clone().with_task(completed, text));
            } else {
                drop(tasks);
                records.push(base.with_task(completed, text));
                break;
            }
        }
    }
    QueryOutcome::new(records)
}

/// Zips and filters file records and note contents matching the given query
/// source.
///
/// Assumes both vectors are sorted by file path to perform a linear-time scan.
fn matched_pairs<'a>(
    records: Vec<FileRecord>,
    notes: Vec<Note>,
    source: &'a QuerySource,
    class_field: &'a str,
) -> impl Iterator<Item = (FileRecord, Note)> + 'a {
    let mut files = records.into_iter().peekable();
    notes.into_iter().filter_map(move |note| {
        while files.peek().is_some_and(|file| file.path() < note.path()) {
            files.next();
        }
        let file = files.next_if(|file| file.path() == note.path())?;
        source.is_match(&file, &note, class_field).then_some((file, note))
    })
}

/// Constructs a new [`IndexRecord`] with inbound link paths populated from
/// `inlinks`.
fn record_with_inlinks(
    file: FileRecord,
    note: Note,
    inlinks: &mut std::collections::HashMap<PathBuf, Vec<PathBuf>>,
) -> IndexRecord {
    let links = inlinks.remove(file.path()).unwrap_or_default();
    IndexRecord::new(file, note).with_inlinks(links)
}

/// An ordered collection of [`IndexRecord`] rows produced by an index query.
///
/// Page-level outcomes contain one row per Note, while task-level outcomes
/// contain one row per task item. Transformation methods consume and return
/// a [`QueryOutcome`], enabling method chaining.
///
/// # Examples
///
/// ```ignore
/// use traces_pkm::query::QueryOutcome;
///
/// let outcome = QueryOutcome::default();
/// assert!(outcome.is_empty());
/// ```
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

    /// Retains only records matching the filter expression.
    ///
    /// # Syntax Specification
    ///
    /// | Element               | Syntax Example                  | Description                                             |
    /// | :-------------------- | :------------------------------ | :------------------------------------------------------ |
    /// | **Comparisons**       | `rating > 7`                    | Compares fields using `==`, `!=`, `>=`, `<=`, `>`, `<`. |
    /// | **Functions**         | `contains(tags, "#book")`       | Checks list/tag membership or substring containment.    |
    /// | **Logical Operators** | `a and b or not c`              | Standard logic (`and`/`&&`, `or`/`|`, `not`/`!`).       |
    /// | **Grouping**          | `(rating > 5) and ok`           | Overrides operator precedence.                          |
    /// | **Literals**          | `"text"`, `123`, `true`, `null` | Quoted strings, numbers, booleans, and nulls.           |
    ///
    /// # Matching Rules
    ///
    /// - **Text Equality:** `==` and `!=` compare [`String`], `Date`, and
    ///   `Duration` values by text representation.
    /// - **Type Mismatch:** Mismatched data types never match except under
    ///   `!=`.
    /// - **Missing Fields:** Records missing the requested field (`Null`) fail
    ///   equality and ordering checks, but match `!=`.
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
    /// Uses Rust raw identifier syntax (`r#where`) because `where` is a
    /// reserved keyword; `where` is this query API's name for the same
    /// operation as `filter`. Refer to [`Self::filter`] for full syntax details
    /// and matching behavior.
    ///
    /// # Errors
    ///
    /// - [`QueryError::Syntax`] if `expr` cannot be parsed.
    /// - [`QueryError::FieldPath`] if a field path referenced in `expr` is
    ///   malformed.
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
    ///
    /// # Examples
    ///
    /// ```ignore
    /// use traces_pkm::query::QueryOutcome;
    ///
    /// let outcome = QueryOutcome::default();
    /// let limited = outcome.limit(10).unwrap();
    /// assert_eq!(limited.len(), 0);
    /// ```
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
    /// This enables template loops or terminal renderers to detect group
    /// transitions by comparing adjacent records.
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
    /// Behavior:
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
    /// - [`QueryError::FieldPath`] if `path` cannot be parsed as a valid field
    ///   path.
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
    /// `columns`. Table formatting uses the
    /// [`ASCII_MARKDOWN`][`comfy_table::presets::ASCII_MARKDOWN`] preset of
    /// `comfy-table` to align column widths. Pipe characters (`|`)
    /// are escaped and newlines collapse into spaces to prevent table layout
    /// corruption.
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
    ///   fields (built by [`query`] instead of [`query_tasks`]).
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
    ///
    /// Provides shared sorting logic for [`Self::sort`] and [`Self::group_by`],
    /// treating [`FieldValue::Null`] as the minimum value.
    ///
    /// # Performance
    ///
    /// Runs key resolution in O(n) time using [`slice::sort_by_cached_key`],
    /// resolving `path` once per record rather than on every comparison made
    /// by a standard `sort_by` closure.
    ///
    /// # Errors
    ///
    /// - [`QueryError::FieldPath`] if `path` cannot be parsed as a valid field
    ///   path.
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

/// Converts the [`QueryOutcome`] into an iterator over owned [`IndexRecord`]
/// rows.
impl IntoIterator for QueryOutcome {
    type IntoIter = std::vec::IntoIter<Self::Item>;
    type Item = IndexRecord;

    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        self.records.into_iter()
    }
}

/// Creates an iterator over borrowed [`IndexRecord`] rows from the
/// [`QueryOutcome`].
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

/// Escapes pipes (`|`) and collapses newlines to spaces to preserve table
/// formatting.
fn escape_table_text(text: &str) -> String {
    text.replace('\n', " ").replace('|', "\\|")
}

/// Formats a [`FieldValue`] into plain text suitable for Markdown table cells.
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

    mod index_record {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn file_accessor_returns_the_bundled_file_record() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("a.md"), "Filed under #tag.")
                .expect("write file");
            let index = FileIndex::build(temp.path()).expect("build index");
            let file = index.record(Path::new("a.md")).expect("record").clone();
            let note = index.note(Path::new("a.md")).expect("note").clone();

            let record = IndexRecord::new(file.clone(), note);

            assert_eq!(record.file(), &file);
        }

        #[test]
        fn note_accessor_returns_the_bundled_note() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("a.md"), "Filed under #tag.")
                .expect("write file");
            let index = FileIndex::build(temp.path()).expect("build index");
            let file = index.record(Path::new("a.md")).expect("record").clone();
            let note = index.note(Path::new("a.md")).expect("note").clone();

            let record = IndexRecord::new(file, note.clone());

            assert_eq!(record.note(), &note);
        }

        #[test]
        fn with_task_sets_task_completed() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("a.md"), "body").expect("write file");
            let index = FileIndex::build(temp.path()).expect("build index");
            let file = index.record(Path::new("a.md")).expect("record").clone();
            let note = index.note(Path::new("a.md")).expect("note").clone();

            let record =
                IndexRecord::new(file, note).with_task(true, "Buy milk");

            assert_eq!(record.task_completed(), Some(true));
        }

        #[test]
        fn with_task_sets_task_text() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("a.md"), "body").expect("write file");
            let index = FileIndex::build(temp.path()).expect("build index");
            let file = index.record(Path::new("a.md")).expect("record").clone();
            let note = index.note(Path::new("a.md")).expect("note").clone();

            let record =
                IndexRecord::new(file, note).with_task(false, "Buy milk");

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
                Err(QueryError::LimitOutOfRange {
                    value: -1
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
                Err(QueryError::FieldPath(FieldPathError::new(
                    "file.bogus",
                    None
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
                Err(QueryError::FieldPath(FieldPathError::new(
                    "file.bogus",
                    None
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
                Err(QueryError::TaskListRequiresTaskRows)
            );
        }
    }

    mod query_outcome {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn len_returns_zero_for_an_empty_outcome() {
            let empty = QueryOutcome::default();
            assert_eq!(empty.len(), 0);
        }

        #[test]
        fn is_empty_returns_true_for_an_empty_outcome() {
            let empty = QueryOutcome::default();
            assert!(empty.is_empty());
        }

        #[test]
        fn len_returns_record_count_for_a_non_empty_outcome() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("a.md"), "# A").expect("write file");
            let index = FileIndex::build(temp.path()).expect("build index");
            let outcome = index.query(&QuerySource::All);

            assert_eq!(outcome.len(), 1);
        }

        #[test]
        fn is_empty_returns_false_for_a_non_empty_outcome() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("a.md"), "# A").expect("write file");
            let index = FileIndex::build(temp.path()).expect("build index");
            let outcome = index.query(&QuerySource::All);

            assert!(!outcome.is_empty());
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
