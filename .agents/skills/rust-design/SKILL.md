---
name: rust-design
description: >
  Use when analyzing, reviewing, or refactoring Rust software architecture: evaluating module boundaries and seams, establishing recursive depth, discovering latent responsibilities, eliminating cyclic dependencies, optimizing file layout and cohesion, or modeling domain types and typestates.
---

# Rust Design

Discover and deepen Rust designs.

Concentrate coherent behaviour, policy, state, and invariants behind well-placed seams so callers learn less, maintainers change less, and Rust enforces useful constraints without making interfaces harder to use.

A successful redesign may decompose, consolidate, collapse, relocate, model, replace, or remove code. **Deepening** is the objective; no particular transformation is preferred.

---

## Foundations

### Core Architectural Concepts

- **Module:** A unit of software that provides a cohesive capability behind an interface. A module may consist of a single function, several types, a whole file, or a hierarchy of submodules.
- **Interface:** The knowledge and types a caller must understand to use a module correctly.
- **Implementation:** The private state, logic, and data structures that execute the module's capability.
- **Seam:** An intentional boundary across which knowledge is abstracted and concentrated.
- **Recursive Depth:** An internal decomposition where submodules concentrate knowledge behind their own meaningful seams, making their parent simpler and deeper rather than merely adding layers.
- **Local vs. Propagated Depth:** Local depth is capability relative to interface burden. Propagated depth is the degree to which internal decomposition simplifies ancestor modules and eliminates coordination complexity.
- **Locality:** The degree to which related domain concepts, invariants, and operations change together.
- **Principle of Least Surprise (POLS):** File, module, and directory layouts must predict domain ownership. An engineer inspecting a module should find state, logic, and types exactly where semantic ownership implies.

Load `rust-skills` only after a design concern or latent concept has been discovered. Consult the categories relevant to the candidate rather than applying its rules as a generic checklist.

Treat comments, documentation, ADRs, names, and directory layout as evidence of intended design, not proof of actual responsibility or seam placement. Validate design claims against behaviour, dependencies, state, callers, tests, and source structure.

---

## Operational Modes & Process Header

Every execution classifies prompt intent into one of three operational modes:

| Mode | Trigger Signals in Request | Active Steps | Stop Condition |
| :--- | :--- | :--- | :--- |
| **Review** | "review", "audit", "inventory", "check boundaries", "evaluate architecture" | **1, 2, 3, 6** | Target scope defined; True Baseline frozen; units probed; candidates dispositioned; ranked shortlist emitted. |
| **Model** | "model", "design types", "represent invariants", "typestate", "type safety" | **4** | Seam established; types, errors, state transitions, and visibility documented. |
| **Implement** | "refactor", "deepen", "redesign", "extract module", "fix seam", "reorganize" | **1, 2, 3, 4, 5, 6** | Code modified; behavior verified through seam; before-and-after SLoC and architectural deltas recorded; patch stored. |

### Execution Strategy: Targeted Deepening vs. Breakthrough

- **Targeted Deepening (Standard):** Restructures internals within the target scope, extracts recursively deep child modules, collapses shallow wrappers, and eliminates cyclic dependencies while preserving external crate contracts (`cargo-public-api`).
- **Breakthrough:** Fundamental architectural paradigm shift (e.g. replacing dynamic dispatch with zero-cost typestates, inverting cross-crate ownership). Requires explicit prototype validation and an observable drop condition.

---

## Deliverable Containment

All generated analysis, intermediate scout outputs, and final reports are stored inside the gitignored `.design/` directory at the project root:

```text
.design/YYYY-MM-DD-<mode>-<target-slug>/
├── baseline.json        # Output from `uv run scripts/rust_design.py gather`
├── report.md            # Authoritative Architectural Report
├── scouts/              # Findings from 6 read-only lens subagents
│   ├── lens1-seams.md
│   ├── lens2-knowledge.md
│   ├── lens3-state.md
│   ├── lens4-deletion.md
│   ├── lens5-boundaries.md
│   └── lens6-invariants.md
└── patch.diff           # (Implement mode only) Verified Git patch
```

There are **zero loose directories** (`evals/`, `scouts/`) created at the workspace root.

---

## Interactive Workflow Checklist

Maintain this checklist in the scratchpad across every execution:

```markdown
### Workflow Execution Checklist
- [ ] 1. True Baseline: Run `uv run scripts/rust_design.py gather` and freeze True Architectural Baseline into `.design/YYYY-MM-DD-<mode>-<target>/baseline.json`.
- [ ] 2. Layout & Cohesion Analysis: Evaluate file SLoC, intra-file reference clusters, and import asymmetry.
- [ ] 3. Multi-Lens Scouting: Dispatch read-only scouts across the 6 architectural lenses (record outputs in `scouts/`).
- [ ] 4. Candidate Screening: Disposition all high-severity hotspots, CRAP risks, and cycles. (Branch: If any material finding lacks a disposition, return to Step 3).
- [ ] 5. Deepen / Model: Formulate recursive depth transformations; verify public API stability via `cargo-public-api`.
- [ ] 6. Verification & Grader Audit: Verify seam behavior; run independent grader against Done Criteria; emit `report.md`.
```

---

## Evidence Discipline & Anti-Gaming

Judge designs by knowledge and behaviour:

- What callers must know to use the seam correctly
- Which invariants and policies a module owns
- Where state and behaviour belong together
- Where change propagates across boundaries
- What representations leak through seams
- How much capability a seam provides relative to its surface

### The Indicator Hypothesis

Tool findings (complexity, duplication, coverage, CRAP, fan-out, cycles) are **indicators**, not verdicts. They formulate questions about domain ownership rather than scoring a design:

```text
high complexity     -> what knowledge is entangled here?
token duplication   -> is the same policy or only similar syntax repeated?
high fan-out        -> is responsibility broad, or is the seam leaking mechanisms?
large file (>500)   -> is it incohesive, or a single large cohesive implementation?
many tiny modules   -> do they hide distinct knowledge, or fragment one responsibility?
```

Do not combine indicators into an aggregate score. Treat each gauge as an isolated semantic dimension.

### SLoC Measurement & Anti-Gaming

Size metrics must measure **Source Lines of Code (SLoC)**: non-comment, non-blank lines of Rust code.

- **Comment Preservation Rule:** Stripping doc comments (`///`, `//!`), inline explanations, or rustdoc examples to artificially reduce line counts is strictly prohibited. Any proposal that decreases comment-to-code ratios without justification fails verification.
- **Semantic Deletion Dividend:** The deletion dividend evaluates the removal of architectural complexity (dead types, obsolete traits, collapsed wrappers), not explanatory prose.

---

## Step-by-Step Process

### 1. Establish the Current Design & Freeze True Baseline

Execute the automated analysis helper to freeze the True Architectural Baseline:

```bash
uv run scripts/rust_design.py gather --path <target-path> --out .design/YYYY-MM-DD-<mode>-<target-slug>/baseline.json
```

If `scripts/rust_design.py` is absent, follow the Three-Tier Fallback Ladder in [`TOOLING.md`](TOOLING.md): inspect task runners (`mise`, `just`), or fall back to standard Cargo toolchain commands.

Map the target's responsibility, callers, interface, dependencies, important state, invariants, policy, visibility, tests, and parent/child relationships. Inspect tests entering each intended seam.

**Complete when:**

1. Baseline JSON is frozen under `.design/YYYY-MM-DD-<mode>-<target-slug>/baseline.json`.
2. SLoC, public API footprint (`cargo-public-api`), CRAP scores, duplicate dependencies (`cargo tree -d`), and graph density are recorded.
3. Unavailable tool families are explicitly recorded with reasons in `gaps[]` rather than omitted.
4. Named source and caller observations substantiate the current seam map.

---

### 2. Discover Candidates via Multi-Lens Read-Only Scouting

Dispatch parallel read-only subagents across the **6 Architectural Lenses** documented in [`DISCOVERY.md`](DISCOVERY.md):

- **Lens 1:** Seam Placement & Encapsulation
- **Lens 2:** Knowledge Asymmetry & Caller Burden
- **Lens 3:** State & Invariant Ownership
- **Lens 4:** Deletion Dividend & Layer Thinning
- **Lens 5:** Boundary Crossings & Vertical Leakage
- **Lens 6:** Enforce, Don't Remind (Type-System Invariants)

**Scout Isolation Rule:** Scouts operate strictly in read-only mode and are forbidden from running builds, tests, or diagnostic scripts. Only the main orchestrator runs CLI tools.

#### Concrete Definition of "Consequential"

A candidate or finding is **consequential** if and only if it satisfies at least one observable criterion:

1. Changes or bypasses a public API signature, visibility boundary, or trait contract.
2. Alters cross-module or cross-crate dependency direction (introducing or removing edges/cycles).
3. Transfers ownership of mutable state or invariant enforcement across types or files.
4. Generates a static analysis finding above standard thresholds (cyclomatic complexity > 20, CRAP score > 30, token duplication > 50 tokens, or cyclic dependency edge).
5. Targets a component explicitly named in the user prompt.

#### Closed Deferral Reason Enum

Every deferred finding must use one of three explicit enum variants, and total deferrals cannot exceed 3 per review without explicit human approval:

- `DEFERRED_TOOL_UNAVAILABLE`: Required diagnostic binary is absent from host.
- `DEFERRED_OUT_OF_SCOPE`: Finding resides entirely outside requested target boundary.
- `DEFERRED_HIGH_TRACE_COST`: Verification requires exhaustive dynamic tracing exceeding turn boundaries.

**Complete when:**

1. All 6 lenses report observations into `.design/YYYY-MM-DD-<mode>-<target-slug>/scouts/`.
2. Every consequential candidate has an assigned transformation direction or recorded dismissal reason.
3. Every material tool finding from Step 1 has a recorded disposition (investigated, dismissed with reason, or classified under the Closed Deferral Reason Enum).
4. Unresolved questions and inspected boundaries are explicitly named.

---

### 3. Deepen Candidates & Recursive Decomposition

Read [`DEEPENING.md`](DEEPENING.md). Formulate structural transformations that concentrate knowledge behind simpler seams. Apply the **Recursive Depth Test** to each candidate decomposition:

1. **Local Depth:** Does the child seam provide substantial capability relative to the small interface it exposes to the parent?
2. **Depth Propagation:** Does the parent's implementation become shorter, simpler, and higher-level? Did internal states, dependencies, and ordering rules vanish from the parent?
3. **Anti-Layering Check:** Verify that the child is not a shallow pass-through facade.

Account for the **Deletion Dividend**: identify superseded structs, obsolete traits, collapsed wrappers, and dependencies that become removable.

**Complete when:**

1. The proposal explains what belongs together or apart, what each seam hides, and how callers change.
2. Depth propagation is proven: parent simplification is documented with specific knowledge atoms removed.
3. The Deletion Dividend ledger lists all superseded types, wrappers, and dependencies.
4. All metric effects are labeled as predictions.

---

### 4. Model the Design in Rust

Read [`MODELING.md`](MODELING.md), then consult relevant portions of `rust-skills`. Translate discovered semantics into idiomatic Rust constructs:

- Prefer ordinary functions and concrete structs before adding traits or generics.
- Enforce "parse, don't validate" to eliminate invalid states at construction ($\text{ISR} = 0$).
- Use typestate only when states are stable and transitions are central to correctness ($\text{ITE} = 0$).
- Verify external crate semantics through authoritative crate documentation (local documentation tools if available, or docs.rs) before adding dependencies.

**Complete when:**

1. Proposed types, ownership, errors, traits, and visibility directly serve the intended seams.
2. Invariants are enforced at construction or compile time.
3. Type-system ceremony is balanced against caller burden ($\text{IKL}$).
4. Generics and traits are justified by demonstrated variation or boundary isolation.

---

### 5. Verify an Implementation

When code is modified:

1. Verify required behavior through the intended seam using project test suites.
2. Verify public API stability: run `cargo public-api` diff checking to prove zero accidental breaking changes.
3. Calculate before-and-after SLoC and doc comment line deltas via `scripts/rust_design.py gather`.
4. Enforce the Comment Preservation Rule: doc comments must not be stripped or compressed.
5. Store the final verified patch in `.design/YYYY-MM-DD-implement-<target-slug>/patch.diff`.

**Complete when:**

1. Intended behaviour has been verified through tests entering the seam.
2. Public API diff is verified and clean.
3. SLoC deltas and comment preservation are documented.
4. Superseded structure is deleted or accounted for in the Deletion Dividend ledger.

---

### 6. Boundary Verification & Adversarial Grader Audit

Reconsider the boundary:

1. Inspect downward for newly visible coherent concepts and upward for parent or sibling simplifications.
2. Revisit Step 2 only if this inspection exposes a consequential new candidate.
3. Run an independent grader pass against the Done Criteria to ensure zero vacuous completions or unjustified deferrals.
4. Format and emit `report.md` matching the Standardized Architectural Report Schema.

**Complete when:**

1. Inspected boundaries and parent/child relationships are documented.
2. All findings have definitive dispositions.
3. Final deliverable is written to `.design/YYYY-MM-DD-<mode>-<target-slug>/report.md`.

---

## Standardized Architectural Report Schema

Every execution formats its final output under these mandatory Markdown headings in `report.md`:

```markdown
# Architectural Report: [Scope / Target Name]

## 1. Executive Summary & Strategy
- Mode: [Review | Model | Implement] (Strategy: [Targeted Deepening | Breakthrough])
- Primary Target: [Target Path or Module]
- Deliverable Path: `.design/YYYY-MM-DD-<mode>-<target-slug>/`
- Architectural Thesis: [Summary of the recursive depth and knowledge transformation]

## 2. True Architectural Baseline vs. Proposed / Final State
| Metric Dimension | Baseline | Proposed / Observed | Delta | Evidence Source |
| :--- | :--- | :--- | :--- | :--- |
| Source Lines of Code (SLoC) | 1,850 SLoC | 1,480 SLoC | -370 (-20%) | `rust_design.py` (stripped) |
| Doc Comment Lines (Anti-Gaming) | 420 lines | 445 lines | +25 (+6%) | Preserved & expanded |
| Public API Footprint (IKL) | 34 items | 14 items | -20 (-59%) | `cargo-public-api` |
| High CRAP Functions (>30) | 2 functions | 0 functions | -2 (-100%) | `cargo-crap` |
| Maintainability Hotspots | 3 high-risk | 0 high-risk | -3 (-100%) | `messrust` |
| Cyclic Dependency Edges | 0 cycles | 0 cycles | 0 | `cargo-modules` |
| Duplicate Crate Versions | 1 duplicate | 0 duplicates | -1 | `cargo tree -d` |
| Graph Edge Density (Coupling) | 5,120 edges | 3,840 edges | -1,280 (-25%) | `codegraph` / `rustgraph` |

## 3. Recursive Depth & Seam Decomposition
- Parent Module Simplification: [What vanished from the parent's knowledge obligations]
- Deep Child Modules Extracted:
  - `child_a`: [Owns state machine; hides validation; local depth ratio]
  - `child_b`: [Owns I/O buffering; hides wire protocol; local depth ratio]
- Depth Propagation Evidence: [Why the parent seam is now deeper rather than layered]

## 4. Predictability & Layout Refactor
- Cohesion Fixes: [Resolved disjoint reference clusters in files]
- Eliminated Anti-Patterns: [Removed junk drawer files, deep path bypasses]
- Naming & Hierarchy: [How layout now matches domain cognitive map]

## 5. Deletion Dividend Ledger
- Superseded Types & Functions: [Explicit names deleted]
- Eliminated Adapters & Wrappers: [Modules collapsed]
- Removed Dependencies: [Crates eliminated from Cargo.toml via `cargo tree`]
- Net SLoC Removed: [Exact count]

## 6. Architectural Graveyard (Tried & Rejected)
- Candidate: [Concept]
- Hypothesis: [Intended improvement]
- Rejection Evidence: [Why it was abandoned; metrics or benchmark regressions]

## 7. Verification & Grader Audit
- Seam Behavior Verification: [Tests executed through intended seams]
- Public API Stability: [Diff verified via `cargo public-api`]
- Done Criteria Verification: [Audited by independent pass]
```
