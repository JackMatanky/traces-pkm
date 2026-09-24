//! Nearest-candidate resolution for one wikilink basename's file candidates.
//!
//! [`BaseNameIndex`] selects a [`ResolutionType`] once in
//! [`BaseNameIndex::build`] — a linear scan below [`TRIE_THRESHOLD`]
//! candidates, a folder trie at or above it — and answers nearest-by-folder-
//! distance queries through [`BaseNameIndex::nearest`]. Equidistant nearest
//! candidates resolve to `None`.

use std::path::Path;

use indextree::{Arena, NodeId};
use rustc_hash::FxHashMap;

use crate::path::FolderRef;

/// Per-basename candidate index dispatching to flat scan or a folder trie.
///
/// Opaque over its [`ResolutionType`]: callers see one query surface and
/// never the representation.
pub(super) struct BaseNameIndex<'a>(ResolutionType<'a>);

/// Candidate-count strategy selected once at [`BaseNameIndex::build`].
enum ResolutionType<'a> {
    Flat(Vec<&'a Path>),
    Trie(Box<CandidateTrie<'a>>),
}

/// Candidate count at which [`BaseNameIndex::build`] switches to
/// [`CandidateTrie`].
pub(super) const TRIE_THRESHOLD: usize = 64;

impl<'a> BaseNameIndex<'a> {
    /// Builds the index for one basename's `candidates`, selecting the strategy
    /// by [`TRIE_THRESHOLD`].
    pub(super) fn build(candidates: Vec<&'a Path>) -> Self {
        Self(if candidates.len() >= TRIE_THRESHOLD {
            ResolutionType::Trie(Box::new(CandidateTrie::build(&candidates)))
        } else {
            ResolutionType::Flat(candidates)
        })
    }

    /// Returns the candidate nearest to `from`, filtered to `target_ext`.
    ///
    /// Equal-distance nearest candidates resolve to `None`.
    pub(super) fn nearest(
        &self,
        from: &Path,
        target_ext: Option<&str>,
    ) -> Option<&'a Path> {
        match &self.0 {
            ResolutionType::Flat(candidates) => {
                Self::nearest_flat(candidates, from, target_ext)
            }
            ResolutionType::Trie(trie) => trie.nearest(from, target_ext),
        }
    }

    /// Finds the nearest candidate by linear scan.
    ///
    /// Equal-distance nearest candidates resolve to `None`.
    fn nearest_flat(
        candidates: &[&'a Path],
        from: &Path,
        target_ext: Option<&str>,
    ) -> Option<&'a Path> {
        let mut nearest: Option<(usize, &'a Path)> = None;
        let mut tied = false;
        for &candidate in candidates {
            if let Some(expected_ext) = target_ext
                && candidate.extension().and_then(|e| e.to_str())
                    != Some(expected_ext)
            {
                continue;
            }
            let distance =
                FolderRef::of(from).distance_to(FolderRef::of(candidate));
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

/// Folder-component trie for one Wikilink basename's candidates.
///
/// Answers nearest-candidate queries in `O(depth(from))` by precomputing each
/// folder's nearest same-basename candidate inside its subtree. The trie's
/// lifetime and mutability shape keeps [`super::inlinks::InlinkMap::new`]'s
/// parallel path `Sync`.
struct CandidateTrie<'a> {
    arena: Arena<TrieNode<'a>>,
    by_folder: FxHashMap<&'a Path, NodeId>,
    root: NodeId,
}

impl<'a> CandidateTrie<'a> {
    fn build(candidates: &[&'a Path]) -> Self {
        let mut arena = Arena::new();
        let root = arena.new_node(Self::new_node_data(0));
        let mut by_folder = FxHashMap::default();
        by_folder.insert(Path::new(""), root);

        for &path in candidates {
            let folder = FolderRef::of(path).as_path();
            let ext = path.extension().and_then(|e| e.to_str());
            let node_id =
                Self::ensure_folder_node(&mut arena, &mut by_folder, folder);
            Self::node_data_mut(&mut arena, node_id).locals.push((ext, path));
        }

        Self::aggregate(&mut arena);
        Self {
            arena,
            by_folder,
            root,
        }
    }

    /// Finds the nearest candidate, returning `None` on extension miss or
    /// nearest tie.
    fn nearest(
        &self,
        from: &Path,
        target_ext: Option<&str>,
    ) -> Option<&'a Path> {
        let from_folder = FolderRef::of(from).as_path();

        // Walk from `from`'s folder to the nearest trie node. `extra_up` counts
        // skipped source-only folders; the linking note's folder is rarely a
        // candidate folder.
        let mut extra_up = 0_usize;
        let mut probe = from_folder;
        let start_node = loop {
            if let Some(&id) = self.by_folder.get(probe) {
                break id;
            }
            match probe.parent() {
                Some(parent) => {
                    probe = parent;
                    extra_up = extra_up.saturating_add(1);
                }
                None => break self.root,
            }
        };

        let mut best = NearestCandidate::EMPTY;
        let mut node_id = Some(start_node);
        let mut prev_child: Option<NodeId> = None;
        let mut up_steps = extra_up;
        while let Some(id) = node_id {
            let node = Self::node_data(&self.arena, id);
            let agg = match prev_child {
                None => node.full.lookup(target_ext),
                Some(child) => node
                    .excluding
                    .iter()
                    .find(|(c, _)| *c == child)
                    .map(|(_, a)| a.lookup(target_ext))
                    .unwrap_or_default(),
            };
            if agg.count > 0 {
                let distance = up_steps
                    .saturating_add(agg.min_depth)
                    .saturating_sub(node.depth);
                best = best.merge(NearestCandidate {
                    min_depth: distance,
                    count: agg.count,
                    sample: agg.sample,
                });
            }
            prev_child = Some(id);
            node_id = id.parent(&self.arena);
            up_steps = up_steps.saturating_add(1);
        }

        if best.count == 1 {
            best.sample
        } else {
            None
        }
    }

    fn new_node_data(depth: usize) -> TrieNode<'a> {
        TrieNode {
            depth,
            locals: Vec::new(),
            full: SubtreeAggregate::default(),
            excluding: Vec::new(),
        }
    }

    /// Returns `folder`'s node, creating missing ancestors first.
    fn ensure_folder_node(
        arena: &mut Arena<TrieNode<'a>>,
        by_folder: &mut FxHashMap<&'a Path, NodeId>,
        folder: &'a Path,
    ) -> NodeId {
        if let Some(&id) = by_folder.get(folder) {
            return id;
        }
        let parent_folder = FolderRef::of(folder).as_path();
        let parent_id =
            Self::ensure_folder_node(arena, by_folder, parent_folder);
        let depth = Self::node_data(arena, parent_id).depth.saturating_add(1);
        let id = arena.new_node(Self::new_node_data(depth));
        parent_id.append(id, arena);
        by_folder.insert(folder, id);
        id
    }

    /// Computes every node's aggregates in one bottom-up pass.
    ///
    /// `Arena::iter_node_ids` yields nodes in insertion (storage) order, and
    /// [`Self::ensure_folder_node`] always creates a parent before any of its
    /// children; reversing that order visits every child before its parent.
    fn aggregate(arena: &mut Arena<TrieNode<'a>>) {
        let insertion_order: Vec<NodeId> = arena.iter_node_ids().collect();
        for &id in insertion_order.iter().rev() {
            let children: Vec<NodeId> = id.children(arena).collect();
            let mut local_agg = SubtreeAggregate::default();
            for &(ext, path) in &Self::node_data(arena, id).locals {
                local_agg = local_agg.merge(SubtreeAggregate::from_local(
                    Self::node_data(arena, id).depth,
                    ext,
                    path,
                ));
            }
            let child_aggs: Vec<SubtreeAggregate<'a>> = children
                .iter()
                .map(|&c| Self::node_data(arena, c).full.clone())
                .collect();

            let mut full = local_agg.clone();
            for agg in &child_aggs {
                full = full.merge(agg.clone());
            }

            let excluding =
                Self::excluding_per_child(&local_agg, &children, &child_aggs);

            let node = Self::node_data_mut(arena, id);
            node.full = full;
            node.excluding = excluding;
        }
    }

    /// Computes per-child exclusion aggregates with one prefix/suffix sweep.
    ///
    /// This avoids re-merging all siblings for each child (`O(children^2)`).
    /// Prefix and suffix vectors are sized to `children.len() + 1`; fallback
    /// reads are defensive only.
    fn excluding_per_child(
        local_agg: &SubtreeAggregate<'a>,
        children: &[NodeId],
        child_aggs: &[SubtreeAggregate<'a>],
    ) -> Vec<(NodeId, SubtreeAggregate<'a>)> {
        let child_count = children.len();
        let mut prefix: Vec<SubtreeAggregate<'a>> =
            Vec::with_capacity(child_count.saturating_add(1));
        prefix.push(SubtreeAggregate::default());
        for agg in child_aggs {
            let last = prefix.last().cloned().unwrap_or_default();
            prefix.push(last.merge(agg.clone()));
        }
        let mut suffix: Vec<SubtreeAggregate<'a>> =
            vec![SubtreeAggregate::default(); child_count.saturating_add(1)];
        for i in (0..child_count).rev() {
            let after =
                suffix.get(i.saturating_add(1)).cloned().unwrap_or_default();
            let child_agg = child_aggs.get(i).cloned().unwrap_or_default();
            if let Some(slot) = suffix.get_mut(i) {
                *slot = child_agg.merge(after);
            }
        }
        children
            .iter()
            .enumerate()
            .map(|(i, &child)| {
                let before = prefix.get(i).cloned().unwrap_or_default();
                let after = suffix
                    .get(i.saturating_add(1))
                    .cloned()
                    .unwrap_or_default();
                let excl = before.merge(after).merge(local_agg.clone());
                (child, excl)
            })
            .collect()
    }

    /// Returns the data of the node with the given id.
    ///
    /// # Panics
    ///
    /// Panics if `id` was removed from `arena`. This arena never removes nodes
    /// after construction, so every `id` it hands out stays valid.
    #[expect(
        clippy::expect_used,
        reason = "indextree NodeId always valid: this arena never removes \
                  nodes after construction"
    )]
    fn node_data<'n>(
        arena: &'n Arena<TrieNode<'a>>,
        id: NodeId,
    ) -> &'n TrieNode<'a> {
        arena
            .get_data(id)
            .expect("indextree NodeId always valid: arena never removes nodes")
    }

    /// Returns the mutable data of the node with the given id.
    ///
    /// # Panics
    ///
    /// Panics under the same invalid-id condition as [`Self::node_data`].
    #[expect(
        clippy::expect_used,
        reason = "indextree NodeId always valid: this arena never removes \
                  nodes after construction; see Self::node_data"
    )]
    fn node_data_mut<'n>(
        arena: &'n mut Arena<TrieNode<'a>>,
        id: NodeId,
    ) -> &'n mut TrieNode<'a> {
        arena
            .get_data_mut(id)
            .expect("indextree NodeId always valid: arena never removes nodes")
    }
}

/// Folder trie node with local candidates and precomputed subtree aggregates.
struct TrieNode<'a> {
    depth: usize,
    /// Candidates whose containing folder is this node.
    ///
    /// Multiple entries imply the same folder and basename with distinct
    /// extensions.
    locals: Vec<(Option<&'a str>, &'a Path)>,
    /// Aggregate for this node's locals and every descendant.
    full: SubtreeAggregate<'a>,
    /// Per-child aggregate excluding that child's subtree.
    ///
    /// Queries walking up through child `c` use this instead of `full` to
    /// avoid counting shallower candidates twice. Stored in a small `Vec`
    /// because same-basename subfolders are usually few.
    excluding: Vec<(NodeId, SubtreeAggregate<'a>)>,
}

/// Subtree aggregate with optional per-extension buckets.
///
/// `by_ext` is a `Vec`, not a `HashMap`: same-basename candidates rarely span
/// enough extensions for hashing and per-node allocation to win.
#[derive(Clone, Debug, Default)]
struct SubtreeAggregate<'a> {
    combined: NearestCandidate<'a>,
    by_ext: Vec<(&'a str, NearestCandidate<'a>)>,
}

impl<'a> SubtreeAggregate<'a> {
    fn from_local(depth: usize, ext: Option<&'a str>, path: &'a Path) -> Self {
        let agg = NearestCandidate::candidate(depth, path);
        Self {
            combined: agg,
            by_ext: ext.into_iter().map(|extension| (extension, agg)).collect(),
        }
    }

    /// Merges unfiltered and per-extension aggregate state.
    fn merge(mut self, other: Self) -> Self {
        self.combined = self.combined.merge(other.combined);
        for (ext, agg) in other.by_ext {
            if let Some(existing) =
                self.by_ext.iter_mut().find(|(e, _)| *e == ext)
            {
                existing.1 = existing.1.merge(agg);
            } else {
                self.by_ext.push((ext, agg));
            }
        }
        self
    }

    fn lookup(&self, target_ext: Option<&str>) -> NearestCandidate<'a> {
        match target_ext {
            None => self.combined,
            Some(ext) => self
                .by_ext
                .iter()
                .find(|(e, _)| *e == ext)
                .map_or(NearestCandidate::EMPTY, |(_, agg)| *agg),
        }
    }
}

/// Minimum candidate depth, tie count, and sample path for one folder
/// aggregate.
///
/// [`Self::EMPTY`] is the merge identity and represents no candidate.
#[derive(Copy, Clone, Debug)]
struct NearestCandidate<'a> {
    min_depth: usize,
    count: usize,
    sample: Option<&'a Path>,
}

impl<'a> NearestCandidate<'a> {
    const EMPTY: Self = Self {
        min_depth: usize::MAX,
        count: 0,
        sample: None,
    };

    fn candidate(depth: usize, path: &'a Path) -> Self {
        Self {
            min_depth: depth,
            count: 1,
            sample: Some(path),
        }
    }

    /// Combines aggregates with closest-distance wins and equal-distance tie
    /// counts.
    fn merge(self, other: Self) -> Self {
        match self.min_depth.cmp(&other.min_depth) {
            std::cmp::Ordering::Less => self,
            std::cmp::Ordering::Greater => other,
            std::cmp::Ordering::Equal => Self {
                min_depth: self.min_depth,
                count: self.count.saturating_add(other.count),
                sample: self.sample.or(other.sample),
            },
        }
    }
}

impl Default for NearestCandidate<'_> {
    fn default() -> Self {
        Self::EMPTY
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    mod candidate_trie {
        use pretty_assertions::assert_eq;

        use super::*;

        /// Brute-force nearest-candidate oracle using pre-trie scan semantics.
        fn naive_nearest<'a>(
            candidates: &[&'a Path],
            from: &Path,
        ) -> Option<&'a Path> {
            let mut nearest: Option<(usize, &'a Path)> = None;
            let mut tied = false;
            for &candidate in candidates {
                let distance =
                    FolderRef::of(from).distance_to(FolderRef::of(candidate));
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

        #[test]
        fn resolves_a_single_candidate_at_its_own_folder() {
            let path = PathBuf::from("a/b/note.md");
            let candidates = [path.as_path()];
            let trie = CandidateTrie::build(&candidates);

            assert_eq!(
                trie.nearest(Path::new("a/b/linking.md"), None),
                Some(path.as_path())
            );
        }

        #[test]
        fn returns_none_when_no_candidate_matches_the_requested_extension() {
            let path = PathBuf::from("a/note.md");
            let candidates = [path.as_path()];
            let trie = CandidateTrie::build(&candidates);

            assert_eq!(
                trie.nearest(Path::new("a/linking.md"), Some("txt")),
                None
            );
        }

        #[test]
        fn isolates_candidates_by_extension_bucket_within_one_folder() {
            let md_path = PathBuf::from("a/note.md");
            let txt_path = PathBuf::from("a/note.txt");
            let other_path = PathBuf::from("b/note.md");
            let candidates =
                [md_path.as_path(), txt_path.as_path(), other_path.as_path()];
            let trie = CandidateTrie::build(&candidates);

            assert_eq!(
                trie.nearest(Path::new("a/linking.md"), Some("txt")),
                Some(txt_path.as_path())
            );
            assert_eq!(
                trie.nearest(Path::new("a/linking.md"), Some("md")),
                Some(md_path.as_path())
            );
        }

        #[test]
        fn agrees_with_naive_scan_over_a_flat_ambiguous_cluster() {
            let paths: Vec<PathBuf> =
                ["a/note.md", "b/note.md", "c/note.md", "note.md"]
                    .iter()
                    .map(PathBuf::from)
                    .collect();
            let candidates: Vec<&Path> =
                paths.iter().map(PathBuf::as_path).collect();
            let trie = CandidateTrie::build(&candidates);

            for from in [
                "a/linking.md",
                "b/x/linking.md",
                "linking.md",
                "d/e/linking.md",
            ] {
                let from = Path::new(from);
                assert_eq!(
                    trie.nearest(from, None),
                    naive_nearest(&candidates, from),
                    "mismatch for from = {from:?}"
                );
            }
        }

        #[test]
        fn agrees_with_naive_scan_over_deeply_nested_and_ties_across_disjoint_clusters()
         {
            let paths: Vec<PathBuf> = [
                "shared/deep/nested/sub/note.md",
                "far/note.md",
                "shared/a/b/note.md",
                "other/note.md",
                "shared/note.md",
                "x/y/z/note.md",
                "x/y/note.md",
            ]
            .iter()
            .map(PathBuf::from)
            .collect();
            let candidates: Vec<&Path> =
                paths.iter().map(PathBuf::as_path).collect();
            let trie = CandidateTrie::build(&candidates);

            for from in [
                "shared/linking.md",
                "x/linking.md",
                "x/y/w/linking.md",
                "other/sub/linking.md",
                "unrelated/branch/linking.md",
            ] {
                let from = Path::new(from);
                assert_eq!(
                    trie.nearest(from, None),
                    naive_nearest(&candidates, from),
                    "mismatch for from = {from:?}"
                );
            }
        }

        mod agrees_with_naive_scan {
            use pretty_assertions::assert_eq;

            use super::*;

            /// Deterministic Linear Congruential Generator (LCG):
            /// `state = state * a + c` mod 2^64. One multiply and one add per
            /// draw, with no dependency on `rand`, reproducible across runs so
            /// a failure is always replayable from the fixed seed.
            fn lcg_next(state: &mut u64) -> u64 {
                *state = state
                    .wrapping_mul(6_364_136_223_846_793_005)
                    .wrapping_add(1_442_695_040_888_963_407);
                *state
            }

            /// Draws one folder component from the four-entry pool this
            /// generator indexes with two bits per draw.
            fn draw_component<'a>(
                components: &[&'a str],
                state: &mut u64,
            ) -> &'a str {
                let idx =
                    usize::try_from(lcg_next(state) >> 62).unwrap_or_default();
                components.get(idx).copied().unwrap_or("a")
            }

            /// Draws a folder depth in `1..=4`.
            fn draw_depth(state: &mut u64) -> usize {
                usize::try_from((lcg_next(state) >> 62).wrapping_add(1))
                    .unwrap_or_default()
            }

            /// Builds one randomized folder-prefixed path: a `1..=4`
            /// component folder prefix plus `file_name`.
            fn draw_folder_prefixed_path(
                components: &[&str],
                state: &mut u64,
                file_name: &str,
            ) -> PathBuf {
                let depth = draw_depth(state);
                let mut path = PathBuf::new();
                for _ in 0..depth {
                    path.push(draw_component(components, state));
                }
                path.push(file_name);
                path
            }

            /// Checks one randomized candidate set against the oracle from
            /// four randomized query points.
            fn assert_random_set_agrees_with_oracle(
                components: &[&str],
                state: &mut u64,
            ) {
                let candidate_count =
                    usize::try_from((lcg_next(state) >> 57).wrapping_add(2))
                        .unwrap_or_default();
                let paths: Vec<PathBuf> = (0..candidate_count)
                    .map(|_| {
                        draw_folder_prefixed_path(components, state, "note.md")
                    })
                    .collect();
                let candidates: Vec<&Path> =
                    paths.iter().map(PathBuf::as_path).collect();
                let trie = CandidateTrie::build(&candidates);

                for from in (0..4).map(|_| {
                    draw_folder_prefixed_path(components, state, "linking.md")
                }) {
                    let from = from.as_path();
                    assert_eq!(
                        trie.nearest(from, None),
                        naive_nearest(&candidates, from),
                        "mismatch for candidates = {candidates:?}, from = \
                         {from:?}"
                    );
                }
            }

            #[test]
            fn matches_the_naive_oracle_on_randomized_folder_sets() {
                let mut state = 0x243f_6a88_85a3_08d3_u64;
                let components: &[&str] = &["a", "b", "c", "d"];
                for _ in 0..128 {
                    assert_random_set_agrees_with_oracle(
                        components, &mut state,
                    );
                }
            }
        }
    }
}
