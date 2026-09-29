---
name: rust-design
description: >
  Rust design and deepening. Use when reviewing or redesigning a Rust
  codebase, crate, module, file, type, function, or coupled cluster; finding
  design candidates or technical debt; discovering latent concepts and seams;
  consolidating shallow abstractions; or when another skill needs Rust-specific
  structural design.
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

The requested scope is a **starting search region**, not an assumed
architectural boundary.

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

When comparing a concrete redesign, read [`METRICS.md`](METRICS.md). Baseline
only the metrics corresponding to the candidate's design claim. Freeze any
scenario used for comparison before changing the design.

Prefer explicit before/after evidence and semantic trade-offs over claims such
as "cleaner", "simpler", or "more modular".

## Process

### 1. Establish the current design

Map the target's:

- observable responsibility
- callers
- interface
- dependencies
- important state
- invariants and policy
- visibility
- tests and test seams
- parent and child relationships

Trace important control and data flows far enough to locate where knowledge
actually lives.

For broad scopes, use structural tooling to narrow the search before inspecting
individual implementations.

**Complete when:** the target can be explained in terms of responsibilities,
seams, knowledge ownership, and important relationships without treating the
filesystem layout as the design.

### 2. Discover candidates

Read [`DISCOVERY.md`](DISCOVERY.md).

Search both directions:

- **expansion** — behaviour that must be separated provisionally to expose
  latent concerns
- **compression** — fragmented, redundant, misplaced, or obsolete abstractions
  that should be consolidated, collapsed, relocated, replaced, or removed

Include implicit domain concepts, leaked implementation knowledge, misplaced
seams, duplicated policy, invalid representable states, and technical debt that
a deeper design could eliminate.

Use [`TOOLING.md`](TOOLING.md) to offload structural mapping and mechanical
analysis where useful.

Once a candidate has a concrete design hypothesis, use
[`METRICS.md`](METRICS.md) to choose only the dimensions that can clarify that
hypothesis. Record their baseline and freeze any representative change scenario
before proposing the redesign.

**Complete when:** every material candidate in the investigated scope has
concrete evidence, a design concern, a plausible transformation direction,
enough surrounding context to judge its architectural level, and—where
comparison will be used—a stable baseline.

### 3. Deepen candidates

Read [`DEEPENING.md`](DEEPENING.md).

Choose the transformation from the concern. Treat pieces exposed by
decomposition as provisional until they prove they deserve independent
responsibility or a seam.

For consequential interface choices, use `codebase-design`'s **Design It Twice**
process.

Evaluate:

- **local depth** — what meaningful knowledge the candidate hides from its
  immediate callers
- **depth propagation** — what lower-level knowledge disappears from its parent
  and higher levels
- **deletion dividend** — what existing complexity becomes unnecessary if the
  design is correct

For substantial redesigns, use the comparative gate in
[`METRICS.md`](METRICS.md). A design need not improve every applicable metric,
but every material regression requires an explicit semantic or correctness
trade-off.

**Complete when:** the proposal explains what belongs together or apart, what
each seam hides, how the parent becomes simpler, what becomes removable, and how
the relevant evidence supports the redesign or exposes its trade-offs.

### 4. Model the design in Rust

Read [`MODELING.md`](MODELING.md).

Now consult the relevant portions of `rust-skills`.

Choose Rust constructs from the semantics already discovered. Treat type-system
complexity as part of the interface cost: compile-time guarantees are valuable
only when their leverage justifies what callers must learn.

**Complete when:** every introduced Rust abstraction has a semantic
responsibility, every trait or generic abstraction has demonstrated need, and
the type, ownership, error, and visibility design implements the intended seams.

### 5. Verify and compare

Use the smallest useful set of tools from [`TOOLING.md`](TOOLING.md).

Verify behaviour through the deepest meaningful seam. When deeper modules
replace shallow ones, apply `codebase-design`'s interface-as-test-surface and
replace-don't-layer rules.

Recompute the applicable values from [`METRICS.md`](METRICS.md) using the same
counting basis and frozen scenarios.

Apply the comparative gate:

- required behaviour and correctness are preserved
- at least one claimed design dimension materially improves
- every material regression is explicitly justified
- superseded structure is accounted for
- no claimed improvement depends only on a vanity measure

Account for obsolete functions, modules, types, traits, conversions, tests,
dependencies, configuration, and compatibility scaffolding as appropriate. These
are things to account for, not quantities to minimize.

**Complete when:** the redesign's claims are supported by comparable evidence,
its trade-offs are explicit, intended behaviour remains verified, and no
material regression in depth, locality, or correctness remains unexplained.

### 6. Recurse

Inspect both directions after each meaningful deepening.

**Downward:** does the implementation still contain coherent knowledge that
deserves discovery?

**Upward:** has hiding lower-level knowledge exposed a simpler design for the
parent or its siblings?

Continue while further work can meaningfully improve knowledge placement.

**Complete when:** remaining seams correspond to coherent responsibility or real
variation, further decomposition exposes no useful concept, further
consolidation would merge independently changing knowledge, and remaining
complexity is intrinsic to the responsibility that owns it.

## Output

When reviewing rather than implementing, report:

1. the current responsibility and seam map at the relevant scale
2. evidence-backed candidates
3. applicable baseline metrics or scenarios where useful
4. proposed transformations and alternatives considered
5. resulting module and seam relationships
6. what becomes simpler, private, consolidated, replaced, or removable
7. justified Rust modeling decisions
8. before/after evidence and explicit trade-offs
9. remaining uncertainty

Prioritize candidates by expected leverage, locality, correctness, and misplaced
knowledge—not by vanity measures or aggregate scores.
