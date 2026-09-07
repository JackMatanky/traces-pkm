//! Derived inbound link graph computed from indexed outlinks.
//!
//! [`InlinkMap`] represents the full graph of inbound links (backlinks)
//! pointing to each indexed file or note in the personal knowledge base.
//! [`super::IndexerService::build`] and [`super::IndexerService::refresh`]
//! construct an [`InlinkMap`] and persist it into the database; query execution
//! and [`super::FileIndex`] read inbound links directly off
//! [`super::FileEntry`].
//!
//! # Link resolution
//!
//! Building the graph means resolving every [`Note`] outlink to the file it
//! targets. [`LinkResolver`] tries three tiers per link, in order: an exact
//! project-relative path, that path with an implied `.md` extension, and
//! finally, for a bare Obsidian-style wikilink with no path component,
//! whichever indexed file shares the link's file stem and sits nearest the
//! linking note. An ambiguous stem with more than one nearest candidate at the
//! same distance resolves to no target rather than guessing.
//!
//! Nearest-candidate lookup is a folder-distance query: [`StemIndex`] answers
//! it with either a flat linear scan or a precomputed [`CandidateTrie`], chosen
//! per stem by candidate count (see [`TRIE_THRESHOLD`]). Most stems in a real
//! vault are unique filenames with exactly one candidate, so the flat scan
//! handles the overwhelming majority of lookups at zero construction cost; the
//! trie exists purely to keep genuinely ambiguous, large stem clusters off an
//! `O(candidates)` path.

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
///
/// 1. Every target key has at least one inbound source.
/// 2. Sources for each target are strictly deduplicated.
/// 3. Sources for each target are canonically sorted in ascending path order.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct InlinkMap(HashMap<PathBuf, Box<[PathBuf]>>);

impl InlinkMap {
    /// Parses vault notes and file records into a canonical, validated inlink
    /// graph.
    ///
    /// Resolves both internal [`Note`] outlinks and non-note attachment targets
    /// (images, PDFs, audio). Multiple outlinks from the same source note to
    /// the same target collapse into a single edge.
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

    /// Returns the inbound link sources pointing to `target`.
    ///
    /// Returns an empty slice if `target` has no inbound links.
    #[inline]
    #[must_use]
    pub fn inlinks_of(&self, target: &Path) -> &[PathBuf] {
        self.0.get(target).map_or(&[], |sources| sources.as_ref())
    }

    /// Returns an iterator over every `(target, sources)` pair in the graph.
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
    pub fn has_target(&self, target: &Path) -> bool {
        self.0.contains_key(target)
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

    /// Reconstructs an [`InlinkMap`] from trusted, pre-sorted persistence
    /// storage.
    #[inline]
    #[must_use]
    pub(super) fn from_raw(edges: HashMap<PathBuf, Box<[PathBuf]>>) -> Self {
        Self(edges)
    }

    /// Removes every edge sourced by any path in `sources`, dropping a target
    /// left with no remaining inbound sources.
    ///
    /// Takes `&self` rather than consuming: callers (incremental refresh) still
    /// need the original map afterward to diff against the patched result, so
    /// this copies only the *retained* sources rather than cloning the whole
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

    /// Adds `edges` (`(target, source)` pairs), inserting each source into its
    /// target's sorted, deduplicated source list.
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

/// Resolves every outlink in `notes` against `files`, returning
/// `(target, source)` pairs ready for [`InlinkMap::with_edges`].
///
/// Builds one [`LinkResolver`] shared across every note in `notes`, so patching
/// several modified notes at once still pays the resolver's file-stem indexing
/// cost only once.
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

/// Index of files and their stems, used during link resolution.
struct LinkResolver<'a> {
    files: &'a [FileBase],
    stem_index: HashMap<BaseNameRef<'a>, StemIndex<'a>>,
}

impl<'a> LinkResolver<'a> {
    /// Builds the resolver, indexing every file's stem in one `O(n)` pass and
    /// compiling each stem's candidates into a [`StemIndex`].
    fn new(files: &'a [FileBase]) -> Self {
        let mut by_stem: HashMap<BaseNameRef<'a>, Vec<&'a Path>> =
            HashMap::with_capacity(files.len());
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

    /// Resolves every one of `note`'s outlinks against this resolver's indexed
    /// file set, returning deduplicated target paths.
    ///
    /// Pure and I/O-free: reused by [`InlinkMap::new`] (one shared resolver
    /// across every note, cold build) and incremental refresh (the same shared
    /// resolver, called only for modified notes), so resolving a handful of
    /// edited notes never pays the cost of rebuilding the resolver's file-stem
    /// index per note.
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

    /// Looks up `stem`'s [`StemIndex`], if any file has that stem, and finds
    /// the candidate nearest `from` within it.
    fn nearest_by_stem(
        &self,
        stem: &str,
        from: &Path,
        target_ext: Option<&str>,
    ) -> Option<&'a Path> {
        self.stem_index.get(stem)?.nearest(from, target_ext)
    }

    /// Finds the indexed file whose path exactly equals `path`.
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

/// Per-stem candidate index: a flat, linearly scanned list for the common case
/// of a handful of same-stem candidates, or a [`CandidateTrie`] once candidate
/// count crosses [`TRIE_THRESHOLD`], where the trie's `O(depth)` per-query cost
/// beats a flat scan's `O(k)` despite the trie's higher one-time build cost.
///
/// This distinction exists because [`LinkResolver::new`] builds one entry per
/// *distinct file stem* in the whole vault, and the overwhelming majority of
/// stems are unique filenames with exactly one candidate (no ambiguity to
/// resolve at all). Building a multi-node tree for a single-element set is pure
/// overhead: measured, a [`CandidateTrie`] with one candidate takes roughly
/// 2-3x longer to build-and-query-once than a one-element flat scan. Only stems
/// with real, sizeable ambiguity (many files sharing one name) benefit from the
/// trie's precomputed subtree aggregates.
enum StemIndex<'a> {
    Flat(Vec<&'a Path>),
    Trie(Box<CandidateTrie<'a>>),
}

/// Candidate-count threshold above which [`StemIndex::build`] compiles a
/// [`CandidateTrie`] instead of keeping a flat, linearly scanned list.
///
/// Measured via `benches/index_inlinks.rs`'s `deep_paths` group (candidates
/// spread across mostly unique deep folders, one query per candidate): a flat
/// scan and a freshly built trie are within noise of each other around 50-100
/// candidates, and the trie wins by a growing margin from there (2-25x faster
/// by 1,000 candidates). 64 sits just past that measured break-even point, so
/// genuinely ambiguous, sizeable clusters get the trie's asymptotic protection
/// while ordinary vaults, where even a heavily duplicated filename rarely
/// exceeds a few dozen instances, never pay trie construction cost at all.
const TRIE_THRESHOLD: usize = 64;

impl<'a> StemIndex<'a> {
    /// Compiles `candidates` (every file sharing one stem) into whichever
    /// representation suits its size; see [`TRIE_THRESHOLD`].
    fn build(candidates: Vec<&'a Path>) -> Self {
        if candidates.len() >= TRIE_THRESHOLD {
            Self::Trie(Box::new(CandidateTrie::build(&candidates)))
        } else {
            Self::Flat(candidates)
        }
    }

    /// Finds the candidate nearest `from`, dispatching to whichever strategy
    /// this index was built with.
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

    /// The original linear scan: `O(candidates)` per query, no construction
    /// cost. Correct and fast enough below [`TRIE_THRESHOLD`]; see
    /// [`CandidateTrie`]'s doc comment for the `O(depth)` alternative this
    /// falls back from above it.
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

/// A trie over folder-path components for one Wikilink stem's candidates,
/// answering "nearest candidate to `from`, with tie detection" in
/// `O(depth(from))` instead of `O(candidates)`.
///
/// Built fresh per stem, per [`LinkResolver::new`] call: same lifetime and
/// mutability shape as the flat `Vec` it replaces, so it stays `Sync` for
/// [`InlinkMap::new`]'s `notes.par_iter()`.
///
/// [`folder_distance`] between two paths is exactly the graph distance
/// between two nodes in the rooted tree formed by folder paths:
/// `depth(from) + depth(candidate) - 2 * depth(LCA(from, candidate))`. This
/// trie exploits that by precomputing, once per stem, each folder's nearest
/// same-stem candidate *within its own subtree* (bottom-up), then answering
/// each query by walking from `from`'s folder to the trie root and combining
/// each level's "newly visible" candidates (candidates whose nearest shared
/// ancestor with `from` is exactly that level) via the exclusion aggregate on
/// [`TrieNode`].
struct CandidateTrie<'a> {
    nodes: Vec<TrieNode<'a>>,
    by_folder: HashMap<&'a Path, usize>,
}

impl<'a> CandidateTrie<'a> {
    /// Builds a trie over `candidates` (every file sharing one stem).
    fn build(candidates: &[&'a Path]) -> Self {
        let mut nodes = vec![Self::new_node(None, 0)];
        let mut by_folder = HashMap::new();
        by_folder.insert(Path::new(""), 0_usize);

        for &path in candidates {
            let folder = path.parent().unwrap_or_else(|| Path::new(""));
            let ext = path.extension().and_then(|e| e.to_str());
            let node_idx =
                Self::ensure_folder_node(&mut nodes, &mut by_folder, folder);
            Self::node_mut(&mut nodes, node_idx).locals.push((ext, path));
        }

        Self::aggregate(&mut nodes);
        Self {
            nodes,
            by_folder,
        }
    }

    /// Finds the nearest candidate to `from`, or `None` if no candidate exists
    /// (for `target_ext`, when given) or the nearest distance ties.
    fn nearest(
        &self,
        from: &Path,
        target_ext: Option<&str>,
    ) -> Option<&'a Path> {
        let from_folder = from.parent().unwrap_or_else(|| Path::new(""));

        // Walk from_folder's own ancestor chain (independent of the trie) until
        // an ancestor that actually exists as a trie node is found; `extra_up`
        // counts how many of from_folder's own levels were skipped before that
        // first match (from_folder itself is rarely a trie node: it is the
        // *linking* note's folder, not a candidate's).
        let mut extra_up = 0_usize;
        let mut probe = from_folder;
        let start_node = loop {
            if let Some(&idx) = self.by_folder.get(probe) {
                break idx;
            }
            match probe.parent() {
                Some(parent) => {
                    probe = parent;
                    extra_up = extra_up.saturating_add(1);
                }
                None => break 0,
            }
        };

        let mut best = StemAgg::EMPTY;
        let mut node_idx = Some(start_node);
        let mut prev_child: Option<usize> = None;
        let mut i = extra_up;
        while let Some(idx) = node_idx {
            let node = Self::node(&self.nodes, idx);
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
                best = best.merge(StemAgg {
                    min_depth: distance,
                    count: agg.count,
                    sample: agg.sample,
                });
            }
            prev_child = Some(idx);
            node_idx = node.parent;
            i = i.saturating_add(1);
        }

        if best.count == 1 {
            best.sample
        } else {
            None
        }
    }

    /// Creates an empty node with no candidates and no children yet.
    fn new_node(parent: Option<usize>, depth: usize) -> TrieNode<'a> {
        TrieNode {
            parent,
            depth,
            children: Vec::new(),
            locals: Vec::new(),
            full: NodeAgg::default(),
            excluding: Vec::new(),
        }
    }

    /// Returns the node index for `folder`, creating it and every missing
    /// ancestor along the way. A child is only ever created after its parent
    /// already exists, so every child's index is strictly greater than its
    /// parent's; [`Self::aggregate`] relies on this to process children before
    /// parents by walking the arena in reverse.
    fn ensure_folder_node(
        nodes: &mut Vec<TrieNode<'a>>,
        by_folder: &mut HashMap<&'a Path, usize>,
        folder: &'a Path,
    ) -> usize {
        if let Some(&idx) = by_folder.get(folder) {
            return idx;
        }
        let parent_folder = folder.parent().unwrap_or_else(|| Path::new(""));
        let parent_idx =
            Self::ensure_folder_node(nodes, by_folder, parent_folder);
        let depth = Self::node(nodes, parent_idx).depth.saturating_add(1);
        let idx = nodes.len();
        nodes.push(Self::new_node(Some(parent_idx), depth));
        Self::node_mut(nodes, parent_idx).children.push(idx);
        by_folder.insert(folder, idx);
        idx
    }

    /// Computes every node's `full` and `excluding` aggregates in one bottom-up
    /// pass. Processing indices in reverse guarantees every child is finalized
    /// before its parent needs it (see [`Self::ensure_folder_node`]).
    fn aggregate(nodes: &mut [TrieNode<'a>]) {
        for idx in (0..nodes.len()).rev() {
            let children = Self::node(nodes, idx).children.clone();
            let mut local_agg = NodeAgg::default();
            for &(ext, path) in &Self::node(nodes, idx).locals {
                local_agg = local_agg.merge(NodeAgg::from_local(
                    Self::node(nodes, idx).depth,
                    ext,
                    path,
                ));
            }
            let child_aggs: Vec<NodeAgg<'a>> = children
                .iter()
                .map(|&c| Self::node(nodes, c).full.clone())
                .collect();

            let mut full = local_agg.clone();
            for agg in &child_aggs {
                full = full.merge(agg.clone());
            }

            let excluding =
                Self::excluding_per_child(&local_agg, &children, &child_aggs);

            let node = Self::node_mut(nodes, idx);
            node.full = full;
            node.excluding = excluding;
        }
    }

    /// For each child, aggregates `local_agg` with every *other* child's `full`
    /// via a prefix/suffix sweep: `O(children)` total instead of
    /// `O(children^2)` from re-merging all-but-one children from scratch per
    /// child. `.get(..).unwrap_or_default()` rather than indexing: `prefix` and
    /// `suffix` are sized to `children.len() + 1` before either is indexed, so
    /// this never actually falls back, but it costs nothing to let an
    /// off-by-one fail safe instead of panicking.
    fn excluding_per_child(
        local_agg: &NodeAgg<'a>,
        children: &[usize],
        child_aggs: &[NodeAgg<'a>],
    ) -> Vec<(usize, NodeAgg<'a>)> {
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
        (0..n)
            .map(|i| {
                let before = prefix.get(i).cloned().unwrap_or_default();
                let after = suffix
                    .get(i.saturating_add(1))
                    .cloned()
                    .unwrap_or_default();
                let excl = before.merge(after).merge(local_agg.clone());
                let child = children.get(i).copied().unwrap_or_default();
                (child, excl)
            })
            .collect()
    }

    /// Returns the node at `idx`.
    ///
    /// # Panics
    ///
    /// Never in practice: every index this type stores or computes (`parent`,
    /// `children`, `excluding` keys, and every local variable derived from them
    /// below) is only ever produced by [`Self::ensure_folder_node`], which
    /// always returns either an existing, already-valid arena slot or the index
    /// of a slot it just pushed; no stored index can exceed `nodes.len()`.
    #[expect(
        clippy::expect_used,
        reason = "arena index always valid by construction; see doc comment"
    )]
    fn node<'n>(nodes: &'n [TrieNode<'a>], idx: usize) -> &'n TrieNode<'a> {
        nodes.get(idx).expect("trie arena index always valid by construction")
    }

    /// Mutable counterpart of [`Self::node`]; same invariant, same panic
    /// guarantee.
    #[expect(
        clippy::expect_used,
        reason = "arena index always valid by construction; see Self::node"
    )]
    fn node_mut<'n>(
        nodes: &'n mut [TrieNode<'a>],
        idx: usize,
    ) -> &'n mut TrieNode<'a> {
        nodes
            .get_mut(idx)
            .expect("trie arena index always valid by construction")
    }
}

/// One folder in a [`CandidateTrie`]: its depth, its parent and child node
/// indices, the same-stem candidates whose containing folder is exactly this
/// node, and the bottom-up aggregates computed over its subtree.
struct TrieNode<'a> {
    parent: Option<usize>,
    depth: usize,
    children: Vec<usize>,
    /// Candidates in exactly this folder, by extension. Rarely more than one
    /// entry: two candidates here would need the same folder, stem (already
    /// the whole trie's grouping key), and extension, which is the same
    /// path.
    locals: Vec<(Option<&'a str>, &'a Path)>,
    /// This subtree's combined aggregate (this node's `locals` plus every
    /// child's `full`), computed once after every candidate is inserted.
    full: NodeAgg<'a>,
    /// Per-child aggregate of this subtree *excluding* that child's own
    /// subtree, keyed by child node index. A query walking up through child
    /// `c` reads the entry for `c` instead of `full` so `c`'s candidates
    /// (already accounted for at a shallower level) are never
    /// double-counted. A linearly scanned `Vec`, not a `HashMap`, for the
    /// same reason as [`NodeAgg::by_ext`]: most folders have only a handful of
    /// same-stem-candidate subfolders.
    excluding: Vec<(usize, NodeAgg<'a>)>,
}

/// One trie node's bottom-up aggregate: the extension-unfiltered `combined`
/// statistics plus a per-extension breakdown, so
/// [`LinkResolver::nearest_by_stem`] can answer an extension-filtered query
/// without rescanning candidates.
///
/// `by_ext` is a linearly scanned `Vec`, not a `HashMap`: real vaults rarely
/// have more than one or two distinct extensions among same-stem candidates (in
/// practice almost always just `.md`), so a `HashMap`'s hashing overhead and
/// per-node allocation would cost more than it saves.
#[derive(Clone, Debug, Default)]
struct NodeAgg<'a> {
    combined: StemAgg<'a>,
    by_ext: Vec<(&'a str, StemAgg<'a>)>,
}

impl<'a> NodeAgg<'a> {
    /// Builds the single-candidate aggregate for one file at `depth`.
    fn from_local(depth: usize, ext: Option<&'a str>, path: &'a Path) -> Self {
        let agg = StemAgg::candidate(depth, path);
        Self {
            combined: agg,
            by_ext: ext.into_iter().map(|extension| (extension, agg)).collect(),
        }
    }

    /// Merges `other` into `self`, combining both the unfiltered aggregate and
    /// each side's per-extension breakdown.
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

    /// Looks up this node's aggregate for `target_ext`; `None` means every
    /// candidate in the subtree without extension filtering.
    fn lookup(&self, target_ext: Option<&str>) -> StemAgg<'a> {
        match target_ext {
            None => self.combined,
            Some(ext) => self
                .by_ext
                .iter()
                .find(|(e, _)| *e == ext)
                .map_or(StemAgg::EMPTY, |(_, agg)| *agg),
        }
    }
}

/// One folder's minimum same-stem-candidate depth and tie count, optionally
/// scoped to one file extension.
///
/// `min_depth == usize::MAX` (via [`Self::EMPTY`]) represents "no candidate";
/// [`Self::merge`] treats it as the identity element, so folding `EMPTY` into
/// any real aggregate is a no-op in either direction.
#[derive(Copy, Clone, Debug)]
struct StemAgg<'a> {
    min_depth: usize,
    count: usize,
    sample: Option<&'a Path>,
}

impl<'a> StemAgg<'a> {
    const EMPTY: Self = Self {
        min_depth: usize::MAX,
        count: 0,
        sample: None,
    };

    /// Builds the aggregate for a single candidate at `depth`.
    fn candidate(depth: usize, path: &'a Path) -> Self {
        Self {
            min_depth: depth,
            count: 1,
            sample: Some(path),
        }
    }

    /// Combines two aggregates, keeping the smaller `min_depth` and summing
    /// `count` when they tie: the same "closest wins, equal-distance ties
    /// accumulate" rule [`LinkResolver::nearest_by_stem`] used before this trie
    /// existed.
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

impl Default for StemAgg<'_> {
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

        #[test]
        fn resolves_to_a_distant_shallow_sibling_over_a_close_deep_nesting() {
            // Candidate A shares a real ancestor folder with `from` ("shared")
            // but is nested three levels deeper under it. Candidate B shares
            // no ancestor with `from` beyond the root, but sits directly off
            // the root. Total path-segment distance still favors B (2) over
            // A (3): folder_distance counts total hops, not "shares a
            // meaningful ancestor", which guards against a shortcut that
            // stops walking as soon as it finds any non-empty ancestor level.
            let files = files_from_notes(&[
                "shared/deep/nested/sub/note.md",
                "far/note.md",
            ]);

            assert_eq!(
                resolve(&files, "shared/linking.md", LinkTarget::Path("note")),
                Some(Target(Path::new("far/note.md")))
            );
        }

        #[test]
        fn resolves_none_for_a_tie_spanning_an_ancestor_and_a_descendant_branch()
         {
            // Candidate A sits two levels below `from`'s own folder ("shared").
            // Candidate B sits one level below a folder ("other") sharing
            // only the root with `from`. Both are at true distance 2, via
            // structurally asymmetric branches (one nested under an ancestor
            // of `from`, the other off a disjoint root-level folder); unlike
            // the existing symmetric-sibling tie test, this exercises the
            // exclusion aggregate combining a candidate already counted at a
            // shallower level with a genuinely new one at a deeper level,
            // without double- or under-counting either.
            let files =
                files_from_notes(&["shared/a/b/note.md", "other/note.md"]);

            assert_eq!(
                resolve(&files, "shared/linking.md", LinkTarget::Path("note")),
                None
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

    // Direct unit tests for `folder_distance`, the flat-scan path's distance
    // primitive.
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
            // 2 up from c to a, 2 down from a to e.
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

    // Direct unit tests for `CandidateTrie`, differentially checked against a
    // brute-force linear scan over many query points.
    mod candidate_trie {
        use pretty_assertions::assert_eq;

        use super::*;

        /// Brute-force reference implementation mirroring the pre-trie
        /// `nearest_by_stem` scan, used to differentially check
        /// [`CandidateTrie`] against many query points without duplicating
        /// the trie's own aggregate logic in the assertions below.
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
    }
}
