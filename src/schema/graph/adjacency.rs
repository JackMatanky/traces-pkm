//! CSR (compressed sparse row) adjacency for the `extends` DAG.
//!
//! [`SchemaAdjacency`] owns the dense index mapping and parent→child edge
//! storage that [`SchemaGraph`] and [`SchemaGraphBuilder`] query during
//! topological sort and hierarchy traversal.
//!
//! [`SchemaGraph`]: super::SchemaGraph
//! [`SchemaGraphBuilder`]: super::builder::SchemaGraphBuilder

use indexmap::{IndexMap, IndexSet};

use super::super::{
    RawSchema, SchemaName, SchemaNameRef, error::SchemaWarning,
};

/// A validated parent→child edge between two dense indices.
struct SchemaGraphEdge {
    source: DenseIndex,
    target: DenseIndex,
}

/// `extends` adjacency between Schemas, stored as a CSR graph over dense
/// [`DenseIndex`]es.
///
/// The Global Schema (`excluded`) is never assigned a node. It participates via
/// `$ref`, never `extends`, so it must never compete for topological position
/// or appear in any hierarchy query.
#[derive(Debug)]
pub(super) struct SchemaAdjacency<'a> {
    /// The Global Schema name, excluded from the graph.
    excluded: SchemaNameRef<'a>,
    /// Dense index to name, in raw-map insertion order, `excluded` skipped.
    names: Vec<SchemaNameRef<'a>>,
    /// Name to dense index, built once at construction.
    index_of: IndexMap<SchemaNameRef<'a>, DenseIndex>,
    /// Each schema's raw `extends` list, deduplicated by first occurrence and
    /// order-preserved. [`Self::parents_of`] exposes this instead of the raw
    /// list so a duplicated `extends` target (already warned via
    /// [`DuplicateExtendsTarget`](SchemaWarning::DuplicateExtendsTarget)) is
    /// processed once by callers, not once per repetition.
    declared_parents: IndexMap<SchemaNameRef<'a>, Vec<SchemaName>>,
    /// CSR parent→child offsets: node `i`'s children occupy
    /// `child_targets[child_offsets[i]..child_offsets[i + 1]]`.
    child_offsets: Vec<u32>,
    /// CSR parent→child targets, indexed via
    /// [`child_offsets`](Self::child_offsets).
    child_targets: Vec<DenseIndex>,
}

impl<'a> SchemaAdjacency<'a> {
    /// Builds the `extends` adjacency, skipping `excluded` entirely.
    ///
    /// An `extends` edge naming `excluded` is silently ignored (the Global
    /// Schema is referenced via `$ref`, never `extends`).
    ///
    /// # Warnings
    ///
    /// - [`DuplicateExtendsTarget`] if the same `extends` target appears more
    ///   than once; checked before target-existence, so a repeated unresolvable
    ///   target warns `Missing` on first occurrence and `Duplicate` thereafter
    /// - [`MissingExtendsTarget`] if an `extends` target has no corresponding
    ///   Schema file (other than `excluded`)
    ///
    /// [`MissingExtendsTarget`]: SchemaWarning::MissingExtendsTarget
    /// [`DuplicateExtendsTarget`]: SchemaWarning::DuplicateExtendsTarget
    pub(super) fn build(
        raw: &'a IndexMap<SchemaName, RawSchema>,
        excluded: SchemaNameRef<'a>,
    ) -> (Self, Vec<SchemaWarning>) {
        let (names, index_of) = Self::assign_indices(raw, excluded);
        let count = names.len();
        let (edges, warnings) =
            Self::validate_edges(raw, excluded, &names, &index_of);
        let (child_offsets, child_targets) = Self::build_csr(edges, count);
        let declared_parents = Self::dedupe_declared_parents(raw);
        (
            Self {
                excluded,
                names,
                index_of,
                declared_parents,
                child_offsets,
                child_targets,
            },
            warnings,
        )
    }

    /// Deduplicates each schema's raw `extends` list by first occurrence,
    /// preserving declaration order. See [`Self::declared_parents`].
    fn dedupe_declared_parents(
        raw: &'a IndexMap<SchemaName, RawSchema>,
    ) -> IndexMap<SchemaNameRef<'a>, Vec<SchemaName>> {
        raw.iter()
            .map(|(name, raw_schema)| {
                let mut seen = IndexSet::new();
                let parents = raw_schema
                    .extends
                    .iter()
                    .filter(|target| seen.insert(target.as_str()))
                    .cloned()
                    .collect();
                (name.as_ref(), parents)
            })
            .collect()
    }

    /// Assigns dense indices in raw-map insertion order, skipping `excluded`.
    fn assign_indices(
        raw: &'a IndexMap<SchemaName, RawSchema>,
        excluded: SchemaNameRef<'a>,
    ) -> (Vec<SchemaNameRef<'a>>, IndexMap<SchemaNameRef<'a>, DenseIndex>) {
        let names: Vec<SchemaNameRef<'a>> = raw
            .keys()
            .map(SchemaName::as_ref)
            .filter(|&name| name != excluded)
            .collect();
        let index_of: IndexMap<SchemaNameRef<'a>, DenseIndex> = names
            .iter()
            .enumerate()
            .map(|(i, &name)| (name, DenseIndex::from_usize(i)))
            .collect();
        (names, index_of)
    }

    /// Validates and deduplicates each schema's `extends` targets, producing
    /// the edge list for CSR construction and any warnings.
    fn validate_edges(
        raw: &'a IndexMap<SchemaName, RawSchema>,
        excluded: SchemaNameRef<'a>,
        names: &[SchemaNameRef<'a>],
        index_of: &IndexMap<SchemaNameRef<'a>, DenseIndex>,
    ) -> (Vec<SchemaGraphEdge>, Vec<SchemaWarning>) {
        let mut warnings = Vec::new();
        let mut valid_edges: Vec<SchemaGraphEdge> = Vec::new();
        let mut seen_targets = IndexSet::new();
        for (i, &name) in names.iter().enumerate() {
            let Some(raw_schema) = raw.get(name.as_str()) else {
                continue;
            };
            seen_targets.clear();
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
                valid_edges.push(SchemaGraphEdge {
                    source: DenseIndex::from_usize(i),
                    target: target_index,
                });
            }
        }
        (valid_edges, warnings)
    }

    /// Builds CSR offsets and targets from the validated edge list.
    fn build_csr(
        valid_edges: Vec<SchemaGraphEdge>,
        count: usize,
    ) -> (Vec<u32>, Vec<DenseIndex>) {
        let mut child_offsets: Vec<u32> =
            Vec::with_capacity(count.saturating_add(1));
        let mut running: u32 = 0;
        child_offsets.push(running);

        // Compute out-degree per node from the edge list.
        let mut out_degree: Vec<u32> = vec![0; count];
        for edge in &valid_edges {
            if let Some(degree) = out_degree.get_mut(edge.target.index()) {
                *degree = degree.saturating_add(1);
            }
        }
        for &out in &out_degree {
            running = running.saturating_add(out);
            child_offsets.push(running);
        }
        let edge_count = DenseIndex::widen_usize(running);

        // Fill CSR targets via a scratch write cursor per node.
        let mut cursor: Vec<u32> =
            child_offsets.iter().take(count).copied().collect();
        let mut child_targets: Vec<DenseIndex> =
            vec![DenseIndex::from(0u32); edge_count];
        for edge in valid_edges {
            let Some(slot) = cursor.get_mut(edge.target.index()) else {
                continue;
            };
            if let Some(target_slot) =
                child_targets.get_mut(DenseIndex::widen_usize(*slot))
            {
                *target_slot = edge.source;
            }
            *slot = slot.saturating_add(1);
        }
        (child_offsets, child_targets)
    }

    /// Computes a topological rank vector from `topo_order`.
    ///
    /// Nodes not present in `topo_order` receive rank `u32::MAX`. Used by
    /// both [`SchemaGraph::hierarchy`] and
    /// [`SchemaGraphBuilder::build`](super::builder::SchemaGraphBuilder::build).
    pub(super) fn compute_topo_rank(
        &self,
        topo_order: &IndexSet<SchemaNameRef<'a>>,
    ) -> Vec<u32> {
        let count = self.node_count();
        let mut rank: Vec<u32> = vec![u32::MAX; count];
        for (r, name) in topo_order.iter().enumerate() {
            if let Some(index) = self.index_of(*name)
                && let Some(slot) = rank.get_mut(index.index())
            {
                *slot = DenseIndex::saturating_u32(r);
            }
        }
        rank
    }

    /// Returns `node`'s CSR children slice (its direct `extends`
    /// reverse-adjacency), or an empty slice if `node` is out of range.
    pub(super) fn children_slice(&self, node: DenseIndex) -> &[DenseIndex] {
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

    /// Mutable counterpart of [`children_slice`](Self::children_slice), for
    /// sorting a node's children in place.
    pub(super) fn children_slice_mut(
        &mut self,
        node: DenseIndex,
    ) -> &mut [DenseIndex] {
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

    /// Borrows `name`'s deduplicated, declaration-order `extends` parent list.
    /// Returns an empty slice if `name` is unknown or is `excluded`.
    pub(super) fn parents_of(&self, name: SchemaNameRef<'_>) -> &[SchemaName] {
        if name == self.excluded {
            return &[];
        }
        self.declared_parents.get(name.as_str()).map_or(&[], Vec::as_slice)
    }

    /// Returns `name`'s dense index, or `None` if unknown or `excluded`.
    pub(super) fn index_of(
        &self,
        name: SchemaNameRef<'_>,
    ) -> Option<DenseIndex> {
        self.index_of.get(name.as_str()).copied()
    }

    /// Returns the name at dense index `node`, or `None` if out of range.
    pub(super) fn name_of(
        &self,
        node: DenseIndex,
    ) -> Option<SchemaNameRef<'a>> {
        self.names.get(node.index()).copied()
    }

    /// Returns the number of nodes (Schemas excluding `excluded`).
    pub(super) fn node_count(&self) -> usize {
        self.names.len()
    }

    /// Borrows every dense-indexed Schema name, in construction order.
    pub(super) fn names_iter(
        &self,
    ) -> impl Iterator<Item = SchemaNameRef<'a>> + '_ {
        self.names.iter().copied()
    }

    /// Borrows every CSR child target, across every node, in CSR storage order.
    /// Used to derive per-node in-degree without re-scanning raw `extends`
    /// lists.
    pub(super) fn child_targets_iter(
        &self,
    ) -> impl Iterator<Item = DenseIndex> + '_ {
        self.child_targets.iter().copied()
    }
}

/// Dense per-schema array index, assigned once at construction in raw-map
/// insertion order.
///
/// Array indexing replaces string-hashed lookups on every graph operation. Caps
/// a schema set at `u32::MAX` entries, which is not a real constraint for a
/// filesystem-enumerated `.traces/schemas/*.toml` registry.
#[derive(Copy, Clone, Debug, Eq, Hash, PartialEq)]
pub(super) struct DenseIndex(u32);

impl DenseIndex {
    /// Narrows `n` to `u32`, saturating to `u32::MAX` on overflow.
    pub(super) fn saturating_u32(n: usize) -> u32 {
        u32::try_from(n).unwrap_or(u32::MAX)
    }

    /// Builds a dense index from an array position, saturating per
    /// [`Self::saturating_u32`].
    pub(super) fn from_usize(n: usize) -> Self {
        Self(Self::saturating_u32(n))
    }

    /// Widens back to `usize` for slice indexing.
    pub(super) fn index(self) -> usize {
        Self::widen_usize(self.0)
    }

    /// Widens `n` to `usize`. Saturates to `usize::MAX` rather than panicking
    /// if `usize` is narrower than `u32` (unreachable on all targets this crate
    /// supports).
    pub(super) fn widen_usize(n: u32) -> usize {
        usize::try_from(n).unwrap_or(usize::MAX)
    }
}

/// Builds a dense index from an already-valid `u32` position (e.g. a loop
/// counter already bounded by [`DenseIndex::saturating_u32`]). Unlike
/// [`DenseIndex::from_usize`], performs no saturation.
impl From<u32> for DenseIndex {
    fn from(n: u32) -> Self {
        Self(n)
    }
}

#[cfg(test)]
mod tests {
    use indexmap::IndexMap;

    use super::*;
    use crate::schema::{GLOBAL_SCHEMA_NAME, error::SchemaWarning};

    fn schema(extends: &[&str]) -> crate::schema::RawSchema {
        crate::schema::RawSchema {
            extends: extends.iter().map(|&s| SchemaName::from(s)).collect(),
            ..crate::schema::RawSchema::default()
        }
    }

    fn build_adj(
        raw: &IndexMap<SchemaName, crate::schema::RawSchema>,
    ) -> SchemaAdjacency<'_> {
        let (adj, _warnings) = SchemaAdjacency::build(
            raw,
            SchemaNameRef::from(GLOBAL_SCHEMA_NAME),
        );
        adj
    }

    mod dense_index {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn saturating_u32_narrows_normal_value() {
            assert_eq!(DenseIndex::saturating_u32(42), 42);
        }

        #[test]
        fn saturating_u32_saturates_on_overflow() {
            assert_eq!(DenseIndex::saturating_u32(usize::MAX), u32::MAX);
        }

        #[test]
        fn from_usize_matches_saturating_u32() {
            let idx = DenseIndex::from_usize(7);
            assert_eq!(idx.0, 7);
        }

        #[test]
        fn from_usize_saturates_on_overflow() {
            let idx = DenseIndex::from_usize(usize::MAX);
            assert_eq!(idx.0, u32::MAX);
        }

        #[test]
        fn widen_usize_narrows_normal_value() {
            assert_eq!(DenseIndex::widen_usize(99), 99);
        }

        #[test]
        fn index_round_trips_through_from_usize() {
            let idx = DenseIndex::from_usize(5);
            assert_eq!(idx.index(), 5);
        }
    }

    mod index_of {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn returns_the_insertion_order_index() {
            let mut raw = IndexMap::new();
            raw.insert(SchemaName::from("alpha"), schema(&[]));
            raw.insert(SchemaName::from("book"), schema(&[]));
            raw.insert(SchemaName::from("sci_fi"), schema(&[]));
            let adj = build_adj(&raw);

            assert_eq!(
                adj.index_of(SchemaNameRef::from("alpha")).map(|i| i.0),
                Some(0)
            );
            assert_eq!(
                adj.index_of(SchemaNameRef::from("book")).map(|i| i.0),
                Some(1)
            );
            assert_eq!(
                adj.index_of(SchemaNameRef::from("sci_fi")).map(|i| i.0),
                Some(2)
            );
            assert_eq!(adj.index_of(SchemaNameRef::from("missing")), None);
        }
    }

    mod name_of {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn returns_the_name_at_the_given_index() {
            let mut raw = IndexMap::new();
            raw.insert(SchemaName::from("alpha"), schema(&[]));
            raw.insert(SchemaName::from("book"), schema(&[]));
            let adj = build_adj(&raw);

            assert_eq!(
                adj.name_of(DenseIndex(0)),
                Some(SchemaNameRef::from("alpha"))
            );
            assert_eq!(
                adj.name_of(DenseIndex(1)),
                Some(SchemaNameRef::from("book"))
            );
            assert_eq!(adj.name_of(DenseIndex(2)), None);
        }
    }

    mod node_count_mod {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn matches_the_number_of_names() {
            let mut raw = IndexMap::new();
            raw.insert(SchemaName::from("a"), schema(&[]));
            raw.insert(SchemaName::from("b"), schema(&[]));
            raw.insert(SchemaName::from("c"), schema(&[]));
            let adj = build_adj(&raw);

            assert_eq!(adj.node_count(), 3);
        }
    }

    mod parents_of_tests {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn returns_raw_extends_for_book_extending_global() {
            let mut raw = IndexMap::new();
            raw.insert(SchemaName::from(GLOBAL_SCHEMA_NAME), schema(&[]));
            raw.insert(SchemaName::from("book"), schema(&[GLOBAL_SCHEMA_NAME]));
            let adj = build_adj(&raw);

            assert_eq!(adj.parents_of(SchemaNameRef::from("book")), &[
                SchemaName::from(GLOBAL_SCHEMA_NAME)
            ]);
        }

        #[test]
        fn deduplicates_repeated_extends_targets_by_first_occurrence() {
            let mut raw = IndexMap::new();
            raw.insert(SchemaName::from("book"), schema(&[]));
            raw.insert(SchemaName::from("child"), schema(&["book", "book"]));
            let adj = build_adj(&raw);

            assert_eq!(adj.parents_of(SchemaNameRef::from("child")), &[
                SchemaName::from("book"),
            ]);
        }

        #[test]
        fn returns_empty_slice_for_unknown_schema() {
            let mut raw = IndexMap::new();
            raw.insert(SchemaName::from("book"), schema(&[]));
            let adj = build_adj(&raw);

            assert_eq!(adj.parents_of(SchemaNameRef::from("missing")), &[]);
        }

        #[test]
        fn returns_empty_slice_for_the_excluded_schema_even_with_its_own_extends()
         {
            let mut raw = IndexMap::new();
            raw.insert(SchemaName::from(GLOBAL_SCHEMA_NAME), schema(&["book"]));
            raw.insert(SchemaName::from("book"), schema(&[]));
            let adj = build_adj(&raw);

            assert_eq!(
                adj.parents_of(SchemaNameRef::from(GLOBAL_SCHEMA_NAME)),
                &[]
            );
        }
    }

    mod children_slice_tests {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn returns_empty_slice_for_leaf_node() {
            let mut raw = IndexMap::new();
            raw.insert(SchemaName::from("book"), schema(&[]));
            let adj = build_adj(&raw);

            assert!(adj.children_slice(DenseIndex(0)).is_empty());
        }

        #[test]
        fn returns_direct_children_for_a_parent() {
            // book <- {sci_fi, memoir}
            let mut raw = IndexMap::new();
            raw.insert(SchemaName::from("book"), schema(&[]));
            raw.insert(SchemaName::from("sci_fi"), schema(&["book"]));
            raw.insert(SchemaName::from("memoir"), schema(&["book"]));
            let adj = build_adj(&raw);

            let book_idx = adj.index_of(SchemaNameRef::from("book")).unwrap();
            let children: Vec<SchemaNameRef<'_>> = adj
                .children_slice(book_idx)
                .iter()
                .filter_map(|&child| adj.name_of(child))
                .collect();
            assert_eq!(children, vec![
                SchemaNameRef::from("sci_fi"),
                SchemaNameRef::from("memoir"),
            ]);
        }

        #[test]
        fn returns_empty_slice_for_out_of_range_index() {
            let mut raw = IndexMap::new();
            raw.insert(SchemaName::from("book"), schema(&[]));
            let adj = build_adj(&raw);

            assert!(adj.children_slice(DenseIndex(99)).is_empty());
        }
    }

    mod compute_topo_rank_tests {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn assigns_sequential_ranks_from_topological_order() {
            let mut raw = IndexMap::new();
            raw.insert(SchemaName::from("author"), schema(&[]));
            raw.insert(SchemaName::from("book"), schema(&["author"]));
            let adj = build_adj(&raw);

            let mut topo_order = IndexSet::new();
            topo_order.insert(SchemaNameRef::from("author"));
            topo_order.insert(SchemaNameRef::from("book"));

            let rank = adj.compute_topo_rank(&topo_order);

            let author_rank = adj
                .index_of(SchemaNameRef::from("author"))
                .and_then(|index| rank.get(index.index()));
            let book_rank = adj
                .index_of(SchemaNameRef::from("book"))
                .and_then(|index| rank.get(index.index()));

            assert_eq!(author_rank, Some(&0));
            assert_eq!(book_rank, Some(&1));
        }

        #[test]
        fn assigns_max_rank_for_nodes_not_in_topological_order() {
            let mut raw = IndexMap::new();
            raw.insert(SchemaName::from("a"), schema(&[]));
            raw.insert(SchemaName::from("b"), schema(&[]));
            let adj = build_adj(&raw);

            let mut topo_order = IndexSet::new();
            topo_order.insert(SchemaNameRef::from("a"));
            // "b" not in topo_order

            let rank = adj.compute_topo_rank(&topo_order);

            assert_eq!(rank.as_slice(), [0, u32::MAX]);
        }
    }

    mod build_warnings {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn warns_on_duplicate_extends_target() {
            let mut raw = IndexMap::new();
            raw.insert(SchemaName::from("book"), schema(&[]));
            raw.insert(SchemaName::from("child"), schema(&["book", "book"]));
            let (_adj, warnings) = SchemaAdjacency::build(
                &raw,
                SchemaNameRef::from(GLOBAL_SCHEMA_NAME),
            );

            assert_eq!(warnings, vec![SchemaWarning::DuplicateExtendsTarget {
                schema: SchemaName::from("child"),
                target: SchemaName::from("book"),
            }]);
        }

        #[test]
        fn warns_on_missing_extends_target() {
            let mut raw = IndexMap::new();
            raw.insert(SchemaName::from("child"), schema(&["nonexistent"]));
            let (_adj, warnings) = SchemaAdjacency::build(
                &raw,
                SchemaNameRef::from(GLOBAL_SCHEMA_NAME),
            );

            assert_eq!(warnings, vec![SchemaWarning::MissingExtendsTarget {
                schema: SchemaName::from("child"),
                target: SchemaName::from("nonexistent"),
            }]);
        }

        #[test]
        fn warns_missing_then_duplicate_for_repeated_unresolvable_target() {
            let mut raw = IndexMap::new();
            raw.insert(
                SchemaName::from("child"),
                schema(&["missing", "missing"]),
            );
            let (_adj, warnings) = SchemaAdjacency::build(
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
            let (_adj, warnings) = SchemaAdjacency::build(
                &raw,
                SchemaNameRef::from(GLOBAL_SCHEMA_NAME),
            );

            assert!(warnings.is_empty());
        }

        #[test]
        fn ignores_extends_targeting_excluded_schema() {
            let mut raw = IndexMap::new();
            raw.insert(SchemaName::from("book"), schema(&[GLOBAL_SCHEMA_NAME]));
            let (_adj, warnings) = SchemaAdjacency::build(
                &raw,
                SchemaNameRef::from(GLOBAL_SCHEMA_NAME),
            );

            assert!(
                warnings.is_empty(),
                "extending the excluded schema must not produce a warning"
            );
        }
    }
}
