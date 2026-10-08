# Deepening

## Contents

1. [Overview & Core Principles](#overview--core-principles)
2. [Recursive Depth Decomposition & Depth Propagation](#recursive-depth-decomposition--depth-propagation)
3. [The Recursive Depth Test](#the-recursive-depth-test)
4. [Consolidate Fragmented Responsibility](#consolidate-fragmented-responsibility)
5. [Collapse Shallow Seams](#collapse-shallow-seams)
6. [Redesign a Real Seam](#redesign-a-real-seam)
7. [Relocate Ownership & Model Implicit Concepts](#relocate-ownership--model-implicit-concepts)
8. [Replace the Wrong Abstraction & Deletion Dividend](#replace-the-wrong-abstraction--deletion-dividend)
9. [Protect Sibling Isolation & Contain Vertical Leakage](#protect-sibling-isolation--contain-vertical-leakage)
10. [Transformations & Trade-Off Accounting](#transformations--trade-off-accounting)
11. [Empirical Trial-and-Revert Protocol](#empirical-trial-and-revert-protocol)
12. [The Active Architectural Graveyard](#the-active-architectural-graveyard)
13. [Search Convergence & Budget Gates](#search-convergence--budget-gates)

---

## Overview & Core Principles

Deepening concentrates coherent behaviour, policy, state, and invariants behind a simpler seam. It may require splitting one thing, joining several things, moving responsibility, replacing an abstraction, or deleting dead structure.

Choose the transformation from the concern, not from a desire to apply a Rust pattern.

---

## Recursive Depth Decomposition & Depth Propagation

A central failure of superficial refactoring is "flat layering": introducing intermediate structs or forwarding functions that simply pass calls through without hiding knowledge. The primary structural pillar of this skill is **Recursive Depth**:

> **A module is recursively deep when its internal decomposition concentrates knowledge behind meaningful seams, and those seams make their parent simpler and deeper rather than merely adding layers.**

```text
+------------------------------------------------------------------------------+
| Parent Seam: Unified Domain Concept (Minimal Caller Knowledge Burden)        |
+------------------------------------------------------------------------------+
                                          |
                                          v
+------------------------------------------------------------------------------+
| Deep Internal Child A                    | Deep Internal Child B             |
| - Owns state machine and lifecycle       | - Owns wire encoding and framing  |
| - Hides all transition validation rules  | - Hides ring-buffer memory pool   |
+------------------------------------------------------------------------------+
                                          |
                                          v
+------------------------------------------------------------------------------+
| Low-Level Leaf Operations: System Calls, Network Drivers, Storage I/O        |
+------------------------------------------------------------------------------+
```

### Depth Propagation

For a lower-level candidate with a parent, inspect that parent. What vanished from the parent's required knowledge?

- Child concepts and representations
- Child dependencies and configuration
- Ordering rules and state machine invariants
- Child-specific error hierarchies

Good lower-level design raises the abstraction level of the parent. A locally tidy child that leaves its parent equally complicated does not earn an independent seam.

---

## The Recursive Depth Test

When extracting or reviewing an internal child module $C$ within parent $P$:

1. **Local Depth:** Does $C$'s seam provide substantial capability relative to the small interface it exposes to $P$?
2. **Depth Propagation:** Does $P$'s implementation become shorter, simpler, and higher-level? Did internal states, dependencies, and ordering rules vanish from $P$?
3. **The Anti-Layering Rule:** If $P$ still coordinates $C$'s internal sequence, inspects $C$'s internal states, or maps $C$'s errors directly upward, $C$ is a shallow layer, not a recursively deep module.

---

## Consolidate Fragmented Responsibility

Consolidate when callers currently assemble one concern from several shallow pieces.
Evidence includes:

- Fixed call sequences across call sites
- Shared invariants spread across helpers
- Repeated orchestration or loops
- Several modules constantly changing together
- Intermediate types whose sole purpose is crossing shallow seams

The result must remove knowledge from callers. Moving functions into one file without changing responsibility or seams is not deepening.

---

## Collapse Shallow Seams

Collapse a seam whose interface costs roughly as much knowledge as the behaviour it hides:

- Forwarding wrappers
- One-to-one delegation layers
- Redundant facades
- Helper modules that merely rename operations
- Traits with no meaningful variation or substitution need

Ask: *If the module disappeared, would its complexity reappear in callers, or would the abstraction simply vanish?* If it simply vanishes, delete it.

---

## Redesign a Real Seam

Keep the responsibility but change the seam when the module is meaningful and its callers learn the wrong things:

- Raise parameters or results to domain semantics
- Absorb ordering requirements internally
- Hide storage, serialization, or network representations
- Enforce invariants internally via types
- Replace several mechanism-level operations with one semantic operation
- Narrow visibility

---

## Relocate Ownership & Model Implicit Concepts

### Relocate Ownership

Move behaviour when another module owns the state, invariant, lifecycle, or policy that gives it meaning. Code enforcing an invariant must have direct authority over the state involved. Avoid placing domain logic in "junk drawer" files (`utils.rs`, `helpers.rs`).

### Model Implicit Concepts

Make a concept explicit when doing so concentrates an invariant, eliminates invalid states, gives a seam semantic vocabulary, or eliminates runtime validation checks. Read [`MODELING.md`](MODELING.md) before selecting the Rust representation.

---

## Replace the Wrong Abstraction & Deletion Dividend

### Replace Rather than Layer

Replace rather than incrementally repair when the current abstraction preserves the wrong responsibility or seam. A replacement plan must identify:

- Callers to migrate
- Behaviour to preserve
- Tests whose seam changes
- Types or layers superseded
- Dependencies that become unnecessary

### The Deletion Dividend

A redesign must account for structure made obsolete by its new ownership. Search for obsolete helpers, shallow wrappers, redundant modules, parallel representations, and superseded dependencies. A deeper module should pay a **deletion dividend**.

---

## Protect Sibling Isolation & Contain Vertical Leakage

### Protect Sibling Isolation

When a parent contains several child modules, ensure their knowledge is independent. Frequent cross-sibling changes indicate misplaced boundaries, unowned shared invariants, or leaked representations.

### Contain Vertical Leakage

Prevent lower-level knowledge from escaping upward: descendant types in ancestor interfaces, storage or serialization details, child-specific error structures, or internal state constructors.

---

## Transformations & Trade-Off Accounting

Compare current caller knowledge and change paths with the proposed seam:

- For a review, state expected effects as predictions in `ledger.json`.
- For an implementation, compare observed results against baseline evidence using `diff.py`.
- Document trade-offs, including any additional caller burden accepted to eliminate invalid transitions.

## Empirical Trial-and-Revert Protocol

Architectural breakthroughs require rapid prototyping without accumulating accidental debt. Execute each trial using strict isolation:

1. **Sandboxed State:** Ensure working tree is clean or create an isolated Git worktree or branch (`improve/<run-id>-<trial-id>`).
2. **Explicit Drop Conditions:** For Breakthrough mode, state the boundary condition that aborts the trial before editing (for example: "If prototype requires unsafe pointer casts or fails to reduce heap allocations by at least 30%, drop it").
3. **Time and Attempt Limits:** Cap iterative compilation attempts to at most 3 refinements per candidate.
4. **Clean Rollback:** If the candidate fails tests, breaks external API stability, or triggers its drop condition, revert immediately with `git reset --hard` or worktree disposal. Do not leave partially broken scaffolding in the workspace.

## The Active Architectural Graveyard

The Architectural Graveyard (`.design/.../graveyard.md`) is an active filter for subsequent discovery, not passive post-run documentation:

- Record every abandoned bet with:
  - **Hypothesis ID:** ID from `ledger.json`.
  - **Concept & Intended Gain:** What architectural transformation was attempted.
  - **Concrete Failure Evidence:** Compiler errors, regression deltas, benchmark timings, or API leakages.
  - **Rejection Reason:** Why the approach was fundamentally flawed or unviable.
- Subsequent scout passes consult `graveyard.md` to avoid proposing variants of known dead ends.

## Search Convergence & Budget Gates

The search loop iterates until one of three stopping conditions is met:

1. **Convergence Gate:** All candidate hypotheses with Expected Architectural Value $\text{EAV} \ge 1.0$ have been evaluated and either committed or cleanly rejected.
2. **Plateau Gate:** Two consecutive trials fail to produce an accepted change. The orchestrator triggers an adversarial 10-lens re-scouting pass. If re-scouting produces no new hypotheses above the EAV threshold, the search terminates.
3. **Budget Gate:** The allocated run budget (time limit or maximum trial count) is exhausted.
