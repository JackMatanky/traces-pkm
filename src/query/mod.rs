//! Query source selection, field resolution, and result transformation.
//!
//! [`QueryService`] borrows a [`FileIndex`] and executes a [`QueryBuilder`].
//! The pipeline selects Notes via [`SourceSelector`], pairs each matching Note
//! with its [`FileBase`] as a [`QueryRow`], and applies chained
//! transformations through [`QuerySet`].
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
//! - [`QueryService`] drives query execution: [`QueryService::execute`] borrows
//!   a [`FileIndex`] and a [`QueryBuilder`], producing a [`QuerySet`].
//! - [`QueryBuilder`] describes page/task mode, source selection, and ordered
//!   transformations.
//! - [`SourceSelector`] is the top-level entry point: either all Notes or a
//!   parsed expression.
//! - [`QueryRow`] pairs a [`FileBase`] with its parsed [`Note`] and resolves
//!   `file.*`, `task.*`, frontmatter, tag, and inlinks fields.
//! - [`QuerySet`] stores result rows and provides chained transformation
//!   methods (`filter`, `sort`, `limit`, `group_by`, `flatten`) and terminal
//!   rendering methods (`table`, `list`, `task_list`).
//! - [`QueryError`] reports malformed field paths, invalid expressions, and
//!   transformation constraint violations.
//!
//! [`FileBase`]: crate::file::FileBase
//! [`FileIndex`]: crate::index::FileIndex
//! [`Note`]: crate::note::Note

mod builder;
mod error;
mod format;
mod grammar;
mod plan;
mod row;
mod service;
mod sort;
mod value;

pub use builder::QueryBuilder;
use builder::QueryMode;
#[cfg(test)]
pub(crate) use error::{FieldPathError, QuerySyntaxError};
pub use error::{QueryBuilderError, QueryDialect, QueryError, QueryResult};
pub(crate) use format::TaskPathStyle;
pub use grammar::SourceSelector;
pub(crate) use grammar::{
    ClassExpansionMode, FieldPath, FileClassExpander, FileField, SourceAtom,
    SourceExpr,
};
use plan::{QueryPlan, QueryTransform};
pub use row::{QueryRow, QuerySet};
pub use service::QueryService;
pub(crate) use sort::SortOrder;

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::{Path, PathBuf},
        sync::Arc,
    };

    use super::{format::QueryDisplayFormat, *};
    use crate::{index::IndexerService, note::NoteFieldValue};

    mod fixtures {
        use std::{fs, path::Path};

        use super::*;

        /// Builds a [`QuerySet`] over every Markdown Note in `files`
        /// written under `temp`.
        pub(super) fn outcome_for_files(
            temp: &Path,
            files: &[(&str, &str)],
        ) -> QuerySet {
            for (name, content) in files {
                fs::write(temp.join(name), content).expect("write note");
            }
            let index = Arc::new(
                IndexerService::new(temp).build().expect("build index"),
            );
            QueryService::new("class")
                .execute(&index, QueryBuilder::pages(SourceSelector::All))
        }

        /// Builds a single-record [`QuerySet`] from a single Markdown
        /// Note's content.
        pub(super) fn outcome_for(temp: &Path, content: &str) -> QuerySet {
            outcome_for_files(temp, &[("note.md", content)])
        }

        /// Finds a [`FileEntry`] by path in a sorted entries slice.
        pub(super) fn find_entry<'a>(
            entries: &'a [crate::index::FileEntry],
            path: &Path,
        ) -> &'a crate::index::FileEntry {
            entries
                .iter()
                .find(|e| e.file().path() == path)
                .expect("entry not found")
        }

        /// Finds a [`FileBase`] by path in a sorted entries slice.
        pub(super) fn find_base<'a>(
            entries: &'a [crate::index::FileEntry],
            path: &Path,
        ) -> &'a crate::file::FileBase {
            find_entry(entries, path).file()
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
            let index = Arc::new(
                IndexerService::new(temp.path()).build().expect("build index"),
            );
            let file = find_base(index.entries(), Path::new("a.md"));
            let outcome = QueryService::new("class")
                .execute(&index, QueryBuilder::pages(SourceSelector::All));
            let record = outcome.get(0).expect("record");
            assert_eq!(record.file(), file);
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
            let record = outcome.get(0).expect("record");
            assert_eq!(record.note(), Some(note));
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
            let record = outcome.get(0).expect("record");
            assert_eq!(record.task_completed(), Some(true));
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
            let record = outcome.get(0).expect("record");
            assert_eq!(record.task_text(), Some("Buy milk"));
        }

        #[test]
        fn task_accessors_return_none_for_page_level_records() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("a.md"), "body").expect("write file");
            let outcome = outcome_for(temp.path(), "body");
            let record = outcome.get(0).expect("record");

            assert_eq!(record.task_completed(), None);
            assert_eq!(record.task_text(), None);
        }

        #[test]
        fn inlinks_accessor_returns_the_bundled_inlinks() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let outcome = outcome_for_files(temp.path(), &[
                ("target.md", "# Target"),
                ("b.md", "[[target]]"),
            ]);
            let record = outcome
                .iter()
                .find(|record| record.file().path() == Path::new("target.md"))
                .expect("target record");

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
                Ok(NoteFieldValue::String("notes/todo.md".to_owned()))
            );
            assert_eq!(
                record.field("file.name"),
                Ok(NoteFieldValue::String("todo".to_owned()))
            );
            assert_eq!(
                record.field("file.folder"),
                Ok(NoteFieldValue::String("notes".to_owned()))
            );
            assert_eq!(
                record.field("file.size"),
                Ok(NoteFieldValue::Number(4.0))
            );
        }

        #[test]
        fn resolves_dataview_style_time_accessors_from_file_record() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let outcome = outcome_for(temp.path(), "body");
            let record = outcome.get(0).expect("record");
            let file = record.file();

            assert_eq!(
                record.field("file.mtime"),
                Ok(NoteFieldValue::Date(
                    file.modified_at().to_datetime_string()
                ))
            );
            assert_eq!(
                record.field("file.mdate"),
                Ok(NoteFieldValue::Date(file.modified_at().to_date_string()))
            );
            assert_eq!(
                record.field("file.ctime"),
                Ok(NoteFieldValue::Date(
                    file.created_at_or_modified().to_datetime_string()
                ))
            );
            assert_eq!(
                record.field("file.cdate"),
                Ok(NoteFieldValue::Date(
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

            assert_eq!(record.field("rating"), Ok(NoteFieldValue::Number(5.0)));
            assert_eq!(
                record.field("Status"),
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
            let record = outcome.get(0).expect("record");

            assert_eq!(
                record.field("status"),
                Ok(NoteFieldValue::String("Approved".to_owned()))
            );
        }

        #[test]
        fn resolves_tags_as_a_list_of_tag_strings() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let outcome = outcome_for(temp.path(), "Filed under #book #read");
            let record = outcome.get(0).expect("record");

            assert_eq!(
                record.field("tags"),
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
            let record = outcome
                .iter()
                .find(|record| record.file().path() == Path::new("target.md"))
                .expect("target record");

            assert_eq!(
                record.field("inlinks"),
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
            let record = outcome.get(0).expect("record");

            assert_eq!(
                record.field("inlinks"),
                Ok(NoteFieldValue::List(vec![]))
            );
        }

        #[test]
        fn missing_field_resolves_to_null() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let outcome = outcome_for(temp.path(), "body, no frontmatter");
            let record = outcome.get(0).expect("record");

            assert_eq!(record.field("no_such_field"), Ok(NoteFieldValue::Null));
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
            let record = outcome.get(0).expect("record");
            assert_eq!(
                record.field("task.completed"),
                Ok(NoteFieldValue::Bool(true))
            );
            assert_eq!(
                record.field("task.text"),
                Ok(NoteFieldValue::String("Buy milk".to_owned()))
            );
        }

        #[test]
        fn task_fields_resolve_to_null_on_page_level_records() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let outcome = outcome_for(temp.path(), "body");
            let record = outcome.get(0).expect("record");

            assert_eq!(
                record.field("task.completed"),
                Ok(NoteFieldValue::Null)
            );
            assert_eq!(record.field("task.text"), Ok(NoteFieldValue::Null));
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
                Err(QueryError::Request(QueryBuilderError::LimitOutOfRange {
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
                .map(|record| record.field("category").expect("valid path"))
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
                Err(QueryError::Request(QueryBuilderError::FieldPath(
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
                .map(|record| record.field("authors").expect("valid path"))
                .collect();
            assert_eq!(authors, [
                NoteFieldValue::String("Alice".to_owned()),
                NoteFieldValue::String("Bob".to_owned()),
            ]);
            // Every other field still resolves from the original record.
            for record in &flattened {
                assert_eq!(
                    record.field("title"),
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
                flattened.get(0).expect("record").field("rating"),
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
                .map(|record| record.field("tags").expect("valid path"))
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
                Err(QueryError::Request(QueryBuilderError::FieldPath(
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
                filtered.get(0).expect("record").field("authors"),
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
                .map(|record| {
                    (
                        record.field("authors").expect("valid authors"),
                        record.field("tags").expect("valid tags"),
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

        /// Two chained `.filter()` calls and one combined filter expression
        /// must reach the same rows once both sides are materialized,
        /// proving `QueryRecordSet`'s manual `PartialEq` compares evaluated
        /// content, not the (structurally different) pending plan each side
        /// accumulated.
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

        /// Chained `.sort().limit(n)` on a `QueryRecordSet` (the CTE path)
        /// must match a full sort's first `n` rows, including tie order —
        /// the same property `request.rs`'s
        /// `top_k_matches_full_sort_order_for_tied_keys` proves for
        /// `QueryRequest` (the pre-fetch path) — confirming the deferred
        /// plan reaches the same `Sort`+`Limit` -> `TopK` fusion.
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

        /// Two branches derived from the same base `QueryRecordSet` must
        /// each see every base row, and the base itself must be untouched —
        /// proving `.filter()`/etc. consume-and-return a new value rather
        /// than mutating shared state.
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
