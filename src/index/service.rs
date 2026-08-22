//! Index lifecycle service.

use std::path::PathBuf;

use super::{FileIndex, FileIndexError, builder, store::IndexStore};

/// Drives the [`FileIndex`] lifecycle for one project root: build, persist,
/// load, and refresh.
///
/// Mirrors `ConfigService`/`SchemaService`: a fixed-configuration service
/// (here, the project root) with methods that read or write against it,
/// rather than a bare `root: &Path` parameter repeated at every call site.
///
/// # Examples
///
/// ```ignore
/// # use traces_pkm::index::IndexerService;
/// let indexer = IndexerService::new("/path/to/project");
/// let index = indexer.build().expect("build index");
/// indexer.persist(&index).expect("persist index");
/// ```
#[derive(Clone, Debug)]
pub struct IndexerService {
    root: PathBuf,
}

impl IndexerService {
    /// Creates a service scoped to `root`.
    #[inline]
    #[must_use]
    pub fn new<P: Into<PathBuf>>(root: P) -> Self {
        Self {
            root: root.into(),
        }
    }

    /// Scans this service's root and builds a [`FileIndex`] in memory.
    ///
    /// Markdown files are parsed into [`crate::note::Note`] records. The index
    /// is not persisted until [`Self::persist`] is called.
    ///
    /// # Errors
    ///
    /// - [`FileIndexError::Io`] if a directory cannot be read, a file's
    ///   metadata cannot be inspected, or a markdown file cannot be read.
    #[inline]
    pub fn build(&self) -> Result<FileIndex, FileIndexError> {
        Ok(builder::IndexBuilder::from_scan(&self.root)?.build(&self.root)?)
    }

    /// Refreshes the persisted index for this service's root against current
    /// filesystem state.
    ///
    /// Re-scans the root and compares each current file's `(created_at,
    /// modified_at, size)` tuple against the previously persisted
    /// [`crate::file::FileRecord`]:
    ///
    /// - Unchanged markdown Notes reuse their parsed [`crate::note::Note`].
    /// - Added or changed markdown Notes are parsed from disk.
    /// - Deleted files disappear because they are absent from the fresh scan.
    ///
    /// Returns the fresh [`FileIndex`] without persisting. Call
    /// [`Self::persist`] to write the result to disk.
    ///
    /// Derived inlinks are recomputed in full whenever a Note's content or
    /// metadata changed since the last persist. A full recompute (not a
    /// per-note patch) is required because link target resolution considers
    /// every indexed Note: an unedited Note's *resolved* target can change
    /// when an unrelated Note is added or removed. For example, a wikilink
    /// that was ambiguous becomes resolvable once one of the ambiguous
    /// candidates is deleted.
    ///
    /// # Errors
    ///
    /// - [`FileIndexError::Io`] if a directory cannot be read, a file's
    ///   metadata cannot be inspected, or a markdown file cannot be read.
    /// - [`FileIndexError::Store`] or [`FileIndexError::Deserialize`] if the
    ///   previous index cannot be loaded.
    #[inline]
    pub fn refresh(&self) -> Result<FileIndex, FileIndexError> {
        let previous = self.load()?;
        Ok(builder::IndexBuilder::from_scan(&self.root)?
            .reuse_unchanged(previous)
            .build(&self.root)?)
    }

    /// Persists `index` to this service's root, replacing any existing index
    /// contents.
    ///
    /// [`crate::file::FileRecord`], [`crate::note::Note`], and derived inlink
    /// records are all written atomically.
    ///
    /// # Errors
    ///
    /// - [`FileIndexError::Io`] if the database's parent directory cannot be
    ///   created.
    /// - [`FileIndexError::Store`] if the database transaction fails.
    /// - [`FileIndexError::Serialize`] if a record cannot be encoded.
    #[inline]
    pub fn persist(&self, index: &FileIndex) -> Result<(), FileIndexError> {
        IndexStore::open(&self.root)?.replace_all(
            &index.records,
            &index.notes,
            &index.inlinks,
        )
    }

    /// Loads the index previously persisted for this service's root.
    ///
    /// Returns an empty [`FileIndex`] if no index has been persisted yet.
    ///
    /// # Errors
    ///
    /// - [`FileIndexError::Store`] if the database cannot be read.
    /// - [`FileIndexError::Deserialize`] if stored bytes are not a valid
    ///   record.
    #[inline]
    pub fn load(&self) -> Result<FileIndex, FileIndexError> {
        let (records, notes, inlinks) =
            IndexStore::open(&self.root)?.load_all()?;
        Ok(FileIndex {
            records,
            notes,
            inlinks,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, path::Path};

    use super::*;
    use crate::{
        file::FileRecord,
        note::Note,
        query::{QueryRecordSet, QueryRequest, QueryService, QuerySource},
    };

    /// Runs a page-level query via [`QueryService`].
    fn query_all(index: FileIndex, source: &QuerySource) -> QueryRecordSet {
        QueryService::new("class")
            .execute(&index, QueryRequest::pages(source.clone()))
    }

    mod build {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn extracts_note_metadata_only_for_markdown_files() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::create_dir_all(temp.path().join("notes")).expect("mkdir notes");
            fs::write(temp.path().join("notes/todo.md"), "- [ ] task 1")
                .expect("write note");
            fs::write(temp.path().join("readme.txt"), "text content")
                .expect("write txt");

            let index =
                IndexerService::new(temp.path()).build().expect("build index");

            assert_eq!(index.records().len(), 2);
            assert_eq!(index.notes().len(), 1);
            assert_eq!(
                index.notes().first().map(Note::path),
                Some(Path::new("notes/todo.md"))
            );
        }

        #[test]
        fn includes_frontmatter_fields_in_the_indexed_note() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("todo.md"), "---\ntitle: Todo\n---")
                .expect("write note");

            let index =
                IndexerService::new(temp.path()).build().expect("build index");

            assert_eq!(
                index
                    .note(Path::new("todo.md"))
                    .and_then(Note::frontmatter)
                    .map(|fm| fm.fields().len()),
                Some(1)
            );
        }

        #[test]
        fn includes_tasks_in_the_indexed_note() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("todo.md"), "- [ ] task 1")
                .expect("write note");

            let index =
                IndexerService::new(temp.path()).build().expect("build index");

            assert_eq!(
                index
                    .note(Path::new("todo.md"))
                    .map(Note::tasks)
                    .map(Iterator::count),
                Some(1)
            );
        }

        #[test]
        fn indexing_never_rewrites_the_source_markdown_file() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let original = "Status:: Draft #urgent\n\n- Reviewer:: Jane \
                            #book\n\n# Heading #chapter\n";
            fs::write(temp.path().join("note.md"), original)
                .expect("write note");

            IndexerService::new(temp.path()).build().expect("build index");

            let after = fs::read_to_string(temp.path().join("note.md"))
                .expect("read note back");
            assert_eq!(after, original);
        }

        #[test]
        fn returns_io_error_when_markdown_file_is_not_utf8() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("bad.md"), [0xFF, 0xFE])
                .expect("write invalid utf8");

            let result = IndexerService::new(temp.path()).build();

            assert!(matches!(result, Err(FileIndexError::Io { .. })));
        }

        #[test]
        fn sorts_indexed_notes_by_path() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("b.md"), "# B").expect("write b");
            fs::write(temp.path().join("a.md"), "# A").expect("write a");
            let index =
                IndexerService::new(temp.path()).build().expect("build index");

            let paths: Vec<&Path> =
                index.notes().iter().map(Note::path).collect();
            assert_eq!(paths, [Path::new("a.md"), Path::new("b.md")]);
        }
    }

    mod persistence {
        use pretty_assertions::assert_eq;
        use rstest::rstest;

        use super::*;
        use crate::note::{Frontmatter, Link, LinkType, NoteFieldValue, Tag};

        #[test]
        fn round_trips_records() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(
                temp.path().join("note.md"),
                "---\ntitle: Hello\n---\n[[other_note]]\n- [x] done",
            )
            .expect("write note");
            let indexer = IndexerService::new(temp.path());
            let built = indexer.build().expect("build index");
            indexer.persist(&built).expect("persist index");
            let loaded = indexer.load().expect("load index");

            assert_eq!(loaded.records(), built.records());
        }

        #[test]
        fn round_trips_notes_with_outlinks() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(
                temp.path().join("note.md"),
                "---\ntitle: Hello\n---\n[[other_note]]\n- [x] done",
            )
            .expect("write note");
            let indexer = IndexerService::new(temp.path());
            let built = indexer.build().expect("build index");
            indexer.persist(&built).expect("persist index");
            let loaded = indexer.load().expect("load index");

            assert_eq!(loaded.notes(), built.notes());

            let loaded_note =
                loaded.note(Path::new("note.md")).expect("loaded note");
            assert_eq!(loaded_note.outlinks().len(), 1);
            assert_eq!(
                loaded_note.outlinks().first().map(Link::target),
                Some("other_note")
            );
        }

        #[test]
        fn round_trips_task_count() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(
                temp.path().join("note.md"),
                "---\ntitle: Hello\n---\n[[other_note]]\n- [x] done",
            )
            .expect("write note");
            let indexer = IndexerService::new(temp.path());
            let built = indexer.build().expect("build index");
            indexer.persist(&built).expect("persist index");
            let loaded = indexer.load().expect("load index");

            let loaded_note =
                loaded.note(Path::new("note.md")).expect("loaded note");
            assert_eq!(loaded_note.tasks().count(), 1);
        }

        #[test]
        fn persist_then_load_recovers_frontmatter_link_fields() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(
                temp.path().join("note.md"),
                "---\nrelated: \"[[Project Alpha|Alpha]]\"\n---\nBody text.",
            )
            .expect("write note");
            let indexer = IndexerService::new(temp.path());
            let built = indexer.build().expect("build index");
            indexer.persist(&built).expect("persist index");
            let loaded = indexer.load().expect("load index");

            let field = loaded
                .note(Path::new("note.md"))
                .and_then(Note::frontmatter)
                .into_iter()
                .flat_map(Frontmatter::fields)
                .find(|(k, _)| k.is_canonical_match("related"))
                .expect("related field");
            assert_eq!(
                field.1,
                &NoteFieldValue::Link(Link::new(
                    "Project Alpha",
                    "Alpha",
                    LinkType::Wikilink
                ))
            );
        }

        #[rstest]
        #[case::body("Status:: Draft", "status", "Draft")]
        #[case::visible_key("[Status:: Draft]", "status", "Draft")]
        #[case::hidden_key("(Status:: Draft)", "status", "Draft")]
        fn persist_then_load_recovers_inline_fields(
            #[case] source: &str,
            #[case] expected_key: &str,
            #[case] expected_value: &str,
        ) {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("note.md"), source).expect("write note");
            let indexer = IndexerService::new(temp.path());
            let built = indexer.build().expect("build index");
            indexer.persist(&built).expect("persist index");
            let loaded = indexer.load().expect("load index");

            let loaded_note =
                loaded.note(Path::new("note.md")).expect("loaded note");
            let built_note =
                built.note(Path::new("note.md")).expect("built note");
            assert_eq!(loaded_note.inline_fields(), built_note.inline_fields());
            let (key, values) = loaded_note
                .inline_fields()
                .iter()
                .next()
                .expect("inline field present");
            assert!(key.is_canonical_match(expected_key));
            assert_eq!(
                values.first().and_then(|v| v.as_str()),
                Some(expected_value)
            );
        }

        #[test]
        fn persist_then_load_recovers_typed_inline_field_values() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(
                temp.path().join("note.md"),
                "[duration:: 7 hours]\n[values:: 1, 2]",
            )
            .expect("write note");
            let indexer = IndexerService::new(temp.path());
            let built = indexer.build().expect("build index");
            indexer.persist(&built).expect("persist index");
            let loaded = indexer.load().expect("load index");

            let loaded_note =
                loaded.note(Path::new("note.md")).expect("loaded note");
            let built_note =
                built.note(Path::new("note.md")).expect("built note");
            assert_eq!(loaded_note.inline_fields(), built_note.inline_fields());
            let values: Vec<&NoteFieldValue> = loaded_note
                .inline_fields()
                .values()
                .flat_map(|vals| vals.iter())
                .collect();
            assert_eq!(values, [
                &NoteFieldValue::Duration("7 hours".to_owned()),
                &NoteFieldValue::List(vec![
                    NoteFieldValue::Number(1.0),
                    NoteFieldValue::Number(2.0)
                ])
            ]);
        }

        #[test]
        fn persist_then_load_recovers_tags() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("note.md"), "Filed under #book today.")
                .expect("write note");
            let indexer = IndexerService::new(temp.path());
            let built = indexer.build().expect("build index");
            indexer.persist(&built).expect("persist index");
            let loaded = indexer.load().expect("load index");

            let loaded_note =
                loaded.note(Path::new("note.md")).expect("loaded note");
            let built_note =
                built.note(Path::new("note.md")).expect("built note");
            assert_eq!(loaded_note.tags(), built_note.tags());
            assert_eq!(loaded_note.tags(), [Tag::new("#book")]);
        }

        #[test]
        fn returns_empty_when_nothing_persisted() {
            let temp = tempfile::tempdir().expect("create temp dir");

            let index =
                IndexerService::new(temp.path()).load().expect("load index");

            assert_eq!(index.records().len(), 0);
            assert_eq!(index.notes().len(), 0);
        }

        #[test]
        fn persists_rebuilds_rather_than_appends() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("first.md"), "- [ ] first")
                .expect("write first");
            let indexer = IndexerService::new(temp.path());
            indexer
                .persist(&indexer.build().expect("build first index"))
                .expect("persist first index");
            fs::remove_file(temp.path().join("first.md"))
                .expect("remove first");
            fs::write(temp.path().join("second.md"), "- [x] second")
                .expect("write second");

            indexer
                .persist(&indexer.build().expect("build second index"))
                .expect("persist second index");
            let loaded = indexer.load().expect("load index");

            assert_eq!(loaded.records().len(), 1);
            assert_eq!(loaded.notes().len(), 1);
            assert_eq!(
                loaded.records().first().map(FileRecord::path),
                Some(Path::new("second.md"))
            );
            assert_eq!(
                loaded.notes().first().map(Note::path),
                Some(Path::new("second.md"))
            );
        }
    }

    mod refresh {
        use std::path::PathBuf;

        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn reparses_a_note_whose_content_and_size_changed() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("note.md"), "- [ ] task")
                .expect("write note");
            let indexer = IndexerService::new(temp.path());
            indexer
                .persist(&indexer.build().expect("build index"))
                .expect("persist index");

            fs::write(temp.path().join("note.md"), "- [ ] task\n- [x] second")
                .expect("rewrite note");

            let refreshed = indexer.refresh().expect("refresh index");
            assert_eq!(
                refreshed
                    .note(Path::new("note.md"))
                    .map(|note| note.tasks().count()),
                Some(2)
            );
        }

        #[test]
        #[expect(
            clippy::disallowed_methods,
            reason = "not async code; this test waits for the filesystem \
                      mtime to advance after rewriting a file with the same \
                      byte length; tokio::time::sleep does not apply here and \
                      filetime is not worth the dep"
        )]
        fn reparses_a_note_when_modified_timestamp_changes_with_same_file_size()
        {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("note.md"), "Status:: Draft")
                .expect("write note");
            let indexer = IndexerService::new(temp.path());
            indexer
                .persist(&indexer.build().expect("build index"))
                .expect("persist index");

            std::thread::sleep(std::time::Duration::from_millis(15));
            fs::write(temp.path().join("note.md"), "Status:: Final")
                .expect("rewrite note with same byte length");

            let refreshed = indexer.refresh().expect("refresh index");
            let value = refreshed
                .note(Path::new("note.md"))
                .and_then(|n| n.inline_fields().iter().next())
                .and_then(|(_, vals)| vals.first())
                .and_then(|v| v.as_str());
            assert_eq!(value, Some("Final"));
        }

        #[test]
        fn includes_newly_added_note() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("first.md"), "# First")
                .expect("write first");
            let indexer = IndexerService::new(temp.path());
            indexer
                .persist(&indexer.build().expect("build index"))
                .expect("persist index");

            fs::write(temp.path().join("second.md"), "# Second")
                .expect("write second");

            let refreshed = indexer.refresh().expect("refresh index");
            assert_eq!(refreshed.notes().len(), 2);
        }

        #[test]
        fn excludes_deleted_note() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("gone.md"), "# Gone")
                .expect("write note");
            let indexer = IndexerService::new(temp.path());
            indexer
                .persist(&indexer.build().expect("build index"))
                .expect("persist index");

            fs::remove_file(temp.path().join("gone.md")).expect("delete note");

            let refreshed = indexer.refresh().expect("refresh index");
            assert_eq!(refreshed.notes().len(), 0);
            assert_eq!(refreshed.records().len(), 0);
        }

        #[test]
        fn excludes_inlink_after_linker_deletion() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("target.md"), "# Target")
                .expect("write target");
            fs::write(temp.path().join("linker.md"), "[[target]]")
                .expect("write linker");
            let indexer = IndexerService::new(temp.path());
            indexer
                .persist(&indexer.build().expect("build index"))
                .expect("persist index");

            fs::remove_file(temp.path().join("linker.md"))
                .expect("delete linker");

            let refreshed = indexer.refresh().expect("refresh index");
            let outcome = query_all(refreshed, &QuerySource::All);
            let target = outcome.iter().next().expect("target record");

            assert_eq!(target.file().path(), Path::new("target.md"));
            assert!(target.inlinks().is_empty());
        }

        #[test]
        fn moves_inlink_when_linker_retargets() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("old-target.md"), "# Old")
                .expect("write old target");
            fs::write(temp.path().join("new-target.md"), "# New")
                .expect("write new target");
            fs::write(temp.path().join("linker.md"), "[[old-target]]")
                .expect("write linker");
            let indexer = IndexerService::new(temp.path());
            indexer
                .persist(&indexer.build().expect("build index"))
                .expect("persist index");

            fs::write(temp.path().join("linker.md"), "[[new-target]]")
                .expect("repoint linker");

            let refreshed = indexer.refresh().expect("refresh index");
            let outcome = query_all(refreshed, &QuerySource::All);
            let old_target = outcome
                .iter()
                .find(|record| {
                    record.file().path() == Path::new("old-target.md")
                })
                .expect("old target record");
            let new_target = outcome
                .iter()
                .find(|record| {
                    record.file().path() == Path::new("new-target.md")
                })
                .expect("new target record");

            assert!(old_target.inlinks().is_empty());
            assert_eq!(new_target.inlinks(), [PathBuf::from("linker.md")]);
        }

        #[test]
        fn persists_refreshed_changes_survive_load() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("note.md"), "# Draft")
                .expect("write note");
            let indexer = IndexerService::new(temp.path());
            indexer
                .persist(&indexer.build().expect("build index"))
                .expect("persist index");

            fs::write(temp.path().join("extra.md"), "# Extra")
                .expect("write extra");
            indexer
                .persist(&indexer.refresh().expect("refresh index"))
                .expect("persist index");

            let loaded = indexer.load().expect("load index");
            assert_eq!(loaded.notes().len(), 2);
        }

        #[test]
        fn builds_an_index_when_nothing_was_persisted_yet() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("note.md"), "# Note")
                .expect("write note");

            let refreshed = IndexerService::new(temp.path())
                .refresh()
                .expect("refresh index");
            assert_eq!(refreshed.notes().len(), 1);
        }

        #[test]
        fn preserves_inlinks_after_noop_refresh() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("target.md"), "# Target")
                .expect("write target");
            fs::write(temp.path().join("linker.md"), "[[target]]")
                .expect("write linker");
            let indexer = IndexerService::new(temp.path());
            indexer
                .persist(&indexer.build().expect("build index"))
                .expect("persist index");

            let refreshed = indexer.refresh().expect("refresh index");
            let outcome = query_all(refreshed, &QuerySource::All);
            let target = outcome
                .iter()
                .find(|record| record.file().path() == Path::new("target.md"))
                .expect("target record");

            assert_eq!(target.inlinks(), [PathBuf::from("linker.md")]);
        }

        #[test]
        fn returns_unpersisted_index_from_refresh() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("note.md"), "---\ntitle: Draft\n---")
                .expect("write note");
            let indexer = IndexerService::new(temp.path());
            indexer
                .persist(&indexer.build().expect("build index"))
                .expect("persist index");

            fs::write(temp.path().join("note.md"), "# Revised")
                .expect("rewrite note");

            let refreshed = indexer.refresh().expect("refresh index");

            // The refreshed index reflects the new content...
            assert_eq!(
                refreshed
                    .note(Path::new("note.md"))
                    .and_then(Note::frontmatter)
                    .and_then(|fm| fm.fields().values().next())
                    .and_then(|v| v.as_str()),
                None // "# Revised" has no frontmatter
            );
            // ...but a fresh load from disk still shows the OLD content,
            // because refresh did not persist.
            let loaded = indexer.load().expect("load index");
            assert_eq!(
                loaded
                    .note(Path::new("note.md"))
                    .and_then(Note::frontmatter)
                    .and_then(|fm| fm.fields().values().next())
                    .and_then(|v| v.as_str()),
                Some("Draft") // OLD frontmatter, not the revised content
            );
        }

        #[test]
        fn resolves_stale_ambiguous_wikilink_after_unrelated_deletion() {
            // `a.md`'s own bytes never change in this test. Its `[[foo]]`
            // link starts ambiguous (two Notes named `foo`) and later
            // becomes resolvable purely because a *different* Note is
            // deleted. `refresh`'s per-file staleness check would mark
            // `a.md` "unchanged, reused" and skip re-parsing it — proving
            // inlinks must come from a full recompute over every indexed
            // Note (gated on whether *anything* changed), not a patch
            // limited to the notes `refresh` actually re-parsed.
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::create_dir_all(temp.path().join("notes")).expect("mkdir notes");
            fs::create_dir_all(temp.path().join("archive"))
                .expect("mkdir archive");
            fs::write(temp.path().join("notes/foo.md"), "# Foo")
                .expect("write notes/foo.md");
            fs::write(temp.path().join("archive/foo.md"), "# Old Foo")
                .expect("write archive/foo.md");
            fs::write(temp.path().join("a.md"), "[[foo]]").expect("write a");
            let indexer = IndexerService::new(temp.path());
            indexer
                .persist(&indexer.build().expect("build index"))
                .expect("persist index");

            fs::remove_file(temp.path().join("archive/foo.md"))
                .expect("delete archive/foo.md");

            let refreshed = indexer.refresh().expect("refresh index");
            let outcome = query_all(refreshed, &QuerySource::All);
            let target = outcome
                .iter()
                .find(|record| {
                    record.file().path() == Path::new("notes/foo.md")
                })
                .expect("notes/foo.md record");

            assert_eq!(target.inlinks(), [PathBuf::from("a.md")]);
        }
    }
}
