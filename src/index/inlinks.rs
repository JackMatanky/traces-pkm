//! Derived inbound link graph computed from indexed outlinks.
//!
//! [`InlinkMap`] stores deduplicated source paths keyed by each indexed target.
//! [`super::IndexerService::build`] and [`super::IndexerService::refresh`]
//! construct and persist it; query execution reads inlinks from
//! [`super::FileEntry`] through [`super::WorkspaceIndex`].
//!
//! # Link resolution
//!
//! [`LinkResolver`] resolves each [`Note`] outlink by trying an exact path, the
//! same path with an implied `.md` extension, then the nearest indexed file
//! sharing a basename-only Wikilink target. Equal-distance basename matches
//! remain unresolved.
//!
//! # Basename index design
//!
//! Most file basenames are unique, so a flat linear scan is fastest for the
//! common case. When a basename has many candidates (≥
//! [`crate::index::trie::TRIE_THRESHOLD`]), a `CandidateTrie` precomputes
//! per-folder subtree aggregates so nearest-
//! candidate queries run in `O(depth)` instead of `O(candidates)`. The trie
//! models folder containment as a tree, where
//! `FolderRef::of(a).distance_to(FolderRef::of(b)) = depth(a) + depth(b) - 2 *
//! depth(LCA(a, b))`.
//!
//! # Hash map choice
//!
//! [`FxHashMap`] is used instead of [`HashMap`] for internal indexes. The keys
//! are project-internal file paths, not attacker-controlled input, so
//! `SipHash`'s denial-of-service resistance is unnecessary. [`FxHashMap`] uses
//! a simpler, non-cryptographic hash function that avoids the per-entry
//! computational overhead of `SipHash`, yielding measurable gains at the
//! thousand-entry scale typical of project path indexes.

use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};

use rayon::prelude::*;
use rustc_hash::{FxBuildHasher, FxHashMap};

use super::trie::BaseNameIndex;
use crate::{
    BaseNameRef, FileMeta,
    note::{LinkTarget, Note},
};

/// Target-keyed inbound link graph.
///
/// Each target path maps to its deduplicated, canonically sorted inbound source
/// paths. Construction through [`InlinkMap::new`] enforces:
///
/// 1. Every target key has at least one inbound source.
/// 2. Sources for each target are strictly deduplicated.
/// 3. Sources for each target are canonically sorted in ascending path order.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct InlinkMap(HashMap<PathBuf, Box<[PathBuf]>>);

impl InlinkMap {
    /// Builds a canonical, validated inlink graph.
    ///
    /// Resolves note outlinks and non-note attachment targets. Multiple
    /// outlinks from the same source note to the same target collapse into one
    /// edge.
    #[inline]
    #[must_use]
    pub fn new(notes: &[Note], files: &[FileMeta]) -> Self {
        let resolver = LinkResolver::new(files);
        let mut flat_edges: Vec<(Target<'_>, Source<'_>)> = notes
            .par_iter()
            .flat_map_iter(|source| {
                let src = Source(source.path());
                resolver
                    .resolve_note(source)
                    .into_iter()
                    .map(move |target| (target, src))
            })
            .collect();

        flat_edges.sort_unstable();
        flat_edges.dedup();
        Self::from_flat_edges(flat_edges)
    }

    /// Groups sorted, deduplicated edge pairs into canonical source lists.
    fn from_flat_edges(flat_edges: Vec<(Target<'_>, Source<'_>)>) -> Self {
        let mut edges: HashMap<PathBuf, Box<[PathBuf]>> = HashMap::new();
        let mut current_target: Option<Target<'_>> = None;
        let mut current_sources: Vec<PathBuf> = Vec::new();

        for (target, source) in flat_edges {
            if Some(target) != current_target {
                if let Some(prev_target) = current_target {
                    edges.insert(
                        prev_target.to_path_buf(),
                        current_sources.into_boxed_slice(),
                    );
                    current_sources = Vec::new();
                }
                current_target = Some(target);
            }
            current_sources.push(source.to_path_buf());
        }
        if let Some(prev_target) = current_target {
            edges.insert(
                prev_target.to_path_buf(),
                current_sources.into_boxed_slice(),
            );
        }

        Self(edges)
    }

    /// Returns inbound sources for `target`.
    ///
    /// Returns an empty slice when `target` has no inbound links.
    #[inline]
    #[must_use]
    pub fn inlinks_of(&self, target: &Path) -> &[PathBuf] {
        self.0.get(target).map_or(&[], |sources| sources.as_ref())
    }

    /// Returns an iterator over `(target, sources)` pairs for every entry.
    #[inline]
    pub fn iter(&self) -> impl Iterator<Item = (&Path, &[PathBuf])> {
        self.0
            .iter()
            .map(|(target, sources)| (target.as_path(), sources.as_ref()))
    }

    /// Consumes the map into an iterator of owned `(target, sources)` pairs.
    #[inline]
    pub fn into_entries(
        self,
    ) -> impl Iterator<Item = (PathBuf, Box<[PathBuf]>)> {
        self.0.into_iter()
    }

    /// Returns `true` if `target` has recorded inbound links.
    #[cfg_attr(
        not(any(test, feature = "test-utils")),
        expect(
            dead_code,
            reason = "part of the InlinkMap test/bench introspection surface, \
                      gated the same as InlinkMap's own export"
        )
    )]
    #[inline]
    #[must_use]
    pub fn has_target(&self, target: &Path) -> bool {
        self.0.contains_key(target)
    }

    /// Returns `true` if no targets have recorded inbound links.
    #[cfg_attr(
        not(any(test, feature = "test-utils")),
        expect(
            dead_code,
            reason = "part of the InlinkMap test/bench introspection surface, \
                      gated the same as InlinkMap's own export"
        )
    )]
    #[inline]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Returns the number of targets with recorded inbound links.
    #[cfg_attr(
        not(any(test, feature = "test-utils")),
        expect(
            dead_code,
            reason = "part of the InlinkMap test/bench introspection surface, \
                      gated the same as InlinkMap's own export"
        )
    )]
    #[inline]
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Reconstructs trusted persisted inlink storage.
    #[inline]
    #[must_use]
    pub(super) fn from_raw(edges: HashMap<PathBuf, Box<[PathBuf]>>) -> Self {
        Self::from(edges)
    }

    /// Removes edges from `sources`, dropping targets left without inlinks.
    ///
    /// `&self` lets incremental refresh diff against the original graph; the
    /// implementation copies only retained sources instead of cloning the whole
    /// map up front.
    #[inline]
    #[must_use]
    pub(super) fn without_sources(&self, sources: &HashSet<&Path>) -> Self {
        let mut edges = HashMap::with_capacity(self.0.len());
        for (target, srcs) in &self.0 {
            let mut filtered = Vec::with_capacity(srcs.len());
            for source in srcs {
                if !sources.contains(source.as_path()) {
                    filtered.push(source.clone());
                }
            }
            if !filtered.is_empty() {
                edges.insert(target.clone(), filtered.into_boxed_slice());
            }
        }
        Self(edges)
    }

    /// Inserts edge pairs into sorted, deduplicated per-target source lists.
    #[inline]
    #[must_use]
    pub(super) fn with_edges(
        mut self,
        edges: impl IntoIterator<Item = (PathBuf, PathBuf)>,
    ) -> Self {
        for (target, source) in edges {
            let slot = self.0.entry(target).or_default();
            let mut sources = std::mem::take(slot).into_vec();
            if let Err(pos) = sources.binary_search(&source) {
                sources.insert(pos, source);
            }
            *slot = sources.into_boxed_slice();
        }
        self
    }
}

impl From<HashMap<PathBuf, Box<[PathBuf]>>> for InlinkMap {
    #[inline]
    fn from(edges: HashMap<PathBuf, Box<[PathBuf]>>) -> Self {
        Self(edges)
    }
}

/// Resolves notes into edge pairs for [`InlinkMap::with_edges`].
///
/// Shares one [`LinkResolver`] so batch refresh pays basename-index
/// construction once.
pub(super) fn resolve_edges_for(
    notes: &[Note],
    files: &[FileMeta],
) -> Vec<(PathBuf, PathBuf)> {
    let resolver = LinkResolver::new(files);
    let edge_capacity = notes.iter().map(|note| note.outlinks().len()).sum();
    let mut edges = Vec::with_capacity(edge_capacity);
    for note in notes {
        for target in resolver.resolve_note(note) {
            edges.push((target.to_path_buf(), note.path().to_path_buf()));
        }
    }
    edges
}

/// Path and basename index used during link resolution.
struct LinkResolver<'a> {
    files: &'a [FileMeta],
    basename_index: FxHashMap<BaseNameRef<'a>, BaseNameIndex<'a>>,
}

impl<'a> LinkResolver<'a> {
    /// Indexes file basenames in one `O(n)` pass.
    fn new(files: &'a [FileMeta]) -> Self {
        let mut by_basename: FxHashMap<BaseNameRef<'a>, Vec<&'a Path>> =
            FxHashMap::with_capacity_and_hasher(files.len(), FxBuildHasher);
        for file in files {
            let path = file.path();
            if let Some(basename) = BaseNameRef::from_path(path) {
                by_basename.entry(basename).or_default().push(path);
            }
        }
        let basename_index = by_basename
            .into_iter()
            .map(|(basename, candidates)| {
                (basename, BaseNameIndex::build(candidates))
            })
            .collect();
        Self {
            files,
            basename_index,
        }
    }

    /// Resolves one note's outlinks to deduplicated target paths.
    ///
    /// Pure and I/O-free; callers can share one resolver across many notes
    /// instead of rebuilding the basename index per note.
    fn resolve_note(&self, note: &Note) -> Vec<Target<'a>> {
        let mut targets: Vec<Target<'a>> = note
            .outlinks()
            .iter()
            .filter_map(|outlink| {
                self.resolve(note.path(), outlink.target_parts())
            })
            .collect();
        targets.sort_unstable();
        targets.dedup();
        targets
    }

    /// Resolves a split [`LinkTarget`] to an indexed file path.
    ///
    /// Tries exact path, missing-`.md` path, then nearest basename match
    /// for Obsidian-style Wikilinks. Equal-distance basename ties stay
    /// unresolved.
    fn resolve(
        &self,
        from: &Path,
        target: LinkTarget<'_>,
    ) -> Option<Target<'a>> {
        if !target.has_path() {
            return None;
        }
        let path_part = target.path()?;
        let candidate = Path::new(path_part);
        if let Some(path) = self.get_by_path(candidate) {
            return Some(Target(path));
        }
        if candidate.extension().is_none() {
            let with_extension = candidate.with_extension("md");
            if let Some(path) = self.get_by_path(&with_extension) {
                return Some(Target(path));
            }
        }
        if !target.is_basename() {
            return None;
        }
        let basename =
            candidate.file_stem().and_then(|s| s.to_str()).unwrap_or(path_part);
        let target_ext = candidate.extension().and_then(|s| s.to_str());
        if let Some(ext) = target_ext
            && let Some(path) =
                self.nearest_by_basename(basename, from, Some(ext))
        {
            return Some(Target(path));
        }
        self.nearest_by_basename(basename, from, None).map(Target)
    }

    fn nearest_by_basename(
        &self,
        basename: &str,
        from: &Path,
        target_ext: Option<&str>,
    ) -> Option<&'a Path> {
        self.basename_index.get(basename)?.nearest(from, target_ext)
    }

    fn get_by_path(&self, path: &Path) -> Option<&'a Path> {
        self.files
            .binary_search_by(|file| file.path().cmp(path))
            .ok()
            .and_then(|i| self.files.get(i))
            .map(FileMeta::path)
    }
}

/// A resolved link target: the path of the file an outlink points to.
#[derive(Copy, Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
struct Target<'a>(&'a Path);

impl Target<'_> {
    fn to_path_buf(self) -> PathBuf {
        self.0.to_path_buf()
    }
}

/// A [`Note`] that links to a [`Target`]: the path recorded as an inbound edge.
#[derive(Copy, Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
struct Source<'a>(&'a Path);

impl Source<'_> {
    fn to_path_buf(self) -> PathBuf {
        self.0.to_path_buf()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{file::FileFormat, note::LinkType, parse_note as parse};

    fn file_for_note(path: &str) -> FileMeta {
        FileMeta::note_for_test(path)
    }

    fn file_for_attachment(path: &str, format: FileFormat) -> FileMeta {
        FileMeta::for_test(path, format)
    }

    fn note_with_outlink(path: &str, target: &str, kind: LinkType) -> Note {
        let src = match kind {
            LinkType::Wikilink => format!("[[{target}]]"),
            LinkType::Markdown => format!("[link]({target})"),
        };
        parse(path, &src)
    }

    mod link_resolver {

        use super::*;

        fn resolve<'a>(
            files: &'a [FileMeta],
            from: &str,
            target: LinkTarget<'_>,
        ) -> Option<Target<'a>> {
            let resolver = LinkResolver::new(files);
            resolver.resolve(Path::new(from), target)
        }

        fn files_from_notes(paths: &[&str]) -> Vec<FileMeta> {
            let mut files: Vec<FileMeta> =
                paths.iter().map(|p| file_for_note(p)).collect();
            files.sort_by(|a, b| a.path().cmp(b.path()));
            files
        }

        mod exact_path {
            use pretty_assertions::assert_eq;

            use super::*;

            #[test]
            fn resolves_exact_root_relative_markdown_path() {
                let files = files_from_notes(&["notes/other.md"]);

                assert_eq!(
                    resolve(
                        &files,
                        "linking.md",
                        LinkTarget::Path("notes/other.md")
                    ),
                    Some(Target(Path::new("notes/other.md")))
                );
            }

            #[test]
            fn resolves_markdown_path_missing_extension() {
                let files = files_from_notes(&["other.md"]);

                assert_eq!(
                    resolve(&files, "linking.md", LinkTarget::Path("other")),
                    Some(Target(Path::new("other.md")))
                );
            }

            #[test]
            fn resolves_path_with_anchor_by_path_segment() {
                let files = files_from_notes(&["other.md"]);

                assert_eq!(
                    resolve(
                        &files,
                        "linking.md",
                        LinkTarget::PathWithAnchor("other", "Some Heading")
                    ),
                    Some(Target(Path::new("other.md")))
                );
            }
        }

        mod basename_fallback {
            use super::*;

            mod nearest_wins {
                use pretty_assertions::assert_eq;

                use super::*;

                #[test]
                fn resolves_wikilink_by_unique_file_stem() {
                    let files = files_from_notes(&[
                        "notes/Project Alpha.md",
                        "notes/other.md",
                    ]);

                    assert_eq!(
                        resolve(
                            &files,
                            "linking.md",
                            LinkTarget::Path("Project Alpha")
                        ),
                        Some(Target(Path::new("notes/Project Alpha.md")))
                    );
                }

                #[test]
                fn resolves_ambiguous_basename_match_to_nearest_candidate() {
                    let files = files_from_notes(&[
                        "notes/a/note.md",
                        "notes/b/note.md",
                    ]);

                    assert_eq!(
                        resolve(
                            &files,
                            "notes/a/linking.md",
                            LinkTarget::Path("note")
                        ),
                        Some(Target(Path::new("notes/a/note.md")))
                    );
                }

                #[test]
                fn resolves_to_nearer_candidate_despite_farther_tie() {
                    let files = files_from_notes(&[
                        "far/a/note.md",
                        "far/b/note.md",
                        "near/note.md",
                    ]);

                    assert_eq!(
                        resolve(
                            &files,
                            "near/linking.md",
                            LinkTarget::Path("note")
                        ),
                        Some(Target(Path::new("near/note.md")))
                    );
                }

                #[test]
                fn resolves_to_a_distant_shallow_sibling_over_a_close_deep_nesting()
                 {
                    // The shallow root sibling is nearer (2) than the deeply
                    // nested shared-ancestor candidate (3), even though the
                    // latter shares a non-root ancestor with `from`.
                    let files = files_from_notes(&[
                        "shared/deep/nested/sub/note.md",
                        "far/note.md",
                    ]);

                    assert_eq!(
                        resolve(
                            &files,
                            "shared/linking.md",
                            LinkTarget::Path("note")
                        ),
                        Some(Target(Path::new("far/note.md")))
                    );
                }
            }

            mod ties_resolve_to_none {
                use pretty_assertions::assert_eq;

                use super::*;

                #[test]
                fn returns_none_for_ambiguous_basename_match_at_equal_distance()
                {
                    let files = files_from_notes(&["a/note.md", "b/note.md"]);

                    assert_eq!(
                        resolve(&files, "linking.md", LinkTarget::Path("note")),
                        None
                    );
                }

                #[test]
                fn resolves_none_for_a_tie_spanning_an_ancestor_and_a_descendant_branch()
                 {
                    // Exercises an asymmetric tie: one candidate below
                    // `from`'s ancestor, one below a disjoint root folder.
                    // Both are true distance 2, so the exclusion aggregate
                    // must combine shallow and newly visible candidates
                    // without double-counting either.
                    let files = files_from_notes(&[
                        "shared/a/b/note.md",
                        "other/note.md",
                    ]);

                    assert_eq!(
                        resolve(
                            &files,
                            "shared/linking.md",
                            LinkTarget::Path("note")
                        ),
                        None
                    );
                }
            }

            mod self_reference {
                use pretty_assertions::assert_eq;

                use super::*;

                #[test]
                fn resolves_ambiguous_self_referential_basename_to_itself() {
                    let files = files_from_notes(&["a.md", "b/a.md"]);

                    assert_eq!(
                        resolve(&files, "a.md", LinkTarget::Path("a")),
                        Some(Target(Path::new("a.md")))
                    );
                }
            }

            mod extension_narrowing {
                use pretty_assertions::assert_eq;

                use super::*;

                #[test]
                fn resolves_basename_with_extension() {
                    let files = files_from_notes(&["notes/report.md"]);

                    assert_eq!(
                        resolve(
                            &files,
                            "linking.md",
                            LinkTarget::Path("report.txt")
                        ),
                        Some(Target(Path::new("notes/report.md")))
                    );
                }

                #[test]
                fn resolves_basename_with_matching_extension() {
                    let mut files = files_from_notes(&[
                        "near/linking.md",
                        "near/report.md",
                    ]);
                    files.push(file_for_attachment(
                        "far/report.txt",
                        FileFormat::Other,
                    ));
                    files.sort_by(|a, b| a.path().cmp(b.path()));

                    assert_eq!(
                        resolve(
                            &files,
                            "near/linking.md",
                            LinkTarget::Path("report.txt")
                        ),
                        Some(Target(Path::new("far/report.txt")))
                    );
                }
            }
        }

        mod attachments {
            use pretty_assertions::assert_eq;

            use super::*;

            #[test]
            fn resolves_links_to_image_attachments() {
                let mut files = files_from_notes(&["note.md"]);
                files.push(file_for_attachment(
                    "assets/diagram.png",
                    FileFormat::Other,
                ));
                files.sort_by(|a, b| a.path().cmp(b.path()));

                assert_eq!(
                    resolve(&files, "note.md", LinkTarget::Path("diagram.png")),
                    Some(Target(Path::new("assets/diagram.png")))
                );
            }

            #[test]
            fn resolves_links_to_pdf_attachments() {
                let mut files = files_from_notes(&["note.md"]);
                files.push(file_for_attachment(
                    "docs/specification.pdf",
                    FileFormat::Other,
                ));
                files.sort_by(|a, b| a.path().cmp(b.path()));

                assert_eq!(
                    resolve(
                        &files,
                        "note.md",
                        LinkTarget::Path("specification.pdf")
                    ),
                    Some(Target(Path::new("docs/specification.pdf")))
                );
            }
        }

        mod unresolvable {
            use pretty_assertions::assert_eq;

            use super::*;

            #[test]
            fn skips_basename_fallback_for_qualified_paths() {
                let files =
                    files_from_notes(&["archive/foo.md", "notes/bar.md"]);

                assert_eq!(
                    resolve(
                        &files,
                        "linking.md",
                        LinkTarget::Path("notes/foo")
                    ),
                    None
                );
            }

            #[test]
            fn returns_none_for_basename_matching_no_indexed_file() {
                let files = files_from_notes(&["other.md"]);

                assert_eq!(
                    resolve(
                        &files,
                        "linking.md",
                        LinkTarget::Path("nonexistent")
                    ),
                    None
                );
            }

            #[test]
            fn returns_none_for_unresolvable_url_target() {
                let files = files_from_notes(&["other.md"]);

                assert_eq!(
                    resolve(
                        &files,
                        "linking.md",
                        LinkTarget::Path("https://example.com")
                    ),
                    None
                );
            }

            #[test]
            fn returns_none_for_anchor_only_target() {
                let files = files_from_notes(&["other.md"]);

                assert_eq!(
                    resolve(
                        &files,
                        "linking.md",
                        LinkTarget::AnchorOnly("Some Heading")
                    ),
                    None
                );
            }
        }
    }

    mod inlink_map {
        use super::*;

        mod construction {
            use pretty_assertions::assert_eq;

            use super::*;

            fn build_graph(
                notes: &[Note],
                extra_files: &[FileMeta],
            ) -> InlinkMap {
                let mut files: Vec<FileMeta> = notes
                    .iter()
                    .map(|n| file_for_note(n.path().to_str().unwrap()))
                    .collect();
                files.extend_from_slice(extra_files);
                files.sort_by(|a, b| a.path().cmp(b.path()));
                InlinkMap::new(notes, &files)
            }

            #[test]
            fn maps_target_to_single_linking_note() {
                let notes = [
                    note_with_outlink("a.md", "b", LinkType::Wikilink),
                    parse("b.md", "# B"),
                ];

                let inlinks = build_graph(&notes, &[]);

                assert_eq!(inlinks.inlinks_of(Path::new("b.md")), [
                    PathBuf::from("a.md")
                ]);
            }

            #[test]
            fn maps_target_to_every_linking_note() {
                let notes = [
                    note_with_outlink("a.md", "target", LinkType::Wikilink),
                    note_with_outlink("b.md", "target", LinkType::Wikilink),
                    parse("target.md", "# Target"),
                ];

                let inlinks = build_graph(&notes, &[]);

                assert_eq!(inlinks.inlinks_of(Path::new("target.md")), [
                    PathBuf::from("a.md"),
                    PathBuf::from("b.md")
                ]);
            }

            #[test]
            fn collapses_duplicate_outlinks_within_one_note_to_one_edge() {
                let note = parse("a.md", "[[b]] and [[b]] again");
                let notes = [note, parse("b.md", "# B")];

                let inlinks = build_graph(&notes, &[]);

                assert_eq!(inlinks.inlinks_of(Path::new("b.md")), [
                    PathBuf::from("a.md")
                ]);
            }

            #[test]
            fn records_self_link_without_corrupting_other_edges() {
                let notes = [
                    note_with_outlink("a.md", "a", LinkType::Wikilink),
                    note_with_outlink("b.md", "a", LinkType::Wikilink),
                ];

                let inlinks = build_graph(&notes, &[]);

                assert_eq!(inlinks.inlinks_of(Path::new("a.md")), [
                    PathBuf::from("a.md"),
                    PathBuf::from("b.md")
                ]);
            }

            #[test]
            fn omits_notes_with_no_inbound_links() {
                let notes = [parse("lonely.md", "# Lonely")];

                let inlinks = build_graph(&notes, &[]);

                assert!(!inlinks.has_target(Path::new("lonely.md")));
                assert_eq!(
                    inlinks.inlinks_of(Path::new("lonely.md")),
                    Vec::<std::path::PathBuf>::new()
                );
            }

            #[test]
            fn ignores_outlinks_that_do_not_resolve_to_any_file() {
                let notes = [note_with_outlink(
                    "a.md",
                    "https://example.com",
                    LinkType::Markdown,
                )];

                let inlinks = build_graph(&notes, &[]);

                assert!(inlinks.is_empty());
                assert_eq!(inlinks.len(), 0);
            }

            #[test]
            fn resolves_ambiguous_wikilink_to_nearest_note() {
                let notes = [
                    note_with_outlink(
                        "notes/a/linking.md",
                        "note",
                        LinkType::Wikilink,
                    ),
                    parse("notes/a/note.md", "# Near"),
                    parse("notes/b/note.md", "# Far"),
                ];

                let inlinks = build_graph(&notes, &[]);

                assert_eq!(inlinks.inlinks_of(Path::new("notes/a/note.md")), [
                    PathBuf::from("notes/a/linking.md")
                ]);
                assert_eq!(
                    inlinks.inlinks_of(Path::new("notes/b/note.md")),
                    Vec::<std::path::PathBuf>::new()
                );
            }

            #[test]
            fn indexes_non_note_attachments_as_inlink_targets() {
                let notes = [
                    note_with_outlink(
                        "note1.md",
                        "chart.png",
                        LinkType::Wikilink,
                    ),
                    note_with_outlink(
                        "note2.md",
                        "chart.png",
                        LinkType::Wikilink,
                    ),
                ];
                let attachments = [file_for_attachment(
                    "images/chart.png",
                    FileFormat::Other,
                )];

                let inlinks = build_graph(&notes, &attachments);

                assert_eq!(
                    inlinks.inlinks_of(Path::new("images/chart.png")),
                    [PathBuf::from("note1.md"), PathBuf::from("note2.md")]
                );
            }
        }

        mod invariants {
            use super::*;

            #[test]
            fn inbound_sources_are_always_sorted_in_ascending_path_order() {
                let notes = [
                    note_with_outlink("z.md", "target", LinkType::Wikilink),
                    note_with_outlink("a.md", "target", LinkType::Wikilink),
                    note_with_outlink("m.md", "target", LinkType::Wikilink),
                    parse("target.md", "# Target"),
                ];

                let mut files: Vec<FileMeta> = notes
                    .iter()
                    .map(|n| file_for_note(n.path().to_str().unwrap()))
                    .collect();
                files.sort_by(|a, b| a.path().cmp(b.path()));
                let inlinks = InlinkMap::new(&notes, &files);

                let sources = inlinks.inlinks_of(Path::new("target.md"));
                assert_eq!(sources, [
                    PathBuf::from("a.md"),
                    PathBuf::from("m.md"),
                    PathBuf::from("z.md"),
                ]);
            }

            #[test]
            fn every_target_in_map_has_at_least_one_source() {
                use pretty_assertions::{assert_eq, assert_ne};
                let notes = [
                    note_with_outlink("a.md", "target", LinkType::Wikilink),
                    parse("target.md", "# Target"),
                    parse("standalone.md", "# Standalone"),
                ];

                let mut files: Vec<FileMeta> = notes
                    .iter()
                    .map(|n| file_for_note(n.path().to_str().unwrap()))
                    .collect();
                files.sort_by(|a, b| a.path().cmp(b.path()));
                let inlinks = InlinkMap::new(&notes, &files);

                assert_eq!(inlinks.len(), 1);

                for (_, sources) in inlinks.iter() {
                    assert_ne!(sources, Vec::<std::path::PathBuf>::new());
                }
            }
        }

        mod accessors {
            use super::*;

            #[test]
            fn inlinks_of_returns_sources_for_known_target() {
                let mut raw = HashMap::new();
                raw.insert(
                    PathBuf::from("target.md"),
                    vec![PathBuf::from("src.md")].into_boxed_slice(),
                );
                let inlinks = InlinkMap::from_raw(raw);

                assert_eq!(inlinks.inlinks_of(Path::new("target.md")), [
                    PathBuf::from("src.md")
                ]);
            }

            #[test]
            fn inlinks_of_returns_empty_slice_for_unknown_target() {
                use pretty_assertions::assert_eq;
                let inlinks = InlinkMap::default();
                assert_eq!(
                    inlinks.inlinks_of(Path::new("nonexistent.md")),
                    Vec::<std::path::PathBuf>::new()
                );
            }

            #[test]
            fn has_target_checks_presence() {
                let mut raw = HashMap::new();
                raw.insert(
                    PathBuf::from("target.md"),
                    vec![PathBuf::from("src.md")].into_boxed_slice(),
                );
                let inlinks = InlinkMap::from_raw(raw);

                assert!(inlinks.has_target(Path::new("target.md")));
                assert!(!inlinks.has_target(Path::new("other.md")));
            }

            #[test]
            fn iter_and_len_reflect_graph_cardinality() {
                let mut raw = HashMap::new();
                raw.insert(
                    PathBuf::from("t1.md"),
                    vec![PathBuf::from("s1.md")].into_boxed_slice(),
                );
                raw.insert(
                    PathBuf::from("t2.md"),
                    vec![PathBuf::from("s2.md")].into_boxed_slice(),
                );
                let inlinks = InlinkMap::from_raw(raw);

                assert_eq!(inlinks.len(), 2);
                assert!(!inlinks.is_empty());

                let collected: HashMap<_, _> = inlinks
                    .iter()
                    .map(|(target, sources)| {
                        (target.to_path_buf(), sources.to_vec())
                    })
                    .collect();
                assert_eq!(collected.len(), 2);
            }

            #[test]
            fn into_entries_yields_owned_entries() {
                let mut raw = HashMap::new();
                raw.insert(
                    PathBuf::from("target.md"),
                    vec![PathBuf::from("source.md")].into_boxed_slice(),
                );
                let inlinks = InlinkMap::from_raw(raw);
                let mut entries = inlinks.into_entries();
                let (target, sources) = entries.next().expect("one entry");
                assert_eq!(target, PathBuf::from("target.md"));
                assert_eq!(sources.as_ref(), [PathBuf::from("source.md")]);
                assert!(entries.next().is_none());
            }
        }

        mod refresh_support {
            use super::*;

            mod without_sources {
                use pretty_assertions::assert_eq;

                use super::*;

                fn graph_with_two_sources() -> InlinkMap {
                    let mut raw = HashMap::new();
                    raw.insert(
                        PathBuf::from("target.md"),
                        vec![PathBuf::from("a.md"), PathBuf::from("b.md")]
                            .into_boxed_slice(),
                    );
                    InlinkMap::from_raw(raw)
                }

                #[test]
                fn removing_the_only_source_drops_the_target() {
                    let mut raw = HashMap::new();
                    raw.insert(
                        PathBuf::from("target.md"),
                        vec![PathBuf::from("a.md")].into_boxed_slice(),
                    );
                    let inlinks = InlinkMap::from_raw(raw);
                    let stale: HashSet<&Path> =
                        std::iter::once(Path::new("a.md")).collect();

                    let patched = inlinks.without_sources(&stale);

                    assert!(!patched.has_target(Path::new("target.md")));
                }

                #[test]
                fn removing_one_of_several_sources_keeps_the_target() {
                    let inlinks = graph_with_two_sources();
                    let stale: HashSet<&Path> =
                        std::iter::once(Path::new("a.md")).collect();

                    let patched = inlinks.without_sources(&stale);

                    assert_eq!(patched.inlinks_of(Path::new("target.md")), [
                        PathBuf::from("b.md")
                    ]);
                }

                #[test]
                fn removing_an_absent_source_keeps_the_graph_unchanged() {
                    let inlinks = graph_with_two_sources();
                    let stale: HashSet<&Path> =
                        std::iter::once(Path::new("missing.md")).collect();

                    let patched = inlinks.without_sources(&stale);

                    assert_eq!(patched.inlinks_of(Path::new("target.md")), [
                        PathBuf::from("a.md"),
                        PathBuf::from("b.md")
                    ]);
                }
            }

            mod with_edges {
                use pretty_assertions::assert_eq;

                use super::*;

                #[test]
                fn adding_an_edge_to_an_existing_target_appends_and_resorts() {
                    let mut raw = HashMap::new();
                    raw.insert(
                        PathBuf::from("target.md"),
                        vec![PathBuf::from("z.md")].into_boxed_slice(),
                    );
                    let inlinks = InlinkMap::from_raw(raw);

                    let patched = inlinks.with_edges([(
                        PathBuf::from("target.md"),
                        PathBuf::from("a.md"),
                    )]);

                    assert_eq!(patched.inlinks_of(Path::new("target.md")), [
                        PathBuf::from("a.md"),
                        PathBuf::from("z.md")
                    ]);
                }

                #[test]
                fn adding_an_edge_to_a_new_target_inserts_the_source() {
                    let patched = InlinkMap::default().with_edges([(
                        PathBuf::from("target.md"),
                        PathBuf::from("source.md"),
                    )]);

                    assert_eq!(patched.inlinks_of(Path::new("target.md")), [
                        PathBuf::from("source.md")
                    ]);
                }

                #[test]
                fn adding_a_duplicate_edge_keeps_sources_deduplicated() {
                    let mut raw = HashMap::new();
                    raw.insert(
                        PathBuf::from("target.md"),
                        vec![PathBuf::from("a.md"), PathBuf::from("z.md")]
                            .into_boxed_slice(),
                    );
                    let inlinks = InlinkMap::from_raw(raw);

                    let patched = inlinks.with_edges([(
                        PathBuf::from("target.md"),
                        PathBuf::from("a.md"),
                    )]);

                    assert_eq!(patched.inlinks_of(Path::new("target.md")), [
                        PathBuf::from("a.md"),
                        PathBuf::from("z.md")
                    ]);
                }
            }
        }
    }

    mod folder_ref_distance {
        use pretty_assertions::assert_eq;

        use super::*;
        use crate::path::FolderRef;

        #[test]
        fn same_folder_has_zero_distance() {
            assert_eq!(
                FolderRef::of(Path::new("folder/a.md"))
                    .distance_to(FolderRef::of(Path::new("folder/b.md"))),
                0
            );
        }

        #[test]
        fn parent_and_child_have_distance_one() {
            assert_eq!(
                FolderRef::of(Path::new("folder/sub/a.md"))
                    .distance_to(FolderRef::of(Path::new("folder/b.md"))),
                1
            );
            assert_eq!(
                FolderRef::of(Path::new("folder/a.md"))
                    .distance_to(FolderRef::of(Path::new("folder/sub/b.md"))),
                1
            );
        }

        #[test]
        fn siblings_have_distance_two() {
            assert_eq!(
                FolderRef::of(Path::new("folder/sub1/a.md"))
                    .distance_to(FolderRef::of(Path::new("folder/sub2/b.md"))),
                2
            );
        }

        #[test]
        fn different_subtrees_step_through_common_ancestor() {
            assert_eq!(
                FolderRef::of(Path::new("a/b/c/file.md"))
                    .distance_to(FolderRef::of(Path::new("a/d/e/file.md"))),
                4
            );
        }

        #[test]
        fn root_and_nested_folder_compute_correct_distance() {
            assert_eq!(
                FolderRef::of(Path::new("root.md"))
                    .distance_to(FolderRef::of(Path::new("a/b/c/nested.md"))),
                3
            );
            assert_eq!(
                FolderRef::of(Path::new("a/b/c/nested.md"))
                    .distance_to(FolderRef::of(Path::new("root.md"))),
                3
            );
        }
    }

    mod basename_index {
        use super::*;
        use crate::index::trie::TRIE_THRESHOLD;

        mod dispatch {
            use pretty_assertions::assert_eq;

            use super::*;

            fn same_basename_files(count: usize) -> Vec<FileMeta> {
                let paths: Vec<String> =
                    (0..count).map(|i| format!("d{i}/note.md")).collect();
                let mut files: Vec<FileMeta> =
                    paths.iter().map(|p| file_for_note(p)).collect();
                files.sort_by(|a, b| a.path().cmp(b.path()));
                files
            }

            #[test]
            fn resolves_identically_at_the_trie_threshold_boundary() {
                for count in [TRIE_THRESHOLD - 1, TRIE_THRESHOLD] {
                    let files = same_basename_files(count);
                    let resolver = LinkResolver::new(&files);

                    assert_eq!(
                        resolver.resolve(
                            Path::new("d0/linking.md"),
                            LinkTarget::Path("note"),
                        ),
                        Some(Target(Path::new("d0/note.md"))),
                        "candidate count = {count}"
                    );
                }
            }
        }
    }

    mod resolve_edges_for {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn preserves_resolution_order_and_cross_note_duplicate_edges() {
            let notes = [
                note_with_outlink("z.md", "b", LinkType::Wikilink),
                note_with_outlink("y.md", "a", LinkType::Wikilink),
                note_with_outlink("w.md", "b", LinkType::Wikilink),
                note_with_outlink("v.md", "missing", LinkType::Wikilink),
            ];
            let mut files: Vec<FileMeta> =
                ["z.md", "y.md", "w.md", "v.md", "a.md", "b.md"]
                    .iter()
                    .map(|p| file_for_note(p))
                    .collect();
            files.sort_by(|a, b| a.path().cmp(b.path()));

            let edges = resolve_edges_for(&notes, &files);

            assert_eq!(edges, [
                (PathBuf::from("b.md"), PathBuf::from("z.md")),
                (PathBuf::from("a.md"), PathBuf::from("y.md")),
                (PathBuf::from("b.md"), PathBuf::from("w.md")),
            ]);
        }
    }
}
