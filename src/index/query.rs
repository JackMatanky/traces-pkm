//! Page-level query source selection, field resolution, and
//! filtering/ordering transformations over indexed markdown Notes.
//!
//! [`Source`] selects which Notes a page-level query includes. [`IndexRecord`]
//! pairs a [`FileRecord`] with its [`Note`] and resolves `file.*` fields and
//! Note Metadata (frontmatter, inline fields, tags) by path through
//! [`IndexRecord::field`]. [`QueryOutcome`] is the iterable collection
//! [`super::FileIndex::query`] returns; its [`QueryOutcome::filter`],
//! [`QueryOutcome::sort`], [`QueryOutcome::limit`], [`QueryOutcome::group_by`],
//! and [`QueryOutcome::flatten`] methods each consume a `QueryOutcome` and
//! return a new, transformed one for method chaining.

use std::{cmp::Ordering, path::PathBuf};

use thiserror::Error;

use super::file::FileRecord;
use crate::note::{FieldValue, Note};

/// Selects which markdown Notes a page-level query includes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Source {
    /// Every indexed markdown Note.
    All,
    /// Notes tagged with a markdown tag, or a sub-tag nested under it, e.g.
    /// `#book` or `#projects` (which also matches `#projects/active`).
    Tag(String),
    /// Notes whose [`FileRecord::folder`] is the requested project-relative
    /// folder, or a folder nested under it.
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

/// Errors returned by [`IndexRecord`] field resolution and [`QueryOutcome`]
/// transformation methods.
///
/// These report malformed *inputs* — an unparsable field path or filter
/// expression. A well-formed field path that a given [`IndexRecord`] simply
/// does not have a value for resolves to [`FieldValue::Null`] instead of
/// erroring.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub(crate) enum QueryError {
    /// A field path was empty, used an unknown `file.*` accessor, or had
    /// unexpected `.` structure.
    #[error(
        "invalid field path {path:?}; expected `file.<field>` (path, name, \
         folder, size, ctime, cdate, mtime, mdate) or a single frontmatter, \
         inline field, or `tags` name"
    )]
    UnknownFieldPath {
        /// The unparsable field path.
        path: String,
    },
    /// A `.filter()`/`.where()` expression did not match
    /// `<field> <op> <value>`.
    #[error(
        "invalid filter expression {expr:?}; expected `<field> <op> <value>` \
         with op one of ==, !=, >=, <=, >, < and value a quoted string, \
         number, or boolean"
    )]
    UnparsableFilterExpression {
        /// The unparsable filter expression.
        expr: String,
    },
    /// A `.limit()` count was negative or otherwise did not fit in a
    /// [`usize`] on this platform.
    #[error("invalid limit {n}; expected a non-negative row count")]
    NegativeLimit {
        /// The rejected limit value.
        n: i64,
    },
}

/// A query field path, resolved once per [`QueryOutcome`] transformation and
/// then applied to every [`IndexRecord`].
#[derive(Clone, Debug, Eq, PartialEq)]
enum FieldPath {
    /// A `file.<field>` accessor.
    File(FileField),
    /// A frontmatter or inline field, looked up by key.
    Metadata(String),
    /// The Note's markdown tags, as a [`FieldValue::List`] of tag strings.
    Tags,
}

/// General `file.*` metadata accessors available to query field paths.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FileField {
    /// [`FileRecord::path`].
    Path,
    /// [`FileRecord::name`].
    Name,
    /// [`FileRecord::folder`].
    Folder,
    /// [`FileRecord::size`].
    Size,
    /// [`FileRecord::created_at_or_modified`], as an RFC 3339 datetime.
    CreatedDateTime,
    /// [`FileRecord::created_at_or_modified`], as a bare date.
    CreatedDate,
    /// [`FileRecord::modified_at`], as an RFC 3339 datetime.
    ModifiedDateTime,
    /// [`FileRecord::modified_at`], as a bare date.
    ModifiedDate,
}

impl FieldPath {
    /// Parses a query field path.
    ///
    /// `file.<field>` resolves one of the known [`FileField`] accessors.
    /// `tags` resolves the Note's markdown tags. Any other single-segment
    /// name resolves a frontmatter or inline field by key.
    ///
    /// # Errors
    ///
    /// Returns [`QueryError::UnknownFieldPath`] if `path` is empty, uses an
    /// unknown `file.*` accessor, or has unexpected `.` structure.
    fn parse(path: &str) -> Result<Self, QueryError> {
        let path = path.trim();
        let invalid = || QueryError::UnknownFieldPath {
            path: path.to_owned(),
        };
        if let Some(field) = path.strip_prefix("file.") {
            return if field.is_empty() || field.contains('.') {
                Err(invalid())
            } else {
                FileField::parse(field).map(Self::File)
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

impl FileField {
    /// Parses a `file.<field>` accessor name (the part after `"file."`).
    ///
    /// # Errors
    ///
    /// Returns [`QueryError::UnknownFieldPath`] if `name` is not a known
    /// accessor.
    fn parse(name: &str) -> Result<Self, QueryError> {
        match name {
            "path" => Ok(Self::Path),
            "name" => Ok(Self::Name),
            "folder" => Ok(Self::Folder),
            "size" => Ok(Self::Size),
            "created_at" | "ctime" => Ok(Self::CreatedDateTime),
            "cdate" => Ok(Self::CreatedDate),
            "modified_at" | "mtime" => Ok(Self::ModifiedDateTime),
            "mdate" => Ok(Self::ModifiedDate),
            _ => Err(QueryError::UnknownFieldPath {
                path: format!("file.{name}"),
            }),
        }
    }

    /// Resolves this accessor against `file`.
    fn resolve(self, file: &FileRecord) -> FieldValue {
        match self {
            Self::Path => {
                FieldValue::String(file.path().to_string_lossy().into_owned())
            }
            Self::Name => FieldValue::String(file.name().as_str().to_owned()),
            Self::Folder => {
                FieldValue::String(file.folder().to_string_lossy().into_owned())
            }
            #[expect(
                clippy::as_conversions,
                clippy::cast_precision_loss,
                reason = "file sizes stay well under 2^53 bytes for PKM-scale \
                          projects, so f64 keeps exact byte counts"
            )]
            Self::Size => FieldValue::Number(file.size() as f64),
            Self::CreatedDateTime => {
                FieldValue::Date(file.created_at_or_modified().to_rfc3339())
            }
            Self::CreatedDate => {
                FieldValue::Date(file.created_at_or_modified().to_date_string())
            }
            Self::ModifiedDateTime => {
                FieldValue::Date(file.modified_at().to_rfc3339())
            }
            Self::ModifiedDate => {
                FieldValue::Date(file.modified_at().to_date_string())
            }
        }
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
    /// Set by [`QueryOutcome::flatten`]: overrides resolution of one field
    /// path with a single element taken from that field's original list
    /// value.
    flattened: Option<(FieldPath, FieldValue)>,
}

impl IndexRecord {
    /// Pairs `file` with its parsed `note`.
    pub(super) fn new(file: FileRecord, note: Note) -> Self {
        Self {
            file,
            note,
            flattened: None,
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

    /// Resolves `path` against this record's `file.*` metadata or Note
    /// Metadata (frontmatter fields, inline fields, and `tags`).
    ///
    /// Frontmatter fields take precedence over an inline field with the same
    /// key (see [`Note::fields`]). A well-formed path this record has no
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

    /// Resolves an already-[parsed](FieldPath::parse) `path`, applying a
    /// [`Self::flattened`] override first if one matches `path`.
    fn resolve(&self, path: &FieldPath) -> FieldValue {
        if let Some((flattened_path, value)) = &self.flattened
            && flattened_path == path
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

    /// Returns this record with `path` overridden to resolve to `value`, for
    /// one exploded row produced by [`QueryOutcome::flatten`].
    #[must_use]
    fn with_flattened(mut self, path: FieldPath, value: FieldValue) -> Self {
        self.flattened = Some((path, value));
        self
    }
}

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

    /// Keeps only records matching `expr` (`"<field> <op> <value>"`, e.g.
    /// `"rating > 7"` or `"status == \"done\""`).
    ///
    /// Supported operators: `==`, `!=`, `>=`, `<=`, `>`, `<`. `value` is a
    /// double-quoted string (no escape support), a number, or
    /// `true`/`false`. `==`/`!=` treat `String`, `Date`, and `Duration`
    /// field values as the same kind, comparing their text (so a quoted
    /// literal matches a `file.mtime`-style `Date` field with equal text);
    /// every other kind mismatch (e.g. a numeric field against a string
    /// literal) never matches under any operator except `!=`. A record
    /// missing the field entirely (`Null`) behaves the same way: it never
    /// matches `==` or an ordering operator, but does match `!=`.
    ///
    /// # Errors
    ///
    /// Returns [`QueryError::UnparsableFilterExpression`] if `expr` cannot be
    /// parsed, or [`QueryError::UnknownFieldPath`] if its field path is
    /// malformed.
    pub(crate) fn filter(self, expr: &str) -> Result<Self, QueryError> {
        let expr = FilterExpr::parse(expr)?;
        let records = self
            .records
            .into_iter()
            .filter(|record| expr.matches(record))
            .collect();
        Ok(Self::new(records))
    }

    /// Orders records by `path`, ascending unless `descending` is set.
    ///
    /// Matches Dataview's own sort semantics: records missing `path`
    /// (resolving to [`FieldValue::Null`]) sort as the minimum value, so
    /// they lead ascending and trail descending, the same as any other
    /// value. The sort is stable: records with equal or incomparable
    /// values keep their relative order.
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

    /// Orders records by `path`, ascending, clustering equal values so
    /// consumers (a `{% for %}` loop or terminal renderer) can detect group
    /// boundaries by comparing each record's resolved `path` value to the
    /// previous one.
    ///
    /// # Errors
    ///
    /// Returns [`QueryError::UnknownFieldPath`] if `path` is malformed.
    #[inline]
    pub(crate) fn group_by(self, path: &str) -> Result<Self, QueryError> {
        self.sort_by_field(path, false)
    }

    /// Explodes each record's `path` field into one row per list element,
    /// for fields that resolve to a [`FieldValue::List`] (frontmatter/inline
    /// multi-value fields, or `tags`).
    ///
    /// A record whose `path` value is an empty list contributes no rows. A
    /// record whose `path` value is not a list passes through unchanged. On
    /// every exploded row, `path` itself resolves to that row's single list
    /// element; every other field still resolves from the original record.
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

    /// Shared implementation for [`Self::sort`] and [`Self::group_by`]: a
    /// stable sort by `path`'s resolved value, with [`FieldValue::Null`]
    /// always last.
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

/// A comparison operator parsed from a [`QueryOutcome::filter`] expression.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CompareOp {
    /// `==`
    Eq,
    /// `!=`
    Ne,
    /// `<`
    Lt,
    /// `<=`
    Le,
    /// `>`
    Gt,
    /// `>=`
    Ge,
}

impl CompareOp {
    /// Strips a leading comparison operator from `s`, returning it with the
    /// remainder of `s` after it. Multi-character operators (`==`, `!=`,
    /// `>=`, `<=`) are checked before their `>`/`<` prefixes.
    fn strip_prefix(s: &str) -> Option<(Self, &str)> {
        const OPERATORS: [(&str, CompareOp); 6] = [
            ("==", CompareOp::Eq),
            ("!=", CompareOp::Ne),
            (">=", CompareOp::Ge),
            ("<=", CompareOp::Le),
            (">", CompareOp::Gt),
            ("<", CompareOp::Lt),
        ];
        OPERATORS.into_iter().find_map(|(token, op)| {
            s.strip_prefix(token).map(|rest| (op, rest))
        })
    }

    /// Whether `field` matches `literal` under this operator.
    ///
    /// `==`/`!=` are total: every value kind, including
    /// [`FieldValue::Null`], compares. They use [`fields_equal`], not raw
    /// [`FieldValue`] equality, so a `String`, `Date`, or `Duration` field
    /// matches a same-text literal of any of those three kinds — the same
    /// cross-kind text normalization [`compare_field_values`] applies to
    /// the ordering operators below.
    fn is_satisfied_by(self, field: &FieldValue, literal: &FieldValue) -> bool {
        match self {
            Self::Eq => fields_equal(field, literal),
            Self::Ne => !fields_equal(field, literal),
            Self::Lt => {
                compare_field_values(field, literal) == Some(Ordering::Less)
            }
            Self::Gt => {
                compare_field_values(field, literal) == Some(Ordering::Greater)
            }
            Self::Le => matches!(
                compare_field_values(field, literal),
                Some(Ordering::Less | Ordering::Equal)
            ),
            Self::Ge => matches!(
                compare_field_values(field, literal),
                Some(Ordering::Greater | Ordering::Equal)
            ),
        }
    }
}

/// A parsed `.filter()` expression: a field path, a comparison operator,
/// and the literal value to compare it against.
///
/// Names the shape [`QueryOutcome::filter`] parses once per call instead of
/// threading an anonymous `(FieldPath, CompareOp, FieldValue)` triple
/// through it.
#[derive(Clone, Debug, PartialEq)]
struct FilterExpr {
    field: FieldPath,
    op: CompareOp,
    value: FieldValue,
}

impl FilterExpr {
    /// Parses `"<field> <op> <value>"` — see [`QueryOutcome::filter`] for
    /// the full grammar.
    ///
    /// # Errors
    ///
    /// Returns [`QueryError::UnparsableFilterExpression`] if `expr` does not
    /// match `<field> <op> <value>`, or [`QueryError::UnknownFieldPath`] if
    /// its field path is malformed.
    fn parse(expr: &str) -> Result<Self, QueryError> {
        let invalid = || QueryError::UnparsableFilterExpression {
            expr: expr.to_owned(),
        };
        let trimmed = expr.trim();
        let op_start = trimmed
            .find(|c: char| {
                !(c.is_alphanumeric() || c == '_' || c == '.' || c == '-')
            })
            .ok_or_else(invalid)?;
        let (field, rest) = trimmed.split_at(op_start);
        let field = field.trim();
        if field.is_empty() {
            return Err(invalid());
        }
        let (op, rest) =
            CompareOp::strip_prefix(rest.trim_start()).ok_or_else(invalid)?;
        let value = parse_filter_literal(rest.trim()).ok_or_else(invalid)?;
        Ok(Self {
            field: FieldPath::parse(field)?,
            op,
            value,
        })
    }

    /// Whether `record`'s resolved field satisfies this expression.
    fn matches(&self, record: &IndexRecord) -> bool {
        self.op.is_satisfied_by(&record.resolve(&self.field), &self.value)
    }
}

/// Parses a filter expression's literal value: a double-quoted string (no
/// escape support — a literal `"` cannot appear in the value), a number, or
/// `true`/`false`.
fn parse_filter_literal(s: &str) -> Option<FieldValue> {
    if let Some(inner) = s.strip_prefix('"').and_then(|s| s.strip_suffix('"')) {
        return Some(FieldValue::String(inner.to_owned()));
    }
    match s {
        "true" => Some(FieldValue::Bool(true)),
        "false" => Some(FieldValue::Bool(false)),
        _ => s.parse::<f64>().ok().map(FieldValue::Number),
    }
}

/// Orders two resolved field values when they are the same comparable kind:
/// numbers by magnitude; strings, dates, and durations lexicographically (via
/// [`FieldValue::as_str`], so a string literal compares against a `Date` or
/// `Duration` field the same way); booleans with `false < true`. Any other
/// pairing, including differing kinds, has no ordering.
fn compare_field_values(a: &FieldValue, b: &FieldValue) -> Option<Ordering> {
    match (a, b) {
        (FieldValue::Number(x), FieldValue::Number(y)) => x.partial_cmp(y),
        (FieldValue::Bool(x), FieldValue::Bool(y)) => Some(x.cmp(y)),
        _ => match (a.as_str(), b.as_str()) {
            (Some(x), Some(y)) => Some(x.cmp(y)),
            _ => None,
        },
    }
}

/// Whether `a` and `b` represent the same value for `.filter()`'s
/// `==`/`!=`. Falls back to [`compare_field_values`] returning
/// [`Ordering::Equal`] when [`FieldValue`]'s own structural equality says
/// no — the same cross-kind text normalization that lets a `String`
/// literal match a `Date`/`Duration` field for the ordering operators.
fn fields_equal(a: &FieldValue, b: &FieldValue) -> bool {
    a == b || compare_field_values(a, b) == Some(Ordering::Equal)
}

/// Total order for [`QueryOutcome::sort`]/[`QueryOutcome::group_by`],
/// matching Dataview's `compareValue`: [`FieldValue::Null`] (a record
/// missing the field) sorts as the minimum value, and `descending` reverses
/// the whole comparator uniformly — so `Null` sorts first ascending, last
/// descending, exactly like every other value. Non-`Null` records order by
/// [`compare_field_values`], falling back to [`Ordering::Equal`] (keeping
/// stable relative order) for incomparable kinds.
fn sort_key_cmp(a: &FieldValue, b: &FieldValue, descending: bool) -> Ordering {
    let ord = match (a, b) {
        (FieldValue::Null, FieldValue::Null) => Ordering::Equal,
        (FieldValue::Null, _) => Ordering::Less,
        (_, FieldValue::Null) => Ordering::Greater,
        _ => compare_field_values(a, b).unwrap_or(Ordering::Equal),
    };
    if descending {
        ord.reverse()
    } else {
        ord
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

    mod field {
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
                Ok(FieldValue::Date(file.modified_at().to_rfc3339()))
            );
            assert_eq!(
                record.field("file.mdate"),
                Ok(FieldValue::Date(file.modified_at().to_date_string()))
            );
            assert_eq!(
                record.field("file.ctime"),
                Ok(FieldValue::Date(
                    file.created_at_or_modified().to_rfc3339()
                ))
            );
            assert_eq!(
                record.field("file.cdate"),
                Ok(FieldValue::Date(
                    file.created_at_or_modified().to_date_string()
                ))
            );
            // `created_at`/`modified_at` are aliases for `ctime`/`mtime`.
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

    mod filter {
        use pretty_assertions::assert_eq;
        use rstest::rstest;

        use super::*;

        fn rated_outcome(temp: &Path) -> QueryOutcome {
            outcome_for_files(temp, &[
                ("low.md", "---\nrating: 3\nstatus: draft\n---"),
                ("high.md", "---\nrating: 7\nstatus: done\n---"),
                ("unrated.md", "---\nstatus: done\n---"),
            ])
        }

        fn names(outcome: &QueryOutcome) -> Vec<String> {
            outcome
                .iter()
                .map(|record| record.file().name().as_str().to_owned())
                .collect()
        }

        #[rstest]
        #[case::greater_than("rating > 5", &["high"])]
        #[case::greater_or_equal("rating >= 7", &["high"])]
        #[case::less_than("rating < 5", &["low"])]
        #[case::less_or_equal("rating <= 3", &["low"])]
        #[case::numeric_equal("rating == 7", &["high"])]
        #[case::string_equal("status == \"done\"", &["high", "unrated"])]
        #[case::string_not_equal("status != \"done\"", &["low"])]
        fn keeps_only_matching_records(
            #[case] expr: &str,
            #[case] expected: &[&str],
        ) {
            let temp = tempfile::tempdir().expect("create temp dir");
            let outcome = rated_outcome(temp.path());

            let filtered = outcome.filter(expr).expect("valid filter");

            assert_eq!(names(&filtered), expected);
        }

        #[test]
        fn missing_field_never_matches_equality_or_ordering() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let outcome = rated_outcome(temp.path());

            let filtered = outcome.filter("rating > 0").expect("valid filter");

            // Alphabetical build order: "high" sorts before "low".
            assert_eq!(names(&filtered), ["high", "low"]);
        }

        #[test]
        fn missing_field_matches_not_equal() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let outcome = rated_outcome(temp.path());

            let filtered = outcome.filter("rating != 7").expect("valid filter");

            assert_eq!(names(&filtered), ["low", "unrated"]);
        }

        #[test]
        fn type_mismatch_never_matches_ordering_or_equality() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let outcome = rated_outcome(temp.path());

            let filtered = outcome.filter("status > 5").expect("valid filter");

            assert!(filtered.is_empty());
        }

        #[rstest]
        #[case::no_operator("rating")]
        #[case::empty_field(" > 5")]
        #[case::empty_value("rating >")]
        #[case::unquoted_string("status == done")]
        fn rejects_malformed_expressions(#[case] expr: &str) {
            let temp = tempfile::tempdir().expect("create temp dir");
            let outcome = rated_outcome(temp.path());

            assert_eq!(
                outcome.filter(expr),
                Err(QueryError::UnparsableFilterExpression {
                    expr: expr.to_owned()
                })
            );
        }

        #[test]
        fn rejects_malformed_field_path_in_expression() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let outcome = rated_outcome(temp.path());

            assert_eq!(
                outcome.filter("file.bogus == 1"),
                Err(QueryError::UnknownFieldPath {
                    path: "file.bogus".to_owned()
                })
            );
        }

        #[test]
        fn chains_across_multiple_filter_calls() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let outcome = rated_outcome(temp.path());

            let filtered = outcome
                .filter("status == \"done\"")
                .expect("valid filter")
                .filter("rating >= 7")
                .expect("valid filter");

            assert_eq!(names(&filtered), ["high"]);
        }

        #[test]
        fn equal_matches_a_date_field_against_a_string_literal() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let outcome = outcome_for(temp.path(), "---\ndue: 2026-01-01\n---");

            let filtered =
                outcome.filter("due == \"2026-01-01\"").expect("valid filter");

            assert_eq!(filtered.len(), 1);
        }
    }

    mod sort {
        use pretty_assertions::assert_eq;

        use super::*;

        fn names(outcome: &QueryOutcome) -> Vec<String> {
            outcome
                .iter()
                .map(|record| record.file().name().as_str().to_owned())
                .collect()
        }

        #[test]
        fn orders_ascending_by_default() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let outcome = outcome_for_files(temp.path(), &[
                ("b.md", "---\nrating: 7\n---"),
                ("a.md", "---\nrating: 3\n---"),
            ]);

            let sorted = outcome.sort("rating", false).expect("valid sort");

            assert_eq!(names(&sorted), ["a", "b"]);
        }

        #[test]
        fn orders_descending_when_requested() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let outcome = outcome_for_files(temp.path(), &[
                ("b.md", "---\nrating: 7\n---"),
                ("a.md", "---\nrating: 3\n---"),
            ]);

            let sorted = outcome.sort("rating", true).expect("valid sort");

            assert_eq!(names(&sorted), ["b", "a"]);
        }

        #[test]
        fn missing_field_sorts_as_the_minimum_value() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let outcome = outcome_for_files(temp.path(), &[
                ("rated.md", "---\nrating: 3\n---"),
                ("unrated.md", "no frontmatter"),
            ]);

            let ascending =
                outcome.clone().sort("rating", false).expect("valid sort");
            let descending = outcome.sort("rating", true).expect("valid sort");

            // Matches Dataview: Null is the minimum value, so it leads
            // ascending and trails descending, like any other value would.
            assert_eq!(names(&ascending), ["unrated", "rated"]);
            assert_eq!(names(&descending), ["rated", "unrated"]);
        }

        #[test]
        fn ties_keep_original_relative_order() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let outcome = outcome_for_files(temp.path(), &[
                ("a.md", "---\nrating: 5\n---"),
                ("b.md", "---\nrating: 5\n---"),
            ]);

            let sorted = outcome.sort("rating", false).expect("valid sort");

            assert_eq!(names(&sorted), ["a", "b"]);
        }

        #[test]
        fn rejects_malformed_field_path() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let outcome = outcome_for(temp.path(), "body");

            assert_eq!(
                outcome.sort("file.bogus", false),
                Err(QueryError::UnknownFieldPath {
                    path: "file.bogus".to_owned()
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
