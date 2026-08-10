//! Linearize the `extends` DAG into a resolution order with Kahn's topological
//! sort.
//!
//! [`SchemaGraph`] owns the DAG bookkeeping — parent/child adjacency, Kahn
//! in-degrees, and the Global-first tie-break — so [`super::resolve`] can drive
//! resolution order without tangling graph mechanics into its field-merge
//! logic.
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
//! [`next_ready`]: SchemaGraph::next_ready
//! [`parents_of`]: SchemaGraph::parents_of
//! [`mark_resolved`]: SchemaGraph::mark_resolved
//! [`cyclic_remainder`]: SchemaGraph::cyclic_remainder

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
/// [`super::resolve::build_schema`].
pub(super) struct SchemaGraph<'a> {
    /// Each Schema's `extends` parents, filtered to present targets and kept
    /// in declaration order; the Global Schema's list is force-emptied so it
    /// never inherits. Backs [`parents_of`](Self::parents_of).
    parents_by_name: BTreeMap<SchemaNameRef<'a>, Vec<SchemaNameRef<'a>>>,
    /// Reverse `extends` adjacency (parent to children), used by
    /// [`mark_resolved`](Self::mark_resolved) to decrement children's
    /// in-degrees.
    children_by_name: BTreeMap<SchemaNameRef<'a>, Vec<SchemaNameRef<'a>>>,
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
        let mut children_by_name: BTreeMap<
            SchemaNameRef<'_>,
            Vec<SchemaNameRef<'_>>,
        > = BTreeMap::new();
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

        Self {
            parents_by_name,
            children_by_name,
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
}
