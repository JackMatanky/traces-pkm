//! `extends` DAG linearization via Kahn's topological sort.
//!
//! [`SchemaGraph`] owns the DAG bookkeeping so [`super::resolver`] can drive
//! resolution order without tangling graph mechanics into field-merge logic.
//!
//! # Ordering
//!
//! A Schema is yielded after all its present `extends` parents. Schemas whose
//! in-degree reaches zero simultaneously yield in their raw-map insertion
//! order. The resolver strips the Global Schema from the graph, so it never
//! competes for queue position.
//!
//! # Driving resolution
//!
//! Build with [`SchemaGraph::new`], then loop
//! [`next_ready`]/[`parents_of`]/[`mark_resolved`] in [`Building`] state. Call
//! [`into_resolved`] to check for cycles and transition to [`Resolved`], where
//! [`children_by_name`]/[`descendants_by_name`] give the bulk hierarchy sets.
//!
//! [`SchemaGraph::new`]: SchemaGraph::new
//! [`next_ready`]: SchemaGraph::next_ready
//! [`parents_of`]: SchemaGraph::parents_of
//! [`mark_resolved`]: SchemaGraph::mark_resolved
//! [`into_resolved`]: SchemaGraph::into_resolved
//! [`children_by_name`]: SchemaGraph::children_by_name
//! [`descendants_by_name`]: SchemaGraph::descendants_by_name

use std::{cell::OnceCell, collections::VecDeque};

use bit_vec::BitVec;
use indexmap::{IndexMap, IndexSet};

use super::{RawSchema, SchemaName, SchemaNameRef, error::SchemaWarning};

/// Dense per-schema array index, assigned once at construction in raw-map
/// insertion order. Private to this module: it never appears in any
/// signature `super::resolver` can see. Array indexing replaces
/// string-hashed `HashMap`/`IndexMap` lookups on every graph operation.
///
/// Caps a schema set at `u32::MAX` (roughly 4 billion) entries, which is not
/// a real constraint for a filesystem-enumerated `.traces/schemas/*.toml`
/// registry.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
struct DenseIndex(u32);

/// Narrows `n` to `u32`, saturating to `u32::MAX` on overflow. Values this
/// module narrows (schema counts, dense indices, topological ranks) stay far
/// below `u32::MAX` for any real schema registry; see [`DenseIndex`]'s own
/// doc for the accepted cap.
fn saturating_u32(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

/// Widens `n` to `usize`. `usize` is at least as wide as `u32` on every
/// platform this crate targets, so `try_from` failing here is unreachable in
/// practice; it saturates to `usize::MAX` rather than panicking on the off
/// chance it ever isn't.
fn widen_usize(n: u32) -> usize {
    usize::try_from(n).unwrap_or(usize::MAX)
}

impl DenseIndex {
    /// Builds a dense index from an array position, saturating per
    /// [`saturating_u32`].
    fn from_usize(n: usize) -> Self {
        Self(saturating_u32(n))
    }

    /// Widens back to `usize` for indexing. See [`widen_usize`].
    fn index(self) -> usize {
        widen_usize(self.0)
    }
}

/// Building state: Kahn's-sort scratch (in-degree counters and the ready
/// queue). Dropped on transition to [`Resolved`].
#[derive(Debug)]
pub(super) struct Building {
    /// index -> remaining unresolved-parent count.
    in_degree: Vec<u32>,
    /// Ready queue, dense-indexed.
    queue: VecDeque<DenseIndex>,
}

/// Resolved state: the DAG is confirmed acyclic. Retains the topological order
/// [`descendants_by_name`](SchemaGraph::descendants_by_name)'s reverse-order
/// pass needs; drops `in_degree`/`queue`.
#[derive(Debug)]
pub(super) struct Resolved<'a> {
    /// Topological order, parents before children.
    order: Vec<DenseIndex>,
    /// Lazily built, cached on first [`children_by_name`] call.
    ///
    /// [`children_by_name`]: SchemaGraph::children_by_name
    children_by_name:
        OnceCell<IndexMap<SchemaNameRef<'a>, Vec<SchemaNameRef<'a>>>>,
}

/// Kahn's-algorithm state for linearizing the `extends` DAG.
///
/// `State` enforces valid transitions at compile time:
/// - [`Building`] for the resolution loop
/// - [`Resolved`] for hierarchy queries after cycle-check.
#[derive(Debug)]
pub(super) struct SchemaGraph<'a, State> {
    /// Borrowed raw schemas: the source of truth for `extends` parents.
    raw: &'a IndexMap<SchemaName, RawSchema>,
    /// Dense index -> name, in raw-map insertion order.
    names: Vec<SchemaNameRef<'a>>,
    /// Name -> dense index, built once at construction.
    index_of: IndexMap<SchemaNameRef<'a>, DenseIndex>,
    /// CSR adjacency (parent → children): node `i`'s children occupy
    /// `child_targets[child_offsets[i]..child_offsets[i + 1]]`.
    child_offsets: Vec<u32>,
    child_targets: Vec<DenseIndex>,
    /// Schemas already popped by [`next_ready`](Self::next_ready).
    visited: IndexSet<SchemaNameRef<'a>>,
    state: State,
}

impl<State> SchemaGraph<'_, State> {
    /// `v`'s CSR children slice (its direct `extends` reverse-adjacency), or an
    /// empty slice if `v` is out of range (never happens for a `DenseIndex`
    /// this module produced, but avoids raw indexing).
    fn children_slice(&self, v: DenseIndex) -> &[DenseIndex] {
        let idx = v.index();
        let bounds = self
            .child_offsets
            .get(idx)
            .zip(self.child_offsets.get(idx.saturating_add(1)));
        let Some((&start, &end)) = bounds else {
            return &[];
        };
        self.child_targets
            .get(widen_usize(start)..widen_usize(end))
            .unwrap_or(&[])
    }

    /// Mutable counterpart of [`children_slice`](Self::children_slice), used
    /// only to sort a node's children in place.
    fn children_slice_mut(&mut self, v: DenseIndex) -> &mut [DenseIndex] {
        let idx = v.index();
        let start = self.child_offsets.get(idx).copied();
        let end = self.child_offsets.get(idx.saturating_add(1)).copied();
        match (start, end) {
            (Some(s), Some(e)) => self
                .child_targets
                .get_mut(widen_usize(s)..widen_usize(e))
                .unwrap_or(&mut []),
            _ => &mut [],
        }
    }
}

impl<'a> SchemaGraph<'a, Building> {
    /// Build the `extends` adjacency (as CSR) and seed the ready queue.
    ///
    /// Scans each schema's `extends` targets, emitting warnings for targets
    /// absent from `raw_schemas` and for repeated targets.
    ///
    /// # Warnings
    ///
    /// - [`MissingExtendsTarget`] if an `extends` target has no corresponding
    ///   Schema file
    /// - [`DuplicateExtendsTarget`] if the same `extends` target appears more
    ///   than once
    ///
    /// [`MissingExtendsTarget`]: SchemaWarning::MissingExtendsTarget
    /// [`DuplicateExtendsTarget`]: SchemaWarning::DuplicateExtendsTarget
    pub(super) fn new(
        raw_schemas: &'a IndexMap<SchemaName, RawSchema>,
    ) -> (Self, Vec<SchemaWarning>) {
        let mut warnings = Vec::new();
        let count = raw_schemas.len();

        // Assign dense indices in raw-map insertion order.
        let names: Vec<SchemaNameRef<'a>> =
            raw_schemas.keys().map(SchemaName::as_ref).collect();
        let index_of: IndexMap<SchemaNameRef<'a>, DenseIndex> = names
            .iter()
            .enumerate()
            .map(|(i, &name)| (name, DenseIndex::from_usize(i)))
            .collect();

        // Validate + dedup each schema's `extends` targets, accumulating
        // in-degree (parent count) and out-degree (child count) per node, and
        // the validated edge list the CSR fill pass needs.
        let mut in_degree: Vec<u32> = vec![0; count];
        let mut out_degree: Vec<u32> = vec![0; count];
        let mut valid_edges: Vec<(DenseIndex, DenseIndex)> = Vec::new();
        for (i, (name, raw)) in raw_schemas.iter().enumerate() {
            let mut seen_targets = IndexSet::new();
            for target in &raw.extends {
                let Some(&target_index) = index_of.get(target.as_str()) else {
                    warnings.push(SchemaWarning::MissingExtendsTarget {
                        schema: name.clone(),
                        target: target.clone(),
                    });
                    continue;
                };
                if !seen_targets.insert(target.as_str()) {
                    warnings.push(SchemaWarning::DuplicateExtendsTarget {
                        schema: name.clone(),
                        target: target.clone(),
                    });
                    continue;
                }
                if let Some(degree) = in_degree.get_mut(i) {
                    *degree = degree.saturating_add(1);
                }
                if let Some(degree) = out_degree.get_mut(target_index.index()) {
                    *degree = degree.saturating_add(1);
                }
                valid_edges.push((DenseIndex::from_usize(i), target_index));
            }
        }

        // Prefix-sum out-degrees into CSR offsets.
        let mut child_offsets: Vec<u32> =
            Vec::with_capacity(count.saturating_add(1));
        let mut running: u32 = 0;
        child_offsets.push(running);
        for &out in &out_degree {
            running = running.saturating_add(out);
            child_offsets.push(running);
        }
        let edge_count = widen_usize(running);

        // Fill CSR targets via a scratch write cursor per node.
        let mut cursor: Vec<u32> =
            child_offsets.iter().take(count).copied().collect();
        let mut child_targets: Vec<DenseIndex> =
            vec![DenseIndex(0); edge_count];
        for (schema_index, target_index) in valid_edges {
            let Some(slot) = cursor.get_mut(target_index.index()) else {
                continue;
            };
            if let Some(target_slot) = child_targets.get_mut(widen_usize(*slot))
            {
                *target_slot = schema_index;
            }
            *slot = slot.saturating_add(1);
        }

        let queue: VecDeque<DenseIndex> = (0..saturating_u32(count))
            .map(DenseIndex)
            .filter(|idx| in_degree.get(idx.index()).copied() == Some(0))
            .collect();

        (
            Self {
                raw: raw_schemas,
                names,
                index_of,
                child_offsets,
                child_targets,
                visited: IndexSet::new(),
                state: Building {
                    in_degree,
                    queue,
                },
            },
            warnings,
        )
    }

    /// Pop the next Schema whose in-degree reached zero, marking it visited.
    ///
    /// Returns `None` once the ready queue drains.
    pub(super) fn next_ready(&mut self) -> Option<SchemaNameRef<'a>> {
        let index = self.state.queue.pop_front()?;
        let name = *self.names.get(index.index())?;
        self.visited.insert(name);
        Some(name)
    }

    /// Borrow `name`'s raw `extends` parent list.
    ///
    /// Returns an empty slice if `name` is not a known Schema.
    pub(super) fn parents_of(&self, name: SchemaNameRef<'_>) -> &[SchemaName] {
        self.raw.get(name.as_str()).map_or(&[], |s| s.extends.as_slice())
    }

    /// Record `name` as resolved, releasing children whose in-degree hit zero
    /// into the ready queue.
    pub(super) fn mark_resolved(&mut self, name: SchemaNameRef<'_>) {
        let Some(&index) = self.index_of.get(name.as_str()) else {
            return;
        };
        let idx = index.index();
        // Reads `child_offsets`/`child_targets` directly (not through
        // `children_slice`, which borrows all of `self`) so the borrow checker
        // can see this as disjoint from the `self.state` mutation below,
        // avoiding a per-call `Vec` allocation.
        let bounds = self
            .child_offsets
            .get(idx)
            .zip(self.child_offsets.get(idx.saturating_add(1)));
        let Some((&start, &end)) = bounds else {
            return;
        };
        let Some(children) =
            self.child_targets.get(widen_usize(start)..widen_usize(end))
        else {
            return;
        };
        for &child in children {
            let Some(degree) = self.state.in_degree.get_mut(child.index())
            else {
                continue;
            };
            *degree = degree.saturating_sub(1);
            if *degree == 0 {
                self.state.queue.push_back(child);
            }
        }
    }

    /// Whether `v` was already resolved by Kahn's sort (provably acyclic).
    fn is_kahn_visited(&self, v: DenseIndex) -> bool {
        self.names
            .get(v.index())
            .is_some_and(|name| self.visited.contains(name.as_str()))
    }

    /// Scans `v`'s raw `extends` list starting at raw position `from`, skipping
    /// unknown or Kahn-visited targets, and returns the first
    /// unvisited-and-known target found alongside the raw position to resume
    /// scanning from on the next call. `None` once the list is exhausted.
    ///
    /// Each raw `extends` entry is examined at most once across the whole life
    /// of one [`run_tarjan_from`](Self::run_tarjan_from) call, because `from`
    /// always advances monotonically between calls for the same `v`: the
    /// traversal never rescans a prefix, giving `O(V + E)` total work instead
    /// of the `O(degree²)` a full re-filter on every step would pay.
    fn next_unvisited_parent(
        &self,
        v: DenseIndex,
        from: usize,
    ) -> Option<(DenseIndex, usize)> {
        let &name = self.names.get(v.index())?;
        let parents = self.parents_of(name);
        let mut pos = from;
        while let Some(parent) = parents.get(pos) {
            pos = pos.saturating_add(1);
            if self.visited.contains(parent.as_str()) {
                continue;
            }
            if let Some(&target) = self.index_of.get(parent.as_str()) {
                return Some((target, pos));
            }
        }
        None
    }

    /// Whether `v`'s raw `extends` list contains a valid (known, unvisited)
    /// reference back to itself.
    fn has_self_extend(&self, v: DenseIndex) -> bool {
        let Some(&name) = self.names.get(v.index()) else {
            return false;
        };
        self.parents_of(name).iter().any(|parent| {
            !self.visited.contains(parent.as_str())
                && self.index_of.get(parent.as_str()) == Some(&v)
        })
    }

    /// Return every unvisited Schema that participates in a cycle: every member
    /// of a nontrivial strongly-connected component in the unvisited subgraph,
    /// plus any node with a direct self-`extends`.
    ///
    /// Excludes Schemas that never reached in-degree zero only because they
    /// `extends` into a cycle without being cyclic themselves. For example,
    /// given `c extends a` where `a` cycles with `b`, `c` is excluded and
    /// `a`/`b` are not, a direct consequence of mutual reachability (the SCC
    /// definition), not special-cased logic.
    ///
    /// Returns an empty `Vec` if every Schema was visited (no cycle).
    ///
    /// Iterative Tarjan's SCC (explicit work-stack simulating the call stack,
    /// no recursion), so it cannot stack-overflow regardless of `extends`-chain
    /// depth. Scoped to the unvisited node subset only: visited nodes are
    /// provably acyclic (Kahn's convergence to in-degree zero proves it), an
    /// early-exit optimization over a full-graph scan. `O(V + E)` over the
    /// unvisited subgraph.
    fn cyclic_schemas(&self) -> Vec<SchemaName> {
        if self.visited.len() == self.raw.len() {
            return Vec::new();
        }
        let count = self.names.len();
        let mut tarjan = TarjanState::new(count);
        let mut cyclic: Vec<SchemaName> = Vec::new();

        for start in 0..saturating_u32(count) {
            let start = DenseIndex(start);
            if self.is_kahn_visited(start) || tarjan.is_discovered(start) {
                continue;
            }
            self.run_tarjan_from(start, &mut tarjan, &mut cyclic);
        }
        cyclic
    }

    /// Explicit work-stack DFS (no recursion) rooted at `start`, extracting
    /// every nontrivial SCC (or self-loop) it discovers into `cyclic`.
    ///
    /// Each stack frame's `usize` is a raw `extends`-list scan position, not a
    /// filtered child index: see [`next_unvisited_parent`].
    ///
    /// [`next_unvisited_parent`]: Self::next_unvisited_parent
    fn run_tarjan_from(
        &self,
        start: DenseIndex,
        tarjan: &mut TarjanState,
        cyclic: &mut Vec<SchemaName>,
    ) {
        let mut work_stack: Vec<(DenseIndex, usize)> = vec![(start, 0)];
        while let Some(&(v, scan_pos)) = work_stack.last() {
            if scan_pos == 0 {
                tarjan.discover(v);
            }
            match self.next_unvisited_parent(v, scan_pos) {
                Some((w, next_pos)) => {
                    Self::advance_child(
                        &mut work_stack,
                        tarjan,
                        v,
                        w,
                        next_pos,
                    );
                }
                None => self.finish_node(&mut work_stack, tarjan, v, cyclic),
            }
        }
    }

    /// Advance `v`'s frame to scan position `next_pos`, then descend into `w`
    /// if undiscovered, otherwise fold `w`'s index into `v`'s lowlink if `w` is
    /// a live back edge (still on the Tarjan stack).
    fn advance_child(
        work_stack: &mut Vec<(DenseIndex, usize)>,
        tarjan: &mut TarjanState,
        v: DenseIndex,
        w: DenseIndex,
        next_pos: usize,
    ) {
        if let Some(frame) = work_stack.last_mut() {
            frame.1 = next_pos;
        }
        if !tarjan.is_discovered(w) {
            work_stack.push((w, 0));
            return;
        }
        if !tarjan.is_on_stack(w) {
            return;
        }
        if let Some(w_index) = tarjan.index_of(w) {
            tarjan.merge_lowlink(v, w_index);
        }
    }

    /// `v` is fully explored: if it is an SCC root, pop and record its
    /// component (if nontrivial), then pop `v`'s own frame and propagate its
    /// lowlink to its caller.
    fn finish_node(
        &self,
        work_stack: &mut Vec<(DenseIndex, usize)>,
        tarjan: &mut TarjanState,
        v: DenseIndex,
        cyclic: &mut Vec<SchemaName>,
    ) {
        let v_index = tarjan.index_of(v);
        if v_index.is_some() && v_index == tarjan.lowlink_of(v) {
            let mut scc = tarjan.pop_scc(v);
            let is_cyclic = scc.len() > 1 || self.has_self_extend(v);
            if is_cyclic {
                // Insertion order, not Tarjan pop order: matches the resolver's
                // error-reporting convention and this module's own cycle tests.
                scc.sort_by_key(|idx| idx.0);
                cyclic.extend(scc.iter().filter_map(|&idx| {
                    self.names
                        .get(idx.index())
                        .map(|&name| SchemaName::from(name))
                }));
            }
        }
        work_stack.pop();
        let Some(&(parent, _)) = work_stack.last() else {
            return;
        };
        if let Some(v_lowlink) = tarjan.lowlink_of(v) {
            tarjan.merge_lowlink(parent, v_lowlink);
        }
    }

    /// Consume the building graph, returning a resolved graph if the DAG is
    /// acyclic, or the cyclic Schemas if a cycle exists.
    ///
    /// Drives any remaining [`next_ready`] / [`mark_resolved`] steps before
    /// checking for cycles, so callers get correct results even if the loop was
    /// not fully exhausted.
    ///
    /// # Errors
    ///
    /// Returns `Err(Vec<SchemaName>)` listing only the Schemas that participate
    /// in a cycle. A Schema that merely `extends` into a cycle without being
    /// part of one itself is excluded.
    ///
    /// [`next_ready`]: SchemaGraph::next_ready
    /// [`mark_resolved`]: SchemaGraph::mark_resolved
    pub(super) fn into_resolved(
        mut self,
    ) -> Result<SchemaGraph<'a, Resolved<'a>>, Vec<SchemaName>> {
        while let Some(parent) = self.next_ready() {
            self.mark_resolved(parent);
        }
        let cyclic = self.cyclic_schemas();
        if !cyclic.is_empty() {
            return Err(cyclic);
        }

        // Kahn's sort fully drained (no cycle): `self.visited` holds every
        // schema in topological order (parents before children). Rank each
        // node, then sort every CSR children slice into rank order (the
        // precondition `descendants_by_name`'s closure algorithm requires)
        // before dropping `in_degree`/`queue` by not carrying them forward.
        let order: Vec<DenseIndex> = self
            .visited
            .iter()
            .filter_map(|&name| self.index_of.get(name.as_str()).copied())
            .collect();
        let mut topo_rank: Vec<u32> = vec![0; self.names.len()];
        for (rank, &idx) in order.iter().enumerate() {
            if let Some(slot) = topo_rank.get_mut(idx.index()) {
                *slot = saturating_u32(rank);
            }
        }
        let rank_of = |c: &DenseIndex| {
            topo_rank.get(c.index()).copied().unwrap_or(u32::MAX)
        };
        for i in 0..saturating_u32(self.names.len()) {
            self.children_slice_mut(DenseIndex(i)).sort_by_key(rank_of);
        }

        Ok(SchemaGraph {
            raw: self.raw,
            names: self.names,
            index_of: self.index_of,
            child_offsets: self.child_offsets,
            child_targets: self.child_targets,
            visited: self.visited,
            state: Resolved {
                order,
                children_by_name: OnceCell::new(),
            },
        })
    }
}

/// Iterative Tarjan's-SCC bookkeeping over the unvisited (Kahn-unresolved)
/// dense-index subgraph. Arrays sized to the full node count; entries for nodes
/// this traversal never reaches stay at their initial value.
struct TarjanState {
    /// Discovery order, assigned once per node on first visit.
    index: Vec<Option<u32>>,
    /// Lowest discovery index reachable from each node.
    lowlink: Vec<u32>,
    /// Whether each node is currently on [`stack`](Self::stack).
    on_stack: Vec<bool>,
    /// SCC-accumulation stack, in discovery order.
    stack: Vec<DenseIndex>,
    /// Next discovery index to assign.
    counter: u32,
}

impl TarjanState {
    fn new(count: usize) -> Self {
        Self {
            index: vec![None; count],
            lowlink: vec![0; count],
            on_stack: vec![false; count],
            stack: Vec::new(),
            counter: 0,
        }
    }

    fn is_discovered(&self, v: DenseIndex) -> bool {
        self.index.get(v.index()).copied().flatten().is_some()
    }

    fn index_of(&self, v: DenseIndex) -> Option<u32> {
        self.index.get(v.index()).copied().flatten()
    }

    fn lowlink_of(&self, v: DenseIndex) -> Option<u32> {
        self.lowlink.get(v.index()).copied()
    }

    fn is_on_stack(&self, v: DenseIndex) -> bool {
        self.on_stack.get(v.index()).copied().unwrap_or(false)
    }

    /// Assign `v` its Tarjan discovery index/lowlink and push it onto the
    /// Tarjan stack.
    fn discover(&mut self, v: DenseIndex) {
        let rank = self.counter;
        self.counter = self.counter.saturating_add(1);
        if let Some(slot) = self.index.get_mut(v.index()) {
            *slot = Some(rank);
        }
        if let Some(slot) = self.lowlink.get_mut(v.index()) {
            *slot = rank;
        }
        self.stack.push(v);
        if let Some(slot) = self.on_stack.get_mut(v.index()) {
            *slot = true;
        }
    }

    fn merge_lowlink(&mut self, v: DenseIndex, candidate: u32) {
        if let Some(slot) = self.lowlink.get_mut(v.index()) {
            *slot = (*slot).min(candidate);
        }
    }

    /// Pops the strongly-connected component rooted at `root` off the Tarjan
    /// stack (LIFO order: most-recently-discovered first).
    fn pop_scc(&mut self, root: DenseIndex) -> Vec<DenseIndex> {
        let mut scc = Vec::new();
        while let Some(popped) = self.stack.pop() {
            if let Some(slot) = self.on_stack.get_mut(popped.index()) {
                *slot = false;
            }
            let is_root = popped == root;
            scc.push(popped);
            if is_root {
                break;
            }
        }
        scc
    }
}

impl<'a> SchemaGraph<'a, Resolved<'a>> {
    /// Return every Schema's direct `extends` children, keyed by parent name.
    ///
    /// Only Schemas that have at least one child appear in the returned map.
    /// Schemas without children are omitted (not mapped to empty vectors).
    ///
    /// Built lazily from the CSR adjacency on first call and cached for the
    /// lifetime of this resolved graph. [`super::resolver`]'s hierarchy-set
    /// computation is this method's only caller, once per `resolve()` call.
    #[must_use]
    pub(super) fn children_by_name(
        &self,
    ) -> &IndexMap<SchemaNameRef<'a>, Vec<SchemaNameRef<'a>>> {
        self.state.children_by_name.get_or_init(|| {
            let mut map: IndexMap<SchemaNameRef<'a>, Vec<SchemaNameRef<'a>>> =
                IndexMap::new();
            for (i, &name) in self.names.iter().enumerate() {
                let slice = self.children_slice(DenseIndex::from_usize(i));
                if slice.is_empty() {
                    continue;
                }
                let children: Vec<SchemaNameRef<'a>> = slice
                    .iter()
                    .filter_map(|&child| self.names.get(child.index()).copied())
                    .collect();
                map.insert(name, children);
            }
            map
        })
    }

    /// Return every Schema's transitive `extends` descendants, keyed by
    /// ancestor name.
    ///
    /// Output-sensitive transitive closure (Habib–Morvan–Rampon, adapted from
    /// `petgraph 0.8.3`'s `algo::tred::dag_transitive_reduction_closure`,
    /// closure half only; this module has no use for the reduction half):
    /// `O(V + E + Σ|closure(x)|)`, proportional to the closure's actual size,
    /// rather than the `O(V²/w)` a bitset DP pays unconditionally regardless
    /// of how sparse the real closure is. Requires children iterated in
    /// topological-rank order, which
    /// [`into_resolved`](SchemaGraph::into_resolved) already sorted
    /// `child_targets` into.
    #[must_use]
    pub(super) fn descendants_by_name(
        &self,
    ) -> IndexMap<SchemaName, IndexSet<SchemaName>> {
        let count = self.names.len();
        let mut topo_rank: Vec<u32> = vec![0; count];
        for (rank, &idx) in self.state.order.iter().enumerate() {
            if let Some(slot) = topo_rank.get_mut(idx.index()) {
                *slot = saturating_u32(rank);
            }
        }

        // Scratch, reused across the whole outer loop: `mark[y]` is only ever
        // `true` transiently while `closure[i]` (the current node) is being
        // built, then cleared, bounded by `|closure[i]|` rather than a
        // full-array clear per node.
        let mut mark = BitVec::from_elem(count, false);
        let mut closure: Vec<Vec<DenseIndex>> = vec![Vec::new(); count];

        // Leaves first, roots last: each child's closure is already
        // finalized (later topological position) before its parent runs.
        for &i in self.state.order.iter().rev() {
            self.accumulate_descendants(i, &mut mark, &mut closure);
            // Required, not optional: without this, closure[i]'s order is
            // child-processing-interleaved, not globally topological, and
            // downstream callers observe this order directly (it becomes
            // `Schema::descendants()`'s `IndexSet` iteration order).
            if let Some(node_closure) = closure.get_mut(i.index()) {
                node_closure.sort_by_key(|d| {
                    topo_rank.get(d.index()).copied().unwrap_or(u32::MAX)
                });
            }
        }

        let mut result: IndexMap<SchemaName, IndexSet<SchemaName>> =
            IndexMap::new();
        for (i, &name) in self.names.iter().enumerate() {
            let Some(node_closure) = closure.get(i) else {
                continue;
            };
            if node_closure.is_empty() {
                continue;
            }
            let descendants: IndexSet<SchemaName> = node_closure
                .iter()
                .filter_map(|&d| {
                    self.names.get(d.index()).map(|&n| SchemaName::from(n))
                })
                .collect();
            result.insert(SchemaName::from(name), descendants);
        }
        result
    }

    /// Accumulate node `i`'s direct-plus-inherited-from-children descendants
    /// into `closure[i]`. `mark` is scratch, reused across the whole outer
    /// loop and cleared back to all-`false` before returning.
    fn accumulate_descendants(
        &self,
        i: DenseIndex,
        mark: &mut BitVec,
        closure: &mut [Vec<DenseIndex>],
    ) {
        for &x in self.children_slice(i) {
            Self::merge_child_closure(i, x, mark, closure);
        }
        let Some(node_closure) = closure.get(i.index()) else {
            return;
        };
        for &y in node_closure {
            mark.set(y.index(), false);
        }
    }

    /// Fold child `x`'s already-finalized closure into `closure[i]`,
    /// deduplicating via `mark`.
    fn merge_child_closure(
        i: DenseIndex,
        x: DenseIndex,
        mark: &mut BitVec,
        closure: &mut [Vec<DenseIndex>],
    ) {
        if mark.get(x.index()) == Some(true) {
            return;
        }
        mark.set(x.index(), true);
        push_descendant(closure, i, x);
        let child_descendants: Vec<DenseIndex> =
            closure.get(x.index()).cloned().unwrap_or_default();
        for y in child_descendants {
            if mark.get(y.index()) == Some(true) {
                continue;
            }
            mark.set(y.index(), true);
            push_descendant(closure, i, y);
        }
    }
}

/// Append `value` to `closure[i]`, a no-op if `i` is out of range.
fn push_descendant(
    closure: &mut [Vec<DenseIndex>],
    i: DenseIndex,
    value: DenseIndex,
) {
    if let Some(node_closure) = closure.get_mut(i.index()) {
        node_closure.push(value);
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
            let (_graph, warnings) = SchemaGraph::new(&raw);

            assert_eq!(warnings, vec![SchemaWarning::DuplicateExtendsTarget {
                schema: SchemaName::from("child"),
                target: SchemaName::from("book"),
            }]);
        }

        #[test]
        fn returns_missing_extends_target_warning() {
            let mut raw = IndexMap::new();
            raw.insert(SchemaName::from("child"), schema(&["nonexistent"]));
            let (_graph, warnings) = SchemaGraph::new(&raw);

            assert_eq!(warnings, vec![SchemaWarning::MissingExtendsTarget {
                schema: SchemaName::from("child"),
                target: SchemaName::from("nonexistent"),
            }]);
        }

        #[test]
        fn returns_no_warnings_for_empty_input() {
            let raw = IndexMap::new();
            let (_graph, warnings) = SchemaGraph::new(&raw);

            assert!(warnings.is_empty());
        }

        #[test]
        fn does_not_warn_when_global_is_present_in_raw() {
            let mut raw = IndexMap::new();
            raw.insert(SchemaName::from(GLOBAL_SCHEMA_NAME), schema(&[]));
            raw.insert(SchemaName::from("book"), schema(&[GLOBAL_SCHEMA_NAME]));
            let (_graph, warnings) = SchemaGraph::new(&raw);

            assert!(warnings.is_empty());
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
            let (graph, _warnings) = SchemaGraph::new(&raw);

            assert_eq!(graph.parents_of(SchemaNameRef::from("book")), &[
                SchemaName::from(GLOBAL_SCHEMA_NAME)
            ]);
        }

        #[test]
        fn returns_raw_extends_including_duplicates() {
            let mut raw = IndexMap::new();
            raw.insert(SchemaName::from("book"), schema(&[]));
            raw.insert(SchemaName::from("child"), schema(&["book", "book"]));
            let (graph, _warnings) = SchemaGraph::new(&raw);

            assert_eq!(graph.parents_of(SchemaNameRef::from("child")), &[
                SchemaName::from("book"),
                SchemaName::from("book"),
            ]);
        }

        #[test]
        fn returns_empty_slice_for_unknown_schema() {
            let mut raw = IndexMap::new();
            raw.insert(SchemaName::from("book"), schema(&[]));
            let (graph, _warnings) = SchemaGraph::new(&raw);

            assert_eq!(graph.parents_of(SchemaNameRef::from("missing")), &[]);
        }
    }

    mod state {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn returns_roots_in_declaration_order() {
            let mut raw = IndexMap::new();
            raw.insert(SchemaName::from("author"), schema(&[]));
            raw.insert(SchemaName::from("book"), schema(&["author"]));
            let (mut graph, _warnings) = SchemaGraph::new(&raw);

            assert_eq!(graph.next_ready(), Some(SchemaNameRef::from("author")));
            graph.mark_resolved(SchemaNameRef::from("author"));
            assert_eq!(graph.next_ready(), Some(SchemaNameRef::from("book")));
            assert_eq!(graph.next_ready(), None);
        }

        #[test]
        fn releases_multiple_simultaneous_roots_in_raw_map_insertion_order() {
            let mut raw = IndexMap::new();
            raw.insert(SchemaName::from("zebra"), schema(&[]));
            raw.insert(SchemaName::from("apple"), schema(&[]));
            raw.insert(SchemaName::from("mango"), schema(&[]));
            let (mut graph, _warnings) = SchemaGraph::new(&raw);

            assert_eq!(graph.next_ready(), Some(SchemaNameRef::from("zebra")));
            assert_eq!(graph.next_ready(), Some(SchemaNameRef::from("apple")));
            assert_eq!(graph.next_ready(), Some(SchemaNameRef::from("mango")));
            assert_eq!(graph.next_ready(), None);
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
            let graph = SchemaGraph::new(&raw).0.into_resolved().unwrap();
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
            let graph = SchemaGraph::new(&raw).0.into_resolved().unwrap();
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
            let graph = SchemaGraph::new(&raw).0.into_resolved().unwrap();
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
            let graph = SchemaGraph::new(&raw).0.into_resolved().unwrap();
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
            let graph = SchemaGraph::new(&raw).0.into_resolved().unwrap();
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
            let graph = SchemaGraph::new(&raw).0.into_resolved().unwrap();
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
            let graph = SchemaGraph::new(&raw).0.into_resolved().unwrap();
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
            let graph = SchemaGraph::new(&raw).0.into_resolved().unwrap();
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
            let (graph, _warnings) = SchemaGraph::new(&raw);

            assert_eq!(graph.index_of.get("alpha").map(|i| i.0), Some(0));
            assert_eq!(graph.index_of.get("book").map(|i| i.0), Some(1));
            assert_eq!(graph.index_of.get("sci_fi").map(|i| i.0), Some(2));
            assert_eq!(graph.index_of.get("missing"), None);
        }

        #[test]
        fn returns_the_name_at_the_given_index() {
            let mut raw = IndexMap::new();
            raw.insert(SchemaName::from("alpha"), schema(&[]));
            raw.insert(SchemaName::from("book"), schema(&[]));
            let (graph, _warnings) = SchemaGraph::new(&raw);

            assert_eq!(
                graph.names.first(),
                Some(&SchemaNameRef::from("alpha"))
            );
            assert_eq!(graph.names.get(1), Some(&SchemaNameRef::from("book")));
            assert_eq!(graph.names.get(2), None);
        }

        #[test]
        fn index_count_matches_the_number_of_names() {
            let mut raw = IndexMap::new();
            raw.insert(SchemaName::from("a"), schema(&[]));
            raw.insert(SchemaName::from("b"), schema(&[]));
            raw.insert(SchemaName::from("c"), schema(&[]));
            let (graph, _warnings) = SchemaGraph::new(&raw);

            assert_eq!(graph.names.len(), 3);
        }
    }

    mod cycles {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn into_resolved_rejects_a_direct_two_node_cycle() {
            let mut raw = IndexMap::new();
            raw.insert(SchemaName::from("a"), schema(&["b"]));
            raw.insert(SchemaName::from("b"), schema(&["a"]));
            let (graph, _warnings) = SchemaGraph::new(&raw);

            let err = graph.into_resolved().expect_err("cycle rejected");
            assert_eq!(err, vec![SchemaName::from("a"), SchemaName::from("b")]);
        }

        #[test]
        fn into_resolved_excludes_a_schema_that_only_extends_into_the_cycle() {
            let mut raw = IndexMap::new();
            raw.insert(SchemaName::from("a"), schema(&["b"]));
            raw.insert(SchemaName::from("b"), schema(&["a"]));
            raw.insert(SchemaName::from("c"), schema(&["a"]));
            let (graph, _warnings) = SchemaGraph::new(&raw);

            let err = graph.into_resolved().expect_err("cycle rejected");
            assert_eq!(err, vec![SchemaName::from("a"), SchemaName::from("b")]);
        }

        #[test]
        fn into_resolved_rejects_a_three_node_cycle_in_declaration_order() {
            let mut raw = IndexMap::new();
            raw.insert(SchemaName::from("a"), schema(&["b"]));
            raw.insert(SchemaName::from("b"), schema(&["c"]));
            raw.insert(SchemaName::from("c"), schema(&["a"]));
            let (graph, _warnings) = SchemaGraph::new(&raw);

            let err = graph.into_resolved().expect_err("cycle rejected");
            assert_eq!(err, vec![
                SchemaName::from("a"),
                SchemaName::from("b"),
                SchemaName::from("c")
            ]);
        }

        #[test]
        fn into_resolved_rejects_a_self_loop() {
            let mut raw = IndexMap::new();
            raw.insert(SchemaName::from("a"), schema(&["a"]));
            let (graph, _warnings) = SchemaGraph::new(&raw);

            let err = graph.into_resolved().expect_err("self-loop rejected");
            assert_eq!(err, vec![SchemaName::from("a")]);
        }
    }
}
