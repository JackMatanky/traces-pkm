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
//!    checks for a cycle, entirely internally. There is no stepwise driving
//!    API. Returns a [`SchemaGraph`] on success, or the list of cyclic Schemas
//!    on failure.
//! 3. Query the resulting [`SchemaGraph`]: [`topological_order`],
//!    [`parents_of`], [`children_by_name`], [`descendants_by_name`].
//!
//! [`topological_order`]: SchemaGraph::topological_order
//! [`parents_of`]: SchemaGraph::parents_of
//! [`children_by_name`]: SchemaGraph::children_by_name
//! [`descendants_by_name`]: SchemaGraph::descendants_by_name

mod adjacency;
mod builder;
mod cycle;

use adjacency::{DenseIndex, SchemaAdjacency};
pub(super) use builder::SchemaGraphBuilder;
use indexmap::{IndexMap, IndexSet};

use super::{SchemaName, SchemaNameRef};

/// A validated, acyclic `extends` DAG in topological order.
///
/// Only constructible via [`SchemaGraphBuilder::build`], so querying hierarchy
/// is impossible before cycle-checking.
#[derive(Debug)]
pub(super) struct SchemaGraph<'a> {
    adjacency: SchemaAdjacency<'a>,
    /// Resolved Schemas in topological order (parents before children;
    /// simultaneous roots in raw-map insertion order).
    topological_order: IndexSet<SchemaNameRef<'a>>,
}

impl<'a> SchemaGraph<'a> {
    /// Returns every resolved Schema in topological order.
    pub(super) fn topological_order(
        &self,
    ) -> impl Iterator<Item = SchemaNameRef<'a>> + '_ {
        self.topological_order.iter().copied()
    }

    /// Borrows `name`'s raw `extends` parent list. Returns an empty slice if
    /// `name` is unknown or is the excluded Global Schema.
    #[must_use]
    pub(super) fn parents_of(&self, name: SchemaNameRef<'_>) -> &[SchemaName] {
        self.adjacency.parents_of(name)
    }

    /// Returns every Schema's direct `extends` children, keyed by parent name.
    /// Only Schemas with at least one child appear.
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

    /// Returns every Schema's transitive `extends` descendants, keyed by
    /// ancestor name.
    ///
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
        let topo_rank =
            self.adjacency.compute_topo_rank(&self.topological_order);

        // Leaves first, roots last: each child's closure is already finalized
        // (later topological position) before its parent runs.
        let mut accumulator = DescendantAccumulator::new(count, topo_rank);
        for &name in self.topological_order.iter().rev() {
            let Some(node) = self.adjacency.index_of(name) else {
                continue;
            };
            accumulator.accumulate(node, self.adjacency.children_slice(node));
        }

        accumulator.into_descendant_names(&self.adjacency)
    }
}

/// Per-node transitive-descendant accumulator for
/// [`SchemaGraph::descendants_by_name`]'s reverse-topological sweep.
///
/// `seen` is scratch, cleared before each `accumulate` call returns, so
/// deduplication costs `O(|descendants(node)|)` rather than a full-array clear
/// per node.
struct DescendantAccumulator {
    seen: bit_vec::BitVec,
    /// Descendant list per node, finalized once every node has been swept
    /// (leaves first).
    descendants: Vec<Vec<DenseIndex>>,
    /// Topological rank per node, used to keep each finalized descendant list
    /// in globally topological order.
    rank: Vec<u32>,
}

impl DescendantAccumulator {
    fn new(node_count: usize, rank: Vec<u32>) -> Self {
        Self {
            seen: bit_vec::BitVec::from_elem(node_count, false),
            descendants: vec![Vec::new(); node_count],
            rank,
        }
    }

    /// Accumulates `node`'s direct-plus-inherited descendants from its
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

    /// Folds `child`'s already-finalized descendant list into `node`'s,
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

    /// Appends `descendant` to `node`'s list. No-op if `node` is out of range.
    fn record(&mut self, node: DenseIndex, descendant: DenseIndex) {
        if let Some(list) = self.descendants.get_mut(node.index()) {
            list.push(descendant);
        }
    }

    /// Consumes the accumulator, returning every node's transitive descendants
    /// as owned [`SchemaName`] sets keyed by ancestor name. Schemas with no
    /// descendants are omitted.
    fn into_descendant_names(
        self,
        adjacency: &SchemaAdjacency<'_>,
    ) -> IndexMap<SchemaName, IndexSet<SchemaName>> {
        let lists = self.descendants;
        let mut result: IndexMap<SchemaName, IndexSet<SchemaName>> =
            IndexMap::new();
        for (i, name) in adjacency.names.iter().enumerate() {
            let Some(node_descendants) = lists.get(i) else {
                continue;
            };
            if node_descendants.is_empty() {
                continue;
            }
            let descendants: IndexSet<SchemaName> = node_descendants
                .iter()
                .filter_map(|&d| adjacency.names.get(d.index()))
                .map(|&n| SchemaName::from(n))
                .collect();
            result.insert(SchemaName::from(*name), descendants);
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::{super::GLOBAL_SCHEMA_NAME, builder::SchemaGraphBuilder, *};

    /// Builds an empty [`RawSchema`] extending `extends`.
    fn schema(extends: &[&str]) -> super::super::RawSchema {
        super::super::RawSchema {
            extends: extends.iter().map(|&s| SchemaName::from(s)).collect(),
            ..super::super::RawSchema::default()
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

        #[test]
        fn sorts_children_by_topological_rank_not_insertion_order() {
            // To exercise the sort, raw CSR order must differ from
            // topological order.  sci_fi extends parent (declared before
            // book), so raw CSR for parent's children is [sci_fi, book].
            // Topological order is [parent, book, sci_fi] (book first
            // because sci_fi depends on it).  After sort by rank the
            // children become [book, sci_fi].
            let mut raw = IndexMap::new();
            raw.insert(SchemaName::from("parent"), schema(&[]));
            raw.insert(SchemaName::from("sci_fi"), schema(&["parent", "book"]));
            raw.insert(SchemaName::from("book"), schema(&["parent"]));
            let graph = SchemaGraphBuilder::new(
                &raw,
                SchemaNameRef::from(GLOBAL_SCHEMA_NAME),
            )
            .0
            .build()
            .unwrap();
            let children = graph.children_by_name();

            let parent_children: Vec<&str> = children
                .get("parent")
                .unwrap()
                .iter()
                .map(|n| n.as_str())
                .collect();
            assert_eq!(
                parent_children,
                vec!["book", "sci_fi"],
                "children must be in topological order (book before sci_fi), \
                 not raw CSR insertion order"
            );
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
}
