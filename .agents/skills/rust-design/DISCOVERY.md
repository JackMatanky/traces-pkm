# Discovery

Find design candidates before choosing abstractions.

Discovery asks where knowledge is misplaced, fragmented, duplicated, implicit,
or unnecessarily exposed.

Work at the scale of the requested scope, then traverse upward or downward as
the evidence requires.

## Traverse scales deliberately

### Workspace or repository

For an audit, inventory every in-scope coarse responsibility or seam; start
at crate/module boundaries and split distinct ownership clusters. Use configured
module tree/dependency views and caller/test entry paths from
[`TOOLING.md`](TOOLING.md) to map:

- crates and dependency direction
- major external and internal seams
- important shared domain types
- cross-crate call and data flows
- central or highly connected areas
- cycles and unexpected dependencies
- repeated orchestration or policy
- major test surfaces
- broad structural hotspots

Probe each inventory unit before concluding; read candidate implementations
deeply rather than treating a tool graph as the design.

### Crate or module

Inspect:

- the interface presented upward
- child modules and sibling relationships
- imports and dependency direction
- visibility and re-exports
- state and invariant ownership
- shared domain vocabulary
- callers bypassing the intended seam
- tests and their entry points
- policy distributed across children or callers

Descend where relationships need lower-level explanation. For an audit, keep
distinct state or policy clusters with different callers or contracts as
separate inventory entries, even when they share a file or module.

### File, type, or `impl`

Determine its architectural role rather than assuming the file is one Module.

Inspect:

- state owned or borrowed
- invariants maintained
- operations that change together
- dependencies used by subsets of behaviour
- vocabulary used by different regions
- conversions into and out of represented state
- callers that know representation details
- responsibilities distributed into nearby files

Several files may implement one Module. One file may contain several candidate
Modules.

### Function or method

Inspect the owner and callers before extracting anything.

Look for:

- mixed abstraction levels
- independent state or invariant clusters
- policy mixed with mechanism
- orchestration duplicated elsewhere
- conditionals encoding states or transitions
- primitives carrying unstated domain semantics
- results that require callers to finish the same operation manually
- dependencies relevant to only part of the behaviour

Function length can direct attention but cannot justify decomposition.

## Search both directions

Deepening candidates appear through **expansion** and **compression**.

### Expansion candidates

One scope may contain multiple coherent concerns whose structure is hidden.

Evidence includes:

- independent invariants
- distinct state or lifecycle
- distinct domain vocabulary
- policy mixed with low-level mechanism
- unrelated dependency subsets
- hidden state transitions
- one interface exposing unrelated capabilities
- different behaviours changing for different reasons

These may justify provisional decomposition to expose what is actually present.

The result of decomposition is evidence, not automatically a permanent set of
functions or modules.

### Compression candidates

Several pieces may collectively represent too little.

Look for:

- forwarding functions
- one-to-one delegation chains
- pass-through modules
- wrappers that expose nearly everything they hide
- helpers whose callers must know execution order
- multiple types representing temporary fragments of one operation
- duplicated orchestration
- distributed policy
- sibling modules that constantly change together
- single-purpose abstractions without meaningful leverage
- unnecessary abstraction layers left by previous refactors

These may justify consolidation, collapse, relocation, replacement, or removal.

## Find latent concepts

Look for concepts already implemented implicitly.

Signals include:

- primitives with distinct semantics
- repeated validation of the same value
- boolean or `Option` combinations representing closed states
- invalid states representable in ordinary data
- repeated state-transition conditionals
- operations acting on the same conceptual subset of state
- repeated conversions around one semantic boundary
- data and operations jointly enforcing one invariant
- a vocabulary that appears repeatedly but has no explicit owner

Do not choose the Rust representation yet.

Record the semantics first.

## Find misplaced seams

Inspect where knowledge crosses architectural levels.

Look for:

- callers constructing internal representations
- parents coordinating child ordering requirements
- storage, serialization, hashing, protocol, or framework types escaping upward
- child-specific failures leaking through unrelated parent interfaces
- callers bypassing an intended seam through descendant imports
- broad visibility compensating for unclear ownership
- tests manipulating internals across the intended seam
- sibling modules coupled through implementation state
- abstractions whose primary purpose is forwarding dependencies

Ask whether the seam is missing, misplaced, too wide, or unnecessary.

## Find removable debt

Every redesign search should include subtraction.

Look for:

- shallow abstractions superseded by another owner
- obsolete compatibility scaffolding
- parallel representations of the same concept
- production seams introduced only for tests
- traits or generic parameters without meaningful variation
- conversions whose source or destination can disappear
- wrappers preserved after earlier redesigns
- apparently unreachable or bypassed code
- dependencies used only by obsolete structure
- reimplemented stdlib or already-declared dependencies
- duplicated configuration or policy

Confirm reachability across `cfg` and feature combinations, macro-generated uses,
and public downstream callers before proposing removal. A direct descendant import
may be legitimate when it is itself an intended entry point.

Removal is a candidate transformation, not deferred cleanup.

## Inspect depth propagation

For every promising lower-level candidate, inspect its parent.

Ask:

- Which concepts would the parent stop understanding?
- Which invariants would move behind the child seam?
- Which dependencies would disappear from the parent?
- Would the parent operate at a more coherent abstraction level?
- Would changes remain below the parent seam?
- Could several parent operations reuse one implementation of policy?

A locally tidy abstraction with no useful effect on its callers is weak evidence
for another seam.

## Use indicators correctly

Indicators choose **where to investigate** or help test a design hypothesis.

Useful indicators include:

- complexity
- duplication
- coverage
- mutation escapes
- CRAP
- fan-in and fan-out
- dependency cycles
- visibility
- graph centrality
- file and function size
- counts of modules, files, types, or functions

No threshold determines a design transformation.

Use an indicator to ask a question:

```text
high complexity
→ what knowledge is entangled here?

duplication
→ is the same policy or only similar syntax repeated?

high fan-out
→ is responsibility broad, or is the seam leaking mechanisms?

large file
→ is it incohesive, or merely a large cohesive implementation?

many tiny modules
→ do they hide distinct knowledge, or fragment one responsibility?
```

## Evidence for a candidate

Record the concrete caller knowledge, leaked representation, misplaced policy,
or change path behind the concern. Screen the gauge families in
[`TOOLING.md`](TOOLING.md) for each consequential candidate and use
[`METRICS.md`](METRICS.md) for selected definitions. Keep raw observations
beside any count. Freeze a representative change scenario before comparing
alternatives; preserve its basis if implementation is later measured.

## Search and candidate record

For each audit inventory unit, or the inspected region of a focused review,
record adjacent callers or owners and both expansion and compression/removal
probes with their tool/source, observation, and outcome. Account for units with
no candidate as well as those with one; list any uninspected unit as a coverage
gap rather than a negative result. Group material tool findings (hotspots,
clones, orphans, suspicious dependency edges) under their inventory unit, and
mark each investigated, dismissed with a reason, or deferred as a named gap;
a production/test location classification accompanies duplication findings.

For each consequential candidate, record:

```text
current seam and caller knowledge at issue:
proposed direction and parent effect (if a parent exists):
what could become removable (if anything):
selected gauges, raw evidence, and counting basis:
plausible inapplicable or not-measured gauges and reasons:
uncertainty:
```

A review records expected effects as predictions. An implementation may later
add observations using the same comparison basis. Describe the design problem
before selecting a Rust pattern.

## Completion criterion

Discovery is complete for a focused review when consequential candidates
found in the inspected region are carried forward with evidence or dismissed
with a reason, both probes have observed results, and adjacent callers or
owners checked are named. For an audit, those conditions apply to every
in-scope inventory unit and every material tool finding carries its
disposition. Expand investigation when evidence crosses the boundary; record
unresolved questions and uninspected adjacent regions.

The number of findings is not a success measure.
