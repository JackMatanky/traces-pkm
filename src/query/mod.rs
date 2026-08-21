//! Query source selection, field resolution, and result transformation.
//!
//! [`QueryService`] powers page-level results ([`QueryService::query`]) and
//! task-level rows ([`QueryService::query_tasks`]). It consumes the sorted
//! records, notes, and inbound-link map a [`FileIndex`] decomposes into via
//! [`FileIndex::into_parts`] — `query` never names `FileIndex` itself,
//! keeping `index` and `query` free of a mutual dependency. The pipeline
//! selects Notes via [`QuerySource`], pairs each matching Note with its
//! [`FileRecord`] as a [`QueryRecord`], and applies chained transformations
//! through [`QueryRecordSet`].
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
//! ### Paths
//!
//! A path leaf matches an exact file path, every file under a folder prefix,
//! or an explicit glob.
//!
//! ### File Classes
//!
//! File Class leaves match Notes whose frontmatter class field contains the
//! named class.
//!
//! # Main Types
//!
//! - [`QueryService`] drives query execution: [`QueryService::query`] and
//!   [`QueryService::query_tasks`] consume a decomposed [`FileIndex`] and a
//!   [`QuerySource`], producing a [`QueryRecordSet`].
//! - [`QuerySource`] is the top-level entry point: either all Notes or a parsed
//!   expression.
//! - [`source::QuerySourceExpr`] wraps the expression AST.
//! - [`QueryRecord`] pairs a [`FileRecord`] with its parsed [`Note`] and
//!   resolves `file.*`, `task.*`, frontmatter, tag, and inlinks fields.
//! - [`QueryRecordSet`] stores result rows and provides chained transformation
//!   methods (`filter`, `sort`, `limit`, `group_by`, `flatten`) and terminal
//!   rendering methods (`table`, `list`, `task_list`).
//! - [`QueryError`] reports malformed field paths, invalid expressions, and
//!   transformation constraint violations.
//!
//! [`FieldValue`]: crate::note::FieldValue
//! [`FileRecord`]: crate::file::FileRecord
//! [`FileIndex`]: crate::index::FileIndex
//! [`FileIndex::into_parts`]: crate::index::FileIndex::into_parts
//! [`Note`]: crate::note::Note

mod comparison;
mod error;
mod field;
mod filter;
mod logic;
mod record;
mod sort;
mod source;

#[cfg(test)]
pub(crate) use error::{FieldPathError, QuerySyntaxError};
pub use error::{QueryDialect, QueryError};
use field::FieldPath;
pub(crate) use field::FileField;
pub use record::{QueryRecord, QueryRecordSet};
pub(crate) use sort::SortOrder;
pub use source::{ClassExpansionMode, QuerySource};
pub(crate) use source::{
    FileClassExpander, QuerySourceExpr, SourceAtom, compile_glob,
    resolve_classes,
};

use crate::{file::FileRecord, index::InlinkMap, note::Note};

/// Executes queries over decomposed [`FileIndex`] data, matching a
/// [`QuerySource`] against records/notes and resolving File Class values
/// through a fixed `class_field`.
///
/// Mirrors `ConfigService`/`SchemaService`/[`crate::index::IndexerService`]:
/// fixed configuration (here, the frontmatter class field name) with methods
/// that read against it, rather than an extra parameter repeated at every
/// call site. Takes decomposed `records`/`notes`/`inlinks` — never a
/// [`FileIndex`] — so `query` has no dependency on `index`; callers get these
/// via [`FileIndex::into_parts`].
///
/// [`FileIndex`]: crate::index::FileIndex
/// [`FileIndex::into_parts`]: crate::index::FileIndex::into_parts
#[derive(Clone, Debug)]
pub struct QueryService {
    class_field: String,
}

impl QueryService {
    /// Creates a service that reads File Class values from `class_field`.
    #[inline]
    #[must_use]
    pub fn new<S: Into<String>>(class_field: S) -> Self {
        Self {
            class_field: class_field.into(),
        }
    }

    /// Executes a page-level query, returning one [`QueryRecord`] per Note
    /// matching `source`.
    ///
    /// Pairs `records`/`notes` and resolves inlinks from `inlinks`. Uses this
    /// service's `class_field` to read File Class values from each Note's
    /// frontmatter when `source` contains class atoms.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// # use traces_pkm::query::{QueryService, QuerySource};
    /// # use traces_pkm::index::FileIndex;
    /// # let (records, notes, inlinks) = FileIndex::default().into_parts();
    /// # let source = QuerySource::parse("#book").unwrap();
    /// # let outcome = QueryService::new("class").query(records, notes, inlinks, &source);
    /// ```
    #[inline]
    pub fn query(
        &self,
        records: Vec<FileRecord>,
        notes: Vec<Note>,
        mut inlinks: InlinkMap,
        source: &QuerySource,
    ) -> QueryRecordSet {
        let records = matched_base_records(
            records,
            notes,
            &mut inlinks,
            source,
            &self.class_field,
        )
        .collect();
        QueryRecordSet::new(records)
    }

    /// Executes a task-level query, returning one [`QueryRecord`] per task
    /// item across all Notes matching `source`.
    ///
    /// Each matching Note is expanded into multiple task-level rows via
    /// [`QueryRecord::with_task`]. Uses this service's `class_field` to read
    /// File Class values from each Note's frontmatter when `source` contains
    /// class atoms.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// # use traces_pkm::query::{QueryService, QuerySource};
    /// # use traces_pkm::index::FileIndex;
    /// # let (records, notes, inlinks) = FileIndex::default().into_parts();
    /// # let source = QuerySource::parse("#book").unwrap();
    /// # let outcome = QueryService::new("class").query_tasks(records, notes, inlinks, &source);
    /// ```
    #[inline]
    pub fn query_tasks(
        &self,
        records: Vec<FileRecord>,
        notes: Vec<Note>,
        mut inlinks: InlinkMap,
        source: &QuerySource,
    ) -> QueryRecordSet {
        let mut out = Vec::new();
        for base in matched_base_records(
            records,
            notes,
            &mut inlinks,
            source,
            &self.class_field,
        ) {
            let Some(note) = base.note() else {
                continue;
            };
            let mut tasks = note.tasks().peekable();
            while let Some(item) = tasks.next() {
                let completed = item.is_completed();
                let text = item.text().to_owned();
                if tasks.peek().is_some() {
                    out.push(base.clone().with_task(completed, text));
                } else {
                    drop(tasks);
                    out.push(base.with_task(completed, text));
                    break;
                }
            }
        }
        QueryRecordSet::new(out)
    }
}

/// Zips and filters file records and note contents matching the given query
/// source.
///
/// Iterates `records` (every indexed file) as the primary sequence, pairing
/// each with the [`Note`] at the same path when one exists — a `file` may have
/// no [`Note`] (non-Markdown files matched by a `file`-typed Schema field).
///
/// Assumes both vectors are sorted by file path to perform a linear-time scan.
fn matched_pairs<'a>(
    records: Vec<FileRecord>,
    notes: Vec<Note>,
    source: &'a QuerySource,
    class_field: &'a str,
) -> impl Iterator<Item = (FileRecord, Option<Note>)> + 'a {
    let mut notes = notes.into_iter().peekable();
    records.into_iter().filter_map(move |file| {
        while notes.peek().is_some_and(|note| note.path() < file.path()) {
            notes.next();
        }
        let note = notes.next_if(|note| note.path() == file.path());
        source
            .is_match(&file, note.as_ref(), class_field)
            .then_some((file, note))
    })
}

/// Pairs matched records/notes ([`matched_pairs`]) and resolves each into a
/// [`QueryRecord`] with inbound links attached ([`QueryRecord::from_parts`]).
///
/// Consolidates the shared setup [`QueryService::query`] and
/// [`QueryService::query_tasks`] both need, leaving each method's own loop
/// body as its only distinct contribution.
fn matched_base_records<'a>(
    records: Vec<FileRecord>,
    notes: Vec<Note>,
    inlinks: &'a mut InlinkMap,
    source: &'a QuerySource,
    class_field: &'a str,
) -> impl Iterator<Item = QueryRecord> + 'a {
    matched_pairs(records, notes, source, class_field)
        .map(move |(file, note)| QueryRecord::from_parts(file, note, inlinks))
}
#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::{Path, PathBuf},
    };

    use super::*;
    use crate::{index::IndexerService, note::FieldValue};

    mod fixtures {
        use std::{fs, path::Path};

        use super::*;

        /// Builds a [`QueryOutcome`] over every Markdown Note in `files`
        /// written under `temp`.
        pub(super) fn outcome_for_files(
            temp: &Path,
            files: &[(&str, &str)],
        ) -> QueryRecordSet {
            for (name, content) in files {
                fs::write(temp.join(name), content).expect("write note");
            }
            let index = IndexerService::new(temp).build().expect("build index");
            let (records, notes, inlinks) = index.into_parts();
            QueryService::new("class").query(
                records,
                notes,
                inlinks,
                &QuerySource::All,
            )
        }

        /// Builds a single-record [`QueryOutcome`] from a single Markdown
        /// Note's content.
        pub(super) fn outcome_for(
            temp: &Path,
            content: &str,
        ) -> QueryRecordSet {
            outcome_for_files(temp, &[("note.md", content)])
        }

        /// Finds a [`FileRecord`] by path in a sorted records slice.
        pub(super) fn find_record<'a>(
            records: &'a [crate::file::FileRecord],
            path: &Path,
        ) -> &'a crate::file::FileRecord {
            records.iter().find(|r| r.path() == path).expect("record not found")
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
            let index =
                IndexerService::new(temp.path()).build().expect("build index");
            let file = find_record(index.records(), Path::new("a.md")).clone();
            let note = index.note(Path::new("a.md")).expect("note").clone();

            let record = QueryRecord::new(file.clone(), Some(note));

            assert_eq!(record.file(), &file);
        }

        #[test]
        fn note_accessor_returns_the_bundled_note() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("a.md"), "Filed under #tag.")
                .expect("write file");
            let index =
                IndexerService::new(temp.path()).build().expect("build index");
            let file = find_record(index.records(), Path::new("a.md")).clone();
            let note = index.note(Path::new("a.md")).expect("note").clone();

            let record = QueryRecord::new(file, Some(note.clone()));

            assert_eq!(record.note(), Some(&note));
        }

        #[test]
        fn with_task_sets_task_completed() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("a.md"), "body").expect("write file");
            let index =
                IndexerService::new(temp.path()).build().expect("build index");
            let file = find_record(index.records(), Path::new("a.md")).clone();
            let note = index.note(Path::new("a.md")).expect("note").clone();

            let record =
                QueryRecord::new(file, Some(note)).with_task(true, "Buy milk");

            assert_eq!(record.task_completed(), Some(true));
        }

        #[test]
        fn with_task_sets_task_text() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("a.md"), "body").expect("write file");
            let index =
                IndexerService::new(temp.path()).build().expect("build index");
            let file = find_record(index.records(), Path::new("a.md")).clone();
            let note = index.note(Path::new("a.md")).expect("note").clone();

            let record =
                QueryRecord::new(file, Some(note)).with_task(false, "Buy milk");

            assert_eq!(record.task_text(), Some("Buy milk"));
        }

        #[test]
        fn task_accessors_return_none_for_page_level_records() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("a.md"), "body").expect("write file");
            let index =
                IndexerService::new(temp.path()).build().expect("build index");
            let file = find_record(index.records(), Path::new("a.md")).clone();
            let note = index.note(Path::new("a.md")).expect("note").clone();

            let record = QueryRecord::new(file, Some(note));

            assert_eq!(record.task_completed(), None);
            assert_eq!(record.task_text(), None);
        }

        #[test]
        fn with_inlinks_sets_the_inlinks_accessor() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("a.md"), "body").expect("write file");
            let index =
                IndexerService::new(temp.path()).build().expect("build index");
            let file = find_record(index.records(), Path::new("a.md")).clone();
            let note = index.note(Path::new("a.md")).expect("note").clone();

            let record = QueryRecord::new(file, Some(note))
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

        fn outcome_of_three(temp: &Path) -> QueryRecordSet {
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
            let table = QueryRecordSet::default()
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
            let table = QueryRecordSet::default()
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
                QueryRecordSet::default().list("rating").expect("valid list");

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
            let index =
                IndexerService::new(temp.path()).build().expect("build index");
            let (records, notes, inlinks) = index.into_parts();
            let outcome = QueryService::new("class").query_tasks(
                records,
                notes,
                inlinks,
                &QuerySource::All,
            );

            let rendered = outcome.task_list().expect("valid task_list");

            assert_eq!(rendered, "- [ ] Buy milk\n- [x] Walk dog\n");
        }

        #[test]
        fn renders_an_empty_string_for_an_empty_outcome() {
            let rendered =
                QueryRecordSet::default().task_list().expect("valid task_list");

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
            let empty = QueryRecordSet::default();
            assert_eq!(empty.len(), 0);
        }

        #[test]
        fn is_empty_returns_true_for_an_empty_outcome() {
            let empty = QueryRecordSet::default();
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
}
