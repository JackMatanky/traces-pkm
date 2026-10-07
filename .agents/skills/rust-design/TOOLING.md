# Tooling

## Contents

1. [Overview & Resource Isolation](#overview--resource-isolation)
2. [Three-Tier Fallback Ladder](#three-tier-fallback-ladder)
3. [Universal Tool Guidance & Execution](#universal-tool-guidance--execution)
4. [Choose Evidence by Question](#choose-evidence-by-question)
5. [Audit Views & Evidence Protocol](#audit-views--evidence-protocol)
6. [Evidence for Selected Gauges](#evidence-for-selected-gauges)
7. [Verify Changes, Not Proposals](#verify-changes-not-proposals)

---

## Overview & Resource Isolation

An architecture audit needs independent structural, caller, and test-surface evidence before choosing the cheapest tool for a particular claim. A focused review uses the smallest set that answers its scoped question.

### Resource Isolation Rule

Parallel subagents (e.g. read-only scouting lenses during Step 2) operate strictly in **read-only mode**. Scouts are **forbidden** from running `cargo build`, `cargo test`, or diagnostic scripts, eliminating target directory lock contention. Only the main orchestrator executes CLI tools or test suites.

---

## Three-Tier Fallback Ladder

To ensure deterministic execution across diverse host environments, follow this three-tier capability ladder:

```text
+-------------------------------------------------------------------------------+
| Tier 1: Automated Script (Preferred)                                          |
| Run `uv run .agents/skills/rust-design/scripts/gather.py <target> [out]`      |
| Multi-tool execution, SLoC counting, and normalized JSON output               |
+-------------------------------------------------------------------------------+
                                  | (if script/uv absent)
                                  v
+-------------------------------------------------------------------------------+
| Tier 2: Configured Environment Tasks (Fallback)                               |
| Inspect `mise tasks`, `just --list`, `cargo make`, or `Makefile`              |
| Look for tasks providing: tree, deps, orphans, hotspots, crap, clones         |
+-------------------------------------------------------------------------------+
                                  | (if task runner absent)
                                  v
+-------------------------------------------------------------------------------+
| Tier 3: Standard Cargo & Toolchain Baseline (Universal Minimal)               |
| Structure: `cargo check`, manual `mod.rs`/`lib.rs` inspection, LSP references |
| Quality:   `cargo clippy --all-targets -- -D warnings`                        |
| Testing:   `cargo test`                                                       |
| Dependencies: `cargo tree`, `cargo tree -d`                                   |
| Unmet tool metrics marked explicitly as `not measured (tool unavailable)`     |
+-------------------------------------------------------------------------------+
```

1. **Tier 1 (Automated Helper):** Execute `uv run .agents/skills/rust-design/scripts/gather.py <target> [dest]`. The script auto-detects `mise`, probes `PATH`, extracts SLoC and comment counts, executes available analyzers, queries `codegraph` / `rustgraph`, and normalizes output into condensed JSON.
2. **Tier 2 (Environment Task Runners):** If the helper cannot run, inspect project task runners (`mise`, `just`, `cargo make`). Execute configured aliases (e.g. `modules:tree`, `mess`, `crap`) directly.
3. **Tier 3 (Universal Standard Cargo):** If third-party analyzers are unavailable, fall back to built-in `cargo` commands (`cargo check`, `cargo test`, `cargo clippy`, `cargo tree`). Mark missing analyzer metrics explicitly as `not measured (tool unavailable)` rather than omitting or fabricating them.

---

## Universal Tool Guidance & Execution

| Tool | Capability & Invocation | Evidence & Limit |
|---|---|---|
| `codegraph` | `codegraph status -j`, `codegraph files -j --filter <path>` | Whole-crate node/edge density and per-file symbol counts. Fast SQLite-backed AST graph. |
| `rustgraph` | `rustgraph structure --json` | AST-aware code navigation fallback when `codegraph` is unavailable. |
| `cargo-public-api` | `cargo public-api` / `cargo public-api --diff-git-checkouts <base> <head>` | Precise mathematical count of public API surface (IKL); automated diff proving zero breaking changes. |
| `cargo-modules` | `cargo-modules tree`, `cargo-modules dependencies --lib` | Module hierarchy and dependency edges. Note: cycle detection flags can report spurious cycles on re-exports. |
| `cargo-crap` | `cargo-crap` (requires test coverage data) | Change Risk Anti-Pattern scores (cyclomatic complexity combined with low test coverage). |
| `messrust` | `messrust` | Advisory maintainability hotspots and tangled responsibilities. |
| `jscpd` | `jscpd --format rust --reporters console <paths>` | Token-clone sites and duplicate token clusters. A clone is a lead, not proof of repeated policy. |
| `cargo tree` | `cargo tree`, `cargo tree -d` (duplicate versions) | Built-in Cargo dependency hierarchies and duplicate crate resolution. |

Before anything else, resolve external crate semantics through authoritative crate documentation (local documentation tools if available, or `docs.rs`) rather than recalled knowledge, web search, or guessing from source. Treat remembered API behavior as a hypothesis to verify.

---

## Choose Evidence by Question

| Question | First evidence | Escalate when needed |
|---|---|---|
| Who calls this and where does its state or policy live? | CodeGraph/LSP for flow; definitions and references | Inspect relevant source and callers for semantic ownership |
| Is a seam bypassed or a dependency direction wrong? | Graph paths, imports, and intended entry points | Existing module/dependency graph tasks for structure |
| Does a representation permit invalid states or operations? | Enumerated state and transition model | Behavior tests for an implementation |
| Will a change cross parent seams? | Frozen change scenario and graph impact | Inspect affected responsibilities and source paths |
| Did a redesign leave obsolete structure? | Changed interfaces, references, and dependency declarations | `cargo tree -d` and orphan checks |
| Does the implemented seam preserve behavior? | Project build and tests through the intended interface | Scoped coverage or mutation checks for uncertain policy |

---

## Audit Views & Evidence Protocol

At module scope and above, obtain these views before declaring a design audit complete:

1. **Structure & Topology:** Run `uv run .agents/skills/rust-design/scripts/gather.py <target> [out]` (or `cargo-modules tree` alongside CodeGraph/rustgraph). Confirm material edges in source.
2. **Callers & Ownership:** Use LSP definitions/references and inspect source for intended entry paths, bypasses, visibility, state, and policy. If a tool misses a known caller, use another query rather than treating an empty result as proof.
3. **Hotspots, Duplication, & Risk:** Inspect CRAP risks, complexity hotspots, and token clones. Classify each material finding by production or test location within the audited scope. Identify tests entering each intended seam.

---

## Evidence for Selected Gauges

Screen these families for every consequential candidate; read the applicable definitions in [`METRICS.md`](METRICS.md). Tool output supplies inputs, not a verdict. Keep the underlying caller facts, paths, or state sets beside each count and name the query or command that produced them.

| Gauges | Inspect first |
|---|---|
| `IKL`, `KC`, `PS`, `L` | Caller obligations, interface types, and parent knowledge before and after (`cargo-public-api`) |
| `BR`, `VE`, `GPD` | Intended entry points, call/import paths, visibility and re-exports, generic propagation |
| `PD`, `DD` | Frozen change path and superseded seams, representations, coordination, dependencies (`cargo tree`) |
| `V` | Meaningful implementations of the proposed seam |
| `ISR`, `ITE` | Exactly enumerated states and transitions |
| `TR`, `MA` | Tests that cross the intended seam; scoped behavior-changing mutants when useful |

---

## Verify Changes, Not Proposals

For an implementation, run the project's required checks and exercise the changed behavior through its intended seam. Compare only evidence selected before the redesign, with the same counting basis and frozen scenarios. For a review, state what a future check would distinguish; do not report it as run.
