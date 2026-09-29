# Tooling

An architecture audit needs independent structural, caller, and test-surface
evidence before choosing the cheapest tool for a particular claim. A focused
review uses the smallest set that answers its scoped question. For indexed
navigation start with CodeGraph; use LSP for exact definitions and references.
Check graph inferences against source, callers, and tests. Record each tool
query, its result, and any unavailable or omitted evidence family with a reason.

## Choose evidence by question

| Question | First evidence | Escalate when needed |
|---|---|---|
| Who calls this and where does its state or policy live? | CodeGraph for flow; LSP for definitions and references | Inspect relevant source and callers for semantic ownership |
| Is a seam bypassed or a dependency direction wrong? | Graph paths, imports, and intended entry points | Existing module/dependency graph tasks for structure |
| Does a representation permit invalid states or operations? | Enumerated state and transition model | Behavior tests for an implementation |
| Will a change cross parent seams? | Frozen change scenario and graph impact | Inspect affected responsibilities and source paths |
| Did a redesign leave obsolete structure? | Changed interfaces, references, and dependency declarations | Configured orphan or unused-dependency checks |
| Does the implemented seam preserve behavior? | Project build and tests through the intended interface | Scoped coverage or mutation checks for uncertain policy |

## Audit tools

For a module, crate, or workspace architecture audit, run the structural,
hotspot, and duplication tools below, then inspect their findings in source.
For a focused review, use tools that answer its specific claim. Run configured
tasks through `run_task`; call the globally installed `jscpd` CLI directly.

| Invocation (tool) | Evidence and limit |
|---|---|
| `modules:tree`, `modules:deps` (`cargo-modules`) | Module inventory, visibility, and dependency candidates; confirm inferred edges in source. `modules:deps --acyclic` is informational, not a gate: its graph model reports spurious cycles here. |
| `modules:orphans` (`cargo-modules`) | Source files outside the module tree, not unused functions; run for workspace audits and file-removal proposals. |
| `mess` (`messrust`) | Advisory maintainability hotspots, including complexity and tangled responsibilities under `messrust.xml`; exit 2 means findings, not a broken tool. Triage findings against actual ownership, not thresholds alone. |
| `crap` (`cargo-crap`, depends on `coverage:lcov` via `cargo-llvm-cov`) | Per-function complexity/coverage risk; inspect matching audited functions and coverage exclusions before interpreting scores. `coverage:html -m <module>` provides a scoped coverage view when test reach-through needs inspection. |
| `jscpd --format rust --reporters console <paths>` (global `jscpd`) | Rust token-clone sites and duplication rate within the named paths; use `src` for crate-wide scans, or relevant files/modules for focused scans. It has no mise task here. A clone is a lead, not proof of repeated policy: compare semantics and callers. |
| `test:mutants -f <file>` or `-m <module>` (`mutarust`, depends on `test`) | Killed/escaped behavior-changing mutants for candidate code; scope to the responsible file or module, distinguish equivalent/invalid mutants, and check test strength. The report does not name which test killed each mutant, so a raw score cannot establish `MA`; isolate seam tests or report `MA` as `not measured`. This is not a static architecture score. |

Keep `jscpd`'s scanned paths, scan boundary, and clone locations beside its
percentage, and classify each clone's location as production or test code
separately: a whole-file rate can be dominated by inline tests. The scan only
sees the named paths, so it cannot observe clones into siblings; widen the
scan when a claim crosses the boundary. Token clones and semantic duplication
are different claims.

## Audit evidence

At module scope and above, obtain these views before declaring a design audit
complete:

1. **Structure:** run `modules:tree` and `modules:deps` alongside CodeGraph
   flows; at workspace scope also run `modules:orphans`. Confirm material edges
   in source.
2. **Callers and ownership:** use LSP definitions/references and inspect source
   for intended entry paths, bypasses, visibility, state, and policy. If a tool
   misses a known caller, use another source/structural query rather than
   treating an empty result as proof.
3. **Hotspots, duplication, and tests:** run `mess`, `crap`, and `jscpd`
   for a module-level or broader audit; classify each material finding by
   production or test location within the audited scope and identify tests
   entering each intended seam. Run `test:mutants` on consequential
   test-strength claims and isolate seam tests before reporting `MA`. State
   which behaviour was actually verified; a proposal is not a test result.

Use cached Rust crate docs when external API semantics affect the design.
Complexity, duplication, CRAP, coverage percentage, size, counts, and graph
centrality are candidate indicators. They suggest where to inspect but do not
enter a design acceptance gate. A simpler graph is not inherently a deeper
design.

## Evidence for selected gauges

Screen these families for every consequential candidate; read the applicable
definitions in [`METRICS.md`](METRICS.md). Tool output supplies inputs, not
a verdict. Keep the underlying caller facts, paths, or state sets beside each
count and name the query or command that produced them.

| Gauges | Inspect first |
|---|---|
| `IKL`, `KC`, `PS`, `L` | Caller obligations, interface types, and parent knowledge before and after |
| `BR`, `VE`, `GPD` | Intended entry points, call/import paths, visibility and re-exports, generic propagation |
| `PD`, `DD` | Frozen change path and superseded seams, representations, coordination, dependencies |
| `V` | Meaningful implementations of the proposed seam |
| `ISR`, `ITE` | Exactly enumerated states and transitions |
| `TR`, `MA` | Tests that cross the intended seam; scoped behavior-changing mutants when useful |

Derive selected gauges' current inputs from available graph, module, source,
or behavioural evidence. Record failed queries and measurement limits as
specified in [`METRICS.md`](METRICS.md).

## Use project-available commands

Inspect `mise://tasks` and the active worktree's tool configuration before
naming a configured task. Route configured build, test, format, lint, and audit
tasks through `run_task`; `verify` gates non-trivial implementations. Run
the global `jscpd` CLI directly because no mise task wraps it. The repository's
`audit` task checks security, not architecture. If a tool is unavailable,
report the missing measurement and use remaining evidence.

## Verify changes, not proposals

For an implementation, run the project's required checks and exercise the
changed behavior through its intended seam. Compare only evidence selected
before the redesign, with the same counting basis and frozen scenarios. For a
review, state what a future check would distinguish; do not report it as run.

Use expensive behavioral tools only when a candidate requires their evidence. A
design session does not become stronger merely by running more tools.
