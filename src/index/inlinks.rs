//! Derived inbound links computed from indexed outlinks.
//!
//! [`InlinkMap`] represents the full graph of inbound links (backlinks)
//! pointing to each indexed file or note in the personal knowledge base.
//!
//! - [`super::IndexerService::build`] and [`super::IndexerService::refresh`]
//!   construct an [`InlinkMap`] and persist it into the database.
//! - Query execution and [`super::FileIndex`] distribute inbound links directly
//!   to [`super::FileEntry`].

use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};

use rayon::prelude::*;

use crate::{
    BaseNameRef, FileBase,
    note::{LinkTarget, Note},
};

/// Target-keyed inbound link graph: maps each target path to its deduplicated,
/// canonically sorted inbound source paths.
///
/// Invariants enforced by construction ([`InlinkMap::new`]):
/// 1. Every target key has at least one inbound source.
/// 2. Sources for each target are strictly deduplicated.
/// 3. Sources for each target are canonically sorted in ascending path order.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct InlinkMap(HashMap<PathBuf, Box<[PathBuf]>>);

impl InlinkMap {
    /// Parses vault notes and file records into a canonical, validated inlink
    /// graph.
    ///
    /// Resolves both internal Note outlinks and non-note attachment targets
    /// (images, PDFs, audio). Edges pointing to identical targets within
    /// the same source Note are deduplicated into a single edge.
    #[inline]
    #[must_use]
    pub fn new(notes: &[Note], files: &[FileBase]) -> Self {
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

    /// Groups pre-sorted, deduplicated `(target, source)` pairs into the
    /// canonical per-target source-list representation.
    fn from_flat_edges(flat_edges: Vec<(Target<'_>, Source<'_>)>) -> Self {
        let mut edges: HashMap<PathBuf, Box<[PathBuf]>> = HashMap::new();
        let mut current_target: Option<Target<'_>> = None;
        let mut current_sources: Vec<PathBuf> = Vec::new();

        for (target, source) in flat_edges {
            if Some(target) == current_target {
                current_sources.push(source.to_path_buf());
            } else {
                if let Some(prev_target) = current_target {
                    edges.insert(
                        prev_target.to_path_buf(),
                        current_sources.into_boxed_slice(),
                    );
                    current_sources = Vec::new();
                }
                current_target = Some(target);
                current_sources.push(source.to_path_buf());
            }
        }
        if let Some(prev_target) = current_target {
            edges.insert(
                prev_target.to_path_buf(),
                current_sources.into_boxed_slice(),
            );
        }

        Self(edges)
    }

    /// Reconstructs an [`InlinkMap`] from trusted, pre-sorted persistence
    /// storage.
    #[inline]
    #[must_use]
    pub(super) fn from_raw(edges: HashMap<PathBuf, Box<[PathBuf]>>) -> Self {
        Self(edges)
    }

    /// Returns the inbound link sources pointing to `target`.
    ///
    /// If `target` has no inbound links, returns an empty slice.
    #[inline]
    #[must_use]
    pub fn inlinks_of(&self, target: &Path) -> &[PathBuf] {
        self.0.get(target).map_or(&[], |sources| sources.as_ref())
    }

    /// Returns `true` if `target` has at least one inbound link.
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
    pub fn contains_target(&self, target: &Path) -> bool {
        self.0.contains_key(target)
    }

    /// Returns an iterator over all `(target, sources)` pairs in the graph.
    #[inline]
    pub fn iter(&self) -> impl Iterator<Item = (&Path, &[PathBuf])> {
        self.0
            .iter()
            .map(|(target, sources)| (target.as_path(), sources.as_ref()))
    }

    /// Consumes the map into an iterator yielding owned `(target, sources)`
    /// pairs.
    #[inline]
    pub fn into_entries(
        self,
    ) -> impl Iterator<Item = (PathBuf, Box<[PathBuf]>)> {
        self.0.into_iter()
    }

    /// Returns `true` if the graph contains no inbound link edges.
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

    /// Returns the number of unique target paths with inbound links.
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

    /// Removes every edge sourced by any path in `sources`, dropping a
    /// target left with no remaining inbound sources.
    ///
    /// Takes `&self` rather than consuming: callers (incremental refresh)
    /// still need the original map afterward to diff against the patched
    /// result, so this copies only the *retained* sources rather than
    /// cloning the whole map up front.
    #[inline]
    #[must_use]
    pub(super) fn without_sources(&self, sources: &HashSet<&Path>) -> Self {
        let mut edges = HashMap::with_capacity(self.0.len());
        for (target, srcs) in &self.0 {
            let filtered: Vec<PathBuf> = srcs
                .iter()
                .filter(|s| !sources.contains(s.as_path()))
                .cloned()
                .collect();
            if !filtered.is_empty() {
                edges.insert(target.clone(), filtered.into_boxed_slice());
            }
        }
        Self(edges)
    }

    /// Adds `edges` (`(target, source)` pairs), inserting each source into
    /// its target's sorted, deduplicated source list.
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

/// Index of files and their stems, used during link resolution.
struct LinkResolver<'a> {
    files: &'a [FileBase],
    stem_index: HashMap<BaseNameRef<'a>, Vec<&'a Path>>,
}

impl<'a> LinkResolver<'a> {
    /// Builds the resolver, indexing every file's stem in one O(n) pass.
    fn new(files: &'a [FileBase]) -> Self {
        let mut stem_index: HashMap<BaseNameRef<'a>, Vec<&'a Path>> =
            HashMap::with_capacity(files.len());
        for file in files {
            let path = file.path();
            if let Some(stem) = BaseNameRef::from_path(path) {
                stem_index.entry(stem).or_default().push(path);
            }
        }
        Self {
            files,
            stem_index,
        }
    }

    /// Resolves an already-split [`LinkTarget`] to an indexed file's path.
    ///
    /// Resolution tries three tiers in order:
    ///
    /// 1. An exact project-relative path match (Markdown-style links).
    /// 2. The same path with a `.md` extension appended, when `target` has no
    ///    extension of its own (Markdown-style links that omit the extension).
    /// 3. The file nearest `from` among every indexed file sharing `target`'s
    ///    stem (Obsidian wikilink-by-name resolution), attempted only when
    ///    [`LinkTarget::is_basename`] says `target`'s path has no directory
    ///    prefix.
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
        if let Some(path) = self.find_by_path(candidate) {
            return Some(Target(path));
        }
        if candidate.extension().is_none() {
            let with_extension = candidate.with_extension("md");
            if let Some(path) = self.find_by_path(&with_extension) {
                return Some(Target(path));
            }
        }
        if !target.is_basename() {
            return None;
        }
        let stem =
            candidate.file_stem().and_then(|s| s.to_str()).unwrap_or(path_part);
        let target_ext = candidate.extension().and_then(|s| s.to_str());
        if let Some(ext) = target_ext
            && let Some(path) = self.nearest_by_stem(stem, from, Some(ext))
        {
            return Some(Target(path));
        }
        self.nearest_by_stem(stem, from, None).map(Target)
    }

    fn nearest_by_stem(
        &self,
        stem: &str,
        from: &Path,
        target_ext: Option<&str>,
    ) -> Option<&'a Path> {
        let candidates = self.stem_index.get(stem)?;
        let mut nearest: Option<(usize, &'a Path)> = None;
        let mut tied = false;
        for &candidate in candidates {
            if let Some(expected_ext) = target_ext
                && candidate.extension().and_then(|e| e.to_str())
                    != Some(expected_ext)
            {
                continue;
            }
            let distance = folder_distance(from, candidate);
            match nearest {
                Some((best, _)) if distance > best => {}
                Some((best, _)) if distance == best => tied = true,
                _ => {
                    nearest = Some((distance, candidate));
                    tied = false;
                }
            }
        }
        if tied {
            None
        } else {
            nearest.map(|(_, path)| path)
        }
    }

    fn find_by_path(&self, path: &Path) -> Option<&'a Path> {
        self.files
            .binary_search_by(|file| file.path().cmp(path))
            .ok()
            .and_then(|i| self.files.get(i))
            .map(FileBase::path)
    }

    /// Resolves every one of `note`'s outlinks against this resolver's
    /// indexed file set, returning deduplicated target paths.
    ///
    /// Pure and I/O-free: reused by [`InlinkMap::new`] (one shared resolver
    /// across every note, cold build) and incremental refresh (the same
    /// shared resolver, called only for modified notes) so resolving a
    /// handful of edited notes never pays the cost of rebuilding the
    /// resolver's file-stem index per note.
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
}

/// Resolves each of `notes`' outlinks against `files`, returning
/// `(target, source)` pairs ready for [`InlinkMap::with_edges`]. Builds one
/// [`LinkResolver`] shared across every note in `notes`, so patching several
/// modified notes at once still pays the resolver-build cost only once.
pub(super) fn resolve_edges_for(
    notes: &[Note],
    files: &[FileBase],
) -> Vec<(PathBuf, PathBuf)> {
    let resolver = LinkResolver::new(files);
    let mut edges = Vec::new();
    for note in notes {
        for target in resolver.resolve_note(note) {
            edges.push((target.to_path_buf(), note.path().to_path_buf()));
        }
    }
    edges
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

/// Computes the path-segment distance between `a`'s and `b`'s containing
/// folders in a single pass.
///
/// Steps up from `a`'s folder to its nearest shared ancestor with `b`'s folder,
/// then down to `b`'s folder; files in the same folder have distance `0`.
fn folder_distance(a: &Path, b: &Path) -> usize {
    let a_folder = a.parent().unwrap_or_else(|| Path::new(""));
    let b_folder = b.parent().unwrap_or_else(|| Path::new(""));
    let mut a_iter = a_folder.components();
    let mut b_iter = b_folder.components();
    let mut shared: usize = 0;
    let mut a_count: usize = 0;
    let mut b_count: usize = 0;
    let mut matching = true;
    loop {
        match (a_iter.next(), b_iter.next()) {
            (Some(ac), Some(bc)) => {
                a_count = a_count.saturating_add(1);
                b_count = b_count.saturating_add(1);
                if matching && ac == bc {
                    shared = shared.saturating_add(1);
                } else {
                    matching = false;
                }
            }
            (Some(_), None) => {
                a_count =
                    a_count.saturating_add(1).saturating_add(a_iter.count());
                break;
            }
            (None, Some(_)) => {
                b_count =
                    b_count.saturating_add(1).saturating_add(b_iter.count());
                break;
            }
            (None, None) => break,
        }
    }
    a_count
        .saturating_sub(shared)
        .saturating_add(b_count.saturating_sub(shared))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        file::FileFormat,
        note::{LinkType, MarkdownParserInput, parse_markdown},
    };

    fn parse(path: &str, src: &str) -> Note {
        let input = MarkdownParserInput::for_test(Path::new(path), src);
        parse_markdown(&input)
    }

    fn file_for_note(path: &str) -> FileBase {
        FileBase::new_test(
            PathBuf::from(path),
            Path::new(path)
                .parent()
                .map_or_else(PathBuf::new, Path::to_path_buf),
            FileFormat::Note,
        )
    }

    fn file_for_attachment(path: &str, format: FileFormat) -> FileBase {
        FileBase::new_test(
            PathBuf::from(path),
            Path::new(path)
                .parent()
                .map_or_else(PathBuf::new, Path::to_path_buf),
            format,
        )
    }

    fn note_with_outlink(path: &str, target: &str, kind: LinkType) -> Note {
        let src = match kind {
            LinkType::Wikilink => format!("[[{target}]]"),
            LinkType::Markdown => format!("[link]({target})"),
        };
        parse(path, &src)
    }

    mod target_resolution {
        use pretty_assertions::assert_eq;

        use super::*;

        fn resolve<'a>(
            files: &'a [FileBase],
            from: &str,
            target: LinkTarget<'_>,
        ) -> Option<Target<'a>> {
            let resolver = LinkResolver::new(files);
            resolver.resolve(Path::new(from), target)
        }

        fn files_from_notes(paths: &[&str]) -> Vec<FileBase> {
            let mut files: Vec<FileBase> =
                paths.iter().map(|p| file_for_note(p)).collect();
            files.sort_by(|a, b| a.path().cmp(b.path()));
            files
        }

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
        fn resolves_wikilink_by_unique_file_stem() {
            let files =
                files_from_notes(&["notes/Project Alpha.md", "notes/other.md"]);

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
        fn returns_none_for_unmatched_qualified_path_without_stem_fallback() {
            let files = files_from_notes(&["archive/foo.md", "notes/bar.md"]);

            assert_eq!(
                resolve(&files, "linking.md", LinkTarget::Path("notes/foo")),
                None
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

        #[test]
        fn resolves_ambiguous_stem_match_to_nearest_candidate() {
            let files =
                files_from_notes(&["notes/a/note.md", "notes/b/note.md"]);

            assert_eq!(
                resolve(&files, "notes/a/linking.md", LinkTarget::Path("note")),
                Some(Target(Path::new("notes/a/note.md")))
            );
        }

        #[test]
        fn resolves_ambiguous_self_referential_stem_to_itself() {
            let files = files_from_notes(&["a.md", "b/a.md"]);

            assert_eq!(
                resolve(&files, "a.md", LinkTarget::Path("a")),
                Some(Target(Path::new("a.md")))
            );
        }

        #[test]
        fn returns_none_for_ambiguous_stem_match_at_equal_distance() {
            let files = files_from_notes(&["a/note.md", "b/note.md"]);

            assert_eq!(
                resolve(&files, "linking.md", LinkTarget::Path("note")),
                None
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
                resolve(&files, "near/linking.md", LinkTarget::Path("note")),
                Some(Target(Path::new("near/note.md")))
            );
        }

        #[test]
        fn returns_none_for_basename_matching_no_indexed_file() {
            let files = files_from_notes(&["other.md"]);

            assert_eq!(
                resolve(&files, "linking.md", LinkTarget::Path("nonexistent")),
                None
            );
        }

        #[test]
        fn resolves_basename_with_extension_by_stem() {
            let files = files_from_notes(&["notes/report.md"]);

            assert_eq!(
                resolve(&files, "linking.md", LinkTarget::Path("report.txt")),
                Some(Target(Path::new("notes/report.md")))
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

    mod inlink_map_construction {
        use pretty_assertions::assert_eq;

        use super::*;

        fn build_graph(notes: &[Note], extra_files: &[FileBase]) -> InlinkMap {
            let mut files: Vec<FileBase> = notes
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

            assert_eq!(inlinks.inlinks_of(Path::new("b.md")), [PathBuf::from(
                "a.md"
            )]);
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

            assert_eq!(inlinks.inlinks_of(Path::new("b.md")), [PathBuf::from(
                "a.md"
            )]);
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

            assert!(!inlinks.contains_target(Path::new("lonely.md")));
            assert!(inlinks.inlinks_of(Path::new("lonely.md")).is_empty());
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
            assert!(
                inlinks.inlinks_of(Path::new("notes/b/note.md")).is_empty()
            );
        }

        #[test]
        fn indexes_non_note_attachments_as_inlink_targets() {
            let notes = [
                note_with_outlink("note1.md", "chart.png", LinkType::Wikilink),
                note_with_outlink("note2.md", "chart.png", LinkType::Wikilink),
            ];
            let attachments =
                [file_for_attachment("images/chart.png", FileFormat::Other)];

            let inlinks = build_graph(&notes, &attachments);

            assert_eq!(inlinks.inlinks_of(Path::new("images/chart.png")), [
                PathBuf::from("note1.md"),
                PathBuf::from("note2.md")
            ]);
        }
    }

    mod inlink_map_invariants {
        use super::*;

        #[test]
        fn inbound_sources_are_always_sorted_in_ascending_path_order() {
            let notes = [
                note_with_outlink("z.md", "target", LinkType::Wikilink),
                note_with_outlink("a.md", "target", LinkType::Wikilink),
                note_with_outlink("m.md", "target", LinkType::Wikilink),
                parse("target.md", "# Target"),
            ];

            let mut files: Vec<FileBase> = notes
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
            assert!(
                sources
                    .windows(2)
                    .all(|w| w.first().unwrap() < w.get(1).unwrap())
            );
        }

        #[test]
        fn inbound_sources_contain_no_duplicates() {
            let note =
                parse("source.md", "[[target]] and [[target]] and [[target]]");
            let target = parse("target.md", "# Target");
            let notes = [note, target];

            let mut files: Vec<FileBase> = notes
                .iter()
                .map(|n| file_for_note(n.path().to_str().unwrap()))
                .collect();
            files.sort_by(|a, b| a.path().cmp(b.path()));
            let inlinks = InlinkMap::new(&notes, &files);

            let sources = inlinks.inlinks_of(Path::new("target.md"));
            assert_eq!(sources.len(), 1);
            assert_eq!(
                sources.first().map(PathBuf::as_path),
                Some(Path::new("source.md"))
            );
        }

        #[test]
        fn every_target_in_map_has_at_least_one_source() {
            let notes = [
                note_with_outlink("a.md", "target", LinkType::Wikilink),
                parse("target.md", "# Target"),
                parse("standalone.md", "# Standalone"),
            ];

            let mut files: Vec<FileBase> = notes
                .iter()
                .map(|n| file_for_note(n.path().to_str().unwrap()))
                .collect();
            files.sort_by(|a, b| a.path().cmp(b.path()));
            let inlinks = InlinkMap::new(&notes, &files);

            for (_, sources) in inlinks.iter() {
                assert!(!sources.is_empty());
            }
        }
    }

    mod inlink_map_accessors {
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
            let inlinks = InlinkMap::default();
            assert!(inlinks.inlinks_of(Path::new("nonexistent.md")).is_empty());
        }

        #[test]
        fn contains_target_checks_presence() {
            let mut raw = HashMap::new();
            raw.insert(
                PathBuf::from("target.md"),
                vec![PathBuf::from("src.md")].into_boxed_slice(),
            );
            let inlinks = InlinkMap::from_raw(raw);

            assert!(inlinks.contains_target(Path::new("target.md")));
            assert!(!inlinks.contains_target(Path::new("other.md")));
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
            for (target, sources) in inlinks.into_entries() {
                assert_eq!(target, PathBuf::from("target.md"));
                assert_eq!(sources.as_ref(), [PathBuf::from("source.md")]);
            }
        }
    }

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
                [Path::new("a.md")].into_iter().collect();

            let patched = inlinks.without_sources(&stale);

            assert!(!patched.contains_target(Path::new("target.md")));
        }

        #[test]
        fn removing_one_of_several_sources_keeps_the_target() {
            let inlinks = graph_with_two_sources();
            let stale: HashSet<&Path> =
                [Path::new("a.md")].into_iter().collect();

            let patched = inlinks.without_sources(&stale);

            assert_eq!(patched.inlinks_of(Path::new("target.md")), [
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
    }
    mod folder_proximity {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn same_folder_has_zero_distance() {
            assert_eq!(
                folder_distance(
                    Path::new("folder/a.md"),
                    Path::new("folder/b.md")
                ),
                0
            );
        }

        #[test]
        fn parent_and_child_have_distance_one() {
            assert_eq!(
                folder_distance(
                    Path::new("folder/sub/a.md"),
                    Path::new("folder/b.md")
                ),
                1
            );
            assert_eq!(
                folder_distance(
                    Path::new("folder/a.md"),
                    Path::new("folder/sub/b.md")
                ),
                1
            );
        }

        #[test]
        fn siblings_have_distance_two() {
            assert_eq!(
                folder_distance(
                    Path::new("folder/sub1/a.md"),
                    Path::new("folder/sub2/b.md")
                ),
                2
            );
        }

        #[test]
        fn different_subtrees_step_through_common_ancestor() {
            assert_eq!(
                folder_distance(
                    Path::new("a/b/c/file.md"),
                    Path::new("a/d/e/file.md")
                ),
                4 // 2 up from c to a, 2 down from a to e
            );
        }

        #[test]
        fn root_and_nested_folder_compute_correct_distance() {
            assert_eq!(
                folder_distance(
                    Path::new("root.md"),
                    Path::new("a/b/c/nested.md")
                ),
                3
            );
            assert_eq!(
                folder_distance(
                    Path::new("a/b/c/nested.md"),
                    Path::new("root.md")
                ),
                3
            );
        }
    }
}
