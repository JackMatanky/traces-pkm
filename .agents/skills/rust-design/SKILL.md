---
name: rust-design
description: >
  Review or redesign Rust responsibilities and module seams, or model an
  established structural design in Rust.
---

# Rust Design

Discover and deepen Rust designs.

Concentrate coherent behaviour, policy, state, and invariants behind well-placed
seams so callers learn less, maintainers change less, and Rust enforces useful
constraints without making interfaces harder to use.

A successful redesign may decompose, consolidate, collapse, relocate, model,
replace, or remove code. **Deepening** is the objective; no particular
transformation is preferred.

## Foundations

Load `codebase-design` before analysing the target. Its definitions of
**Module**, **Interface**, **Implementation**, **Seam**, **Adapter**, **Depth**,
**Leverage**, and **Locality** are authoritative.

Use `rust-skills` only after a design concern or latent concept has been
discovered. Consult the categories relevant to the candidate rather than
applying its rules as a checklist.

The responsibilities are distinct:

- `codebase-design` defines what makes a design deep.
- `rust-design` discovers what design should exist and how responsibilities and
  seams should change.
- `rust-skills` guides how that design should be represented and implemented in
  Rust.

Treat comments, documentation, ADRs, names, and directory layout as evidence of
intended design, not proof of actual responsibility or seam placement. Validate
design claims against behaviour, dependencies, state, callers, tests, and source
structure.

## Scope

For discovery or redesign, the requested scope is a **starting search region**,
not an assumed architectural boundary. A modeling-only task starts from its
established seam instead.

It may be a:

- workspace or repository
- crate
- module or submodule
- file
- type or `impl`
- function or method
- coupled cluster spanning several of these

Inspect enough context above, below, and beside the starting scope to understand
its responsibility and seams.

Expand when the concern crosses the requested scope. Contract when a
higher-level symptom requires lower-level behavioural analysis.

For broad scopes, work coarse-to-fine: identify candidate regions before deeply
reading their implementations.

## Evidence discipline

Judge designs by knowledge and behaviour:

- what callers must know
- which invariants and policies a module owns
- where state and behaviour belong together
- where change propagates
- what representations leak through seams
- whether responsibilities cohere
- how much capability a seam provides
- whether lower-level depth simplifies higher levels
- what existing complexity becomes unnecessary

Tool findings and quantitative properties are **indicators**. They may identify
where to inspect or help test a hypothesis.

Examples include:

- complexity
- duplication
- coverage
- mutation results
- CRAP scores
- fan-in and fan-out
- dependency cycles
- graph centrality
- visibility
- file size
- function length
- file, type, function, or module counts

None of these establishes design quality by itself. Do not optimize them as
architecture metrics or combine them into an architecture score.

A large cohesive implementation may be correctly shaped. Several smaller,
clearly named cohesive files may be better when they represent genuinely
distinct responsibilities. Size and count never decide between them.

For a concrete design claim, read [`METRICS.md`](METRICS.md) when a comparison
would clarify the decision. Record current evidence and freeze any change
scenario before comparing alternatives. An unimplemented proposal predicts
possible effects; only an implemented change has observed after-state evidence.

Prefer specific caller knowledge and semantic trade-offs to claims such as
"cleaner", "simpler", or "more modular".

## Process

For discovery or redesign, steps 1–4 and 6 apply to reviews and implementations;
an implementation also performs step 5. When the responsibility and seam are
already established, confirm their current contract and enter step 4 directly,
then perform step 5 if code changes. Reviews label expected effects as
predictions.

### 1. Establish the current design

Map the target's responsibility, callers, interface, dependencies, important
state, invariants, policy, visibility, tests, and parent/child relationships
where they exist.

Trace important control and data flows far enough to locate knowledge ownership.
For broad scopes, narrow candidate regions before inspecting implementations.

**Complete when:** the inspected region and adjacent callers or owners are
named, and the target's responsibilities, seams, and important relationships can
be explained without mistaking filesystem layout for design.

### 2. Discover candidates

Read [`DISCOVERY.md`](DISCOVERY.md). Search both **expansion** (latent concerns
within one scope) and **compression** (fragmented, redundant, misplaced, or
obsolete abstractions). Use [`TOOLING.md`](TOOLING.md) for structural mapping
when it answers a specific question.

For a concrete hypothesis, select evidence that could change the decision. Use
[`METRICS.md`](METRICS.md) when a comparison helps; freeze any representative
change scenario before comparing alternatives.

**Complete when:** consequential candidates in the inspected region have
evidence and a plausible direction or a reason for dismissal; both directions
were considered; and unresolved questions are recorded. Expand when evidence
crosses the region's boundary. If none is supported, report the inspected
boundary and reasons, then stop without inventing a redesign.

### 3. Deepen candidates

Read [`DEEPENING.md`](DEEPENING.md). Choose the transformation from the concern.
Treat pieces exposed by decomposition as provisional until they justify their
own responsibility or seam.

Use `codebase-design`'s **Design It Twice** when the user requests alternative
interface designs. Otherwise, compare viable choices locally and explain a
rejected alternative only when it changes the decision.

Evaluate local depth, simplification of parents and higher levels where they
exist, and complexity the design makes removable, if any. Explain material
trade-offs without requiring every proposal to produce a numeric result.

**Complete when:** the proposal explains what belongs together or apart, what
each seam hides, the effect on its callers and any parent, what becomes
removable or why nothing does, and which claims are supported by current
evidence versus still predicted.

### 4. Model the design in Rust

Read [`MODELING.md`](MODELING.md), then consult relevant portions of
`rust-skills`. Choose Rust constructs from the discovered or established
semantics. Include type-system complexity in interface cost: compile-time
guarantees are useful when they justify what callers must learn.

**Complete when:** proposed types, ownership, errors, traits, and visibility
serve the intended seams; each abstraction has a semantic responsibility and
each generic or trait has a demonstrated need.

### 5. Verify an implementation

When code was changed, verify required behaviour through the intended seam.
Follow `codebase-design`'s interface-as-test-surface and replace-don't-layer
rules when a deeper interface supersedes shallow ones. Use project checks from
[`TOOLING.md`](TOOLING.md).

Compare observed results and recompute any selected gauges from
[`METRICS.md`](METRICS.md) using the same counting basis and frozen scenarios.
Explain material regressions and account for obsolete functions, modules, types,
traits, conversions, tests, dependencies, and compatibility scaffolding. Keep
necessary behavior even when a design metric worsens; do not claim success from
a vanity measure alone.

**Complete when:** intended behaviour has been exercised, claimed improvements
have observed evidence, trade-offs are explicit, and superseded structure has
been accounted for.

### 6. Reconsider the boundary

After a meaningful proposal or implementation, inspect downward for a newly
visible coherent concept and upward for a simpler parent or sibling design where
one exists. Revisit discovery only if this exposes a consequential new
candidate. Stop when another pass yields no material change to the candidate set
or design decision; report any remaining uncertainty and uninspected adjacent
regions.

## Output

For a review, report the current responsibility and seam map, evidence-backed
candidates or why none qualified, proposed transformations where applicable,
relevant alternatives, predicted effects on callers and any parent, justified
Rust modeling where needed, trade-offs, and uncertainty. State the inspected
boundary. Report current baselines or frozen scenarios only when useful; label
every unimplemented effect as a prediction.

For modeling-only work, report the established seam, chosen Rust representation,
trade-offs, and verification if code changed.

For an implementation, also report changed seams and behavior, verification
through those seams, comparable observed effects, and superseded structure.

Prioritize by expected leverage, locality, correctness, and misplaced knowledge,
not by vanity measures or aggregate scores.
