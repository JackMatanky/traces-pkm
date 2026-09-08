//! Derived inbound link graph computed from indexed outlinks.
//!
//! [`InlinkMap`] stores deduplicated source paths keyed by each indexed target.
//! [`super::IndexerService::build`] and [`super::IndexerService::refresh`]
//! construct and persist it; query execution reads inlinks from
//! [`super::FileEntry`] through [`super::FileIndex`].
//!
//! # Link resolution
//!
//! [`LinkResolver`] resolves each [`Note`] outlink by trying an exact path, the
//! same path with an implied `.md` extension, then the nearest indexed file
//! sharing a basename-only Wikilink stem. Equal-distance stem matches remain
//! unresolved.
//!
//! [`StemIndex`] chooses a flat scan or [`CandidateTrie`] per stem (see
//! [`TRIE_THRESHOLD`]). Flat scan avoids trie construction for the common
//! unique-stem case; the trie keeps large ambiguous clusters off an
//! `O(candidates)` lookup path.

use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};

use indextree::{Arena, NodeId};
use rayon::prelude::*;
use rustc_hash::{FxBuildHasher, FxHashMap};

use crate::{
    BaseNameRef, FileBase,
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

    /// Groups sorted, deduplicated edge pairs into canonical source lists.
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
        Self(edges)
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

/// Resolves notes into edge pairs for [`InlinkMap::with_edges`].
///
/// Shares one [`LinkResolver`] so batch refresh pays stem-index construction
/// once.
pub(super) fn resolve_edges_for(
    notes: &[Note],
    files: &[FileBase],
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

/// Path and stem index used during link resolution.
struct LinkResolver<'a> {
    files: &'a [FileBase],
    /// `rustc_hash::FxHashMap`: measured against `SipHash` under matched
    /// conditions (`benches/index_inlinks`, `deep_paths` group,
    /// 2026-09-08) and found 9-11% faster at 1,000+ candidates (e.g.
    /// `deep_paths/20000`: 17.242ms -> 15.652ms). Neither this map nor
    /// [`CandidateTrie::by_folder`] keys on attacker-controlled input (both
    /// are built from the vault's own file paths), so `SipHash`'s `DoS`
    /// resistance buys nothing here.
    stem_index: FxHashMap<BaseNameRef<'a>, StemIndex<'a>>,
}

impl<'a> LinkResolver<'a> {
    /// Indexes file stems in one `O(n)` pass.
    fn new(files: &'a [FileBase]) -> Self {
        let mut by_stem: FxHashMap<BaseNameRef<'a>, Vec<&'a Path>> =
            FxHashMap::with_capacity_and_hasher(files.len(), FxBuildHasher);
        for file in files {
            let path = file.path();
            if let Some(stem) = BaseNameRef::from_path(path) {
                by_stem.entry(stem).or_default().push(path);
            }
        }
        let stem_index = by_stem
            .into_iter()
            .map(|(stem, candidates)| (stem, StemIndex::build(candidates)))
            .collect();
        Self {
            files,
            stem_index,
        }
    }

    /// Resolves one note's outlinks to deduplicated target paths.
    ///
    /// Pure and I/O-free; callers can share one resolver across many notes
    /// instead of rebuilding the stem index per note.
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
    /// Tries exact path, missing-`.md` path, then nearest basename-stem match
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
        self.stem_index.get(stem)?.nearest(from, target_ext)
    }

    fn find_by_path(&self, path: &Path) -> Option<&'a Path> {
        self.files
            .binary_search_by(|file| file.path().cmp(path))
            .ok()
            .and_then(|i| self.files.get(i))
            .map(FileBase::path)
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

/// Per-stem candidate index.
///
/// Small clusters use flat scans to avoid trie construction. Large ambiguous
/// clusters use [`CandidateTrie`] to avoid `O(candidates)` lookup cost; the
/// switch point is [`TRIE_THRESHOLD`].
enum StemIndex<'a> {
    Flat(Vec<&'a Path>),
    Trie(Box<CandidateTrie<'a>>),
}

/// Candidate count at which [`StemIndex::build`] switches to [`CandidateTrie`].
///
/// `benches/index_inlinks.rs` shows flat scan and fresh-trie build within noise
/// around 50-100 candidates, with trie 2-25x faster by 1,000 candidates. `64`
/// sits just past that break-even point while ordinary duplicated filenames
/// avoid trie construction.
const TRIE_THRESHOLD: usize = 64;

impl<'a> StemIndex<'a> {
    fn build(candidates: Vec<&'a Path>) -> Self {
        if candidates.len() >= TRIE_THRESHOLD {
            Self::Trie(Box::new(CandidateTrie::build(&candidates)))
        } else {
            Self::Flat(candidates)
        }
    }

    fn nearest(
        &self,
        from: &Path,
        target_ext: Option<&str>,
    ) -> Option<&'a Path> {
        match self {
            Self::Flat(candidates) => {
                Self::nearest_flat(candidates, from, target_ext)
            }
            Self::Trie(trie) => trie.nearest(from, target_ext),
        }
    }

    /// Finds the nearest candidate by linear scan.
    ///
    /// Equal-distance nearest candidates resolve to `None`; below
    /// [`TRIE_THRESHOLD`], this is cheaper than building a trie.
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

/// Containing folder of a path, used to compute tree distance between linking
/// notes and candidates.
#[derive(Copy, Clone)]
struct Folder<'a>(&'a Path);

impl<'a> Folder<'a> {
    /// Returns `path`'s containing folder, defaulting to the root for
    /// top-level paths.
    fn of(path: &'a Path) -> Self {
        Self(path.parent().unwrap_or_else(|| Path::new("")))
    }

    /// Computes folder path distance in one pass.
    ///
    /// The same folder has distance `0`; otherwise distance is the hops up to
    /// the nearest shared ancestor plus down to the other folder.
    fn distance_to(self, other: Folder<'_>) -> usize {
        let mut a_iter = self.0.components();
        let mut b_iter = other.0.components();
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
                    a_count = a_count
                        .saturating_add(1)
                        .saturating_add(a_iter.count());
                    break;
                }
                (None, Some(_)) => {
                    b_count = b_count
                        .saturating_add(1)
                        .saturating_add(b_iter.count());
                    break;
                }
                (None, None) => break,
            }
        }
        a_count
            .saturating_sub(shared)
            .saturating_add(b_count.saturating_sub(shared))
    }
}

/// Computes containing-folder path distance in one pass.
///
/// Files in the same folder have distance `0`; otherwise distance is the hops
/// up to the nearest shared ancestor plus down to the other folder.
fn folder_distance(a: &Path, b: &Path) -> usize {
    Folder::of(a).distance_to(Folder::of(b))
}

/// Folder-component trie for one Wikilink stem's candidates.
///
/// Answers nearest-candidate queries in `O(depth(from))` by precomputing each
/// folder's nearest same-stem candidate inside its subtree. It has the same
/// lifetime and mutability shape as the flat `Vec`, so [`InlinkMap::new`]'s
/// `notes.par_iter()` path remains `Sync`.
///
/// [`folder_distance`] equals tree distance:
/// `depth(from) + depth(candidate) - 2 * depth(LCA(from, candidate))`.
/// Querying walks from `from`'s folder toward the root and combines candidates
/// newly visible at each level through [`TrieNode`]'s exclusion aggregates.
struct CandidateTrie<'a> {
    arena: Arena<TrieNode<'a>>,
    /// `rustc_hash::FxHashMap`, for the same measured reason as
    /// [`LinkResolver::stem_index`].
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
            let folder = Folder::of(path).0;
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
        let from_folder = Folder::of(from).0;

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
        let mut i = extra_up;
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
                let distance =
                    i.saturating_add(agg.min_depth).saturating_sub(node.depth);
                best = best.merge(NearestCandidate {
                    min_depth: distance,
                    count: agg.count,
                    sample: agg.sample,
                });
            }
            prev_child = Some(id);
            node_id = id.parent(&self.arena);
            i = i.saturating_add(1);
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
            full: NodeAgg::default(),
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
        let parent_folder = Folder::of(folder).0;
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
            let mut local_agg = NodeAgg::default();
            for &(ext, path) in &Self::node_data(arena, id).locals {
                local_agg = local_agg.merge(NodeAgg::from_local(
                    Self::node_data(arena, id).depth,
                    ext,
                    path,
                ));
            }
            let child_aggs: Vec<NodeAgg<'a>> = children
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
        local_agg: &NodeAgg<'a>,
        children: &[NodeId],
        child_aggs: &[NodeAgg<'a>],
    ) -> Vec<(NodeId, NodeAgg<'a>)> {
        let n = children.len();
        let mut prefix: Vec<NodeAgg<'a>> =
            Vec::with_capacity(n.saturating_add(1));
        prefix.push(NodeAgg::default());
        for agg in child_aggs {
            let last = prefix.last().cloned().unwrap_or_default();
            prefix.push(last.merge(agg.clone()));
        }
        let mut suffix: Vec<NodeAgg<'a>> =
            vec![NodeAgg::default(); n.saturating_add(1)];
        for i in (0..n).rev() {
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
    /// Multiple entries imply the same folder and stem with distinct
    /// extensions.
    locals: Vec<(Option<&'a str>, &'a Path)>,
    /// Aggregate for this node's locals and every descendant.
    full: NodeAgg<'a>,
    /// Per-child aggregate excluding that child's subtree.
    ///
    /// Queries walking up through child `c` use this instead of `full` to
    /// avoid counting shallower candidates twice. Stored in a small `Vec`
    /// because same-stem subfolders are usually few.
    excluding: Vec<(NodeId, NodeAgg<'a>)>,
}

/// Subtree aggregate with optional per-extension buckets.
///
/// `by_ext` is a `Vec`, not a `HashMap`: same-stem candidates rarely span
/// enough extensions for hashing and per-node allocation to win.
#[derive(Clone, Debug, Default)]
struct NodeAgg<'a> {
    combined: NearestCandidate<'a>,
    by_ext: Vec<(&'a str, NearestCandidate<'a>)>,
}

impl<'a> NodeAgg<'a> {
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

    mod link_resolver {

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

        mod stem_fallback {
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
                fn resolves_ambiguous_stem_match_to_nearest_candidate() {
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
                fn returns_none_for_ambiguous_stem_match_at_equal_distance() {
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
                fn resolves_ambiguous_self_referential_stem_to_itself() {
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
                fn resolves_basename_with_extension_by_stem() {
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
                fn resolves_basename_with_matching_extension_by_stem() {
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
            fn returns_none_for_unmatched_qualified_path_without_stem_fallback()
            {
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
                extra_files: &[FileBase],
            ) -> InlinkMap {
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

                assert_eq!(inlinks.len(), 1);

                for (_, sources) in inlinks.iter() {
                    assert!(!sources.is_empty());
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
                let inlinks = InlinkMap::default();
                assert!(
                    inlinks.inlinks_of(Path::new("nonexistent.md")).is_empty()
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
                        [Path::new("a.md")].into_iter().collect();

                    let patched = inlinks.without_sources(&stale);

                    assert!(!patched.has_target(Path::new("target.md")));
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

                #[test]
                fn removing_an_absent_source_keeps_the_graph_unchanged() {
                    let inlinks = graph_with_two_sources();
                    let stale: HashSet<&Path> =
                        [Path::new("missing.md")].into_iter().collect();

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

    mod folder_distance {
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
                4
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

    mod stem_index {
        use super::*;

        mod dispatch {
            use pretty_assertions::assert_eq;

            use super::*;

            fn same_stem_files(count: usize) -> Vec<FileBase> {
                let paths: Vec<String> =
                    (0..count).map(|i| format!("d{i}/note.md")).collect();
                let mut files: Vec<FileBase> =
                    paths.iter().map(|p| file_for_note(p)).collect();
                files.sort_by(|a, b| a.path().cmp(b.path()));
                files
            }

            #[test]
            fn resolves_identically_at_the_trie_threshold_boundary() {
                for count in [TRIE_THRESHOLD - 1, TRIE_THRESHOLD] {
                    let files = same_stem_files(count);
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
            let mut files: Vec<FileBase> =
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
