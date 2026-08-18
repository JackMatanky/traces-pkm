# Schema Graph Deepen Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Deepen `schema/graph.rs` from a shallow protocol into a deep, safe, scalable module via IndexMap/IndexSet migration, typestate pattern, field-merge decomposition, and SchemaResolver.

**Architecture:** Five phases, each independently testable and committable. Phase 1 migrates leaf types to IndexMap/IndexSet (no behavioral change). Phase 2 adds typestate to SchemaGraph. Phase 3 extracts resolution logic into `resolver.rs` (field merging + SchemaResolver). Phase 4 migrates SchemaService to IndexMap. Phase 5 stores `children_by_name` correctly (stop re-inverting) and optimizes `descendants_by_name` with bit-vec bitsets (O(V²/w) DFS).

**Tech Stack:** Rust, indexmap crate (2.14), existing test infrastructure (1607 tests), mise tasks for build/lint/test.

**Working directory:** `.worktrees/07-schema-service-refactor` (branch: `issue-07-schema-service-refactor`)

**Starting state:** All 1607 tests pass. Clippy clean. `bit-vec` not yet added.

---

## File Structure

### Files to create
| File                              | Responsibility                                      |
| --------------------------------- | --------------------------------------------------- |
| `src/schema/resolver.rs`            | Resolution logic: field merging, SchemaResolver, hierarchy filtering |
| `docs/superpowers/plans/2026-08-18-schema-graph-deepen.md` | This plan                                |

### Files to modify
| File                              | Changes                                             |
| --------------------------------- | --------------------------------------------------- |
| `Cargo.toml`                        | Add `indexmap` and `bit-vec` dependencies              |
| `src/schema/mod.rs`                 | Add `mod resolver;` declaration                        |
| `src/schema/graph.rs`               | IndexMap/IndexSet internals, typestate (Building/Resolved) |
| `src/schema/service.rs`             | IndexMap/IndexSet throughout, use resolver::SchemaResolver |
| `src/schema/model.rs`               | IndexMap/IndexSet fields, method signatures           |
| `src/schema/raw.rs`                 | IndexMap for RawSchema fields/options                |
| `src/schema/fields/builder.rs`      | IndexMap/IndexSet in RefAddressResolver, parse_options |
| `src/schema/fields/parser.rs`       | IndexMap for options parameters                       |
| `src/schema/fields/select.rs`       | IndexMap for extra field, parse param                 |
| `src/schema/fields/number.rs`       | IndexMap for parse param                              |
| `src/schema/fields/date.rs`         | IndexMap for parse param                              |
| `src/schema/fields/file.rs`         | IndexMap for parse param                              |
| `src/template/engine/schema.rs`     | IndexMap in select_entry_value()                      |

### Files unchanged
| File                              | Why unchanged                                       |
| --------------------------------- | --------------------------------------------------- |
| `src/schema/name.rs`                | Already derives Hash+Ord, no collections             |
| `src/schema/error.rs`               | No collection types in error variants                |
| `src/schema/fields.rs`              | Module root, just re-exports                         |
| `src/schema/fields/address.rs`      | No collections                                       |
| `src/schema/fields/error.rs`        | No collections                                       |
| External consumers (cli, template)  | Use Arc<SchemaService>, not map types                |

---

## Task 1: Add indexmap dependency

**Files:**
- Modify: `Cargo.toml`

- [ ] **Step 1: Add indexmap to dependencies**

```toml
# In Cargo.toml [dependencies] section, add:
indexmap = { version = "2.14", features = ["serde"] }
```

The `serde` feature is required because `RawSchema` and its fields derive `Deserialize` and currently use `BTreeMap` (which serde supports natively). `IndexMap` needs the serde feature for the same support.

- [ ] **Step 2: Verify it compiles**

Run: `mise run check`
Expected: Compiles successfully (no code changes yet, just dependency addition)

- [ ] **Step 3: Commit**

```bash
git add Cargo.toml Cargo.lock
git commit -m "chore: add indexmap 2.14 dependency with serde feature"
```

---

## Task 2: Migrate RawSchema to IndexMap

**Files:**
- Modify: `src/schema/raw.rs` (lines 11, 33, 47, 62, 78, 114)

This is a leaf change — RawSchema is parsed from TOML and consumed by service.rs. Changing it first means all downstream consumers see the new type.

- [ ] **Step 1: Update imports in raw.rs**

Change line 11 from:
```rust
use std::collections::BTreeMap;
```
to:
```rust
use indexmap::IndexMap;
```

- [ ] **Step 2: Change RawSchema.fields type**

Change line 33 from:
```rust
pub(crate) fields: BTreeMap<FieldName, RawSchemaFieldDef>,
```
to:
```rust
pub(crate) fields: IndexMap<FieldName, RawSchemaFieldDef>,
```

- [ ] **Step 3: Change RawSchemaFieldDef.options type**

Change line 47 from:
```rust
pub(crate) options: BTreeMap<String, FieldValue>,
```
to:
```rust
pub(crate) options: IndexMap<String, FieldValue>,
```

- [ ] **Step 4: Update test helpers**

Change lines 62 and 78 from `BTreeMap::new()` to `IndexMap::new()`.

Change line 114 from:
```rust
let mut options = BTreeMap::new();
```
to:
```rust
let mut options = IndexMap::new();
```

- [ ] **Step 5: Run tests to verify compilation**

Run: `mise run check`
Expected: May have compilation errors in service.rs and builder.rs where RawSchema is consumed — those will be fixed in later tasks. For now, just verify raw.rs itself compiles.

Actually — since RawSchema is used throughout, this will cause cascading compile errors. We need to fix all consumers in one go. Let's do that now.

- [ ] **Step 6: Fix all imports that use RawSchema's fields**

The following files import and use `RawSchema.fields` or `RawSchemaFieldDef.options`:

In `src/schema/service.rs`:
- Line 33: `use std::collections::BTreeMap;` — add `use indexmap::IndexMap;`
- Line 544: `fields: &BTreeMap<FieldName, super::fields::SchemaFieldDef>` — this is `build_schema`'s local, not RawSchema. Leave for now.
- The `build_schema` function at line 518 iterates `raw.fields` — the iteration works identically on IndexMap.

In `src/schema/fields/builder.rs`:
- Line 3: `use std::collections::{BTreeMap, BTreeSet};` — add `use indexmap::IndexMap;`
- Line 309: `options: &BTreeMap<String, FieldValue>` in `parse_options` — change to `&IndexMap<String, FieldValue>`

In `src/schema/fields/parser.rs`:
- Line 3: `use std::collections::{BTreeMap, BTreeSet};` — add `use indexmap::IndexMap;`
- Lines 110, 114, 180: `options: &BTreeMap<String, FieldValue>` — change to `&IndexMap<String, FieldValue>`
- Line 224: `fn options(pairs: &[(&str, FieldValue)]) -> BTreeMap<String, FieldValue>` — change return type

In `src/schema/fields/select.rs`:
- Line 3: `use std::collections::BTreeMap;` — change to `use indexmap::IndexMap;`
- Line 40: `options: &BTreeMap<String, FieldValue>` — change to `&IndexMap<String, FieldValue>`
- Line 64: `extra: BTreeMap<String, FieldValue>` — change to `IndexMap<String, FieldValue>`
- Line 73: `extra: BTreeMap::new()` — change to `IndexMap::new()`
- Line 94: `pub(crate) fn extra(&self) -> &BTreeMap<String, FieldValue>` — change return type
- Line 101, 115, 207, 210: test helpers

In `src/schema/fields/number.rs`:
- Line 3: `use std::collections::BTreeMap;` — change to `use indexmap::IndexMap;`
- Line 44: `options: &BTreeMap<String, FieldValue>` — change to `&IndexMap<String, FieldValue>`
- Line 59, 71, 101: test helpers

In `src/schema/fields/date.rs`:
- Line 3: `use std::collections::BTreeMap;` — change to `use indexmap::IndexMap;`
- Line 44: `options: &BTreeMap<String, FieldValue>` — change to `&IndexMap<String, FieldValue>`
- Line 59, 71, 105, 106: test helpers

In `src/schema/fields/file.rs`:
- Line 12: `use std::collections::BTreeMap;` — change to `use indexmap::IndexMap;`
- Line 85: `options: &BTreeMap<String, FieldValue>` — change to `&IndexMap<String, FieldValue>`
- Line 117, 132: test helpers

In `src/template/engine/schema.rs`:
- Line 58: `use std::collections::BTreeMap;` — change to `use indexmap::IndexMap;`
- Line 256: `let mut object: BTreeMap<String, FieldValue> = entry.extra().clone();` — change to `IndexMap`

- [ ] **Step 7: Run clippy and tests**

Run: `cargo clippy --workspace --all-targets --all-features -- -D warnings && mise run test`
Expected: All 1607 tests pass. Clippy clean.

- [ ] **Step 8: Commit**

```bash
git add -A
git commit -m "refactor(schema): migrate RawSchema fields/options to IndexMap

Preserves TOML declaration order for field iteration in templates."
```

---

## Task 3: Migrate Schema model to IndexMap/IndexSet

**Files:**
- Modify: `src/schema/model.rs` (lines 3, 13, 15, 17, 19, 31, 32, 38, 39, 46, 47, 63, 78, 85, 92)

This changes the public interface of Schema. All consumers that access `fields()`, `ancestors()`, `children()`, `descendants()` will see the new types.

- [ ] **Step 1: Update imports in model.rs**

Change line 3 from:
```rust
use std::collections::{BTreeMap, BTreeSet};
```
to:
```rust
use indexmap::{IndexMap, IndexSet};
```

- [ ] **Step 2: Change Schema struct fields**

Change lines 13-19 from:
```rust
fields: BTreeMap<FieldName, SchemaFieldDef>,
ancestors: BTreeSet<SchemaName>,
children: BTreeSet<SchemaName>,
descendants: BTreeSet<SchemaName>,
```
to:
```rust
fields: IndexMap<FieldName, SchemaFieldDef>,
ancestors: IndexSet<SchemaName>,
children: IndexSet<SchemaName>,
descendants: IndexSet<SchemaName>,
```

- [ ] **Step 3: Change Schema::new parameters**

Change lines 31-32 from:
```rust
fields: BTreeMap<FieldName, SchemaFieldDef>,
ancestors: BTreeSet<SchemaName>,
```
to:
```rust
fields: IndexMap<FieldName, SchemaFieldDef>,
ancestors: IndexSet<SchemaName>,
```

- [ ] **Step 4: Change set_hierarchy parameters**

Change lines 46-47 from:
```rust
children: BTreeSet<SchemaName>,
descendants: BTreeSet<SchemaName>,
```
to:
```rust
children: IndexSet<SchemaName>,
descendants: IndexSet<SchemaName>,
```

- [ ] **Step 5: Change accessor return types**

Change line 63 from:
```rust
pub(crate) fn fields(&self) -> &BTreeMap<FieldName, SchemaFieldDef> {
```
to:
```rust
pub(crate) fn fields(&self) -> &IndexMap<FieldName, SchemaFieldDef> {
```

Change line 78 from:
```rust
pub(super) fn ancestors(&self) -> &BTreeSet<SchemaName> {
```
to:
```rust
pub(super) fn ancestors(&self) -> &IndexSet<SchemaName> {
```

Change line 85 from:
```rust
pub(super) fn children(&self) -> &BTreeSet<SchemaName> {
```
to:
```rust
pub(super) fn children(&self) -> &IndexSet<SchemaName> {
```

Change line 92 from:
```rust
pub(super) fn descendants(&self) -> &BTreeSet<SchemaName> {
```
to:
```rust
pub(super) fn descendants(&self) -> &IndexSet<SchemaName> {
```

- [ ] **Step 6: Update test code in model.rs**

All test helpers constructing `BTreeMap::new()` and `BTreeSet::new()` need to change to `IndexMap::new()` and `IndexSet::new()`. The test at line 154 imports `use std::collections::{BTreeMap, BTreeSet};` — change to `use indexmap::{IndexMap, IndexSet};`.

Specific changes in tests:
- Line 167: `let mut fields = BTreeMap::new();` → `IndexMap::new()`
- Line 172: `let mut ancestors = BTreeSet::new();` → `IndexSet::new()`
- Lines 192-193: `BTreeMap::new()`, `BTreeSet::new()` → `IndexMap::new()`, `IndexSet::new()`
- Lines 195, 197: `BTreeSet::new()` → `IndexSet::new()`
- Line 209: `BTreeMap::new()` → `IndexMap::new()`
- Lines 227-228, 238-239, 251, 262-263: same pattern
- Line 271: test import change
- Lines 292-293: collect to `IndexMap` and `IndexSet`

- [ ] **Step 7: Fix consumers in service.rs**

In `src/schema/service.rs`:
- Line 501: `ancestors.extend(parent_schema.ancestors().iter().cloned());` — works unchanged (IndexSet has same iterator)
- Line 524: `fields.extend(own_fields);` — works unchanged (IndexMap has same extend)
- Line 526: `reject_ambiguous_canonical_names(name, &fields)?;` — parameter type needs update
- Line 544: `fields: &BTreeMap<FieldName, super::fields::SchemaFieldDef>` — change to `&IndexMap<FieldName, super::fields::SchemaFieldDef>`
- Line 546: `let mut seen: BTreeMap<String, FieldName> = BTreeMap::new();` — change to `HashMap` (pure lookup, no ordering needed)
- Line 426: `resolved_ancestors: BTreeMap<SchemaName, BTreeSet<SchemaName>>` — change to `IndexMap<SchemaName, IndexSet<SchemaName>>`
- Line 424: iterate `resolved` (now IndexMap) — works unchanged

- [ ] **Step 8: Fix consumers in fields/builder.rs**

In `src/schema/fields/builder.rs`:
- Line 3: `use std::collections::{BTreeMap, BTreeSet};` — change to `use indexmap::{IndexMap, IndexSet}; use std::collections::HashMap;` (if needed for other uses)
- Line 309: `ancestors: &'a BTreeSet<SchemaName>` — change to `&'a IndexSet<SchemaName>`
- Line 315: `ancestors: &ancestors,` — works unchanged
- Line 329: `ancestors: &BTreeSet::new()` — change to `&IndexSet::new()`

- [ ] **Step 9: Run clippy and tests**

Run: `cargo clippy --workspace --all-targets --all-features -- -D warnings && mise run test`
Expected: All 1607 tests pass. Clippy clean.

- [ ] **Step 10: Commit**

```bash
git add -A
git commit -m "refactor(schema): migrate Schema model to IndexMap/IndexSet

- fields: IndexMap (definition order)
- ancestors/children/descendants: IndexSet (definition order)
- All accessor return types updated
- All consumer call sites updated"
```

---

## Task 4: Migrate graph.rs internals to IndexMap/HashMap/IndexSet

**Files:**
- Modify: `src/schema/graph.rs` (lines 25, 36-44, 53, 55, 59-62, 88-92, 107-111, 125, 167, 193-195, 229-232, 248-250, 254)

This changes graph.rs internal data structures and return types. The typestate pattern is added in Task 5.

- [ ] **Step 1: Update imports in graph.rs**

Change line 25 from:
```rust
use std::collections::{BTreeMap, BTreeSet, VecDeque};
```
to:
```rust
use std::collections::{HashMap, VecDeque};
use indexmap::{IndexMap, IndexSet};
```

- [ ] **Step 2: Change SchemaGraph struct fields**

Change lines 36-44 from:
```rust
parents_by_name: BTreeMap<SchemaNameRef<'a>, Vec<SchemaNameRef<'a>>>,
children_by_name: BTreeMap<SchemaNameRef<'a>, Vec<SchemaNameRef<'a>>>,
in_degree: BTreeMap<SchemaNameRef<'a>, usize>,
queue: VecDeque<SchemaNameRef<'a>>,
visited: BTreeSet<SchemaNameRef<'a>>,
```
to:
```rust
parents_by_name: IndexMap<SchemaNameRef<'a>, Vec<SchemaNameRef<'a>>>,
children_by_name: IndexMap<SchemaNameRef<'a>, Vec<SchemaNameRef<'a>>>,
in_degree: HashMap<SchemaNameRef<'a>, usize>,
queue: VecDeque<SchemaNameRef<'a>>,
visited: IndexSet<SchemaNameRef<'a>>,
```

- [ ] **Step 3: Change new() parameter type**

Change line 53 from:
```rust
raw_schemas: &'a BTreeMap<SchemaName, RawSchema>,
```
to:
```rust
raw_schemas: &'a IndexMap<SchemaName, RawSchema>,
```

- [ ] **Step 4: Update new() body**

Change line 59 from:
```rust
let mut parents_by_name: BTreeMap<SchemaNameRef<'_>, Vec<SchemaNameRef<'_>>> = BTreeMap::new();
```
to:
```rust
let mut parents_by_name: IndexMap<SchemaNameRef<'_>, Vec<SchemaNameRef<'_>>> = IndexMap::new();
```

Change line 65 from:
```rust
let mut seen_targets = BTreeSet::new();
```
to:
```rust
let mut seen_targets = IndexSet::new();
```

Change line 88 from:
```rust
let mut in_degree: BTreeMap<SchemaNameRef<'_>, usize> = BTreeMap::new();
```
to:
```rust
let mut in_degree: HashMap<SchemaNameRef<'_>, usize> = HashMap::new();
```

Change line 89 from:
```rust
let mut children_by_name: BTreeMap<SchemaNameRef<'_>, Vec<SchemaNameRef<'_>>> = BTreeMap::new();
```
to:
```rust
let mut children_by_name: IndexMap<SchemaNameRef<'_>, Vec<SchemaNameRef<'_>>> = IndexMap::new();
```

Change line 125 from:
```rust
visited: BTreeSet::new(),
```
to:
```rust
visited: IndexSet::new(),
```

- [ ] **Step 5: Change cyclic_remainder parameter type**

Change line 167 from:
```rust
raw_schemas: &BTreeMap<SchemaName, RawSchema>,
```
to:
```rust
raw_schemas: &IndexMap<SchemaName, RawSchema>,
```

- [ ] **Step 6: Change children_by_name return type**

Change lines 193-195 from:
```rust
pub(super) fn children_by_name(
    &self,
) -> BTreeMap<SchemaName, BTreeSet<SchemaName>> {
    let mut children: BTreeMap<SchemaName, BTreeSet<SchemaName>> = BTreeMap::new();
```
to:
```rust
pub(super) fn children_by_name(
    &self,
) -> IndexMap<SchemaName, IndexSet<SchemaName>> {
    let mut children: IndexMap<SchemaName, IndexSet<SchemaName>> = IndexMap::new();
```

- [ ] **Step 7: Change descendants_by_name return type and internals**

Change lines 229-232 from:
```rust
pub(super) fn descendants_by_name(
    &self,
) -> BTreeMap<SchemaName, BTreeSet<SchemaName>> {
    let children = self.children_by_name();
    let mut memo: BTreeMap<SchemaName, BTreeSet<SchemaName>> = BTreeMap::new();
```
to:
```rust
pub(super) fn descendants_by_name(
    &self,
) -> IndexMap<SchemaName, IndexSet<SchemaName>> {
    let children = self.children_by_name();
    let mut memo: IndexMap<SchemaName, IndexSet<SchemaName>> = IndexMap::new();
```

Change lines 248-250 (descendants_of helper) from:
```rust
children: &BTreeMap<SchemaName, BTreeSet<SchemaName>>,
memo: &mut BTreeMap<SchemaName, BTreeSet<SchemaName>>,
) -> BTreeSet<SchemaName> {
```
to:
```rust
children: &IndexMap<SchemaName, IndexSet<SchemaName>>,
memo: &mut IndexMap<SchemaName, IndexSet<SchemaName>>,
) -> IndexSet<SchemaName> {
```

Change line 254 from:
```rust
let mut result = BTreeSet::new();
```
to:
```rust
let mut result = IndexSet::new();
```

- [ ] **Step 8: Update test code in graph.rs**

All test helpers constructing `BTreeMap::new()` and `BTreeSet::new()` need to change. The test module at line 267 uses `super::*` which now includes IndexMap/IndexSet.

Specific changes:
- Line 288: `let mut raw = BTreeMap::new();` → `IndexMap::new()`
- Line 309: same
- Line 329: `fn set(names: &[&str]) -> BTreeSet<SchemaName>` → `IndexSet<SchemaName>`
- Line 330: `names.iter().map(|&name| SchemaName::from(name)).collect()` — works unchanged
- Lines 336, 355, 372, 386, 393, 411, 432: all `BTreeMap::new()` → `IndexMap::new()`

- [ ] **Step 9: Run clippy and tests**

Run: `cargo clippy --workspace --all-targets --all-features -- -D warnings && mise run test`
Expected: All 1607 tests pass. Clippy clean.

- [ ] **Step 10: Commit**

```bash
git add -A
git commit -m "refactor(schema): migrate graph.rs internals to IndexMap/HashMap/IndexSet

- parents_by_name, children_by_name: IndexMap (insertion order)
- in_degree: HashMap (pure lookup, no ordering needed)
- visited: IndexSet
- Return types: IndexMap<SchemaName, IndexSet<SchemaName>>
- Enables stable indices for future bitset optimization"
```

---

## Task 5: Add typestate to SchemaGraph

**Files:**
- Modify: `src/schema/graph.rs` (add state types, generic parameter, impl blocks, transition methods)

This adds compile-time enforcement of the resolution protocol. No behavioral change — same logic, safer interface.

- [ ] **Step 1: Add state types at the top of graph.rs (after imports)**

Add after line 30:
```rust
/// Building state: resolution in progress, queue and in_degree active.
pub(super) struct Building;

/// Resolved state: DAG is acyclic, hierarchy queries available.
pub(super) struct Resolved;
```

- [ ] **Step 2: Add PhantomData import**

Change line 25 to also import `PhantomData`:
```rust
use std::collections::{HashMap, VecDeque};
use std::marker::PhantomData;
use indexmap::{IndexMap, IndexSet};
```

- [ ] **Step 3: Make SchemaGraph generic over state**

Change lines 32-45 from:
```rust
/// Kahn's-algorithm state for linearizing the `extends` DAG.
pub(super) struct SchemaGraph<'a> {
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
}
```
to:
```rust
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
```

- [ ] **Step 4: Add transition_to helper**

Add inside `impl<'a, State> SchemaGraph<'a, State>` (new block):
```rust
impl<'a, State> SchemaGraph<'a, State> {
    /// Moves the graph into the next lifecycle state.
    fn transition_to<NextState>(
        self,
    ) -> SchemaGraph<'a, NextState> {
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
```

- [ ] **Step 5: Change impl block from `impl<'a> SchemaGraph<'a>` to `impl<'a> SchemaGraph<'a, Building>`**

Change line 47 from:
```rust
impl<'a> SchemaGraph<'a> {
```
to:
```rust
impl<'a> SchemaGraph<'a, Building> {
```

- [ ] **Step 6: Add `_marker: PhantomData` to the struct construction in new()**

Change lines 119-128 from:
```rust
(
    Self {
        parents_by_name,
        children_by_name,
        in_degree,
        queue,
        visited: IndexSet::new(),
    },
    warnings,
)
```
to:
```rust
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
```

- [ ] **Step 7: Simplify and make cyclic_remainder private**

The `raw_schemas` parameter is redundant. `SchemaGraph::new` builds `parents_by_name` by iterating `raw_schemas.keys()` (`graph.rs:63-83`), so `parents_by_name.keys()` is identical to `raw_schemas.keys()` and `parents_by_name.len()` equals `raw_schemas.len()`. This means `cyclic_remainder` can derive everything from internal state without the external parameter. The original design kept `raw_schemas` for explicitness (visual signal that both `new()` and `into_resolved()` operate on the same dataset), but the redundancy adds a parameter that carries no new information.

Drop the parameter, make the method private (it's now internal to `into_resolved`):

Change from:
```rust
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
```
to:
```rust
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
```

- [ ] **Step 8: Add into_resolved fallible transition**

`into_resolved` only delegates to `cyclic_remainder` (which now takes no parameters per Step 7), so it also drops `raw_schemas`. The caller's responsibility is unchanged: pass the same `raw_schemas` to `SchemaGraph::new()` — the graph captures everything it needs at construction time.

Add inside `impl<'a> SchemaGraph<'a, Building>` (after `cyclic_remainder`):
```rust
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
```

- [ ] **Step 9: Add Resolved impl block**

Add after the Building impl block:
```rust
impl<'a> SchemaGraph<'a, Resolved> {
    /// Return every Schema's direct `extends` children, keyed by parent name.
    ///
    /// Excludes the Global Schema as a parent: it is a flat reference pool,
    /// never a real link in the `extends` chain.
    #[must_use]
    pub(super) fn children_by_name(
        &self,
    ) -> IndexMap<SchemaName, IndexSet<SchemaName>> {
        // ... (move existing children_by_name implementation here)
    }

    /// Return every Schema's transitive `extends` descendants, keyed by
    /// ancestor name.
    ///
    /// Computed as a memoized depth-first walk over
    /// [`children_by_name`](Self::children_by_name).
    #[must_use]
    pub(super) fn descendants_by_name(
        &self,
    ) -> IndexMap<SchemaName, IndexSet<SchemaName>> {
        // ... (move existing descendants_by_name implementation here)
    }
}
```

Move the existing `children_by_name()` and `descendants_by_name()` method bodies from the Building impl block to this new Resolved impl block.

- [ ] **Step 10: Update resolve_all in service.rs to use typestate**

Step 7 already simplified `cyclic_remainder` and made it private. Now update `resolve_all` in service.rs to use `into_resolved()`:

```rust
// Before (in resolve_all):
let (mut graph, mut warnings) = SchemaGraph::new(raw_schemas);
// ... loop ...
if let Some(schemas) = graph.cyclic_remainder(raw_schemas) {
    return Err(SchemaError::Cycle { schemas });
}
let children_by_name = graph.children_by_name();
let descendants_by_name = graph.descendants_by_name();

// After:
let (mut graph, mut warnings) = SchemaGraph::new(raw_schemas);
// ... loop ...
let graph = graph.into_resolved().map_err(|schemas| {
    SchemaError::Cycle { schemas }
})?;
let children_by_name = graph.children_by_name();
let descendants_by_name = graph.descendants_by_name();
```

Remove the `#[expect(clippy::expect_used, ...)]` annotation that was guarding the `cyclic_remainder` call — `into_resolved` handles the check internally.

- [ ] **Step 11: Run clippy and tests**

Run: `cargo clippy --workspace --all-targets --all-features -- -D warnings && mise run test`
Expected: All 1607 tests pass. Clippy clean. Key verification: `graph.children_by_name()` before `into_resolved()` would be a compile error.

- [ ] **Step 12: Commit**

```bash
git add -A
git commit -m "refactor(schema): add typestate to SchemaGraph (Building/Resolved)

- Building state: next_ready/parents_of/mark_resolved available
- Resolved state: children_by_name/descendants_by_name available
- into_resolved() fallible transition encapsulates cycle check
- Compile-time enforcement of resolution protocol"
```

---

## Task 6: Create resolver.rs

**Files:**
- Create: `src/schema/resolver.rs`
- Modify: `src/schema/mod.rs` (add `mod resolver;`)
- Modify: `src/schema/service.rs` (remove `build_schema`, `reject_ambiguous_canonical_names`, `resolve_all`; use `resolver::SchemaResolver`)

This extracts all resolution logic into a cohesive `resolver.rs` module: field merging (`merge_fields`), the `SchemaResolver` orchestrator, and hierarchy filtering. `service.rs` becomes a thin facade.

- [ ] **Step 1: Create resolver.rs with merge_fields**

Create `src/schema/resolver.rs`:
```rust
//! Schema resolution: linearize the `extends` DAG, merge inherited fields,
//! and compute hierarchy sets.
//!
//! [`SchemaResolver`] is the single-method entry point. Internally it drives
//! [`SchemaGraph`]'s typestate protocol, delegates per-Schema field merging to
//! [`merge_fields`], and filters hierarchy sets against resolution failures.

use std::collections::HashMap;

use indexmap::{IndexMap, IndexSet};

use super::{
    GLOBAL_SCHEMA_NAME, RawSchema, SchemaName, SchemaNameRef,
    error::{SchemaError, SchemaWarning},
    fields::{FieldAddressRef, RefAddressResolver, SchemaFieldBuilder},
    graph::SchemaGraph,
    model::Schema,
};
use crate::field::FieldName;

/// One Schema whose own fields failed to build during resolution, alongside the
/// [`SchemaError`] it failed with.
///
/// Excluded from the resolved Schemas; any Schema naming it as a parent
/// inherits none of its fields (see
/// [`SchemaWarning::ParentFailedToResolve`]).
#[derive(Debug)]
pub(super) struct SchemaFailure {
    pub(super) schema: SchemaName,
    pub(super) error: SchemaError,
}

/// Output of schema resolution: resolved Schemas, any warnings from degraded
/// resolution, and any per-Schema failures.
pub(super) struct ResolvedSchemas {
    pub(super) schemas: IndexMap<SchemaName, Schema>,
    pub(super) warnings: Vec<SchemaWarning>,
    pub(super) failures: Vec<SchemaFailure>,
}

/// Resolves all raw Schemas into a complete, hierarchy-aware registry.
///
/// Consumes the raw TOML declarations and produces a flat map of resolved
/// [`Schema`] objects with inherited fields and precomputed hierarchy sets.
///
/// # Errors
///
/// - [`SchemaError::Cycle`] if the `extends` DAG contains a cycle.
/// - [`SchemaError::ReadDirectory`], [`ReadFile`], [`Parse`] for I/O and TOML errors.
///
/// [`ReadDirectory`]: SchemaError::ReadDirectory
/// [`ReadFile`]: SchemaError::ReadFile
/// [`Parse`]: SchemaError::Parse
pub(super) struct SchemaResolver<'a> {
    raw: &'a IndexMap<SchemaName, RawSchema>,
}

impl<'a> SchemaResolver<'a> {
    /// Create a resolver for the given raw schemas.
    pub(super) fn new(raw: &'a IndexMap<SchemaName, RawSchema>) -> Self {
        Self { raw }
    }

    /// Resolve all schemas, returning the complete registry.
    ///
    /// Linearizes the `extends` DAG, resolves each Schema's effective fields,
    /// and computes hierarchy sets (children, descendants) for every resolved
    /// Schema.
    pub(super) fn resolve(self) -> Result<ResolvedSchemas, SchemaError> {
        let (mut graph, mut warnings) = SchemaGraph::new(self.raw);
        let mut resolved: IndexMap<SchemaName, Schema> = IndexMap::new();
        let mut failures: Vec<SchemaFailure> = Vec::new();

        while let Some(name) = graph.next_ready() {
            #[expect(
                clippy::expect_used,
                reason = "SchemaGraph::new builds parents_by_name/in_degree/ \
                          children_by_name/queue exclusively from raw's \
                          own keys, so next_ready() can never yield a name \
                          absent from raw; failure here means the graph \
                          itself is broken, not a recoverable caller error"
            )]
            let raw = self.raw.get(name.as_str()).expect(
                "SchemaGraph::next_ready only ever yields names present in raw",
            );
            match merge_fields(
                name,
                raw,
                graph.parents_of(name),
                &resolved,
            ) {
                Ok((schema, schema_warnings)) => {
                    warnings.extend(schema_warnings);
                    resolved.insert(SchemaName::from(name), schema);
                }
                Err(error) => {
                    failures.push(SchemaFailure {
                        schema: SchemaName::from(name),
                        error,
                    });
                }
            }
            graph.mark_resolved(name);
        }

        let graph = graph.into_resolved().map_err(|schemas| {
            SchemaError::Cycle { schemas }
        })?;

        // Filter hierarchy against resolution failures: a Schema downstream
        // of a `ParentFailedToResolve` break is still linked in the raw
        // topology, even though it no longer semantically `is_a` that
        // ancestor. Each resolved Schema's own `ancestors()` is the
        // authoritative, failure-aware signal.
        let children_by_name = graph.children_by_name();
        let descendants_by_name = graph.descendants_by_name();
        let resolved_ancestors: IndexMap<SchemaName, IndexSet<SchemaName>> =
            resolved
                .iter()
                .map(|(name, schema)| (name.clone(), schema.ancestors().clone()))
                .collect();
        for (name, schema) in &mut resolved {
            let still_descends_from = |candidate: &SchemaName| {
                resolved_ancestors
                    .get(candidate)
                    .is_some_and(|ancestors| ancestors.contains(name))
            };
            let children = children_by_name
                .get(name)
                .into_iter()
                .flatten()
                .filter(|child| still_descends_from(child))
                .cloned()
                .collect();
            let descendants = descendants_by_name
                .get(name)
                .into_iter()
                .flatten()
                .filter(|descendant| still_descends_from(descendant))
                .cloned()
                .collect();
            schema.set_hierarchy(children, descendants);
        }

        Ok(ResolvedSchemas {
            schemas: resolved,
            warnings,
            failures,
        })
    }
}

/// Resolve one Schema's effective fields and transitive ancestors, alongside
/// every warning degraded validation raised while building its own fields.
///
/// Merges `parents`' fields first-listed-wins, applies `raw.excludes`, then
/// overrides the result with `raw`'s own (`$ref`-resolved) fields.
///
/// `parents` must already be resolved in `resolved`: the caller guarantees
/// this by calling in Kahn topological order.
///
/// # Arguments
///
/// * `name`: the Schema being resolved (its filename stem).
/// * `raw`: `name`'s own parsed TOML: `extends`, `excludes`, and fields.
/// * `parents`: `raw.extends`, filtered to targets that resolved.
/// * `resolved`: Schemas already resolved earlier in Kahn order, keyed by name.
///
/// # Errors
///
/// - Any [`SchemaError`] that [`SchemaFieldBuilder::build`] returns while
///   resolving `raw`'s own fields.
/// - [`AmbiguousFieldName`] if two of the resolved fields share a [`FieldKey`]
///   canonical form.
///
/// [`AmbiguousFieldName`]: SchemaError::AmbiguousFieldName
/// [`FieldKey`]: crate::field::FieldKey
fn merge_fields(
    name: SchemaNameRef<'_>,
    raw: &RawSchema,
    parents: &[SchemaNameRef<'_>],
    resolved: &IndexMap<SchemaName, Schema>,
) -> Result<(Schema, Vec<SchemaWarning>), SchemaError> {
    let mut fields = IndexMap::new();
    let mut ancestors = IndexSet::new();
    let mut warnings = Vec::new();
    for &parent in parents {
        let Some(parent_schema) = resolved.get(parent.as_str()) else {
            warnings.push(SchemaWarning::ParentFailedToResolve {
                schema: SchemaName::from(name),
                parent: SchemaName::from(parent),
            });
            continue;
        };
        for (field_name, field) in parent_schema.fields() {
            fields.entry(field_name.clone()).or_insert_with(|| field.clone());
        }
        ancestors.insert(SchemaName::from(parent));
        ancestors.extend(parent_schema.ancestors().iter().cloned());
    }
    for excluded in &raw.excludes {
        fields.remove(excluded);
    }

    let refs = RefAddressResolver {
        ancestors: &ancestors,
        resolved,
    };
    let builder = SchemaFieldBuilder {
        refs: &refs,
    };
    let mut own_fields = IndexMap::new();
    for (field_name, raw_field) in &raw.fields {
        let address = FieldAddressRef::new(name, field_name.as_ref());
        let (field, field_warnings) = builder.build(address, raw_field)?;
        warnings.extend(field_warnings);
        own_fields.insert(field_name.clone(), field);
    }
    fields.extend(own_fields);

    reject_ambiguous_canonical_names(name, &fields)?;

    Ok((Schema::new(SchemaName::from(name), fields, ancestors), warnings))
}

/// Reject `fields` if two entries share a [`FieldKey`] canonical form:
/// ambiguous field identities would make later note-vs-schema field matching
/// and unknown-field suggestions unreliable.
///
/// # Errors
///
/// - [`AmbiguousFieldName`] naming the first two (name-sorted) colliding field
///   names.
///
/// [`FieldKey`]: crate::field::FieldKey
/// [`AmbiguousFieldName`]: SchemaError::AmbiguousFieldName
fn reject_ambiguous_canonical_names(
    name: SchemaNameRef<'_>,
    fields: &IndexMap<FieldName, super::fields::SchemaFieldDef>,
) -> Result<(), SchemaError> {
    let mut seen: HashMap<String, FieldName> = HashMap::new();
    for field_name in fields.keys() {
        let canonical = field_name.to_key().canonical().to_owned();
        if let Some(first) = seen.get(&canonical) {
            return Err(SchemaError::AmbiguousFieldName {
                schema: SchemaName::from(name),
                first: first.clone(),
                second: Box::new(field_name.clone()),
            });
        }
        seen.insert(canonical, field_name.clone());
    }
    Ok(())
}
```

- [ ] **Step 2: Add mod declaration to schema/mod.rs**

Add after the existing `mod graph;` declaration:
```rust
mod resolver;
```

- [ ] **Step 3: Remove build_schema, reject_ambiguous_canonical_names, and resolve_all from service.rs**

Remove these functions from service.rs:
- `build_schema` (lines 480-529)
- `reject_ambiguous_canonical_names` (lines 542-559)
- `resolve_all` (lines 373-453)

Also remove the `SchemaFailure` struct (lines 48-52) since it's now in resolver.rs.

- [ ] **Step 4: Update service.rs imports**

Remove from service.rs imports:
- `graph::SchemaGraph`
- `fields::{FieldAddressRef, RefAddressResolver, SchemaFieldBuilder}`

Add to service.rs imports:
- `resolver::{SchemaResolver, SchemaFailure}`

Remove `crate::field::FieldName` from service.rs imports if no longer used directly.

- [ ] **Step 5: Update SchemaService::new to use SchemaResolver**

Change `SchemaService::new` from:
```rust
pub(crate) fn new(
    spec: SchemaConfigSpec,
) -> Result<SchemaConstruction, SchemaError> {
    let raw = read_raw_schemas(spec.directory())?;
    let (schemas, warnings, failures) = resolve_all(&raw)?;
    let schemas = schemas
        .into_iter()
        .map(|(name, schema)| (name, Arc::new(schema)))
        .collect();
    Ok((
        Self {
            spec,
            schemas,
        },
        warnings,
        failures,
    ))
}
```
to:
```rust
pub(crate) fn new(
    spec: SchemaConfigSpec,
) -> Result<SchemaConstruction, SchemaError> {
    let raw = read_raw_schemas(spec.directory())?;
    let resolved = SchemaResolver::new(&raw).resolve()?;
    let schemas = resolved
        .schemas
        .into_iter()
        .map(|(name, schema)| (name, Arc::new(schema)))
        .collect();
    Ok((
        Self {
            spec,
            schemas,
        },
        resolved.warnings,
        resolved.failures,
    ))
}
```

- [ ] **Step 6: Update type alias**

Change the `SchemaConstruction` type alias if it references `SchemaFailure` — it should now import from `resolver`:
```rust
use super::resolver::{SchemaFailure, ResolvedSchemas};
```

Actually, `SchemaFailure` is already imported via step 4. The type alias stays the same:
```rust
type SchemaConstruction = (SchemaService, Vec<SchemaWarning>, Vec<SchemaFailure>);
```

- [ ] **Step 7: Run clippy and tests**

Run: `cargo clippy --workspace --all-targets --all-features -- -D warnings && mise run test`
Expected: All 1607 tests pass. Clippy clean.

- [ ] **Step 8: Commit**

```bash
git add -A
git commit -m "refactor(schema): extract resolver.rs with SchemaResolver

- merge_fields() handles inheritance, excludes, \$ref resolution
- SchemaResolver::resolve() orchestrates graph + fields + hierarchy
- reject_ambiguous_canonical_names() co-located with merge_fields
- service.rs becomes thin facade: loading + query methods only"
```

---

## Task 7: Migrate SchemaService to IndexMap

**Files:**
- Modify: `src/schema/service.rs` (lines 39, read_raw_schemas, matches, expand_classes)

This completes the IndexMap migration by updating the service facade.

- [ ] **Step 1: Add IndexMap import to service.rs**

Add to imports:
```rust
use indexmap::{IndexMap, IndexSet};
```

- [ ] **Step 2: Change SchemaService.schemas type**

Change line 39 from:
```rust
schemas: BTreeMap<SchemaName, Arc<Schema>>,
```
to:
```rust
schemas: IndexMap<SchemaName, Arc<Schema>>,
```

- [ ] **Step 3: Change read_raw_schemas return type**

Change the return type of `read_raw_schemas` from:
```rust
fn read_raw_schemas(
    directory: &Path,
) -> Result<BTreeMap<SchemaName, RawSchema>, SchemaError> {
```
to:
```rust
fn read_raw_schemas(
    directory: &Path,
) -> Result<IndexMap<SchemaName, RawSchema>, SchemaError> {
```

Change the internal collection from `BTreeMap::new()` to `IndexMap::new()`.

- [ ] **Step 4: Change matches() return type**

Change `matches()` return type from `BTreeSet<String>` to `IndexSet<String>`.

Update the internal `expanded` collection from `BTreeSet<String>` to `IndexSet<String>`.

- [ ] **Step 5: Update test code**

All test helpers constructing `BTreeMap::new()` for `SchemaService.schemas` need to change to `IndexMap::new()`.

Test assertions that compare sets using `assert_eq!` should work unchanged (IndexSet comparison is element-based, like BTreeSet).

Tests that use template rendering with `| join(',')` will produce definition-order output instead of name-order — update assertion expectations if needed.

- [ ] **Step 6: Run clippy and tests**

Run: `cargo clippy --workspace --all-targets --all-features -- -D warnings && mise run test`
Expected: All 1607 tests pass. Clippy clean.

- [ ] **Step 7: Commit**

```bash
git add -A
git commit -m "refactor(schema): migrate SchemaService to IndexMap

- schemas: IndexMap (preserves insertion order)
- read_raw_schemas returns IndexMap (filesystem order)
- matches() returns IndexSet
- Deterministic iteration for template consumers"
```

---

## Task 8: Final verification and cleanup

**Files:**
- Verify all files compile and pass tests

- [ ] **Step 1: Full clippy pass**

Run: `cargo clippy --workspace --all-targets --all-features -- -D warnings`
Expected: Clean, no warnings.

- [ ] **Step 2: Full test suite**

Run: `mise run test`
Expected: All tests pass (count should still be ~1607).

- [ ] **Step 3: Verify no leftover BTreeMap/BTreeSet in schema module**

Run: `grep -rn "BTreeMap\|BTreeSet" src/schema/`
Expected: No matches in production code (only in test code if any remain for specific ordering tests).

- [ ] **Step 4: Verify typestate enforcement**

The following should be compile errors (verify by reading code, not running):
- `graph.children_by_name()` before `graph.into_resolved()` — method not available on `SchemaGraph<Building>`
- `graph.next_ready()` after `graph.into_resolved()` — method not available on `SchemaGraph<Resolved>`

- [ ] **Step 5: Update issue doc status**

Update `.scratch/metadata-schemas/issues/07-schema-service-refactor.md` to reflect completed items.

- [ ] **Step 6: Final commit (if any cleanup needed)**

```bash
git add -A
git commit -m "chore(schema): final cleanup for graph deepen refactor"
```

---

## Task 9: Store children_by_name correctly and stop re-inverting

**Files:**
- Modify: `src/schema/graph.rs` (new(), Resolved impl block)

`children_by_name` is already a field on `SchemaGraph`, built in `new()` from filtered `parents_by_name`. Two problems: (a) GLOBAL_SCHEMA_NAME is not excluded as a parent during construction, so GLOBAL's children are in the field; (b) the `Resolved::children_by_name()` method ignores the field and re-inverts `parents_by_name` from scratch, adding the GLOBAL filter at query time. Fix: filter GLOBAL during `new()`, return a reference to the field.

- [ ] **Step 1: Filter GLOBAL out of children_by_name during construction**

In `new()`, add a GLOBAL check to the inner loop that builds `children_by_name`. Change lines 120-125 from:

```rust
for (&name, parents) in &parents_by_name {
    in_degree.insert(name, parents.len());
    for &parent in parents {
        children_by_name.entry(parent).or_default().push(name);
    }
}
```

to:

```rust
for (&name, parents) in &parents_by_name {
    in_degree.insert(name, parents.len());
    for &parent in parents {
        if parent.as_str() != GLOBAL_SCHEMA_NAME {
            children_by_name.entry(parent).or_default().push(name);
        }
    }
}
```

- [ ] **Step 2: Replace Resolved::children_by_name() re-inversion with a field accessor**

Change the `Resolved` impl's `children_by_name` method from:

```rust
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
```

to:

```rust
pub(super) fn children_by_name(
    &self,
) -> &IndexMap<SchemaNameRef<'_>, Vec<SchemaNameRef<'_>>> {
    &self.children_by_name
}
```

- [ ] **Step 3: Update descendants_by_name to use the field directly**

`descendants_by_name` currently calls `self.children_by_name()` (the method). After Step 2, this returns a borrowed reference. Change `let children = self.children_by_name();` to `let children = &self.children_by_name;`. The DFS logic stays the same.

- [ ] **Step 4: Update resolver.rs callers**

The caller in `resolver.rs` (line 943) does:
```rust
let children_by_name = graph.children_by_name();
```
This now returns `&IndexMap<SchemaNameRef<'_>, Vec<SchemaNameRef<'_>>>` instead of `IndexMap<SchemaName, IndexSet<SchemaName>>`. The downstream code (line 956-962) does `.get(name).into_iter().flatten().filter(...)` — update the types to match the new borrowed form. The key change: iterate `Vec<SchemaNameRef>` instead of `IndexSet<SchemaName>`.

- [ ] **Step 5: Run clippy and tests**

Run: `mise run clippy && mise run test`
Expected: All tests pass. Clippy clean.

- [ ] **Step 6: Commit**

```bash
git add src/schema/graph.rs src/schema/resolver.rs
git commit -m "fix(schema): stop re-inverting children_by_name in Resolved state

Filter GLOBAL_SCHEMA_NAME out during construction in new().
Return a reference to the stored field instead of recomputing."
```

---

## Task 10: Add bit-vec dependency

**Files:**
- Modify: `Cargo.toml`

- [ ] **Step 1: Add bit-vec to dependencies**

```toml
# In Cargo.toml [dependencies] section, add:
bit-vec = "0.6"
```

- [ ] **Step 2: Verify it compiles**

Run: `mise run check`
Expected: Compiles successfully (no code changes yet).

- [ ] **Step 3: Commit**

```bash
git add Cargo.toml Cargo.lock
git commit -m "chore: add bit-vec 0.6 dependency"
```

---

## Task 11: Add SchemaIndex bidirectional mapping

**Files:**
- Modify: `src/schema/graph.rs` (add `SchemaIndex` struct and tests)

- [ ] **Step 1: Add SchemaIndex to graph.rs**

Add at the bottom of `graph.rs`, before the `#[cfg(test)]` module:

```rust
/// Bidirectional mapping between schema names and bit positions.
///
/// Built once from the schema set at resolve time. Provides O(1) lookup
/// in both directions: name → bit index and bit index → name.
struct SchemaIndex {
    name_to_bit: IndexMap<SchemaName, usize>,
    bit_to_name: Vec<SchemaName>,
}

impl SchemaIndex {
    /// Build the index from schema names in declaration order.
    fn new<'a>(
        names: impl Iterator<Item = SchemaNameRef<'a>>,
    ) -> Self {
        let mut name_to_bit = IndexMap::new();
        let mut bit_to_name = Vec::new();
        for name in names {
            let bit = bit_to_name.len();
            bit_to_name.push(SchemaName::from(name));
            name_to_bit.insert(SchemaName::from(name), bit);
        }
        Self { name_to_bit, bit_to_name }
    }

    /// Number of schemas (bitset capacity).
    fn bit_count(&self) -> usize {
        self.bit_to_name.len()
    }

    /// Schema name → bit index.
    fn bit_of(&self, name: &str) -> Option<usize> {
        self.name_to_bit.get(name).copied()
    }

    /// Bit index → schema name.
    fn name_of(&self, bit: usize) -> Option<&SchemaName> {
        self.bit_to_name.get(bit)
    }
}
```

- [ ] **Step 2: Add SchemaIndex tests inside the existing `#[cfg(test)] mod tests` block**

Add a new submodule at the end of the existing test module:

```rust
mod schema_index {
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn bit_of_returns_the_insertion_order_index() {
        let index = SchemaIndex::new(
            ["global", "book", "sci_fi"]
                .iter()
                .map(|&s| SchemaNameRef::from(s)),
        );

        assert_eq!(index.bit_of("global"), Some(0));
        assert_eq!(index.bit_of("book"), Some(1));
        assert_eq!(index.bit_of("sci_fi"), Some(2));
        assert_eq!(index.bit_of("missing"), None);
    }

    #[test]
    fn name_of_returns_the_name_at_the_given_bit() {
        let index = SchemaIndex::new(
            ["global", "book"]
                .iter()
                .map(|&s| SchemaNameRef::from(s)),
        );

        assert_eq!(index.name_of(0), Some(&SchemaName::from("global")));
        assert_eq!(index.name_of(1), Some(&SchemaName::from("book")));
        assert_eq!(index.name_of(2), None);
    }

    #[test]
    fn bit_count_matches_the_number_of_names() {
        let index = SchemaIndex::new(
            ["a", "b", "c"].iter().map(|&s| SchemaNameRef::from(s)),
        );

        assert_eq!(index.bit_count(), 3);
    }
}
```

- [ ] **Step 3: Run tests for SchemaIndex**

Run: `mise run test -- --lib schema::graph::tests::schema_index`
Expected: 3 tests pass.

- [ ] **Step 4: Commit**

```bash
git add src/schema/graph.rs
git commit -m "feat(schema): add SchemaIndex bidirectional name-bit mapping"
```

---

## Task 12: Rewrite descendants_by_name DFS to use BitVec

**Files:**
- Modify: `src/schema/graph.rs` (descendants_by_name, descendants_of)
- Modify: `src/schema/resolver.rs` (hierarchy filtering)

This replaces the `IndexSet<SchemaName>` memo with `BitVec` during the DFS, then expands back to `IndexSet<SchemaName>` at the end. The union operation drops from O(k) hash-set extend to O(n/w) bitwise OR.

- [ ] **Step 1: Add imports to graph.rs**

Add to the existing imports:
```rust
use bit_vec::BitVec;
```

- [ ] **Step 2: Rewrite descendants_by_name to use SchemaIndex and BitVec**

Replace the `descendants_by_name` method and `descendants_of` helper in the `Resolved` impl block. First, add the `bitset` import at the top of graph.rs:

```rust
use super::bitset::SchemaIndex;
```

Then replace `descendants_by_name` and `descendants_of`:

```rust
pub(super) fn descendants_by_name(
    &self,
) -> IndexMap<SchemaName, IndexSet<SchemaName>> {
    let index = SchemaIndex::new(self.parents_by_name.keys().copied());
    let capacity = index.bit_count();
    let children = &self.children_by_name;

    let mut memo: IndexMap<SchemaName, BitVec> = IndexMap::new();
    for name in children.keys() {
        Self::descendants_of(name, children, &index, capacity, &mut memo);
    }

    // Expand bitsets back to IndexSet via BFS from children_by_name to
    // preserve parent-before-child ordering (required by template consumers
    // that | join(',') the result).
    let mut result: IndexMap<SchemaName, IndexSet<SchemaName>> =
        IndexMap::new();
    for (name, bits) in memo {
        if !bits.iter().any(|b| b) {
            continue;
        }
        let mut descendants = IndexSet::new();
        let mut queue: VecDeque<SchemaNameRef<'_>> = VecDeque::new();
        if let Some(direct) = children.get(name.as_str()) {
            for &child in direct {
                queue.push_back(child);
            }
        }
        while let Some(current) = queue.pop_front() {
            let owned = SchemaName::from(current);
            if !descendants.insert(owned) {
                continue;
            }
            if let Some(direct) = children.get(current.as_str()) {
                queue.extend(direct.iter().copied());
            }
        }
        result.insert(name, descendants);
    }
    result
}

fn descendants_of(
    name: &SchemaName,
    children: &IndexMap<SchemaNameRef<'_>, Vec<SchemaNameRef<'_>>>,
    index: &SchemaIndex,
    capacity: usize,
    memo: &mut IndexMap<SchemaName, BitVec>,
) -> BitVec {
    if let Some(cached) = memo.get(name) {
        return cached.clone();
    }
    let mut result = BitVec::from_elem(capacity, false);
    if let Some(direct) = children.get(name.as_str()) {
        for child in direct {
            if let Some(bit) = index.bit_of(child.as_str()) {
                result.set(bit, true);
            }
            let child_bits =
                Self::descendants_of(child, children, index, capacity, memo);
            result |= &child_bits;
        }
    }
    memo.insert(name.clone(), result.clone());
    result
}
```

- [ ] **Step 3: Run tests to verify DFS still produces correct results**

Run: `mise run test -- --lib schema::graph`
Expected: All graph tests pass. The existing `descendants_by_name` tests (diamond dedup, three-level chain, leaf schema) exercise the new code.

- [ ] **Step 4: Run full test suite**

Run: `mise run test`
Expected: All tests pass. The resolver.rs caller (line 943-970) gets the same `IndexMap<SchemaName, IndexSet<SchemaName>>` as before.

- [ ] **Step 5: Run clippy**

Run: `mise run clippy`
Expected: Clean.

- [ ] **Step 6: Update doc comment on descendants_by_name**

Change the complexity note from:

```
/// Total work is
/// `O(V + E + Σ|descendants(v)|)` — `O(V + E)` graph traversal plus the
/// unavoidable cost of materializing every entry, which degrades to
/// `O(V²)` in the worst case
```

to:

```
/// Total work is `O(V²/w)` for the bitset DFS (where `w` is the
/// machine word size, typically 64) plus `O(V²)` for expanding bitsets
/// back to name sets. Degrades to `O(V²)` in the worst case for the
/// expansion phase.
```

- [ ] **Step 7: Commit**

```bash
git add src/schema/graph.rs
git commit -m "perf(schema): use BitVec for descendants DFS computation

Union operations drop from O(k) hash-set extend to O(n/w) bitwise OR.
Final expansion to IndexSet<SchemaName> unchanged."
```

---

## Task 13: Verify bitset correctness and clean up

**Files:**
- Verify all files compile and pass tests

- [ ] **Step 1: Full clippy pass**

Run: `mise run clippy`
Expected: Clean, no warnings.

- [ ] **Step 2: Full test suite**

Run: `mise run test`
Expected: All tests pass.

- [ ] **Step 3: Verify no BTreeMap/BTreeSet in schema production code**

Run: `grep -rn "BTreeMap\|BTreeSet" src/schema/ --include="*.rs" | grep -v "#\[cfg(test)\]" | grep -v "mod tests"`
Expected: No matches in production code.

- [ ] **Step 4: Verify bit-vec is used correctly**

Run: `grep -rn "BitVec" src/schema/`
Expected: Only in `graph.rs` (the DFS computation).

- [ ] **Step 5: Update issue doc status**

Update `.scratch/metadata-schemas/issues/07-schema-service-refactor.md` to reflect bitset optimization as completed.

- [ ] **Step 6: Final commit (if any cleanup needed)**

```bash
git add -A
git commit -m "chore(schema): verify bitset optimization, final cleanup"
```

---

## Side Effects and Risk Mitigation

### Iteration order changes
**Risk:** Tests that assert specific iteration order (e.g., field names, children) may break.
**Mitigation:** IndexSet/IndexMap preserve insertion order. Tests using `assert_eq!` on sets compare elements, not order. Tests using `map(attribute='name') | join(',')` in templates will produce definition-order output instead of name-order.

**Affected tests:**
- `template/engine/schema.rs` tests that join children/descendants with commas
- `model.rs` tests that compare field iteration
- `service.rs` tests that compare matches sets

**Action:** Update assertion expectations to match definition order where applicable.

### PhantomData variance
**Risk:** `PhantomData<State>` introduces a lifetime parameter on `SchemaGraph<'a, State>`.
**Mitigation:** `State` is always a concrete type (Building or Resolved), never a reference. The `'a` lifetime is on `SchemaNameRef`, not on `State`. No variance issues.

### IndexMap serde support
**Risk:** `RawSchema` derives `Deserialize`. IndexMap needs the `serde` feature.
**Mitigation:** Added in Task 1: `indexmap = { version = "2.14", features = ["serde"] }`.

### borrowck with IndexMap
**Risk:** IndexMap's borrowck behavior differs from BTreeMap for entry API.
**Mitigation:** The codebase uses `entry().or_insert_with()` pattern, which works identically on IndexMap. No known borrowck differences for this pattern.

### Test count
**Risk:** Refactor may accidentally drop or duplicate tests.
**Mitigation:** Verify test count after each phase. Starting count: 1607. Final count should be >= 1607.

---

## Dependency Graph

```
Task 1 (add indexmap)
  └─ Task 2 (migrate RawSchema)
       └─ Task 3 (migrate Schema model)
            └─ Task 4 (migrate graph.rs internals)
                 └─ Task 5 (add typestate)
                      └─ Task 6 (create resolver.rs)
                           └─ Task 7 (migrate SchemaService)
                                └─ Task 8 (final verification)
                                     └─ Task 9 (store children_by_name correctly)
                                          └─ Task 10 (add bit-vec)
                                               └─ Task 11 (SchemaIndex)
                                                    └─ Task 12 (BitVec DFS)
                                                         └─ Task 13 (verify + clean)
```

Tasks 1-8 are foundational (type migrations, typestate, resolver). Task 9 is a correctness fix (stop re-inverting). Tasks 10-13 are the bitset optimization.

Each task produces a working, testable state. Tasks can be committed independently.
