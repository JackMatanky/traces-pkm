//! Query execution service over in-memory indexes and persisted key-value
//! storage.
//!
//! This module provides [`QueryService`], the central coordination engine that
//! executes queries against either an in-memory [`WorkspaceIndex`] or directly
//! against on-disk [`IndexStore`] tables without loading the full index into
//! memory.
//! It resolves candidate paths from [`SourceSelector`] expressions, expands
//! File Class inheritance trees via [`FileClassExpander`], instantiates
//! [`QueryRow`] items, and applies the optimized
//! [`super::plan::ExecutionPlan`].
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use super::{
    QueryBuilder, QueryMode, QueryRow, QuerySet,
    grammar::{BooleanExpr, FileClassExpander, SourceAtom, SourceSelector},
};
#[cfg(any(test, feature = "test-utils"))]
use crate::index::IndexerService;
use crate::{
    ListItem,
    index::{IndexResult, IndexStore, RowIndex, SortedByPath, WorkspaceIndex},
};

/// Evaluates source expressions against a borrowed [`WorkspaceIndex`].
///
/// Supports page/list/task modes, optional File Class expansion, and pending
/// plan transformations.
///
/// # Examples
///
/// ```rust
/// # #[cfg(feature = "test-utils")]
/// # {
/// use std::sync::Arc;
///
/// use traces_pkm::{
///     IndexerService, QueryBuilder, QueryService, SourceSelector,
/// };
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let temp = tempfile::tempdir()?;
/// let index = Arc::new(IndexerService::for_tests(temp.path()).build()?);
///
/// let service = QueryService::new("class");
/// let outcome = service.run(&index, QueryBuilder::pages(SourceSelector::All));
/// assert_eq!(outcome.len(), 0);
/// # Ok(())
/// # }
/// # }
/// ```
#[derive(Clone)]
pub struct QueryService {
    class_field: String,
    class_expander: Option<Arc<dyn FileClassExpander>>,
}

impl QueryService {
    /// Creates a service with case- and key-normalized `class_field` matching.
    #[inline]
    #[must_use]
    pub fn new<S: Into<String>>(class_field: S) -> Self {
        let class_field = class_field.into();
        let class_field = crate::FieldKey::try_new(&class_field).map_or_else(
            |_| class_field.to_lowercase(),
            |key| key.canonical().to_owned(),
        );
        Self {
            class_field,
            class_expander: None,
        }
    }

    /// Attaches class hierarchy expansion.
    #[inline]
    #[must_use]
    pub(crate) fn with_class_expander(
        mut self,
        expander: Arc<dyn FileClassExpander>,
    ) -> Self {
        self.class_expander = Some(expander);
        self
    }

    /// Expands `source`'s File Class atoms through this service's expander,
    /// if one is attached. Both execution paths call this before row
    /// generation so class matching shares one implementation.
    fn resolve_source_classes(&self, source: &mut SourceSelector) {
        if source.has_classes()
            && let Some(expander) = self.class_expander.as_deref()
        {
            source.resolve_classes(expander);
        }
    }

    /// Applies File Class expansion and plan transformations to `builder`.
    #[inline]
    pub fn run(
        &self,
        index: &Arc<WorkspaceIndex>,
        builder: QueryBuilder,
    ) -> QuerySet {
        let (mode, mut source, plan) = builder.into_parts();
        self.resolve_source_classes(&mut source);
        let rows = self.rows_for(mode, index, &source);
        QuerySet::new(plan.run(rows))
    }

    /// Runs `builder` against persisted records without building a full
    /// [`WorkspaceIndex`].
    ///
    /// Resolves candidate paths, reads only matching notes/files/inlinks,
    /// assembles a temporary index, and applies the execution plan.
    ///
    /// # Errors
    ///
    /// - [`crate::index::IndexError`] if source resolution or any batch read
    ///   from `store` fails.
    #[inline]
    pub(crate) fn run_from_store(
        &self,
        store: &IndexStore,
        builder: QueryBuilder,
    ) -> IndexResult<QuerySet> {
        let (mode, mut source, plan) = builder.into_parts();
        self.resolve_source_classes(&mut source);
        let resolver = SourceResolver::new(store);
        let candidate_paths = resolver.resolve(&source)?;
        let (notes_result, (files_result, inlinks_result)) = rayon::join(
            || {
                store.read_notes_batch(
                    candidate_paths.iter().map(PathBuf::as_path),
                )
            },
            || {
                rayon::join(
                    || {
                        store.read_files_batch(
                            candidate_paths.iter().map(PathBuf::as_path),
                        )
                    },
                    || {
                        store.read_links_for_targets(
                            candidate_paths.iter().map(PathBuf::as_path),
                        )
                    },
                )
            },
        );
        let notes = notes_result?;
        let matching_files = files_result?;
        let inlinks = inlinks_result?;
        let index = Arc::new(WorkspaceIndex::assemble(
            SortedByPath::assumed_sorted(matching_files),
            SortedByPath::assumed_sorted(notes),
            inlinks,
        ));
        let rows = self.rows_for(mode, &index, &source);
        Ok(QuerySet::new(plan.run(rows)))
    }

    /// Syncs `indexer` and runs `builder` through the persisted-store path.
    ///
    /// Exposes the cold-read path used by `traces list`, `traces table`, and
    /// `traces task` without exposing crate-private `IndexStore`.
    ///
    /// # Errors
    ///
    /// - `IndexError` if syncing the persisted index fails.
    /// - `IndexError` if persisted-store query execution fails.
    #[cfg(any(test, feature = "test-utils"))]
    #[inline]
    pub fn sync_and_run(
        &self,
        indexer: &IndexerService,
        builder: QueryBuilder,
    ) -> IndexResult<QuerySet> {
        let store = indexer.refresh_store()?;
        self.run_from_store(&store, builder)
    }

    /// Instantiates rows at `mode`'s granularity for notes matching `source`.
    fn rows_for(
        &self,
        mode: QueryMode,
        index: &Arc<WorkspaceIndex>,
        source: &SourceSelector,
    ) -> Vec<QueryRow> {
        match mode {
            QueryMode::Pages => self.pages(index, source),
            QueryMode::Lists => self.lists(index, source),
            QueryMode::Tasks => self.tasks(index, source),
        }
    }

    fn pages(
        &self,
        index: &Arc<WorkspaceIndex>,
        source: &SourceSelector,
    ) -> Vec<QueryRow> {
        self.matched_file_rows(index, source).collect()
    }

    /// Expands matching notes into one [`QueryRow`] per list item, including
    /// plain bullets, checkboxes, and tasks, in document order.
    fn lists(
        &self,
        index: &Arc<WorkspaceIndex>,
        source: &SourceSelector,
    ) -> Vec<QueryRow> {
        self.item_rows(index, source, |_| true)
    }

    /// Expands matching notes into one [`QueryRow`] per task list item.
    fn tasks(
        &self,
        index: &Arc<WorkspaceIndex>,
        source: &SourceSelector,
    ) -> Vec<QueryRow> {
        self.item_rows(index, source, |item| item.kind().is_task())
    }

    /// Expands matching notes into one [`QueryRow`] per list item whose kind
    /// satisfies `is_wanted`, in document order.
    fn item_rows(
        &self,
        index: &Arc<WorkspaceIndex>,
        source: &SourceSelector,
        is_wanted: impl Fn(&ListItem) -> bool,
    ) -> Vec<QueryRow> {
        let mut out = Vec::new();
        for base in self.matched_file_rows(index, source) {
            let Some(note) = base.note() else {
                continue;
            };
            for (item_idx, item) in note.lists().iter().enumerate() {
                if is_wanted(item)
                    && let Ok(item_idx) = u32::try_from(item_idx)
                {
                    out.push(base.clone().with_list_item(item_idx));
                }
            }
        }
        out
    }

    /// Creates one row per indexed file matching `source`, at note
    /// granularity. List and task modes expand these base rows into per-item
    /// rows via [`Self::item_rows`].
    fn matched_file_rows<'b>(
        &'b self,
        index: &'b Arc<WorkspaceIndex>,
        source: &'b SourceSelector,
    ) -> impl Iterator<Item = QueryRow> + 'b {
        (0..index.entries().len())
            .map(RowIndex::new)
            .filter(move |&position| {
                source.is_match(index.entry_at(position), &self.class_field)
            })
            .map(move |position| QueryRow::new(index, position))
    }
}

impl std::fmt::Debug for QueryService {
    #[inline]
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("QueryService")
            .field("class_field", &self.class_field)
            .field("has_class_expander", &self.class_expander.is_some())
            .finish()
    }
}

/// Store-backed resolver for source selector candidate paths.
struct SourceResolver<'a> {
    store: &'a IndexStore,
}

impl<'a> SourceResolver<'a> {
    fn new(store: &'a IndexStore) -> Self {
        Self {
            store,
        }
    }

    fn resolve(
        &self,
        selector: &SourceSelector,
    ) -> IndexResult<Box<[PathBuf]>> {
        match selector {
            SourceSelector::All => self.store.paths_in_folder(Path::new("")),
            SourceSelector::Expr(expr) => self.resolve_expr(expr.inner()),
        }
    }

    /// Recursively resolves a boolean source expression, intersecting `And`
    /// branches and unioning `Or` branches. `Not` is unsupported at the
    /// store-scoped resolution level and falls back to every indexed path.
    fn resolve_expr(
        &self,
        expr: &BooleanExpr<SourceAtom>,
    ) -> IndexResult<Box<[PathBuf]>> {
        match expr {
            BooleanExpr::Atom(atom) => self.resolve_atom(atom),
            BooleanExpr::And(terms) => {
                let mut iter = terms.iter();
                let Some(first) = iter.next() else {
                    return self.store.paths_in_folder(Path::new(""));
                };
                let mut acc = self.resolve_expr(first)?;
                for next in iter {
                    let next_paths = self.resolve_expr(next)?;
                    acc = Self::intersect_sorted(&acc, &next_paths);
                }
                Ok(acc)
            }
            BooleanExpr::Or(terms) => {
                let mut acc = Vec::new();
                for term in terms {
                    let paths = self.resolve_expr(term)?;
                    acc.extend_from_slice(&paths);
                }
                acc.sort();
                acc.dedup();
                Ok(acc.into_boxed_slice())
            }
            BooleanExpr::Not(_) => self.store.paths_in_folder(Path::new("")),
        }
    }

    /// Resolves one source atom using tag/class indexes or path matching.
    fn resolve_atom(&self, atom: &SourceAtom) -> IndexResult<Box<[PathBuf]>> {
        match atom {
            SourceAtom::Tag(tag) => self.store.paths_with_tag(tag),
            SourceAtom::Class {
                names,
                mode,
            } => {
                let mut acc = Vec::new();
                if mode.classes().is_empty() {
                    for class in names {
                        let paths = self.store.paths_with_file_class(class)?;
                        acc.extend_from_slice(&paths);
                    }
                } else {
                    for class in mode.classes() {
                        let paths = self.store.paths_with_file_class(class)?;
                        acc.extend_from_slice(&paths);
                    }
                }
                acc.sort();
                acc.dedup();
                Ok(acc.into_boxed_slice())
            }
            SourceAtom::Path(pattern) => {
                let all = self.store.paths_in_folder(Path::new(""))?;
                let matched: Vec<PathBuf> = all
                    .iter()
                    .filter(|p| pattern.is_match(p))
                    .cloned()
                    .collect();
                Ok(matched.into_boxed_slice())
            }
        }
    }

    /// Linear two-pointer intersection of two sorted slices.
    fn intersect_sorted(a: &[PathBuf], b: &[PathBuf]) -> Box<[PathBuf]> {
        let mut out = Vec::new();
        let mut i = 0usize;
        let mut j = 0usize;
        while i < a.len() && j < b.len() {
            let (Some(item_a), Some(item_b)) = (a.get(i), b.get(j)) else {
                break;
            };
            match item_a.cmp(item_b) {
                std::cmp::Ordering::Less => i = i.saturating_add(1),
                std::cmp::Ordering::Greater => j = j.saturating_add(1),
                std::cmp::Ordering::Equal => {
                    out.push(item_a.clone());
                    i = i.saturating_add(1);
                    j = j.saturating_add(1);
                }
            }
        }
        out.into_boxed_slice()
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, path::Path, sync::Arc};

    use super::*;
    use crate::{
        Note,
        index::{IndexerService, WorkspaceIndex},
        query::{QueryBuilder, QueryRow, QuerySet, SourceSelector},
    };

    fn query_pages(
        index: &Arc<WorkspaceIndex>,
        source: &SourceSelector,
    ) -> QuerySet {
        QueryService::new("class")
            .run(index, QueryBuilder::pages(source.clone()))
    }

    fn query_tasks(
        index: &Arc<WorkspaceIndex>,
        source: &SourceSelector,
    ) -> QuerySet {
        QueryService::new("class")
            .run(index, QueryBuilder::tasks(source.clone()))
    }

    fn query_lists(
        index: &Arc<WorkspaceIndex>,
        source: &SourceSelector,
    ) -> QuerySet {
        QueryService::new("class")
            .run(index, QueryBuilder::lists(source.clone()))
    }

    mod source_resolver {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn resolves_all_selector_to_all_paths() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("a.md"), "# A #tag1").expect("write a");
            fs::write(temp.path().join("b.md"), "---\nclass: Book\n---\n# B")
                .expect("write b");
            let indexer = IndexerService::for_tests(temp.path());
            let index = indexer.build().expect("build");
            indexer.persist(&index).expect("persist");

            let store = IndexStore::open(temp.path()).expect("open store");
            let resolver = SourceResolver::new(&store);
            let paths =
                resolver.resolve(&SourceSelector::All).expect("resolve all");
            assert_eq!(paths.as_ref(), [
                PathBuf::from("a.md"),
                PathBuf::from("b.md")
            ]);
        }

        #[test]
        fn resolves_tag_selector_via_multimap() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("a.md"), "# A #project/active")
                .expect("write a");
            fs::write(temp.path().join("b.md"), "# B #other").expect("write b");
            let indexer = IndexerService::for_tests(temp.path());
            let index = indexer.build().expect("build");
            indexer.persist(&index).expect("persist");

            let store = IndexStore::open(temp.path()).expect("open store");
            let resolver = SourceResolver::new(&store);
            let selector =
                SourceSelector::parse("#project").expect("parse selector");
            let paths = resolver.resolve(&selector).expect("resolve tag");
            assert_eq!(paths.as_ref(), [PathBuf::from("a.md")]);
        }

        #[test]
        fn incremental_refresh_removes_a_notes_stale_tag_from_the_multimap() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("a.md"), "# A #project/active")
                .expect("write a");
            let indexer = IndexerService::for_tests(temp.path());
            indexer.persist(&indexer.build().expect("build")).expect("persist");

            // Refresh incrementally after removing a tag.
            fs::write(temp.path().join("a.md"), "# A #other")
                .expect("rewrite a with a different tag");
            indexer.refresh().expect("incremental refresh persists");

            let store = IndexStore::open(temp.path()).expect("open store");
            let resolver = SourceResolver::new(&store);
            let old_tag =
                SourceSelector::parse("#project").expect("parse selector");
            let new_tag =
                SourceSelector::parse("#other").expect("parse selector");

            assert!(
                resolver.resolve(&old_tag).expect("resolve old tag").is_empty(),
                "a.md's old #project tag should no longer resolve any path"
            );
            assert_eq!(
                resolver.resolve(&new_tag).expect("resolve new tag").as_ref(),
                [PathBuf::from("a.md")]
            );
        }

        #[test]
        fn resolves_class_selector_via_multimap() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("a.md"), "---\nclass: Book\n---\n# A")
                .expect("write a");
            fs::write(temp.path().join("b.md"), "# B").expect("write b");
            let indexer = IndexerService::for_tests(temp.path());
            let index = indexer.build().expect("build");
            indexer.persist(&index).expect("persist");

            let store = IndexStore::open(temp.path()).expect("open store");
            let resolver = SourceResolver::new(&store);
            let selector =
                SourceSelector::parse("@Book").expect("parse selector");
            let paths = resolver.resolve(&selector).expect("resolve class");
            assert_eq!(paths.as_ref(), [PathBuf::from("a.md")]);
        }

        #[test]
        fn intersect_sorted_returns_common_paths() {
            let a = [
                PathBuf::from("a.md"),
                PathBuf::from("b.md"),
                PathBuf::from("c.md"),
            ];
            let b = [PathBuf::from("b.md"), PathBuf::from("d.md")];
            let res = SourceResolver::intersect_sorted(&a, &b);
            assert_eq!(res.as_ref(), [PathBuf::from("b.md")]);
        }

        #[test]
        fn run_from_store_evaluates_query_over_only_matching_notes() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("a.md"), "# A #book\n\n- [ ] Task A")
                .expect("write a");
            fs::write(temp.path().join("b.md"), "# B #other\n\n- [ ] Task B")
                .expect("write b");
            let indexer = IndexerService::for_tests(temp.path());
            let index = indexer.build().expect("build");
            indexer.persist(&index).expect("persist");

            let store = IndexStore::open(temp.path()).expect("open store");
            let service = QueryService::new("class");
            let selector =
                SourceSelector::parse("#book").expect("parse selector");
            let query_set = service
                .run_from_store(&store, QueryBuilder::pages(selector))
                .expect("run from store");
            assert_eq!(query_set.len(), 1);
            assert_eq!(
                query_set.get(0).map(|r| r.file().path()),
                Some(Path::new("a.md"))
            );
        }
    }

    mod query {
        use std::path::PathBuf;

        use pretty_assertions::assert_eq;

        use super::*;
        use crate::Tag;

        fn note_paths(outcome: &QuerySet) -> Vec<&Path> {
            outcome
                .iter()
                .filter_map(|row| row.note().map(Note::path))
                .collect()
        }

        fn build_book_index() -> Arc<WorkspaceIndex> {
            crate::build_test_index(&[(
                "book.md",
                "---\ntitle: Dune\n---\nGenre:: Sci-fi\n\nShelved as #book.",
            )])
        }

        #[test]
        fn returns_all_files_in_sorted_order() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("a.md"), "# A").expect("write a");
            fs::write(temp.path().join("b.md"), "# B").expect("write b");
            fs::write(temp.path().join("readme.txt"), "text")
                .expect("write txt");
            let index = Arc::new(
                IndexerService::for_tests(temp.path())
                    .build()
                    .expect("build index"),
            );
            let outcome = query_pages(&index, &SourceSelector::All);

            assert_eq!(
                outcome.get(0).map(|r| r.file().path()),
                Some(Path::new("a.md"))
            );
            assert_eq!(
                outcome.get(1).map(|r| r.file().path()),
                Some(Path::new("b.md"))
            );
            assert_eq!(
                outcome.get(2).map(|r| r.file().path()),
                Some(Path::new("readme.txt"))
            );
            assert!(outcome.get(3).is_none());
        }

        #[test]
        fn excludes_non_markdown_files_from_note_results() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("a.md"), "# A").expect("write a");
            fs::write(temp.path().join("readme.txt"), "text")
                .expect("write txt");
            let index = Arc::new(
                IndexerService::for_tests(temp.path())
                    .build()
                    .expect("build index"),
            );
            let outcome = query_pages(&index, &SourceSelector::All);

            assert_eq!(note_paths(&outcome), [Path::new("a.md")]);
            assert_eq!(outcome.get(1).and_then(|r| r.note()), None);
        }

        #[test]
        fn returns_empty_when_no_notes_match_tag() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("readme.txt"), "text")
                .expect("write txt");
            let index = Arc::new(
                IndexerService::for_tests(temp.path())
                    .build()
                    .expect("build index"),
            );
            let outcome = query_pages(
                &index,
                &SourceSelector::parse("#missing").expect("valid source"),
            );

            assert_eq!(outcome.len(), 0);
        }

        #[test]
        fn returns_matching_note_when_tag_source_is_exact() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("book.md"), "Filed under #book.")
                .expect("write book");
            fs::write(temp.path().join("other.md"), "No tags here.")
                .expect("write other");
            let index = Arc::new(
                IndexerService::for_tests(temp.path())
                    .build()
                    .expect("build index"),
            );
            let outcome = query_pages(
                &index,
                &SourceSelector::parse("#book").expect("valid source"),
            );

            assert_eq!(note_paths(&outcome), [Path::new("book.md")]);
        }

        #[test]
        fn returns_matching_note_when_tag_source_is_nested() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(
                temp.path().join("project.md"),
                "Tracked in #projects/active.",
            )
            .expect("write project");
            fs::write(temp.path().join("other.md"), "No tags here.")
                .expect("write other");
            let index = Arc::new(
                IndexerService::for_tests(temp.path())
                    .build()
                    .expect("build index"),
            );
            let exact = query_pages(
                &index,
                &SourceSelector::parse("#projects/active")
                    .expect("valid source"),
            );
            let parent = query_pages(
                &index,
                &SourceSelector::parse("#projects").expect("valid source"),
            );

            assert_eq!(note_paths(&exact), [Path::new("project.md")]);
            assert_eq!(note_paths(&parent), [Path::new("project.md")]);
        }

        #[test]
        fn returns_empty_when_tag_query_is_too_specific() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("project.md"), "Tracked in #projects.")
                .expect("write project");
            let index = Arc::new(
                IndexerService::for_tests(temp.path())
                    .build()
                    .expect("build index"),
            );
            let outcome = query_pages(
                &index,
                &SourceSelector::parse("#projects/active")
                    .expect("valid source"),
            );

            assert!(outcome.is_empty());
        }

        #[test]
        fn returns_notes_at_and_under_folder_when_source_is_folder() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::create_dir_all(temp.path().join("books/fiction"))
                .expect("mkdir books/fiction");
            fs::write(temp.path().join("books/dune.md"), "# Dune")
                .expect("write dune");
            fs::write(temp.path().join("books/fiction/hobbit.md"), "# Hobbit")
                .expect("write hobbit");
            fs::write(temp.path().join("other.md"), "# Other")
                .expect("write other");
            let index = Arc::new(
                IndexerService::for_tests(temp.path())
                    .build()
                    .expect("build index"),
            );
            let outcome = query_pages(
                &index,
                &SourceSelector::parse("books/").expect("valid source"),
            );

            assert_eq!(note_paths(&outcome), [
                Path::new("books/dune.md"),
                Path::new("books/fiction/hobbit.md")
            ]);
        }

        #[test]
        fn returns_file_path_for_each_record() {
            let index = build_book_index();

            let outcome = query_pages(&index, &SourceSelector::All);
            let row = outcome.iter().next().expect("one row");

            assert_eq!(row.file().path(), Path::new("book.md"));
        }

        #[test]
        fn includes_frontmatter_fields_in_note() {
            let index = build_book_index();

            let outcome = query_pages(&index, &SourceSelector::All);
            let note =
                outcome.iter().next().expect("one row").note().expect("note");

            assert_eq!(note.frontmatter().map(|fm| fm.fields().len()), Some(1));
        }

        #[test]
        fn includes_inline_field_keys() {
            let index = build_book_index();

            let outcome = query_pages(&index, &SourceSelector::All);
            let note =
                outcome.iter().next().expect("one row").note().expect("note");

            assert_eq!(
                note.inline_fields()
                    .iter()
                    .map(|(key, _)| key.canonical())
                    .collect::<Vec<_>>(),
                ["genre"]
            );
        }

        #[test]
        fn includes_note_tags() {
            let index = build_book_index();

            let outcome = query_pages(&index, &SourceSelector::All);
            let note =
                outcome.iter().next().expect("one row").note().expect("note");

            assert_eq!(note.tags(), [Tag::parse("#book").unwrap()]);
        }

        #[test]
        fn derives_inlinks_from_multiple_notes_linking_to_the_same_target() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("target.md"), "# Target")
                .expect("write target");
            fs::write(temp.path().join("a.md"), "[[target]]").expect("write a");
            fs::write(temp.path().join("b.md"), "[[target]]").expect("write b");
            let index = Arc::new(
                IndexerService::for_tests(temp.path())
                    .build()
                    .expect("build index"),
            );
            let outcome = query_pages(&index, &SourceSelector::All);
            let target = outcome
                .iter()
                .find(|row| row.file().path() == Path::new("target.md"))
                .expect("target row");

            assert_eq!(target.inlinks(), [
                PathBuf::from("a.md"),
                PathBuf::from("b.md")
            ]);
        }

        #[test]
        fn includes_a_linking_note_outside_the_source_in_the_targets_inlinks() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("target.md"), "#book\n")
                .expect("write target");
            fs::write(temp.path().join("linker.md"), "[[target]]")
                .expect("write linker");
            let index = Arc::new(
                IndexerService::for_tests(temp.path())
                    .build()
                    .expect("build index"),
            );
            let outcome = query_pages(
                &index,
                &SourceSelector::parse("#book").expect("valid source"),
            );
            let target = outcome.iter().next().expect("target row");

            assert_eq!(target.file().path(), Path::new("target.md"));
            assert_eq!(target.inlinks(), [PathBuf::from("linker.md")]);
        }

        #[test]
        fn deduplicates_outlinks_from_same_source() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("target.md"), "# Target")
                .expect("write target");
            fs::write(
                temp.path().join("a.md"),
                "[[target]] and [[target]] again",
            )
            .expect("write a");
            let index = Arc::new(
                IndexerService::for_tests(temp.path())
                    .build()
                    .expect("build index"),
            );
            let outcome = query_pages(&index, &SourceSelector::All);
            let target = outcome
                .iter()
                .find(|row| row.file().path() == Path::new("target.md"))
                .expect("target row");

            assert_eq!(target.inlinks(), [PathBuf::from("a.md")]);
        }

        #[test]
        fn preserves_a_self_linking_notes_own_inlink() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("b.md"), "[[b]]").expect("write b");
            let index = Arc::new(
                IndexerService::for_tests(temp.path())
                    .build()
                    .expect("build index"),
            );
            let outcome = query_pages(&index, &SourceSelector::All);
            let source = outcome
                .iter()
                .find(|row| row.file().path() == Path::new("b.md"))
                .expect("self-linking row");

            assert_eq!(source.inlinks(), [PathBuf::from("b.md")]);
        }

        #[test]
        fn derives_inlinks_from_outlinks() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("target.md"), "# Target")
                .expect("write target");
            fs::write(temp.path().join("linker.md"), "[[target]]")
                .expect("write linker");

            let index = Arc::new(
                IndexerService::for_tests(temp.path())
                    .build()
                    .expect("build index"),
            );
            let outcome = query_pages(&index, &SourceSelector::All);
            let target = outcome
                .iter()
                .find(|r| r.file().path() == Path::new("target.md"))
                .expect("target row");

            assert_eq!(target.inlinks(), [PathBuf::from("linker.md")]);
        }
    }

    mod query_tasks {
        use std::path::PathBuf;

        use pretty_assertions::assert_eq;

        use super::*;

        fn task_states(outcome: &QuerySet) -> Vec<(Option<bool>, &str)> {
            outcome
                .iter()
                .map(|row| {
                    (row.task_completed(), row.task_text().unwrap_or_default())
                })
                .collect()
        }

        #[test]
        fn contributes_no_rows_when_note_has_no_tasks() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("no-tasks.md"), "Just prose, no tasks.")
                .expect("write note");
            fs::write(temp.path().join("todo.md"), "- [ ] buy milk\n")
                .expect("write note");
            let index = Arc::new(
                IndexerService::for_tests(temp.path())
                    .build()
                    .expect("build index"),
            );
            let outcome = query_tasks(&index, &SourceSelector::All);

            assert_eq!(outcome.len(), 1);
            assert_eq!(
                outcome.iter().next().and_then(QueryRow::task_text),
                Some("buy milk")
            );
        }

        #[test]
        fn returns_empty_outcome_when_no_notes_match_source() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("readme.txt"), "text")
                .expect("write txt");
            let index = Arc::new(
                IndexerService::for_tests(temp.path())
                    .build()
                    .expect("build index"),
            );
            let outcome = query_tasks(&index, &SourceSelector::All);

            assert!(outcome.is_empty());
        }

        #[test]
        fn retains_file_path() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(
                temp.path().join("project.md"),
                "---\ntitle: Launch\n---\nFiled under #projects.\n\n- [ ] \
                 ship it\n",
            )
            .expect("write note");
            let index = Arc::new(
                IndexerService::for_tests(temp.path())
                    .build()
                    .expect("build index"),
            );
            let outcome = query_tasks(&index, &SourceSelector::All);
            let row = outcome.iter().next().expect("one task row");

            assert_eq!(row.file().path(), Path::new("project.md"));
        }

        #[test]
        fn retains_frontmatter_field() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(
                temp.path().join("project.md"),
                "---\ntitle: Launch\n---\nFiled under #projects.\n\n- [ ] \
                 ship it\n",
            )
            .expect("write note");
            let index = Arc::new(
                IndexerService::for_tests(temp.path())
                    .build()
                    .expect("build index"),
            );
            let outcome = query_tasks(&index, &SourceSelector::All);
            let row = outcome.iter().next().expect("one task row");

            assert_eq!(
                row.field("title"),
                Ok(crate::NoteFieldValue::String("Launch".to_owned()))
            );
        }

        #[test]
        fn retains_tag_field() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(
                temp.path().join("project.md"),
                "---\ntitle: Launch\n---\nFiled under #projects.\n\n- [ ] \
                 ship it\n",
            )
            .expect("write note");
            let index = Arc::new(
                IndexerService::for_tests(temp.path())
                    .build()
                    .expect("build index"),
            );
            let outcome = query_tasks(&index, &SourceSelector::All);
            let row = outcome.iter().next().expect("one task row");

            assert_eq!(
                row.field("tags"),
                Ok(crate::NoteFieldValue::List(Box::default()))
            );
            assert_eq!(
                row.field("file.tags"),
                Ok(crate::NoteFieldValue::List(
                    vec![
                        crate::NoteFieldValue::String("#projects".to_owned(),)
                    ]
                    .into(),
                ))
            );
        }

        #[test]
        fn retains_the_parent_notes_inlinks() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("target.md"), "- [ ] ship it\n")
                .expect("write target");
            fs::write(temp.path().join("linker.md"), "[[target]]")
                .expect("write linker");
            let index = Arc::new(
                IndexerService::for_tests(temp.path())
                    .build()
                    .expect("build index"),
            );
            let outcome = query_tasks(&index, &SourceSelector::All);
            let task = outcome.iter().next().expect("one task row");

            assert_eq!(task.file().path(), Path::new("target.md"));
            assert_eq!(task.inlinks(), [PathBuf::from("linker.md")]);
        }

        #[test]
        fn returns_only_tasks_from_notes_matching_the_tag_source() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(
                temp.path().join("a.md"),
                "#projects\n- [ ] project task\n",
            )
            .expect("write a");
            fs::write(temp.path().join("b.md"), "#books\n- [ ] book task\n")
                .expect("write b");
            let index = Arc::new(
                IndexerService::for_tests(temp.path())
                    .build()
                    .expect("build index"),
            );
            let outcome = query_tasks(
                &index,
                &SourceSelector::parse("#projects").expect("valid source"),
            );

            assert_eq!(task_states(&outcome), [(Some(false), "project task")]);
        }

        #[test]
        fn returns_only_tasks_from_notes_under_the_folder_source() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::create_dir_all(temp.path().join("projects")).expect("mkdir");
            fs::write(
                temp.path().join("projects/a.md"),
                "- [ ] project task\n",
            )
            .expect("write a");
            fs::write(temp.path().join("b.md"), "- [ ] other task\n")
                .expect("write b");
            let index = Arc::new(
                IndexerService::for_tests(temp.path())
                    .build()
                    .expect("build index"),
            );
            let outcome = query_tasks(
                &index,
                &SourceSelector::parse("projects/").expect("valid source"),
            );

            assert_eq!(task_states(&outcome), [(Some(false), "project task")]);
        }

        #[test]
        fn filters_tasks_by_completion_status() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(
                temp.path().join("todo.md"),
                "- [ ] buy milk\n- [x] pay rent\n",
            )
            .expect("write note");
            let index = Arc::new(
                IndexerService::for_tests(temp.path())
                    .build()
                    .expect("build index"),
            );
            let outcome = query_tasks(&index, &SourceSelector::All)
                .filter("list.completed == true")
                .expect("valid filter");

            // Filtering must keep only matching task rows, not every row from a
            // note with one match.
            assert_eq!(task_states(&outcome), [(Some(true), "pay rent")]);
        }
    }

    mod query_lists {
        use pretty_assertions::assert_eq;

        use super::*;
        use crate::NoteFieldValue;

        #[test]
        fn emits_plain_checkbox_and_tasks_in_document_order() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(
                temp.path().join("items.md"),
                "- plain bullet\n- [ ] checkbox\n- [x] done #task\n",
            )
            .expect("write note");
            // Default classification would promote every status-marked item
            // to a Task; the tag filter keeps the bare checkbox a checkbox.
            let config = crate::Config::test_default(temp.path().to_path_buf())
                .with_tasks(crate::TaskConfig::from_tags(&["#task"]));
            let index = Arc::new(
                IndexerService::from(&config).build().expect("build index"),
            );

            let kinds: Vec<_> = query_lists(&index, &SourceSelector::All)
                .iter()
                .map(|row| row.field("list.kind").expect("list.kind resolves"))
                .collect();

            assert_eq!(kinds, [
                NoteFieldValue::String("plain".into()),
                NoteFieldValue::String("checkbox".into()),
                NoteFieldValue::String("task".into()),
            ]);
        }

        #[test]
        fn contributes_no_rows_when_note_has_no_list_items() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("prose.md"), "Just prose, no lists.")
                .expect("write note");
            let index = Arc::new(
                IndexerService::for_tests(temp.path())
                    .build()
                    .expect("build index"),
            );
            let outcome = query_lists(&index, &SourceSelector::All);

            assert_eq!(outcome.len(), 0);
        }

        #[test]
        fn filters_list_items_by_is_task() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(
                temp.path().join("items.md"),
                "- plain bullet 1\n- plain bullet 2\n- [x] task\n",
            )
            .expect("write note");
            let index = Arc::new(
                IndexerService::for_tests(temp.path())
                    .build()
                    .expect("build index"),
            );
            let non_tasks = query_lists(&index, &SourceSelector::All)
                .filter("list.is_task == false")
                .expect("valid filter");
            let tasks = query_lists(&index, &SourceSelector::All)
                .filter("list.is_task == true")
                .expect("valid filter");

            assert_eq!(non_tasks.len(), 2);
            assert_eq!(tasks.len(), 1);
        }

        #[test]
        fn runs_from_store_matching_in_memory_lists() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(
                temp.path().join("items.md"),
                "- plain bullet\n- [x] task #todo\n",
            )
            .expect("write note");
            let service = QueryService::new("class");
            let indexer = IndexerService::for_tests(temp.path());
            let index = Arc::new(indexer.build().expect("build index"));

            let from_store = service
                .sync_and_run(
                    &indexer,
                    QueryBuilder::lists(SourceSelector::All),
                )
                .expect("run from store");
            let in_memory =
                service.run(&index, QueryBuilder::lists(SourceSelector::All));

            // Persisted list items must reconstruct rows identical to the
            // in-memory path without reparsing Markdown.
            assert_eq!(from_store, in_memory);
        }
    }
}
