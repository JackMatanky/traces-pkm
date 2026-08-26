//! Cycle detection over the unvisited subgraph after topological sort.

use indexmap::IndexSet;

use super::{
    super::{SchemaName, SchemaNameRef},
    adjacency::{DenseIndex, SchemaAdjacency},
};

/// Finds every Schema participating in an `extends` cycle.
///
/// Restricted to the subgraph the topological sort never resolved (a resolved
/// node is provably acyclic). Iterative strongly-connected-components search
/// (explicit work-stack, no recursion) over that unvisited subgraph, walking
/// `extends` backward (parent-direction, via [`SchemaAdjacency::parents_of`])
/// rather than the forward adjacency the rest of the graph uses. `O(V + E)`
/// over the unvisited subgraph.
pub(super) struct CycleDetector<'a, 'b> {
    adjacency: &'b SchemaAdjacency<'a>,
    /// Schemas Kahn's sort already resolved; only unvisited nodes are
    /// explored.
    visited: &'b IndexSet<SchemaNameRef<'a>>,
    search: CycleSearchState,
    cyclic: Vec<SchemaName>,
}

impl<'a> CycleDetector<'a, '_> {
    pub(super) fn new<'b>(
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

    /// Returns every unvisited Schema in a cycle, in declaration order.
    /// Empty if `visited` already covers every node.
    pub(super) fn find(mut self) -> Vec<SchemaName> {
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

    /// Whether `node` was resolved by Kahn's sort.
    fn is_kahn_visited(&self, node: DenseIndex) -> bool {
        self.adjacency
            .name_of(node)
            .is_some_and(|name| self.visited.contains(&name))
    }

    /// Explicit work-stack DFS rooted at `start`, extracting every nontrivial
    /// strongly-connected component (or self-loop) into `self.cyclic`.
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

    /// Scans `node`'s raw `extends` list from `from`, skipping unknown or
    /// Kahn-visited targets. Returns the first unvisited-and-known parent and
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

    /// Advances `node`'s frame to `resume_at`, then descends into `parent` if
    /// undiscovered, or folds `parent`'s discovery order into `node`'s
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

    /// Completes `node`: if it is a component root, pops and records its
    /// strongly-connected component into `self.cyclic` in insertion order,
    /// then propagates its lowest-reachable rank to its caller.
    fn finish_node(
        &mut self,
        work_stack: &mut Vec<(DenseIndex, usize)>,
        node: DenseIndex,
    ) {
        self.record_cyclic_component(node);
        work_stack.pop();
        let Some(&(parent, _)) = work_stack.last() else {
            return;
        };
        if let Some(node_lowest) = self.search.lowest_reachable_of(node) {
            self.search.lower_reachable(parent, node_lowest);
        }
    }

    /// If `node` is an SCC root, pops the component and extends `self.cyclic`
    /// with its members (in declaration order) when nontrivial or self-looping.
    fn record_cyclic_component(&mut self, node: DenseIndex) {
        let node_order = self.search.discovery_order_of(node);
        if node_order.is_none()
            || node_order != self.search.lowest_reachable_of(node)
        {
            return;
        }
        let mut component = self.search.pop_component(node);
        let is_cyclic = component.len() > 1 || self.has_self_extend(node);
        if !is_cyclic {
            return;
        }
        // Insertion order, not Tarjan pop order: matches the resolver's
        // error-reporting convention.
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

/// Per-node bookkeeping for [`CycleDetector`]'s SCC search.
///
/// Sized to the full unvisited-subgraph node count; entries for unreached nodes
/// stay at their initial value.
struct CycleSearchState {
    /// Discovery order, assigned once per node on first visit.
    discovery_order: Vec<Option<u32>>,
    /// Lowest discovery order reachable from each node via tree or back edges
    /// (Tarjan's "lowlink").
    lowest_reachable: Vec<u32>,
    /// Whether each node is currently on `component_stack`.
    on_stack: Vec<bool>,
    /// SCC-accumulation stack, in discovery order.
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

    /// Whether `node` has been assigned a discovery order.
    fn is_discovered(&self, node: DenseIndex) -> bool {
        self.discovery_order.get(node.index()).copied().flatten().is_some()
    }

    /// Returns `node`'s discovery order, or `None` if undiscovered.
    fn discovery_order_of(&self, node: DenseIndex) -> Option<u32> {
        self.discovery_order.get(node.index()).copied().flatten()
    }

    /// Returns `node`'s current lowest-reachable rank, or `None` if
    /// undiscovered.
    fn lowest_reachable_of(&self, node: DenseIndex) -> Option<u32> {
        self.lowest_reachable.get(node.index()).copied()
    }

    /// Whether `node` is on the SCC stack (not the traversal work-stack).
    fn is_on_stack(&self, node: DenseIndex) -> bool {
        self.on_stack.get(node.index()).copied().unwrap_or(false)
    }

    /// Assigns `node` its discovery order and pushes it onto `component_stack`.
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

    /// Folds `candidate` into `node`'s lowest-reachable rank, keeping the
    /// smaller value.
    fn lower_reachable(&mut self, node: DenseIndex, candidate: u32) {
        if let Some(slot) = self.lowest_reachable.get_mut(node.index()) {
            *slot = (*slot).min(candidate);
        }
    }

    /// Pops the SCC rooted at `root` off `component_stack` (most recently
    /// discovered first).
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

#[cfg(test)]
mod tests {
    use indexmap::IndexMap;

    use super::super::{
        super::{GLOBAL_SCHEMA_NAME, SchemaName, SchemaNameRef},
        builder::SchemaGraphBuilder,
    };

    fn schema(extends: &[&str]) -> super::super::super::RawSchema {
        super::super::super::RawSchema {
            extends: extends.iter().map(|&s| SchemaName::from(s)).collect(),
            ..super::super::super::RawSchema::default()
        }
    }

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

    #[test]
    fn returns_empty_when_no_cycle_exists() {
        let mut raw = IndexMap::new();
        raw.insert(SchemaName::from("root"), schema(&[]));
        raw.insert(SchemaName::from("child"), schema(&["root"]));
        let (builder, _warnings) = SchemaGraphBuilder::new(
            &raw,
            SchemaNameRef::from(GLOBAL_SCHEMA_NAME),
        );

        let graph = builder.build().expect("acyclic graph resolves");
        assert!(!graph.topological_order().next().is_none());
    }

    #[test]
    fn rejects_multiple_independent_cycles() {
        let mut raw = IndexMap::new();
        raw.insert(SchemaName::from("a"), schema(&["b"]));
        raw.insert(SchemaName::from("b"), schema(&["a"]));
        raw.insert(SchemaName::from("x"), schema(&["y"]));
        raw.insert(SchemaName::from("y"), schema(&["x"]));
        let (builder, _warnings) = SchemaGraphBuilder::new(
            &raw,
            SchemaNameRef::from(GLOBAL_SCHEMA_NAME),
        );

        let err = builder.build().expect_err("cycles rejected");
        assert_eq!(err.len(), 4, "both cycles must be reported");
        assert!(err.contains(&SchemaName::from("a")));
        assert!(err.contains(&SchemaName::from("b")));
        assert!(err.contains(&SchemaName::from("x")));
        assert!(err.contains(&SchemaName::from("y")));
    }
}
