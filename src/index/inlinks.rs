//! Derived inbound links, computed from indexed outlinks.
//!
//! [`derive_inlinks`] runs as a post-processing pass over already-parsed
//! [`Note`] outlinks: nothing about an inlink is persisted. Recomputing from
//! the notes already held in memory is cheap enough that
//! [`super::FileIndex::query`] and [`super::FileIndex::query_tasks`] just
//! rebuild the map on every call, so there is no separate derived-data table
//! to keep in sync with `notes` on refresh.

use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

use crate::note::Note;

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
/// O(l log n), where `l` is the total outlink count across `notes` and `n`
/// is `notes.len()`: resolving each outlink binary-searches or linear-scans
/// the path-sorted slice `notes` (already sorted by
/// [`super::FileIndex::build`]/[`super::FileIndex::refresh`]/
/// [`super::FileIndex::load`]).
pub(super) fn derive_inlinks(
    notes: &[Note],
) -> BTreeMap<PathBuf, Vec<PathBuf>> {
    let mut edges: BTreeMap<&Path, BTreeSet<&Path>> = BTreeMap::new();
    for source in notes {
        for outlink in source.outlinks() {
            if let Some(target) = resolve_target(notes, outlink.target()) {
                edges.entry(target).or_default().insert(source.path());
            }
        }
    }
    edges
        .into_iter()
        .map(|(target, sources)| {
            (
                target.to_path_buf(),
                sources.into_iter().map(Path::to_path_buf).collect(),
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
fn resolve_target<'a>(notes: &'a [Note], target: &str) -> Option<&'a Path> {
    let path_part = target.split('#').next().unwrap_or(target).trim();
    if path_part.is_empty() {
        return None;
    }
    let candidate = Path::new(path_part);
    if let Some(path) = find_by_path(notes, candidate) {
        return Some(path);
    }
    if candidate.extension().is_none() {
        let with_extension = candidate.with_extension("md");
        if let Some(path) = find_by_path(notes, &with_extension) {
            return Some(path);
        }
    }
    let stem =
        candidate.file_stem().and_then(|s| s.to_str()).unwrap_or(path_part);
    find_unique_by_stem(notes, stem)
}

/// Binary-searches path-sorted `notes` for an exact path match.
fn find_by_path<'a>(notes: &'a [Note], path: &Path) -> Option<&'a Path> {
    let idx = notes.binary_search_by(|note| note.path().cmp(path)).ok()?;
    notes.get(idx).map(Note::path)
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
                Some(Path::new("notes/other.md"))
            );
        }

        #[test]
        fn resolves_a_markdown_path_missing_its_extension() {
            let notes = [parse_markdown("other.md", "# Other")];

            assert_eq!(
                resolve_target(&notes, "other"),
                Some(Path::new("other.md"))
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
                Some(Path::new("notes/Project Alpha.md"))
            );
        }

        #[test]
        fn strips_a_heading_fragment_before_matching() {
            let notes = [parse_markdown("other.md", "# Other")];

            assert_eq!(
                resolve_target(&notes, "other#Some Heading"),
                Some(Path::new("other.md"))
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
    }

    mod derive_inlinks_tests {
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
