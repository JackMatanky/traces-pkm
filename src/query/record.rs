//! Query rows and field resolution for [`super::QueryOutcome`].
//!
//! This module implements [`IndexRecord`], which pairs a [`FileRecord`] with
//! its parsed [`Note`] and resolves field paths for template rendering and CLI
//! output. Each record resolves `file.*`, `task.*`, frontmatter, inline fields,
//! `tags`, and derived inlinks.
//!
//! # Main Types
//!
//! - [`IndexRecord`] is the primary query row, produced by [`super::query`] or
//!   [`super::query_tasks`].
//! - [`TaskInfo`] carries per-task fields layered onto an `IndexRecord` by
//!   [`crate::index::FileIndex::query_tasks`].
//!
//! # Examples
//!
//! ```ignore
//! use std::path::Path;
//!
//! use traces_pkm::{index::FileRecord, note::Note, query::IndexRecord};
//!
//! let note = Note::default();
//! let record = IndexRecord::new(file, Some(note));
//!
//! assert_eq!(record.file().path().to_str(), Some("note.md"));
//! ```
//!
//! [`FileRecord`]: crate::index::FileRecord
//! [`Note`]: crate::note::Note

use std::{path::PathBuf, sync::Arc};

use super::{
    QueryError,
    field::{FieldPath, TaskField},
};
use crate::{
    index::FileRecord,
    note::{FieldValue, Note},
};

/// A query row pairing a [`FileRecord`] with parsed [`Note`] metadata.
///
/// Each record resolves `file.*`, `task.*`, frontmatter, inline fields, `tags`,
/// and derived inlinks for template rendering and CLI output.
///
/// The [`Note`] is reference-counted via [`Arc`] to share data efficiently when
/// expanding one note into multiple task-level rows.
///
/// # Examples
///
/// ```ignore
/// use std::path::Path;
///
/// use traces_pkm::{index::FileRecord, note::Note, query::IndexRecord};
///
/// let note = Note::default();
/// let record = IndexRecord::new(file, Some(note));
/// ```
#[derive(Clone, Debug, PartialEq)]
pub struct IndexRecord {
    file: FileRecord,
    /// Reference-counted, not owned outright: exploding one Note into several
    /// rows (see [`crate::index::FileIndex::query_tasks`] and
    /// [`super::QueryOutcome::flatten`]) shares this field across every row
    /// instead of deep-cloning frontmatter, links, tags, and lists per row.
    ///
    /// `None` when the underlying file has no parsed [`Note`] (for example, an
    /// image or PDF referenced by a `file`-typed Schema field).
    note: Option<Arc<Note>>,
    /// Overrides field resolution for exploded rows produced by
    /// [`super::QueryOutcome::flatten`].
    flattened: Vec<(FieldPath, FieldValue)>,
    /// Stores per-task fields set by [`crate::index::FileIndex::query_tasks`],
    /// or `None` for page-level records.
    task: Option<TaskInfo>,
    /// Stores project-relative paths of Notes whose outlinks resolve to this
    /// row's Note, set by [`crate::index::FileIndex::query`] and
    /// [`crate::index::FileIndex::query_tasks`].
    inlinks: Vec<PathBuf>,
}

impl IndexRecord {
    /// Creates a new [`IndexRecord`] pairing `file` with its parsed `note`.
    ///
    /// `note` is `None` for files with no parsed [`Note`] (non-Markdown files
    /// matched by a `file`-typed Schema field).
    pub(super) fn new(file: FileRecord, note: Option<Note>) -> Self {
        Self {
            file,
            note: note.map(Arc::new),
            flattened: Vec::new(),
            task: None,
            inlinks: Vec::new(),
        }
    }

    /// Converts this record into a task-level row.
    ///
    /// Attaches task completion state and text, used by
    /// [`crate::index::FileIndex::query_tasks`] to expand a page-level record
    /// into one row per task item while retaining parent Note metadata for
    /// filtering and display via [`Self::field`].
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

    /// Attaches project-relative paths of Notes whose wikilinks resolve to
    /// this record's Note.
    pub(super) fn with_inlinks(mut self, inlinks: Vec<PathBuf>) -> Self {
        self.inlinks = inlinks;
        self
    }

    /// Returns task completion state if this is a task-level record, or
    /// `None` for page-level records.
    ///
    /// Returns `true` for `- [x]` and `false` for `- [ ]`.
    #[inline]
    #[must_use]
    pub fn task_completed(&self) -> Option<bool> {
        self.task.as_ref().map(|task| task.completed)
    }

    /// Returns the task item's text if this is a task-level record, or
    /// `None` for page-level records.
    #[inline]
    #[must_use]
    pub(crate) fn task_text(&self) -> Option<&str> {
        self.task.as_ref().map(|task| task.text.as_str())
    }

    /// Returns general metadata for the indexed file.
    #[inline]
    #[must_use]
    pub fn file(&self) -> &FileRecord {
        &self.file
    }

    /// Returns parsed [`Note`] metadata for the indexed file, or `None` if the
    /// file has no parsed Note (a non-Markdown file matched by a `file`-typed
    /// Schema field).
    ///
    /// The returned reference shares the underlying [`Arc`] allocation with
    /// any task-level rows derived from the same Note.
    #[inline]
    #[must_use]
    pub(crate) fn note(&self) -> Option<&Note> {
        self.note.as_deref()
    }

    /// Returns project-relative paths of Notes whose wikilinks resolve to
    /// this record's Note, or an empty slice if no Notes link to it.
    #[inline]
    #[must_use]
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "no current caller outside tests; documented deliberate \
                      API in index-query#10's derived-inlinks design \
                      (Templates/CLI select or filter inlinks via \
                      field(\"inlinks\") today; this direct accessor for \
                      display output is not yet wired to a CLI/Template \
                      renderer)"
        )
    )]
    pub(crate) fn inlinks(&self) -> &[PathBuf] {
        &self.inlinks
    }

    /// Resolves a field path string against this record's metadata.
    ///
    /// Resolves `file.*` accessors, `task.*` accessors, frontmatter fields,
    /// inline fields, `tags`, and `inlinks`. Resolution rules include:
    /// - Frontmatter fields take precedence over inline fields sharing the same
    ///   key (see [`Note::fields`]).
    /// - Well-formed paths without values (such as a missing key or a `task.*`
    ///   accessor on a page-level record) resolve to [`FieldValue::Null`].
    ///
    /// # Errors
    ///
    /// - [`QueryError::FieldPath`] if `path` cannot be parsed as a valid field
    ///   path.
    #[inline]
    pub(crate) fn field(&self, path: &str) -> Result<FieldValue, QueryError> {
        Ok(self.resolve(&FieldPath::parse(path)?))
    }

    /// Resolves a pre-parsed field path against this record, applying
    /// overrides.
    pub(super) fn resolve(&self, path: &FieldPath) -> FieldValue {
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
                    .iter()
                    .flat_map(|note| note.tags())
                    .map(|tag| FieldValue::String(tag.as_str().to_owned()))
                    .collect(),
            ),
            FieldPath::Inlinks => FieldValue::List(
                self.inlinks
                    .iter()
                    .map(|linking_note| {
                        FieldValue::String(
                            linking_note.to_string_lossy().into_owned(),
                        )
                    })
                    .collect(),
            ),
            FieldPath::Metadata(key) => self
                .note
                .as_deref()
                .and_then(|note| {
                    note.fields()
                        .find(|field| field.key().is_match(key.as_str()))
                })
                .map_or(FieldValue::Null, |field| field.value().clone()),
        }
    }

    /// Returns a copy of this record with `path` overridden to `value`.
    ///
    /// Used by [`super::QueryOutcome::flatten`] to set the resolved value for
    /// exploded list rows. If `path` already has an override, the value is
    /// updated in place.
    pub(super) fn with_flattened(
        mut self,
        path: FieldPath,
        value: FieldValue,
    ) -> Self {
        if let Some(entry) = self.flattened.iter_mut().find(|(p, _)| p == &path)
        {
            entry.1 = value;
        } else {
            self.flattened.push((path, value));
        }
        self
    }
}

/// Per-task fields layered onto an [`IndexRecord`] by
/// [`crate::index::FileIndex::query_tasks`].
///
/// Task-level rows retain parent [`Note`] file and metadata fields for
/// filtering and display while attaching task completion and text. This is
/// distinct from [`IndexRecord::flattened`], which overrides existing field
/// paths rather than adding new ones.
#[derive(Clone, Debug, PartialEq)]
struct TaskInfo {
    completed: bool,
    text: String,
}
