---
name: rust-design
description: >
  Audit Rust architecture across a module, crate, or workspace; review or
  redesign a focused responsibility or seam; model an established Rust design.
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

For a focused review or redesign, the requested scope is a **starting search
region**, not an assumed architectural boundary. For an audit, it is a
**coverage boundary**: account for every in-scope coarse module and seam before
concluding, while following cross-boundary callers and dependencies. A
modeling-only task starts from its established seam.

It may be a:

- workspace or repository
- crate
- module or submodule
- file
- type or `impl`
- function or method
- coupled cluster spanning several of these

Inspect enough context above, below, and beside the starting scope to understand
its responsibility and seams. Expand when a concern crosses the requested scope;
contract when a higher-level symptom requires lower-level behavioural analysis.

For an audit, inventory each coarse responsibility and intended seam within the
requested scope, starting at crate/module boundaries. Split a file or module
when state or policy serves distinct callers or contracts. Work coarse-to-fine:
examine each unit's interface and relationships, then read candidates deeply.
Uninspected in-scope units are coverage gaps, not negative evidence.

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

For each design claim, obtain source or behavioural evidence with
[`TOOLING.md`](TOOLING.md) and screen its gauge families; consult
[`METRICS.md`](METRICS.md) for selected definitions. Keep underlying facts
beside any count. A proposal predicts effects; only an implemented change
has observed after-state evidence.

Prefer specific caller knowledge and semantic trade-offs to claims such as
"cleaner", "simpler", or "more modular".

## Process

Focused reviews and audits use steps 1-4 and 6 for candidates selected for
deepening; an implementation also completes step 5. A focused review with none
may stop after step 2. An audit with none stops only after every in-scope
inventory unit has been probed and every material tool finding has a recorded
disposition. Enter step 4 directly only when the existing responsibility,
intended seam, callers, and contract are established by source and caller
evidence. Record that evidence; otherwise start at step 1.

### 1. Establish the current design

Read [`TOOLING.md`](TOOLING.md) and map the target's responsibility,
callers, interface, dependencies, important state, invariants, policy,
visibility, tests, and parent/child relationships where they exist. Trace
important control and data flows from caller through seam to implementation.

For an audit, inventory each in-scope responsibility and seam. At module scope
and above, collect both caller/source paths and a configured module and
dependency view; inspect tests entering each intended seam. At workspace scope,
also check for orphan source. Record tool results and gaps per inventory unit.

After running the audit tools in [`TOOLING.md`](TOOLING.md), group each
material tool finding (hotspot, clone, orphan, dependency edge) under its
inventory unit and classify its location as production or test code. Each
group is later investigated, dismissed with a reason, or deferred as a named
gap in the audit report.

**Complete when:** named source and caller observations substantiate the
responsibility and seam map, including where policy and state live and the
inspected boundary. An audit also accounts for distinct ownership clusters
within files/modules and has module/dependency and test-surface evidence. An
inferred graph edge or a directory layout alone does not prove a design claim.

### 2. Discover candidates

Read [`DISCOVERY.md`](DISCOVERY.md). Probe both **expansion** (latent concerns
within one scope) and **compression** (fragmented, redundant, misplaced, or
obsolete abstractions). For an audit, apply both probes to every inventory
unit, recording its inspected neighbours, tool/source observation, and outcome.
Expand investigation when evidence crosses the boundary.

Screen the gauge families in [`TOOLING.md`](TOOLING.md) against each
consequential candidate, then read the selected definitions in
[`METRICS.md`](METRICS.md). Gather current raw inputs and compute a baseline
where the gauge has a current value; for post-change gauges such as `DD`,
inventory existing structure and label projected effects as predictions.
Record the counting basis and why a plausible gauge remains unmeasured after
an evidence attempt. Freeze a representative change scenario before comparison;
disputed units call for raw evidence, not a number.

**Complete when:** both search directions have observed evidence for the
inspected region, every consequential candidate has a direction or reason for
dismissal and a gauge disposition, and unresolved questions are named. For an
audit, every in-scope inventory unit has both outcomes and every material tool
finding is investigated, dismissed with a reason, or recorded as a deferred
gap. Report any coverage gap explicitly instead of claiming the audit
complete. With no supported candidate, report the probes, boundary, and
reasons, then stop.

### 3. Deepen candidates

Read [`DEEPENING.md`](DEEPENING.md). Choose the transformation from the concern.
Treat pieces exposed by decomposition as provisional until they justify their
own responsibility or seam.

An audit may stop after prioritizing its candidates into a ranked shortlist
when the requested outcome is an audit report rather than a redesign; apply
the remaining steps to the selected candidates only. A focused review or
redesign deepens each qualified candidate.

Use `codebase-design`'s **Design It Twice** when the user requests alternative
interface designs. Otherwise, compare viable choices locally and explain a
rejected alternative only when it changes the decision.

Compare each candidate's current caller knowledge and change path with its
proposed seam, selected gauges or raw evidence, effect on any parent, and
complexity it makes removable. Explain material trade-offs without demanding
a numeric result from an inapplicable or unrepeatable gauge.

**Complete when:** the proposal explains what belongs together or apart,
what each seam hides, how callers and any parent change, what becomes removable
or why nothing does, and which effects are predictions rather than observations.

### 4. Model the design in Rust

Read [`MODELING.md`](MODELING.md), then consult relevant portions of
`rust-skills`. Choose Rust constructs from the discovered or established
semantics. Inspect existing construction and visibility paths with
[`TOOLING.md`](TOOLING.md) and screen [`METRICS.md`](METRICS.md) for
representation claims. Enumerate proposed paths when code has not changed;
their guarantees remain predictions. Include type-system complexity in
interface cost: compile-time guarantees are useful when they justify what
callers must learn.

**Complete when:** proposed types, ownership, errors, traits, and visibility
serve the intended seams; each abstraction has a semantic responsibility,
each generic or trait has a demonstrated need, and each new dependency
demonstrates a need that stdlib and already-declared dependencies cannot
meet, verified through rust-docs-mcp per [`TOOLING.md`](TOOLING.md). Claims
about invalid states or exposure have an observed existing path or an
explicit proposed state/path enumeration.

### 5. Verify an implementation

When code was changed, verify required behaviour through the intended seam.
Follow `codebase-design`'s interface-as-test-surface and replace-don't-layer
rules when a deeper interface supersedes shallow ones. Use project checks from
[`TOOLING.md`](TOOLING.md).

Compare observed results and recompute selected gauges from
[`METRICS.md`](METRICS.md) using the same counting basis and frozen scenarios.
If evidence is unavailable, report `not measured` and withhold the claimed
improvement. Explain material regressions and account for obsolete functions,
modules, types, traits, conversions, tests, dependencies, and compatibility
scaffolding. Keep necessary behavior even when a design metric worsens; do not
claim success from a vanity measure alone.

**Complete when:** intended behaviour has been exercised, claimed improvements
have observed evidence, trade-offs are explicit, and superseded structure has
been accounted for.

### 6. Reconsider the boundary

After a meaningful proposal or implementation, inspect downward for a newly
visible coherent concept and upward for a simpler parent or sibling design
where one exists. Revisit discovery only if this exposes a consequential new
candidate. For an audit, reconcile the candidate list against the full
inventory, including cross-boundary dependencies found during investigation.

**Complete when:** the inspected child and parent or sibling boundaries are
named, each new candidate has returned to step 2, and another pass adds no
material candidate or changes no design decision. An audit also accounts for
every in-scope unit and confirms each material tool finding is investigated,
dismissed with a reason, or recorded as a deferred gap. Report remaining
uncertainty and uninspected adjacent regions.

## Output

For every consequential design claim, report the source or tool query and its
observed result, selected gauges with current raw inputs and baselines where
defined, counting bases, and any projected-only effects. State why no gauge
applies or an applicable one remains unmeasured after an evidence attempt.
Separate current evidence, predictions, and observed after-state effects.

For a focused review, report the current responsibility and seam map,
expansion and compression probes, candidates or why none qualified, proposed
transformations where applicable, trade-offs, and the inspected boundary. For
an audit, also give the in-scope inventory with each unit's tool-backed probes,
test surface, candidate disposition, the disposition of every material tool
finding (investigated, dismissed with reason, or deferred), and any coverage
gaps or omitted tool families with reasons; end with a prioritized shortlist
of candidates. For modeling-only work, show the established seam and
caller evidence, chosen Rust representation, and applicable checks.

For an implementation, also report changed seams and behavior, verification
through those seams, comparable observed effects, and superseded structure.

Prioritize by expected leverage, locality, correctness, and misplaced knowledge,
not by vanity measures or aggregate scores.
