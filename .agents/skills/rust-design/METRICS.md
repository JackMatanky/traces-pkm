# Metrics

## Contents

1. [Overview & Measurement Discipline](#overview--measurement-discipline)
2. [Claim Routing & Gauge Selection](#claim-routing--gauge-selection)
3. [Knowledge Atoms & Interface Knowledge Load (`IKL`)](#knowledge-atoms--interface-knowledge-load-ikl)
4. [Knowledge Compression (`KC`) & Parent Simplification (`PS`)](#knowledge-compression-kc--parent-simplification-ps)
5. [Cross-Seam Leakage (`L`) & Seam Bypass Ratio (`BR`)](#cross-seam-leakage-l--seam-bypass-ratio-br)
6. [Change Propagation Distance (`PD`) & Deletion Dividend (`DD`)](#change-propagation-distance-pd--deletion-dividend-dd)
7. [Source Lines of Code (`SLoC`) & Anti-Gaming Guardrails](#source-lines-of-code-sloc--anti-gaming-guardrails)
8. [Risk & Structural Gauges: CRAP, Public API Footprint, Recursive Depth](#risk--structural-gauges-crap-public-api-footprint-recursive-depth)
9. [Typestate, States, & Transitions: `ISR` & `ITE`](#typestate-states--transitions-isr--ite)
10. [Test Reachability (`TR`) & Mutation Acceptance (`MA`)](#test-reachability-tr--mutation-acceptance-ma)
11. [Isolated Semantic Gauges & Anti-Gaming](#isolated-semantic-gauges--anti-gaming)

---

## Overview & Measurement Discipline

Use architectural metrics to make comparisons explicit and repeatable.
Metrics support design reasoning; they do not define good design. Prefer before/after comparisons over universal targets.

### Isolated Semantic Gauges

Never combine metrics into an aggregate score. Each gauge measures an isolated semantic dimension. When comparing designs, evaluate each gauge independently.

### Measurement Record

For each consequential design claim, screen the gauge families against its semantics, select applicable gauges, and gather current raw inputs from available source or tool evidence:

```text
responsibility and intended seam:
claim, current raw evidence, and source/tool observation:
selected gauges and counting basis, or reason none apply:
frozen scenario (if applicable):
proposed effect (prediction until implemented):
observed effect (only after implementation):
trade-offs and uncertainty:
```

Freeze representative scenarios before comparing alternatives. Use `N/A` for an inapplicable gauge or a zero denominator, `not measured (tool unavailable)` for missing evidence, and `0` only for an observed zero.

---

## Claim Routing & Gauge Selection

| Claim Type | Applicable Gauges | Primary Evidence Source |
|---|---|---|
| Seam width & caller burden | `IKL`, `KC`, `PS`, `L` | Public API items, `cargo-public-api`, LSP references |
| Boundary integrity & encapsulation | `BR`, `VE`, `GPD` | Import paths, `cargo-modules dependencies`, re-exports |
| Impact of changes across seams | `PD` | Frozen change scenario evaluated against seam boundaries |
| Structure made obsolete & simplified | `DD`, `SLoC` delta | Removed types, collapsed modules, `cargo tree -d` |
| Risk & test fragility | `CRAP`, `TR`, `MA` | `cargo-crap`, test coverage, mutation runs |
| Invariant & state representation | `ISR`, `ITE` | Typestates, enum state machines, compile-time invariants |

---

## Knowledge Atoms & Interface Knowledge Load (`IKL`)

A **knowledge atom** is one independently necessary fact a legitimate caller must know to use a seam correctly.

Classify atoms as:

- `type`: domain concepts or representations the caller must understand
- `invariant`: preconditions or validity rules the caller must preserve
- `order`: ordering, lifecycle, or transition requirements
- `error`: failure distinctions on which the caller must act
- `config`: configuration or environmental requirements
- `ownership`: ownership, borrowing, lifetime, concurrency, or aliasing constraints
- `performance`: performance characteristics required for correct use
- `leak`: lower-level implementation concepts exposed across the seam

### `IKL` Vector

For seam `S`, record the vector:

```text
IKL(S) = (K_type, K_invariant, K_order, K_error, K_config, K_ownership, K_performance, K_leak)
```

Preserve the vector when comparing designs. Equal totals can represent very different interfaces.

---

## Knowledge Compression (`KC`) & Parent Simplification (`PS`)

For child `C` and parent `P`, isolate the knowledge in `P` concerning the responsibility being moved behind `C`.

```text
KC(C -> P) = 1 - K_after / K_before
```

- `KC > 0`: parent knowledge decreased
- `KC = 0`: no knowledge compression
- `KC < 0`: parent burden increased

### Parent Simplification (`PS`)

```text
PS(C -> P) = K_before - K_after
```

`PS` is a signed net change: positive means fewer parent knowledge atoms.

---

## Cross-Seam Leakage (`L`) & Seam Bypass Ratio (`BR`)

### Cross-Seam Leakage (`L`)

`L(S)` is the number of lower-level concepts unnecessarily exposed across `S`: storage types, serialization representations, descendant state types, child-specific errors.

```text
Delta L = L_after - L_before
```

### Seam Bypass Ratio (`BR`)

For intended seam `S`, count external caller-to-item access paths:

- `E_bypass`: recorded paths entering descendants without an intended entry point
- `E_in`: all recorded paths entering the subtree, including intended paths

```text
BR(S) = E_bypass / E_in
```

`BR = 0` means all external access respects the intended seam.

---

## Change Propagation Distance (`PD`) & Deletion Dividend (`DD`)

### Change Propagation Distance (`PD`)

For representative change scenario `q`:

```text
PD(q) = number of intended architectural seams crossed by the minimal correct change
```

Freeze `q` before comparing designs. Count architectural seams crossed, not files touched.

### Deletion Dividend (`DD`)

A redesign must account for structure made obsolete by its new ownership:

```text
DD = (S, R, C, D)
```

- `S` = superseded seams or abstraction layers
- `R` = redundant semantic representations
- `C` = duplicated coordination or policy sites
- `D` = dependencies required only by superseded design (`cargo tree`)

Keep `DD` as a vector; do not sum or weight dimensions into a single score.

---

## Source Lines of Code (`SLoC`) & Anti-Gaming Guardrails

Size metrics must measure **Source Lines of Code (SLoC)**: non-comment, non-blank lines of Rust source code.

### Anti-Gaming Guardrails

- **Comment Preservation Rule:** Deleting, stripping, or compacting doc comments (`///`, `//!`), inline explanations, or rustdoc examples to artificially reduce line counts is strictly prohibited. Any proposal that decreases comment-to-code ratios without justification fails verification.
- **Semantic Deletion Dividend:** The deletion dividend evaluates the removal of architectural complexity (dead types, obsolete traits, collapsed wrappers), not explanatory prose.

---

## Risk & Structural Gauges: CRAP, Public API Footprint, Recursive Depth

### Change Risk Anti-Pattern (`CRAP`)

$$\text{CRAP}(f) = \text{comp}(f)^2 \cdot (1 - \text{cov}(f))^3 + \text{comp}(f)$$

- **Elevated Risk ($\text{CRAP} > 8.0$):** Notable complexity or testing deficiency; early warning for seam decomposition.
- **High Risk ($\text{CRAP} > 15.0$):** Primary refactoring and regression hazard; requires dedicated tests entering the intended seam before restructuring. Gathered via `cargo-crap`.

### Public API Footprint (`IKL_pub`)

The exact count of publicly exported items (`pub fn`, `pub struct`, `pub trait`, `pub enum`, `pub type`) measured via `cargo-public-api`.

- A deep module refactoring must preserve or compress `IKL_pub`.

### Recursive Depth Ratio (`RDR`)

$$\text{RDR}(C) = \frac{\text{Capability Encapsulated by } C}{\text{Knowledge Exposed by } C \text{ to Parent}}$$
When $C$ is recursively deep, $\text{RDR} \gg 1$.

### Graph Edge Density (`GED`)

Total dependency and caller edges divided by node count, gathered via `codegraph` or `rustgraph`. A cohesive decomposition reduces cross-boundary edge density.

---

## Typestate, States, & Transitions: `ISR` & `ITE`

- **Invalid States Representable (`ISR`):** Count of logically invalid states permitted by struct field combinations. Ideal: $\text{ISR} = 0$.
- **Illegal Transitions Expressible (`ITE`):** Count of invalid state transitions permitted by method signatures at compile time. Ideal: $\text{ITE} = 0$ via typestate.

---

## Test Reachability (`TR`) & Mutation Acceptance (`MA`)

- **Test Reachability (`TR`):** Number of tests exercising behavior through the intended seam rather than internal details.
- **Mutation Acceptance (`MA`):** Percentage of mutants killed by seam-level tests (`mutarust`).

---

## Isolated Semantic Gauges & Anti-Gaming

A metric becomes harmful when the implementation is altered to manipulate the number rather than improve domain ownership:

- Replacing semantic operations with an opaque `execute` call does not create depth if callers now need complex payload builders.
- Merging unrelated responsibilities into one module does not create locality.
- Encoding every trivial state into complex typestate hurts readability if `IKL` explodes.
- Hiding child modules behind a forwarding facade does not create a deep seam.

The semantic explanation remains authoritative.
