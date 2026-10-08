# Discovery

## Contents

1. [Overview & Scope Traversal](#overview--scope-traversal)
2. [Multi-Lens Read-Only Scouting](#multi-lens-read-only-scouting)
3. [Predictability, Cohesion, & Layout Indicators](#predictability-cohesion--layout-indicators)
4. [Search Directions: Expansion & Compression](#search-directions-expansion--compression)
5. [Latent Concepts & Misplaced Seams](#latent-concepts--misplaced-seams)
6. [Removable Debt & The Deletion Dividend](#removable-debt--the-deletion-dividend)
7. [Depth Propagation](#depth-propagation)
8. [The Indicator Hypothesis](#the-indicator-hypothesis)
9. [Candidate Output Expectations](#candidate-output-expectations)

---

## Overview & Scope Traversal

Find design candidates before choosing abstractions. Discovery asks where knowledge is misplaced, fragmented, duplicated, implicit, or unnecessarily exposed.

Work at the scale of the requested scope, traversing upward or downward as evidence demands. Probe each inventory unit before concluding; read candidate implementations deeply rather than treating tool graphs or AST summaries as the design.

---

## Multi-Lens Read-Only Scouting

During architectural discovery, the orchestrator dispatches read-only investigations across ten distinct lenses.

### Strict Scout Isolation Rules

Scouts inspect source code, documentation, and AST facts. They are **forbidden** from running builds, tests, or diagnostic scripts. Only the main orchestrator runs CLI tools. Each lens reports up to 5 ranked hypotheses citing exact `file:line` locations, estimated SLoC deletion dividend, and Expected Architectural Value (EAV).

- **Lens 1: Modern Rust & Upstream Capabilities**
  Upstream language or standard library features that obsolete hand-rolled machinery (for example: `std::sync::Exclusive`, modern GAT patterns, slice chunking, `portable_simd`, or upstream crate capabilities).
- **Lens 2: Data-Oriented Layout & Memory Footprint**
  Memory layout inefficiencies, padding holes, cache-line misses, Struct-of-Arrays opportunities, and unnecessary heap boxing.
- **Lens 3: Allocation, Lifecycles & Zero-Copy**
  Defensive `.clone()` calls, unbounded per-item allocations, missing arena or bump allocators for scoped lifetimes, and opportunities for borrowed views or `Cow<'a, T>`.
- **Lens 4: Seam Placement & Knowledge Asymmetry**
  Callers bypassing public abstractions, direct field manipulation, leaked storage or serialization types, and improper visibility declarations.
- **Lens 5: State & Invariant Ownership**
  Invariants split across multiple types, mutable state disconnected from enforcing logic, invalid finite states representable in structs, and runtime assertions that could be compile-time typestates. Identifies runtime invariant leakage (defensive `assert!`, `panic!`, unhandled `.unwrap()`) that should be lifted into constructor-validated domain types ("parse, don't validate").
- **Lens 6: Boundary Crossings & Vertical Leakage**
  Internal child error types exposed in ancestor APIs, low-level configuration bubbling upward, and circular imports between sibling modules. Identifies tight coupling to volatile internal representations (raw database/wire schemas leaked across seams) that block safe expand-and-contract migrations.
- **Lens 7: Concurrency, I/O & Contention**
  Lock contention, overly coarse `Mutex` scopes, blocking operations on async reactors, serialization round-trips, and missing batching channels. Identifies unbounded resource buffering (`unbounded_channel()`, unbounded in-memory queues) and uncontained panic propagation across thread or task boundaries that risk cascading failure.
- **Lens 8: Algorithmic Scaling & Hot-Path Complexity**
  $O(n^2)$ lookups, redundant hashing on hot paths, repeated traversals, and opportunities for memoization or early-exit fast paths.
- **Lens 9: Deletion Dividend & Layer Thinning**
  Forwarding wrappers, redundant facade traits, one-to-one adapters, dead type conversions, and compatibility scaffolding that can be deleted.
- **Lens 10: Checks That Can't Fail & Test Ergonomics**
  Tautological tests, over-mocked boundaries that hide implementation regressions, tests asserting default states, and unasserted error pathways.

### Scout Synthesis & Cross-Examination

Before hypotheses are committed to the Hypothesis Ledger, the orchestrator synthesizes scout reports to identify compounding transformations. For example, a Lens 2 data-layout reorganization may directly enable a Lens 3 zero-copy parser. Conflicting hypotheses are reconciled by prioritizing higher Expected Architectural Value.

## Predictability, Cohesion, & Layout Indicators

File and directory layouts represent the cognitive map of a Rust codebase. A design must adhere to the **Principle of Least Surprise (POLS)**: an engineer inspecting a module should find state, logic, and types exactly where their semantic ownership implies.

1. **SLoC Concentration (>500 SLoC):** A single `.rs` file exceeding 500 SLoC is a primary indicator of mixed lifecycles, entangled invariants, or multiple independent modules trapped in one file.
2. **Disjoint Reference Clusters (Intra-File Incohesion):** In knowledge graph analysis (via `codegraph` or `rustgraph`), types and functions within the same file that have zero references to each other, but heavy references to distinct external modules, represent misplaced responsibilities grouped by accident.
3. **Feature Envy (Cross-Boundary Attraction):** A type or function in `module_a.rs` that references fields, methods, or errors in `module_b.rs` more frequently than its own file belongs inside `module_b.rs`.
4. **Asymmetric / Sprawling Import Footprint:** A file importing numerous distinct sibling modules to perform a single operation indicates an unencapsulated coordinator rather than a cohesive owner.
5. **Bypassed Seams (Deep Path Traversal):** Callers reaching deep into child submodules (e.g. `use crate::pipeline::stage::internal::parse_header;`) rather than using the parent interface indicate a misplaced or ineffective seam.
6. **"Junk Drawer" Anti-Patterns:** Files named `utils.rs`, `helpers.rs`, `common.rs`, or `misc.rs` violate predictability. Functions in utility files must be relocated to the domain type they operate upon or encapsulated behind an intentional seam.

---

## Search Directions: Expansion & Compression

Deepening candidates appear through **expansion** and **compression**.

### Expansion Candidates

One scope may contain multiple coherent concerns whose structure is hidden.
Signals include: independent invariants, distinct state or lifecycle, distinct domain vocabulary, policy mixed with low-level mechanism, unrelated dependency subsets, hidden state transitions.

### Compression Candidates

Several pieces may collectively represent too little.
Signals include: forwarding functions, one-to-one delegation chains, pass-through modules, wrappers exposing nearly everything they hide, helpers whose callers must know execution order, duplicated orchestration, distributed policy.

---

## Latent Concepts & Misplaced Seams

### Find Latent Concepts

Look for concepts already implemented implicitly:

- Primitives carrying unstated domain semantics
- Repeated validation of the same value
- Boolean or `Option` combinations representing closed states
- Invalid states representable in ordinary data
- Data and operations jointly enforcing an unstated invariant

Record the semantic concept and caller burden first before selecting a Rust representation.

### Find Misplaced Seams

Inspect where knowledge crosses architectural levels:

- Callers constructing internal representations
- Parents coordinating child ordering requirements
- Storage, serialization, hashing, or protocol types escaping upward
- Child-specific failures leaking through unrelated parent interfaces
- Callers bypassing an intended seam through descendant imports

---

## Removable Debt & The Deletion Dividend

Every redesign search includes subtraction. Removal is a primary candidate transformation, not deferred cleanup.

Look for:

- Shallow abstractions superseded by another owner
- Obsolete compatibility scaffolding
- Parallel representations of the same concept
- Production seams introduced only for tests
- Traits or generic parameters without meaningful variation
- Conversions whose source or destination can disappear

---

## Depth Propagation

For every promising lower-level candidate, inspect its parent:

- Which concepts would the parent stop understanding?
- Which invariants would move behind the child seam?
- Which dependencies would disappear from the parent?
- Would the parent operate at a more coherent abstraction level?
- Would changes remain below the parent seam?

A locally tidy abstraction with no simplifying effect on its callers is weak evidence for a seam.

---

## The Indicator Hypothesis

Diagnostic indicators (complexity, CRAP, duplication, fan-out, file size) do not establish design verdicts or scores. Instead, formulate questions about domain ownership:

```text
high complexity     -> what knowledge is entangled here?
token duplication   -> is the same policy or only similar syntax repeated?
high fan-out        -> is responsibility broad, or is the seam leaking mechanisms?
large file (>500)   -> is it incohesive, or a single large cohesive implementation?
many tiny modules   -> do they hide distinct knowledge, or fragment one responsibility?
```

---

## Candidate Output Expectations

For each candidate hypothesis, record:

```text
id: HYP-###
lens: [Lens Name]
thesis: [Transformational thesis]
current seam and caller knowledge at issue:
proposed direction and parent effect:
what becomes removable (Deletion Dividend):
estimated EAV (Gain * Confidence / (Blast Radius + Friction)):
drop condition: [Concrete condition triggering immediate rollback]
selected gauges and raw evidence:
uncertainty:
```

A review records expected effects as predictions in `ledger.json`. An implementation later adds observations using the same comparison basis.
