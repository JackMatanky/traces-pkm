//! Page-level query source selection, field resolution, and outcome
//! transformations.
//!
//! Main components:
//! - [`Source`]: Selects which Notes a page-level query includes.
//! - [`IndexRecord`]: Pairs a [`FileRecord`] with its parsed [`Note`] and
//!   resolves fields by path.
//! - [`QueryOutcome`]: Page-level query result collection supporting method
//!   chaining ([`QueryOutcome::filter`], [`QueryOutcome::sort`],
//!   [`QueryOutcome::limit`], [`QueryOutcome::group_by`],
//!   [`QueryOutcome::flatten`]).
//! - [`QueryError`]: Errors returned by field resolution and outcome
//!   transformations.

mod error;
mod filter;
mod operators;
mod sort;

use std::path::PathBuf;

pub(crate) use error::QueryError;
use filter::FilterExpr;
use sort::sort_key_cmp;

use super::file::{FileField, FileRecord};
use crate::note::{FieldValue, Note};

/// Iterable, page-level collection of [`IndexRecord`] values returned by
/// [`super::FileIndex::query`].
///
/// [`Self::filter`], [`Self::sort`], [`Self::limit`], [`Self::group_by`],
/// and [`Self::flatten`] each consume this outcome and return a new,
/// transformed one, so calls chain naturally:
/// `outcome.filter("rating > 7")?.sort("rating", true)?.limit(10)?`.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct QueryOutcome {
    records: Vec<IndexRecord>,
}

impl QueryOutcome {
    /// Wraps `records` as a page-level query result.
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
    /// `expr` can be a binary comparison (e.g. `"rating > 7"` or `"status ==
    /// \"done\""`), a function call (e.g. `"contains(tags, \"#book\")"`),
    /// or a boolean combination using `AND`, `OR`, `NOT`, and nested
    /// parentheses `( ... )`.
    ///
    /// # Matching Rules
    ///
    /// - **Operators**: `==`, `!=`, `>=`, `<=`, `>`, `<`.
    /// - **Functions**: `contains(field, value)` checks whether a list field
    ///   contains `value` (or a tag prefix like `#book` matching
    ///   `#book/fiction`) or whether a string field contains `value` as a
    ///   substring.
    /// - **Boolean Logic**: `AND` / `and` / `&&`, `OR` / `or` / `||`, `NOT` /
    ///   `not` / `!`.
    /// - **Parentheses**: `( ... )` overrides standard operator precedence.
    /// - **Literals**: Double-quoted strings (with escape support `\"`),
    ///   numbers, `true`/`false`, or `null`/`Null`.
    /// - **Text Normalization**: `==` and `!=` treat `String`, `Date`, and
    ///   `Duration` values as textually comparable (e.g. `"2026-07-29"` matches
    ///   a `Date` field with equal text).
    /// - **Type Mismatches**: Other cross-kind comparisons (e.g. comparing a
    ///   number to a string) never match under any operator except `!=`.
    /// - **Null Values**: Records missing the field (`Null`) never match `==`
    ///   or ordering operators, but do match `!=`.
    ///
    /// # Errors
    ///
    /// - [`QueryError::UnparsableFilterExpression`] if `expr` cannot be parsed
    /// - [`QueryError::UnknownFieldPath`] if its field path is malformed
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
    /// - **Null Values**: Records missing `path` ([`FieldValue::Null`]) sort as
    ///   minimum values, so they lead ascending and trail descending.
    /// - **Stability**: The sort is stable — equal or incomparable records
    ///   preserve their original relative order.
    ///
    /// # Errors
    ///
    /// Returns [`QueryError::UnknownFieldPath`] if `path` is malformed.
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
    /// Returns [`QueryError::NegativeLimit`] if `n` is negative or does not
    /// fit in a [`usize`] on this platform.
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
    /// Returns [`QueryError::UnknownFieldPath`] if `path` is malformed.
    #[inline]
    pub(crate) fn group_by(self, path: &str) -> Result<Self, QueryError> {
        self.sort_by_field(path, false)
    }

    /// Explodes each record's `path` field into one row per list element.
    ///
    /// Behavioral details:
    /// - **Target Fields**: Applies to fields resolving to [`FieldValue::List`]
    ///   (frontmatter lists, inline list fields, or `tags`).
    /// - **Non-List Fields**: Records with scalar values pass through
    ///   unchanged.
    /// - **Empty Lists**: Records with empty list values contribute no rows to
    ///   the outcome.
    /// - **Row Resolution**: On exploded rows, `path` resolves to that row's
    ///   single element, while all other fields resolve from the original
    ///   record.
    ///
    /// # Errors
    ///
    /// Returns [`QueryError::UnknownFieldPath`] if `path` is malformed.
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

    /// Sorts records by the resolved value of `path`.
    ///
    /// Shared implementation for [`Self::sort`] and [`Self::group_by`]: a
    /// stable sort by `path`'s resolved value, treating [`FieldValue::Null`]
    /// as the minimum value.
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

/// A query field path, resolved once per [`QueryOutcome`] transformation
/// and then applied to every [`IndexRecord`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum FieldPath {
    /// A `file.<field>` accessor.
    File(FileField),
    /// A frontmatter or inline field, looked up by key.
    Metadata(String),
    /// The Note's markdown tags, as a [`FieldValue::List`] of tag strings.
    Tags,
}

impl FieldPath {
    /// Parses a query field path string into a [`FieldPath`].
    ///
    /// Resolves `file.<field>` accessors, `tags`, or frontmatter/inline
    /// field keys.
    ///
    /// # Errors
    ///
    /// Returns [`QueryError::UnknownFieldPath`] if `path` is empty, uses an
    /// unknown `file.*` accessor, or has unexpected `.` structure.
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
        if path.is_empty() || path == "file" || path.contains('.') {
            return Err(invalid());
        }
        if path == "tags" {
            return Ok(Self::Tags);
        }
        Ok(Self::Metadata(path.to_owned()))
    }
}

/// One page-level query result: a [`FileRecord`] paired with its [`Note`].
///
/// Exposes both `file.*` fields and Note Metadata (frontmatter, inline
/// fields, tags) through one value for Template and CLI callers.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct IndexRecord {
    file: FileRecord,
    note: Note,
    /// Overrides field resolution for exploded rows produced by
    /// [`QueryOutcome::flatten`].
    flattened: Vec<(FieldPath, FieldValue)>,
}

impl IndexRecord {
    /// Pairs `file` with its parsed `note`.
    pub(super) fn new(file: FileRecord, note: Note) -> Self {
        Self {
            file,
            note,
            flattened: Vec::new(),
        }
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
    /// Resolves `file.*` accessors, frontmatter fields, inline fields, and
    /// `tags`. Frontmatter fields take precedence over an inline field with the
    /// same key (see [`Note::fields`]). A well-formed path this record has no
    /// value for (e.g. a frontmatter key it does not define) resolves to
    /// [`FieldValue::Null`], not an error.
    ///
    /// # Errors
    ///
    /// Returns [`QueryError::UnknownFieldPath`] if `path` is malformed; see
    /// [`FieldPath::parse`].
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
                .find(|field| field.key() == key)
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

/// Selects which markdown Notes a page-level query includes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Source {
    /// Every indexed markdown Note.
    All,
    /// Notes tagged with a markdown tag, or a sub-tag nested under it (e.g.
    /// `#book` or `#projects`, which also matches `#projects/active`).
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
