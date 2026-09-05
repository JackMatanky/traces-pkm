//! Query service executing [`QueryBuilder`] queries over a borrowed
//! [`FileIndex`].
//!
//! Defines [`QueryService`], which matches notes against candidate source
//! selectors and applies pre-fetch query plans.

use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use super::{
    QueryBuilder, QueryMode, QueryRow, QuerySet,
    grammar::{BooleanExpr, FileClassExpander, SourceAtom, SourceSelector},
};
use crate::index::{FileIndex, IndexResult, IndexStore, RowIndex};
/// Query execution engine over a borrowed [`FileIndex`].
///
/// `QueryService` executes [`QueryBuilder`] specifications against an indexed
/// repository. It evaluates candidate source expressions, filters page or task
/// rows, resolves optional File Class hierarchies via an attached
/// `FileClassExpander`, and produces a [`QuerySet`].
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
/// let index = Arc::new(IndexerService::new(temp.path()).build()?);
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
    /// Creates a query service configured to read File Class values from
    /// `class_field`.
    ///
    /// Canonicalizes `class_field` so matching is case- and key-normalized.
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

    /// Attaches a [`FileClassExpander`] to resolve class hierarchy expansions.
    #[inline]
    #[must_use]
    pub(crate) fn with_class_expander(
        mut self,
        expander: Arc<dyn FileClassExpander>,
    ) -> Self {
        self.class_expander = Some(expander);
        self
    }

    /// Runs `builder` against `index` and returns a [`QuerySet`].
    ///
    /// Resolves candidate notes from `index` according to `builder`'s mode and
    /// source selector, then attaches any pending transformations to the
    /// returned [`QuerySet`].
    #[inline]
    pub fn run(
        &self,
        index: &Arc<FileIndex>,
        builder: QueryBuilder,
    ) -> QuerySet {
        let (mode, mut source, plan) = builder.into_parts();
        if source.has_classes()
            && let Some(expander) = self.class_expander.as_deref()
        {
            source.resolve_classes(expander);
        }
        let rows = match mode {
            QueryMode::Pages => self.page_rows(index, &source),
            QueryMode::Tasks => self.task_rows(index, &source),
        };
        QuerySet::new(plan.run(rows))
    }

    /// Executes `builder` directly against `store`, bypassing full
    /// [`FileIndex`] materialization by resolving only candidate records
    /// matching `builder`'s source selector.
    ///
    /// # Errors
    ///
    /// Returns [`IndexError`] if candidate resolution or storage reads fail.
    #[inline]
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "part of storage query acceleration surface"
        )
    )]
    pub(crate) fn run_from_store(
        &self,
        store: &IndexStore,
        builder: QueryBuilder,
    ) -> IndexResult<QuerySet> {
        let (mode, mut source, plan) = builder.into_parts();
        if source.has_classes()
            && let Some(expander) = self.class_expander.as_deref()
        {
            source.resolve_classes(expander);
        }
        let resolver = SourceResolver::new(store);
        let candidate_paths = resolver.resolve(&source)?;
        let notes = store
            .read_notes_batch(candidate_paths.iter().map(PathBuf::as_path))?;
        let (all_files, inlinks) = store.read_files_and_links()?;
        let mut matching_files = Vec::with_capacity(candidate_paths.len());
        for path in &candidate_paths {
            if let Ok(idx) = all_files.binary_search_by(|f| f.path().cmp(path))
                && let Some(f) = all_files.get(idx)
            {
                matching_files.push(f.clone());
            }
        }
        let index =
            Arc::new(FileIndex::assemble(matching_files, notes, inlinks));
        let rows = match mode {
            QueryMode::Pages => self.page_rows(&index, &source),
            QueryMode::Tasks => self.task_rows(&index, &source),
        };
        Ok(QuerySet::new(plan.run(rows)))
    }

    fn page_rows(
        &self,
        index: &Arc<FileIndex>,
        source: &SourceSelector,
    ) -> Vec<QueryRow> {
        self.matched_file_rows(index, source).collect()
    }

    fn task_rows(
        &self,
        index: &Arc<FileIndex>,
        source: &SourceSelector,
    ) -> Vec<QueryRow> {
        let mut out = Vec::new();
        for base in self.matched_file_rows(index, source) {
            let Some(note) = base.note() else {
                continue;
            };
            for item in note.tasks() {
                out.push(base.clone().with_task_item(item));
            }
        }
        out
    }

    fn matched_file_rows<'b>(
        &'b self,
        index: &'b Arc<FileIndex>,
        source: &'b SourceSelector,
    ) -> impl Iterator<Item = QueryRow> + 'b {
        (0..index.entries().len())
            .map(RowIndex::new)
            .filter(move |&position| {
                source.is_match(index.entry_at(position), &self.class_field)
            })
            .map(move |position| QueryRow::from_row(index, position))
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

/// Candidate path resolver evaluating [`SourceSelector`] expressions directly
/// against [`IndexStore`] multimap indexes and folder hierarchies.
pub(crate) struct SourceResolver<'a> {
    store: &'a IndexStore,
}

impl<'a> SourceResolver<'a> {
    /// Creates a resolver scoped to `store`.
    pub(crate) fn new(store: &'a IndexStore) -> Self {
        Self {
            store,
        }
    }

    /// Resolves matching candidate paths for `selector`.
    pub(crate) fn resolve(
        &self,
        selector: &SourceSelector,
    ) -> IndexResult<Box<[PathBuf]>> {
        match selector {
            SourceSelector::All => self.store.paths_in_folder(Path::new("")),
            SourceSelector::Expr(expr) => self.resolve_expr(expr.expr()),
        }
    }

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
        index::{FileIndex, IndexerService},
        query::{QueryBuilder, QueryRow, QuerySet, SourceSelector},
    };

    /// Runs a page-level query via [`QueryService`].
    fn query_pages(
        index: &Arc<FileIndex>,
        source: &SourceSelector,
    ) -> QuerySet {
        QueryService::new("class")
            .run(index, QueryBuilder::pages(source.clone()))
    }

    /// Task-level counterpart to [`query_pages`].
    fn query_tasks(
        index: &Arc<FileIndex>,
        source: &SourceSelector,
    ) -> QuerySet {
        QueryService::new("class")
            .run(index, QueryBuilder::tasks(source.clone()))
    }

    mod source_resolver {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn resolves_all_selector_to_all_paths() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("a.md"), "# A #tag1").expect("write a");
            fs::write(
                temp.path().join("b.md"),
                "---\nfileClass: Book\n---\n# B",
            )
            .expect("write b");
            let indexer = IndexerService::new(temp.path());
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
            let indexer = IndexerService::new(temp.path());
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
        fn resolves_class_selector_via_multimap() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(
                temp.path().join("a.md"),
                "---\nfileClass: Book\n---\n# A",
            )
            .expect("write a");
            fs::write(temp.path().join("b.md"), "# B").expect("write b");
            let indexer = IndexerService::new(temp.path());
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
            let indexer = IndexerService::new(temp.path());
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

        fn build_book_index() -> Arc<FileIndex> {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(
                temp.path().join("book.md"),
                "---\ntitle: Dune\n---\nGenre:: Sci-fi\n\nShelved as #book.",
            )
            .expect("write note");
            Arc::new(
                IndexerService::new(temp.path()).build().expect("build index"),
            )
        }

        #[test]
        fn returns_all_files_in_sorted_order() {
            let temp = tempfile::tempdir().expect("create temp dir");
            fs::write(temp.path().join("a.md"), "# A").expect("write a");
            fs::write(temp.path().join("b.md"), "# B").expect("write b");
            fs::write(temp.path().join("readme.txt"), "text")
                .expect("write txt");
            let index = Arc::new(
                IndexerService::new(temp.path()).build().expect("build index"),
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
                IndexerService::new(temp.path()).build().expect("build index"),
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
                IndexerService::new(temp.path()).build().expect("build index"),
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
                IndexerService::new(temp.path()).build().expect("build index"),
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
                IndexerService::new(temp.path()).build().expect("build index"),
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
                IndexerService::new(temp.path()).build().expect("build index"),
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
                IndexerService::new(temp.path()).build().expect("build index"),
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
                IndexerService::new(temp.path()).build().expect("build index"),
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
                IndexerService::new(temp.path()).build().expect("build index"),
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
                IndexerService::new(temp.path()).build().expect("build index"),
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
                IndexerService::new(temp.path()).build().expect("build index"),
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
                IndexerService::new(temp.path()).build().expect("build index"),
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

        /// `(completed, text)` pairs for every row in `outcome`, in order.
        fn task_rows(outcome: &QuerySet) -> Vec<(Option<bool>, &str)> {
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
                IndexerService::new(temp.path()).build().expect("build index"),
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
                IndexerService::new(temp.path()).build().expect("build index"),
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
                IndexerService::new(temp.path()).build().expect("build index"),
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
                IndexerService::new(temp.path()).build().expect("build index"),
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
                IndexerService::new(temp.path()).build().expect("build index"),
            );
            let outcome = query_tasks(&index, &SourceSelector::All);
            let row = outcome.iter().next().expect("one task row");

            assert_eq!(
                row.field("tags"),
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
                IndexerService::new(temp.path()).build().expect("build index"),
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
                IndexerService::new(temp.path()).build().expect("build index"),
            );
            let outcome = query_tasks(
                &index,
                &SourceSelector::parse("#projects").expect("valid source"),
            );

            assert_eq!(task_rows(&outcome), [(Some(false), "project task")]);
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
                IndexerService::new(temp.path()).build().expect("build index"),
            );
            let outcome = query_tasks(
                &index,
                &SourceSelector::parse("projects/").expect("valid source"),
            );

            assert_eq!(task_rows(&outcome), [(Some(false), "project task")]);
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
                IndexerService::new(temp.path()).build().expect("build index"),
            );
            let outcome = query_tasks(&index, &SourceSelector::All)
                .filter("task.completed == true")
                .expect("valid filter");

            // The Note has one complete and one incomplete task: filtering
            // must keep only the matching task row, not both rows from the
            // one Note that has at least one match.
            assert_eq!(task_rows(&outcome), [(Some(true), "pay rent")]);
        }
    }
}
