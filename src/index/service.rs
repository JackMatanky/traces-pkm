//! Index lifecycle service.
//!
//! [`IndexerService`] scans, parses, persists, loads, refreshes, and opens one
//! project root's [`super::WorkspaceIndex`] through `IndexStore`.
//!
//! `refresh` and `refresh_store` share one incremental core: content-only
//! deltas patch inbound links from touched notes, while path-set changes force
//! a full recompute because wikilink resolution depends on every indexed path.

use std::path::{Path, PathBuf};

use rayon::prelude::*;

use super::{
    INDEX_FILE, IndexError, IndexResult, WorkspaceIndex,
    inlinks::InlinkMap,
    refresh::{PendingApply, RefreshPlan, RefreshReport, RefreshState},
    sort::SortedByPath,
    store::{IndexDimensions, IndexStore, PersistRequest},
};
use crate::{
    Config, DirTree, Note, TaskConfig,
    config::FrontmatterConfig,
    file::{FileBase, FileFormat},
    note::{MarkdownParserInput, parse_markdown},
    path::RelativePath,
};

/// Drives the file-index lifecycle for one project root.
///
/// `refresh` returns a full [`WorkspaceIndex`] and logs persist failures;
/// `refresh_store` keeps only the persisted store current and propagates
/// persist failures because it has no in-memory fallback.
#[derive(Clone, Debug)]
pub struct IndexerService {
    root: PathBuf,
    tasks: TaskConfig,
    frontmatter: FrontmatterConfig,
    class_field: String,
}

impl From<&Config> for IndexerService {
    #[inline]
    fn from(config: &Config) -> Self {
        Self {
            root: config.root().to_path_buf(),
            tasks: config.tasks().clone(),
            frontmatter: config.frontmatter().clone(),
            class_field: config.schemas().class_field_name().to_owned(),
        }
    }
}

impl IndexerService {
    /// Builds a default-configured service for tests.
    #[cfg(any(test, feature = "test-utils"))]
    #[inline]
    #[must_use]
    pub fn for_tests<P: Into<PathBuf>>(root: P) -> Self {
        Self::from(&Config::test_default(root))
    }

    /// Scans this service's root and builds a [`WorkspaceIndex`] in memory.
    ///
    /// # Errors
    ///
    /// - `IndexError::Walk` if a directory cannot be read.
    /// - `IndexError::Inspect` if a file's metadata cannot be inspected.
    /// - `IndexError::NoteParse` if a Markdown file cannot be read or parsed.
    /// - `IndexError::Path` if a walked file cannot be derived as a safe
    ///   project-relative path.
    #[inline]
    pub fn build(&self) -> IndexResult<WorkspaceIndex> {
        let files = SortedByPath::assumed_sorted(Self::scan(&self.root)?);
        let notes =
            SortedByPath::assumed_sorted(self.parse_notes(files.as_slice())?);
        let inlinks = InlinkMap::new(notes.as_slice(), files.as_slice());
        Ok(WorkspaceIndex::assemble(files, notes, inlinks))
    }

    /// Refreshes the persisted index and returns a full in-memory
    /// [`WorkspaceIndex`].
    ///
    /// Unchanged Markdown notes reuse persisted parses; changed notes are
    /// parsed from disk; deleted files vanish with the fresh scan. Persist
    /// failures are logged and the refreshed in-memory index is still returned.
    ///
    /// Inlinks are patched only for content-only deltas. Any added, deleted, or
    /// renamed path forces a full recompute because an unedited note's resolved
    /// wikilink target can change when candidate paths change.
    ///
    /// # Errors
    ///
    /// - `IndexError::Walk` if a directory cannot be read.
    /// - `IndexError::Inspect` if file metadata cannot be inspected.
    /// - `IndexError::NoteParse` if a Markdown file cannot be read or parsed,
    ///   or an unchanged note cannot be recalled.
    /// - `IndexError::Path` if a walked file cannot be derived as a safe
    ///   project-relative path.
    /// - `IndexError::Store` if the persisted index cannot be opened or read.
    #[inline]
    pub fn refresh(&self) -> IndexResult<WorkspaceIndex> {
        let (index, _) = self.refresh_with_report()?;
        Ok(index)
    }

    /// Refreshes the persisted index and returns changed-row counts.
    ///
    /// # Errors
    ///
    /// - `IndexError::Walk` if a directory cannot be read.
    /// - `IndexError::Inspect` if file metadata cannot be inspected.
    /// - `IndexError::NoteParse` if a Markdown file cannot be read or parsed,
    ///   or an unchanged note cannot be recalled.
    /// - `IndexError::Path` if a walked file cannot be derived as a safe
    ///   project-relative path.
    /// - `IndexError::Store` if the persisted index cannot be opened or read.
    #[inline]
    pub fn refresh_with_report(
        &self,
    ) -> IndexResult<(WorkspaceIndex, RefreshReport)> {
        match self.prepare_pass()? {
            RefreshState::Fresh(store) => Self::assemble_unchanged(&store),
            RefreshState::Stale(pending) => self.apply_reconciled(*pending),
        }
    }

    fn assemble_unchanged(
        store: &IndexStore,
    ) -> IndexResult<(WorkspaceIndex, RefreshReport)> {
        let (files, notes, links) = store.read_all()?;
        Ok((
            WorkspaceIndex::assemble(files, notes, links),
            RefreshReport::default(),
        ))
    }

    fn apply_reconciled(
        &self,
        pending: PendingApply,
    ) -> IndexResult<(WorkspaceIndex, RefreshReport)> {
        let report = pending.report();
        let dimensions = IndexDimensions::for_class_field(&self.class_field);
        let index = match pending.apply(dimensions) {
            Ok(persisted) => {
                Self::log_report(&report);
                persisted.into_index()?
            }
            Err(failed) => {
                tracing::warn!(
                    source = %failed.source(),
                    "failed to persist refreshed index"
                );
                failed.into_index()?
            }
        };
        Ok((index, report))
    }

    fn prepare_pass(&self) -> IndexResult<RefreshState> {
        let plan = RefreshPlan::collect(&self.root)?;
        if plan.is_fresh() {
            return Ok(plan.into_fresh());
        }
        let modified_notes = self.parse_notes(plan.upserted_files())?;
        let pending = plan.reconcile(modified_notes)?;
        Ok(RefreshState::Stale(Box::new(pending)))
    }

    /// Rebuilds and persists the index from scratch, returning it.
    ///
    /// Private helper for the `traces index` command, factored so this crate
    /// owns the rebuild policy and the CLI handler stays a single forwarding
    /// call.
    ///
    /// # Errors
    ///
    /// - [`IndexError::Walk`] if a directory cannot be read.
    /// - [`IndexError::Inspect`] if file metadata cannot be inspected.
    /// - [`IndexError::NoteParse`] if a note cannot be read or parsed.
    /// - [`IndexError::Path`] if a walked file cannot be derived as a safe
    ///   project-relative path.
    /// - [`IndexError::Store`] if persisting the rebuilt index fails.
    pub(crate) fn rebuild(&self) -> IndexResult<WorkspaceIndex> {
        let index = self.build()?;
        self.persist(&index)?;
        Ok(index)
    }

    /// Synchronizes the persisted index without materializing a
    /// [`WorkspaceIndex`].
    ///
    /// Cold empty deltas return the opened store without decoding notes or
    /// writing rows. Unlike [`Self::refresh`] (which absorbs persist failures
    /// by logging a warning and falling back to an in-memory index), persist
    /// failures are propagated directly as [`IndexError::Store`] because
    /// callers read from the returned persisted store and have no in-memory
    /// fallback.
    ///
    /// # Errors
    ///
    /// - [`IndexError::Walk`] if a directory cannot be read.
    /// - [`IndexError::Inspect`] if file metadata cannot be inspected.
    /// - [`IndexError::NoteParse`] if a Markdown file cannot be read or parsed,
    ///   or an unchanged note cannot be recalled.
    /// - [`IndexError::Path`] if a walked file cannot be derived as a safe
    ///   project-relative path.
    /// - [`IndexError::Store`] if the database cannot be opened or read,
    ///   required note bodies cannot be read, or incremental persistence fails.
    #[inline]
    pub(crate) fn refresh_store(&self) -> IndexResult<IndexStore> {
        match self.prepare_pass()? {
            RefreshState::Fresh(store) => Ok(store),
            RefreshState::Stale(pending) => {
                let report = pending.report();
                let dimensions =
                    IndexDimensions::for_class_field(&self.class_field);
                match (*pending).apply(dimensions) {
                    Ok(persisted) => {
                        Self::log_report(&report);
                        Ok(persisted.into_store())
                    }
                    Err(failed) => Err(failed.into_error()),
                }
            }
        }
    }

    fn log_report(report: &RefreshReport) {
        tracing::debug!(
            upserted = report.upserted_count(),
            deleted = report.deleted_count(),
            links_modified = report.links_modified_count(),
            "index refreshed"
        );
    }

    /// Parses every Markdown-classified file in `files` in parallel, stopping
    /// at the first parse failure.
    fn parse_notes(&self, files: &[FileBase]) -> IndexResult<Vec<Note>> {
        files
            .par_iter()
            .filter(|file| file.format() == FileFormat::Note)
            .map(|file| self.parse_note(file))
            .collect::<IndexResult<Vec<Note>>>()
    }

    /// Reads and parses the Markdown file at `file`'s path, resolved relative
    /// to this service's root.
    fn parse_note(&self, file: &FileBase) -> IndexResult<Note> {
        let full_path = self.root.join(file.path());
        let content =
            std::fs::read_to_string(&full_path).map_err(|source| {
                IndexError::NoteParse {
                    path: full_path,
                    source,
                }
            })?;
        let input = MarkdownParserInput::new(
            file.path(),
            &content,
            &self.tasks,
            &self.frontmatter,
        );
        Ok(parse_markdown(&input))
    }

    /// Persists `index` to this service's root, replacing any existing index.
    ///
    /// # Errors
    ///
    /// - `IndexError::Store` if the database's parent directory cannot be
    ///   created, the transaction fails, or a row cannot be encoded.
    #[inline]
    pub fn persist(&self, index: &WorkspaceIndex) -> IndexResult<()> {
        IndexStore::open(&self.root)?.persist(&PersistRequest::rebuild(
            IndexDimensions::for_class_field(&self.class_field),
            index.entries(),
        ))
    }

    /// Loads the index previously persisted for this service's root, or an
    /// empty [`WorkspaceIndex`] if none exists.
    ///
    /// # Errors
    ///
    /// - `IndexError::Store` if the database cannot be read or stored bytes are
    ///   not a valid row.
    #[inline]
    #[cfg_attr(
        not(any(test, feature = "test-utils")),
        expect(
            dead_code,
            reason = "no current caller in cli; kept for IndexerService \
                      lifecycle symmetry and tests"
        )
    )]
    pub fn load(&self) -> IndexResult<WorkspaceIndex> {
        let (files, notes, inlinks) =
            IndexStore::open(&self.root)?.read_all()?;
        Ok(WorkspaceIndex::assemble(files, notes, inlinks))
    }

    /// Recursively scans `root` for regular files, skipping `.git` directories,
    /// the index database, and symlinks. Metadata reads run in parallel via
    /// `rayon` because each is an independent syscall; results are path-sorted.
    ///
    /// # Errors
    ///
    /// - [`IndexError::Walk`] if a directory cannot be read.
    /// - [`IndexError::Inspect`] if a file's metadata cannot be inspected.
    /// - [`IndexError::Path`] if a walked file cannot be derived as a safe
    ///   project-relative path.
    pub(super) fn scan(root: &Path) -> IndexResult<Vec<FileBase>> {
        let index_db = root.join(INDEX_FILE);
        let paths = DirTree::descendants(root)
            .filter(|node| crate::env_vars::is_ignored_dir(node.file_name()))
            .filter_map(|node| {
                let node = match node {
                    Ok(node) => node,
                    Err(error) => return Some(Err(IndexError::Walk(error))),
                };
                let path = node.path();
                (node.file_type().is_file() && path != index_db)
                    .then(|| Ok(path.to_path_buf()))
            })
            .collect::<IndexResult<Vec<PathBuf>>>()?;
        let mut files = paths
            .into_par_iter()
            .map(|path| scan_file_metadata(&path, root))
            .collect::<IndexResult<Vec<FileBase>>>()?;
        files.sort_by(|a, b| a.path().cmp(b.path()));
        Ok(files)
    }
}

/// Builds `path`'s [`FileBase`] from metadata relative to `root`.
fn scan_file_metadata(path: &Path, root: &Path) -> IndexResult<FileBase> {
    let relative = RelativePath::derive(root, path)?;
    let metadata =
        std::fs::metadata(path).map_err(|source| IndexError::Inspect {
            path: path.to_path_buf(),
            source,
        })?;
    FileBase::from_metadata(relative, &metadata).map_err(|source| {
        IndexError::Inspect {
            path: path.to_path_buf(),
            source,
        }
    })
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::{Path, PathBuf},
        sync::Arc,
    };

    use super::{super::IndexError, *};
    use crate::{
        Note,
        file::FileBase,
        index::FileEntry,
        query::{QueryBuilder, QueryService, QuerySet, SourceSelector},
    };

    fn query_pages(
        index: &Arc<WorkspaceIndex>,
        source: &SourceSelector,
    ) -> QuerySet {
        QueryService::new("class")
            .run(index, QueryBuilder::pages(source.clone()))
    }

    #[test]
    fn preserves_path_sorted_order_for_hyphenated_sibling_folders() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let notes_dir = temp.path().join("notes");
        fs::create_dir_all(&notes_dir).expect("mkdir notes");
        fs::write(notes_dir.join("note.md"), "# Note\n")
            .expect("write note.md");
        fs::write(temp.path().join("notes-x.md"), "# Sibling\n")
            .expect("write notes-x.md");

        let index = IndexerService::for_tests(temp.path())
            .build()
            .expect("build index");
        let paths: Vec<_> = index
            .entries()
            .iter()
            .map(|e| e.file().path().to_str().unwrap())
            .collect();
        assert_eq!(paths, ["notes/note.md", "notes-x.md"]);
    }

    #[test]
    fn produces_identical_entries_through_persist_and_load_roundtrip() {
        let temp = tempfile::tempdir().expect("create temp dir");
        fs::write(
            temp.path().join("a.md"),
            "---\ntag: test\n---\n# Note A\n[[b]]",
        )
        .expect("write a");
        fs::write(
            temp.path().join("b.md"),
            "---\ntag: test\n---\n# Note B\n[[c]]",
        )
        .expect("write b");
        fs::write(temp.path().join("c.md"), "# Note C\n").expect("write c");

        let service = IndexerService::for_tests(temp.path());
        let fresh = service.build().expect("build");
        service.persist(&fresh).expect("persist");
        let loaded = service.load().expect("load");

        assert_eq!(fresh.entries().len(), loaded.entries().len());
        for (f, l) in fresh.entries().iter().zip(loaded.entries()) {
            assert_eq!(f.file().path(), l.file().path());
            assert_eq!(f.file().size(), l.file().size());
            assert_eq!(f.inlinks(), l.inlinks());
            assert_eq!(
                f.note().map(Note::outlinks),
                l.note().map(Note::outlinks)
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn returns_note_parse_error_when_a_note_is_unreadable() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir().expect("create temp dir");
        let bad = temp.path().join("bad.md");
        fs::write(&bad, "# Bad\n").expect("write bad");
        fs::set_permissions(&bad, fs::Permissions::from_mode(0o000))
            .expect("chmod bad");

        let result = IndexerService::for_tests(temp.path()).build();
        let err = result.expect_err("must fail on unreadable file");

        fs::set_permissions(&bad, fs::Permissions::from_mode(0o600))
            .expect("restore bad");

        assert!(matches!(err, IndexError::NoteParse { .. }));
    }

    fn find_note<'a>(
        index: &'a WorkspaceIndex,
        path: &str,
    ) -> Option<&'a Note> {
        index
            .entries()
            .iter()
            .find(|entry| entry.file().path() == Path::new(path))
            .and_then(FileEntry::note)
    }

    mod empty_delta {
        use super::*;

        #[test]
        fn refresh_store_skips_note_decode_on_empty_delta() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let root = temp.path();
            fs::write(root.join("a.md"), "# A").expect("write note");
            let service = IndexerService::for_tests(root);
            let index = service.build().expect("build index");
            service.persist(&index).expect("persist index");
            IndexStore::open(root)
                .expect("open store")
                .poison_note_row(Path::new("a.md"))
                .expect("poison note row");

            service.refresh_store().expect("empty delta sync");
        }

        #[test]
        fn refresh_materializes_on_empty_delta() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let root = temp.path();
            fs::write(root.join("a.md"), "# A").expect("write note");
            let service = IndexerService::for_tests(root);
            let index = service.build().expect("build index");
            service.persist(&index).expect("persist index");
            IndexStore::open(root)
                .expect("open store")
                .poison_note_row(Path::new("a.md"))
                .expect("poison note row");

            service.refresh().expect_err("refresh reads poisoned note");
        }
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

            let index = IndexerService::for_tests(temp.path())
                .build()
                .expect("build index");

            assert_eq!(index.entries().len(), 2);
            assert_eq!(
                index
                    .entries()
                    .iter()
                    .filter(|entry| entry.note().is_some())
                    .count(),
                1
            );
            assert_eq!(
                index
                    .entries()
                    .iter()
                    .find_map(FileEntry::note)
                    .map(Note::path),
                Some(Path::new("notes/todo.md"))
            );
        }

        #[test]
        fn includes_frontmatter_fields_in_the_indexed_note() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("todo.md"), "---\ntitle: Todo\n---")
                .expect("write note");

            let index = IndexerService::for_tests(temp.path())
                .build()
                .expect("build index");

            assert_eq!(
                find_note(&index, "todo.md")
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

            let index = IndexerService::for_tests(temp.path())
                .build()
                .expect("build index");

            assert_eq!(
                find_note(&index, "todo.md")
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

            IndexerService::for_tests(temp.path())
                .build()
                .expect("build index");

            let after = fs::read_to_string(temp.path().join("note.md"))
                .expect("read note back");
            assert_eq!(after, original);
        }

        #[test]
        fn returns_parse_error_when_markdown_file_has_invalid_utf8() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("bad.md"), [0xFF, 0xFE])
                .expect("write invalid utf8");

            let result = IndexerService::for_tests(temp.path()).build();

            assert!(matches!(result, Err(IndexError::NoteParse { .. })));
        }

        #[test]
        fn sorts_indexed_notes_by_path() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("b.md"), "# B").expect("write b");
            fs::write(temp.path().join("a.md"), "# A").expect("write a");
            let index = IndexerService::for_tests(temp.path())
                .build()
                .expect("build index");

            let paths: Vec<&Path> = index
                .entries()
                .iter()
                .filter_map(FileEntry::note)
                .map(Note::path)
                .collect();
            assert_eq!(paths, [Path::new("a.md"), Path::new("b.md")]);
        }
    }

    mod scan {
        use std::fs;

        use pretty_assertions::assert_eq;

        use super::*;
        #[cfg(unix)]
        use crate::index::tests::fixtures::PermissionsGuard;

        fn paths(files: &[FileBase]) -> Vec<&Path> {
            files.iter().map(FileBase::path).collect()
        }

        #[test]
        fn scans_nested_files_in_sorted_order() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let root = temp.path();
            fs::create_dir_all(root.join("b")).expect("mkdir b");
            fs::write(root.join("b/one.md"), "1").expect("write b/one.md");
            fs::write(root.join("a.md"), "2").expect("write a.md");

            let files = IndexerService::scan(root).expect("scan");

            assert_eq!(paths(&files), vec![
                Path::new("a.md"),
                Path::new("b/one.md")
            ]);
        }

        #[test]
        fn orders_sibling_files_and_directories_by_relative_path() {
            // Component-wise comparison orders `b` before `b.txt`, matching the
            // depth-first walk.
            let temp = tempfile::tempdir().expect("create temp dir");
            let root = temp.path();
            fs::create_dir_all(root.join("b")).expect("mkdir b");
            fs::write(root.join("b/one.md"), "1").expect("write b/one.md");
            fs::write(root.join("b.txt"), "2").expect("write b.txt");

            let files = IndexerService::scan(root).expect("scan");

            assert_eq!(paths(&files), vec![
                Path::new("b/one.md"),
                Path::new("b.txt")
            ]);
        }

        #[test]
        fn skips_git_directories() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let root = temp.path();
            fs::create_dir_all(root.join(".git")).expect("mkdir .git");
            fs::write(root.join(".git/HEAD"), "ref: refs/heads/main")
                .expect("write .git/HEAD");
            fs::write(root.join("note.md"), "content").expect("write note.md");

            let files = IndexerService::scan(root).expect("scan");

            assert_eq!(paths(&files), vec![Path::new("note.md")]);
        }

        #[test]
        fn skips_its_own_index_database_file() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let root = temp.path();
            fs::create_dir_all(root.join(".traces")).expect("mkdir .traces");
            fs::write(root.join(INDEX_FILE), b"redb-bytes")
                .expect("write index db");
            fs::write(root.join("note.md"), "content").expect("write note.md");

            let files = IndexerService::scan(root).expect("scan");

            assert_eq!(paths(&files), vec![Path::new("note.md")]);
        }

        #[cfg(unix)]
        #[test]
        fn skips_symlinks_entirely() {
            use std::os::unix::fs::symlink;

            let temp = tempfile::tempdir().expect("create temp dir");
            let outside = tempfile::tempdir().expect("create outside dir");
            let root = temp.path();
            let target = outside.path().join("outside.md");
            fs::write(&target, "content").expect("write link target");
            symlink(&target, root.join("link.md")).expect("create symlink");
            fs::write(root.join("note.md"), "content").expect("write note.md");

            let files = IndexerService::scan(root).expect("scan");

            assert_eq!(paths(&files), vec![Path::new("note.md")]);
        }

        #[test]
        fn empty_root_yields_no_files() {
            let temp = tempfile::tempdir().expect("create temp dir");

            let files = IndexerService::scan(temp.path()).expect("scan");

            assert_eq!(files.len(), 0);
        }

        #[cfg(unix)]
        #[test]
        fn returns_an_io_error_when_a_directory_is_unreadable() {
            use std::os::unix::fs::PermissionsExt as _;

            let temp = tempfile::tempdir().expect("create temp dir");
            let root = temp.path();
            let locked = root.join("locked");
            fs::create_dir(&locked).expect("create locked dir");
            fs::set_permissions(&locked, fs::Permissions::from_mode(0o000))
                .expect("revoke read permission");
            let _guard = PermissionsGuard(&locked);

            let error =
                IndexerService::scan(root).expect_err("unreadable dir fails");

            assert!(matches!(error, IndexError::Walk(_)));
        }
    }

    mod persistence {
        use pretty_assertions::assert_eq;
        use rstest::rstest;

        use super::*;
        use crate::{
            DurationValue, Tag,
            note::{Frontmatter, Link, LinkType, NoteFieldValue},
        };

        // The fixture materializes a raw non-Unicode filename on disk; macOS
        // filename normalization makes that unreliable, so this helper and the
        // tests using it run only on Linux.
        #[cfg(target_os = "linux")]
        fn non_unicode_path() -> PathBuf {
            use std::os::unix::ffi::OsStringExt as _;
            PathBuf::from(std::ffi::OsString::from_vec(
                b"weird\xFF.md".to_vec(),
            ))
        }

        /// Seeds three notes and returns the service plus untouched `a`/`c`
        /// baselines.
        fn seed_three_notes(root: &Path) -> (IndexerService, Note, Note) {
            fs::write(root.join("a.md"), "---\ntitle: A\n---\nBody A.")
                .expect("write a");
            fs::write(root.join("b.md"), "---\ntitle: B\n---\nBody B.")
                .expect("write b");
            fs::write(root.join("c.md"), "---\ntitle: C\n---\nBody C.")
                .expect("write c");
            let indexer = IndexerService::for_tests(root);
            let built = indexer.build().expect("build index");
            indexer.persist(&built).expect("persist index");
            let a = find_note(&built, "a.md").expect("note a").clone();
            let c = find_note(&built, "c.md").expect("note c").clone();
            (indexer, a, c)
        }

        #[test]
        fn round_trips_entries() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(
                temp.path().join("note.md"),
                "---\ntitle: Hello\n---\n[[other_note]]\n- [x] done",
            )
            .expect("write note");

            let indexer = IndexerService::for_tests(temp.path());
            let built = indexer.build().expect("build index");
            indexer.persist(&built).expect("persist index");
            let loaded = indexer.load().expect("load index");

            assert_eq!(loaded.entries(), built.entries());
        }

        #[test]
        fn load_keeps_inlinks_to_non_markdown_attachments() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("attachment.png"), [
                0x89, 0x50, 0x4E, 0x47,
            ])
            .expect("write attachment");
            fs::write(
                temp.path().join("note.md"),
                "# Note\n\n[[attachment.png]]\n",
            )
            .expect("write note");

            let service = IndexerService::for_tests(temp.path());
            let built = service.build().expect("build index");
            let built_inlinks = attachment_inlinks(&built);
            service.persist(&built).expect("persist index");
            let loaded = service.load().expect("load index");

            let loaded_inlinks = attachment_inlinks(&loaded);

            assert_eq!(built_inlinks, [PathBuf::from("note.md")]);
            assert_eq!(loaded_inlinks, [PathBuf::from("note.md")]);
        }

        /// Returns the attachment entry's inbound links in `index`.
        fn attachment_inlinks(index: &WorkspaceIndex) -> Vec<PathBuf> {
            index
                .entries()
                .iter()
                .find(|entry| {
                    entry.file().path() == Path::new("attachment.png")
                })
                .expect("attachment entry exists")
                .inlinks()
                .to_vec()
        }

        #[test]
        fn round_trips_notes_with_outlinks() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(
                temp.path().join("note.md"),
                "---\ntitle: Hello\n---\n[[other_note]]\n- [x] done",
            )
            .expect("write note");
            let indexer = IndexerService::for_tests(temp.path());
            let built = indexer.build().expect("build index");
            indexer.persist(&built).expect("persist index");
            let loaded = indexer.load().expect("load index");

            assert_eq!(loaded.entries(), built.entries());

            let loaded_note =
                find_note(&loaded, "note.md").expect("loaded note");
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
            let indexer = IndexerService::for_tests(temp.path());
            let built = indexer.build().expect("build index");
            indexer.persist(&built).expect("persist index");
            let loaded = indexer.load().expect("load index");

            let loaded_note =
                find_note(&loaded, "note.md").expect("loaded note");
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
            let indexer = IndexerService::for_tests(temp.path());
            let built = indexer.build().expect("build index");
            indexer.persist(&built).expect("persist index");
            let loaded = indexer.load().expect("load index");

            let field = find_note(&loaded, "note.md")
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
            let indexer = IndexerService::for_tests(temp.path());
            let built = indexer.build().expect("build index");
            indexer.persist(&built).expect("persist index");
            let loaded = indexer.load().expect("load index");

            let loaded_note =
                find_note(&loaded, "note.md").expect("loaded note");
            let built_note = find_note(&built, "note.md").expect("built note");
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
            let indexer = IndexerService::for_tests(temp.path());
            let built = indexer.build().expect("build index");
            indexer.persist(&built).expect("persist index");
            let loaded = indexer.load().expect("load index");

            let loaded_note =
                find_note(&loaded, "note.md").expect("loaded note");
            let built_note = find_note(&built, "note.md").expect("built note");
            assert_eq!(loaded_note.inline_fields(), built_note.inline_fields());
            let values: Vec<&NoteFieldValue> = loaded_note
                .inline_fields()
                .values()
                .flat_map(|vals| vals.iter())
                .collect();
            assert_eq!(values, [
                &NoteFieldValue::Duration(
                    DurationValue::parse("7 hours").expect("valid duration"),
                ),
                &NoteFieldValue::List(
                    vec![
                        NoteFieldValue::Number(1.0),
                        NoteFieldValue::Number(2.0),
                    ]
                    .into(),
                ),
            ]);
        }

        #[test]
        fn persist_then_load_recovers_tags() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("note.md"), "Filed under #book today.")
                .expect("write note");
            let indexer = IndexerService::for_tests(temp.path());
            let built = indexer.build().expect("build index");
            indexer.persist(&built).expect("persist index");
            let loaded = indexer.load().expect("load index");

            let loaded_note =
                find_note(&loaded, "note.md").expect("loaded note");
            let built_note = find_note(&built, "note.md").expect("built note");
            assert_eq!(loaded_note.tags(), built_note.tags());
            assert_eq!(loaded_note.tags(), [Tag::parse("#book").unwrap()]);
        }

        #[test]
        fn returns_empty_when_nothing_persisted() {
            let temp = tempfile::tempdir().expect("create temp dir");

            let index = IndexerService::for_tests(temp.path())
                .load()
                .expect("load index");

            assert_eq!(index.entries().len(), 0);
            assert_eq!(
                index
                    .entries()
                    .iter()
                    .filter(|entry| entry.note().is_some())
                    .count(),
                0
            );
        }

        #[test]
        fn persists_rebuilds_rather_than_appends() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("first.md"), "- [ ] first")
                .expect("write first");
            let indexer = IndexerService::for_tests(temp.path());
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

            assert_eq!(loaded.entries().len(), 1);
            assert_eq!(
                loaded
                    .entries()
                    .iter()
                    .filter(|entry| entry.note().is_some())
                    .count(),
                1
            );
            assert_eq!(
                loaded
                    .entries()
                    .first()
                    .map(FileEntry::file)
                    .map(FileBase::path),
                Some(Path::new("second.md"))
            );
            assert_eq!(
                loaded
                    .entries()
                    .iter()
                    .find_map(FileEntry::note)
                    .map(Note::path),
                Some(Path::new("second.md"))
            );
        }

        #[test]
        fn incremental_refresh_persist_actually_removes_a_deleted_notes_row_from_disk()
         {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("keep.md"), "# Keep")
                .expect("write keep");
            fs::write(temp.path().join("gone.md"), "# Gone")
                .expect("write gone");
            let indexer = IndexerService::for_tests(temp.path());
            indexer
                .persist(&indexer.build().expect("build index"))
                .expect("persist index");

            fs::remove_file(temp.path().join("gone.md")).expect("delete gone");
            indexer.refresh().expect("refresh persists internally");

            let loaded = indexer.load().expect("load index");
            assert_eq!(loaded.entries().len(), 1);
            assert_eq!(
                loaded
                    .entries()
                    .iter()
                    .filter(|entry| entry.note().is_some())
                    .count(),
                1
            );
            assert!(find_note(&loaded, "gone.md").is_none());
            assert_eq!(
                loaded
                    .entries()
                    .first()
                    .map(FileEntry::file)
                    .map(FileBase::path),
                Some(Path::new("keep.md"))
            );
        }

        #[test]
        fn refresh_updates_changed_path_in_persisted_store() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let (indexer, ..) = seed_three_notes(temp.path());
            fs::write(
                temp.path().join("b.md"),
                "---\ntitle: B\n---\nBody B changed.",
            )
            .expect("rewrite b");

            let (_refreshed, report) =
                indexer.refresh_with_report().expect("refresh index");

            assert_eq!(report.upserted_count(), 1);
            assert_eq!(report.deleted_count(), 0);
            let store = IndexStore::open(temp.path()).expect("open store");
            let notes = store
                .read_notes_batch([Path::new("b.md")])
                .expect("read batch");
            assert_eq!(notes.len(), 1);
            let note_b = notes.first().expect("note b exists");
            assert_eq!(
                note_b.frontmatter().and_then(|fm| fm.get("title").cloned()),
                Some(crate::NoteFieldValue::String("B".to_owned()))
            );
        }

        #[test]
        fn incremental_persist_preserves_unchanged_notes_and_updates_changed_note()
         {
            let temp = tempfile::tempdir().expect("create temp dir");
            let (indexer, original_a, original_c) =
                seed_three_notes(temp.path());
            fs::write(
                temp.path().join("b.md"),
                "---\ntitle: B\n---\nBody B changed.",
            )
            .expect("rewrite b");
            let refreshed = indexer.refresh().expect("refresh index");

            indexer.persist(&refreshed).expect("persist refreshed index");
            let loaded = indexer.load().expect("load index");

            // Untouched notes persist byte-identical to their original writes.
            assert_eq!(find_note(&loaded, "a.md"), Some(&original_a));
            assert_eq!(find_note(&loaded, "c.md"), Some(&original_c));

            let loaded_b = find_note(&loaded, "b.md").expect("note b");
            assert_eq!(
                loaded_b.frontmatter().and_then(|fm| fm.get("title").cloned()),
                Some(crate::NoteFieldValue::String("B".to_owned()))
            );
        }

        #[test]
        fn noop_refresh_after_incremental_persist_reports_empty_delta() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let (indexer, ..) = seed_three_notes(temp.path());
            fs::write(
                temp.path().join("b.md"),
                "---\ntitle: B\n---\nBody B changed.",
            )
            .expect("rewrite b");
            indexer.refresh().expect("refresh index");

            let (_, report) =
                indexer.refresh_with_report().expect("noop refresh");

            assert_eq!(report, RefreshReport::default());
        }

        // See `non_unicode_path`: macOS normalization makes the on-disk
        // fixture unreliable, so this runs only on Linux.
        #[cfg(target_os = "linux")]
        #[test]
        fn preserves_byte_exact_non_unicode_inlinks_on_unchanged_refresh() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let store = IndexStore::open(temp.path()).expect("open store");
            let weird = non_unicode_path();
            let normal = PathBuf::from("normal.md");
            fs::write(temp.path().join(&weird), "link to [[normal]]")
                .expect("write weird note");
            fs::write(temp.path().join(&normal), "link to [[weird]]")
                .expect("write normal note");
            let files = IndexerService::scan(temp.path()).expect("scan root");
            assert!(
                files.iter().any(|file| file.path() == weird),
                "fixture requires a filesystem that materializes the \
                 non-Unicode path"
            );
            let mut notes = vec![
                crate::parse_note(&weird, "link to [[normal]]"),
                crate::parse_note(&normal, "link to [[weird]]"),
            ];
            notes.sort_by(|a, b| a.path().cmp(b.path()));
            let mut links = std::collections::HashMap::new();
            links.insert(weird.clone(), vec![normal.clone()].into());
            links.insert(normal.clone(), vec![weird.clone()].into());
            let links = InlinkMap::from_raw(links);
            store
                .persist(&PersistRequest::rebuild(
                    IndexDimensions::for_class_field("class"),
                    WorkspaceIndex::assemble(
                        SortedByPath::sorted(files),
                        SortedByPath::sorted(notes),
                        links,
                    )
                    .entries(),
                ))
                .expect("persist index");
            drop(store);

            let service = IndexerService::for_tests(temp.path());
            let (refreshed, report) =
                service.refresh_with_report().expect("refresh unchanged");

            assert_eq!(report, RefreshReport::default());
            let inlinks_of = |path: &Path| -> Vec<PathBuf> {
                refreshed
                    .entries()
                    .iter()
                    .find(|entry| entry.file().path() == path)
                    .expect("entry exists")
                    .inlinks()
                    .to_vec()
            };
            assert_eq!(inlinks_of(&weird), std::slice::from_ref(&normal));
            assert_eq!(inlinks_of(&normal), std::slice::from_ref(&weird));
        }

        #[test]
        fn content_change_without_outlink_change_backdates_and_skips_inlink_recompute()
         {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("target.md"), "# Target")
                .expect("write target");
            fs::write(temp.path().join("linker.md"), "[[target]]\n- [ ] task")
                .expect("write linker");
            let indexer = IndexerService::for_tests(temp.path());
            indexer
                .persist(&indexer.build().expect("build index"))
                .expect("persist index");

            fs::write(temp.path().join("linker.md"), "[[target]]\n- [x] task")
                .expect("rewrite linker: task checked, same outlink");

            let (_, report) =
                indexer.refresh_with_report().expect("refresh index");

            assert_eq!(report.upserted_count(), 1);
            assert_eq!(report.links_modified_count(), 0);
        }

        #[test]
        fn outlink_change_modifies_links_and_recomputes_inlinks() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("old-target.md"), "# Old")
                .expect("write old target");
            fs::write(temp.path().join("new-target.md"), "# New")
                .expect("write new target");
            fs::write(temp.path().join("linker.md"), "[[old-target]]")
                .expect("write linker");
            let indexer = IndexerService::for_tests(temp.path());
            indexer
                .persist(&indexer.build().expect("build index"))
                .expect("persist index");

            fs::write(temp.path().join("linker.md"), "[[new-target]]")
                .expect("retarget linker");

            let (_, report) =
                indexer.refresh_with_report().expect("refresh index");
            assert_eq!(report.upserted_count(), 1);
            assert!(report.links_modified_count() > 0);

            let store = IndexStore::open(temp.path()).expect("open store");
            let (_, _, links) = store.read_all().expect("read all");
            assert_eq!(links.inlinks_of(Path::new("new-target.md")), [
                PathBuf::from("linker.md")
            ]);
            assert!(!links.has_target(Path::new("old-target.md")));
        }

        #[test]
        fn reordered_or_relabeled_outlinks_with_same_targets_still_backdates() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("a.md"), "# A").expect("write a");
            fs::write(temp.path().join("b.md"), "# B").expect("write b");
            fs::write(temp.path().join("linker.md"), "[[a]]\n[[b]]")
                .expect("write linker");
            let indexer = IndexerService::for_tests(temp.path());
            indexer
                .persist(&indexer.build().expect("build index"))
                .expect("persist index");

            fs::write(temp.path().join("linker.md"), "[[b|Bee]]\n[[a]]")
                .expect("reorder links and relabel display text, same targets");

            let (_, report) =
                indexer.refresh_with_report().expect("refresh index");
            assert_eq!(report.links_modified_count(), 0);
        }

        #[test]
        fn brand_new_note_always_contributes_to_staleness() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("a.md"), "# A").expect("write a");
            let indexer = IndexerService::for_tests(temp.path());
            indexer
                .persist(&indexer.build().expect("build index"))
                .expect("persist index");

            fs::write(temp.path().join("b.md"), "# B, no links")
                .expect("write new note");

            let (_, report) =
                indexer.refresh_with_report().expect("refresh index");
            assert_eq!(report.upserted_count(), 1);
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
            let indexer = IndexerService::for_tests(temp.path());
            indexer
                .persist(&indexer.build().expect("build index"))
                .expect("persist index");

            fs::write(temp.path().join("note.md"), "- [ ] task\n- [x] second")
                .expect("rewrite note");

            let refreshed = indexer.refresh().expect("refresh index");
            assert_eq!(
                find_note(&refreshed, "note.md")
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
            let indexer = IndexerService::for_tests(temp.path());
            indexer
                .persist(&indexer.build().expect("build index"))
                .expect("persist index");

            std::thread::sleep(std::time::Duration::from_millis(15));
            fs::write(temp.path().join("note.md"), "Status:: Final")
                .expect("rewrite note with same byte length");

            let refreshed = indexer.refresh().expect("refresh index");
            let value = find_note(&refreshed, "note.md")
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
            let indexer = IndexerService::for_tests(temp.path());
            indexer
                .persist(&indexer.build().expect("build index"))
                .expect("persist index");

            fs::write(temp.path().join("second.md"), "# Second")
                .expect("write second");

            let refreshed = indexer.refresh().expect("refresh index");
            assert_eq!(
                refreshed
                    .entries()
                    .iter()
                    .filter(|entry| entry.note().is_some())
                    .count(),
                2
            );
        }

        #[test]
        fn excludes_deleted_note() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("gone.md"), "# Gone")
                .expect("write note");
            let indexer = IndexerService::for_tests(temp.path());
            indexer
                .persist(&indexer.build().expect("build index"))
                .expect("persist index");

            fs::remove_file(temp.path().join("gone.md")).expect("delete note");

            let refreshed = indexer.refresh().expect("refresh index");
            assert_eq!(
                refreshed
                    .entries()
                    .iter()
                    .filter(|entry| entry.note().is_some())
                    .count(),
                0
            );
            assert_eq!(refreshed.entries().len(), 0);
        }

        #[test]
        fn excludes_inlink_after_linker_deletion() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("target.md"), "# Target")
                .expect("write target");
            fs::write(temp.path().join("linker.md"), "[[target]]")
                .expect("write linker");
            let indexer = IndexerService::for_tests(temp.path());
            indexer
                .persist(&indexer.build().expect("build index"))
                .expect("persist index");

            fs::remove_file(temp.path().join("linker.md"))
                .expect("delete linker");

            let refreshed = Arc::new(indexer.refresh().expect("refresh index"));
            let pages = query_pages(&refreshed, &SourceSelector::All);
            let target = pages.iter().next().expect("target entry");

            assert_eq!(target.file().path(), Path::new("target.md"));
            assert_eq!(target.inlinks(), Vec::<std::path::PathBuf>::new());
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
            let indexer = IndexerService::for_tests(temp.path());
            indexer
                .persist(&indexer.build().expect("build index"))
                .expect("persist index");

            fs::write(temp.path().join("linker.md"), "[[new-target]]")
                .expect("repoint linker");

            let refreshed = Arc::new(indexer.refresh().expect("refresh index"));
            let pages = query_pages(&refreshed, &SourceSelector::All);
            let old_target = pages
                .iter()
                .find(|entry| entry.file().path() == Path::new("old-target.md"))
                .expect("old target entry");
            let new_target = pages
                .iter()
                .find(|entry| entry.file().path() == Path::new("new-target.md"))
                .expect("new target entry");

            assert_eq!(old_target.inlinks(), Vec::<std::path::PathBuf>::new());
            assert_eq!(new_target.inlinks(), [PathBuf::from("linker.md")]);
        }

        #[test]
        fn content_only_refresh_keeps_untouched_notes_in_the_returned_index() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let root = temp.path();
            fs::write(root.join("target.md"), "---\ntitle: Keep\n---")
                .expect("write target");
            fs::write(root.join("linker.md"), "- [ ] first")
                .expect("write linker");
            let indexer = IndexerService::for_tests(root);
            indexer
                .persist(&indexer.build().expect("build index"))
                .expect("persist index");

            fs::write(root.join("linker.md"), "- [ ] second")
                .expect("edit linker only");

            let refreshed = indexer.refresh().expect("refresh index");

            assert_eq!(
                find_note(&refreshed, "target.md")
                    .and_then(Note::frontmatter)
                    .map(|fm| fm.fields().len()),
                Some(1)
            );
        }

        #[test]
        fn persists_refreshed_changes_survive_load() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("note.md"), "# Draft")
                .expect("write note");
            let indexer = IndexerService::for_tests(temp.path());
            indexer
                .persist(&indexer.build().expect("build index"))
                .expect("persist index");

            fs::write(temp.path().join("extra.md"), "# Extra")
                .expect("write extra");
            indexer
                .persist(&indexer.refresh().expect("refresh index"))
                .expect("persist index");

            let loaded = indexer.load().expect("load index");
            assert_eq!(
                loaded
                    .entries()
                    .iter()
                    .filter(|entry| entry.note().is_some())
                    .count(),
                2
            );
        }

        #[test]
        fn builds_an_index_when_nothing_was_persisted_yet() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("note.md"), "# Note")
                .expect("write note");

            let refreshed = IndexerService::for_tests(temp.path())
                .refresh()
                .expect("refresh index");
            assert_eq!(
                refreshed
                    .entries()
                    .iter()
                    .filter(|entry| entry.note().is_some())
                    .count(),
                1
            );
        }

        #[test]
        fn preserves_inlinks_after_noop_refresh() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("target.md"), "# Target")
                .expect("write target");
            fs::write(temp.path().join("linker.md"), "[[target]]")
                .expect("write linker");
            let indexer = IndexerService::for_tests(temp.path());
            indexer
                .persist(&indexer.build().expect("build index"))
                .expect("persist index");

            let refreshed = Arc::new(indexer.refresh().expect("refresh index"));
            let pages = query_pages(&refreshed, &SourceSelector::All);
            let target = pages
                .iter()
                .find(|entry| entry.file().path() == Path::new("target.md"))
                .expect("target entry");

            assert_eq!(target.inlinks(), [PathBuf::from("linker.md")]);
        }

        #[test]
        fn refresh_persists_so_a_fresh_load_reflects_the_change() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("note.md"), "---\ntitle: Draft\n---")
                .expect("write note");
            let indexer = IndexerService::for_tests(temp.path());
            indexer
                .persist(&indexer.build().expect("build index"))
                .expect("persist index");

            fs::write(temp.path().join("note.md"), "# Revised")
                .expect("rewrite note");

            let refreshed = indexer.refresh().expect("refresh index");

            assert_eq!(
                find_note(&refreshed, "note.md")
                    .and_then(Note::frontmatter)
                    .and_then(|fm| fm.fields().values().next())
                    .and_then(|v| v.as_str()),
                None
            );
            // No separate `persist` call precedes this load.
            let loaded = indexer.load().expect("load index");
            assert_eq!(
                find_note(&loaded, "note.md")
                    .and_then(Note::frontmatter)
                    .and_then(|fm| fm.fields().values().next())
                    .and_then(|v| v.as_str()),
                None
            );
        }

        #[test]
        fn resolves_stale_ambiguous_wikilink_after_unrelated_deletion() {
            // `a.md` is never reparsed: deleting one `foo.md` changes an
            // ambiguous `[[foo]]` into a resolvable link, so `refresh` must
            // full-recompute inlinks when indexed paths change.
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::create_dir_all(temp.path().join("notes")).expect("mkdir notes");
            fs::create_dir_all(temp.path().join("archive"))
                .expect("mkdir archive");
            fs::write(temp.path().join("notes/foo.md"), "# Foo")
                .expect("write notes/foo.md");
            fs::write(temp.path().join("archive/foo.md"), "# Old Foo")
                .expect("write archive/foo.md");
            fs::write(temp.path().join("a.md"), "[[foo]]").expect("write a");
            let indexer = IndexerService::for_tests(temp.path());
            indexer
                .persist(&indexer.build().expect("build index"))
                .expect("persist index");

            fs::remove_file(temp.path().join("archive/foo.md"))
                .expect("delete archive/foo.md");

            let refreshed = Arc::new(indexer.refresh().expect("refresh index"));
            let pages = query_pages(&refreshed, &SourceSelector::All);
            let target = pages
                .iter()
                .find(|entry| entry.file().path() == Path::new("notes/foo.md"))
                .expect("notes/foo.md entry");

            assert_eq!(target.inlinks(), [PathBuf::from("a.md")]);
        }
    }

    mod class_field {
        use pretty_assertions::assert_eq;
        use rstest::rstest;

        use super::*;
        use crate::{
            config::{Config, SchemasConfig},
            index::store::IndexStore,
        };

        #[test]
        fn indexes_file_class_under_the_configured_field() {
            // Arrange
            let temp = tempfile::tempdir().expect("create temp dir");
            let root = temp.path();
            fs::write(
                root.join("a.md"),
                "---\nkind: real\nfileClass: Legacy\n---\n# A",
            )
            .expect("write a");
            let config = Config::test_default(root.to_path_buf())
                .with_schemas(SchemasConfig::for_test("kind"));
            let service = IndexerService::from(&config);
            let index = service.build().expect("build index");

            // Act
            service.persist(&index).expect("persist index");

            // Assert
            let store = IndexStore::open(root).expect("open store");
            let real = store.paths_with_file_class("real").expect("read real");
            assert_eq!(real.as_ref(), [PathBuf::from("a.md")]);
            let legacy =
                store.paths_with_file_class("legacy").expect("read legacy");
            assert_eq!(legacy.as_ref(), <&[PathBuf]>::default());
        }

        #[test]
        fn indexes_class_frontmatter_under_default_config() {
            // Arrange
            let temp = tempfile::tempdir().expect("create temp dir");
            let root = temp.path();
            fs::write(root.join("a.md"), "---\nclass: Book\n---\n# A")
                .expect("write a");
            let service = IndexerService::for_tests(root);
            let index = service.build().expect("build index");

            // Act
            service.persist(&index).expect("persist index");

            // Assert
            let store = IndexStore::open(root).expect("open store");
            let paths =
                store.paths_with_file_class("book").expect("read class");
            assert_eq!(paths.as_ref(), [PathBuf::from("a.md")]);
        }

        #[test]
        fn rederives_class_under_configured_field_on_incremental_refresh() {
            // Arrange
            let temp = tempfile::tempdir().expect("create temp dir");
            let root = temp.path();
            fs::write(root.join("a.md"), "---\nkind: book\n---\n# A")
                .expect("write a");
            fs::write(root.join("b.md"), "---\nkind: movie\n---\n# B")
                .expect("write b");
            let config = Config::test_default(root.to_path_buf())
                .with_schemas(SchemasConfig::for_test("kind"));
            let service = IndexerService::from(&config);
            let index = service.build().expect("build index");
            service.persist(&index).expect("persist index");

            // Act: rewrite b.md to new class and run incremental refresh
            fs::write(root.join("b.md"), "---\nkind: novel\n---\n# B")
                .expect("rewrite b");
            let (_refreshed_index, _report) =
                service.refresh_with_report().expect("refresh with report");

            // Assert: store reflected the incremental upsert and removal
            let store = IndexStore::open(root).expect("open store");
            let movie =
                store.paths_with_file_class("movie").expect("read movie");
            assert_eq!(movie.as_ref(), <&[PathBuf]>::default());
            let novel =
                store.paths_with_file_class("novel").expect("read novel");
            assert_eq!(novel.as_ref(), [PathBuf::from("b.md")]);
            let book = store.paths_with_file_class("book").expect("read book");
            assert_eq!(book.as_ref(), [PathBuf::from("a.md")]);
        }

        #[rstest]
        #[case::file_class("fileClass")]
        #[case::file_class_snake("file_class")]
        #[case::classes_plural("classes")]
        fn rejects_obsolete_alias_keys(#[case] key: &str) {
            let temp = tempfile::tempdir().expect("create temp dir");
            let root = temp.path();
            fs::write(root.join("a.md"), format!("---\n{key}: Book\n---\n# A"))
                .expect("write a");
            let service = IndexerService::for_tests(root);
            let index = service.build().expect("build index");
            service.persist(&index).expect("persist index");

            let store = IndexStore::open(root).expect("open store");
            let paths =
                store.paths_with_file_class("book").expect("read class");
            assert_eq!(paths.as_ref(), <&[PathBuf]>::default());
        }

        #[test]
        fn indexes_multiple_classes_under_configured_field() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let root = temp.path();
            fs::write(
                root.join("a.md"),
                "---\nkind:\n  - Book\n  - Novel\n---\n# A",
            )
            .expect("write a");
            let config = Config::test_default(root.to_path_buf())
                .with_schemas(SchemasConfig::for_test("kind"));
            let service = IndexerService::from(&config);
            let index = service.build().expect("build index");
            service.persist(&index).expect("persist index");

            let store = IndexStore::open(root).expect("open store");
            let book = store.paths_with_file_class("book").expect("read book");
            assert_eq!(book.as_ref(), [PathBuf::from("a.md")]);
            let novel =
                store.paths_with_file_class("novel").expect("read novel");
            assert_eq!(novel.as_ref(), [PathBuf::from("a.md")]);
        }

        #[test]
        fn matches_configured_field_key_case_insensitively() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let root = temp.path();
            fs::write(root.join("a.md"), "---\nKIND: Book\n---\n# A")
                .expect("write a");
            let config = Config::test_default(root.to_path_buf())
                .with_schemas(SchemasConfig::for_test("kind"));
            let service = IndexerService::from(&config);
            let index = service.build().expect("build index");
            service.persist(&index).expect("persist index");

            let store = IndexStore::open(root).expect("open store");
            let book = store.paths_with_file_class("book").expect("read book");
            assert_eq!(book.as_ref(), [PathBuf::from("a.md")]);
        }
    }
}
