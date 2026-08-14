//! Query rows and field resolution.

use std::{path::PathBuf, sync::Arc};

use super::{
    QueryError,
    field::{FieldPath, TaskField},
};
use crate::{
    index::FileRecord,
    note::{FieldValue, Note},
};

/// Represents a query row pairing a [`FileRecord`] with parsed [`Note`]
/// metadata.
///
/// Task-level rows also carry fields for a single task item. Each row resolves
/// `file.*`, `task.*`, frontmatter, inline fields, tags, and derived inlinks
/// for template rendering and CLI output.
#[derive(Clone, Debug, PartialEq)]
pub struct IndexRecord {
    file: FileRecord,
    /// Reference-counted, not owned outright: exploding one Note into
    /// several rows (see [`crate::index::FileIndex::query_tasks`] and
    /// [`super::QueryOutcome::flatten`]) shares this field across every row
    /// instead of deep-cloning frontmatter, links, tags, and lists per
    /// row.
    note: Arc<Note>,
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
    pub(super) fn new(file: FileRecord, note: Note) -> Self {
        Self {
            file,
            note: Arc::new(note),
            flattened: Vec::new(),
            task: None,
            inlinks: Vec::new(),
        }
    }

    /// Converts this record into a task-level row with specified completion
    /// state and text.
    ///
    /// Used by [`crate::index::FileIndex::query_tasks`] to turn a page-level
    /// record into one row per task item while retaining parent Note
    /// metadata for filtering and display via [`Self::field`].
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

    /// Attaches project-relative paths of Notes whose outlinks resolve to this
    /// record's Note.
    pub(super) fn with_inlinks(mut self, inlinks: Vec<PathBuf>) -> Self {
        self.inlinks = inlinks;
        self
    }

    /// Returns task completion state (`true` for `- [x]`, `false` for
    /// `- [ ]`) if this is a task-level record, or `None` for page-level
    /// records.
    #[inline]
    #[must_use]
    pub fn task_completed(&self) -> Option<bool> {
        self.task.as_ref().map(|task| task.completed)
    }

    /// Returns the task item's text if this is a task-level record, or `None`
    /// for page-level records.
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

    /// Returns parsed [`Note`] metadata for the indexed file.
    #[inline]
    #[must_use]
    pub(crate) fn note(&self) -> &Note {
        self.note.as_ref()
    }

    /// Returns project-relative paths of Notes whose outlinks resolve to this
    /// record's Note, or an empty slice if no Notes link to it.
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

    /// Resolves a parsed [`FieldPath`], applying any [`Self::flattened`]
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
                    .tags()
                    .iter()
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
                .fields()
                .find(|field| field.key().is_match(key.as_str()))
                .map_or(FieldValue::Null, |field| field.value().clone()),
        }
    }

    /// Returns a copy of this record with `path` overridden to `value` for
    /// exploded [`super::QueryOutcome::flatten`] rows.
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

/// Represents per-task fields layered onto an [`IndexRecord`] by
/// [`crate::index::FileIndex::query_tasks`].
///
/// Task-level rows retain parent [`Note`] file and metadata fields for
/// filtering and display while attaching task completion and text attributes.
/// This is distinct from [`IndexRecord::flattened`], which overrides existing
/// field paths rather than adding new ones.
#[derive(Clone, Debug, PartialEq)]
struct TaskInfo {
    completed: bool,
    text: String,
}
