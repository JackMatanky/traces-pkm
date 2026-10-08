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

## Operational Modes & Execution Strategies

Every execution classifies prompt intent into an operational mode and strategy:

| Mode | Strategy | Trigger Signals | Active Steps | Stop Condition |
| :--- | :--- | :--- | :--- | :--- |
| **Review** | **Diagnostic** | "review", "audit", "inventory", "check boundaries", "evaluate architecture" | **1, 2, 6** | True Baseline frozen; 10 lenses probed; Hypothesis Ledger populated and ranked by EAV; report emitted. |
| **Model** | **Type Modeling** | "model", "design types", "represent invariants", "typestate", "type safety" | **4** | Seam established; types, errors, state transitions, and compile-time invariants documented. |
| **Implement** | **Steady** | "refactor", "deepen", "optimize", "fix seam", "reorganize", "steady" | **1, 2, 3, 4, 5, 6** | High-EAV hypotheses worked through empirical trial loop; verified against baseline; deltas recorded; patch stored. |
| **Implement** | **Breakthrough** | "breakthrough", "radical", "paradigm shift", "fundamental", "from scratch" | **1, 2, 3, 4, 5, 6** | Single high-leverage bet prototyped against explicit drop condition; verified or reverted to graveyard; final state clean. |

### Execution Strategies: Steady vs. Breakthrough

- **Steady (Targeted Deepening):** Works the Hypothesis Ledger sequentially by Expected Architectural Value (EAV). Restructures internals within the target scope, extracts recursively deep child modules, collapses shallow wrappers, and eliminates cyclic dependencies while preserving external crate contracts.
- **Breakthrough (Architectural Leap):** Focuses the run budget on a single ambitious bet (such as replacing dynamic dispatch with zero-cost typestates, data-oriented layout restructuring, or inverting cross-crate ownership). Requires explicit prototype validation, sandboxed trial mechanics, and an observable drop condition.

## Deliverable Containment

All generated analysis, intermediate scout outputs, trial records, and final reports are stored inside the gitignored `.design/` directory at the project root:

```text
.design/YYYY-MM-DD-<mode>-<target-slug>/
├── baseline.json        # Output from `uv run .../scripts/measure.py`
├── ledger.json          # Architectural Hypothesis Ledger & scoreboards
├── graveyard.md         # Active Graveyard (tried and rejected prototypes)
├── report.md            # Authoritative Architectural Report
├── scouts/              # Findings from 10 read-only lens subagents
│   ├── lens1-modern.md
│   ├── lens2-data-layout.md
│   ├── lens3-zero-copy.md
│   ├── lens4-seams.md
│   ├── lens5-state.md
│   ├── lens6-boundaries.md
│   ├── lens7-concurrency.md
│   ├── lens8-scaling.md
│   ├── lens9-deletion.md
│   └── lens10-checks.md
└── patch.diff           # (Implement mode only) Verified Git patch
```

## There are **zero loose directories** (`evals/`, `scouts/`) created at the workspace root

## Interactive Workflow Checklist

Maintain this checklist in the scratchpad across every execution:

```markdown
### Workflow Execution Checklist
- [ ] 1. Establish Current Design & Baseline: Run `measure.py` to freeze True Architectural Baseline.
- [ ] 2. Discover Hypotheses via 10-Lens Scouting: Dispatch 10 read-only scouts; populate and rank Hypothesis Ledger by EAV.
- [ ] 3. Empirical Trial-and-Revert Search Loop: Prototype top bets in sandboxed trials; evaluate deltas; commit wins or revert to graveyard.
- [ ] 4. Model Design in Rust: Translate semantics into concrete types, parse-don't-validate, typestates, and data-oriented layouts.
- [ ] 5. Verify Implementation: Run test suites, verify API stability, compute deltas with `diff.py`.
- [ ] 6. Boundary Verification & Adversarial Grader Audit: Verify boundaries, run independent grader pass, emit `report.md`.
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
uv run .agents/skills/rust-design/scripts/measure.py <target-path> .design/YYYY-MM-DD-<mode>-<target-slug>/baseline.json
```

If the automated helper is absent, follow the Three-Tier Fallback Ladder in [`TOOLING.md`](TOOLING.md): inspect task runners (`mise`, `just`), or fall back to standard Cargo toolchain commands.

Map the target's responsibility, callers, interface, dependencies, important state, invariants, policy, visibility, tests, and parent/child relationships. Inspect tests entering each intended seam.

**Complete when:**

1. Baseline JSON is frozen under `.design/YYYY-MM-DD-<mode>-<target-slug>/baseline.json`.
2. SLoC, public API footprint (`cargo-public-api`), CRAP scores, duplicate dependencies (`cargo tree -d`), and graph density are recorded.
3. Unavailable tool families are explicitly recorded with reasons in `gaps[]` rather than omitted.
4. Named source and caller observations substantiate the current seam map.

---

### 2. Discover Hypotheses via 10-Lens Read-Only Scouting

Dispatch parallel read-only subagents across the **10 Architectural Lenses** documented in [`DISCOVERY.md`](DISCOVERY.md):

1. **Lens 1:** Modern Rust & Upstream Capabilities
2. **Lens 2:** Data-Oriented Layout & Memory Footprint
3. **Lens 3:** Allocation, Lifecycles & Zero-Copy
4. **Lens 4:** Seam Placement & Knowledge Asymmetry
5. **Lens 5:** State & Invariant Ownership
6. **Lens 6:** Boundary Crossings & Vertical Leakage
7. **Lens 7:** Concurrency, I/O & Contention
8. **Lens 8:** Algorithmic Scaling & Hot-Path Complexity
9. **Lens 9:** Deletion Dividend & Layer Thinning
10. **Lens 10:** Checks That Can't Fail & Test Ergonomics

**Scout Isolation Rule:** Scouts operate strictly in read-only mode and are forbidden from running builds, tests, or diagnostic scripts. Only the main orchestrator runs CLI tools.

#### Expected Architectural Value (EAV)

Rank candidate redesigns by **Expected Architectural Value (EAV)**:

$$\text{EAV} = \frac{\text{Structural or Computational Gain} \times \text{Confidence}}{\text{Blast Radius} + \text{Implementation Friction}}$$

A candidate is **consequential** if $\text{EAV} \ge 1.0$, or if it satisfies at least one observable trigger:

1. Changes or bypasses a public API signature, visibility boundary, or trait contract.
2. Alters cross-module or cross-crate dependency direction (introducing or removing edges/cycles).
3. Transfers ownership of mutable state, memory layout, or invariant enforcement across types or files.
4. Reduces big-O computational or memory scaling on identifiable hot paths.
5. Targets a component explicitly named in the user prompt.

#### Closed Deferral Reason Enum

Every deferred finding must use one of three explicit enum variants, and total deferrals cannot exceed 3 per review without explicit human approval:

- `DEFERRED_TOOL_UNAVAILABLE`: Required diagnostic binary is absent from host.
- `DEFERRED_OUT_OF_SCOPE`: Finding resides entirely outside requested target boundary.
- `DEFERRED_HIGH_TRACE_COST`: Verification requires exhaustive dynamic tracing exceeding turn boundaries.

**Complete when:**

1. All 10 lenses report observations into `.design/.../scouts/`.
2. The Hypothesis Ledger (`ledger.json`) is populated and sorted in descending order of EAV.
3. Every material tool finding from Step 1 has a recorded disposition (investigated, dismissed with reason, or classified under the Closed Deferral Reason Enum).
4. Unresolved questions and inspected boundaries are explicitly named.

### 3. Empirical Trial-and-Revert Search Loop

Read [`DEEPENING.md`](DEEPENING.md). In Implement mode, execute an empirical hypothesis search:

1. **Select Candidate:** In Steady mode, select the top-ranked EAV item from `ledger.json`. In Breakthrough mode, select the single highest-leverage fundamental bet.
2. **Sandboxed Trial:**
   - Create a clean git checkpoint, isolated worktree, or branch before applying edits.
   - For Breakthrough bets, record the explicit drop condition before writing code (for example: "If prototype fails to compile within 3 refinements or introduces unbounded lifetimes, revert and log to graveyard").
3. **Execute & Measure:**
   - Apply candidate transformations across the intended seam.
   - Run project test suites and diagnostic benchmarks.
   - Run `diff.py` to compare against baseline.
4. **Commit or Revert Decision:**
   - **Keep Condition:** Tests pass, external API contracts hold, and the scoreboard demonstrates a positive delta (reduced SLoC, collapsed layers, improved throughput, or verified invariant consolidation) without uncompensated regressions. Commit the change, update `ledger.json`, and advance the baseline.
   - **Revert Condition:** If tests fail, drop conditions trigger, or regressions exceed noise tolerances, cleanly roll back the changes immediately. Append the trial record and rejection post-mortem to `.design/.../graveyard.md`.
5. **Convergence, Plateau, and Budget Gates:**
   - **Convergence Gate:** Stop when all high-EAV hypotheses are resolved.
   - **Plateau Gate:** If two consecutive hypotheses fail to pay off, relaunch the 10-lens reviewers against the updated boundary state. If re-scouting yields zero new candidates with $\text{EAV} \ge 1.0$, terminate the search.
   - **Budget Gate:** Stop when the specified trial limit or session budget is reached.

**Complete when:**

1. All selected hypotheses have been empirically evaluated and committed or reverted.
2. The Deletion Dividend ledger lists all superseded types, wrappers, and dependencies.
3. The Active Graveyard documents all rejected bets with concrete failure evidence.
4. The scoreboard in `ledger.json` reflects all kept progression steps.

### 4. Model the Design in Rust

Read [`MODELING.md`](MODELING.md), then consult relevant portions of `rust-skills`. Translate discovered semantics into idiomatic Rust constructs:

- Prefer ordinary functions and concrete structs before adding traits or generics.
- Apply data-oriented design: organize memory layouts for cache locality and contiguous access.
- Enforce "parse, don't validate" to eliminate invalid states at construction ($\text{ISR} = 0$).
- Use typestate only when states are stable and transitions are central to correctness ($\text{ITE} = 0$).
- Verify external crate semantics through authoritative crate documentation before adding dependencies.

**Complete when:**

1. Proposed types, ownership, errors, traits, and visibility directly serve the intended seams.
2. Invariants are enforced at construction or compile time.
3. Type-system ceremony is balanced against caller burden ($\text{IKL}$).
4. Generics and traits are justified by demonstrated variation or boundary isolation.

### 5. Verify an Implementation

When code is modified:

1. Verify required behavior through the intended seam using project test suites.
2. Verify public API stability: run `cargo public-api` diff checking to prove zero accidental breaking changes.
3. Calculate before-and-after SLoC and doc comment line deltas via:

   ```bash
   uv run .agents/skills/rust-design/scripts/measure.py <target-path> .design/YYYY-MM-DD-implement-<target-slug>/final.json
   uv run .agents/skills/rust-design/scripts/diff.py .design/.../baseline.json .design/.../final.json
   ```

4. Enforce the Comment Preservation Rule: doc comments must not be stripped or compressed.
5. Store the final verified patch in `.design/YYYY-MM-DD-implement-<target-slug>/patch.diff`.

**Complete when:**

1. Intended behaviour has been verified through tests entering the seam.
2. Public API diff is verified and clean.
3. SLoC deltas and comment preservation are documented.
4. Superseded structure is deleted or accounted for in the Deletion Dividend ledger.

### 6. Boundary Verification & Adversarial Grader Audit

Reconsider the boundary:

1. Inspect downward for newly visible coherent concepts and upward for parent or sibling simplifications.
2. Run an independent grader pass against the Done Criteria to ensure zero vacuous completions or unjustified deferrals.
3. Format and emit `report.md` matching the Standardized Architectural Report Schema.

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
| Source Lines of Code (SLoC) | 1,850 SLoC | 1,480 SLoC | -370 (-20%) | `measure.py` (stripped) |
| Doc Comment Lines (Anti-Gaming) | 420 lines | 445 lines | +25 (+6%) | Preserved & expanded |
| Public API Footprint (IKL) | 34 items | 14 items | -20 (-59%) | `cargo-public-api` |
| Elevated CRAP Functions (>8.0) | 5 functions | 1 function | -4 (-80%) | `cargo-crap` |
| High CRAP Functions (>15.0) | 2 functions | 0 functions | -2 (-100%) | `cargo-crap` |
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
