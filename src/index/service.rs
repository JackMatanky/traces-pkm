//! Index lifecycle service.
//!
//! [`IndexerService`] scans, parses, persists, loads, refreshes, and syncs one
//! project root's [`super::FileIndex`] through `IndexStore`.
//!
//! `refresh` and `sync` share one incremental core: content-only deltas patch
//! inbound links from touched notes, while path-set changes force a full
//! recompute because wikilink resolution depends on every indexed path.

use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};

use rayon::prelude::*;

use super::{
    FileIndex, INDEX_FILE, IndexError, IndexResult,
    delta::{IndexDelta, InlinkDelta},
    entry::{self, ListEntry},
    inlinks::{self, InlinkMap},
    store::IndexStore,
};
use crate::{
    Config, DirTree, Note, TaskConfig,
    config::FrontmatterConfig,
    file::FileBase,
    note::{MarkdownParserInput, parse_markdown},
};

/// Changed-row counts from an incremental synchronization.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub struct SyncReport {
    upserted: usize,
    deleted: usize,
    links_modified: usize,
}

impl SyncReport {
    /// Creates a report from changed-row counts.
    #[inline]
    #[must_use]
    pub const fn new(
        upserted: usize,
        deleted: usize,
        links_modified: usize,
    ) -> Self {
        Self {
            upserted,
            deleted,
            links_modified,
        }
    }

    /// Files inserted or updated.
    #[inline]
    #[must_use]
    pub const fn upserted(self) -> usize {
        self.upserted
    }

    /// Files deleted from the index.
    #[inline]
    #[must_use]
    pub const fn deleted(self) -> usize {
        self.deleted
    }

    /// Inbound link edges updated.
    #[inline]
    #[must_use]
    pub const fn links_modified(self) -> usize {
        self.links_modified
    }
}

/// State passed across one incremental refresh/sync pipeline.
struct RefreshContext<'a> {
    store: &'a IndexStore,
    current_files: Vec<FileBase>,
    persisted_files: &'a [FileBase],
    prev_links: &'a InlinkMap,
}

/// Inbound-link graph, link diff, and optional merged notes from a sync pass.
struct SyncOutcome {
    current_links: InlinkMap,
    inlink_delta: InlinkDelta,
    all_notes: Option<Vec<Note>>,
}

/// Drives the file-index lifecycle for one project root.
///
/// `refresh` returns a full [`FileIndex`] and logs persist failures; `sync`
/// keeps only the persisted store current and propagates persist failures
/// because it has no in-memory fallback.
#[derive(Clone, Debug)]
pub struct IndexerService {
    root: PathBuf,
    tasks: TaskConfig,
    frontmatter: FrontmatterConfig,
}

impl IndexerService {
    /// Creates a service scoped to `root`.
    #[inline]
    #[must_use]
    pub fn new<P: Into<PathBuf>>(root: P) -> Self {
        Self {
            root: root.into(),
            tasks: TaskConfig::default(),
            frontmatter: FrontmatterConfig::default(),
        }
    }

    /// Attaches resolved [`Config`] settings for task and frontmatter
    /// classification.
    #[inline]
    #[must_use]
    pub fn with_config(mut self, config: &Config) -> Self {
        self.tasks = config.tasks().clone();
        self.frontmatter = config.frontmatter().clone();
        self
    }

    /// Scans this service's root and builds a [`FileIndex`] in memory.
    ///
    /// # Errors
    ///
    /// - `IndexError::Walk` if a directory cannot be read.
    /// - `IndexError::NoteParse` if a file's metadata cannot be inspected, or a
    ///   Markdown file cannot be parsed.
    #[inline]
    pub fn build(&self) -> IndexResult<FileIndex> {
        let files = self.scan()?;
        let notes = self.parse_notes(&files)?;
        let inlinks = InlinkMap::new(&notes, &files);
        Ok(FileIndex::assemble(files, notes, inlinks))
    }

    /// Refreshes the persisted index and returns a full in-memory
    /// [`FileIndex`].
    ///
    /// Unchanged Markdown notes reuse persisted parses; changed notes are
    /// parsed from disk; deleted files vanish with the fresh scan. Persist
    /// failures are logged and the refreshed in-memory index is still
    /// returned.
    ///
    /// Inlinks are patched only for content-only deltas. Any added, deleted, or
    /// renamed path forces a full recompute because an unedited note's resolved
    /// wikilink target can change when candidate paths change.
    ///
    /// # Errors
    ///
    /// - `IndexError::Walk` if a directory cannot be read.
    /// - `IndexError::NoteParse` if file metadata cannot be inspected, a
    ///   Markdown file cannot be parsed, or an unchanged note cannot be
    ///   recalled.
    /// - `IndexError::Store` if the persisted index cannot be opened or read.
    #[inline]
    pub fn refresh(&self) -> IndexResult<FileIndex> {
        let (index, _) = self.refresh_with_report()?;
        Ok(index)
    }

    /// Refreshes the persisted index and returns changed-row counts.
    ///
    /// # Errors
    ///
    /// - `IndexError::Walk` if a directory cannot be read.
    /// - `IndexError::NoteParse` if file metadata cannot be inspected, a
    ///   Markdown file cannot be parsed, or an unchanged note cannot be
    ///   recalled.
    /// - `IndexError::Store` if the persisted index cannot be opened or read.
    #[inline]
    pub fn refresh_with_report(&self) -> IndexResult<(FileIndex, SyncReport)> {
        let (store, current_files, persisted_files, prev_links) =
            self.open_scan_and_read_persisted()?;
        let delta = IndexDelta::compute(&current_files, &persisted_files);

        if delta.is_empty() {
            let (files, notes, inlinks) = store.read_all()?;
            let index = FileIndex::assemble(files, notes, inlinks);
            return Ok((index, SyncReport::default()));
        }

        let ctx = RefreshContext {
            store: &store,
            current_files,
            persisted_files: &persisted_files,
            prev_links: &prev_links,
        };
        self.assemble_refreshed_index(ctx, &delta)
    }

    /// Synchronizes the persisted index without materializing a [`FileIndex`].
    ///
    /// Cold empty deltas return the opened store without decoding notes or
    /// writing rows. Persist failures are propagated because callers read
    /// directly from the returned store.
    ///
    /// # Errors
    ///
    /// - `IndexError::Walk` if a directory cannot be read.
    /// - `IndexError::NoteParse` if file metadata cannot be inspected, a
    ///   Markdown file cannot be parsed, or an unchanged note cannot be
    ///   recalled.
    /// - `IndexError::Store` if the database cannot be opened or read, required
    ///   note bodies cannot be read, or incremental persistence fails.
    #[inline]
    pub(crate) fn sync(&self) -> IndexResult<IndexStore> {
        let (store, current_files, persisted_files, prev_links) =
            self.open_scan_and_read_persisted()?;
        let delta = IndexDelta::compute(&current_files, &persisted_files);
        if delta.is_empty() {
            return Ok(store);
        }

        let modified_notes = self.parse_notes(delta.upserted())?;
        let ctx = RefreshContext {
            store: &store,
            current_files,
            persisted_files: &persisted_files,
            prev_links: &prev_links,
        };
        let outcome =
            Self::compute_sync_outcome(&ctx, &delta, &modified_notes)?;
        store.persist_incremental(
            &delta,
            &modified_notes,
            &outcome.inlink_delta,
        )?;
        Self::log_sync(&delta, &outcome.inlink_delta);
        Ok(store)
    }

    /// Opens `IndexStore`, scans the filesystem, and reads persisted files and
    /// inlinks.
    ///
    /// The filesystem walk runs concurrently with `IndexStore::open` and the
    /// store read. Only the read waits on open; the scan continues
    /// independently.
    ///
    /// # Errors
    ///
    /// - `IndexError::Walk` if a directory cannot be read.
    /// - `IndexError::NoteParse` if file metadata cannot be inspected.
    /// - `IndexError::Store` if opening or reading the store fails.
    #[allow(
        clippy::type_complexity,
        reason = "pre-existing tuple in return type"
    )]
    fn open_scan_and_read_persisted(
        &self,
    ) -> IndexResult<(IndexStore, Vec<FileBase>, Vec<FileBase>, InlinkMap)>
    {
        let (opened, scanned) = rayon::join(
            || -> IndexResult<_> {
                let store = IndexStore::open(&self.root)?;
                let (persisted_files, prev_links) =
                    store.read_files_and_links()?;
                Ok((store, persisted_files, prev_links))
            },
            || self.scan(),
        );
        let (store, persisted_files, prev_links) = opened?;
        let current_files = scanned?;
        Ok((store, current_files, persisted_files, prev_links))
    }

    fn assemble_refreshed_index(
        &self,
        ctx: RefreshContext<'_>,
        delta: &IndexDelta,
    ) -> IndexResult<(FileIndex, SyncReport)> {
        let modified_notes = self.parse_notes(delta.upserted())?;
        let outcome = Self::compute_sync_outcome(&ctx, delta, &modified_notes)?;

        if let Err(source) = ctx.store.persist_incremental(
            delta,
            &modified_notes,
            &outcome.inlink_delta,
        ) {
            tracing::warn!(%source, "failed to persist refreshed index");
        } else {
            Self::log_sync(delta, &outcome.inlink_delta);
        }

        let report = SyncReport::new(
            delta.upserted().len(),
            delta.deleted().len(),
            outcome
                .inlink_delta
                .upserted()
                .len()
                .saturating_add(outcome.inlink_delta.deleted().len()),
        );
        let all_notes = match outcome.all_notes {
            Some(notes) => notes,
            None => {
                Self::merge_refreshed_notes(ctx.store, delta, &modified_notes)?
            }
        };
        let index = FileIndex::assemble(
            ctx.current_files,
            all_notes,
            outcome.current_links,
        );
        Ok((index, report))
    }

    /// Computes fresh inlinks and their diff for one non-empty `delta`.
    ///
    /// Content-only deltas patch from `modified_notes`; any path-set change
    /// does a full recompute and returns merged notes for reuse. Link
    /// resolution uses every indexed path and stem, so unedited notes can
    /// retarget when unrelated candidate paths change.
    ///
    /// # Errors
    ///
    /// - `IndexError::Store` if the full-recompute fallback cannot read every
    ///   persisted note body.
    fn compute_sync_outcome(
        ctx: &RefreshContext<'_>,
        delta: &IndexDelta,
        modified_notes: &[Note],
    ) -> IndexResult<SyncOutcome> {
        let store = ctx.store;
        let current_files = &ctx.current_files;
        let persisted_files = ctx.persisted_files;
        let prev_links = ctx.prev_links;
        let (current_links, all_notes) =
            if Self::is_paths_unchanged(delta, persisted_files) {
                let links = Self::patch_links(
                    prev_links,
                    modified_notes,
                    current_files,
                );
                (links, None)
            } else {
                let all_notes =
                    Self::merge_refreshed_notes(store, delta, modified_notes)?;
                let links = InlinkMap::new(&all_notes, current_files);
                (links, Some(all_notes))
            };
        let inlink_delta = InlinkDelta::compute(&current_links, prev_links);
        Ok(SyncOutcome {
            current_links,
            inlink_delta,
            all_notes,
        })
    }

    /// Reports whether `delta` changed only content at previously indexed
    /// paths.
    ///
    /// Only this case leaves link resolution for unedited notes invariant.
    fn is_paths_unchanged(
        delta: &IndexDelta,
        persisted_files: &[FileBase],
    ) -> bool {
        delta.deleted().is_empty()
            && delta.upserted().iter().all(|file| {
                persisted_files
                    .binary_search_by(|p| p.path().cmp(file.path()))
                    .is_ok()
            })
    }

    /// Replaces inbound edges sourced by modified notes after re-resolving
    /// their current outlinks.
    ///
    /// Sound only when [`Self::is_paths_unchanged`] holds.
    fn patch_links(
        prev_links: &InlinkMap,
        modified_notes: &[Note],
        current_files: &[FileBase],
    ) -> InlinkMap {
        let edited: HashSet<&Path> =
            modified_notes.iter().map(Note::path).collect();
        let new_edges =
            inlinks::resolve_edges_for(modified_notes, current_files);
        prev_links.without_sources(&edited).with_edges(new_edges)
    }

    fn log_sync(delta: &IndexDelta, inlink_delta: &InlinkDelta) {
        let report = SyncReport::new(
            delta.upserted().len(),
            delta.deleted().len(),
            inlink_delta
                .upserted()
                .len()
                .saturating_add(inlink_delta.deleted().len()),
        );
        tracing::debug!(
            upserted = report.upserted(),
            deleted = report.deleted(),
            links_modified = report.links_modified(),
            "index synced"
        );
    }

    /// Merges persisted notes with deletions and reparsed notes for a full
    /// recompute.
    ///
    /// Bulk-reads all persisted notes because this fallback needs the full note
    /// set. Path-indexed deletes and replacements avoid an `O((deleted +
    /// modified) * n)` scan over the persisted note count `n`.
    fn merge_refreshed_notes(
        store: &IndexStore,
        delta: &IndexDelta,
        modified_notes: &[crate::Note],
    ) -> IndexResult<Vec<crate::Note>> {
        let mut all_notes = store.read_all_notes()?;

        if !delta.deleted().is_empty() {
            let deleted: HashSet<&Path> =
                delta.deleted().iter().map(FileBase::path).collect();
            all_notes.retain(|n| !deleted.contains(n.path()));
        }

        let mut index_of_path: HashMap<PathBuf, usize> = all_notes
            .iter()
            .enumerate()
            .map(|(idx, note)| (note.path().to_path_buf(), idx))
            .collect();
        for new_note in modified_notes {
            if let Some(&idx) = index_of_path.get(new_note.path())
                && let Some(target) = all_notes.get_mut(idx)
            {
                *target = new_note.clone();
            } else {
                index_of_path
                    .insert(new_note.path().to_path_buf(), all_notes.len());
                all_notes.push(new_note.clone());
            }
        }
        all_notes.sort_by(|a, b| a.path().cmp(b.path()));
        Ok(all_notes)
    }

    /// Parses every Markdown-classified file in `files` in parallel, stopping
    /// at the first parse failure.
    fn parse_notes(&self, files: &[FileBase]) -> IndexResult<Vec<crate::Note>> {
        let note_files: Vec<&FileBase> = files
            .iter()
            .filter(|f| f.format() == crate::file::FileFormat::Note)
            .collect();
        let results: Vec<IndexResult<crate::Note>> = note_files
            .into_par_iter()
            .map(|file| self.parse_note(file))
            .collect();
        let mut notes = Vec::with_capacity(results.len());
        for res in results {
            notes.push(res?);
        }
        Ok(notes)
    }

    /// Reads and parses the Markdown file at `file`'s path, resolved relative
    /// to this service's root.
    fn parse_note(&self, file: &FileBase) -> IndexResult<crate::Note> {
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
    ///   created, the transaction fails, or a record cannot be encoded.
    #[inline]
    pub fn persist(&self, index: &FileIndex) -> IndexResult<()> {
        IndexStore::open(&self.root)?.persist_index(index)
    }

    /// Loads the index previously persisted for this service's root, or an
    /// empty [`FileIndex`] if none exists.
    ///
    /// # Errors
    ///
    /// - `IndexError::Store` if the database cannot be read or stored bytes are
    ///   not a valid record.
    #[inline]
    #[cfg_attr(
        not(any(test, feature = "test-utils")),
        expect(
            dead_code,
            reason = "no current caller in cli; kept for IndexerService \
                      lifecycle symmetry and tests"
        )
    )]
    pub fn load(&self) -> IndexResult<FileIndex> {
        let (files, notes, inlinks) =
            IndexStore::open(&self.root)?.read_all()?;
        Ok(FileIndex::new(entry::assemble_entries(files, notes, inlinks)))
    }

    /// Reads all persisted [`ListEntry`]s from the `LISTS` table.
    ///
    /// # Errors
    ///
    /// - `IndexError::Store` if the database cannot be opened or read.
    #[cfg_attr(
        not(any(test, feature = "test-utils")),
        expect(
            dead_code,
            reason = "consumed by task queries added in issue 08"
        )
    )]
    #[inline]
    pub fn read_lists(&self) -> IndexResult<Vec<ListEntry>> {
        let store = IndexStore::open(&self.root)?;
        Ok(store.read_all_lists()?)
    }

    /// Recursively scans this service's root for regular files, skipping `.git`
    /// directories, the index database, and symlinks. Metadata reads run in
    /// parallel via `rayon` because each is an independent syscall; results are
    /// path-sorted.
    ///
    /// # Errors
    ///
    /// - [`IndexError::Walk`] if a directory cannot be read.
    /// - [`IndexError::NoteParse`] if a file's metadata cannot be inspected.
    #[inline]
    pub(super) fn scan(&self) -> IndexResult<Vec<FileBase>> {
        let index_db = self.root.join(INDEX_FILE);
        let paths = DirTree::descendants(&self.root)
            .filter(|node| {
                node.file_name() != ".traces"
                    && crate::env_vars::is_ignored_dir(node.file_name())
            })
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
            .map(|path| scan_file_metadata(&path, &self.root))
            .collect::<IndexResult<Vec<FileBase>>>()?;
        files.sort_by(|a, b| a.path().cmp(b.path()));
        Ok(files)
    }
}

/// Builds `path`'s [`FileBase`] from metadata relative to `root`.
fn scan_file_metadata(path: &Path, root: &Path) -> IndexResult<FileBase> {
    let metadata =
        std::fs::metadata(path).map_err(|source| IndexError::NoteParse {
            path: path.to_path_buf(),
            source,
        })?;
    FileBase::from_metadata(path, root, &metadata).map_err(|source| {
        IndexError::NoteParse {
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
        index: &Arc<FileIndex>,
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

        let index =
            IndexerService::new(temp.path()).build().expect("build index");
        let paths: Vec<_> = index
            .entries()
            .iter()
            .map(|e| e.file().path().to_str().unwrap())
            .collect();
        assert_eq!(paths, ["notes/note.md", "notes-x.md"]);
    }

    #[test]
    fn produces_identical_index_through_parallel_and_serial_rebuilds() {
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

        let service = IndexerService::new(temp.path());
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
    fn preserves_deterministic_first_error_on_parallel_parse_failure() {
        use std::os::unix::fs::PermissionsExt;
        let temp = tempfile::tempdir().expect("create temp dir");
        let a_dir = temp.path().join("a");
        let z_dir = temp.path().join("z");
        fs::create_dir_all(&a_dir).expect("mkdir a");
        fs::create_dir_all(&z_dir).expect("mkdir z");
        let bad_a = a_dir.join("bad.md");
        let bad_z = z_dir.join("bad.md");
        fs::write(&bad_a, "# Bad A\n").expect("write bad_a");
        fs::write(&bad_z, "# Bad Z\n").expect("write bad_z");
        fs::set_permissions(&bad_a, fs::Permissions::from_mode(0o000))
            .expect("chmod bad_a");
        fs::set_permissions(&bad_z, fs::Permissions::from_mode(0o000))
            .expect("chmod bad_z");

        let result = IndexerService::new(temp.path()).build();
        let err = result.expect_err("must fail on unreadable file");
        let IndexError::NoteParse {
            path,
            ..
        } = err
        else {
            return;
        };
        assert_eq!(path, bad_a);

        fs::set_permissions(&bad_a, fs::Permissions::from_mode(0o600))
            .expect("restore bad_a");
        fs::set_permissions(&bad_z, fs::Permissions::from_mode(0o600))
            .expect("restore bad_z");
    }

    fn find_note<'a>(index: &'a FileIndex, path: &str) -> Option<&'a Note> {
        index
            .entries()
            .iter()
            .find(|entry| entry.file().path() == Path::new(path))
            .and_then(FileEntry::note)
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

            let index =
                IndexerService::new(temp.path()).build().expect("build index");

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

            let index =
                IndexerService::new(temp.path()).build().expect("build index");

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

            assert!(matches!(result, Err(IndexError::NoteParse { .. })));
        }

        #[test]
        fn sorts_indexed_notes_by_path() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("b.md"), "# B").expect("write b");
            fs::write(temp.path().join("a.md"), "# A").expect("write a");
            let index =
                IndexerService::new(temp.path()).build().expect("build index");

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
        use crate::index::tests::fixtures::RestorePermissions;

        fn names(files: &[FileBase]) -> Vec<&Path> {
            files.iter().map(FileBase::path).collect()
        }

        #[test]
        fn scans_nested_files_in_sorted_order() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let root = temp.path();
            fs::create_dir_all(root.join("b")).expect("mkdir b");
            fs::write(root.join("b/one.md"), "1").expect("write b/one.md");
            fs::write(root.join("a.md"), "2").expect("write a.md");

            let files = IndexerService::new(root).scan().expect("scan");

            assert_eq!(names(&files), vec![
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

            let files = IndexerService::new(root).scan().expect("scan");

            assert_eq!(names(&files), vec![
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

            let files = IndexerService::new(root).scan().expect("scan");

            assert_eq!(names(&files), vec![Path::new("note.md")]);
        }

        #[test]
        fn skips_its_own_index_database_file() {
            let temp = tempfile::tempdir().expect("create temp dir");
            let root = temp.path();
            fs::create_dir_all(root.join(".traces")).expect("mkdir .traces");
            fs::write(root.join(INDEX_FILE), b"redb-bytes")
                .expect("write index db");
            fs::write(root.join("note.md"), "content").expect("write note.md");

            let files = IndexerService::new(root).scan().expect("scan");

            assert_eq!(names(&files), vec![Path::new("note.md")]);
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

            let files = IndexerService::new(root).scan().expect("scan");

            assert_eq!(names(&files), vec![Path::new("note.md")]);
        }

        #[test]
        fn empty_root_yields_no_records() {
            let temp = tempfile::tempdir().expect("create temp dir");

            let files = IndexerService::new(temp.path()).scan().expect("scan");

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
            let _restore = RestorePermissions(&locked);

            let error = IndexerService::new(root)
                .scan()
                .expect_err("unreadable dir fails");

            assert!(matches!(error, IndexError::Walk(_)));
        }
    }

    mod persistence {
        use pretty_assertions::assert_eq;
        use rstest::rstest;

        use super::*;
        use crate::{
            Tag,
            note::{Frontmatter, Link, LinkType, NoteFieldValue},
        };

        /// Seeds three notes and returns the service plus untouched `a`/`c`
        /// baselines.
        fn seed_three_notes(root: &Path) -> (IndexerService, Note, Note) {
            fs::write(root.join("a.md"), "---\ntitle: A\n---\nBody A.")
                .expect("write a");
            fs::write(root.join("b.md"), "---\ntitle: B\n---\nBody B.")
                .expect("write b");
            fs::write(root.join("c.md"), "---\ntitle: C\n---\nBody C.")
                .expect("write c");
            let indexer = IndexerService::new(root);
            let built = indexer.build().expect("build index");
            indexer.persist(&built).expect("persist index");
            let a = find_note(&built, "a.md").expect("note a").clone();
            let c = find_note(&built, "c.md").expect("note c").clone();
            (indexer, a, c)
        }

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

            assert_eq!(loaded.entries(), built.entries());
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
            let indexer = IndexerService::new(temp.path());
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
            let indexer = IndexerService::new(temp.path());
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
            let indexer = IndexerService::new(temp.path());
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
            let indexer = IndexerService::new(temp.path());
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
                &NoteFieldValue::Duration("7 hours".to_owned()),
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
            let indexer = IndexerService::new(temp.path());
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

            let index =
                IndexerService::new(temp.path()).load().expect("load index");

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
            let indexer = IndexerService::new(temp.path());
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

            assert_eq!(report.upserted, 1);
            assert_eq!(report.deleted, 0);
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
        fn persist_incremental_preserves_unchanged_notes_and_updates_changed_note()
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

            assert_eq!(report, SyncReport::default());
        }

        #[test]
        fn refresh_after_corruption_recovery_reports_every_file_upserted_and_nothing_deleted()
         {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("a.md"), "# A").expect("write a");
            fs::write(temp.path().join("b.md"), "# B").expect("write b");
            let indexer = IndexerService::new(temp.path());
            indexer
                .persist(&indexer.build().expect("build index"))
                .expect("persist index");

            let db_path = temp.path().join(".traces/index.redb");
            let mut corrupted = fs::read(&db_path).expect("read valid db");
            corrupted
                .get_mut(9..)
                .expect("db file longer than the 9-byte magic number")
                .fill(0xFF);
            fs::write(&db_path, &corrupted).expect("corrupt the database file");

            let (_, report) = indexer
                .refresh_with_report()
                .expect("refresh recovers from corruption");

            assert_eq!(report.upserted, 2);
            assert_eq!(report.deleted, 0);

            let store = IndexStore::open(temp.path()).expect("open store");
            let metadata = store.load_file_metadata().expect("load metadata");
            assert_eq!(metadata.len(), 2);
        }

        #[test]
        fn content_change_without_outlink_change_backdates_and_skips_inlink_recompute()
         {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("target.md"), "# Target")
                .expect("write target");
            fs::write(temp.path().join("linker.md"), "[[target]]\n- [ ] task")
                .expect("write linker");
            let indexer = IndexerService::new(temp.path());
            indexer
                .persist(&indexer.build().expect("build index"))
                .expect("persist index");

            fs::write(temp.path().join("linker.md"), "[[target]]\n- [x] task")
                .expect("rewrite linker: task checked, same outlink");

            let (_, report) =
                indexer.refresh_with_report().expect("refresh index");

            assert_eq!(report.upserted, 1);
            assert_eq!(report.links_modified, 0);
        }

        #[test]
        fn outlink_change_is_not_backdated_and_recomputes_inlinks() {
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
                .expect("retarget linker");

            let (_, report) =
                indexer.refresh_with_report().expect("refresh index");
            assert_eq!(report.upserted, 1);
            assert!(report.links_modified > 0);

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
            let indexer = IndexerService::new(temp.path());
            indexer
                .persist(&indexer.build().expect("build index"))
                .expect("persist index");

            fs::write(temp.path().join("linker.md"), "[[b|Bee]]\n[[a]]")
                .expect("reorder links and relabel display text, same targets");

            let (_, report) =
                indexer.refresh_with_report().expect("refresh index");
            assert_eq!(report.links_modified, 0);
        }

        #[test]
        fn brand_new_note_always_contributes_to_staleness() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("a.md"), "# A").expect("write a");
            let indexer = IndexerService::new(temp.path());
            indexer
                .persist(&indexer.build().expect("build index"))
                .expect("persist index");

            fs::write(temp.path().join("b.md"), "# B, no links")
                .expect("write new note");

            let (_, report) =
                indexer.refresh_with_report().expect("refresh index");
            assert_eq!(report.upserted, 1);
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
            let indexer = IndexerService::new(temp.path());
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
            let indexer = IndexerService::new(temp.path());
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
            let indexer = IndexerService::new(temp.path());
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
            let indexer = IndexerService::new(temp.path());
            indexer
                .persist(&indexer.build().expect("build index"))
                .expect("persist index");

            fs::remove_file(temp.path().join("linker.md"))
                .expect("delete linker");

            let refreshed = Arc::new(indexer.refresh().expect("refresh index"));
            let outcome = query_pages(&refreshed, &SourceSelector::All);
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

            let refreshed = Arc::new(indexer.refresh().expect("refresh index"));
            let outcome = query_pages(&refreshed, &SourceSelector::All);
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

            let refreshed = IndexerService::new(temp.path())
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
            let indexer = IndexerService::new(temp.path());
            indexer
                .persist(&indexer.build().expect("build index"))
                .expect("persist index");

            let refreshed = Arc::new(indexer.refresh().expect("refresh index"));
            let outcome = query_pages(&refreshed, &SourceSelector::All);
            let target = outcome
                .iter()
                .find(|record| record.file().path() == Path::new("target.md"))
                .expect("target record");

            assert_eq!(target.inlinks(), [PathBuf::from("linker.md")]);
        }

        #[test]
        fn refresh_persists_so_a_fresh_load_reflects_the_change() {
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
            let indexer = IndexerService::new(temp.path());
            indexer
                .persist(&indexer.build().expect("build index"))
                .expect("persist index");

            fs::remove_file(temp.path().join("archive/foo.md"))
                .expect("delete archive/foo.md");

            let refreshed = Arc::new(indexer.refresh().expect("refresh index"));
            let outcome = query_pages(&refreshed, &SourceSelector::All);
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
