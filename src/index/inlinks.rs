//! Derived inbound links, computed from indexed outlinks.
//!
//! [`derive_inlinks`] runs as a post-processing pass over already-parsed
//! [`Note`] outlinks. It is a full recompute over every indexed Note, never a
//! per-note patch, because resolving one Note's outlink can depend on every
//! *other* indexed Note (see [`resolve_target`]'s stem-matching tier).
//! [`super::FileIndex::build`] and [`super::FileIndex::refresh`] (the latter
//! only when something changed) call this once and persist the result;
//! [`super::FileIndex::query`]/[`super::FileIndex::query_tasks`] read the
//! already-computed map instead of calling it.

use std::{
    collections::{BTreeSet, HashMap},
    path::{Path, PathBuf},
};

use crate::note::Note;

/// Target-keyed inbound link edges: every Note path to the paths of every
/// Note whose outlinks resolve to it. Returned by [`derive_inlinks`] and
/// the type persisted/reloaded by [`super::store`].
pub(super) type InlinkMap = HashMap<PathBuf, Vec<PathBuf>>;

/// A resolved link target: the path of the Note an outlink points to.
///
/// Distinct from [`Source`] so [`derive_inlinks`]'s `edges` map can't
/// accidentally record an edge in the wrong direction — both wrap the same
/// `&Path` representation, so nothing but the type system would catch a
/// swapped `edges.entry(source).or_default().insert(target)`.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
struct Target<'a>(&'a Path);

impl Target<'_> {
    fn to_path_buf(self) -> PathBuf {
        self.0.to_path_buf()
    }
}

/// A Note that links *to* a [`Target`]: the path recorded as an inbound
/// edge. See [`Target`] for why this is a separate type.
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
struct Source<'a>(&'a Path);

impl Source<'_> {
    fn to_path_buf(self) -> PathBuf {
        self.0.to_path_buf()
    }
}

/// Derives inbound links for every indexed Note from its peers' outlinks.
///
/// For each outlink in each Note, resolves the link target against `notes`
/// (see [`resolve_target`]) and records an inbound edge from the linking
/// Note to the resolved target Note. Unresolvable targets (external URLs,
/// links to non-Notes, or links with no matching Note) contribute no edge.
///
/// Duplicate outlinks to the same target within one Note, and self-links,
/// collapse to a single edge rather than duplicating or otherwise
/// corrupting the result: edges are deduplicated `(target, source)` pairs.
///
/// # Performance
///
/// O(l log n) when every outlink resolves by exact path (the common
/// Markdown-link case): each lookup binary-searches the path-sorted slice
/// `notes` (already sorted by [`super::FileIndex::build`]/
/// [`super::FileIndex::refresh`]/[`super::FileIndex::load`]). An outlink
/// that falls through to stem matching (the wikilink-by-name case) costs
/// O(n) instead, since [`find_unique_by_stem`] scans every indexed Note;
/// worst case across `l` such outlinks is O(l·n).
pub(super) fn derive_inlinks(notes: &[Note]) -> InlinkMap {
    let mut edges: HashMap<Target<'_>, BTreeSet<Source<'_>>> = HashMap::new();
    for source in notes {
        for outlink in source.outlinks() {
            if let Some(target) = resolve_target(notes, outlink.target()) {
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

/// Resolves an outlink's raw `target` text to an indexed Note's path.
///
/// Tries, in order:
/// 1. An exact project-relative path match (Markdown-style links).
/// 2. The same path with a `.md` extension appended, when `target` has no
///    extension of its own (Markdown-style links that omit the extension).
/// 3. A unique match on file stem across every indexed Note (Obsidian
///    wikilink-by-name resolution). An ambiguous stem match resolves to `None`
///    rather than guessing which Note was meant.
///
/// Any `#fragment` heading suffix is stripped before matching, so a link
/// into a heading still resolves to the Note that owns it.
///
/// ponytail: no note-directory-relative resolution (`../sibling.md`) and no
/// Obsidian shortest-unique-path search across ambiguous folders. Upgrade
/// if real vaults need either.
fn resolve_target<'a>(notes: &'a [Note], target: &str) -> Option<Target<'a>> {
    let path_part = target.split('#').next().unwrap_or(target).trim();
    if path_part.is_empty() {
        return None;
    }
    let candidate = Path::new(path_part);
    if let Some(note) = find_by_path(notes, candidate) {
        return Some(Target(note.path()));
    }
    if candidate.extension().is_none() {
        let with_extension = candidate.with_extension("md");
        if let Some(note) = find_by_path(notes, &with_extension) {
            return Some(Target(note.path()));
        }
    }
    let stem =
        candidate.file_stem().and_then(|s| s.to_str()).unwrap_or(path_part);
    find_unique_by_stem(notes, stem).map(Target)
}

/// Binary-searches path-sorted `notes` for an exact path match.
///
/// Shared with [`super::FileIndex::note`], which does the same lookup once
/// a [`FileIndex`](super::FileIndex) exists; this free-function form is for
/// [`resolve_target`], which only has a bare `&[Note]` slice to search
/// during [`super::FileIndex::build`].
pub(super) fn find_by_path<'a>(
    notes: &'a [Note],
    path: &Path,
) -> Option<&'a Note> {
    let idx = notes.binary_search_by(|note| note.path().cmp(path)).ok()?;
    notes.get(idx)
}

/// Finds the one Note whose file stem equals `stem`, or `None` if zero or
/// more than one Note matches.
fn find_unique_by_stem<'a>(notes: &'a [Note], stem: &str) -> Option<&'a Path> {
    let mut matches = notes.iter().filter(|note| {
        note.path().file_stem().and_then(|s| s.to_str()) == Some(stem)
    });
    let first = matches.next()?;
    matches.next().is_none().then(|| first.path())
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

    mod resolve_target {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn resolves_an_exact_root_relative_markdown_path() {
            let notes = [parse_markdown("notes/other.md", "# Other")];

            assert_eq!(
                resolve_target(&notes, "notes/other.md"),
                Some(Target(Path::new("notes/other.md")))
            );
        }

        #[test]
        fn resolves_a_markdown_path_missing_its_extension() {
            let notes = [parse_markdown("other.md", "# Other")];

            assert_eq!(
                resolve_target(&notes, "other"),
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
                resolve_target(&notes, "Project Alpha"),
                Some(Target(Path::new("notes/Project Alpha.md")))
            );
        }

        #[test]
        fn strips_a_heading_fragment_before_matching() {
            let notes = [parse_markdown("other.md", "# Other")];

            assert_eq!(
                resolve_target(&notes, "other#Some Heading"),
                Some(Target(Path::new("other.md")))
            );
        }

        #[test]
        fn returns_none_for_an_ambiguous_stem_match() {
            let notes = [
                parse_markdown("a/note.md", "# A"),
                parse_markdown("b/note.md", "# B"),
            ];

            assert_eq!(resolve_target(&notes, "note"), None);
        }

        #[test]
        fn returns_none_for_an_unresolvable_target() {
            let notes = [parse_markdown("other.md", "# Other")];

            assert_eq!(resolve_target(&notes, "https://example.com"), None);
        }

        #[test]
        fn returns_none_for_a_fragment_only_target() {
            let notes = [parse_markdown("other.md", "# Other")];

            assert_eq!(resolve_target(&notes, "#Some Heading"), None);
        }
    }

    mod derive_inlinks {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn maps_a_target_to_the_single_note_linking_to_it() {
            let notes = [
                note_with_outlink("a.md", "b", LinkType::Wikilink),
                parse_markdown("b.md", "# B"),
            ];

            let inlinks = derive_inlinks(&notes);

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

            let inlinks = derive_inlinks(&notes);

            assert_eq!(
                inlinks.get(Path::new("target.md")),
                Some(&vec![PathBuf::from("a.md"), PathBuf::from("b.md")])
            );
        }

        #[test]
        fn collapses_duplicate_outlinks_within_one_note_to_one_edge() {
            let note = parse_markdown("a.md", "[[b]] and [[b]] again");
            let notes = [note, parse_markdown("b.md", "# B")];

            let inlinks = derive_inlinks(&notes);

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

            let inlinks = derive_inlinks(&notes);

            assert_eq!(
                inlinks.get(Path::new("a.md")),
                Some(&vec![PathBuf::from("a.md"), PathBuf::from("b.md")])
            );
        }

        #[test]
        fn omits_notes_with_no_inbound_links() {
            let notes = [parse_markdown("lonely.md", "# Lonely")];

            let inlinks = derive_inlinks(&notes);

            assert_eq!(inlinks.get(Path::new("lonely.md")), None);
        }

        #[test]
        fn ignores_outlinks_that_do_not_resolve_to_any_note() {
            let notes = [note_with_outlink(
                "a.md",
                "https://example.com",
                LinkType::Markdown,
            )];

            let inlinks = derive_inlinks(&notes);

            assert!(inlinks.is_empty());
        }
    }
}
