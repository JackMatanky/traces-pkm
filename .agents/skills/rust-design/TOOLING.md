# Tooling

Use tools to offload mechanical exploration and verification.

Tools answer factual questions about the current code. The agent remains
responsible for identifying concepts, assigning responsibility, placing seams,
and judging depth.

Tool output is **evidence**, never a design verdict.

## Environment first

Before installing anything:

1. inspect the repository's existing toolchain and scripts
2. detect which relevant commands or MCP tools are already available
3. prefer existing project conventions
4. inspect each tool's current help or tool schema rather than relying on cached
   command-line syntax

If the workspace uses `mise`, prefer its configured environment and tool
versions rather than introducing a parallel tool-management mechanism.

Analysis tools belong in the developer environment unless the repository has a
durable reason to standardize them. Do not add a crate, configuration file,
generated report, hook, or CI gate merely because the design session used a
tool.

Prefer machine-readable or agent-oriented output when the installed tool
supports it.

## Minimize the tool surface

Use the smallest tool set that answers the current questions.

Do not run every analyzer by default.

A broad codebase review may justify several cheap discovery tools. A focused
function or type review may need none beyond normal code navigation.

Use expensive behavioral tools only after a candidate requires their evidence.

## Indicators vs metrics

Some tools identify places worth investigating. Others can help gather inputs
for an architectural metric already defined in [`METRICS.md`](METRICS.md).

Keep these roles separate.

### Candidate indicators

These may direct discovery but do not enter the comparative gate directly:

- `messrust`
- `jscpd`
- `cargo-crap`
- raw complexity
- raw coverage percentage
- file or function size
- graph centrality
- counts of files, modules, functions, or types

They primarily answer:

> Where should I inspect?

### Metric-supporting evidence

These may help calculate or verify a metric selected for a concrete design
hypothesis:

- code knowledge graphs
- `cargo-modules`
- source and caller inspection
- `cargo-public-api`
- `cargo-llvm-cov`
- `mutarust`
- `cargo-shear`

They primarily answer:

> Did this specific design claim hold?

## Knowledge graph

Use an existing code knowledge graph when available to avoid manually
reconstructing cross-file relationships.

Possible backends include:

- CodeGraph
- Graphify
- GitNexus
- another code graph already integrated into the harness

Use one graph backend for ordinary discovery. Add another only when the first
cannot answer a material question or its result needs independent
cross-checking.

Use the graph to offload questions such as:

- callers and callees
- import and dependency relationships
- transitive impact
- paths between symbols
- execution or call flows
- highly connected symbols
- structural communities or clusters
- candidate blast radius

Treat inferred relationships as hypotheses where the backend distinguishes them
from directly extracted edges.

Do not infer design quality from graph density, community count, centrality, or
similar graph statistics.

## `cargo-modules`

Prefer `cargo-modules` for Rust-specific structural evidence.

Use it to inspect:

- module hierarchy
- item visibility
- internal dependencies
- dependency cycles
- unlinked source files

It is especially useful for checking:

- parent/child dependency direction
- sibling coupling
- unexpected visibility
- seam bypass
- whether a redesign changed structure or merely moved code

Re-run the relevant view after a structural redesign.

A simpler graph is not inherently a better design; interpret changes through
responsibility and knowledge containment.

## `messrust`

Use `messrust` as a cheap syntax-level candidate finder.

Relevant findings may include:

- complexity
- design and coupling smells
- code-size anomalies
- unused code
- structural findings

Use findings to select places for inspection.

Determine which design concern, if any, produces a finding before changing the
code.

Prefer focused rulesets over indiscriminately optimizing the entire report.

## `jscpd`

Use `jscpd` to locate repeated source structure.

Duplication is a **concept-discovery prompt**, not an automatic DRY command.

For each meaningful clone, ask:

- Is the same policy repeated?
- Is the same orchestration repeated?
- Do the regions maintain the same invariant?
- Is the syntax similar while semantics differ?
- Would a shared abstraction hide knowledge or merely parameterize variation?

Duplication percentage is not a design metric.

Consolidate only when the duplicated regions represent a coherent shared
concern.

## `cargo-crap`

Use `cargo-crap` to identify functions combining complexity with weak test
coverage.

Treat high-risk results as investigation priorities.

CRAP is a discovery indicator only. Do not use its before/after score in the
architectural acceptance gate.

Splitting a function can lower CRAP while preserving or worsening the actual
design problem.

## Coverage with `cargo-llvm-cov`

Use coverage to answer whether tests execute behaviour affected by a proposed or
completed redesign.

Coverage is particularly useful when:

- replacing tests at shallow seams with tests through a deeper seam
- identifying unexercised branches before restructuring
- checking whether removed tests leave behaviour unexercised

Coverage does not establish that tests assert the right behaviour.

Raw coverage percentage does not enter the architectural acceptance gate.

## Mutation testing with `mutarust`

Use `mutarust` when test strength matters to a seam decision.

It is especially useful after:

- moving tests to a deeper interface
- consolidating several shallow modules
- replacing implementation-coupled tests
- introducing a module that owns important policy or invariants

Interpret escaped mutants from the caller's perspective:

> If this implementation were wrong in this way, what observable contract would
> be violated?

Strengthen tests at the meaningful seam rather than exposing internals merely
for testing.

Mutation testing is comparatively expensive. Scope it to the candidate or
affected production region when a whole-codebase run does not justify its cost.

Use mutation results for `MA` only when mutants and the tests exercising the
candidate can be identified consistently.

Mutation score is evidence about verification strength, not architectural
depth.

## Public API tools

### `cargo-public-api`

Use for library crates when a redesign may alter the externally public Rust
surface.

It can help answer:

- What public items exist before and after?
- Did an internal concept accidentally become public?
- Did consolidation alter the type-level public surface?
- Which externally visible items changed?

The `codebase-design` Interface remains broader than Rust's public item list;
invariants, ordering, errors, configuration, and performance knowledge still
require design analysis.

### `cargo-semver-checks`

Use when compatibility with a previously released library API is a material
constraint.

Treat SemVer compatibility as a constraint on the redesign, not a reason to
preserve weak internals indefinitely.

Where compatibility blocks a stronger target design, distinguish migration
strategy from target architecture.

## `cargo-shear`

Use after structural redesigns to detect residue such as:

- unused dependencies
- misplaced dependencies
- unlinked Rust source files

Use findings to support the dependency portion of `DD`.

An unused dependency is evidence of residue, not architectural success by
itself.

## Existing Cargo checks

Use the project's existing formatting, checking, linting, and test commands.

At minimum, where applicable, verify the same classes of checks the repository
already relies on:

- compilation
- tests
- formatting
- Clippy or configured lints

Use `cargo-nextest` when the workspace already uses it or when it is available
and provides a practical test-running benefit. It accelerates verification; it
does not provide architectural evidence itself.

## Tool-to-metric mapping

Use tools only when they reduce mechanical work required to answer the selected
metric.

| Metric | Useful evidence |
|---|---|
| `IKL` | interface, caller, and source inspection |
| `KC`, `PS` | before/after parent and caller inspection |
| `L` | source inspection, graph, `cargo-modules` |
| `BR` | graph, `cargo-modules` |
| `PD` | graph, dependency analysis, source inspection |
| `DD` | source diff, seam inspection, `cargo-shear` |
| `V` | implementation/adapter discovery |
| `VE` | `cargo-modules`, visibility and caller analysis |
| `GPD` | source inspection, graph, type propagation |
| `ISR` | type/state analysis |
| `ITE` | state-transition interface analysis |
| `TR` | test and source inspection |
| `MA` | scoped `mutarust` |

A tool helps gather inputs; it does not interpret the architectural meaning.

## Tool sequence by question

### Broad candidate discovery

Prefer:

```text
knowledge graph
+ cargo-modules
+ focused messrust
```

Add `jscpd` or `cargo-crap` when duplication or risky complexity is materially
relevant.

### Focused local investigation

Prefer normal code navigation and existing harness tools.

Run another analyzer only when it answers a specific unresolved question.

### Structural redesign

Before and after, compare the relevant:

```text
cargo-modules structure/dependencies
knowledge-graph relationships
visibility
duplication evidence where relevant
```

Interpret differences semantically.

### Test-seam redesign

Use:

```text
ordinary tests
→ coverage if execution is uncertain
→ scoped mutarust if assertion strength is uncertain
```

### Published library seam

Add when relevant:

```text
cargo-public-api
cargo-semver-checks
```

### Consolidation or removal

Finish with:

```text
cargo-shear
existing compiler/lint/test checks
```

## Completion criterion

Tooling has done enough when every invocation answers a concrete design or
verification question and another tool would provide overlapping evidence
rather than change the decision.

A design session does not become stronger merely by running more tools.
