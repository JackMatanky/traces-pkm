# Rust Testing Skill Split Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Split the broad `rust-testing` skill into a thin router plus focused Rust unit, integration, and benchmarking workflow skills.

**Architecture:** `rust-testing` becomes the router and shared-reference home. `rust-unit-testing`, `rust-integration-testing`, and `rust-benchmarking` become directly model-invoked workflow skills with checkable completion criteria. Shared references stay linked instead of duplicated; workflow-owned references move under the child skill that owns them.

**Tech Stack:** Markdown skill files under `.agents/skills/`; no application code changes; verification via `git diff`, `grep`, and path checks.

---

## File Structure

**Create:**
- `.agents/skills/rust-unit-testing/SKILL.md` — unit suite write/review/refactor workflow skill.
- `.agents/skills/rust-unit-testing/references/writing-suites.md` — source-derived case-surface workflow.
- `.agents/skills/rust-unit-testing/references/reviewing-suites.md` — gap table + per-suite audit workflow.
- `.agents/skills/rust-unit-testing/references/unit-suites.md` — unit placement, `#[cfg(test)]`, private items, determinism.
- `.agents/skills/rust-unit-testing/references/doctests.md` — rustdoc executable examples and `cargo test --doc` guidance.
- `.agents/skills/rust-unit-testing/references/code-quality.md` — unit suite readability/fixture/helper/assertion quality review.
- `.agents/skills/rust-unit-testing/references/linting.md` — `#[expect]`/`#[allow]` policy for unit suite code.
- `.agents/skills/rust-unit-testing/references/naming.md` — unit suite/module/function naming.
- `.agents/skills/rust-integration-testing/SKILL.md` — public API/boundary/CLI workflow skill.
- `.agents/skills/rust-integration-testing/references/boundary-suites.md` — `tests/` layout, public workflow matrix, external dependency strategy.
- `.agents/skills/rust-benchmarking/SKILL.md` — Criterion workflow/review skill.
- `.agents/skills/rust-benchmarking/references/criterion.md` — Criterion inputs, baselines, `black_box`, setup exclusion.
- `.agents/skills/rust-testing/references/boundaries.md` — router decision matrix.
- `.agents/skills/rust-testing/references/commands.md` — cargo/nextest/doctest/clippy command guide.
- `.agents/skills/rust-testing/references/fixtures.md` — fixtures, cleanup, RAII, table-driven `rstest` cases.
- `.agents/skills/rust-testing/references/property-based.md` — proptest invariants/shrinking/perf notes.
- `.agents/skills/rust-testing/references/mocks.md` — fakes first, mockall expectations, globals caveat.
- `.agents/skills/rust-testing/references/snapshots.md` — insta snapshots, redactions, review flow.
- `.agents/skills/rust-testing/references/async.md` — Tokio test runtime, paused time, async I/O fakes.
- `.agents/skills/rust-testing/references/concurrency.md` — Loom model checking and limits.

**Modify:**
- `.agents/skills/rust-testing/SKILL.md` — replace broad catalog with a router.
- `.agents/skills/rust-testing/references/assertions.md` — keep shared and fold panic-contract guidance into it.

**Delete after moving/adapting content:**
- `.agents/skills/rust-testing/references/async-testing.md`
- `.agents/skills/rust-testing/references/benchmarking.md`
- `.agents/skills/rust-testing/references/concurrency-testing.md`
- `.agents/skills/rust-testing/references/fixtures-and-cleanup.md`
- `.agents/skills/rust-testing/references/integration-testing.md`
- `.agents/skills/rust-testing/references/mocking.md`
- `.agents/skills/rust-testing/references/panics.md`
- `.agents/skills/rust-testing/references/property-based-testing.md`
- `.agents/skills/rust-testing/references/reviewing-a-test-suite.md`
- `.agents/skills/rust-testing/references/running-tests.md`
- `.agents/skills/rust-testing/references/snapshot-testing.md`
- `.agents/skills/rust-testing/references/table-driven-testing.md`
- `.agents/skills/rust-testing/references/test-naming.md`
- `.agents/skills/rust-testing/references/unit-testing.md`
- `.agents/skills/rust-testing/references/writing-a-test-suite.md`

**Do not modify:**
- `.agents/skills/rust-skills/**`

---

### Task 1: Rewrite `rust-testing` as a Router

**Files:**
- Modify: `.agents/skills/rust-testing/SKILL.md`
- Create: `.agents/skills/rust-testing/references/boundaries.md`

- [ ] **Step 1: Replace router skill content**

Write `.agents/skills/rust-testing/SKILL.md` as a thin router with this shape:

```markdown
---
name: rust-testing
description: >
  Rust testing router. Use when the user asks broadly about Rust testing,
  asks which Rust testing approach or skill applies, or mentions multiple
  testing concerns at once. Routes to rust-unit-testing, rust-integration-testing,
  or rust-benchmarking; keeps shared references for boundaries, commands,
  assertions, fixtures, properties, mocks, snapshots, async, and concurrency.
license: MIT
metadata:
  version: "2.0.0"
  companion_to: rust-skills
---

# Rust Testing Router

Use this skill to choose the smallest focused Rust testing workflow. Do not run a full testing workflow from this router.

## Route

| User intent | Use |
|---|---|
| Write, review, or refactor `#[cfg(test)]` / `#[test]` unit suites from source understanding | `rust-unit-testing` |
| Maintain rustdoc executable examples / doctests | `rust-unit-testing` |
| Review unit suite lint suppressions or suite-code quality | `rust-unit-testing` |
| Write or review `tests/` integration suites, public API workflows, CLI/binary behavior, or external boundaries | `rust-integration-testing` |
| Write or review Criterion benchmarks, baselines, representative inputs, or `cargo bench` output | `rust-benchmarking` |
| Unsure which level applies | Read `references/boundaries.md`, then route to exactly one child skill |

## Shared References

| Reference | Use when |
|---|---|
| `references/boundaries.md` | Choosing unit vs integration vs doctest vs property vs snapshot vs benchmark vs concurrency |
| `references/commands.md` | Choosing cargo/nextest/doctest/clippy commands |
| `references/assertions.md` | Assertions, `matches!`, panic-contract checks |
| `references/fixtures.md` | Fixtures, cleanup, RAII, table-driven cases |
| `references/property-based.md` | Proptest properties, shrinking, generated input caveats |
| `references/mocks.md` | Fakes, trait seams, mockall expectations |
| `references/snapshots.md` | Insta snapshots, redactions, review flow |
| `references/async.md` | Tokio async tests, paused time, async I/O fakes |
| `references/concurrency.md` | Loom model checking and limits |

## Completion

Stop after selecting a child workflow or explicitly stating that no Rust testing skill applies.
```

- [ ] **Step 2: Create boundaries reference**

Write `.agents/skills/rust-testing/references/boundaries.md` with this content:

```markdown
# Boundaries

> Pick the testing workflow by the boundary being verified.

| Boundary / question | Use |
|---|---|
| Private branch, helper, invariant, error variant, `Option::None`, panic contract | `rust-unit-testing` |
| Public API workflow exercised as an external caller | `rust-integration-testing` |
| CLI/binary behavior | `rust-integration-testing` |
| External service, filesystem boundary, or database behavior | `rust-integration-testing` unless unit code can use a fake or mock |
| Rustdoc executable example for a public item | `rust-unit-testing` and `rust-unit-testing/references/doctests.md` |
| Same behavior across generated input space / invariant | Unit workflow plus `references/property-based.md` |
| Large structured output or CLI text that needs reviewable diffs | Relevant workflow plus `references/snapshots.md` |
| Async time or async I/O behavior | Relevant workflow plus `references/async.md` |
| Atomic/lock-free interleaving correctness | Relevant workflow plus `references/concurrency.md` |
| Performance question, baseline, throughput, or implementation comparison | `rust-benchmarking` |

Completion: one row is selected, or the request is explicitly outside Rust testing.
```

- [ ] **Step 3: Verify router is lean**

Run:

```bash
grep -n "mechanically enumerates\|Criterion benchmark groups\|mockall\|proptest!" .agents/skills/rust-testing/SKILL.md
```

Expected: no output. The router should route, not carry reference content.

---

### Task 2: Create Shared References with Lean Names

**Files:**
- Create: `.agents/skills/rust-testing/references/commands.md`
- Modify: `.agents/skills/rust-testing/references/assertions.md`
- Create: `.agents/skills/rust-testing/references/fixtures.md`
- Create: `.agents/skills/rust-testing/references/property-based.md`
- Create: `.agents/skills/rust-testing/references/mocks.md`
- Create: `.agents/skills/rust-testing/references/snapshots.md`
- Create: `.agents/skills/rust-testing/references/async.md`
- Create: `.agents/skills/rust-testing/references/concurrency.md`

- [ ] **Step 1: Create commands reference**

Use current `running-tests.md` as source material and write `.agents/skills/rust-testing/references/commands.md`. It must include:

```markdown
# Commands

> Run the smallest command that verifies the branch you touched.

| Need | Command |
|---|---|
| Unit and integration suites with nextest | `cargo nextest run` |
| Doctests | `cargo test --doc` |
| Unit-only target | `cargo nextest run --lib` |
| Integration target | `cargo nextest run --test '<name>'` |
| All integration targets | `cargo nextest run --test '*'` |
| Clippy including suite code | `cargo clippy --all-targets --all-features -- -D warnings` |
| Criterion benchmarks | `cargo bench` |

`cargo nextest run` does not run doctests; use `cargo test --doc` whenever doctests are relevant.

If a command is not run, state the reason in the final response or review output.
```

- [ ] **Step 2: Merge panic-contract guidance into assertions**

Update `.agents/skills/rust-testing/references/assertions.md` so it includes:

```markdown
## Panic Contracts

Use `#[should_panic(expected = "...")]` only when panic is intentional API behavior. Prefer the `expected` message so an unrelated panic does not accidentally pass the check. For recoverable invalid input, assert on `Result` or `Option` instead.
```

- [ ] **Step 3: Create fixtures reference**

Use current `fixtures-and-cleanup.md` and `table-driven-testing.md` as source material. Write `.agents/skills/rust-testing/references/fixtures.md` with sections:

```markdown
# Fixtures

## RAII Cleanup
Use `Drop`, `tempfile`, or `scopeguard` when cleanup must run on panic.

## Table-Driven Cases
Use `rstest` when one behavior has many named literal cases. Do not use it when each case is a separate behavior.

## Completion
Every fixture either builds Arrange data only or is explicitly named as a cleanup guard; hidden assertions in helpers belong in `rust-unit-testing/references/code-quality.md` findings.
```

- [ ] **Step 4: Rename tool references without changing substance**

Move/adapt content:

```text
property-based-testing.md -> property-based.md
mocking.md -> mocks.md
snapshot-testing.md -> snapshots.md
async-testing.md -> async.md
concurrency-testing.md -> concurrency.md
```

When adapting links, point child-skill references back to `../../rust-testing/references/<name>.md`.

- [ ] **Step 5: Verify no forbidden shared reference filenames remain**

Run:

```bash
find .agents/skills/rust-testing/references -maxdepth 1 -type f | grep -E 'test|testing'
```

Expected: no output.

---

### Task 3: Create `rust-unit-testing`

**Files:**
- Create: `.agents/skills/rust-unit-testing/SKILL.md`
- Create: `.agents/skills/rust-unit-testing/references/writing-suites.md`
- Create: `.agents/skills/rust-unit-testing/references/reviewing-suites.md`
- Create: `.agents/skills/rust-unit-testing/references/unit-suites.md`
- Create: `.agents/skills/rust-unit-testing/references/doctests.md`
- Create: `.agents/skills/rust-unit-testing/references/code-quality.md`
- Create: `.agents/skills/rust-unit-testing/references/linting.md`
- Create: `.agents/skills/rust-unit-testing/references/naming.md`

- [ ] **Step 1: Write unit skill front matter and workflow index**

Create `.agents/skills/rust-unit-testing/SKILL.md`:

```markdown
---
name: rust-unit-testing
description: >
  Use when writing, reviewing, or refactoring Rust unit suites: #[cfg(test)],
  #[test], source-derived coverage gaps, doctests, lint suppression review,
  and unit suite code-quality review.
license: MIT
metadata:
  version: "1.0.0"
  companion_to: rust-testing
---

# Rust Unit Testing

Unit work starts from source understanding, not from guessed edge cases.

## Choose Workflow

| Task | Use |
|---|---|
| Write a new unit suite | `references/writing-suites.md` |
| Review/refactor an existing unit suite | `references/reviewing-suites.md` |
| Maintain doctests | `references/doctests.md` |
| Review lint suppressions | `references/linting.md` |
| Review suite code clarity | `references/code-quality.md` |

## Shared References

Use `../rust-testing/references/assertions.md`, `fixtures.md`, `property-based.md`, `mocks.md`, `snapshots.md`, `async.md`, `concurrency.md`, and `commands.md` when a workflow reaches that branch.

## Completion

- New suite: every non-`n/a` case-surface row maps to a named test.
- Review: output includes a gap list, a per-test audit, a code-quality finding list, and a lint suppression inventory, each explicitly empty if no findings.
- Doctest work: every changed public example is run with `cargo test --doc`, marked `no_run`/`compile_fail`/`ignore-*` with reason, or explicitly out of scope.
```

- [ ] **Step 2: Move writing workflow**

Move/adapt `.agents/skills/rust-testing/references/writing-a-test-suite.md` to `.agents/skills/rust-unit-testing/references/writing-suites.md`.

Required edits:

```text
test-naming.md -> naming.md
unit-testing.md -> unit-suites.md
fixtures-and-cleanup.md -> ../../rust-testing/references/fixtures.md
property-based-testing.md -> ../../rust-testing/references/property-based.md
snapshot-testing.md -> ../../rust-testing/references/snapshots.md
running-tests.md -> ../../rust-testing/references/commands.md
```

- [ ] **Step 3: Move review workflow**

Move/adapt `.agents/skills/rust-testing/references/reviewing-a-test-suite.md` to `.agents/skills/rust-unit-testing/references/reviewing-suites.md`.

Add explicit steps:

```markdown
## Code Quality Pass

Read the suite code as production-quality Rust. Report hidden assertions, overbroad fixtures, excessive setup, production-like branching, broad object assertions, repeated assertion blocks, and unclear helpers.

## Lint Suppression Pass

List every `#[allow(...)]` and `#[expect(...)]` in unit suite code with file/line, scope, reason presence, classification, and smallest fix.
```

- [ ] **Step 4: Move unit suite reference and naming**

Move/adapt:

```text
unit-testing.md -> rust-unit-testing/references/unit-suites.md
```

Update internal links to shared references.

- [ ] **Step 5: Move doctests reference**

Move/adapt `.agents/skills/rust-testing/references/doctests.md` to `.agents/skills/rust-unit-testing/references/doctests.md`.

Ensure it includes:

```markdown
`cargo nextest run` does not run doctests. Use `cargo test --doc` whenever doctests are relevant.
```

- [ ] **Step 6: Create code-quality reference**

Create `.agents/skills/rust-unit-testing/references/code-quality.md`:

```markdown
# Code Quality

> Unit suite code is code. Review it for clarity, not only coverage.

Flag:
- Hidden assertions in helpers.
- Overbroad fixtures or builders that hide the arranged case.
- Excessive setup relative to the behavior.
- Repeated assertion blocks that should become table-driven cases.
- Assertions against large objects when one field or a snapshot is clearer.
- Production-like branching, loops, or abstractions inside suite code.

For each finding report file/line, why clarity suffers, and the smallest refactor.
```

- [ ] **Step 7: Create linting reference**

Create `.agents/skills/rust-unit-testing/references/linting.md`:

```markdown
# Linting

> `#[allow]` and `#[expect]` in unit suite code are reviewable decisions.

Prefer narrow `#[expect(..., reason = "...")]` when a lint is intentionally triggered. Treat broad `#[allow(...)]`, missing reasons, and module-wide suppressions as suspicious unless justified.

For each suppression report file/line, lint name, scope, reason presence, acceptable/suspicious classification, and smallest fix: keep with reason, narrow scope, rewrite suite code, or delete suppression.
```

- [ ] **Step 8: Verify unit reference filenames**

Run:

```bash
find .agents/skills/rust-unit-testing/references -maxdepth 1 -type f | sort
```

Expected filenames:

```text
code-quality.md
doctests.md
linting.md
naming.md
reviewing-suites.md
unit-suites.md
writing-suites.md
```

---

### Task 4: Create `rust-integration-testing`

**Files:**
- Create: `.agents/skills/rust-integration-testing/SKILL.md`
- Create: `.agents/skills/rust-integration-testing/references/boundary-suites.md`

- [ ] **Step 1: Write integration skill**

Create `.agents/skills/rust-integration-testing/SKILL.md`:

```markdown
---
name: rust-integration-testing
description: >
  Use when writing or reviewing Rust integration suites: tests/ layout,
  public API workflows, CLI/binary behavior, cross-module behavior, and
  external service/filesystem/database boundaries.
license: MIT
metadata:
  version: "1.0.0"
  companion_to: rust-testing
---

# Rust Integration Testing

Integration suites verify public behavior from outside the crate.

## Workflow

1. List public workflows and boundary failures.
2. For each boundary, choose strategy: real, fake, mock, temp-local, container, or out-of-scope with reason.
3. Map each workflow row to a `tests/` case or explicitly mark out of scope.
4. For CLI cases, state how the binary is invoked.
5. Run the smallest relevant command from `../rust-testing/references/commands.md` or state why not run.

## Completion

- Every public workflow and boundary failure is mapped to an integration case or out-of-scope reason.
- Every external dependency has a named strategy.
- CLI cases state the binary invocation path.
```

- [ ] **Step 2: Move boundary suite reference**

Move/adapt `.agents/skills/rust-testing/references/integration-testing.md` to `.agents/skills/rust-integration-testing/references/boundary-suites.md`.

Ensure it includes:

```markdown
`tests/common/mod.rs` is preferred for shared integration helpers because `tests/common.rs` is compiled as its own integration target.
```

---

### Task 5: Create `rust-benchmarking`

**Files:**
- Create: `.agents/skills/rust-benchmarking/SKILL.md`
- Create: `.agents/skills/rust-benchmarking/references/criterion.md`

- [ ] **Step 1: Write benchmarking skill**

Create `.agents/skills/rust-benchmarking/SKILL.md`:

```markdown
---
name: rust-benchmarking
description: >
  Use when writing or reviewing Rust Criterion benchmarks, performance
  measurements, representative inputs, baselines, comparisons, cargo bench,
  or benchmark validity.
license: MIT
metadata:
  version: "1.0.0"
  companion_to: rust-testing
---

# Rust Benchmarking

Benchmarks answer a performance question. They are not correctness tests.

## Workflow

1. State the benchmark question: hot path, regression guard, or implementation comparison.
2. Define measured unit, representative inputs, baseline/comparison, and success criterion.
3. Write Criterion code using `criterion.md`.
4. Review setup exclusion, input black-boxing, and whether Criterion defaults are sufficient.
5. Run `cargo bench` or state why not run.

## Completion

- Benchmark has a stated question, measured unit, representative input set, baseline/comparison, and success criterion.
- Review states whether setup is excluded from timing, inputs are protected from optimization, and Criterion defaults are sufficient.
```

- [ ] **Step 2: Move Criterion reference**

Move/adapt `.agents/skills/rust-testing/references/benchmarking.md` to `.agents/skills/rust-benchmarking/references/criterion.md`.

Ensure it keeps:

```markdown
Use `black_box` unless Criterion already black-boxes the input for the API being used. Exclude setup from measurement where possible.
```

---

### Task 6: Delete Old Reference Files and Fix Links

**Files:**
- Delete old `.agents/skills/rust-testing/references/*testing*.md` and old workflow filenames listed in File Structure.
- Modify any moved references with broken links.

- [ ] **Step 1: Delete old files after content has moved**

Delete all files in the File Structure “Delete after moving/adapting content” list.

- [ ] **Step 2: Verify no old reference filenames remain under `rust-testing`**

Run:

```bash
find .agents/skills/rust-testing/references -maxdepth 1 -type f | sort
```

Expected:

```text
.agents/skills/rust-testing/references/assertions.md
.agents/skills/rust-testing/references/async.md
.agents/skills/rust-testing/references/boundaries.md
.agents/skills/rust-testing/references/commands.md
.agents/skills/rust-testing/references/concurrency.md
.agents/skills/rust-testing/references/fixtures.md
.agents/skills/rust-testing/references/mocks.md
.agents/skills/rust-testing/references/property-based.md
.agents/skills/rust-testing/references/snapshots.md
```

- [ ] **Step 3: Verify no broken old basename references remain**

Run:

```bash
grep -R "async-testing\|benchmarking.md\|concurrency-testing\|fixtures-and-cleanup\|integration-testing\|mocking.md\|panics.md\|property-based-testing\|reviewing-a-test-suite\|running-tests\|snapshot-testing\|table-driven-testing\|test-naming\|unit-testing\|writing-a-test-suite" .agents/skills/rust-testing .agents/skills/rust-unit-testing .agents/skills/rust-integration-testing .agents/skills/rust-benchmarking
```

Expected: no output.

- [ ] **Step 4: Verify `rust-skills` has no diff**

Run:

```bash
git diff -- .agents/skills/rust-skills
```

Expected: no output.

---

### Task 7: Final Verification and Commit

**Files:**
- All skill files created/modified above.

- [ ] **Step 1: Validate required skill directories exist**

Run:

```bash
for d in .agents/skills/rust-testing .agents/skills/rust-unit-testing .agents/skills/rust-integration-testing .agents/skills/rust-benchmarking; do test -f "$d/SKILL.md" || exit 1; done
```

Expected: exit 0.

- [ ] **Step 2: Validate direct model invocation descriptions are focused**

Run:

```bash
grep -n "^description:" -A8 .agents/skills/rust-testing/SKILL.md .agents/skills/rust-unit-testing/SKILL.md .agents/skills/rust-integration-testing/SKILL.md .agents/skills/rust-benchmarking/SKILL.md
```

Expected: each description is short and branch-specific; no child description lists every tool reference.

- [ ] **Step 3: Validate no forbidden reference filenames except doctests**

Run:

```bash
find .agents/skills/rust-testing/references .agents/skills/rust-unit-testing/references .agents/skills/rust-integration-testing/references .agents/skills/rust-benchmarking/references -type f | grep -E 'test|testing' | grep -v '/doctests.md'
```

Expected: no output.

- [ ] **Step 4: Inspect diff stat**

Run:

```bash
git diff --stat
```

Expected: changes only under `.agents/skills/rust-testing`, `.agents/skills/rust-unit-testing`, `.agents/skills/rust-integration-testing`, and `.agents/skills/rust-benchmarking`.

- [ ] **Step 5: Commit**

Run:

```bash
git add .agents/skills/rust-testing .agents/skills/rust-unit-testing .agents/skills/rust-integration-testing .agents/skills/rust-benchmarking
git commit -m "feat(skills): split rust testing workflows"
```
