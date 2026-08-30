//! DAG bookkeeping for the `extends` relationship, linearized via Kahn's
//! topological sort.
//!
//! [`SchemaGraph`] owns graph mechanics so [`super::builder::SchemaBuilder`]
//! can drive resolution order without tangling traversal into field-merge
//! logic. Adjacency is stored as a compressed sparse row (CSR) graph over dense
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
//!    [`parents_of`], [`hierarchy`].
//!
//! [`topological_order`]: SchemaGraph::topological_order
//! [`parents_of`]: SchemaGraph::parents_of
//! [`hierarchy`]: SchemaGraph::hierarchy

mod adjacency;
mod builder;
mod cycle;

use adjacency::{DenseIndex, SchemaAdjacency};
pub(super) use builder::SchemaGraphBuilder;
#[cfg(test)]
use indexmap::IndexMap;
use indexmap::IndexSet;

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
    topo_order: IndexSet<SchemaNameRef<'a>>,
    /// Topological rank per node, computed once by
    /// `SchemaGraphBuilder::build`.
    topo_rank: Vec<u32>,
}

impl<'a> SchemaGraph<'a> {
    /// Returns every resolved Schema in topological order.
    pub(super) fn topological_order(
        &self,
    ) -> impl Iterator<Item = SchemaNameRef<'a>> + '_ {
        self.topo_order.iter().copied()
    }

    /// Borrows `name`'s raw `extends` parent list. Returns an empty slice if
    /// `name` is unknown or is the excluded Global Schema.
    #[must_use]
    pub(super) fn parents_of(&self, name: SchemaNameRef<'_>) -> &[SchemaName] {
        self.adjacency.parents_of(name)
    }

    /// Returns every Schema's direct children and transitive descendants,
    /// filtered by `keep`.
    ///
    /// `keep(candidate, ancestor)` decides whether `candidate` still counts as
    /// a child/descendant of `ancestor` — callers use this to drop links a
    /// failed parent resolution invalidated. Descendant closure is computed
    /// once, then converted straight into the owned sets the caller stores.
    pub(super) fn hierarchy<'b>(
        &'b self,
        mut keep: impl FnMut(SchemaNameRef<'a>, SchemaNameRef<'a>) -> bool + 'b,
    ) -> impl Iterator<
        Item = (SchemaName, IndexSet<SchemaName>, IndexSet<SchemaName>),
    > + 'b {
        let descendant_indices = self.descendant_indices();
        self.adjacency.names_iter().enumerate().map(move |(i, name)| {
            let node = DenseIndex::from_usize(i);
            let children = Self::resolve_names(
                &self.adjacency,
                self.adjacency.children_slice(node),
                name,
                &mut keep,
            );
            let descendants: &[DenseIndex] =
                descendant_indices.get(i).map_or(&[], Vec::as_slice);
            let descendants = Self::resolve_names(
                &self.adjacency,
                descendants,
                name,
                &mut keep,
            );
            (SchemaName::from(name), children, descendants)
        })
    }

    /// Resolves `indices` to names, keeping only those `keep(name, ancestor)`
    /// accepts.
    fn resolve_names(
        adjacency: &SchemaAdjacency<'a>,
        indices: &[DenseIndex],
        ancestor: SchemaNameRef<'a>,
        keep: &mut impl FnMut(SchemaNameRef<'a>, SchemaNameRef<'a>) -> bool,
    ) -> IndexSet<SchemaName> {
        indices
            .iter()
            .filter_map(|&index| adjacency.name_of(index))
            .filter(|&name| keep(name, ancestor))
            .map(SchemaName::from)
            .collect()
    }

    /// Returns every node's transitive descendants as dense indices.
    ///
    /// Output-sensitive Habib-Morvan-Rampon transitive closure, `O(V + E +
    /// Σ|closure(x)|)`, proportional to the closure's actual size rather than
    /// the `O(V²/w)` a bitset DP pays unconditionally. Requires children
    /// iterated in topological-rank order, which [`SchemaGraphBuilder::build`]
    /// already sorted `child_targets` into.
    fn descendant_indices(&self) -> Vec<Vec<DenseIndex>> {
        let count = self.adjacency.node_count();

        // Leaves first, roots last: each child's closure is already finalized
        // (later topological position) before its parent runs.
        let mut accumulator =
            DescendantAccumulator::new(count, &self.topo_rank);
        for &name in self.topo_order.iter().rev() {
            let Some(node) = self.adjacency.index_of(name) else {
                continue;
            };
            accumulator.accumulate(node, self.adjacency.children_slice(node));
        }

        accumulator.into_descendant_indices()
    }
}

/// Per-node transitive-descendant accumulator for [`SchemaGraph::hierarchy`]'s
/// reverse-topological sweep.
///
/// `seen` is scratch, cleared before each `accumulate` call returns, so
/// deduplication costs `O(|descendants(node)|)` rather than a full-array clear
/// per node.
struct DescendantAccumulator<'b> {
    seen: bit_vec::BitVec,
    /// Descendant list per node, finalized once every node has been swept
    /// (leaves first).
    descendants: Vec<Vec<DenseIndex>>,
    /// Topological rank per node, used to keep each finalized descendant list
    /// in globally topological order.
    rank: &'b [u32],
}

impl<'b> DescendantAccumulator<'b> {
    fn new(node_count: usize, rank: &'b [u32]) -> Self {
        Self {
            seen: bit_vec::BitVec::from_elem(node_count, false),
            descendants: vec![Vec::new(); node_count],
            rank,
        }
    }

    /// Accumulates `node`'s direct-plus-inherited descendants from its
    /// already-topological-rank-sorted `children`.
    fn accumulate(&mut self, node: DenseIndex, children: &[DenseIndex]) {
        let node_index = node.index();
        let Some(node_descendants) = self.descendants.get_mut(node_index)
        else {
            return;
        };
        let mut node_descendants = std::mem::take(node_descendants);
        for &child in children {
            self.merge_child_into(&mut node_descendants, child);
        }
        // Required: without this, node_descendants' order is
        // child-processing-interleaved, not globally topological.
        let rank = self.rank;
        node_descendants
            .sort_by_key(|d| rank.get(d.index()).copied().unwrap_or(u32::MAX));
        for &descendant in &node_descendants {
            self.seen.set(descendant.index(), false);
        }
        if let Some(slot) = self.descendants.get_mut(node_index) {
            *slot = node_descendants;
        }
    }

    /// Folds `child`'s already-finalized descendant list into
    /// `node_descendants`, deduplicating via `self.seen`.
    fn merge_child_into(
        &mut self,
        node_descendants: &mut Vec<DenseIndex>,
        child: DenseIndex,
    ) {
        if self.seen.get(child.index()) == Some(true) {
            return;
        }
        self.seen.set(child.index(), true);
        node_descendants.push(child);
        if let Some(child_descendants) = self.descendants.get(child.index()) {
            for &descendant in child_descendants {
                if self.seen.get(descendant.index()) == Some(true) {
                    continue;
                }
                self.seen.set(descendant.index(), true);
                node_descendants.push(descendant);
            }
        }
    }

    /// Consumes the accumulator, returning every node's transitive descendant
    /// indices. Nodes with no descendants keep an empty list.
    fn into_descendant_indices(self) -> Vec<Vec<DenseIndex>> {
        self.descendants
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

    fn hierarchy_for(
        graph: &SchemaGraph<'_>,
        target: &str,
    ) -> (Vec<String>, IndexSet<SchemaName>) {
        graph
            .hierarchy(|_, _| true)
            .find(|(name, _, _)| name.as_str() == target)
            .map(|(_, children, descendants)| {
                (
                    children
                        .iter()
                        .map(|child| child.as_str().to_owned())
                        .collect(),
                    descendants,
                )
            })
            .unwrap_or_default()
    }

    /// Builds a graph from `raw`, discarding construction warnings.
    /// Test-only: eliminates the `SchemaGraphBuilder::new(...).0.build()`
    /// postfix-tuple-indexing chain that appears at every call site below.
    fn build_graph(
        raw: &IndexMap<SchemaName, super::super::RawSchema>,
    ) -> SchemaGraph<'_> {
        let (builder, _warnings) = SchemaGraphBuilder::new(
            raw,
            SchemaNameRef::from(GLOBAL_SCHEMA_NAME),
        );
        builder.build().expect("acyclic fixture resolves")
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
            let graph = build_graph(&raw);

            assert_eq!(hierarchy_for(&graph, "thing").0, vec!["book"]);
            assert_eq!(hierarchy_for(&graph, "book").0, vec![
                "sci_fi", "memoir"
            ]);
            assert!(hierarchy_for(&graph, "sci_fi").0.is_empty());
            assert!(hierarchy_for(&graph, "memoir").0.is_empty());
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
            let graph = build_graph(&raw);

            assert_eq!(hierarchy_for(&graph, "book").0, vec!["adaptation"]);
            assert_eq!(hierarchy_for(&graph, "film").0, vec!["adaptation"]);
            assert_eq!(hierarchy_for(&graph, "thing").0, vec!["book", "film"]);
        }

        #[test]
        fn returns_empty_children_when_no_schema_has_children() {
            let mut raw = IndexMap::new();
            raw.insert(SchemaName::from("a"), schema(&[]));
            raw.insert(SchemaName::from("b"), schema(&[]));
            let graph = build_graph(&raw);

            assert!(hierarchy_for(&graph, "a").0.is_empty());
            assert!(hierarchy_for(&graph, "b").0.is_empty());
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
            let graph = build_graph(&raw);

            assert_eq!(
                hierarchy_for(&graph, "parent").0,
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
            let graph = build_graph(&raw);

            assert_eq!(
                hierarchy_for(&graph, "thing").1,
                set(&["adaptation", "book", "film"])
            );
        }

        #[test]
        fn returns_full_transitive_closure() {
            let mut raw = IndexMap::new();
            raw.insert(SchemaName::from("thing"), schema(&[]));
            raw.insert(SchemaName::from("book"), schema(&["thing"]));
            raw.insert(SchemaName::from("sci_fi"), schema(&["book"]));
            raw.insert(SchemaName::from("space_opera"), schema(&["sci_fi"]));
            let graph = build_graph(&raw);

            assert_eq!(
                hierarchy_for(&graph, "thing").1,
                set(&["book", "sci_fi", "space_opera"])
            );
            assert_eq!(
                hierarchy_for(&graph, "book").1,
                set(&["sci_fi", "space_opera"])
            );
            assert_eq!(
                hierarchy_for(&graph, "sci_fi").1,
                set(&["space_opera"])
            );
            assert!(hierarchy_for(&graph, "space_opera").1.is_empty());
        }

        #[test]
        fn excludes_leaf_from_descendants() {
            let mut raw = IndexMap::new();
            raw.insert(SchemaName::from("book"), schema(&[]));
            raw.insert(SchemaName::from("sci_fi"), schema(&["book"]));
            let graph = build_graph(&raw);

            assert!(hierarchy_for(&graph, "sci_fi").1.is_empty());
        }

        #[test]
        fn returns_independent_sets_for_multiple_roots() {
            let mut raw = IndexMap::new();
            raw.insert(SchemaName::from("a"), schema(&[]));
            raw.insert(SchemaName::from("b"), schema(&["a"]));
            raw.insert(SchemaName::from("c"), schema(&[]));
            raw.insert(SchemaName::from("d"), schema(&["c"]));
            let graph = build_graph(&raw);

            assert_eq!(hierarchy_for(&graph, "a").1, set(&["b"]));
            assert_eq!(hierarchy_for(&graph, "c").1, set(&["d"]));
            assert!(hierarchy_for(&graph, "b").1.is_empty());
            assert!(hierarchy_for(&graph, "d").1.is_empty());
        }

        #[test]
        fn orders_descendants_by_topological_rank_not_raw_declaration_order() {
            // `sci_fi` is declared before `book` but extends both `thing`
            // and `book`, so it can only reach topological rank 2 (after
            // `book`'s rank 1). `descendant_indices`'s accumulator must
            // sort `thing`'s descendants by that rank, not by the order
            // `sci_fi`/`book` were pushed while walking the raw map.
            //
            // `IndexSet`'s `PartialEq` compares as a set and ignores
            // order (`self.len() == other.len() && self.is_subset(other)`),
            // so this asserts against a `Vec` collected in iteration
            // order — an `assert_eq!` against another `IndexSet` here
            // would silently pass even if the sort were removed.
            let mut raw = IndexMap::new();
            raw.insert(SchemaName::from("thing"), schema(&[]));
            raw.insert(SchemaName::from("sci_fi"), schema(&["thing", "book"]));
            raw.insert(SchemaName::from("book"), schema(&["thing"]));
            let graph = build_graph(&raw);

            let descendants: Vec<String> = hierarchy_for(&graph, "thing")
                .1
                .iter()
                .map(|name| name.as_str().to_owned())
                .collect();

            assert_eq!(
                descendants,
                vec!["book".to_owned(), "sci_fi".to_owned()],
                "descendants must be in topological-rank order (book before \
                 sci_fi), not raw declaration order (sci_fi before book)"
            );
        }
    }
}
