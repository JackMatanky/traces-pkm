//! Topological-sort builder that produces a [`SchemaGraph`].

use std::collections::VecDeque;

use indexmap::{IndexMap, IndexSet};

use super::{
    super::{RawSchema, SchemaName, SchemaNameRef, error::SchemaWarning},
    SchemaGraph,
    adjacency::{DenseIndex, SchemaAdjacency},
    cycle::CycleDetector,
};

/// Drives topological sort over a [`SchemaAdjacency`] to completion.
///
/// Build with [`new`](Self::new), then call [`build`](Self::build) once.
pub(crate) struct SchemaGraphBuilder<'a> {
    adjacency: SchemaAdjacency<'a>,
    /// Per-node count of unresolved parents.
    unresolved_parent_count: Vec<u32>,
    /// Nodes whose `unresolved_parent_count` reached zero.
    ready_queue: VecDeque<DenseIndex>,
    /// Schemas popped from `ready_queue`, in topological order.
    visited: IndexSet<SchemaNameRef<'a>>,
}

impl<'a> SchemaGraphBuilder<'a> {
    /// Builds the adjacency and seeds the ready queue.
    ///
    /// Returns the builder and any warnings from [`SchemaAdjacency::build`].
    ///
    /// # Arguments
    ///
    /// * `raw` - All raw Schemas, keyed by name.
    /// * `excluded` - The Global Schema name, excluded from the graph.
    pub(crate) fn new(
        raw: &'a IndexMap<SchemaName, RawSchema>,
        excluded: SchemaNameRef<'a>,
    ) -> (Self, Vec<SchemaWarning>) {
        let (adjacency, warnings) = SchemaAdjacency::build(raw, excluded);
        let node_count = adjacency.node_count();

        // Derive each node's in-degree from the CSR adjacency itself
        // (occurrences of a node across every parent's children list) rather
        // than re-scanning raw `extends` lists a second time.
        let mut unresolved_parent_count: Vec<u32> = vec![0; node_count];
        for child in adjacency.child_targets_iter() {
            if let Some(count) = unresolved_parent_count.get_mut(child.index())
            {
                *count = count.saturating_add(1);
            }
        }

        let ready_queue: VecDeque<DenseIndex> = (0
            ..DenseIndex::saturating_u32(node_count))
            .map(DenseIndex::from)
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

    /// Drains the ready queue, checks for cycles, and returns the completed
    /// [`SchemaGraph`].
    ///
    /// After topological sort completes, sorts every node's CSR children into
    /// topological-rank order (a precondition for [`SchemaGraph::hierarchy`]'s
    /// closure sweep).
    ///
    /// # Errors
    ///
    /// Returns every Schema participating in an `extends` cycle, in declaration
    /// order. A Schema that merely `extends` into a cycle without being part of
    /// one itself is excluded.
    pub(crate) fn build(mut self) -> Result<SchemaGraph<'a>, Vec<SchemaName>> {
        while let Some(name) = self.next_ready() {
            self.mark_resolved(name);
        }
        let cyclic = CycleDetector::new(&self.adjacency, &self.visited).find();
        if !cyclic.is_empty() {
            return Err(cyclic);
        }

        let topo_rank = self.adjacency.compute_topo_rank(&self.visited);
        let rank_of = |c: &DenseIndex| {
            topo_rank.get(c.index()).copied().unwrap_or(u32::MAX)
        };
        for i in 0..DenseIndex::saturating_u32(self.adjacency.node_count()) {
            self.adjacency
                .children_slice_mut(DenseIndex::from(i))
                .sort_by_key(rank_of);
        }

        Ok(SchemaGraph {
            adjacency: self.adjacency,
            topological_order: self.visited,
        })
    }

    /// Pops the next Schema whose `unresolved_parent_count` reached zero.
    /// Returns `None` once the queue drains.
    fn next_ready(&mut self) -> Option<SchemaNameRef<'a>> {
        let index = self.ready_queue.pop_front()?;
        let name = self.adjacency.name_of(index)?;
        self.visited.insert(name);
        Some(name)
    }

    /// Records `name` as resolved, releasing children whose
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

#[cfg(test)]
mod tests {
    use indexmap::IndexMap;

    use super::{
        super::super::{
            GLOBAL_SCHEMA_NAME, RawSchema, SchemaName, SchemaNameRef,
        },
        SchemaGraphBuilder,
    };

    fn schema(extends: &[&str]) -> RawSchema {
        RawSchema {
            extends: extends.iter().map(|&s| SchemaName::from(s)).collect(),
            ..RawSchema::default()
        }
    }

    #[test]
    fn does_not_warn_when_global_is_present_in_raw() {
        let mut raw = IndexMap::new();
        raw.insert(SchemaName::from(GLOBAL_SCHEMA_NAME), schema(&[]));
        raw.insert(SchemaName::from("book"), schema(&[GLOBAL_SCHEMA_NAME]));
        let (_builder, warnings) = SchemaGraphBuilder::new(
            &raw,
            SchemaNameRef::from(GLOBAL_SCHEMA_NAME),
        );
        assert!(warnings.is_empty());
    }

    #[test]
    fn build_returns_graph_in_topological_order() {
        let mut raw = IndexMap::new();
        raw.insert(SchemaName::from(GLOBAL_SCHEMA_NAME), schema(&[]));
        raw.insert(SchemaName::from("book"), schema(&[GLOBAL_SCHEMA_NAME]));
        let (builder, _warnings) = SchemaGraphBuilder::new(
            &raw,
            SchemaNameRef::from(GLOBAL_SCHEMA_NAME),
        );
        let graph = builder.build().expect("acyclic fixture resolves");
        let order: Vec<SchemaNameRef<'_>> = graph.topological_order().collect();
        assert!(order.contains(&SchemaNameRef::from("book")));
        assert!(!order.contains(&SchemaNameRef::from(GLOBAL_SCHEMA_NAME)));
    }
}
