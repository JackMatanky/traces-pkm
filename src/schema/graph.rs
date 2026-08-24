//! DAG bookkeeping for the `extends` relationship, linearized via Kahn's
//! topological sort.
//!
//! [`SchemaGraph`] owns graph mechanics so [`super::resolver`] can drive
//! resolution order without tangling traversal into field-merge logic.
//! Adjacency is stored as a CSR (compressed sparse row) graph over dense
//! [`u32`] indices, making lookups array accesses rather than hash probes.
//!
//! # Ordering
//!
//! A Schema is yielded only after every one of its present `extends` parents
//! has already been yielded. Schemas whose in-degree reaches zero
//! simultaneously yield in raw-map insertion order.
//! [`SchemaGraphBuilder::new`]'s `excluded` parameter keeps the Global Schema
//! out of the graph entirely, so it never competes for queue position.
//!
//! # Driving resolution
//!
//! 1. [`SchemaGraphBuilder::new`] validates every `extends` edge; `excluded`
//!    (the Global Schema) never becomes a node.
//! 2. [`SchemaGraphBuilder::build`] runs topological sort to completion and
//!    checks for a cycle, entirely internally; there is no stepwise driving
//!    API. Returns a [`SchemaGraph`] on success, or the list of cyclic Schemas
//!    on failure.
//! 3. Query the resulting [`SchemaGraph`]: [`topological_order`],
//!    [`parents_of`], [`children_by_name`], [`descendants_by_name`].
//!
//! [`topological_order`]: SchemaGraph::topological_order
//! [`parents_of`]: SchemaGraph::parents_of
//! [`children_by_name`]: SchemaGraph::children_by_name
//! [`descendants_by_name`]: SchemaGraph::descendants_by_name

use std::collections::VecDeque;

use bit_vec::BitVec;
use indexmap::{IndexMap, IndexSet};

use super::{RawSchema, SchemaName, SchemaNameRef, error::SchemaWarning};

/// A validated, acyclic `extends` DAG in topological order. Only constructible
/// via [`SchemaGraphBuilder::build`], so querying hierarchy is impossible
/// before cycle-checking.
#[derive(Debug)]
pub(super) struct SchemaGraph<'a> {
    adjacency: SchemaAdjacency<'a>,
    /// Every resolved Schema, in topological order (parents before children;
    /// simultaneous roots in raw-map insertion order).
    topological_order: IndexSet<SchemaNameRef<'a>>,
}

impl<'a> SchemaGraph<'a> {
    /// Every resolved Schema, in topological order.
    #[must_use]
    pub(super) fn topological_order(
        &self,
    ) -> impl Iterator<Item = SchemaNameRef<'a>> + '_ {
        self.topological_order.iter().copied()
    }

    /// Borrow `name`'s raw `extends` parent list. Returns an empty slice if
    /// `name` is not a known Schema or is the excluded Global Schema.
    #[must_use]
    pub(super) fn parents_of(&self, name: SchemaNameRef<'_>) -> &[SchemaName] {
        self.adjacency.parents_of(name)
    }

    /// Every Schema's direct `extends` children, keyed by parent name. Only
    /// Schemas with at least one child appear.
    #[must_use]
    pub(super) fn children_by_name(
        &self,
    ) -> IndexMap<SchemaNameRef<'a>, Vec<SchemaNameRef<'a>>> {
        let mut map: IndexMap<SchemaNameRef<'a>, Vec<SchemaNameRef<'a>>> =
            IndexMap::new();
        for (i, &name) in self.adjacency.names.iter().enumerate() {
            let slice =
                self.adjacency.children_slice(DenseIndex::from_usize(i));
            if slice.is_empty() {
                continue;
            }
            let children: Vec<SchemaNameRef<'a>> = slice
                .iter()
                .filter_map(|&child| self.adjacency.names.get(child.index()))
                .copied()
                .collect();
            map.insert(name, children);
        }
        map
    }

    /// Every Schema's transitive `extends` descendants, keyed by ancestor name.
    /// Output-sensitive Habib-Morvan-Rampon transitive closure, `O(V + E +
    /// Σ|closure(x)|)`, proportional to the closure's actual size rather than
    /// the `O(V²/w)` a bitset DP pays unconditionally. Requires children
    /// iterated in topological-rank order, which [`SchemaGraphBuilder::build`]
    /// already sorted `child_targets` into.
    #[must_use]
    pub(super) fn descendants_by_name(
        &self,
    ) -> IndexMap<SchemaName, IndexSet<SchemaName>> {
        let count = self.adjacency.node_count();
        let mut topo_rank: Vec<u32> = vec![0; count];
        for (rank, &name) in self.topological_order.iter().enumerate() {
            if let Some(index) = self.adjacency.index_of(name)
                && let Some(slot) = topo_rank.get_mut(index.index())
            {
                *slot = DenseIndex::saturating_u32(rank);
            }
        }

        // Leaves first, roots last: each child's closure is already finalized
        // (later topological position) before its parent runs.
        let mut accumulator = DescendantAccumulator::new(count, topo_rank);
        for &name in self.topological_order.iter().rev() {
            let Some(node) = self.adjacency.index_of(name) else {
                continue;
            };
            accumulator.accumulate(node, self.adjacency.children_slice(node));
        }

        let descendant_lists = accumulator.into_descendant_lists();
        let mut result: IndexMap<SchemaName, IndexSet<SchemaName>> =
            IndexMap::new();
        for (i, &name) in self.adjacency.names.iter().enumerate() {
            let Some(node_descendants) = descendant_lists.get(i) else {
                continue;
            };
            if node_descendants.is_empty() {
                continue;
            }
            let descendants: IndexSet<SchemaName> = node_descendants
                .iter()
                .filter_map(|&d| self.adjacency.names.get(d.index()))
                .map(|&n| SchemaName::from(n))
                .collect();
            result.insert(SchemaName::from(name), descendants);
        }
        result
    }
}

/// Drives topological sort over a [`SchemaAdjacency`] to completion. Build with
/// [`new`](Self::new), then call [`build`](Self::build) once.
pub(super) struct SchemaGraphBuilder<'a> {
    adjacency: SchemaAdjacency<'a>,
    /// Per-node count of parents not yet resolved.
    unresolved_parent_count: Vec<u32>,
    /// Nodes whose `unresolved_parent_count` reached zero, dense-indexed.
    ready_queue: VecDeque<DenseIndex>,
    /// Schemas already popped from `ready_queue`, in topological order.
    visited: IndexSet<SchemaNameRef<'a>>,
}

impl<'a> SchemaGraphBuilder<'a> {
    /// Build the adjacency and seed the ready queue. See
    /// [`SchemaAdjacency::build`] for the warnings this can return.
    ///
    /// # Arguments
    ///
    /// * `raw` - All raw Schemas, keyed by name.
    /// * `excluded` - The Global Schema name, excluded from the graph.
    pub(super) fn new(
        raw: &'a IndexMap<SchemaName, RawSchema>,
        excluded: SchemaNameRef<'a>,
    ) -> (Self, Vec<SchemaWarning>) {
        let (adjacency, warnings) = SchemaAdjacency::build(raw, excluded);
        let node_count = adjacency.node_count();

        // Derive each node's in-degree from the CSR adjacency itself
        // (occurrences of a node across every parent's children list) rather
        // than re-scanning raw `extends` lists a second time.
        let mut unresolved_parent_count: Vec<u32> = vec![0; node_count];
        for &child in &adjacency.child_targets {
            if let Some(count) = unresolved_parent_count.get_mut(child.index())
            {
                *count = count.saturating_add(1);
            }
        }

        let ready_queue: VecDeque<DenseIndex> = (0
            ..DenseIndex::saturating_u32(node_count))
            .map(DenseIndex)
            .filter(|idx| {
                unresolved_parent_count.get(idx.index()).copied() == Some(0)
            })
            .collect();

        (
            Self {
                adjacency,
                unresolved_parent_count,
                ready_queue,
                visited: IndexSet::new(),
            },
            warnings,
        )
    }

    /// Drain any remaining `next_ready`/`mark_resolved` steps, check for a
    /// cycle via [`CycleDetector`], and, if acyclic, sort every node's CSR
    /// children into topological-rank order (a precondition
    /// [`SchemaGraph::descendants_by_name`]'s closure sweep relies on).
    ///
    /// Returns the completed [`SchemaGraph`] when no cycle exists.
    ///
    /// # Errors
    ///
    /// Returns every Schema that participates in an `extends` cycle, in
    /// declaration order. A Schema that merely `extends` into a cycle without
    /// being part of one itself is excluded.
    pub(super) fn build(mut self) -> Result<SchemaGraph<'a>, Vec<SchemaName>> {
        while let Some(name) = self.next_ready() {
            self.mark_resolved(name);
        }
        let cyclic = CycleDetector::new(&self.adjacency, &self.visited).find();
        if !cyclic.is_empty() {
            return Err(cyclic);
        }

        let mut topo_rank: Vec<u32> = vec![0; self.adjacency.node_count()];
        for (rank, &name) in self.visited.iter().enumerate() {
            if let Some(index) = self.adjacency.index_of(name)
                && let Some(slot) = topo_rank.get_mut(index.index())
            {
                *slot = DenseIndex::saturating_u32(rank);
            }
        }
        let rank_of = |c: &DenseIndex| {
            topo_rank.get(c.index()).copied().unwrap_or(u32::MAX)
        };
        for i in 0..DenseIndex::saturating_u32(self.adjacency.node_count()) {
            self.adjacency
                .children_slice_mut(DenseIndex(i))
                .sort_by_key(rank_of);
        }

        Ok(SchemaGraph {
            adjacency: self.adjacency,
            topological_order: self.visited,
        })
    }

    /// Pop the next Schema whose `unresolved_parent_count` reached zero,
    /// marking it visited. `None` once `ready_queue` drains.
    fn next_ready(&mut self) -> Option<SchemaNameRef<'a>> {
        let index = self.ready_queue.pop_front()?;
        let name = self.adjacency.name_of(index)?;
        self.visited.insert(name);
        Some(name)
    }

    /// Record `name` as resolved, releasing children whose
    /// `unresolved_parent_count` hit zero into `ready_queue`.
    fn mark_resolved(&mut self, name: SchemaNameRef<'_>) {
        let Some(index) = self.adjacency.index_of(name) else {
            return;
        };
        for &child in self.adjacency.children_slice(index) {
            let Some(count) =
                self.unresolved_parent_count.get_mut(child.index())
            else {
                continue;
            };
            *count = count.saturating_sub(1);
            if *count == 0 {
                self.ready_queue.push_back(child);
            }
        }
    }
}

/// `extends` adjacency between Schemas, stored as a CSR (compressed sparse row)
/// graph over dense [`DenseIndex`]es: array accesses replace hash-keyed lookups
/// on every graph operation.
///
/// `excluded` is never assigned a node: the Global Schema participates via
/// `$ref`, never `extends`, so it must never compete for topological position
/// or appear in any hierarchy query.
#[derive(Debug)]
struct SchemaAdjacency<'a> {
    /// Every raw Schema, including `excluded`, kept unfiltered (not a
    /// clone-then-strip copy) so building this adjacency never deep-clones the
    /// registry; `excluded` is skipped by index, not by omission from this
    /// map. [`parents_of`](Self::parents_of) guards against `excluded`
    /// explicitly, since a lookup here would otherwise still find its raw
    /// `extends` list.
    raw: &'a IndexMap<SchemaName, RawSchema>,
    /// The one Schema name that never receives a [`DenseIndex`]. Checked by
    /// [`parents_of`](Self::parents_of) directly, since `raw` stays unfiltered
    /// and would otherwise still yield `excluded`'s own raw `extends` list.
    excluded: SchemaNameRef<'a>,
    /// Dense index -> name, in raw-map insertion order, `excluded` skipped.
    names: Vec<SchemaNameRef<'a>>,
    /// Name -> dense index, built once at construction.
    index_of: IndexMap<SchemaNameRef<'a>, DenseIndex>,
    /// CSR adjacency (parent -> children) offsets: node `i`'s children occupy
    /// `child_targets[child_offsets[i]..child_offsets[i + 1]]`.
    child_offsets: Vec<u32>,
    /// CSR adjacency (parent -> children) targets, indexed via
    /// [`child_offsets`](Self::child_offsets).
    child_targets: Vec<DenseIndex>,
}

impl<'a> SchemaAdjacency<'a> {
    /// Build the `extends` adjacency, skipping `excluded` entirely: it never
    /// becomes a node, and an edge naming it as an `extends` target is silently
    /// ignored, never a [`MissingExtendsTarget`] warning, since the Global
    /// Schema is referenced via `$ref`, never `extends`.
    ///
    /// # Warnings
    ///
    /// - [`DuplicateExtendsTarget`] if the same `extends` target appears more
    ///   than once, checked before target-existence; a repeated unresolvable
    ///   target warns `Missing` on its first occurrence and `Duplicate` on
    ///   every occurrence after
    /// - [`MissingExtendsTarget`] if an `extends` target has no corresponding
    ///   Schema file (other than `excluded`)
    ///
    /// [`MissingExtendsTarget`]: SchemaWarning::MissingExtendsTarget
    /// [`DuplicateExtendsTarget`]: SchemaWarning::DuplicateExtendsTarget
    fn build(
        raw: &'a IndexMap<SchemaName, RawSchema>,
        excluded: SchemaNameRef<'a>,
    ) -> (Self, Vec<SchemaWarning>) {
        let mut warnings = Vec::new();

        // Assign dense indices in raw-map insertion order, `excluded` never
        // receiving one.
        let names: Vec<SchemaNameRef<'a>> = raw
            .keys()
            .map(SchemaName::as_ref)
            .filter(|&name| name != excluded)
            .collect();
        let count = names.len();
        let index_of: IndexMap<SchemaNameRef<'a>, DenseIndex> = names
            .iter()
            .enumerate()
            .map(|(i, &name)| (name, DenseIndex::from_usize(i)))
            .collect();

        // Validate + dedup each schema's `extends` targets, accumulating
        // out-degree (child count) per node (in-degree is derived separately by
        // `SchemaGraphBuilder::new` from the CSR adjacency itself), and the
        // validated edge list the CSR fill pass needs.
        let mut out_degree: Vec<u32> = vec![0; count];
        let mut valid_edges: Vec<(DenseIndex, DenseIndex)> = Vec::new();
        for (i, &name) in names.iter().enumerate() {
            let Some(raw_schema) = raw.get(name.as_str()) else {
                continue;
            };
            let mut seen_targets = IndexSet::new();
            for target in &raw_schema.extends {
                if target.as_str() == excluded.as_str() {
                    continue;
                }
                if !seen_targets.insert(target.as_str()) {
                    warnings.push(SchemaWarning::DuplicateExtendsTarget {
                        schema: SchemaName::from(name),
                        target: target.clone(),
                    });
                    continue;
                }
                let Some(&target_index) = index_of.get(target.as_str()) else {
                    warnings.push(SchemaWarning::MissingExtendsTarget {
                        schema: SchemaName::from(name),
                        target: target.clone(),
                    });
                    continue;
                };
                if let Some(degree) = out_degree.get_mut(target_index.index()) {
                    *degree = degree.saturating_add(1);
                }
                valid_edges.push((DenseIndex::from_usize(i), target_index));
            }
        }

        let mut child_offsets: Vec<u32> =
            Vec::with_capacity(count.saturating_add(1));
        let mut running: u32 = 0;
        child_offsets.push(running);
        for &out in &out_degree {
            running = running.saturating_add(out);
            child_offsets.push(running);
        }
        let edge_count = DenseIndex::widen_usize(running);

        // Fill CSR targets via a scratch write cursor per node.
        let mut cursor: Vec<u32> =
            child_offsets.iter().take(count).copied().collect();
        let mut child_targets: Vec<DenseIndex> =
            vec![DenseIndex(0); edge_count];
        for (schema_index, target_index) in valid_edges {
            let Some(slot) = cursor.get_mut(target_index.index()) else {
                continue;
            };
            if let Some(target_slot) =
                child_targets.get_mut(DenseIndex::widen_usize(*slot))
            {
                *target_slot = schema_index;
            }
            *slot = slot.saturating_add(1);
        }

        (
            Self {
                raw,
                excluded,
                names,
                index_of,
                child_offsets,
                child_targets,
            },
            warnings,
        )
    }

    /// `node`'s CSR children slice (its direct `extends` reverse-adjacency), or
    /// an empty slice if `node` is out of range.
    fn children_slice(&self, node: DenseIndex) -> &[DenseIndex] {
        let idx = node.index();
        let bounds = self
            .child_offsets
            .get(idx)
            .zip(self.child_offsets.get(idx.saturating_add(1)));
        let Some((&start, &end)) = bounds else {
            return &[];
        };
        self.child_targets
            .get(DenseIndex::widen_usize(start)..DenseIndex::widen_usize(end))
            .unwrap_or(&[])
    }

    /// Mutable counterpart of [`children_slice`](Self::children_slice), used
    /// only to sort a node's children in place.
    fn children_slice_mut(&mut self, node: DenseIndex) -> &mut [DenseIndex] {
        let idx = node.index();
        let start = self.child_offsets.get(idx).copied();
        let end = self.child_offsets.get(idx.saturating_add(1)).copied();
        match (start, end) {
            (Some(s), Some(e)) => self
                .child_targets
                .get_mut(DenseIndex::widen_usize(s)..DenseIndex::widen_usize(e))
                .unwrap_or(&mut []),
            _ => &mut [],
        }
    }

    /// Borrow `name`'s raw `extends` parent list. Returns an empty slice if
    /// `name` is not a known Schema, or is `excluded`.
    fn parents_of(&self, name: SchemaNameRef<'_>) -> &[SchemaName] {
        if name == self.excluded {
            return &[];
        }
        self.raw.get(name.as_str()).map_or(&[], |s| s.extends.as_slice())
    }

    /// `name`'s dense index, or `None` if unknown or `excluded`.
    fn index_of(&self, name: SchemaNameRef<'_>) -> Option<DenseIndex> {
        self.index_of.get(name.as_str()).copied()
    }

    /// The name at dense index `node`, or `None` if out of range.
    fn name_of(&self, node: DenseIndex) -> Option<SchemaNameRef<'a>> {
        self.names.get(node.index()).copied()
    }

    /// Number of nodes (Schemas excluding `excluded`).
    fn node_count(&self) -> usize {
        self.names.len()
    }
}

/// Finds every Schema participating in an `extends` cycle, restricted to the
/// subgraph the topological sort never resolved (a resolved node is provably
/// acyclic, since it reached in-degree zero). Iterative
/// strongly-connected-components search (explicit work-stack, no recursion)
/// over that unvisited subgraph, walking `extends` backward (parent-direction,
/// via [`SchemaAdjacency::parents_of`]) rather than the forward adjacency the
/// rest of the graph uses. `O(V + E)` over the unvisited subgraph.
struct CycleDetector<'a, 'b> {
    adjacency: &'b SchemaAdjacency<'a>,
    /// The Schemas Kahn's sort already resolved; the search only explores
    /// what's left.
    visited: &'b IndexSet<SchemaNameRef<'a>>,
    search: CycleSearchState,
    cyclic: Vec<SchemaName>,
}

impl<'a> CycleDetector<'a, '_> {
    fn new<'b>(
        adjacency: &'b SchemaAdjacency<'a>,
        visited: &'b IndexSet<SchemaNameRef<'a>>,
    ) -> CycleDetector<'a, 'b> {
        CycleDetector {
            adjacency,
            visited,
            search: CycleSearchState::new(adjacency.node_count()),
            cyclic: Vec::new(),
        }
    }

    /// Return every unvisited Schema that participates in a cycle, in
    /// declaration order. Empty if `visited` already covers every node (no
    /// cycle exists).
    fn find(mut self) -> Vec<SchemaName> {
        if self.visited.len() == self.adjacency.node_count() {
            return Vec::new();
        }
        for start in 0..DenseIndex::saturating_u32(self.adjacency.node_count())
        {
            let start = DenseIndex(start);
            if self.is_kahn_visited(start) || self.search.is_discovered(start) {
                continue;
            }
            self.explore_from(start);
        }
        self.cyclic
    }

    /// Whether `node` was already resolved by Kahn's sort.
    fn is_kahn_visited(&self, node: DenseIndex) -> bool {
        self.adjacency
            .name_of(node)
            .is_some_and(|name| self.visited.contains(&name))
    }

    /// Explicit work-stack DFS (no recursion) rooted at `start`, extracting
    /// every nontrivial strongly-connected component (or self-loop) into
    /// `self.cyclic`.
    fn explore_from(&mut self, start: DenseIndex) {
        let mut work_stack: Vec<(DenseIndex, usize)> = vec![(start, 0)];
        while let Some(&(node, scan_pos)) = work_stack.last() {
            if scan_pos == 0 {
                self.search.discover(node);
            }
            match self.next_unvisited_parent(node, scan_pos) {
                Some((parent, next_pos)) => {
                    self.advance(&mut work_stack, node, parent, next_pos);
                }
                None => self.finish_node(&mut work_stack, node),
            }
        }
    }

    /// Scan `node`'s raw `extends` list from `from`, skipping unknown or
    /// Kahn-visited targets, returning the first unvisited-and-known parent and
    /// the position to resume from. `O(V + E)` total via the
    /// monotonically-advancing `from`.
    fn next_unvisited_parent(
        &self,
        node: DenseIndex,
        from: usize,
    ) -> Option<(DenseIndex, usize)> {
        let name = self.adjacency.name_of(node)?;
        let parents = self.adjacency.parents_of(name);
        let mut pos = from;
        while let Some(parent) = parents.get(pos) {
            pos = pos.saturating_add(1);
            if self.visited.contains(parent.as_str()) {
                continue;
            }
            if let Some(target) = self.adjacency.index_of(parent.as_ref()) {
                return Some((target, pos));
            }
        }
        None
    }

    /// Whether `node`'s raw `extends` list contains a valid, unvisited
    /// self-reference.
    fn has_self_extend(&self, node: DenseIndex) -> bool {
        let Some(name) = self.adjacency.name_of(node) else {
            return false;
        };
        self.adjacency.parents_of(name).iter().any(|parent| {
            !self.visited.contains(parent.as_str())
                && self.adjacency.index_of(parent.as_ref()) == Some(node)
        })
    }

    /// Advance `node`'s frame to `resume_at`, then descend into `parent` if
    /// undiscovered, or fold `parent`'s discovery order into `node`'s
    /// lowest-reachable rank if `parent` is a live back edge.
    fn advance(
        &mut self,
        work_stack: &mut Vec<(DenseIndex, usize)>,
        node: DenseIndex,
        parent: DenseIndex,
        resume_at: usize,
    ) {
        if let Some(frame) = work_stack.last_mut() {
            frame.1 = resume_at;
        }
        if !self.search.is_discovered(parent) {
            work_stack.push((parent, 0));
            return;
        }
        if !self.search.is_on_stack(parent) {
            return;
        }
        if let Some(parent_order) = self.search.discovery_order_of(parent) {
            self.search.lower_reachable(node, parent_order);
        }
    }

    /// `node` is fully explored: if it is a component root, pop and record its
    /// strongly-connected component (if nontrivial or self-looping) into
    /// `self.cyclic` in insertion order (matching the resolver's
    /// error-reporting convention), then propagate its lowest-reachable rank to
    /// its caller.
    fn finish_node(
        &mut self,
        work_stack: &mut Vec<(DenseIndex, usize)>,
        node: DenseIndex,
    ) {
        let node_order = self.search.discovery_order_of(node);
        if node_order.is_some()
            && node_order == self.search.lowest_reachable_of(node)
        {
            let mut component = self.search.pop_component(node);
            let is_cyclic = component.len() > 1 || self.has_self_extend(node);
            if is_cyclic {
                // Insertion order, not Tarjan pop order: matches the
                // resolver's error-reporting convention.
                component.sort_by_key(|idx| idx.0);
                let adjacency = &self.adjacency;
                self.cyclic.extend(
                    component
                        .iter()
                        .filter_map(|&idx| adjacency.name_of(idx))
                        .map(SchemaName::from),
                );
            }
        }
        work_stack.pop();
        let Some(&(parent, _)) = work_stack.last() else {
            return;
        };
        if let Some(node_lowest) = self.search.lowest_reachable_of(node) {
            self.search.lower_reachable(parent, node_lowest);
        }
    }
}

/// Per-node bookkeeping for [`CycleDetector`]'s strongly-connected-components
/// search, sized to the full unvisited-subgraph node count; entries for
/// unreached nodes stay at their initial value.
struct CycleSearchState {
    /// Discovery order, assigned once per node on first visit. (Renamed from
    /// `index` to avoid colliding with [`DenseIndex::index`]'s meaning.)
    discovery_order: Vec<Option<u32>>,
    /// Lowest discovery order reachable from each node via tree or back edges
    /// (Tarjan's "lowlink").
    lowest_reachable: Vec<u32>,
    /// Whether each node is currently on `component_stack`.
    on_stack: Vec<bool>,
    /// Strongly-connected-component-accumulation stack, in discovery order.
    /// (Renamed from `stack` to distinguish it from the DFS traversal
    /// work-stack in `CycleDetector::explore_from`, a separate stack.)
    component_stack: Vec<DenseIndex>,
    /// Next discovery order to assign.
    next_discovery_order: u32,
}

impl CycleSearchState {
    fn new(node_count: usize) -> Self {
        Self {
            discovery_order: vec![None; node_count],
            lowest_reachable: vec![0; node_count],
            on_stack: vec![false; node_count],
            component_stack: Vec::new(),
            next_discovery_order: 0,
        }
    }

    /// Whether `node` has been assigned a Tarjan discovery order.
    fn is_discovered(&self, node: DenseIndex) -> bool {
        self.discovery_order.get(node.index()).copied().flatten().is_some()
    }

    /// `node`'s discovery order, or `None` if undiscovered.
    fn discovery_order_of(&self, node: DenseIndex) -> Option<u32> {
        self.discovery_order.get(node.index()).copied().flatten()
    }

    /// `node`'s current lowest-reachable rank, or `None` if undiscovered.
    fn lowest_reachable_of(&self, node: DenseIndex) -> Option<u32> {
        self.lowest_reachable.get(node.index()).copied()
    }

    /// Whether `node` is currently on the strongly-connected-component stack
    /// (not the traversal work-stack).
    fn is_on_stack(&self, node: DenseIndex) -> bool {
        self.on_stack.get(node.index()).copied().unwrap_or(false)
    }

    /// Assign `node` its discovery order/lowest-reachable rank and push it onto
    /// `component_stack`.
    fn discover(&mut self, node: DenseIndex) {
        let rank = self.next_discovery_order;
        self.next_discovery_order = self.next_discovery_order.saturating_add(1);
        if let Some(slot) = self.discovery_order.get_mut(node.index()) {
            *slot = Some(rank);
        }
        if let Some(slot) = self.lowest_reachable.get_mut(node.index()) {
            *slot = rank;
        }
        self.component_stack.push(node);
        if let Some(slot) = self.on_stack.get_mut(node.index()) {
            *slot = true;
        }
    }

    /// Fold `candidate` into `node`'s lowest-reachable rank, keeping the
    /// smaller value.
    fn lower_reachable(&mut self, node: DenseIndex, candidate: u32) {
        if let Some(slot) = self.lowest_reachable.get_mut(node.index()) {
            *slot = (*slot).min(candidate);
        }
    }

    /// Pop the strongly-connected component rooted at `root` off
    /// `component_stack` (most-recently-discovered first).
    fn pop_component(&mut self, root: DenseIndex) -> Vec<DenseIndex> {
        let mut component = Vec::new();
        while let Some(popped) = self.component_stack.pop() {
            if let Some(slot) = self.on_stack.get_mut(popped.index()) {
                *slot = false;
            }
            let is_root = popped == root;
            component.push(popped);
            if is_root {
                break;
            }
        }
        component
    }
}

/// Per-node transitive-descendant accumulator for
/// [`SchemaGraph::descendants_by_name`]'s reverse-topological sweep. `seen` is
/// scratch, cleared before each node's `accumulate` call returns, so
/// deduplication costs `O(|descendants(node)|)` rather than a full-array clear
/// per node.
struct DescendantAccumulator {
    seen: BitVec,
    /// `descendants[i]` is dense index `i`'s accumulated descendant list,
    /// finalized once every node in topological order has been swept
    /// (leaves first).
    descendants: Vec<Vec<DenseIndex>>,
    /// Topological rank per node, used to keep each finalized descendant
    /// list in globally topological order rather than
    /// child-processing-interleaved order; downstream callers (e.g.
    /// Template `.descendants() | map(attribute='name') | join(',')`)
    /// observe this order directly.
    rank: Vec<u32>,
}

impl DescendantAccumulator {
    fn new(node_count: usize, rank: Vec<u32>) -> Self {
        Self {
            seen: BitVec::from_elem(node_count, false),
            descendants: vec![Vec::new(); node_count],
            rank,
        }
    }

    /// Accumulate `node`'s direct-plus-inherited descendants from its
    /// already-topological-rank-sorted `children`.
    fn accumulate(&mut self, node: DenseIndex, children: &[DenseIndex]) {
        for &child in children {
            self.merge_child(node, child);
        }
        let Some(node_descendants) = self.descendants.get_mut(node.index())
        else {
            return;
        };
        // Required: without this, node_descendants' order is
        // child-processing-interleaved, not globally topological.
        let rank = &self.rank;
        node_descendants
            .sort_by_key(|d| rank.get(d.index()).copied().unwrap_or(u32::MAX));
        for &descendant in node_descendants.iter() {
            self.seen.set(descendant.index(), false);
        }
    }

    /// Fold `child`'s already-finalized descendant list into `node`'s,
    /// deduplicating via `self.seen`.
    fn merge_child(&mut self, node: DenseIndex, child: DenseIndex) {
        if self.seen.get(child.index()) == Some(true) {
            return;
        }
        self.seen.set(child.index(), true);
        self.record(node, child);
        let child_descendants: Vec<DenseIndex> =
            self.descendants.get(child.index()).cloned().unwrap_or_default();
        for descendant in child_descendants {
            if self.seen.get(descendant.index()) == Some(true) {
                continue;
            }
            self.seen.set(descendant.index(), true);
            self.record(node, descendant);
        }
    }

    /// Append `descendant` to `node`'s list. No-op if `node` is out of range.
    fn record(&mut self, node: DenseIndex, descendant: DenseIndex) {
        if let Some(list) = self.descendants.get_mut(node.index()) {
            list.push(descendant);
        }
    }

    /// Consume the accumulator, returning every node's finalized descendant
    /// list.
    fn into_descendant_lists(self) -> Vec<Vec<DenseIndex>> {
        self.descendants
    }
}

/// Dense per-schema array index, assigned once at construction in raw-map
/// insertion order. Array indexing replaces string-hashed
/// [`HashMap`]/[`IndexMap`] lookups on every graph operation.
///
/// Caps a schema set at `u32::MAX` (roughly 4 billion) entries, which is not a
/// real constraint for a filesystem-enumerated `.traces/schemas/*.toml`
/// registry.
///
/// [`HashMap`]: std::collections::HashMap
/// [`IndexMap`]: indexmap::IndexMap
#[derive(Copy, Clone, Debug, Eq, Hash, PartialEq)]
struct DenseIndex(u32);

impl DenseIndex {
    /// Narrows `n` to `u32`, saturating to `u32::MAX` on overflow. Values this
    /// module narrows stay far below `u32::MAX` for any real schema registry.
    fn saturating_u32(n: usize) -> u32 {
        u32::try_from(n).unwrap_or(u32::MAX)
    }

    /// Builds a dense index from an array position, saturating per
    /// [`Self::saturating_u32`].
    fn from_usize(n: usize) -> Self {
        Self(Self::saturating_u32(n))
    }

    /// Widens back to `usize` for indexing. See [`Self::widen_usize`].
    fn index(self) -> usize {
        Self::widen_usize(self.0)
    }

    /// Widens `n` to `usize`. `usize` is at least as wide as `u32` on every
    /// platform this crate targets, so `try_from` failing here is unreachable
    /// in practice; it saturates to `usize::MAX` rather than panicking.
    fn widen_usize(n: u32) -> usize {
        usize::try_from(n).unwrap_or(usize::MAX)
    }
}

#[cfg(test)]
mod tests {
    use super::{super::GLOBAL_SCHEMA_NAME, *};

    /// Builds an empty [`RawSchema`] extending `extends`.
    fn schema(extends: &[&str]) -> RawSchema {
        RawSchema {
            extends: extends.iter().map(|&s| SchemaName::from(s)).collect(),
            ..RawSchema::default()
        }
    }

    mod constructor {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn returns_a_duplicate_extends_target_warning() {
            let mut raw = IndexMap::new();
            raw.insert(SchemaName::from("book"), schema(&[]));
            raw.insert(SchemaName::from("child"), schema(&["book", "book"]));
            let (_builder, warnings) = SchemaGraphBuilder::new(
                &raw,
                SchemaNameRef::from(GLOBAL_SCHEMA_NAME),
            );

            assert_eq!(warnings, vec![SchemaWarning::DuplicateExtendsTarget {
                schema: SchemaName::from("child"),
                target: SchemaName::from("book"),
            }]);
        }

        #[test]
        fn returns_missing_extends_target_warning() {
            let mut raw = IndexMap::new();
            raw.insert(SchemaName::from("child"), schema(&["nonexistent"]));
            let (_builder, warnings) = SchemaGraphBuilder::new(
                &raw,
                SchemaNameRef::from(GLOBAL_SCHEMA_NAME),
            );

            assert_eq!(warnings, vec![SchemaWarning::MissingExtendsTarget {
                schema: SchemaName::from("child"),
                target: SchemaName::from("nonexistent"),
            }]);
        }

        #[test]
        fn returns_missing_then_duplicate_for_a_repeated_unresolvable_target() {
            let mut raw = IndexMap::new();
            raw.insert(
                SchemaName::from("child"),
                schema(&["missing", "missing"]),
            );
            let (_builder, warnings) = SchemaGraphBuilder::new(
                &raw,
                SchemaNameRef::from(GLOBAL_SCHEMA_NAME),
            );

            assert_eq!(warnings, vec![
                SchemaWarning::MissingExtendsTarget {
                    schema: SchemaName::from("child"),
                    target: SchemaName::from("missing"),
                },
                SchemaWarning::DuplicateExtendsTarget {
                    schema: SchemaName::from("child"),
                    target: SchemaName::from("missing"),
                },
            ]);
        }

        #[test]
        fn returns_no_warnings_for_empty_input() {
            let raw = IndexMap::new();
            let (_builder, warnings) = SchemaGraphBuilder::new(
                &raw,
                SchemaNameRef::from(GLOBAL_SCHEMA_NAME),
            );

            assert!(warnings.is_empty());
        }

        #[test]
        fn does_not_warn_when_global_is_present_in_raw() {
            let mut raw = IndexMap::new();
            raw.insert(SchemaName::from(GLOBAL_SCHEMA_NAME), schema(&[]));
            raw.insert(SchemaName::from("book"), schema(&[GLOBAL_SCHEMA_NAME]));
            let (builder, warnings) = SchemaGraphBuilder::new(
                &raw,
                SchemaNameRef::from(GLOBAL_SCHEMA_NAME),
            );
            assert!(warnings.is_empty());

            let graph = builder.build().expect("acyclic fixture resolves");
            let order: Vec<SchemaNameRef<'_>> =
                graph.topological_order().collect();
            assert!(order.contains(&SchemaNameRef::from("book")));
            assert!(!order.contains(&SchemaNameRef::from(GLOBAL_SCHEMA_NAME)));
        }
    }

    mod parents {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn returns_raw_extends_for_book_extending_global() {
            let mut raw = IndexMap::new();
            raw.insert(SchemaName::from(GLOBAL_SCHEMA_NAME), schema(&[]));
            raw.insert(SchemaName::from("book"), schema(&[GLOBAL_SCHEMA_NAME]));
            let (builder, _warnings) = SchemaGraphBuilder::new(
                &raw,
                SchemaNameRef::from(GLOBAL_SCHEMA_NAME),
            );
            let graph = builder.build().expect("acyclic fixture resolves");

            assert_eq!(graph.parents_of(SchemaNameRef::from("book")), &[
                SchemaName::from(GLOBAL_SCHEMA_NAME)
            ]);
        }

        #[test]
        fn returns_raw_extends_including_duplicates() {
            let mut raw = IndexMap::new();
            raw.insert(SchemaName::from("book"), schema(&[]));
            raw.insert(SchemaName::from("child"), schema(&["book", "book"]));
            let (builder, _warnings) = SchemaGraphBuilder::new(
                &raw,
                SchemaNameRef::from(GLOBAL_SCHEMA_NAME),
            );
            let graph = builder.build().expect("acyclic fixture resolves");

            assert_eq!(graph.parents_of(SchemaNameRef::from("child")), &[
                SchemaName::from("book"),
                SchemaName::from("book"),
            ]);
        }

        #[test]
        fn returns_empty_slice_for_unknown_schema() {
            let mut raw = IndexMap::new();
            raw.insert(SchemaName::from("book"), schema(&[]));
            let (builder, _warnings) = SchemaGraphBuilder::new(
                &raw,
                SchemaNameRef::from(GLOBAL_SCHEMA_NAME),
            );
            let graph = builder.build().expect("acyclic fixture resolves");

            assert_eq!(graph.parents_of(SchemaNameRef::from("missing")), &[]);
        }

        #[test]
        fn returns_empty_slice_for_the_excluded_schema_even_with_its_own_extends()
         {
            let mut raw = IndexMap::new();
            raw.insert(SchemaName::from(GLOBAL_SCHEMA_NAME), schema(&["book"]));
            raw.insert(SchemaName::from("book"), schema(&[]));
            let (builder, _warnings) = SchemaGraphBuilder::new(
                &raw,
                SchemaNameRef::from(GLOBAL_SCHEMA_NAME),
            );
            let graph = builder.build().expect("acyclic fixture resolves");

            assert_eq!(
                graph.parents_of(SchemaNameRef::from(GLOBAL_SCHEMA_NAME)),
                &[]
            );
        }
    }

    mod topological_order {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn returns_roots_in_declaration_order() {
            let mut raw = IndexMap::new();
            raw.insert(SchemaName::from("author"), schema(&[]));
            raw.insert(SchemaName::from("book"), schema(&["author"]));
            let (builder, _warnings) = SchemaGraphBuilder::new(
                &raw,
                SchemaNameRef::from(GLOBAL_SCHEMA_NAME),
            );
            let graph = builder.build().expect("acyclic fixture resolves");

            assert_eq!(graph.topological_order().collect::<Vec<_>>(), vec![
                SchemaNameRef::from("author"),
                SchemaNameRef::from("book"),
            ]);
        }

        #[test]
        fn releases_multiple_simultaneous_roots_in_raw_map_insertion_order() {
            let mut raw = IndexMap::new();
            raw.insert(SchemaName::from("zebra"), schema(&[]));
            raw.insert(SchemaName::from("apple"), schema(&[]));
            raw.insert(SchemaName::from("mango"), schema(&[]));
            let (builder, _warnings) = SchemaGraphBuilder::new(
                &raw,
                SchemaNameRef::from(GLOBAL_SCHEMA_NAME),
            );
            let graph = builder.build().expect("acyclic fixture resolves");

            assert_eq!(graph.topological_order().collect::<Vec<_>>(), vec![
                SchemaNameRef::from("zebra"),
                SchemaNameRef::from("apple"),
                SchemaNameRef::from("mango"),
            ]);
        }
    }

    mod children {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn returns_only_direct_extenders() {
            // thing <- book <- {sci_fi, memoir}
            let mut raw = IndexMap::new();
            raw.insert(SchemaName::from("thing"), schema(&[]));
            raw.insert(SchemaName::from("book"), schema(&["thing"]));
            raw.insert(SchemaName::from("sci_fi"), schema(&["book"]));
            raw.insert(SchemaName::from("memoir"), schema(&["book"]));
            let graph = SchemaGraphBuilder::new(
                &raw,
                SchemaNameRef::from(GLOBAL_SCHEMA_NAME),
            )
            .0
            .build()
            .unwrap();
            let children = graph.children_by_name();

            let thing_children: Vec<&str> = children
                .get("thing")
                .unwrap()
                .iter()
                .map(|n| n.as_str())
                .collect();
            assert_eq!(thing_children, vec!["book"]);
            let book_children: Vec<&str> = children
                .get("book")
                .unwrap()
                .iter()
                .map(|n| n.as_str())
                .collect();
            assert_eq!(book_children, vec!["sci_fi", "memoir"]);
            assert_eq!(children.get("sci_fi"), None);
            assert_eq!(children.get("memoir"), None);
        }

        #[test]
        fn includes_schema_in_every_parents_direct_children() {
            // thing <- {book, film} <- adaptation (both parents)
            let mut raw = IndexMap::new();
            raw.insert(SchemaName::from("thing"), schema(&[]));
            raw.insert(SchemaName::from("book"), schema(&["thing"]));
            raw.insert(SchemaName::from("film"), schema(&["thing"]));
            raw.insert(
                SchemaName::from("adaptation"),
                schema(&["book", "film"]),
            );
            let graph = SchemaGraphBuilder::new(
                &raw,
                SchemaNameRef::from(GLOBAL_SCHEMA_NAME),
            )
            .0
            .build()
            .unwrap();
            let children = graph.children_by_name();

            let book_children: Vec<&str> = children
                .get("book")
                .unwrap()
                .iter()
                .map(|n| n.as_str())
                .collect();
            assert_eq!(book_children, vec!["adaptation"]);
            let film_children: Vec<&str> = children
                .get("film")
                .unwrap()
                .iter()
                .map(|n| n.as_str())
                .collect();
            assert_eq!(film_children, vec!["adaptation"]);
            let thing_children: Vec<&str> = children
                .get("thing")
                .unwrap()
                .iter()
                .map(|n| n.as_str())
                .collect();
            assert_eq!(thing_children, vec!["book", "film"]);
        }

        #[test]
        fn returns_empty_map_when_no_schema_has_children() {
            let mut raw = IndexMap::new();
            raw.insert(SchemaName::from("a"), schema(&[]));
            raw.insert(SchemaName::from("b"), schema(&[]));
            let graph = SchemaGraphBuilder::new(
                &raw,
                SchemaNameRef::from(GLOBAL_SCHEMA_NAME),
            )
            .0
            .build()
            .unwrap();
            let children = graph.children_by_name();

            assert!(children.is_empty());
        }
    }

    mod descendants {
        use pretty_assertions::assert_eq;

        use super::*;

        fn set(names: &[&str]) -> IndexSet<SchemaName> {
            names.iter().map(|&name| SchemaName::from(name)).collect()
        }

        #[test]
        fn deduplicates_diamond_shared_descendant() {
            // thing <- {book, film} <- adaptation (both parents)
            let mut raw = IndexMap::new();
            raw.insert(SchemaName::from("thing"), schema(&[]));
            raw.insert(SchemaName::from("book"), schema(&["thing"]));
            raw.insert(SchemaName::from("film"), schema(&["thing"]));
            raw.insert(
                SchemaName::from("adaptation"),
                schema(&["book", "film"]),
            );
            let graph = SchemaGraphBuilder::new(
                &raw,
                SchemaNameRef::from(GLOBAL_SCHEMA_NAME),
            )
            .0
            .build()
            .unwrap();
            let descendants = graph.descendants_by_name();

            assert_eq!(
                descendants.get("thing"),
                Some(&set(&["adaptation", "book", "film"]))
            );
        }

        #[test]
        fn returns_full_transitive_closure() {
            let mut raw = IndexMap::new();
            raw.insert(SchemaName::from("thing"), schema(&[]));
            raw.insert(SchemaName::from("book"), schema(&["thing"]));
            raw.insert(SchemaName::from("sci_fi"), schema(&["book"]));
            raw.insert(SchemaName::from("space_opera"), schema(&["sci_fi"]));
            let graph = SchemaGraphBuilder::new(
                &raw,
                SchemaNameRef::from(GLOBAL_SCHEMA_NAME),
            )
            .0
            .build()
            .unwrap();
            let descendants = graph.descendants_by_name();

            assert_eq!(
                descendants.get("thing"),
                Some(&set(&["book", "sci_fi", "space_opera"]))
            );
            assert_eq!(
                descendants.get("book"),
                Some(&set(&["sci_fi", "space_opera"]))
            );
            assert_eq!(descendants.get("sci_fi"), Some(&set(&["space_opera"])));
            assert_eq!(descendants.get("space_opera"), None);
        }

        #[test]
        fn excludes_leaf_from_result_map() {
            let mut raw = IndexMap::new();
            raw.insert(SchemaName::from("book"), schema(&[]));
            raw.insert(SchemaName::from("sci_fi"), schema(&["book"]));
            let graph = SchemaGraphBuilder::new(
                &raw,
                SchemaNameRef::from(GLOBAL_SCHEMA_NAME),
            )
            .0
            .build()
            .unwrap();
            let descendants = graph.descendants_by_name();

            assert_eq!(descendants.get("sci_fi"), None);
        }

        #[test]
        fn returns_independent_sets_for_multiple_roots() {
            let mut raw = IndexMap::new();
            raw.insert(SchemaName::from("a"), schema(&[]));
            raw.insert(SchemaName::from("b"), schema(&["a"]));
            raw.insert(SchemaName::from("c"), schema(&[]));
            raw.insert(SchemaName::from("d"), schema(&["c"]));
            let graph = SchemaGraphBuilder::new(
                &raw,
                SchemaNameRef::from(GLOBAL_SCHEMA_NAME),
            )
            .0
            .build()
            .unwrap();
            let descendants = graph.descendants_by_name();

            assert_eq!(descendants.get("a"), Some(&set(&["b"])));
            assert_eq!(descendants.get("c"), Some(&set(&["d"])));
            assert_eq!(descendants.get("b"), None);
            assert_eq!(descendants.get("d"), None);
        }

        #[test]
        fn preserves_declaration_order_in_descendant_sets() {
            // Raw schemas are declared as: thing, book, film, adaptation
            // adaptation extends book and film.
            // The descendant set for "thing" must list names in the order
            // they appear in the raw index, not in BFS traversal order.
            let mut raw = IndexMap::new();
            raw.insert(SchemaName::from("thing"), schema(&[]));
            raw.insert(SchemaName::from("book"), schema(&["thing"]));
            raw.insert(SchemaName::from("film"), schema(&["thing"]));
            raw.insert(
                SchemaName::from("adaptation"),
                schema(&["book", "film"]),
            );
            let graph = SchemaGraphBuilder::new(
                &raw,
                SchemaNameRef::from(GLOBAL_SCHEMA_NAME),
            )
            .0
            .build()
            .unwrap();
            let descendants = graph.descendants_by_name();

            let expected: IndexSet<SchemaName> = ["adaptation", "book", "film"]
                .iter()
                .map(|&s| SchemaName::from(s))
                .collect();
            assert_eq!(descendants.get("thing"), Some(&expected));
        }
    }

    mod integrity {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn returns_the_insertion_order_index() {
            let mut raw = IndexMap::new();
            raw.insert(SchemaName::from("alpha"), schema(&[]));
            raw.insert(SchemaName::from("book"), schema(&[]));
            raw.insert(SchemaName::from("sci_fi"), schema(&[]));
            let (builder, _warnings) = SchemaGraphBuilder::new(
                &raw,
                SchemaNameRef::from(GLOBAL_SCHEMA_NAME),
            );

            assert_eq!(
                builder
                    .adjacency
                    .index_of(SchemaNameRef::from("alpha"))
                    .map(|i| i.0),
                Some(0)
            );
            assert_eq!(
                builder
                    .adjacency
                    .index_of(SchemaNameRef::from("book"))
                    .map(|i| i.0),
                Some(1)
            );
            assert_eq!(
                builder
                    .adjacency
                    .index_of(SchemaNameRef::from("sci_fi"))
                    .map(|i| i.0),
                Some(2)
            );
            assert_eq!(
                builder.adjacency.index_of(SchemaNameRef::from("missing")),
                None
            );
        }

        #[test]
        fn returns_the_name_at_the_given_index() {
            let mut raw = IndexMap::new();
            raw.insert(SchemaName::from("alpha"), schema(&[]));
            raw.insert(SchemaName::from("book"), schema(&[]));
            let (builder, _warnings) = SchemaGraphBuilder::new(
                &raw,
                SchemaNameRef::from(GLOBAL_SCHEMA_NAME),
            );

            assert_eq!(
                builder.adjacency.name_of(DenseIndex(0)),
                Some(SchemaNameRef::from("alpha"))
            );
            assert_eq!(
                builder.adjacency.name_of(DenseIndex(1)),
                Some(SchemaNameRef::from("book"))
            );
            assert_eq!(builder.adjacency.name_of(DenseIndex(2)), None);
        }

        #[test]
        fn index_count_matches_the_number_of_names() {
            let mut raw = IndexMap::new();
            raw.insert(SchemaName::from("a"), schema(&[]));
            raw.insert(SchemaName::from("b"), schema(&[]));
            raw.insert(SchemaName::from("c"), schema(&[]));
            let (builder, _warnings) = SchemaGraphBuilder::new(
                &raw,
                SchemaNameRef::from(GLOBAL_SCHEMA_NAME),
            );

            assert_eq!(builder.adjacency.node_count(), 3);
        }
    }

    mod cycles {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn rejects_a_direct_two_node_cycle() {
            let mut raw = IndexMap::new();
            raw.insert(SchemaName::from("a"), schema(&["b"]));
            raw.insert(SchemaName::from("b"), schema(&["a"]));
            let (builder, _warnings) = SchemaGraphBuilder::new(
                &raw,
                SchemaNameRef::from(GLOBAL_SCHEMA_NAME),
            );

            let err = builder.build().expect_err("cycle rejected");
            assert_eq!(err, vec![SchemaName::from("a"), SchemaName::from("b")]);
        }

        #[test]
        fn excludes_a_schema_that_only_extends_into_the_cycle() {
            let mut raw = IndexMap::new();
            raw.insert(SchemaName::from("a"), schema(&["b"]));
            raw.insert(SchemaName::from("b"), schema(&["a"]));
            raw.insert(SchemaName::from("c"), schema(&["a"]));
            let (builder, _warnings) = SchemaGraphBuilder::new(
                &raw,
                SchemaNameRef::from(GLOBAL_SCHEMA_NAME),
            );

            let err = builder.build().expect_err("cycle rejected");
            assert_eq!(err, vec![SchemaName::from("a"), SchemaName::from("b")]);
        }

        #[test]
        fn rejects_a_three_node_cycle_in_declaration_order() {
            let mut raw = IndexMap::new();
            raw.insert(SchemaName::from("a"), schema(&["b"]));
            raw.insert(SchemaName::from("b"), schema(&["c"]));
            raw.insert(SchemaName::from("c"), schema(&["a"]));
            let (builder, _warnings) = SchemaGraphBuilder::new(
                &raw,
                SchemaNameRef::from(GLOBAL_SCHEMA_NAME),
            );

            let err = builder.build().expect_err("cycle rejected");
            assert_eq!(err, vec![
                SchemaName::from("a"),
                SchemaName::from("b"),
                SchemaName::from("c")
            ]);
        }

        #[test]
        fn rejects_a_self_loop() {
            let mut raw = IndexMap::new();
            raw.insert(SchemaName::from("a"), schema(&["a"]));
            let (builder, _warnings) = SchemaGraphBuilder::new(
                &raw,
                SchemaNameRef::from(GLOBAL_SCHEMA_NAME),
            );

            let err = builder.build().expect_err("self-loop rejected");
            assert_eq!(err, vec![SchemaName::from("a")]);
        }
    }
}
