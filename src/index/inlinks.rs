//! Derived inbound links computed from indexed outlinks.
//!
//! [`derive_inlinks`] runs as a full recompute over every indexed [`Note`],
//! never a per-note patch, because resolving one Note's outlink can depend on
//! every *other* indexed Note (see [`LinkResolver::resolve`]'s stem-matching
//! tier).
//!
//! - [`super::IndexerService::build`] and [`super::IndexerService::refresh`]
//!   (when something changed) call this once and persist the result.
//! - Query execution reads the already-computed map from
//!   [`super::FileIndex::entries`].

use std::{
    collections::{BTreeSet, HashMap},
    path::{Path, PathBuf},
};

use crate::{
    BaseNameRef,
    note::{LinkTarget, Note},
};

/// Target-keyed inbound link edges: maps every [`Note`] path to the paths of
/// every [`Note`] whose outlinks resolve to it. Returned by [`derive_inlinks`];
/// persisted and reloaded by [`super::store`].
pub type InlinkMap = HashMap<PathBuf, Vec<PathBuf>>;

/// Derives inbound links for every indexed [`Note`] from its peers' outlinks.
///
/// For each outlink in each [`Note`], resolves the link target against `notes`
/// (see `LinkResolver`) and records an inbound edge from the linking [`Note`]
/// to the resolved target [`Note`]. Unresolvable targets (external URLs, links
/// to non-Notes, or links with no matching [`Note`]) contribute no edge.
///
/// Duplicate outlinks to the same target within one Note, and self-links,
/// collapse to a single edge rather than duplicating or otherwise corrupting
/// the result: edges are deduplicated `(target, source)` pairs.
///
/// # Performance
///
/// - O(n) to build the stem index once (see `LinkResolver::new`).
/// - O(l log n) total for l outlinks: exact-path resolution binary-searches the
///   path-sorted slice `notes` (already sorted by [`IndexerService`]'s
///   [`build`]/[`refresh`]/[`load`]).
/// - The wikilink-by-name fallback tier looks its stem up in the index in O(1)
///   average time, then scans only that stem's candidates (not all of `notes`)
///   to break ties by proximity.
///
/// [`IndexerService`]: super::IndexerService
/// [`build`]: super::IndexerService::build
/// [`refresh`]: super::IndexerService::refresh
/// [`load`]: super::IndexerService::load
#[must_use]
#[inline]
pub fn derive_inlinks(notes: &[&Note]) -> InlinkMap {
    let resolver = LinkResolver::new(notes);
    let mut edges: HashMap<Target<'_>, BTreeSet<Source<'_>>> = HashMap::new();
    for source in notes {
        for outlink in source.outlinks() {
            if let Some(target) =
                resolver.resolve(source.path(), outlink.target_parts())
            {
                edges.entry(target).or_default().insert(Source(source.path()));
            }
        }
    }
    edges
        .into_iter()
        .map(|(target, sources)| {
            (
                target.to_path_buf(),
                sources.into_iter().map(Source::to_path_buf).collect(),
            )
        })
        .collect()
}

/// Snapshot of indexed Notes plus a precomputed file-stem index, built once per
/// [`derive_inlinks`] call and reused across every outlink resolution in that
/// call.
struct LinkResolver<'a, 'b> {
    notes: &'b [&'a Note],
    stem_index: HashMap<BaseNameRef<'a>, Vec<&'a Path>>,
}

impl<'a, 'b> LinkResolver<'a, 'b> {
    /// Builds the resolver, indexing every Note's file stem in one O(n)
    /// pass.
    fn new(notes: &'b [&'a Note]) -> Self {
        let mut stem_index: HashMap<BaseNameRef<'a>, Vec<&'a Path>> =
            HashMap::with_capacity(notes.len());
        for note in notes {
            if let Some(stem) = BaseNameRef::from_path(note.path()) {
                stem_index.entry(stem).or_default().push(note.path());
            }
        }
        Self {
            notes,
            stem_index,
        }
    }

    /// Resolves an already-split [`LinkTarget`] to an indexed Note's path.
    ///
    /// Resolution tries three tiers in order:
    ///
    /// 1. An exact project-relative path match (Markdown-style links).
    /// 2. The same path with a `.md` extension appended, when `target` has no
    ///    extension of its own (Markdown-style links that omit the extension).
    /// 3. The Note nearest `from` among every indexed Note sharing `target`'s
    ///    file stem (Obsidian wikilink-by-name resolution), attempted only when
    ///    [`LinkTarget::is_basename`] says `target`'s path has no directory
    ///    prefix.
    ///
    /// A qualified path (such as `archive/foo`) that fails the first two
    /// attempts resolves to `None` rather than falling back to a whole-index
    /// name search that could match an unrelated same-named Note elsewhere.
    /// Ambiguity among same-stem candidates resolves by
    /// [`Self::nearest_by_stem`]'s proximity rule; a genuine tie still resolves
    /// to `None` rather than guessing.
    ///
    /// `target`'s anchor half, if any, plays no part in resolution: only the
    /// path segment is matched, so a link into a heading still resolves to the
    /// Note that owns it.
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
        if let Some(note) = find_by_path(self.notes, candidate) {
            return Some(Target(note.path()));
        }
        if candidate.extension().is_none() {
            let with_extension = candidate.with_extension("md");
            if let Some(note) = find_by_path(self.notes, &with_extension) {
                return Some(Target(note.path()));
            }
        }
        if !target.is_basename() {
            return None;
        }
        let stem =
            candidate.file_stem().and_then(|s| s.to_str()).unwrap_or(path_part);
        self.nearest_by_stem(stem, from).map(Target)
    }

    /// Finds the Note nearest `from` among every indexed Note sharing `stem`,
    /// by Obsidian's shortest-unique-path rule.
    ///
    /// A single candidate resolves outright. Two or more resolve to whichever
    /// has the smallest [`folder_distance`] from `from`; a genuine tie (no
    /// single nearest candidate) resolves to `None` rather than guessing. Zero
    /// candidates also resolve to `None`.
    fn nearest_by_stem(&self, stem: &str, from: &Path) -> Option<&'a Path> {
        let candidates = self.stem_index.get(stem)?;
        let mut nearest: Option<(usize, &Path)> = None;
        let mut tied = false;
        for &candidate in candidates {
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
}

/// A resolved link target: the path of the Note an outlink points to.
///
/// Distinct from [`Source`] so [`derive_inlinks`]'s `edges` map can't
/// accidentally record an edge in the wrong direction: both wrap the same
/// `&Path` representation, so nothing but the type system would catch a swapped
/// `edges.entry(source).or_default().insert(target)`.
///
/// [`derive_inlinks`]: fn@derive_inlinks
#[derive(Copy, Clone, Debug, Eq, Hash, PartialEq)]
struct Target<'a>(&'a Path);

impl Target<'_> {
    fn to_path_buf(self) -> PathBuf {
        self.0.to_path_buf()
    }
}

/// A Note that links *to* a [`Target`]: the path recorded as an inbound edge.
///
/// See [`Target`] for why this is a separate type.
///
/// [`Target`]: struct@Target
#[derive(Copy, Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct Source<'a>(&'a Path);

impl Source<'_> {
    fn to_path_buf(self) -> PathBuf {
        self.0.to_path_buf()
    }
}

/// Binary-searches path-sorted `notes` for an exact path match.
///
/// Private to this module: [`LinkResolver::resolve`] needs the same search
/// over a `&[&Note]` slice while resolving link targets during
/// [`super::IndexerService::build`]/[`super::IndexerService::refresh`].
fn find_by_path<'a>(notes: &[&'a Note], path: &Path) -> Option<&'a Note> {
    notes
        .binary_search_by(|note| note.path().cmp(path))
        .ok()
        .and_then(|i| notes.get(i))
        .copied()
}

/// Computes the path-segment distance between `a`'s and `b`'s containing
/// folders.
///
/// Steps up from `a`'s folder to its nearest shared ancestor with `b`'s folder,
/// then back down to `b`'s folder; files in the same folder are distance `0`.
/// This is [`LinkResolver::nearest_by_stem`]'s proximity tie-break for
/// ambiguous wikilink stem matches.
///
/// Reads folder placement from each Note's own `path()` rather than
/// [`crate::file::FileBase`]'s precomputed `folder`/`name` fields, because this
/// resolution pass only needs `&[Note]` and pulling in a second sorted
/// collection for folder data that `Note::path()` already provides would be
/// redundant.
///
/// [`LinkResolver::nearest_by_stem`]: LinkResolver::nearest_by_stem
fn folder_distance(a: &Path, b: &Path) -> usize {
    let a_folder = a.parent().unwrap_or_else(|| Path::new(""));
    let b_folder = b.parent().unwrap_or_else(|| Path::new(""));
    let shared = a_folder
        .components()
        .zip(b_folder.components())
        .take_while(|(x, y)| x == y)
        .count();
    a_folder
        .components()
        .count()
        .saturating_sub(shared)
        .saturating_add(b_folder.components().count().saturating_sub(shared))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::note::{LinkType, parse_markdown};

    /// Builds a [`Note`] at `path` whose only outlink is `target`.
    fn note_with_outlink(path: &str, target: &str, kind: LinkType) -> Note {
        let src = match kind {
            LinkType::Wikilink => format!("[[{target}]]"),
            LinkType::Markdown => format!("[link]({target})"),
        };
        parse_markdown(path, &src)
    }

    mod resolve {
        use pretty_assertions::assert_eq;

        use super::*;

        /// Resolves `target` against `notes` as if linked from `from`,
        /// building the stem index the way [`derive_inlinks`] does.
        fn resolve<'a>(
            notes: &'a [Note],
            from: &str,
            target: LinkTarget<'_>,
        ) -> Option<Target<'a>> {
            let refs: Vec<&Note> = notes.iter().collect();
            LinkResolver::new(&refs).resolve(Path::new(from), target)
        }

        #[test]
        fn resolves_an_exact_root_relative_markdown_path() {
            let notes = [parse_markdown("notes/other.md", "# Other")];

            assert_eq!(
                resolve(
                    &notes,
                    "linking.md",
                    LinkTarget::Path("notes/other.md")
                ),
                Some(Target(Path::new("notes/other.md")))
            );
        }

        #[test]
        fn resolves_a_markdown_path_missing_its_extension() {
            let notes = [parse_markdown("other.md", "# Other")];

            assert_eq!(
                resolve(&notes, "linking.md", LinkTarget::Path("other")),
                Some(Target(Path::new("other.md")))
            );
        }

        #[test]
        fn resolves_a_wikilink_by_unique_file_stem() {
            let notes = [
                parse_markdown("notes/Project Alpha.md", "# Alpha"),
                parse_markdown("notes/other.md", "# Other"),
            ];

            assert_eq!(
                resolve(
                    &notes,
                    "linking.md",
                    LinkTarget::Path("Project Alpha")
                ),
                Some(Target(Path::new("notes/Project Alpha.md")))
            );
        }

        #[test]
        fn returns_none_for_an_unmatched_qualified_path_without_a_stem_fallback()
         {
            // "notes/foo" has an explicit directory prefix and matches no
            // indexed path exactly. It must resolve to `None`, not fall
            // back to a whole-index stem search that would otherwise match
            // the unrelated `archive/foo.md`.
            let notes = [
                parse_markdown("archive/foo.md", "# Archived"),
                parse_markdown("notes/bar.md", "# Bar"),
            ];

            assert_eq!(
                resolve(&notes, "linking.md", LinkTarget::Path("notes/foo")),
                None
            );
        }

        #[test]
        fn resolves_a_path_with_an_anchor_by_its_path_segment() {
            let notes = [parse_markdown("other.md", "# Other")];

            assert_eq!(
                resolve(
                    &notes,
                    "linking.md",
                    LinkTarget::PathWithAnchor("other", "Some Heading")
                ),
                Some(Target(Path::new("other.md")))
            );
        }

        #[test]
        fn resolves_an_ambiguous_stem_match_to_the_nearest_candidate() {
            // Both candidates share stem "note"; the linking Note lives in
            // the same folder as `notes/a/note.md`, which is nearer than
            // `notes/b/note.md` (their nearest shared ancestor is `notes`).
            let notes = [
                parse_markdown("notes/a/note.md", "# Near"),
                parse_markdown("notes/b/note.md", "# Far"),
            ];

            assert_eq!(
                resolve(&notes, "notes/a/linking.md", LinkTarget::Path("note")),
                Some(Target(Path::new("notes/a/note.md")))
            );
        }

        #[test]
        fn resolves_an_ambiguous_self_referential_stem_to_itself() {
            // "a.md" and "b/a.md" share stem "a"; linking from "a.md" itself
            // makes "a.md" the unique nearest candidate (distance 0, same
            // folder as itself) over "b/a.md" (distance 1).
            let notes = [
                parse_markdown("a.md", "# Self"),
                parse_markdown("b/a.md", "# Other"),
            ];

            assert_eq!(
                resolve(&notes, "a.md", LinkTarget::Path("a")),
                Some(Target(Path::new("a.md")))
            );
        }

        #[test]
        fn returns_none_for_an_ambiguous_stem_match_at_equal_distance() {
            // Both candidates are one folder away from the root-level
            // linking Note, so proximity itself cannot break the tie.
            let notes = [
                parse_markdown("a/note.md", "# A"),
                parse_markdown("b/note.md", "# B"),
            ];

            assert_eq!(
                resolve(&notes, "linking.md", LinkTarget::Path("note")),
                None
            );
        }

        #[test]
        fn resolves_to_the_nearer_candidate_despite_a_farther_tie() {
            // "far/a" and "far/b" tie with each other (distance 3 from the
            // linking Note) and are indexed *before* "near/note.md" (distance
            // 0), so recording the tie and then finding a strictly closer
            // candidate must reset the tie rather than sticking at `None`.
            let notes = [
                parse_markdown("far/a/note.md", "# Far A"),
                parse_markdown("far/b/note.md", "# Far B"),
                parse_markdown("near/note.md", "# Near"),
            ];

            assert_eq!(
                resolve(&notes, "near/linking.md", LinkTarget::Path("note")),
                Some(Target(Path::new("near/note.md")))
            );
        }

        #[test]
        fn returns_none_for_a_basename_matching_no_indexed_note() {
            let notes = [parse_markdown("other.md", "# Other")];

            assert_eq!(
                resolve(&notes, "linking.md", LinkTarget::Path("nonexistent")),
                None
            );
        }

        #[test]
        fn resolves_a_basename_with_an_extension_by_its_stem() {
            // "report.txt" has its own extension, so tier 2 (appending
            // `.md`) is skipped rather than trying "report.txt.md". It is
            // still a bare basename, so tier 3 falls back to a stem match
            // on "report" (`is_basename` doesn't require an extensionless
            // target).
            let notes = [parse_markdown("notes/report.md", "# Report")];

            assert_eq!(
                resolve(&notes, "linking.md", LinkTarget::Path("report.txt")),
                Some(Target(Path::new("notes/report.md")))
            );
        }

        #[test]
        fn returns_none_for_an_unresolvable_target() {
            let notes = [parse_markdown("other.md", "# Other")];

            assert_eq!(
                resolve(
                    &notes,
                    "linking.md",
                    LinkTarget::Path("https://example.com")
                ),
                None
            );
        }

        #[test]
        fn returns_none_for_an_anchor_only_target() {
            let notes = [parse_markdown("other.md", "# Other")];

            assert_eq!(
                resolve(
                    &notes,
                    "linking.md",
                    LinkTarget::AnchorOnly("Some Heading")
                ),
                None
            );
        }
    }

    mod derive_inlinks {
        use pretty_assertions::assert_eq;

        use super::*;

        /// Runs [`derive_inlinks`] over `notes`, building the `&[&Note]`
        /// view the production signature takes.
        fn run(notes: &[Note]) -> InlinkMap {
            let refs: Vec<&Note> = notes.iter().collect();
            derive_inlinks(&refs)
        }

        #[test]
        fn maps_a_target_to_the_single_note_linking_to_it() {
            let notes = [
                note_with_outlink("a.md", "b", LinkType::Wikilink),
                parse_markdown("b.md", "# B"),
            ];

            let inlinks = run(&notes);

            assert_eq!(
                inlinks.get(Path::new("b.md")),
                Some(&vec![PathBuf::from("a.md")])
            );
        }

        #[test]
        fn maps_a_target_to_every_note_linking_to_it() {
            let notes = [
                note_with_outlink("a.md", "target", LinkType::Wikilink),
                note_with_outlink("b.md", "target", LinkType::Wikilink),
                parse_markdown("target.md", "# Target"),
            ];

            let inlinks = run(&notes);

            assert_eq!(
                inlinks.get(Path::new("target.md")),
                Some(&vec![PathBuf::from("a.md"), PathBuf::from("b.md")])
            );
        }

        #[test]
        fn collapses_duplicate_outlinks_within_one_note_to_one_edge() {
            let note = parse_markdown("a.md", "[[b]] and [[b]] again");
            let notes = [note, parse_markdown("b.md", "# B")];

            let inlinks = run(&notes);

            assert_eq!(
                inlinks.get(Path::new("b.md")),
                Some(&vec![PathBuf::from("a.md")])
            );
        }

        #[test]
        fn records_a_self_link_without_corrupting_other_edges() {
            let notes = [
                note_with_outlink("a.md", "a", LinkType::Wikilink),
                note_with_outlink("b.md", "a", LinkType::Wikilink),
            ];

            let inlinks = run(&notes);

            assert_eq!(
                inlinks.get(Path::new("a.md")),
                Some(&vec![PathBuf::from("a.md"), PathBuf::from("b.md")])
            );
        }

        #[test]
        fn omits_notes_with_no_inbound_links() {
            let notes = [parse_markdown("lonely.md", "# Lonely")];

            let inlinks = run(&notes);

            assert_eq!(inlinks.get(Path::new("lonely.md")), None);
        }

        #[test]
        fn ignores_outlinks_that_do_not_resolve_to_any_note() {
            let notes = [note_with_outlink(
                "a.md",
                "https://example.com",
                LinkType::Markdown,
            )];

            let inlinks = run(&notes);

            assert!(inlinks.is_empty());
        }

        #[test]
        fn resolves_an_ambiguous_wikilink_to_the_nearest_note() {
            let notes = [
                note_with_outlink(
                    "notes/a/linking.md",
                    "note",
                    LinkType::Wikilink,
                ),
                parse_markdown("notes/a/note.md", "# Near"),
                parse_markdown("notes/b/note.md", "# Far"),
            ];

            let inlinks = run(&notes);

            assert_eq!(
                inlinks.get(Path::new("notes/a/note.md")),
                Some(&vec![PathBuf::from("notes/a/linking.md")])
            );
            assert_eq!(inlinks.get(Path::new("notes/b/note.md")), None);
        }
    }
}
