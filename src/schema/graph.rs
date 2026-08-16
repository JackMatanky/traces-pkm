//! Linearize the `extends` DAG into a resolution order with Kahn's topological
//! sort.
//!
//! [`SchemaGraph`] owns the DAG bookkeeping — parent/child adjacency, Kahn
//! in-degrees, and the Global-first tie-break — so [`super::service`] can
//! drive resolution order without tangling graph mechanics into its
//! field-merge logic.
//!
//! # Ordering
//!
//! A Schema is yielded only after all of its present `extends` parents, so a
//! child always resolves against already-resolved parents. Schemas that tie at
//! in-degree zero yield in name order, except the Global Schema, which is
//! forced to the front of its tier so any sibling that `$ref`s it resolves
//! afterward.
//!
//! # Driving resolution
//!
//! [`SchemaGraph::new`] builds the graph; then loop [`next_ready`] to pop the
//! next resolvable Schema, [`parents_of`] to read its parents, and
//! [`mark_resolved`] once it is done to release its children. After the loop,
//! [`cyclic_remainder`] reports any Schemas a cycle left unresolved.
//!
//! Once every Schema resolved (no cycle), [`children_by_name`] and
//! [`descendants_by_name`] give the bulk direct-child and transitive-extender
//! sets [`super::model::Schema::children`]/
//! [`super::model::Schema::descendants`] are populated from — one pass over the
//! whole DAG rather than an O(n)-per-lookup scan.
//!
//! [`next_ready`]: SchemaGraph::next_ready
//! [`parents_of`]: SchemaGraph::parents_of
//! [`mark_resolved`]: SchemaGraph::mark_resolved
//! [`cyclic_remainder`]: SchemaGraph::cyclic_remainder
//! [`children_by_name`]: SchemaGraph::children_by_name
//! [`descendants_by_name`]: SchemaGraph::descendants_by_name

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use super::{
    GLOBAL_SCHEMA_NAME,
    error::SchemaWarning,
    name::{SchemaName, SchemaNameRef},
    raw::RawSchema,
};

/// Track Kahn's-algorithm state for linearizing the `extends` DAG.
///
/// Isolates the Global-first tie-break from the field-merge logic in
/// [`super::service`].
pub(super) struct SchemaGraph<'a> {
    /// Each Schema's `extends` parents, filtered to present targets and kept
    /// in declaration order; the Global Schema's list is force-emptied so it
    /// never inherits. Backs [`parents_of`](Self::parents_of) and the bulk
    /// [`children_by_name`](Self::children_by_name) accessor.
    parents_by_name: BTreeMap<SchemaNameRef<'a>, Vec<SchemaNameRef<'a>>>,
    /// Reverse `extends` adjacency (parent to children), used by
    /// [`mark_resolved`](Self::mark_resolved) to decrement children's
    /// in-degrees. Unlike [`children_by_name`](Self::children_by_name), this
    /// keeps an edge into the Global Schema when some Schema names it as a
    /// parent (Kahn's algorithm needs that edge to release the child's
    /// in-degree, even though Global is never a real `extends` link for
    /// field-inheritance purposes).
    kahn_children_by_name: BTreeMap<SchemaNameRef<'a>, Vec<SchemaNameRef<'a>>>,
    /// Count of not-yet-resolved parents per Schema; a Schema becomes ready
    /// when it reaches zero. The Global Schema is forced to zero at
    /// construction.
    in_degree: BTreeMap<SchemaNameRef<'a>, usize>,
    /// Schemas at in-degree zero awaiting resolution, with the Global Schema
    /// reordered to the front so it resolves before any sibling that `$ref`s
    /// it.
    queue: VecDeque<SchemaNameRef<'a>>,
    /// Schemas already popped by [`next_ready`](Self::next_ready); its
    /// complement in `raw_schemas` is the cyclic remainder.
    visited: BTreeSet<SchemaNameRef<'a>>,
}

impl<'a> SchemaGraph<'a> {
    /// Build `raw_schemas`' `extends` adjacency (parent to children) and Kahn
    /// in-degrees, seeding the ready queue with every in-degree-zero Schema.
    ///
    /// Filtering and tie-breaking:
    ///
    /// - Each Schema's `extends` list is filtered to targets `raw_schemas`
    ///   actually contains; a missing target emits
    ///   [`SchemaWarning::MissingExtendsTarget`].
    /// - The reserved Global Schema has no effective parents for resolution
    ///   ordering or field inheritance. It is a flat `$ref`-able reference
    ///   pool, not a link in the `extends` chain.
    /// - Kahn's sort only guarantees parent-before-child ordering along
    ///   `extends` edges, so several Schemas can tie at in-degree zero. The
    ///   ready queue reorders Global to the front of that tier so it resolves
    ///   before any sibling that might `$ref` it.
    pub(super) fn new(
        raw_schemas: &'a BTreeMap<SchemaName, RawSchema>,
        warnings: &mut Vec<SchemaWarning>,
    ) -> Self {
        // `BTreeMap` iteration is name-sorted, so the parent order for a given
        // schema matches declaration order in its own `extends` list while the
        // overall processing order stays deterministic.
        let mut parents_by_name: BTreeMap<
            SchemaNameRef<'_>,
            Vec<SchemaNameRef<'_>>,
        > = BTreeMap::new();
        for (name, raw) in raw_schemas {
            let mut parents = Vec::with_capacity(raw.extends.len());
            for target in &raw.extends {
                if raw_schemas.contains_key(target) {
                    parents.push(target.as_ref());
                } else {
                    warnings.push(SchemaWarning::MissingExtendsTarget {
                        schema: name.clone(),
                        target: target.clone(),
                    });
                }
            }
            parents_by_name.insert(name.as_ref(), parents);
        }

        // An edge runs parent -> child, so a child's in-degree is its
        // (filtered) parent count.
        let mut in_degree: BTreeMap<SchemaNameRef<'_>, usize> = BTreeMap::new();
        let mut kahn_children_by_name: BTreeMap<
            SchemaNameRef<'_>,
            Vec<SchemaNameRef<'_>>,
        > = BTreeMap::new();
        for (&name, parents) in &parents_by_name {
            in_degree.insert(name, parents.len());
            for &parent in parents {
                kahn_children_by_name.entry(parent).or_default().push(name);
            }
        }

        if let Some(parents) = parents_by_name.get_mut(GLOBAL_SCHEMA_NAME) {
            parents.clear();
        }
        if let Some(degree) = in_degree.get_mut(GLOBAL_SCHEMA_NAME) {
            *degree = 0;
        }

        let mut queue: VecDeque<SchemaNameRef<'_>> = in_degree
            .iter()
            .filter(|&(_, &degree)| degree == 0)
            .map(|(&name, _)| name)
            .collect();
        if let Some(position) =
            queue.iter().position(|&name| name.as_str() == GLOBAL_SCHEMA_NAME)
            && let Some(global) = queue.remove(position)
        {
            queue.push_front(global);
        }

        Self {
            parents_by_name,
            kahn_children_by_name,
            in_degree,
            queue,
            visited: BTreeSet::new(),
        }
    }

    /// Pop the next Schema whose in-degree reached zero, marking it visited, or
    /// `None` once the ready queue drains.
    pub(super) fn next_ready(&mut self) -> Option<SchemaNameRef<'a>> {
        let name = self.queue.pop_front()?;
        self.visited.insert(name);
        Some(name)
    }

    /// Borrow `name`'s filtered `extends` parent list, or an empty slice if
    /// `name` is not a known Schema.
    pub(super) fn parents_of(
        &self,
        name: SchemaNameRef<'_>,
    ) -> &[SchemaNameRef<'a>] {
        self.parents_by_name.get(name.as_str()).map_or(&[], Vec::as_slice)
    }

    /// Record `name` as resolved, releasing any children whose in-degree just
    /// hit zero into the ready queue.
    pub(super) fn mark_resolved(&mut self, name: SchemaNameRef<'_>) {
        for &child in
            self.kahn_children_by_name.get(name.as_str()).into_iter().flatten()
        {
            if let Some(degree) = self.in_degree.get_mut(&child) {
                *degree = degree.saturating_sub(1);
                if *degree == 0 {
                    self.queue.push_back(child);
                }
            }
        }
    }

    /// Return every Schema name that never reached in-degree zero (a cycle
    /// member), or `None` if every Schema in `raw_schemas` was visited.
    pub(super) fn cyclic_remainder(
        &self,
        raw_schemas: &BTreeMap<SchemaName, RawSchema>,
    ) -> Option<Vec<SchemaName>> {
        if self.visited.len() == raw_schemas.len() {
            return None;
        }
        Some(
            raw_schemas
                .keys()
                .filter(|name| !self.visited.contains(name.as_str()))
                .cloned()
                .collect(),
        )
    }

    /// Return every Schema's direct `extends` children, keyed by parent name.
    ///
    /// Excludes the Global Schema as a parent: it is a flat reference pool,
    /// never a real link in the `extends` chain, so a Schema that (unusually)
    /// declares `extends = ["global"]` still contributes no entry here — the
    /// same behavior [`super::model::Schema::is_a`]'s ancestor-based matching
    /// already has today. Only callable once the DAG is known acyclic (after
    /// [`cyclic_remainder`](Self::cyclic_remainder) returns `None`): the
    /// underlying adjacency is otherwise still mid-resolution.
    #[must_use]
    pub(super) fn children_by_name(
        &self,
    ) -> BTreeMap<SchemaName, BTreeSet<SchemaName>> {
        let mut children: BTreeMap<SchemaName, BTreeSet<SchemaName>> =
            BTreeMap::new();
        for (&name, parents) in &self.parents_by_name {
            for &parent in parents {
                if parent.as_str() != GLOBAL_SCHEMA_NAME {
                    children
                        .entry(SchemaName::from(parent))
                        .or_default()
                        .insert(SchemaName::from(name));
                }
            }
        }
        children
    }

    /// Return every Schema's transitive `extends` descendants, keyed by
    /// ancestor name.
    ///
    /// Computed as a memoized depth-first walk over
    /// [`children_by_name`](Self::children_by_name): each name's descendant
    /// set is built once and reused by every ancestor that reaches it through
    /// a different path, so the whole DAG resolves in `O(V + E)` total rather
    /// than one full traversal per name. Only callable once the DAG is known
    /// acyclic, same as [`children_by_name`](Self::children_by_name).
    #[must_use]
    pub(super) fn descendants_by_name(
        &self,
    ) -> BTreeMap<SchemaName, BTreeSet<SchemaName>> {
        let children = self.children_by_name();
        let mut memo: BTreeMap<SchemaName, BTreeSet<SchemaName>> =
            BTreeMap::new();
        for name in children.keys() {
            Self::descendants_of(name, &children, &mut memo);
        }
        // Drops leaf entries (an empty descendant set) rather than keeping
        // them: callers treat "no entry" and "an empty entry" identically
        // (`unwrap_or_default()`), and a smaller map is one less allocation
        // per Schema with no descendants.
        memo.retain(|_, descendants| !descendants.is_empty());
        memo
    }

    /// Return `name`'s transitive descendant set, computing and memoizing it
    /// (and every descendant's own set, transitively) on first visit.
    fn descendants_of(
        name: &SchemaName,
        children: &BTreeMap<SchemaName, BTreeSet<SchemaName>>,
        memo: &mut BTreeMap<SchemaName, BTreeSet<SchemaName>>,
    ) -> BTreeSet<SchemaName> {
        if let Some(cached) = memo.get(name) {
            return cached.clone();
        }
        let mut result = BTreeSet::new();
        if let Some(direct) = children.get(name) {
            for child in direct {
                result.insert(child.clone());
                result.extend(Self::descendants_of(child, children, memo));
            }
        }
        memo.insert(name.clone(), result.clone());
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds an empty [`RawSchema`] extending `extends`.
    fn schema(extends: &[&str]) -> RawSchema {
        RawSchema {
            extends: extends.iter().map(|&s| SchemaName::from(s)).collect(),
            ..RawSchema::default()
        }
    }

    mod next_ready {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn pops_global_before_an_alphabetically_earlier_sibling() {
            // Isolates the Global-first reorder at the graph level, without
            // going through field resolution: `"author"` sorts before
            // `"global"` in name order, so this fails without the reorder.
            let mut raw = BTreeMap::new();
            raw.insert(SchemaName::from(GLOBAL_SCHEMA_NAME), schema(&[]));
            raw.insert(SchemaName::from("author"), schema(&[]));
            let mut warnings = Vec::new();

            let mut graph = SchemaGraph::new(&raw, &mut warnings);

            assert_eq!(
                graph.next_ready(),
                Some(SchemaNameRef::from(GLOBAL_SCHEMA_NAME))
            );
            assert_eq!(graph.next_ready(), Some(SchemaNameRef::from("author")));
            assert_eq!(graph.next_ready(), None);
        }
    }

    mod children_by_name {
        use pretty_assertions::assert_eq;

        use super::*;

        fn set(names: &[&str]) -> BTreeSet<SchemaName> {
            names.iter().map(|&name| SchemaName::from(name)).collect()
        }

        #[test]
        fn returns_only_direct_extenders_for_a_branching_tree() {
            // thing <- book <- {sci_fi, memoir}
            let mut raw = BTreeMap::new();
            raw.insert(SchemaName::from("thing"), schema(&[]));
            raw.insert(SchemaName::from("book"), schema(&["thing"]));
            raw.insert(SchemaName::from("sci_fi"), schema(&["book"]));
            raw.insert(SchemaName::from("memoir"), schema(&["book"]));
            let mut warnings = Vec::new();

            let children =
                SchemaGraph::new(&raw, &mut warnings).children_by_name();

            assert_eq!(children.get("thing"), Some(&set(&["book"])));
            assert_eq!(children.get("book"), Some(&set(&["memoir", "sci_fi"])));
            assert_eq!(children.get("sci_fi"), None);
            assert_eq!(children.get("memoir"), None);
        }

        #[test]
        fn a_multi_parent_schema_appears_in_every_parents_direct_children() {
            // thing <- {book, film} <- adaptation (both parents): the
            // genuine diamond shape, distinct from the branching-tree fixture
            // above — a node with two parents converging, not one parent
            // fanning out to two children.
            let mut raw = BTreeMap::new();
            raw.insert(SchemaName::from("thing"), schema(&[]));
            raw.insert(SchemaName::from("book"), schema(&["thing"]));
            raw.insert(SchemaName::from("film"), schema(&["thing"]));
            raw.insert(
                SchemaName::from("adaptation"),
                schema(&["book", "film"]),
            );
            let mut warnings = Vec::new();

            let children =
                SchemaGraph::new(&raw, &mut warnings).children_by_name();

            assert_eq!(children.get("book"), Some(&set(&["adaptation"])));
            assert_eq!(children.get("film"), Some(&set(&["adaptation"])));
            assert_eq!(children.get("thing"), Some(&set(&["book", "film"])));
        }

        #[test]
        fn excludes_the_global_schema_as_a_parent() {
            let mut raw = BTreeMap::new();
            raw.insert(SchemaName::from(GLOBAL_SCHEMA_NAME), schema(&[]));
            raw.insert(SchemaName::from("book"), schema(&[GLOBAL_SCHEMA_NAME]));
            let mut warnings = Vec::new();

            let children =
                SchemaGraph::new(&raw, &mut warnings).children_by_name();

            assert_eq!(children.get(GLOBAL_SCHEMA_NAME), None);
        }
    }

    mod descendants_by_name {
        use pretty_assertions::assert_eq;

        use super::*;

        fn set(names: &[&str]) -> BTreeSet<SchemaName> {
            names.iter().map(|&name| SchemaName::from(name)).collect()
        }

        #[test]
        fn deduplicates_a_diamond_dags_shared_descendant() {
            // thing <- {book, film} <- adaptation (both parents)
            let mut raw = BTreeMap::new();
            raw.insert(SchemaName::from("thing"), schema(&[]));
            raw.insert(SchemaName::from("book"), schema(&["thing"]));
            raw.insert(SchemaName::from("film"), schema(&["thing"]));
            raw.insert(
                SchemaName::from("adaptation"),
                schema(&["book", "film"]),
            );
            let mut warnings = Vec::new();

            let descendants =
                SchemaGraph::new(&raw, &mut warnings).descendants_by_name();

            assert_eq!(
                descendants.get("thing"),
                Some(&set(&["adaptation", "book", "film"]))
            );
        }

        #[test]
        fn returns_the_full_transitive_closure_through_a_three_level_chain() {
            let mut raw = BTreeMap::new();
            raw.insert(SchemaName::from("thing"), schema(&[]));
            raw.insert(SchemaName::from("book"), schema(&["thing"]));
            raw.insert(SchemaName::from("sci_fi"), schema(&["book"]));
            raw.insert(SchemaName::from("space_opera"), schema(&["sci_fi"]));
            let mut warnings = Vec::new();

            let descendants =
                SchemaGraph::new(&raw, &mut warnings).descendants_by_name();

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
        fn returns_no_entry_for_a_leaf_schema() {
            let mut raw = BTreeMap::new();
            raw.insert(SchemaName::from("book"), schema(&[]));
            raw.insert(SchemaName::from("sci_fi"), schema(&["book"]));
            let mut warnings = Vec::new();

            let descendants =
                SchemaGraph::new(&raw, &mut warnings).descendants_by_name();

            assert_eq!(descendants.get("sci_fi"), None);
        }
    }
}
