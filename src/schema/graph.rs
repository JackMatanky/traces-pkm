//! `extends` DAG linearization via Kahn's topological sort.
//!
//! [`SchemaGraph`] owns the DAG bookkeeping so [`super::service`] can drive
//! resolution order without tangling graph mechanics into field-merge logic.
//!
//! # Ordering
//!
//! A Schema is yielded after all its present `extends` parents. Tied in-degree
//! Schemas yield in name order, with the Global Schema forced first.
//!
//! # Driving resolution
//!
//! Build with [`SchemaGraph::new`], then loop [`next_ready`]/[`parents_of`]/
//! [`mark_resolved`] in [`Building`] state. Call [`into_resolved`] to check
//! for cycles and transition to [`Resolved`], where [`children_by_name`]/
//! [`descendants_by_name`] give the bulk hierarchy sets.
//!
//! [`next_ready`]: SchemaGraph::next_ready
//! [`parents_of`]: SchemaGraph::parents_of
//! [`mark_resolved`]: SchemaGraph::mark_resolved
//! [`into_resolved`]: SchemaGraph::into_resolved
//! [`children_by_name`]: SchemaGraph::children_by_name
//! [`descendants_by_name`]: SchemaGraph::descendants_by_name

use std::{
    collections::{HashMap, VecDeque},
    marker::PhantomData,
};

use indexmap::{IndexMap, IndexSet};

use super::{
    GLOBAL_SCHEMA_NAME, RawSchema, SchemaName, SchemaNameRef,
    error::SchemaWarning,
};

/// Building state: resolution in progress, queue and in_degree active.
pub(super) struct Building;

/// Resolved state: DAG is acyclic, hierarchy queries available.
pub(super) struct Resolved;

/// Kahn's-algorithm state for linearizing the `extends` DAG.
///
/// `State` enforces valid transitions at compile time:
/// - [`Building`]: call `next_ready`/`parents_of`/`mark_resolved` in a loop
/// - [`Resolved`]: call `children_by_name`/`descendants_by_name`
pub(super) struct SchemaGraph<'a, State = Building> {
    /// Each Schema's `extends` parents, filtered to present targets, in
    /// declaration order. Global's list is force-emptied.
    parents_by_name: IndexMap<SchemaNameRef<'a>, Vec<SchemaNameRef<'a>>>,
    /// Reverse adjacency (parent → children) for decrementing in-degrees.
    children_by_name: IndexMap<SchemaNameRef<'a>, Vec<SchemaNameRef<'a>>>,
    /// Not-yet-resolved parent count; ready at zero.
    in_degree: HashMap<SchemaNameRef<'a>, usize>,
    /// Ready queue, with Global forced to the front.
    queue: VecDeque<SchemaNameRef<'a>>,
    /// Schemas already popped by [`next_ready`](Self::next_ready).
    visited: IndexSet<SchemaNameRef<'a>>,
    _marker: PhantomData<State>,
}

impl<'a, State> SchemaGraph<'a, State> {
    /// Moves the graph into the next lifecycle state.
    fn transition_to<NextState>(self) -> SchemaGraph<'a, NextState> {
        SchemaGraph {
            parents_by_name: self.parents_by_name,
            children_by_name: self.children_by_name,
            in_degree: self.in_degree,
            queue: self.queue,
            visited: self.visited,
            _marker: PhantomData,
        }
    }
}

impl<'a> SchemaGraph<'a, Building> {
    /// Build the `extends` adjacency and seed the ready queue.
    ///
    /// Missing `extends` targets emit [`SchemaWarning::MissingExtendsTarget`].
    /// The Global Schema is forced to the front of its tier.
    pub(super) fn new(
        raw_schemas: &'a IndexMap<SchemaName, RawSchema>,
    ) -> (Self, Vec<SchemaWarning>) {
        let mut warnings = Vec::new();
        let mut parents_by_name: IndexMap<
            SchemaNameRef<'_>,
            Vec<SchemaNameRef<'_>>,
        > = IndexMap::new();
        for (name, raw) in raw_schemas {
            let mut parents = Vec::with_capacity(raw.extends.len());
            let mut seen_targets = IndexSet::new();
            for target in &raw.extends {
                if !raw_schemas.contains_key(target) {
                    warnings.push(SchemaWarning::MissingExtendsTarget {
                        schema: name.clone(),
                        target: target.clone(),
                    });
                    continue;
                }
                if !seen_targets.insert(target.as_str()) {
                    warnings.push(SchemaWarning::DuplicateExtendsTarget {
                        schema: name.clone(),
                        target: target.clone(),
                    });
                    continue;
                }
                parents.push(target.as_ref());
            }
            parents_by_name.insert(name.as_ref(), parents);
        }

        // An edge runs parent -> child, so a child's in-degree is its
        // (filtered) parent count.
        let mut in_degree: HashMap<SchemaNameRef<'_>, usize> = HashMap::new();
        let mut children_by_name: IndexMap<
            SchemaNameRef<'_>,
            Vec<SchemaNameRef<'_>>,
        > = IndexMap::new();
        for (&name, parents) in &parents_by_name {
            in_degree.insert(name, parents.len());
            for &parent in parents {
                children_by_name.entry(parent).or_default().push(name);
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

        (
            Self {
                parents_by_name,
                children_by_name,
                in_degree,
                queue,
                visited: IndexSet::new(),
                _marker: PhantomData,
            },
            warnings,
        )
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
            self.children_by_name.get(name.as_str()).into_iter().flatten()
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
    /// member), or `None` if every Schema was visited.
    fn cyclic_remainder(&self) -> Option<Vec<SchemaName>> {
        if self.visited.len() == self.parents_by_name.len() {
            return None;
        }
        Some(
            self.parents_by_name
                .keys()
                .filter(|name| !self.visited.contains(name.as_str()))
                .cloned()
                .collect(),
        )
    }

    /// Consume the building graph, returning a resolved graph if the DAG is
    /// acyclic, or the cyclic schemas if a cycle exists.
    pub(super) fn into_resolved(
        self,
    ) -> Result<SchemaGraph<'a, Resolved>, Vec<SchemaName>> {
        if let Some(schemas) = self.cyclic_remainder() {
            return Err(schemas);
        }
        Ok(self.transition_to())
    }
}

impl<'a> SchemaGraph<'a, Resolved> {
    /// Return every Schema's direct `extends` children, keyed by parent name.
    ///
    /// Excludes the Global Schema as a parent: it is a flat reference pool,
    /// never a real link in the `extends` chain. A Schema that (unusually)
    /// declares `extends = ["global"]` still contributes no entry here.
    #[must_use]
    pub(super) fn children_by_name(
        &self,
    ) -> IndexMap<SchemaName, IndexSet<SchemaName>> {
        let mut children: IndexMap<SchemaName, IndexSet<SchemaName>> =
            IndexMap::new();
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
    /// set is computed once (graph edges are never re-walked), but every
    /// ancestor that reaches a shared descendant still copies that
    /// descendant's already-materialized set into its own, since each entry
    /// in the returned map must be an independently owned set. Total work is
    /// `O(V + E + Σ|descendants(v)|)` — `O(V + E)` graph traversal plus the
    /// unavoidable cost of materializing every entry, which degrades to
    /// `O(V²)` in the worst case (a single deep `extends` chain). Not
    /// measured to matter in practice given realistic, human-authored
    /// Schema counts.
    #[must_use]
    pub(super) fn descendants_by_name(
        &self,
    ) -> IndexMap<SchemaName, IndexSet<SchemaName>> {
        let children = self.children_by_name();
        let mut memo: IndexMap<SchemaName, IndexSet<SchemaName>> =
            IndexMap::new();
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
        children: &IndexMap<SchemaName, IndexSet<SchemaName>>,
        memo: &mut IndexMap<SchemaName, IndexSet<SchemaName>>,
    ) -> IndexSet<SchemaName> {
        if let Some(cached) = memo.get(name) {
            return cached.clone();
        }
        let mut result = IndexSet::new();
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
            let mut raw = IndexMap::new();
            raw.insert(SchemaName::from(GLOBAL_SCHEMA_NAME), schema(&[]));
            raw.insert(SchemaName::from("author"), schema(&[]));
            let (mut graph, _warnings) = SchemaGraph::new(&raw);

            assert_eq!(
                graph.next_ready(),
                Some(SchemaNameRef::from(GLOBAL_SCHEMA_NAME))
            );
            assert_eq!(graph.next_ready(), Some(SchemaNameRef::from("author")));
            assert_eq!(graph.next_ready(), None);
        }
    }

    mod new {
        use pretty_assertions::assert_eq;

        use super::*;

        #[test]
        fn a_duplicate_extends_target_warns_once_and_parents_once() {
            let mut raw = IndexMap::new();
            raw.insert(SchemaName::from("book"), schema(&[]));
            raw.insert(SchemaName::from("child"), schema(&["book", "book"]));
            let (graph, warnings) = SchemaGraph::new(&raw);

            assert_eq!(warnings, vec![SchemaWarning::DuplicateExtendsTarget {
                schema: SchemaName::from("child"),
                target: SchemaName::from("book"),
            }]);
            assert_eq!(graph.parents_of(SchemaNameRef::from("child")), &[
                SchemaNameRef::from("book")
            ]);
        }
    }

    mod children_by_name {
        use pretty_assertions::assert_eq;

        use super::*;

        fn set(names: &[&str]) -> IndexSet<SchemaName> {
            names.iter().map(|&name| SchemaName::from(name)).collect()
        }

        #[test]
        fn returns_only_direct_extenders_for_a_branching_tree() {
            // thing <- book <- {sci_fi, memoir}
            let mut raw = IndexMap::new();
            raw.insert(SchemaName::from("thing"), schema(&[]));
            raw.insert(SchemaName::from("book"), schema(&["thing"]));
            raw.insert(SchemaName::from("sci_fi"), schema(&["book"]));
            raw.insert(SchemaName::from("memoir"), schema(&["book"]));
            let graph = SchemaGraph::new(&raw).0.into_resolved().unwrap();
            let children = graph.children_by_name();

            assert_eq!(children.get("thing"), Some(&set(&["book"])));
            assert_eq!(children.get("book"), Some(&set(&["memoir", "sci_fi"])));
            assert_eq!(children.get("sci_fi"), None);
            assert_eq!(children.get("memoir"), None);
        }

        #[test]
        fn a_multi_parent_schema_appears_in_every_parents_direct_children() {
            // thing <- {book, film} <- adaptation (both parents): the
            // genuine diamond shape, distinct from the branching-tree fixture
            // above -- a node with two parents converging, not one parent
            // fanning out to two children.
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

            assert_eq!(children.get("book"), Some(&set(&["adaptation"])));
            assert_eq!(children.get("film"), Some(&set(&["adaptation"])));
            assert_eq!(children.get("thing"), Some(&set(&["book", "film"])));
        }

        #[test]
        fn excludes_the_global_schema_as_a_parent() {
            let mut raw = IndexMap::new();
            raw.insert(SchemaName::from(GLOBAL_SCHEMA_NAME), schema(&[]));
            raw.insert(SchemaName::from("book"), schema(&[GLOBAL_SCHEMA_NAME]));
            let graph = SchemaGraph::new(&raw).0.into_resolved().unwrap();
            let children = graph.children_by_name();

            assert_eq!(children.get(GLOBAL_SCHEMA_NAME), None);
        }
    }

    mod descendants_by_name {
        use pretty_assertions::assert_eq;

        use super::*;

        fn set(names: &[&str]) -> IndexSet<SchemaName> {
            names.iter().map(|&name| SchemaName::from(name)).collect()
        }

        #[test]
        fn deduplicates_a_diamond_dags_shared_descendant() {
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
        fn returns_the_full_transitive_closure_through_a_three_level_chain() {
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
        fn returns_no_entry_for_a_leaf_schema() {
            let mut raw = IndexMap::new();
            raw.insert(SchemaName::from("book"), schema(&[]));
            raw.insert(SchemaName::from("sci_fi"), schema(&["book"]));
            let graph = SchemaGraph::new(&raw).0.into_resolved().unwrap();
            let descendants = graph.descendants_by_name();

            assert_eq!(descendants.get("sci_fi"), None);
        }
    }
}
