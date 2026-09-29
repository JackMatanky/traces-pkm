# Tooling

Ask what evidence a design claim needs, then choose the least costly available
tool. For indexed code navigation in this repository, start with CodeGraph; use
the language server for exact definitions and references, then source, callers,
and tests to verify semantics. Use tools to offload mechanical work, not to
decide architectural quality.

## Choose evidence by question

| Question | First evidence | Escalate when needed |
|---|---|---|
| Who calls this and where does its state or policy live? | CodeGraph for flow; LSP for definitions and references | Inspect relevant source and callers for semantic ownership |
| Is a seam bypassed or a dependency direction wrong? | Graph paths, imports, and intended entry points | Existing module/dependency graph tasks for structure |
| Does a representation permit invalid states or operations? | Enumerated state and transition model | Behavior tests for an implementation |
| Will a change cross parent seams? | Frozen change scenario and graph impact | Inspect affected responsibilities and source paths |
| Did a redesign leave obsolete structure? | Changed interfaces, references, and dependency declarations | Configured orphan or unused-dependency checks |
| Does the implemented seam preserve behavior? | Project build and tests through the intended interface | Scoped coverage or mutation checks for uncertain policy |

Complexity, duplication, CRAP, coverage percentage, size, counts, and graph
centrality are candidate indicators. They suggest where to inspect but do not
enter a design acceptance gate. Graph edges may be inferred; confirm any
material relationship in source or executable behavior. A simpler graph is not
inherently a deeper design.

## Evidence for selected gauges

Use [`METRICS.md`](METRICS.md) to select gauges for the design claim. Tool
output supplies inputs, not a verdict; keep the underlying lists or scenarios.

| Gauges | Inspect first |
|---|---|
| `IKL`, `KC`, `PS`, `L` | Caller obligations, interface types, and parent knowledge before and after |
| `BR`, `VE`, `GPD` | Intended entry points, call/import paths, visibility and re-exports, generic propagation |
| `PD`, `DD` | Frozen change path and superseded seams, representations, coordination, dependencies |
| `V` | Meaningful implementations of the proposed seam |
| `ISR`, `ITE` | Exactly enumerated states and transitions |
| `TR`, `MA` | Tests that cross the intended seam; scoped behavior-changing mutants when useful |

Use an available code graph or configured module view for mechanical paths, then
check architectural claims against source and callers. Use scoped coverage or
mutation tools only if verification uncertainty warrants their cost.

## Use project-available commands

Inspect `mise://tasks` and the active worktree's tool configuration before
naming a command. In this repository route builds, tests, formatting, lint, and
audits through `run_task`; `verify` is the completion gate for a non-trivial
implementation. Use configured module views such as `modules:tree`,
`modules:deps`, and `modules:orphans` when their evidence answers the question.

Only use a specialized tool if it is installed and its output addresses an
unresolved claim. Examples are module hierarchy inspection, public API
comparison, scoped coverage, and scoped mutation testing. If a suggested tool is
unavailable, use existing source and project tasks; an additional tool is not a
prerequisite to finish a design review.

## Verify changes, not proposals

For an implementation, run the project's required checks and exercise the
changed behavior through its intended seam. Compare only evidence selected
before the redesign, with the same counting basis and frozen scenarios. For a
review, state what a future check would distinguish; do not report it as run.

Use expensive behavioral tools only when a candidate requires their evidence. A
design session does not become stronger merely by running more tools.
