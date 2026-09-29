# Quality-Gates Tooling Implementation Plan (2026-09-27 → 2026-09-29)

> **Status (2026-09-29):** All three phases complete and merged to `main`.
>
> - **Part I (2026-09-27):** messrust + mutarust adoption, policy files, mise tasks, cargo-mutants retirement — shipped (Part I Task 14).
> - **Part II (2026-09-28):** strict declared-flag surface, task UX hardening, docs alignment — shipped (Part II Task 7); spec `docs/superpowers/specs/2026-09-28-quality-gates-task-ux-design.md` approved, committed `01933d0d`.
> - **Part III (2026-09-29):** `mutants` → `test:mutants` rename with hidden `mutants` file-task alias — branch base `90696f25`; merged from `main` in merge `8413178f`, then merged to `main` as `15aba701`; all tasks and verifications ticked. (Two distinct merges — both labels are correct.)
> - This file consolidates the three former plan files (`2026-09-27-quality-gates-messrust-mutarust.md`, `2026-09-28-quality-gates-task-ux.md`, `2026-09-29-mutants-task-rename.md`) on 2026-09-29; the former design spec `2026-09-29-mutants-task-rename-design.md` was absorbed into Part III (deleted; historical mentions retained).

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

## How to read this document

- The three parts are separate top-level headings by design: each part is a byte-faithful record of its era's plan; nesting them under one heading tree would have required rewriting fenced snapshots (forbidden).
- Everything inside a fenced block is a point-in-time record ("as shipped \<date\>") — never re-flow, re-indent, or re-run rename seds against it. The Part II 38-case matrix block is the live extractable executable artifact.
- Commands are verbatim execution records: paths inside code blocks may name files that have since been merged into this document (09-27 plan → Part I; 09-28 plan → Part II). Prose cross-references have been repointed to Part I/II/III.
- Cross-era contradictions and supersessions are resolved in **Evolution and supersessions** — read it once before the parts.

## Evolution and supersessions

Chronological plans accumulate superseded decisions; this table is the authoritative cross-era resolver. Era-local records below are intentionally left as written.

| Topic | Was (era / location) | Now (current state) |
| --- | --- | --- |
| S1 — entrypoint name stability | Part I Goal + Locked decision 9: "the `mutants` entrypoint name never changes — only the engine behind it"; entrypoint is `mutants`, never `mutarust` | Part III Goal + Decisions 1–2: renamed to `test:mutants`, `mutants` kept as a hidden file-task alias; "never `mutarust`" and the `mutants:` message prefix survive (see S19) |
| S2 — task path | Part I File structure / Task 7: `.mise/tasks/mutants/_default` is the live task (directory kept) | Part III Decision 1: `git mv … .mise/tasks/test/mutants`; the `mutants/` directory disappears |
| S3 — task usage-error exit codes | Part I Grounding facts / Task 7 `long_help` / Task 13 doc snapshot: task-level usage errors exit `2` (repo precedent `coverage:html`) | Part II Task 1 `long_help` + matrix: usage-validation failures exit `1` (mise ERROR), scope/target-scan errors exit `2`; Part III Verification 2 inherits rc 2 for scan-style refusals |
| S4 — declared flag after `--` | Part I Grounding facts: ambiguous in the docs; "the task script handles both readings" (`has_fixed_timeout`, dual-read) | Part II Ground rules (caching quirks): mise strips `--` before usage parsing (probe-verified); the defensive dual-read code was deleted (Part II Task 1 notes; spec A4 claim void) |
| S5 / S21 — flag-conflict enforcement locus | Part I Task 7: bash cascades `reject_conflicting_flags` / `reject_declared_in_inspect` / `has_fixed_timeout` (exit 2), incl. declared `--timeout` ⊥ passthrough timeout checks | Part II Task 1: KDL `conflicts`/`exclusive`/`requires` (parse-time exit 1) + slim `reject_bad_targets` (exit 2); those functions deleted, `has_fixed_timeout` removed and `--timeout`/`--exec-timeout`/`--timeout-coefficient` became declared `conflicts` |
| S6 — flag surface / positional | Part I Task 7 `#USAGE` + doc row: 9 declared flags + variadic `[args]` passthrough after `--` | Part II Task 1: ~40 declared flags, `arg "[targets]"` positional, "flags are declared above — never pass them after `--`"; doc row rewritten to `[targets]` |
| S7 — `--git-diff` default | Part I Task 7: `#USAGE flag "--git-diff" negate="--no-git-diff" default=#false` | Part II Task 1: no `default` on `--git-diff` so `--git-diff-base`'s `requires` fires on absence, plus a script backstop |
| S8 — inspect-mode rejection | Part I Grounding facts (list/inspect modes): declared run flags are consumed by mise, "the task rejects them itself (exit 2)" | Part II Task 1: `--list-*`/`--print-ast` carry 34-selector `conflicts` lists → parse-time exit 1 |
| S9 — checkout | Part II Ground rules: worktree `/Users/jack/Documents/41_personal/traces-pkm/.worktrees/quality-gates` — "Never touch the main checkout." | Part III Ground rules: the main directory IS the checkout (user-approved 2026-09-29, worktree retired mid-plan — Part III Incidents I1) |
| S10 — where verification runs | Part II Ground rules: "Verification lives in the repo, not /tmp … All probes below run from the worktree root." | Part III Ground rules: a clean clone under `/var/folders/…/opencode/` is runner-only for the matrix; authoring and edits stay in-repo |
| S11 — report task / `mutants/` directory rationale | Part I Locked decision 4 + File structure: report task deleted at cutover, directory kept ("the `_default` file stays — it *is* the mutants task") | Part III Context + Decision 1: with the report task gone (`ffb725d6`) the directory earns nothing; `git mv` removes `mutants/` |
| S12 — matrix case count | Part II Task 1 matrix shipped as 37 cases | Same block edited in place by Part III Task 3 → 38 cases (37 renamed + alias regression); `MATRIX: ALL PASS (38 cases)` |
| S13 — pre-rename invocations | Part I: 53 × `mise run mutants` (era-1 runnable); Part II: `Line N before/after:` records | Part III Task 3 seds repointed only Part II's runnable lines (12 × `mise run test:mutants`); `Line N` records and all of Part I stay pre-rename by design (Part III Decision 4) |
| S14 — `docs/refs/mutation_testing.md` content | Part I Task 13 full snapshot as shipped 09-27 → Part II Task 6 six UX edits | Part III Task 2: `:30` line + 15 invocations → `test:mutants`; the live doc is the authority, the snapshots stay historical |
| S15 — final hk gate convention | Part I Task 14: scoped `hk check --skip-step gitleaks --format json --files0-from -` (no `--safe`) | Part II Task 7 / Part III Task 5: whole-tree `hk check --safe --skip-step gitleaks --format json` → `passed` (standing; see Ground rules) |
| S16/S17/S18 — dated verification numbers and help-content snapshots | Part I Task 7 help probes / `long_help` examples (passthrough form) and Part II Task 7 expected "~2904 tests" | Part II Task 1 greps (declared-form examples) and Part III Verification 6 (2988 tests) — each era's numbers and help text kept as dated evidence, not reconciled |
| S19 — message prefix after the rename | Part I Locked decision 9: error messages use the task-name prefix `mutants:` (repo convention, cf. `coverage:`) | Task is now `test:mutants` but messages still say `mutants:` (Part III Task 1 Step 6 refusal probe; prefix also in the Part I/Part II script snapshots) — recorded, out of scope, unaddressed |
| S20 — merge labels | Part III Status block: "merge commit `15aba701` … branch base `90696f25`" vs Part III Task 5: "merge `8413178f`" | Two distinct merges — `8413178f` (main → `quality-gates`, Part III Task 5 Step 2) and `15aba701` (`quality-gates` → `main`, later the same day); both labels are correct |
| S22 — `--update-baseline` pair rejection | Part I Grounding facts / Task 7: rejected at task level, exit 2 (verified) | Part II Task 1: same pair rejected by `conflicts` → exit 1 (matrix cases); same semantics, different rc (see S3) |
| S23 — `min-msi` config gate (NOT a contradiction) | Part I `mutarust.yml` / Task 11: `# min_msi: 60` commented until baseline; stays commented after scoped measurement | Part II Task 1 declares `--min-msi` as a user flag — the config gate remains commented/off; standing |
| S24 — `--list-files` ignores `exclude_dirs` (NOT a contradiction) | Part I Grounding facts: known quirk — `--list-files` ignores `exclude_dirs` (absolute paths) | Part II Task 1 help text keeps "(NOTE: ignores exclude_dirs)" — consistent, standing |

## Ground rules (standing, non-negotiable)

Assembled 2026-09-29 from the three era files' Ground rules: canonical current rules (era 3) first, then standing carry-forwards annotated with era. Era-specific quirks live inside each Part.

- **Checkout:** the main directory `/Users/jack/Documents/41_personal/traces-pkm` — during execution on branch `quality-gates` (user-approved 2026-09-29: the nested worktree was retired mid-plan — see Part III Incidents); all editing happened here. This IS the repo root, so the old `.mise/tasks/mutants/` path is gone and no ancestor can shadow the alias. Branch `main` was not touched until Task 5's gates passed and the merge step ran.
- **Clone = runner only:** logic is proven in place; a clean clone under `/var/folders/9w/3qn47_qj3m9b27gkxwr5_k9m0000gn/T/opencode/` exists solely to execute suites that must not see local runtime state (the matrix) — never to author edits.
- **Socket policy:** `.codegraph/daemon.sock` (live codegraph daemon, pid from `.codegraph/daemon.pid`) makes mutarust abort with `could not copy unsupported workspace entry` — gitignore does not protect it (probe-verified: root `.gitignore` entry changes nothing). Single bounded probes in the main dir: move the socket aside under a `trap … EXIT` guard that restores it in all cases. Long suites: use the clone — never leave the socket moved for minutes. `mise run verify` does not invoke mutarust and runs in place.
- **Anomaly protocol:** if on-disk state contradicts git (file missing/extra/restored), STOP — capture `git status --porcelain`, `git log --oneline -3`, `ls -la <path>`, `git reflog -5`; log it under Incidents; reconcile with forward-fix commits only — never amend or re-stage history to "correct" an anomaly.
- **Capture rc safely:** `out=$(cmd 2>&1); rc=$?` — never `cmd | tail; echo $?`; never pipe `mise run …` into `head` (SIGPIPE); zsh aborts unquoted `echo ===x===` (`=` expansion). No `timeout` binary on macOS.
- **All probes use `--skip-deps`** (skips `depends=["test"]`, ~10 s saved per probe). The one alias run (`-f src/lib.rs --match __zz_no_match__`) is bounded by Part III Verification 3 and finishes in seconds.
- **Matrix never commits a baseline:** if the guard prints `BASELINE_RESTORED`, accept it; `mutarust-baseline.json` must be unmodified in every commit.
- **Commits:** conventional via hk, one per task, no `--no-verify`. Final hk gate is `hk check --safe --skip-step gitleaks --format json` (gitleaks is pre-existing-unknown-effect: `--safe` refuses that step's unknown effect; run `gitleaks detect` manually only if secrets are a concern — plan files contain no secrets).
- **Counts:** `rg -c` prints nothing on zero matches — use `rg -o PATTERN FILE | wc -l` for counts that must equal 0.
- **No multi-hour runs.** Every mutarust invocation is scoped (`-m strsim`, `--list-*`, `--git-diff` on a src-unchanged branch) and prefixed `nice -n 10`. Bash tool timeouts ≥ `900000` for cargo-touching commands.
- **Verification lives in the repo, not /tmp.** Usage-constraint enforcement was nondeterministic outside this project (settings drift); inside the worktree it was stable across repeat runs. All probes below run from the worktree root. *(era-2 rule; its worktree-root clause retired 2026-09-29 — see Part III Incidents I1)*
- **mess is advisory** (Part I locked decision 2): no `--strict`, no `--reportfile` by default; exit `0` clean / `2` findings (the normal signal) / `1` tool error; never wired into hk/verify/CI.
- **shellcheck** (verbatim rule: Part I Grounding facts): installed (`/opt/homebrew/bin/shellcheck`); run it on every new/edited task script; the `SC2154` directive idiom above `set -euo pipefail` is the repo standard.
- **Task-owned flag refusals** (Part II design): the five task-owned refusals — `--config`, `--test-flags`, `--exec`/`--no-exec`, `--features` — are by design ("task owns `--config`"); enumerated in Part II's `#USAGE` blocks (exit 2, `mutants:` message).
- **STOP and escalate on surprise** (Part I pattern): if a probe or tool disagrees with the vendored docs or expectations (e.g. flags missing vs the pin), stop and escalate before writing config/tasks.
- **Report artifacts are clobbered every run** (full report facts: Part I Grounding facts): read stats immediately after the scored run — never after an intervening `--dry-run`; never commit regenerated report artifacts (`report.json` etc.).
- **Merge commits** (Part III Incidents I3): stage everything and clear untracked files before any merge commit, or expect the same failure; never run `git stash` during a merge — it deletes `MERGE_HEAD` and the merge commit fails.
- **Final-gates era note:** era 1 used a scoped `--files0-from` variant (Part I Task 14); current convention is whole-tree `hk check --safe --skip-step gitleaks …` → `passed`.

---

# Part I — 2026-09-27: messrust + mutarust adoption

**Goal:** Install `messrust` (static mess detection, advisory) and `mutarust` (mutation testing, replaces `cargo-mutants`) as mise-managed tools with repo-committed policy files and `mise run` tasks, then retire the cargo-mutants artifacts — no hk gate, no CI job.

**Architecture:** Two new mise tools pinned in `mise.toml [tools]`. `mess` is a TOML passthrough task (repo idiom for short bodies) reading `messrust.xml` at repo root. The mutation task keeps the **tool-agnostic name `mutants`** at `.mise/tasks/mutants/_default` (repo idiom for flag→argument mapping), extending `[task_templates.mutants]`; Task 7 replaces the legacy cargo-mutants script in that file with the mutarust implementation, so the `mutants` entrypoint name never changes — only the engine behind it. It always passes `--config mutarust.yml --logger-agentic-json`. Migration is staged: install both tools → policy files (fail-first) → swap the mutants task implementation → pilot/experiments (old engine measured via raw `cargo mutants`) → commit baseline → retire cargo-mutants → rewrite `docs/refs/mutation_testing.md`.

**Tech Stack:** Rust toolchain via mise; `messrust tag:v0.1.15` (PMD-style XML rulesets); `mutarust 0.1.10` (YAML config, Mutago-compatible baseline); bash task scripts with `#MISE`/`#USAGE` headers.

**Primary sources (read before deviating):**
- `docs/refs/quality_gates_mise_adoption.md` — especially §3 (task designs), §8 (checklist), §10 (capability parity, artifact ledger)
- `docs/refs/quality_gates_messrust.md` — §7 (policy design), §8 (rule inventory, §8.4 property names)
- `docs/refs/quality_gates_mutarust.md` — §8 (config/CLI), §9 (semantics, §9.12 exit codes)
- Vendored docs: `docs/refs/messrust/`, `docs/refs/mutarust/`, `docs/refs/mise_tasks/`
- Shell style: `docs/refs/google_shell_style_guide.md` (see "Shell style" under Grounding facts)

**Locked decisions (do not re-litigate during execution):**
1. Tool pins: `"cargo:https://github.com/quality-gates/messrust" = { version = "tag:v0.1.15" }`, `"cargo:mutarust" = "0.1.10"`. If `mise lock` refuses a <24 h-old pin (release-age), fall back to `tag:v0.1.14` / `0.1.9` and note it. If the `mutarust` crate is unpublished (vendored docs say crates.io publication was still issue #28), fall back to `"cargo:https://github.com/quality-gates/mutarust" = { version = "tag:v0.1.10" }` (or the latest existing tag) — see Task 5.
2. `mess` is advisory: no `--strict`, no `--reportfile` by default; exit 2 (findings) is the contract; never wired into hk/verify/CI in this plan.
3. The mutants task always passes `--config mutarust.yml --logger-agentic-json`. `--logger-summary-json` is *not* passed (`report.json.stats` already carries the counts).
4. Report Markdown task (`.mise/tasks/mutants/report`) is replaced by `mutarust-agentic.json` + stdout + `mutarust-report.html`; the report task file is deleted at cutover, not ported (the `_default` file stays — it *is* the mutants task).
5. Baseline semantics: current escapes are accepted as policy at stage 2 (mirrors cargo-mutants status quo); killing survivors is future work.
6. `--check`/`--exclude`/`--iterate`/`-L`/`-v`/`-V`/`--all-logs`/`--json` do not port — documented in task `long_help` as a migration cheatsheet.
7. `skip_with_cfg`/`skip_without_test` stay `false` (documented ambiguity for `#[cfg(any(test, feature = "test-utils"))]`).
8. Do not copy vendored `mutarust.yml.example` (its `min_covered_msi` forces exit 4 without `--coverage`; it also sets `min_msi: 80` — copying would enable two gates).
9. **Task-name stability:** the mutation entrypoint is `mutants` (`.mise/tasks/mutants/_default`), never `mutarust`. The script body is the only tool-specific surface; a future engine swap rewrites that file in place. Error messages from the task use the task-name prefix `mutants:` (repo convention, cf. `coverage:` in `coverage:html`).
10. **`--ignore-msi-with-no-mutations` stays paired with `--min-msi` only under `--git-diff`** (the documented idiom). The consequence — any other zero-mutant scope plus `--min-msi` exits 4 (score 0) — is documented in the task's `long_help`, not silently auto-fixed.

---

## Grounding facts (verified this session)

- **Repo conventions:** TOML passthrough tasks live in `mise.toml` (`check`/`fmt`/`audit*`/`release`/`doc`); flag-mapping tasks are file tasks under `.mise/tasks/` with `#MISE` headers + `#USAGE` specs. File tasks accept exactly one `extends` target. `#MISE` headers reject `timeout` (must live on the template) — `[task_templates.mutants]` already provides `timeout = "1h"`.
  - **Attribution (important):** the "`#MISE` rejects `timeout`", "one `extends` target (array silently dropped)", "`ERROR task failed`", and "task not found" claims are **live-verified repo observations on mise 2026.9.14** (comments at `mise.toml:272-291` + direct runs). The vendored `docs/refs/mise_tasks/` snapshot *contradicts* the first two (`07_configuration.md` marks only `shell`/`usage` as toml-only; no `extends`-in-`#MISE` discussion). **Trust live behavior, not the vendored docs, on these four points** — do not "fix" them from doc reading.
- **Passthrough semantics (probed 2026-09-27):** undeclared flags — both before *and* after `--` — are captured into `usage_args` and forwarded to the task (`mise run check -- --bogus-probe-xyz` reached cargo both ways). Whether a *declared* flag after `--` is parsed as the flag or raw-captured is ambiguous in the docs; **the task script handles both readings** (declared `--timeout` is read from `usage_timeout` *and* scanned for in passthrough). `mise run <task> --help` renders usage blocks fine (probed on `mutants`; the vendored `05_file_tasks.md:305` "help not yet implemented" note is stale).
- **Exit codes:**
  - messrust: `0` clean, `1` error (takes precedence), `2` findings; `--ignore-violations-on-exit` → `0`.
  - mutarust: `0` pass, `1` command error, `2` bash completion (**not a run** — only relevant when the binary is invoked directly), `3` config/source/annotation error, `4` quality gate. Triggers for `4`: score below `min_msi`; score below `min_covered_msi`; **`min_covered_msi` set without `--coverage`**; `--fail-on-escaped` with an escape absent from the baseline. The task *itself* also exits `2` for its own usage errors (repo precedent: `coverage:html`) — the printed legend distinguishes the two.
  - `coverage:html` precedent: task-level usage errors exit `2`.
- **`set -e` trap (discovered live):** a bash function whose last statement is a failing `[[ … ]] && cmd` list returns 1, and `set -e` kills the script. The legacy `.mise/tasks/mutants/_default` is broken *today* by this (verified: `bash -x` dies at end of `build_filter_flags`; `mise run mutants -- --list` → `ERROR task failed`) — which is also why the Task 9 old-engine baseline runs raw `cargo mutants` instead of the legacy task. Every conditional function in this plan ends with explicit `return 0`, and `"$@"`-style empty arrays use the `${arr[@]+…}` guard (macOS bash 3.2 + `set -u`).
- **Pinned mutarust mutual exclusions** (task must honor): `--dry-run` ⊥ timeout controls (`--timeout`/`--exec-timeout`/`--timeout-coefficient`) ⊥ Cargo test controls (`--test-flags`/`--test-recursive`) ⊥ `--exec`/`--no-exec` ⊥ `--workers` ⊥ `--coverage` ⊥ `--per-test` ⊥ `--do-not-remove-tmp-folder`; `--timeout`/`--exec-timeout` ⊥ `--timeout-coefficient`; `--update-baseline` ⊥ `--dry-run`/`--no-exec`/`--run-mutant-id` and writes **no reports**; `--git-diff-base` requires `--git-diff-lines`; `--run-mutant-id` silently bypasses score gates. The task rejects the reachable combinations with a clear `mutants:` message (see `reject_conflicting_flags`).
- **Report facts:** `report.json` is written after a completed run and is **clobbered by every subsequent run, including `--dry-run`** — read stats immediately after the scored run. A run whose *clean* test suite fails stops before mutation and most likely writes no reports (distinguish this from "gate failed but reports missing"). A zero-mutant run still writes reports.
- **List/inspect modes (live-probed on 0.1.10 during Task 7; amends the script):** `--list-mutators`/`--list-files`/`--print-ast` reject ALL configuration and mutation options — `--config`, `--test-flags`, `--dry-run`, `--min-msi`, loggers, even `--verbose` (rc=3) — **and a TARGET placed *before* the list flag counts as a mutation option**: `mutarust --list-files ./src...` rc=0 (123 files, absolute paths), `mutarust ./src... --list-files` rc=3. `--list-mutators` accepts no target at all. The script therefore branches in `main` → `build_inspect_args`: task-owned flags omitted, passthrough appended first (flag before target), target appended only for `--list-files`/`--print-ast` and only when the user supplied none; declared run flags are consumed by mise and cannot reach mutarust, so the task rejects them itself (exit 2).
- **Template merge semantics:** a task's `depends` **replaces** the template's `depends` entirely (never appends) — `docs/refs/mise_tasks/09_templates.md:54,89-97`. `[task_templates.mutants]` currently defines no `depends`, so `#MISE depends=["test"]` survives; if anyone later adds `depends` to the template, it would be silently dropped.
- **shellcheck** is installed (`/opt/homebrew/bin/shellcheck`); use it on every new/edited task script. The directive idiom `# shellcheck disable=SC2154  # usage_* are injected by mise` directly above `set -euo pipefail` is the repo standard and shellcheck-clean (verified against existing tasks).
- **Shell style:** all planned shell follows `docs/refs/google_shell_style_guide.md` **except where mise task conventions conflict — mise conventions win** (per instruction). Documented conflicts: (a) `eval "arr=(${usage_args:-})"` is Google §6.6-avoided but is mise's documented variadic-args pattern (`mise_tasks/06_arguments.md:190-206`) — intentional, do not "fix"; (b) single-line `help "…"` usage strings may exceed 80 cols (Google §5.2) — KDL data strings stay single-line, wrap code/comments only; (c) `#MISE`/`#USAGE` headers must come first in the file, so the Google §4.1 file-description comment sits *after* the header block.

---

## File structure

| File | Action | Responsibility |
| --- | --- | --- |
| `mise.toml` | modify | tools lines; `[tasks.mess]` after `[tasks.crap]`; remove `cargo-mutants` at cutover |
| `mise.lock` | regenerate | pin the two new tools; drop cargo-mutants at cutover |
| `messrust.xml` | create (repo root) | messrust policy: `rust` preset minus redundant/noisy rules + `ShortMethodName` exceptions |
| `mutarust.yml` | create (repo root) | mutarust policy: outputs, `exclude_dirs`, `disable_mutators`, gate commented until baseline |
| `.mise/tasks/mutants/_default` | **replace implementation (Task 7)** | the tool-agnostic `mutants` task: `-f`/`-m`/gate flags/dry-run conditionality/passthrough; legacy cargo-mutants script is superseded in place (history keeps it) |
| `.mise/tasks/mutants/report` | delete at cutover | legacy Markdown report task (never invoked by the new `_default`) |
| `.gitignore` | modify | add mutarust report artifacts; remove `mutants.out`/`mutants-report.md` lines at cutover |
| `.cargo/mutants.toml` | delete at cutover | legacy config (fully ported by then) |
| `Cargo.toml` | conditional at cutover | `[profile.mutants]` kept only if Task 10 P2 experiment passes |
| `mutarust-baseline.json` | create at stage 2, commit | accepted escapes (Mutago format) |
| `docs/refs/mutation_testing.md` | rewrite at stage 3 | user/agent guide for the mutants workflow |
| `docs/refs/quality_gates_messrust.md` | append §9 | first advisory run observations + tuning decisions |
| `docs/refs/quality_gates_mise_adoption.md` | append §11 | pilot results (A/B, experiments) |

---

### Task 0: Commit pending research docs + this plan

**Files:**
- Stage: `docs/refs/quality_gates_messrust.md`, `docs/refs/quality_gates_mutarust.md`, `docs/refs/quality_gates_mise_adoption.md`, `docs/superpowers/plans/2026-09-27-quality-gates-messrust-mutarust.md`

- [x] **Step 1: Confirm exactly these files are pending**

Run: `git status --porcelain`
Expected: the three `docs/refs/quality_gates_*` paths (two modified, one untracked) plus `?? docs/superpowers/plans/`. Nothing else.

- [x] **Step 2: Commit**

```bash
git add docs/refs/quality_gates_messrust.md docs/refs/quality_gates_mutarust.md \
  docs/refs/quality_gates_mise_adoption.md docs/superpowers/plans/2026-09-27-quality-gates-messrust-mutarust.md
git commit -m "docs(refs): quality-gate research for messrust and mutarust"
```
Expected: commit created; `git status --porcelain` empty.

---

### Task 1: Install messrust via mise

**Files:**
- Modify: `mise.toml` (line ~30, `# -- Testing & Quality --` block)
- Modify: `mise.lock`

- [x] **Step 1: Confirm the tool is absent (red)**

Run: `command -v messrust || echo "messrust: not installed"`
Expected: `messrust: not installed`

- [x] **Step 2: Add the tools line**

In `mise.toml`, in the `# -- Testing & Quality --` block, after the line `"cargo:cargo-mutants" = "latest"` (line 30), insert:

```toml
"cargo:https://github.com/quality-gates/messrust" = { version = "tag:v0.1.15" }
```

- [x] **Step 3: Lock**

Run: `mise lock`
Expected: exit 0; `git diff --stat mise.lock` shows a change; `rg 'messrust' mise.lock` prints an entry.
Fallback: if lock refuses the fresh pin (24 h release-age), change to `tag:v0.1.14`, re-run `mise lock`, note the substitution.

- [x] **Step 4: Install (source build, ~1–3 min)**

Run: `mise install`
Expected: exit 0.

- [x] **Step 5: Verify (green)**

Run: `messrust --version; echo "rc=$?"`
Expected: rc=0 and output containing `0.1.15` (exact format unverified; any nonzero rc → stop and read install output).

- [x] **Step 6: Commit**

```bash
git add mise.toml mise.lock
git commit -m "chore: install messrust via mise"
```

---

### Task 2: `messrust.xml` policy (fail-first)

**Files:**
- Create: `messrust.xml` (repo root)

- [x] **Step 1: Run against the missing policy file (red)**

Run: `messrust src text messrust.xml --ignore-tests; echo "rc=$?"`
Expected: **rc=1** (error beats findings) with an error about the ruleset (an unloadable policy is a configuration error; rc=2 is impossible here because rc=2 requires *findings*, which need a loaded policy). The message naming `messrust.xml` is likely but undocumented — rc=1 alone is the pass criterion. If in doubt add `--verbose` (documented as "ruleset load diagnostics").

- [x] **Step 2: Write the policy**

Create `messrust.xml` at repo root:

```xml
<?xml version="1.0" encoding="UTF-8"?>
<ruleset name="traces-pkm policy" xmlns="http://pmd.sf.net/ruleset/1.0.0">
  <description>traces-pkm: rust preset minus rules already denied by
  clippy/rustc, minus rules with open upstream FP bugs.</description>
  <rule ref="rust">
    <!-- Dup of rustc non_snake_case/non_camel_case_types (already -D warnings)
         + upstream FP bug #184 (raw identifiers) -->
    <exclude name="CamelCaseClassName"/>
    <exclude name="CamelCaseMethodName"/>
    <exclude name="CamelCaseParameterName"/>
    <exclude name="CamelCasePropertyName"/>
    <exclude name="CamelCaseVariableName"/>
    <!-- Dup of rustc unused_variables/dead_code + upstream FP bug #183 -->
    <exclude name="UnusedLocalVariable"/>
    <exclude name="UnusedPrivateField"/>
    <exclude name="UnusedPrivateMethod"/>
    <!-- Dup of clippy::too_many_lines (denied) -->
    <exclude name="ExcessiveMethodLength"/>
    <!-- Upstream: quiet compatibility rule that never reports -->
    <exclude name="GotoStatement"/>
    <!-- pending upstream #160 (pub(crate) counted as public; this repo leans
         on pub(crate) heavily) -->
    <exclude name="ExcessivePublicCount"/>
    <exclude name="TooManyPublicMethods"/>
    <!-- redundant with clippy::too_many_arguments (threshold 5 < 10),
         dup of rustc non_upper_case_globals, ~18 intentional-output sites -->
    <exclude name="ExcessiveParameterList"/>
    <exclude name="ConstantNamingConventions"/>
    <exclude name="DevelopmentCodeFragment"/>
  </rule>
  <!-- Later refs override properties of rules already loaded via `rust`. -->
  <rule ref="naming/ShortMethodName">
    <properties>
      <property name="exceptions" value="at,of,or,eq"/>
    </properties>
  </rule>
  <!-- explicitness stays opt-in: run
       `messrust src text messrust.xml,explicitness --ignore-tests` for deep
       passes instead of loading it here (§8.10 of quality_gates_messrust). -->
</ruleset>
```

- [x] **Step 3: Run the policy (green)**

Run: `messrust src text messrust.xml --ignore-tests; echo "rc=$?"`
Expected: rc=2 with findings listed (the research predicts codesize findings on this codebase), **or** rc=0 if the policy is already quiet. rc=1 = config error → add `--verbose` (`messrust src text messrust.xml --ignore-tests --verbose`) and fix the XML (most likely a rule name).

- [x] **Step 4: Machine triage output (format `json`, not `text`)**

Run: `messrust src json messrust.xml --ignore-tests --reportfile /tmp/messrust.json --ignore-violations-on-exit; echo "rc=$?"`
Expected: rc=0; `/tmp/messrust.json` exists and is non-empty.
**Note:** the FORMAT positional (2nd) selects report content — `--reportfile` only redirects it to a file (and empties stdout). The `.json` extension does *not* imply JSON (`text … --reportfile x.json` writes text). Structured fields documented: `path`, `line`, `rule`, `priority`, `message`, `context`, `suppression`.

- [x] **Step 5: Count findings per rule**

Run: `grep -oE '"rule"[[:space:]]*:[[:space:]]*"[^"]+"' /tmp/messrust.json | sort | uniq -c | sort -rn`
Expected: one line per rule with findings. If the grep is empty, inspect the real shape first (`jq 'keys' /tmp/messrust.json`, then drill into the findings array) and adapt — then continue with the counts.

- [x] **Step 6: Commit**

```bash
git add messrust.xml
git commit -m "chore: add messrust policy file"
```

---

### Task 3: `[tasks.mess]` in mise.toml

**Files:**
- Modify: `mise.toml` (after `[tasks.crap]`, line ~222)

- [x] **Step 1: Task absent (red)**

Run: `mise tasks | grep -w mess; echo "rc=$?"`
Expected: rc=1 (no match).

- [x] **Step 2: Add the task**

In `mise.toml`, immediately after the `[tasks.crap]` block (`run = "cargo crap --lcov lcov.info"`), insert:

```toml
[tasks.mess]
description = "Static mess detection with `messrust` (advisory)"
extends = "gate:never-skip"
usage = 'arg "[args]" var=#true help="Extra arguments forwarded to messrust (e.g. --strict, --only Rule, --reportfile f.json)"'
run = '''
#!/bin/bash
# usage_args is shell-quoted (var=#true arg); mise documents eval-into-array
# as the pattern for variadic args — intentional despite Google style §6.6.
# shellcheck disable=SC2154
set -euo pipefail
declare -a extra
eval "extra=(${usage_args:-})"
messrust src text messrust.xml --ignore-tests "${extra[@]+"${extra[@]}"}"
'''
```

- [x] **Step 3: Task present (green)**

Run: `mise tasks | grep -w mess`
Expected: a line `mess  Static mess detection with ...`.

- [x] **Step 4: Run through the task**

Run: `mise run mess; echo "rc=$?"`
Expected: same findings as Task 2 Step 3; rc=2 (mise may wrap it — any nonzero other than a mise usage error is acceptable here; findings must be on stdout).

- [x] **Step 5: Passthrough works**

Run: `mise run mess -- --ignore-violations-on-exit --reportfile /tmp/messrust.json; echo "rc=$?"`
Expected: rc=0; `/tmp/messrust.json` refreshed (text format — this file is only a smoke artifact; use Task 2 Step 4's `json` invocation for real triage).

- [x] **Step 6: Lint surface still healthy**

Run: `mise run lint`
Expected: exit 0 (no Rust changes; confirms mise/hk plumbing still healthy after the mise.toml edit).

- [x] **Step 7: Commit**

```bash
git add mise.toml
git commit -m "feat: add mise mess task"
```

---

### Task 4: First mess triage + tune

**Files:**
- Modify: `messrust.xml` (only if a threshold below trips)
- Modify: `docs/refs/quality_gates_messrust.md` (append §9)

- [x] **Step 1: Get per-rule counts**

Run: `messrust src json messrust.xml --ignore-tests --reportfile /tmp/messrust.json --ignore-violations-on-exit && grep -oE '"rule"[[:space:]]*:[[:space:]]*"[^"]+"' /tmp/messrust.json | sort | uniq -c | sort -rn | tee /tmp/mess-rule-counts.txt`
Expected: file of `count rule` lines. (Format `json` is required — see Task 2 Step 4.)

- [x] **Step 2: Apply the decision table**

For each row, find the count in `/tmp/mess-rule-counts.txt` (missing rule = count 0):

| Rule | Count > threshold → action (exact edit) |
| --- | --- |
| `CyclomaticComplexity` | **> 15**: append before `</ruleset>` in `messrust.xml`: `<rule ref="codesize/CyclomaticComplexity">\n  <properties><property name="reportLevel" value="12"/></properties>\n</rule>` (already at 10; 12 is upstream's own tuning example). Re-run Step 1 after edit. |
| `NPathComplexity` | **> 10**: append `<rule ref="codesize/NPathComplexity">\n  <properties><property name="minimum" value="400"/></properties>\n</rule>` (default 200). Re-run Step 1. |
| `ExcessiveClassLength` | **any**: do **not** tune — record the file list in §9 (this is the signal messrust exists to surface). Its property is `minimum` (default 1000) if ever tuned later. |
| `ShortMethodName` | **any**: names <3 chars outside `at,of,or,eq` — record in §9; suppress in source later only if a real name must stay. |
| `LongVariable` / `LongClassName` / `CouplingBetweenObjects` / `LackOfCohesionOfMethods` | **any**: record in §9; tune only if a finding is a false positive (property name: `maximum` for these — messrust §8.4). Note: the `rust` preset overrides `LongVariable`'s default 20 → **35**; tune from 35. |
| `ShortClassName` | **any**: record in §9. Property is **`minimum`** (default 3) — *not* `maximum` — with a companion `exceptions` list (messrust §8.4 corrects any blanket `maximum` reading). |
| any other rule | **> 25**: append `<exclude name="RuleName"/>` inside the `rust` ref **only with a written reason in §9**. |

- [x] **Step 3: Record the observations**

Append a new section to `docs/refs/quality_gates_messrust.md`:

```markdown
## 9. First advisory run (2026-09-27)

Rule counts from `messrust src json messrust.xml --ignore-tests --reportfile`
(counts as measured, key excerpts):

```
<paste /tmp/mess-rule-counts.txt verbatim>
```

Decisions taken: <threshold edits applied, or "no threshold edits — policy
accepted as-is">.
Notable findings kept as signals (esp. `ExcessiveClassLength` files):
<list, or "none">
```

- [x] **Step 4: Verify still runnable**

Run: `mise run mess -- --ignore-violations-on-exit; echo "rc=$?"`
Expected: rc=0; no config errors.

- [x] **Step 5: Commit**

```bash
git add messrust.xml docs/refs/quality_gates_messrust.md
git commit -m "chore: triage first messrust run and tune policy"
```

---

### Task 5: Install mutarust via mise

**Files:**
- Modify: `mise.toml` (Testing & Quality block)
- Modify: `mise.lock`

- [x] **Step 1: Absent (red)**

Run: `command -v mutarust || echo "mutarust: not installed"`
Expected: `mutarust: not installed`

- [x] **Step 2: Add tools line**

After the messrust line inserted in Task 1, insert:

```toml
"cargo:mutarust" = "0.1.10"
```

- [x] **Step 3: Lock**

Run: `mise lock`
Expected: exit 0; `rg 'mutarust' mise.lock` prints an entry.
Fallback A: fresh-pin refusal → `"cargo:mutarust" = "0.1.9"`, re-lock, note it.
Fallback B (crate/version unpublished — vendored `docs/parity.md` says crates.io publication was still issue #28): switch to the git backend
`"cargo:https://github.com/quality-gates/mutarust" = { version = "tag:v0.1.10" }`
(or the latest existing tag if that ref 404s — check `git ls-remote --tags https://github.com/quality-gates/mutarust`), re-lock, note the substitution and the actual version.

- [x] **Step 4: Install**

Run: `mise install`
Expected: exit 0. (cargo-binstall may report no prebuilt asset and fall back to source build — that is normal, not an error.)

- [x] **Step 5: Verify (green) + doc-conformance spot check**

Run:
```bash
mutarust --version; echo "rc=$?"
mutarust --help | grep -cE -- '--logger-agentic-json|--git-diff-lines|--run-mutant-id'
```
Expected: rc=0; version output resolves the pin; grep count ≥ 3 (proves the installed build matches the vendored docs' flag surface — the docs were generated against 0.2.0, so this check matters). If flags are missing, STOP: the pin and the docs disagree; escalate before writing config/tasks.

- [x] **Step 6: Commit**

```bash
git add mise.toml mise.lock
git commit -m "chore: install mutarust via mise"
```

---

### Task 6: `mutarust.yml` policy (fail-first)

**Files:**
- Create: `mutarust.yml` (repo root)

- [x] **Step 1: Run without the config (red)**

Run: `mise exec -- mutarust --config mutarust.yml --dry-run; echo "rc=$?"`
Expected: nonzero rc (3 config error or 1 command error) with a message naming `mutarust.yml`.

- [x] **Step 2: Write the policy**

Create `mutarust.yml` at repo root:

```yaml
# mutarust.yml — mutation-testing policy for traces-pkm.
# mutarust reads this ONLY via `--config mutarust.yml` (no auto-discovery);
# the `mutants` mise task always passes it. Schema (11 fields,
# additionalProperties: false): docs/refs/mutarust/schema/mutarust.schema.json

# Reports land in CWD (all gitignored): report.json (triage) and
# mutarust-report.html (human). mutarust-agentic.json comes from the task's
# --logger-agentic-json. NOTE: report.json is clobbered by every run,
# including --dry-run — read stats immediately after a scored run.
json_output: true
html_output: true
silent_mode: false

# Port of .cargo/mutants.toml exclude_globs "src/cli/**" (path prefix).
# Accepted gaps (no file-level exclude exists): src/main.rs and the 8 non-cli
# src/**/error.rs files ARE mutated, and exclude_re has no port, so impl-body
# mutants appear that cargo-mutants skipped (noisier than the old baseline).
# Absorbed by the committed baseline; revisit with `// mutator-disable-func`
# annotations only if escape noise demands it.
exclude_dirs:
  - src/cli

# Zero-yield operator group for this repo (no tokio-style select! sites in
# src/; see quality_gates_mutarust.md §9.17) — drops mutants that only burn
# build time.
disable_mutators:
  - "select/*"

# Do NOT enable without a --dry-run A/B: this repo gates many items with
# #[cfg(any(test, feature = "test-utils"))] and the docs never classify
# `any(test, …)` (§9.3) — enabling could suppress active test-support mutants.
skip_without_test: false
skip_with_cfg: false

# Score gate stays commented until the stage-2 baseline run (Task 11):
# enable ONLY if measured MSI >= 60 (ci.md "active" tier; score strictly
# below min_msi exits 4). The same ci.md row's companion 75 is min_covered_msi,
# which we do NOT set: without --coverage a positive min_covered_msi exits 4.
# min_msi: 60

enable_mutators: []
ignore_source_lines: []
```

- [x] **Step 3: Run with the config (green)**

Run: `mise exec -- mutarust --config mutarust.yml --dry-run; echo "rc=$?"`
Expected: rc=0 and a mutant count printed.
**Note:** mutarust's no-target default is *undocumented* (docs only document a default for `--list-files`). If this errors about a missing/invalid target, rerun with an explicit `./src...`, record which form worked in Task 10's §11 record (row "default target"), and continue — the `mutants` task always supplies a target anyway.

- [x] **Step 4: Validate schema-shape acceptance**

Run: `mise exec -- mutarust --config mutarust.yml --dry-run 2>&1 | head -5`
Expected: no config/schema error lines (unknown field or bad type would exit 3 naming the file).

- [x] **Step 5: Commit**

```bash
git add mutarust.yml
git commit -m "chore: add mutarust.yml policy"
```

---

### Task 7: Replace `.mise/tasks/mutants/_default` with the mutarust implementation

The entrypoint keeps its stable name `mutants`; only the engine changes (locked decision 9). The legacy cargo-mutants script remains recoverable in git history (needed context for Task 9 is embedded below).

**Files:**
- Modify (replace): `.mise/tasks/mutants/_default` (keep executable bit)

- [x] **Step 1: Confirm the legacy script is what's there today (red)**

Run: `head -6 .mise/tasks/mutants/_default; mise tasks | grep -w mutants`
Expected: line 2 is `#MISE description="Run parallel mutation testing with \`cargo-mutants\`"`; the `mutants` task (and `mutants:report`) are listed with the legacy description.
Also (background, no action): `bash -x .mise/tasks/mutants/_default --list 2>&1 | tail -3` dies at `build_filter_flags` under `set -e` — the legacy task is dead today, which is fine; we are replacing it, not fixing it.

- [x] **Step 2: Replace the file content**

Overwrite `.mise/tasks/mutants/_default` with exactly this content:

```bash
#!/bin/bash
#MISE description="Run mutation testing (engine: `mutarust`)"
#MISE sources=["@group:rust"]
#MISE extends="mutants"
#MISE depends=["test"]
#USAGE flag "-f --file <file>" {
#USAGE   help "Mutate one file only (e.g. -f src/config/service.rs)"
#USAGE }
#USAGE flag "-m --mod <module>" {
#USAGE   help "Mutate one module only (target src/<module>/ recursively)"
#USAGE }
#USAGE // `complete "module"` completes the value after `-m`/`--mod`, not the flag name.
#USAGE complete "module" run="sed -nE 's/^(pub )?mod ([a-z_]+);$/\\2/p' src/lib.rs"
#USAGE flag "--min-msi <n>" help="Fail (exit 4) if total mutation score < n"
#USAGE flag "--update-baseline" {
#USAGE   help "Accept current escapes into mutarust-baseline.json (writes NO reports, exits 0)"
#USAGE }
#USAGE flag "--fail-on-escaped" help="Fail (exit 4) only on escapes absent from mutarust-baseline.json"
#USAGE flag "--git-diff" negate="--no-git-diff" default=#false {
#USAGE   help "Mutate only lines changed vs origin/HEAD (falls back to current branch; untracked files are invisible — stage new files first)"
#USAGE }
#USAGE flag "--dry-run" {
#USAGE   help "Count mutants only; omits --test-flags/--timeout-coefficient (mutual exclusions). Pass as this declared flag, never as passthrough."
#USAGE }
#USAGE flag "--timeout <secs>" help="Fixed per-test timeout; emits --timeout and suppresses the default --timeout-coefficient (mutual exclusions)"
#USAGE arg "[args]" var=#true {
#USAGE   help "Extra arguments forwarded to mutarust"
#USAGE   long_help """
#USAGE   Examples:
#USAGE   - `mise run mutants -m index` - mutate module `index`
#USAGE   - `mise run mutants -m index --dry-run` - count only for one module
#USAGE   - `mise run mutants --min-msi 60 --fail-on-escaped` - gate on new escapes only
#USAGE   - `mise run mutants --git-diff` - only changed lines (tracked; stage new files)
#USAGE   - `mise run mutants -- --list-mutators` - list built-in mutators
#USAGE   - `mise run mutants -- --list-files` - print selected files (NOTE: ignores exclude_dirs)
#USAGE   - `mise run mutants src/cli/error.rs` - explicit target positional (skips the default)
#USAGE   - `mise run mutants -- --run-mutant-id <id>` - re-run one mutant to verify a kill
#USAGE   List/inspect passthrough (--list-files, --list-mutators,
#USAGE   --print-ast) runs WITHOUT --config or task flags (mutarust
#USAGE   rejects them there); declared run flags are rejected (exit 2).
#USAGE   Exit codes: THIS TASK exits 2 for usage errors (bad -m/-f, conflicting
#USAGE   flags). mutarust directly: 0 pass · 1 tool error · 2 bash completion
#USAGE   (not a run) · 3 config/parse error · 4 quality gate (min_msi,
#USAGE   min_covered_msi, or --fail-on-escaped; --run-mutant-id bypasses gates).
#USAGE   A zero-mutant scope with --min-msi exits 4 (score is 0); only --git-diff
#USAGE   auto-pairs --ignore-msi-with-no-mutations. Retry 4s with narrower scope.
#USAGE   Migration from the previous engine: --exclude → config exclude_dirs /
#USAGE   -f / -m; --check → none (drop); --iterate → none (baseline +
#USAGE   --fail-on-escaped); -L/--all-logs → --verbose / --debug (the old -v/-V
#USAGE   were caught/unviable printers; mutarust -V is --version!); --json →
#USAGE   report.json (on via this task's config); --jobs → --workers N;
#USAGE   --no-config → never pass (task owns --config).
#USAGE   """
#USAGE }

# Optional cap-lints: Task 10 P3 A/B completed — inconclusive at n=1, so it
# stays off by default. Hypothesis: quality_gates_mise_adoption.md §10.2 row
# 2; observed result: §11 P3. Also affects depends=["test"] and invalidates
# build caches.
# #MISE env={ RUSTFLAGS = "--cap-lints=allow" }

# File description (Google style §4.1) — placed after the #MISE/#USAGE block
# because mise requires its headers at the top of the file.
# The `mutants` task: stable, tool-agnostic entrypoint for mutation testing.
# Engine: mutarust. All conditional builders end in `return 0` (set -e trap).

# shellcheck disable=SC2154  # usage_* are injected by mise
set -euo pipefail

declare -a mutarust_args=()
declare -a passthrough=()

# `--profile mutants` accepted in Task 10 P2 (see
# quality_gates_mise_adoption.md §11).
TEST_FLAGS="--features test-utils --all-targets --profile mutants"
TIMEOUT_COEFFICIENT=5
DEFAULT_TARGET="./src..."

########################################
# Parses variadic passthrough tokens into an array.
# Globals:
#   usage_args
#   passthrough
# Arguments:
#   None
# Outputs:
#   None
# Returns:
#   0 on success.
########################################
parse_passthrough_tokens() {
  # usage_args arrives shell-quoted; mise documents eval-into-array as the
  # pattern for variadic args — intentional (mise_tasks 06_arguments.md).
  eval "passthrough=(${usage_args:-})"
  return 0
}

########################################
# True when the passthrough array contains the given exact token.
# Globals:
#   passthrough
# Arguments:
#   $1 - token to look for.
# Outputs:
#   None
# Returns:
#   0 when present, 1 otherwise.
########################################
passthrough_has_token() {
  local t
  for t in "${passthrough[@]+"${passthrough[@]}"}"; do
    if [[ "${t}" == "$1" ]]; then
      return 0
    fi
  done
  return 1
}

########################################
# True when a fixed per-test timeout is requested (declared --timeout, or
# passthrough --timeout/--exec-timeout) — either suppresses the task's
# --timeout-coefficient, which mutarust rejects alongside them.
# Globals:
#   usage_timeout
# Arguments:
#   None
# Outputs:
#   None
# Returns:
#   0 when a fixed timeout is present, 1 otherwise.
########################################
has_fixed_timeout() {
  if [[ -n "${usage_timeout:-}" ]]; then
    return 0
  fi
  if passthrough_has_token "--timeout"; then
    return 0
  fi
  if passthrough_has_token "--exec-timeout"; then
    return 0
  fi
  return 1
}

########################################
# Rejects flag combinations mutarust rejects opaquely, giving a task-level
# diagnostic instead: task-owned --config/--test-flags, --update-baseline
# vs --run-mutant-id, fixed --timeout/--exec-timeout vs
# --timeout-coefficient, and the declared --update-baseline/--timeout vs
# --dry-run pairings.
# Globals:
#   usage_update_baseline
#   usage_dry_run
#   usage_git_diff
#   usage_timeout
#   passthrough
# Arguments:
#   None
# Outputs:
#   Diagnostic on STDERR for a rejected combination.
# Returns:
#   Exits 2 on a known-bad combination; 0 otherwise.
########################################
reject_conflicting_flags() {
  if [[ "${usage_update_baseline:-false}" == "true" &&
    "${usage_dry_run:-false}" == "true" ]]; then
    echo "mutants: --update-baseline cannot be combined with --dry-run" >&2
    exit 2
  fi
  if [[ "${usage_update_baseline:-false}" == "true" ]] &&
    passthrough_has_token "--run-mutant-id"; then
    echo \
      "mutants: --update-baseline cannot be combined with --run-mutant-id" >&2
    exit 2
  fi
  if [[ -n "${usage_timeout:-}" ]] &&
    { passthrough_has_token "--timeout" ||
      passthrough_has_token "--exec-timeout"; }; then
    echo "mutants: --timeout is declared; drop it from passthrough" >&2
    exit 2
  fi
  if [[ "${usage_dry_run:-false}" == "true" && -n "${usage_timeout:-}" ]]; then
    echo "mutants: --timeout cannot be combined with --dry-run" >&2
    exit 2
  fi
  local t
  for t in "${passthrough[@]+"${passthrough[@]}"}"; do
    case "${t}" in
      --exec | --no-exec)
        if [[ "${usage_dry_run:-false}" == "true" ]]; then
          echo "mutants: ${t} cannot be combined with --dry-run" >&2
          exit 2
        fi
        echo "mutants: ${t} conflicts with task-owned --test-flags" >&2
        exit 2
        ;;
      --config | --config=*)
        echo "mutants: task owns --config; policy lives in mutarust.yml" >&2
        exit 2
        ;;
      --test-flags | --test-flags=*)
        if [[ "${usage_dry_run:-false}" == "true" ]]; then
          echo "mutants: --test-flags cannot be combined with --dry-run" >&2
          exit 2
        fi
        echo "mutants: task owns --test-flags; drop it from passthrough" >&2
        exit 2
        ;;
      --update-baseline)
        if [[ "${usage_dry_run:-false}" == "true" ]]; then
          echo \
            "mutants: --update-baseline cannot be combined with --dry-run" >&2
          exit 2
        fi
        if passthrough_has_token "--run-mutant-id"; then
          echo \
            "mutants: --update-baseline cannot be combined with" \
            "--run-mutant-id" >&2
          exit 2
        fi
        ;;
      --dry-run)
        echo "mutants: use the declared --dry-run flag, not passthrough" >&2
        exit 2
        ;;
      --timeout-coefficient)
        if [[ "${usage_dry_run:-false}" == "true" ]]; then
          echo \
            "mutants: --timeout-coefficient cannot be combined with" \
            "--dry-run" >&2
          exit 2
        fi
        if has_fixed_timeout; then
          echo \
            "mutants: --timeout-coefficient cannot be combined with" \
            "--timeout/--exec-timeout" >&2
          exit 2
        fi
        ;;
      --timeout | --exec-timeout | --workers | \
        --coverage | --per-test | --test-recursive | \
        --do-not-remove-tmp-folder)
        if [[ "${usage_dry_run:-false}" == "true" ]]; then
          echo "mutants: ${t} cannot be combined with --dry-run" >&2
          exit 2
        fi
        ;;
      --git-diff-base)
        if [[ "${usage_git_diff:-false}" != "true" ]]; then
          echo "mutants: --git-diff-base requires the --git-diff flag" >&2
          exit 2
        fi
        ;;
    esac
  done
  return 0
}

########################################
# True when passthrough requests a list/inspect mode. mutarust rejects all
# configuration and mutation options there (verified live on 0.1.10: even
# --verbose and loggers are refused), so main() bypasses every task-owned
# flag and forwards only the list flag, optional target, and raw tokens.
# Globals:
#   passthrough
# Arguments:
#   None
# Outputs:
#   None
# Returns:
#   0 in a list/inspect mode, 1 otherwise.
########################################
is_inspect_mode() {
  if passthrough_has_token "--list-mutators"; then
    return 0
  fi
  if passthrough_has_token "--list-files"; then
    return 0
  fi
  if passthrough_has_token "--print-ast"; then
    return 0
  fi
  return 1
}

########################################
# True when the passthrough array carries a non-flag token — a target the
# user supplied themselves. Flag values are rare in list modes and, when
# present, the mode errors anyway, so treating them as positionals here is
# harmless (it only suppresses the task's default-target append).
# Globals:
#   passthrough
# Arguments:
#   None
# Outputs:
#   None
# Returns:
#   0 when a positional token exists, 1 otherwise.
########################################
passthrough_has_positional() {
  local t
  for t in "${passthrough[@]+"${passthrough[@]}"}"; do
    case "${t}" in
      -*) ;;
      *) return 0 ;;
    esac
  done
  return 1
}

########################################
# Rejects declared run flags in list/inspect modes. mise consumes declared
# flags before the script sees them, so they cannot be forwarded for
# mutarust to diagnose — exit 2 with a task-level message instead.
# Globals:
#   usage_min_msi
#   usage_update_baseline
#   usage_fail_on_escaped
#   usage_git_diff
#   usage_dry_run
#   usage_timeout
# Arguments:
#   None
# Outputs:
#   Diagnostic on STDERR for a rejected flag.
# Returns:
#   Exits 2 when a declared run flag is present; 0 otherwise.
########################################
reject_declared_in_inspect() {
  if [[ -n "${usage_min_msi:-}" ]]; then
    echo "mutants: --min-msi cannot be combined with list/inspect mode" >&2
    exit 2
  fi
  if [[ "${usage_update_baseline:-false}" == "true" ]]; then
    echo \
      "mutants: --update-baseline cannot be combined with list/inspect mode" >&2
    exit 2
  fi
  if [[ "${usage_fail_on_escaped:-false}" == "true" ]]; then
    echo \
      "mutants: --fail-on-escaped cannot be combined with list/inspect mode" >&2
    exit 2
  fi
  if [[ "${usage_git_diff:-false}" == "true" ]]; then
    echo "mutants: --git-diff cannot be combined with list/inspect mode" >&2
    exit 2
  fi
  if [[ "${usage_dry_run:-false}" == "true" ]]; then
    echo "mutants: --dry-run cannot be combined with list/inspect mode" >&2
    exit 2
  fi
  if [[ -n "${usage_timeout:-}" ]]; then
    echo "mutants: --timeout cannot be combined with list/inspect mode" >&2
    exit 2
  fi
  return 0
}

########################################
# Assembles arguments for list/inspect modes. Task-owned flags are omitted
# (mutarust forbids them there); the list flag must precede any target, so
# passthrough is appended first. --list-mutators takes no target; the
# target-taking modes append the -f/-m scope, else DEFAULT_TARGET unless
# the user already supplied a positional.
# Globals:
#   passthrough
#   usage_file
#   usage_mod
#   DEFAULT_TARGET
#   mutarust_args
# Arguments:
#   None
# Outputs:
#   Diagnostic on STDERR for -f/-m with --list-mutators.
# Returns:
#   Exits 2 for -f/-m with --list-mutators; 0 on success.
########################################
build_inspect_args() {
  reject_declared_in_inspect
  mutarust_args+=("${passthrough[@]+"${passthrough[@]}"}")
  if passthrough_has_token "--list-mutators"; then
    if [[ -n "${usage_file:-}" || -n "${usage_mod:-}" ]]; then
      echo "mutants: -f/-m cannot be combined with --list-mutators" >&2
      exit 2
    fi
    return 0
  fi
  if [[ -n "${usage_file:-}" || -n "${usage_mod:-}" ]]; then
    build_target_flags
  elif ! passthrough_has_positional; then
    mutarust_args+=("${DEFAULT_TARGET}")
  fi
  return 0
}

########################################
# Appends policy and logging flags, then the Cargo test controls — omitted
# entirely under --dry-run (mutarust: dry-run ⊥ test/timeout controls).
# A passthrough --timeout/--exec-timeout or --timeout-coefficient wins over
# the task's defaults; a declared --timeout is emitted here.
# Globals:
#   usage_dry_run
#   usage_timeout
#   TEST_FLAGS
#   TIMEOUT_COEFFICIENT
#   mutarust_args
# Arguments:
#   None
# Outputs:
#   None
# Returns:
#   0 on success.
########################################
build_static_flags() {
  mutarust_args+=(--config mutarust.yml --logger-agentic-json)
  if [[ "${usage_dry_run:-false}" != "true" ]]; then
    mutarust_args+=(--test-flags "${TEST_FLAGS}")
    if [[ -n "${usage_timeout:-}" ]]; then
      mutarust_args+=(--timeout "${usage_timeout}")
    elif ! has_fixed_timeout &&
      ! passthrough_has_token "--timeout-coefficient"; then
      mutarust_args+=(--timeout-coefficient "${TIMEOUT_COEFFICIENT}")
    fi
  fi
  return 0
}

########################################
# Maps -f / -m to mutarust TARGET positionals; the default target keeps a
# bare `mise run mutants` scoped to production code. It applies only when
# neither -f/-m nor a user-supplied positional target is present: a bare
# positional invocation like `mise run mutants src/cli/x.rs` scopes to
# the user's target. Value-like tokens such as `4` in `--workers 4`
# suppress the append too, which is harmless — mutarust's own no-target
# default selects the same production tree today (counts 3820 == 3820).
# Globals:
#   usage_file
#   usage_mod
#   passthrough
#   DEFAULT_TARGET
#   mutarust_args
# Arguments:
#   None
# Outputs:
#   Diagnostic on STDERR for an unknown file/module.
# Returns:
#   Exits 2 on a bad scope; 0 on success.
########################################
build_target_flags() {
  if [[ -n "${usage_file:-}" ]]; then
    if [[ ! -f "${usage_file}" ]]; then
      echo "mutants: no such file: ${usage_file}" >&2
      exit 2
    fi
    mutarust_args+=("${usage_file}")
  fi
  if [[ -n "${usage_mod:-}" ]]; then
    if [[ -d "src/${usage_mod}" ]]; then
      mutarust_args+=("./src/${usage_mod}...")
    elif [[ -f "src/${usage_mod}.rs" ]]; then
      mutarust_args+=("./src/${usage_mod}.rs")
    else
      echo "mutants: unknown module: ${usage_mod}" >&2
      exit 2
    fi
  fi
  if [[ -z "${usage_file:-}" && -z "${usage_mod:-}" ]] &&
    ! passthrough_has_positional; then
    mutarust_args+=("${DEFAULT_TARGET}")
  fi
  return 0
}

########################################
# Gate flags. An empty --git-diff-lines run must not trip a min-msi gate
# (cli.md pairs --ignore-msi-with-no-mutations with the score gate).
# Globals:
#   usage_min_msi
#   usage_fail_on_escaped
#   usage_update_baseline
#   usage_git_diff
#   usage_dry_run
#   mutarust_args
# Arguments:
#   None
# Outputs:
#   None
# Returns:
#   0 on success.
########################################
build_gate_flags() {
  [[ -n "${usage_min_msi:-}" ]] && mutarust_args+=(--min-msi "${usage_min_msi}")
  [[ "${usage_fail_on_escaped:-false}" == "true" ]] &&
    mutarust_args+=(--fail-on-escaped)
  [[ "${usage_update_baseline:-false}" == "true" ]] &&
    mutarust_args+=(--update-baseline)
  if [[ "${usage_git_diff:-false}" == "true" ]]; then
    mutarust_args+=(--git-diff-lines)
    if [[ -n "${usage_min_msi:-}" ]]; then
      mutarust_args+=(--ignore-msi-with-no-mutations)
    fi
  fi
  [[ "${usage_dry_run:-false}" == "true" ]] && mutarust_args+=(--dry-run)
  return 0
}

########################################
# Assembles the argument list and executes mutarust.
# Globals:
#   mutarust_args
#   passthrough
# Arguments:
#   CLI arguments vector (unused; usage_* carries the parsed values).
# Outputs:
#   mutarust output to STDOUT/STDERR.
# Returns:
#   Status of mutarust.
########################################
main() {
  parse_passthrough_tokens
  reject_conflicting_flags
  if is_inspect_mode; then
    build_inspect_args
  else
    build_static_flags
    build_target_flags
    build_gate_flags
    mutarust_args+=("${passthrough[@]+"${passthrough[@]}"}")
  fi

  mutarust "${mutarust_args[@]+"${mutarust_args[@]}"}"
}

main "$@"
```

- [x] **Step 3: Keep executable + shellcheck**

Run: `chmod +x .mise/tasks/mutants/_default && shellcheck .mise/tasks/mutants/_default; echo "rc=$?"`
Expected: rc=0, no findings.

- [x] **Step 4: Task visible with new description (green)**

Run: `mise tasks | grep -w '^mutants'`
Expected: `mutants` line now reads "Run mutation testing (engine: `mutarust`)" (the `mutants:report` line still exists — deleted at cutover).

- [x] **Step 5: Help renders**

Run: `mise run mutants --help`
Expected: rc=0; usage block showing `-f`, `-m`, `--min-msi`, `--update-baseline`, `--fail-on-escaped`, `--git-diff`, `--dry-run`, `--timeout`, and the long_help (exit codes + migration cheatsheet).

- [x] **Step 6: Passthrough probe (`--` forwarding on file tasks)**

Run: `mise run mutants -- --list-mutators > /tmp/mut-list.txt 2>&1; echo "rc=$?"; tail -n 15 /tmp/mut-list.txt`
Expected: rc=0, mutator names shown from file tail — proves `--`-args land in `usage_args` for this file task (redirect to a file rather than `| head`, which SIGPIPE-kills the `depends` test task and corrupts the rc; the `depends=test` output prefixes the captured stream, so read the tail, not the head). If instead you get a usage error, record it in §11 (Task 10) and stop: every later passthrough step needs rework first.

- [x] **Step 7: Dry-run through the task (proves flag conditionality)**

Run: `mise run mutants --dry-run; echo "rc=$?"`
Expected: rc=0 with a count. If the static `--test-flags`/`--timeout-coefficient` were *not* omitted, mutarust would reject the line (rc 1/3) — rc=0 proves the condition works.

- [x] **Step 8: Default-target scope**

Run: `mise run mutants -- --list-files > /tmp/mf.txt 2>&1; echo "rc=$?"; wc -l < /tmp/mf.txt; grep -c 'src/cli' /tmp/mf.txt || true`
Expected: rc=0; count > 100 files (the task appends DEFAULT_TARGET `./src...` → 123); `src/cli` lines **will** appear — paths are ABSOLUTE (match with plain substring `src/cli`, not `^src/cli`), and `exclude_dirs` intentionally does not affect `--list-files` (known quirk, §9.18.5); scope is enforced at run time.
Branch: if mutarust rejects `./src...` (rc 1/3 with a usage/target error), edit `DEFAULT_TARGET` to `"./src"`, re-run Step 7 (expect rc=0), then compare `mise run mutants --dry-run` counts before/after the change to confirm the fallback still selects the full production tree; record the outcome in Task 10's §11 record (`default target` row).

- [x] **Step 9: `-m` mapping for dir and file modules**

Run: `mise run mutants -m index --dry-run; echo "rc=$?"` then `mise run mutants -m strsim --dry-run; echo "rc=$?"`
Expected: both rc=0 (`index` is a dir → `./src/index...`; `strsim` is `src/strsim.rs` → file target).

- [x] **Step 10: Unknown module errors cleanly**

Run: `mise run mutants -m nosuchmod --dry-run; echo "rc=$?"`
Expected: rc=2, stderr `mutants: unknown module: nosuchmod`.

- [x] **Step 11: Conflicting flags reject at task level**

Run: `mise run mutants --update-baseline --dry-run; echo "rc=$?"` then `mise run mutants --dry-run -- --workers 4; echo "rc=$?"`
Expected: both rc=2 with `mutants:` diagnostics (never reaching mutarust).

- [x] **Step 12: Commit**

```bash
git add .mise/tasks/mutants/_default
git commit -m "feat(mutants): swap task engine from cargo-mutants to mutarust"
```

---

### Task 8: gitignore + smoke tests (report-on-exit-4 FIRST)

**Files:**
- Modify: `.gitignore` (after line 22 `/mutants-report.md`)

- [x] **Step 1: Add mutarust artifacts to .gitignore**

After the `/mutants-report.md` line, insert:

```gitignore
/report.json
/mutarust-agentic.json
/mutarust-summary.json
/mutarust-gitlab.json
/mutarust-report.html
```

(`mutarust-baseline.json` is deliberately NOT ignored — it gets committed in Task 11.)

- [x] **Step 2: Pick a small test module**

Run:
```bash
for m in strsim delimiter hash position dirs duration; do
  printf '%s: ' "$m"
  mise run mutants -m "$m" --dry-run 2>/dev/null | tail -1
done
```
Expected: one printed count per module. Choose the module with the **smallest count > 0**; call it `M` for later steps (it must work for both engines: raw cargo-mutants in Task 9 handles dir modules via `src/<M>/**/*.rs` and file modules via `src/<M>.rs`).

- [x] **Step 3: CRITICAL — reports must exist on gate failure (exit 4)**

Run: `mise run mutants -m M --min-msi 99 -- --timeout 600 > /tmp/m4.txt 2>&1; echo "rc=$?"` (substitute the real module), then `tail -20 /tmp/m4.txt`.
Expected: rc=4 (or rc=0 if M scored 100 — check `jq '.stats.msi' report.json`), **and**:
`ls -la report.json mutarust-agentic.json mutarust-report.html` → **all three exist**, `jq '.escaped_count' mutarust-agentic.json` prints a number.
This run also proves timeout conditionality under *both* possible `--`-parse readings (declared `--timeout` is emitted from `usage_timeout`; if instead raw-passthrough, `has_fixed_timeout` suppresses the coefficient).
**Triage before escalating:**
- rc 1/3 or the run stopped *before* mutation (clean test suite failed — see tail of `/tmp/m4.txt`) → fix the build/test failure first; missing reports are then expected, not a design failure.
- Gate genuinely exited 4 but a report file is missing → **stop.** Record the failure in `docs/refs/quality_gates_mise_adoption.md` §8 checklist and escalate to the user — the agent triage design depends on reports surviving exit 4.

- [x] **Step 4: Zero-mutant run still writes reports**

Run: `rm -f report.json && mise run mutants --git-diff; echo "rc=$?"; jq -c '.stats' report.json`
Expected: rc=0 (docs-only changes ⇒ zero mutable lines) **and** `report.json` exists again with zero-mutant stats. If the file was not rewritten on the empty run, note it in Task 10's §11 record (limitation: rely on stdout for empty runs).
**Reminder:** report.json is clobbered by *every* later run — never read it after an intervening `--dry-run`.

- [x] **Step 5: Mutator inventory reachable**

Run: `mise run mutants -- --list-mutators > /tmp/mutators.txt 2>&1; echo "rc=$?"; tail -n 15 /tmp/mutators.txt`
Expected: rc=0, mutator names shown from file tail.

- [x] **Step 6: Duplicate `--config` precedence**

Run: `printf 'silent_mode: false\n' > /tmp/other.yml && mise run mutants -m strsim --dry-run -- --config /tmp/other.yml > /tmp/cfg.txt 2>&1; echo "rc=$?"; grep -m1 'task owns --config' /tmp/cfg.txt`
Expected: rc=2 and message `mutants: task owns --config; policy lives in mutarust.yml` — the `mutants` task rejects passthrough `--config` itself (per Task 7's conflict-matrix hardening (passthrough `--config` now rejected at task level); raw mutarust would instead rc=3 with `--config can be supplied only once`). The step's purpose: confirm the task's pre-emptive rejection fires (not mutarust's). Record in Task 10's §11 record: `--config passthrough: rejected at task level (rc=2, task-owns message) — task config always wins; the losing-config/report-vanish scenario is unreachable.`

- [x] **Step 7: Commit**

```bash
git add .gitignore
git commit -m "chore: ignore mutarust report artifacts"
```

---

### Task 9: P1a — old-engine baseline (raw `cargo mutants`)

Task 7 replaced the legacy task script, so the old engine is measured via raw CLI — which is the honest baseline anyway, since the legacy task never ran past its `set -e` trap (Grounding facts). The flags below replicate what the legacy script assembled (captured from the pre-swap file; recoverable via `git log -p -- .mise/tasks/mutants/_default`). `.cargo/mutants.toml` still exists and is auto-applied (nextest, `test-utils` feature, excludes, `timeout_multiplier = 5`).

**Files:** none written outside gitignored `mutants.out/`

- [x] **Step 1: Confirm both engines available**

Run: `mise exec -- cargo mutants --version; mutarust --version`
Expected: both rc=0.

- [x] **Step 2: Run the old engine on module M**

Run (substitute `M`; choose the `--file` form by module kind):
```bash
if [[ -d "src/M" ]]; then T="src/M/**/*.rs"; else T="src/M.rs"; fi
/usr/bin/time -p mise exec -- cargo mutants --file "${T}" \
  --baseline run --cap-lints true --build-timeout-multiplier 3 \
  --profile mutants --iterate > /tmp/ab-old.txt 2>&1
echo "old_rc=$?"
tail -3 /tmp/ab-old.txt
```
Expected: rc=0 (a `cargo mutants` failure here is a real blocker for the A/B — read `/tmp/ab-old.txt` and fix before proceeding). Record: wall-clock seconds (`real` from `time -p`), survivor count `if [[ -f mutants.out/missed.txt ]]; then wc -l < mutants.out/missed.txt; else echo 0; fi`.
Notes to carry into §11: the old engine **always** ran with `--cap-lints true` (relevant to interpreting P3), `--profile mutants`, and nextest (config).

- [x] **Step 3: No commit**

Nothing tracked changes (`mutants.out/` is still gitignored). Proceed to Task 10.

---

### Task 10: Pilot A/B + experiments (record results)

**Files:**
- Modify: `.mise/tasks/mutants/_default` (only if P2 passes)
- Modify: `docs/refs/quality_gates_mise_adoption.md` (append §11)

Use the module `M` chosen in Task 8 Step 2; old-engine numbers come from Task 9.

- [x] **Step 1 (P1): New tool on same scope**

Run: `/usr/bin/time -p mise run mutants -m M > /tmp/ab-new.txt 2>&1; echo "new_rc=$?"`
Expected: rc=0 (no gates yet). Record wall-clock from `time -p` and `jq -c '.stats' report.json` (escapedCount, msi, skippedCount) — read it **immediately**, before any other run clobbers it.

- [x] **Step 2 (P2): `--profile mutants` in `--test-flags`**

Run:
```bash
mise exec -- mutarust --config mutarust.yml --logger-agentic-json \
  --test-flags "--features test-utils --all-targets --profile mutants" \
  --timeout-coefficient 5 "./src/M..."
```
Two outcomes:
- **Accepted** (rc=0 or 4, run completes): edit `TEST_FLAGS` in `.mise/tasks/mutants/_default` to `"--features test-utils --all-targets --profile mutants"`; `[profile.mutants]` in `Cargo.toml` is retained at cutover. Re-verify: `mise run mutants -m M --dry-run` → rc 0 (dry-run still omits it).
- **Rejected** (rc=1 or 3, argument error): record "profile does not port"; `[profile.mutants]` gets removed in Task 12.

- [x] **Step 3 (P3): cap-lints A/B (highest-risk gap)**

Run:
```bash
RUSTFLAGS=--cap-lints=allow mise run mutants -m M > /tmp/cl-on.txt 2>&1
jq -c '.stats' report.json
mise run mutants -m M > /tmp/cl-off.txt 2>&1
jq -c '.stats' report.json
```
Record both stats objects (read `report.json` immediately after each run). Decision rule (record, do not enable):
- If `skippedCount(off) − skippedCount(on) > 5% of totalMutantsCount` → lint-driven build failures ARE inflating the score; document prominently as a known bias of the default configuration and leave the commented `#MISE env` line as the opt-in.
- Otherwise → document "no material inflation observed on M"; still leave it off by default (cache invalidation cost).
Context for the write-up: the old engine *always* cap-lints (`--cap-lints true`, Task 9), so "off" here is a deliberate divergence from old behavior.

- [x] **Step 4 (P4): Record everything**

Append to `docs/refs/quality_gates_mise_adoption.md`:

```markdown
## 11. Pilot results (observed 2026-09-27)

Scope module: `M` = <module>

| | wall-clock | rc | escaped | skipped | msi |
| --- | --- | --- | --- | --- | --- |
| old engine, raw `cargo mutants` (Task 9) | <s> | | <missed.txt lines> | n/a | n/a |
| `mise run mutants -m M` (mutarust) | <s> | | | | |

Old-engine invocation notes: raw CLI (legacy task was dead under set -e);
flags replicated; nextest + excludes via .cargo/mutants.toml; always
--cap-lints true, --profile mutants.

P2 (`--profile mutants`): <accepted → TEST_FLAGS updated | rejected → reason>
P3 (cap-lints): stats with RUSTFLAGS=<...>; stats without=<...>;
  skippedCount delta=<n> of <total> → <inflation | no material inflation>.
`default target` (`./src...`): <accepted | replaced with <fallback> — notes>
Duplicate `--config` passthrough: rejected at task level (rc=2, task-owns
  message) — task config always wins; losing-config scenario unreachable.
Passthrough probe (`-- --list-mutators`): <worked | usage error → rework>
Any other surprises: <list or "none">
```

- [x] **Step 5: Commit**

```bash
git add docs/refs/quality_gates_mise_adoption.md .mise/tasks/mutants/_default
git commit -m "chore: record quality-gate pilot results"
```
(The second path is staged even when P2 didn't change it — unstage if `git status` shows it unmodified.)

---

### Task 11: Stage 2 — scoped run + committed baseline

**Deviation note (2026-09-28):** the original full-project run (3,820 mutants × rebuild+test each ≈ 16–24 h per invocation, up to 3 invocations, no resume) was killed mid-flight — multi-hour runs are ruled out on this machine. All steps below run on a **small `--match` scope** instead; the full-project baseline + MSI measurement is deferred to the follow-up issue created in Step 7. Consequence (documented, accepted): the committed baseline covers only scoped escapes, so a future full-scope `--fail-on-escaped` exits 4 until the official full `--update-baseline` run happens — exactly what the issue tracks.

**Files:**
- Create: `mutarust-baseline.json` (commit)
- Modify: `mutarust.yml` (gate comment — measurement record only; never activated from scoped data)
- Modify: `docs/refs/quality_gates_mise_adoption.md` (§11 record)
- Create: `.scratch/<feature-slug>/issues/01-full-project-mutation-baseline.md` (follow-up)

- [x] **Step 0: Pick + probe the scope**

Run: `nice -n 10 mise run mutants -m strsim --dry-run > /tmp/scoped-dry.txt 2>&1; echo "rc=$?"; tail -3 /tmp/scoped-dry.txt`
Expected: rc=0, `Total: 6 mutation(s)`. Default scope is `-m strsim` (known-good, 6 mutants). Optionally probe ONE broader alternation regexp (e.g. `-m 'strsim|dirs'`) and accept it only if the total stays in **6–40**; >40 or rc≠0 → fall back to `-m strsim`. Record the chosen `<SCOPE>` regexp + total. (Dry-run clobbers `report.json` — harmless; stats come after Step 1.)

- [x] **Step 1: Scoped scored run**

Run: `nice -n 10 mise run mutants -m <SCOPE> > /tmp/scoped-run.txt 2>&1; echo "rc=$?"`
Expected: rc=0 (no score gates configured). Runtime ≤ ~10 min (task hooks run the test suite first, then ≤ min(12, total) workers build in isolated temp dirs). If > 15 min, stop and investigate — report the verbatim tail of `/tmp/scoped-run.txt`. Run nothing else cargo-heavy concurrently.

- [x] **Step 2: Record the score**

Run: `jq -c '.stats' report.json | tee /tmp/scoped-stats.json`
Expected: all counts + `msi` (ratio 0–1). Save the output — it goes into the §11 record (Step 6) and the Step 4 comment. Do this **before** any other mutarust invocation (reports are clobbered per run).

- [x] **Step 3: Write the baseline**

Run: `nice -n 10 mise run mutants -m <SCOPE> --update-baseline; echo "rc=$?"` then `jq '.mutants | length' mutarust-baseline.json`
Expected: rc=0; length equals the Step 1 escaped count (0 is valid — an empty `mutants` array still makes the Step 5 gate provable). Note: by design this writes **no report files** and exits before gates — do not mistake exit 0 for a completed scored run. If the scope probe (Step 1) produced different results than Step 1's record, stop and reconcile before proceeding.

- [x] **Step 4: Score gate decision (scoped — never activates)**

Run: `jq '.stats.msi' report.json`
- Scoped data must **not** activate the project gate regardless of value: leave `# min_msi: 60` commented; replace the comment block's last line with `# min_msi: 60  # scoped measurement MSI = <value> on <scope> 2026-09-28; full-project measurement deferred (see .scratch/<feature-slug>/issues/01-full-project-mutation-baseline.md)`.
- Record the value + decision in §11 either way (Step 6).

- [x] **Step 5: Green gate with baseline (scoped, green)**

Run: `nice -n 10 mise run mutants -m <SCOPE> --fail-on-escaped > /tmp/gate.txt 2>&1; echo "rc=$?"; jq -c '.stats' report.json`
Expected: rc=0 (every escape of this scope is in the committed baseline). rc=4 here means `mutarust-baseline.json` is stale vs the run — re-run Step 3 once and retry.

- [x] **Step 6: Record in §11**

Append to `docs/refs/quality_gates_mise_adoption.md` §11 (match the existing entries' style): date 2026-09-28, scope regexp + total, killed/escaped/msi from Step 2, baseline length, gate rc from Step 5, decision (`min_msi` stays commented — scoped measurement cannot activate a project gate), and a pointer to the Step 7 issue.

- [x] **Step 7: Follow-up issue + commits**

Create `.scratch/<feature-slug>/issues/01-full-project-mutation-baseline.md` per `docs/agents/issue-tracker.md` (Status line per `docs/agents/triage-labels.md`; pick/reuse a sensible feature slug — check what already exists under `.scratch/`). Body: why deferred (user constraint: no multi-hour runs; the killed attempt's evidence), acceptance criteria (full-scope scored run; `min_msi` activation only from full-project MSI ≥ 60; baseline covering all current escapes), and the deferred procedure:

```bash
nice -n 10 script -q /tmp/full-run.txt mise exec -- mutarust \
  --config mutarust.yml --logger-agentic-json --workers 4 \
  --test-flags "<TEST_FLAGS value from the task>" --timeout-coefficient 5 "./src..."
```

with the run-count collapse: scored run → construct `mutarust-baseline.json` from `report.json`'s `escaped[]` (`{id, file: originalFilePath, mutator: mutatorName, line: originalStartLine}` per the Mutago-compatible format in `docs/refs/mutarust/docs/cli.md`) → one `--fail-on-escaped` verification run; on rc=4 fall back to the official `--update-baseline` full run. Note: no resume — any kill restarts from zero; keep the machine otherwise idle; 3 runs → 2 runs.

Commits (hooks, no bypasses):
```bash
git add mutarust-baseline.json mutarust.yml
git commit -m "chore: commit scoped mutarust baseline"
git add docs/refs/quality_gates_mise_adoption.md .scratch/
git commit -m "docs: record scoped mutation measurement + full-run follow-up"
```
Expected: `git status --porcelain` empty.

---

### Task 12: Stage 3 — retire cargo-mutants

**Files:**
- Modify: `mise.toml` (remove tools line)
- Modify: `mise.lock` (regenerate)
- Delete: `.mise/tasks/mutants/report`, `.cargo/mutants.toml` (**keep** `.mise/tasks/mutants/_default` — it is the live task)
- Modify: `.gitignore` (remove 3 lines)
- Modify: `Cargo.toml` (conditional: remove `[profile.mutants]`)

- [x] **Step 1: Remove the tools line + relock**

Delete `"cargo:cargo-mutants" = "latest"` from `mise.toml`, then:
Run: `mise lock && mise install && mise ls 2>/dev/null | grep -c cargo-mutants; echo "rc=$?"`
Expected: lock diff drops cargo-mutants; install exit 0. Note: `mise ls` may still print 1 hit sourced from the ancestor main checkout's `mise.toml` (out of scope — the main branch still declares cargo-mutants until this work merges). Verify cleanliness against this worktree's own files: `rg cargo-mutants mise.toml mise.lock` → no output.

- [x] **Step 2: Delete legacy report task, config, and stale artifacts**

Run: `rm .mise/tasks/mutants/report && rm .cargo/mutants.toml && rm -rf mutants.out mutants.out.old mutants-report.md`
Expected: report task + config gone, stale cargo-mutants run artifacts removed (they must NOT be re-ignored once Step 3 lands, or `git add -A` would sweep them in); **`.mise/tasks/mutants/_default` must still exist**; `git status --porcelain` shows only the intended deletions.

- [x] **Step 3: Clean .gitignore**

Remove these three lines (20–22) from `.gitignore`:

```gitignore
/mutants.out/
/mutants.out.old/
/mutants-report.md
```

- [x] **Step 4: `[profile.mutants]` decision**

- If Task 10 P2 **passed** (TEST_FLAGS contains `--profile mutants`): keep `Cargo.toml:264-267` unchanged.
- If P2 **failed**: remove from `Cargo.toml`:

```toml
[profile.mutants]
inherits = "test"
opt-level = 1
debug = false
```

- [x] **Step 5: No dangling references in code/config**

Run:
```bash
rg -n 'cargo-mutants|mutants\.out|mutants-report|mutants\.toml' \
  --hidden -g '!.git' -g '!docs/refs/quality_gates_*' \
  -g '!docs/refs/mutation_testing.md' -g '!docs/superpowers/**' \
  -g '!mutarust.yml' -g '!.mise/tasks/clean/**' .
```
Expected: no output. (Allowed leftovers outside these exclusions: the research docs, the plan itself, `docs/refs/mutation_testing.md` — rewritten next task, `mutarust.yml` provenance comments, and the `.mise/tasks/clean/*` stale-artifact registry, which intentionally keeps deleting old cargo-mutants artifacts when present. The live `.mise/tasks/mutants/_default` intentionally contains none of these strings — its cheatsheet says "the previous engine", not `cargo-mutants`.) Also run `rg 'cargo-mutants' mise.lock` → no output.

- [x] **Step 6: Entrypoints healthy**

Run: `mise run mutants -m strsim --dry-run; echo "mu_rc=$?"` then `out=$(mise run mutants:report 2>&1); rep_rc=$?; printf '%s\n' "$out" | tail -2; echo "rep_rc=$rep_rc"` (capture `rep_rc` BEFORE piping — a `cmd | tail; echo $?` pipeline reports `tail`'s status, not `mise`'s).
Expected: `mu_rc=0` (the stable `mutants` entrypoint runs the mutarust engine); `mutants:report` errors with a task-not-found style message and nonzero `rep_rc`. Caveat: mise resolves tasks through ancestor configs too — if `rep_rc=0` because the main checkout still ships `.mise/tasks/mutants/report`, fall back to the worktree-local assertion: `.mise/tasks/mutants/` contains only `_default` and `mise tasks | grep -c mutants:report` → 0.

- [x] **Step 7: Commit**

```bash
git add -A mise.toml mise.lock .mise/tasks .cargo .gitignore Cargo.toml
git commit -m "chore: retire cargo-mutants in favor of mutarust"
```

---

### Task 13: Rewrite `docs/refs/mutation_testing.md`

**Files:**
- Rewrite: `docs/refs/mutation_testing.md` (replace all 190 lines)

- [x] **Step 1: Replace the file**

Write `docs/refs/mutation_testing.md` with exactly this content:

````markdown
# Mutation Testing Setup

Mutation testing evaluates a test suite by injecting small artificial defects
("mutants") into the source and checking whether the tests detect them.
Unlike line coverage — which only says code was *executed* — mutation testing
measures whether assertions would *notice* the change.

---

## 1. Terminology

- **Mutant**: a synthetic bug injected into one location (e.g. `>` → `>=`,
  removing a statement, replacing a return value).
- **killed**: the suite failed on the mutated code — desired.
- **escaped (survived)**: the suite passed despite the bug — a coverage or
  assertion gap.
- **errored**: the mutant ran but the test command failed or timed out.
- **skipped**: never run — the mutation does not compile, no tests reference
  it, it is cfg-gated, or type-proof machinery declined it. Skipped mutants
  count toward the score — they are not evidence of test strength.
- **MSI**: mutation score = (killed + errored + skipped) / total, reported as
  a 0–1 ratio in JSON and accepted as 0–100 on `--min-msi`.

---

## 2. Tooling: `mutarust` (+ `cargo-nextest` for the test task)

1. **`mutarust`** — the mutation engine. Installed via `mise` in
   `mise.toml` `[tools]` (`"cargo:mutarust" = "0.1.10"`), wrapped by the
   **tool-agnostic `mutants` task** (`.mise/tasks/mutants/_default`) — the
   entrypoint name predates the engine and outlives it.
2. **`cargo-nextest`** — still used by the `mise run test` task for its
   parallel runner. Mutation runs themselves use plain
   `cargo test` (mutarust's default); the custom `--exec` nextest path is
   serial and silently skips type-proof mutators, so it is not used.

Config lives in two committed files (per-tool convention, like
`clippy.toml`/`deny.toml`):

- `mutarust.yml` — policy (exclusions, mutators, outputs, score gate).
  mutarust has **no config auto-discovery**: mutation runs always pass
  `--config mutarust.yml`; list/inspect passthrough runs (`--list-mutators`,
  `--list-files`, `--print-ast`) run without it.
- `mutarust-baseline.json` — the accepted-escapes baseline, written by
  `--update-baseline` and meant to be committed like a lockfile.

---

## 3. The `mutants` mise task

```bash
mise run mutants                 # full scope (./src...), score-reporting
mise run mutants -m index        # one module (src/index/ recursively)
mise run mutants -f src/hash.rs  # one file
mise run mutants --dry-run       # count mutants, no test runs
mise run mutants --git-diff      # only changed tracked lines (stage new files!)
```

Declared flags (see `mise run mutants --help` for the full contract):

| Flag | Meaning |
| --- | --- |
| `-f` / `-m` | scope to one file / one module (with `src/lib.rs` completion) |
| `--min-msi <n>` | exit 4 if total score < n (0–100) |
| `--update-baseline` | accept current escapes into the baseline; writes **no** reports |
| `--fail-on-escaped` | exit 4 only on escapes *not* in the baseline |
| `--git-diff` | scope to lines changed vs `origin/HEAD` (falls back to the current branch) |
| `--dry-run` | count only (omits `--test-flags`/timeout flags) |
| `--timeout <secs>` | fixed per-test timeout (suppresses `--timeout-coefficient`) |
| `[args]` | passthrough after `--` (e.g. `--list-mutators`, `--workers 4`) |

Static behavior of the task: `--config mutarust.yml --logger-agentic-json
--test-flags "--features test-utils --all-targets --profile mutants"
--timeout-coefficient 5`, `depends = ["test"]`, 1 h template timeout.
Conflicting flag combinations (e.g. `--update-baseline --dry-run`) are
rejected at the task level with exit 2 and a `mutants:` message.

**Exit codes:** task usage errors exit `2`. mutarust itself: `0` pass ·
`1` tool error · `2` bash completion (not a run) · `3` config/parse/
annotation error · `4` quality gate red (`min_msi`, `min_covered_msi`, or
`--fail-on-escaped`; `--run-mutant-id` bypasses gates). **Four is the only
failure worth retrying with a narrower scope.** Any zero-mutant scope with
`--min-msi` exits 4 (score 0); only `--git-diff` auto-pairs
`--ignore-msi-with-no-mutations` — it only takes effect paired with
`--min-msi` (the task pairs them).

### Scoping / performance knobs

```bash
mise run mutants --git-diff          # cheapest meaningful local run
mise run mutants -m query            # bounded scope while iterating
mise run mutants -- --workers 4      # cap parallel workers
```
Workers × cargo `-j` share the CPUs; results always print in plan order.
There is no `--iterate`: rerun cost is controlled by *scoping*, and the
baseline only changes what **fails**, not what runs.

---

## 4. Baseline workflow (the methodology centerpiece)

```bash
# 1. Full scored run
mise run mutants

# 2. Accept every current escape as policy
mise run mutants --update-baseline
git add mutarust-baseline.json && git commit -m "chore: commit mutarust baseline"

# 3. Day-to-day gate: only NEW escapes fail
mise run mutants --fail-on-escaped        # rc=0 → no regressions

# 4. Kill a new escape (agent loop)
jq '.mutants[0]' mutarust-agentic.json     # id, diff, context_lines, kill_hint
#    pick an id NOT already in mutarust-baseline.json (.mutants[0] may be
#    an accepted escape; the agentic report carries all escapes of the run)
#    ...write a targeted assertion in the nearby test file...
mise run mutants -- --run-mutant-id <id>  # re-run just that mutant → killed?
# 5. Accept any remaining intentional escapes
mise run mutants --update-baseline
```

`mutarust-agentic.json` (written by every scored run; `--update-baseline`
writes no reports) is the structured successor of the old
`mutants-report.md` "Instructions for Next Agent Session" output (from the
deleted `.mise/tasks/mutants/report` task): per escape it carries the
mutation diff, context lines, nearby test files, and a kill hint. Human
triage: stdout table, `report.json`, and `mutarust-report.html`.

Report files (all gitignored, written to CWD after a completed run):
`report.json`, `mutarust-agentic.json`, `mutarust-report.html`. Every run
that produces reports — including `--dry-run` — overwrites them: read
stats right after the run you care about.

---

## 5. CI / delivery integration (aspirational — no CI job today)

If a mutation job is ever added, scope it to the change set:

```yaml
- name: Install tools via mise
  uses: jdx/mise-action@v2

- name: Mutation test changed lines
  run: mise run mutants --git-diff --min-msi 60 --fail-on-escaped
```

Caveats that make this safe: untracked files are invisible to
`--git-diff-lines` (stage new files first); an empty change-set exits 0 —
the task pairs `--ignore-msi-with-no-mutations` with `--min-msi` so an
empty diff cannot fail the gate.

---

## 6. Version notes

- `mutarust 0.1.10` (pinned in `mise.toml`; bump deliberately — the config
  schema is `docs/refs/mutarust/schema/mutarust.schema.json` and unknown
  fields fail fast).
- Schema/report drift notes and the full flag inventory live in
  `docs/refs/quality_gates_mutarust.md` (§8–§9); migration decisions for
  this repo are in `docs/refs/quality_gates_mise_adoption.md`.
- Replaces `cargo-mutants` (retired 2026-09-28); the historical
  cargo-mutants research remains in `docs/refs/quality_gates_*.md`.
````

- [x] **Step 2: Sanity-scan the rewrite**

Run:
```bash
rg -n 'cargo mutants --|mutants\.out|--in-diff' docs/refs/mutation_testing.md; echo "rc=$?"
rg -n -- '--iterate' docs/refs/mutation_testing.md; echo "rc=$?"
```
Expected: first rg rc=1 (no stale cargo-mutants usage). Second rg rc=0 with **exactly one** hit — the intentional negation line (`There is no \`--iterate\``), which documents the flag's absence; any other `--iterate` hit is stale usage and must be fixed.

- [x] **Step 3: Commit**

```bash
git add docs/refs/mutation_testing.md
git commit -m "docs: rewrite mutation testing guide for mutarust"
```

---

### Task 14: Final verification

- [x] **Step 1: Full gate**

Run: `mise run verify`
Expected: exit 0 (`fmt` → `check`/`lint`/`test` all pass; no Rust sources changed, so this is a plumbing check).

- [x] **Step 2: Both entrypoints from cold**

Run: `mise run mess -- --ignore-violations-on-exit; echo "mess_rc=$?"` then `mise run mutants -m strsim --dry-run; echo "mu_rc=$?"`
Expected: `mess_rc=0`, `mu_rc=0`.

- [x] **Step 3: hk check on changed docs**

Run:
```bash
printf 'docs/refs/quality_gates_messrust.md\0docs/refs/quality_gates_mise_adoption.md\0docs/refs/mutation_testing.md\0docs/superpowers/plans/2026-09-27-quality-gates-messrust-mutarust.md\0' \
  | hk check --skip-step gitleaks --format json --files0-from -
```
Expected: `status: passed` (gitleaks skipped because `--safe` refuses its unknown effect; run `gitleaks detect` manually if secrets are a concern for these files — they contain no secrets).

- [x] **Step 4: Clean tree**

Run: `git status --porcelain`
Expected: empty. `git log --oneline -12` shows the plan's commits in order.

- [x] **Step 5: Final commit (if any verification fix was needed)**

```bash
git add -A
git commit -m "chore: quality-gate verification fixes"
```
(Only if Step 1–4 produced changes; otherwise skip.)

---

# Part II — 2026-09-28: task UX hardening

**Goal:** Replace the `mutants` task's hand-rolled conflict cascades with mise `#USAGE` parse-time constraints over a strict declared flag surface, register mutarust reports in `clean:reports`, and equip the advisory `mess` task with real help + a workflow doc.

**Architecture:** `.mise/tasks/mutants/_default` declares every mutarust flag (minus five task-owned refusals) with `conflicts`/`exclusive`/`requires` constraints; a small script-side target scan replaces the cascades (usage validation exits 1, scan exits 2). `clean/reports` gains the five mutarust root files. `mess` keeps free-form passthrough but gains `help`/`long_help`; `quality_gates_messrust.md` gains a Workflow section; `mutation_testing.md` is updated to match the new UX.

**Tech Stack:** mise task files (`#USAGE` KDL-lite spec), bash (`set -euo pipefail`), mutarust 0.1.10, messrust, markdown docs.

**Spec:** `docs/superpowers/specs/2026-09-28-quality-gates-task-ux-design.md` (approved, committed `01933d0d`).

---

## Ground rules (era 2)

Universal standing rules are assembled once at the top of this document in **Ground rules (standing, non-negotiable)**. Era-2 carry-forwards:

- **Worktree:** `/Users/jack/Documents/41_personal/traces-pkm/.worktrees/quality-gates` (branch `quality-gates`). Never touch the main checkout. *(retired 2026-09-29 — the worktree was retired mid-Part III; see Incidents I1)*
- **Verification lives in the repo, not /tmp.** Usage-constraint enforcement was nondeterministic outside this project (settings drift); inside the worktree it was stable across repeat runs. All probes below run from the worktree root. *(standing; its worktree-root clause retired 2026-09-29)*
- **Caching quirks (spike-verified, mise 2026.9.15):**
  - `mise run --force` bypasses task freshness/replay; `mise run --skip-deps` skips `depends=["test"]` (saves ~10 s per probe — use it for every parse-level case).
  - `conflicts="--a --b"` (space-separated in one attribute) is a **silent no-op**. Multi-selector conflicts MUST use node-args form: `conflicts "--a" "--b"`.
  - `exclusive`/`requires` must be **attributes** on the flag node (child form = invalid spec); `choices` is a child; unknown flags are absorbed by the variadic positional (rc 0) — that is exactly what the target scan catches.
  - A declared flag's `default` **satisfies another flag's `requires`** (verified: `--git-diff` with `default=#false` made `requires="--git-diff"` on `--git-diff-base` never fire). Fix: no `default` on `--git-diff` so `requires` fires on absence, **plus** a script backstop — mise `requires` is satisfied by *any* explicit value, including `--no-git-diff`, which would otherwise slip through to a run.
  - mise strips `--` **before** usage parsing: declared flags after `--` still parse as flags (verified: `-- --workers 4` behaves as `--workers 4` → rc 0). Undeclared post-`--` tokens still hit the target scan (rc 2), so the scan stays.

## File structure

| File | Change |
| --- | --- |
| `.mise/tasks/mutants/_default` | Task 1 — full rewrite of `#USAGE` block (lines 6–54) + script surgery (cascades out, target scan + declared-flag forwarding in); Task 2 verification |
| `.mise/tasks/clean/reports` | Task 3 — registry + `--dry-run` long_help |
| `mise.toml` (`[tasks.mess]`, lines 225–238) | Task 4 — `usage` gains help/long_help (workflow text) |
| `docs/refs/quality_gates_messrust.md` | Task 5 — append `## 10. Workflow` |
| `docs/refs/mutation_testing.md` | Task 6 — six sites updated to the new UX |

No changes to `docs/refs/quality_gates_mise_adoption.md` (all five cited lines are historical research log — see Task 6).

---

### Task 1: `mutants` task — strict declared surface

**Files:**
- Modify: `.mise/tasks/mutants/_default` (replace `#USAGE` block lines 6–54; functions `parse_passthrough_tokens`, `passthrough_has_token`, `has_fixed_timeout`, `reject_conflicting_flags`, `is_inspect_mode`, `passthrough_has_positional`, `reject_declared_in_inspect`, `build_inspect_args`, `build_static_flags`, `build_gate_flags`, `build_target_flags`, `main`)

- [x] **Step 1: Record baseline behavior (red)**

```bash
cd /Users/jack/Documents/41_personal/traces-pkm/.worktrees/quality-gates
out=$(mise run --skip-deps mutants --dry-run --update-baseline 2>&1); echo "rc=$?"; printf '%s\n' "$out" | grep -m1 mutants
out=$(mise run --skip-deps mutants --config mutarust.yml 2>&1); echo "rc=$?"; printf '%s\n' "$out" | grep -m1 mutants
```

Expected: `rc=2` both, with `mutants:` messages (`--update-baseline cannot be combined with --dry-run`; `task owns --config`). These two cases must diverge after the rewrite: first → rc 1 (mise usage), second → rc 2 (scan).

- [x] **Step 2: Replace the entire `#USAGE` block (lines 6–54) with the following**

```kdl
#USAGE flag "-f --file <file>" {
#USAGE   help "Mutate one file only (e.g. -f src/config/service.rs)"
#USAGE }
#USAGE flag "-m --mod <module>" {
#USAGE   help "Mutate one module only (target src/<module>/ recursively)"
#USAGE }
#USAGE // `complete "module"` completes the value after `-m`/`--mod`, not the flag name.
#USAGE complete "module" run="sed -nE 's/^(pub )?mod ([a-z_]+);$/\\2/p' src/lib.rs"
#USAGE flag "--min-msi <n>" help="Fail (exit 4) if total mutation score < n"
#USAGE flag "--min-covered-msi <n>" help="Fail (exit 4) if covered-file score < n (a positive value needs --coverage)"
#USAGE flag "--fail-on-escaped" help="Fail (exit 4) only on escapes absent from mutarust-baseline.json"
#USAGE flag "--update-baseline" conflicts="--run-mutant-id" {
#USAGE   help "Accept current escapes into mutarust-baseline.json (writes NO reports, exits 0)"
#USAGE }
#USAGE flag "--baseline <file>" help="Alternate accepted-escapes baseline path (default mutarust-baseline.json)"
#USAGE flag "--blacklist <file>" help="Read accepted mutation checksums from FILE"
#USAGE // no `default` on --git-diff: a default satisfies --git-diff-base's
#USAGE // `requires` (mise treats the default as a value); absence must fire
#USAGE // the requires, and the script backstop covers an explicit
#USAGE // `--no-git-diff` (mise `requires` accepts any explicit value).
#USAGE flag "--git-diff" negate="--no-git-diff" {
#USAGE   help "Mutate only lines changed vs origin/HEAD (falls back to current branch; untracked files are invisible — stage new files first)"
#USAGE }
#USAGE flag "--git-diff-base <ref>" requires="--git-diff" help="Base REF for --git-diff line selection"
#USAGE flag "--git-diff-lines" help="Scope to changed lines (the task emits this under --git-diff)"
#USAGE flag "--dry-run" {
#USAGE   help "Count mutants only; omits --test-flags and timeout/workers/coverage controls"
#USAGE   conflicts "--update-baseline" "--timeout" "--timeout-coefficient" "--workers" "--coverage" "--per-test" "--test-recursive" "--exec-timeout" "--do-not-remove-tmp-folder"
#USAGE }
#USAGE flag "--timeout <secs>" conflicts="--exec-timeout" help="Fixed test timeout in seconds (alias of --exec-timeout; suppresses the default --timeout-coefficient)"
#USAGE flag "--timeout-coefficient <n>" {
#USAGE   help "Adaptive per-test timeout multiplier (task default 5)"
#USAGE   conflicts "--timeout" "--exec-timeout"
#USAGE }
#USAGE flag "--exec-timeout <secs>" help="Fixed test timeout in seconds (alias --timeout; suppresses the default --timeout-coefficient)"
#USAGE flag "--workers <n>" help="Parallel mutation workers (mutarust default: one per logical CPU)"
#USAGE flag "--run-mutant-id <id>" help="Run one mutant by stable ID (bypasses score gates)"
#USAGE flag "--coverage" conflicts="--per-test" help="Collect coverage (required by a positive --min-covered-msi; cannot combine with --per-test)"
#USAGE flag "--per-test" help="Per-test mutation mode (cannot combine with --coverage)"
#USAGE flag "--test-recursive" help="Pass --workspace to Cargo and test every package"
#USAGE flag "--do-not-remove-tmp-folder" help="Keep the temporary mutated workspace for debugging"
#USAGE flag "--match <regexp>" help="Limit mutations to functions with matching names"
#USAGE flag "--list-mutators" exclusive=#true help="Print built-in mutator names and exit (no flags, no targets)"
#USAGE flag "--list-files" {
#USAGE   help "Print selected files (NOTE: ignores exclude_dirs)"
#USAGE   conflicts "--min-msi" "--min-covered-msi" "--fail-on-escaped" "--update-baseline" "--baseline" "--blacklist" "--git-diff" "--no-git-diff" "--git-diff-base" "--git-diff-lines" "--dry-run" "--timeout" "--timeout-coefficient" "--exec-timeout" "--workers" "--run-mutant-id" "--coverage" "--per-test" "--test-recursive" "--do-not-remove-tmp-folder" "--match" "--no-diffs" "--silent" "--no-silent" "--output-statuses" "--quiet" "--verbose" "--debug" "--html-output" "--logger-agentic-json" "--logger-github" "--logger-gitlab" "--logger-summary-json" "--ignore-msi-with-no-mutations"
#USAGE }
#USAGE flag "--print-ast" {
#USAGE   help "Print the parsed AST and exit (no flags, no targets besides -f/-m scope)"
#USAGE   conflicts "--min-msi" "--min-covered-msi" "--fail-on-escaped" "--update-baseline" "--baseline" "--blacklist" "--git-diff" "--no-git-diff" "--git-diff-base" "--git-diff-lines" "--dry-run" "--timeout" "--timeout-coefficient" "--exec-timeout" "--workers" "--run-mutant-id" "--coverage" "--per-test" "--test-recursive" "--do-not-remove-tmp-folder" "--match" "--no-diffs" "--silent" "--no-silent" "--output-statuses" "--quiet" "--verbose" "--debug" "--html-output" "--logger-agentic-json" "--logger-github" "--logger-gitlab" "--logger-summary-json" "--ignore-msi-with-no-mutations"
#USAGE }
#USAGE flag "--no-diffs" help="Hide escaped-mutant unified diffs in output"
#USAGE flag "--silent" conflicts="--no-silent" help="No per-mutant output (overrides --output-statuses)"
#USAGE flag "--no-silent" conflicts="--silent" help="Emit per-mutant output (task default; overrides a config silent_mode)"
#USAGE flag "--output-statuses <letters>" help="Show only chosen mutant states (k/e/s/n/x; overrides --quiet)"
#USAGE flag "--quiet" help="Escaped-mutant result lines only (hides killed/errored/not-covered/skipped; overridden by --output-statuses; --silent overrides --quiet)"
#USAGE flag "--verbose" help="Print file/line/worker/mutator/test-command detail"
#USAGE flag "--debug" help="Maximum mutarust diagnostics"
#USAGE flag "--html-output" help="Write mutarust-report.html (task config already enables it)"
#USAGE flag "--logger-github" help="GitHub-Actions annotations on STDOUT (in addition to the task's agentic logger)"
#USAGE flag "--logger-gitlab" help="CodeQuality-report lines on STDOUT (in addition to the task's agentic logger)"
#USAGE flag "--logger-summary-json" help="Summary JSON lines on STDOUT (in addition to the task's agentic logger)"
#USAGE flag "--logger-agentic-json" help="Agentic JSON lines (the task always enables this; passing it is a no-op)"
#USAGE flag "--ignore-msi-with-no-mutations" help="Pass score gates when the run has no mutants (auto-paired with --git-diff --min-msi)"
#USAGE arg "[targets]" var=#true {
#USAGE   help "Target paths (files/dirs/`./src...`); flags are declared above — never pass them after `--`"
#USAGE   long_help """
#USAGE   Examples:
#USAGE   - `mise run test:mutants -m index` - mutate module `index`
#USAGE   - `mise run test:mutants -m index --dry-run` - count only for one module
#USAGE   - `mise run test:mutants --min-msi 60 --fail-on-escaped` - gate on new escapes only
#USAGE   - `mise run test:mutants --git-diff` - only changed lines (tracked; stage new files first)
#USAGE   - `mise run test:mutants --workers 4` - cap parallel workers
#USAGE   - `mise run test:mutants --list-mutators` - list built-in mutators
#USAGE   - `mise run test:mutants --list-files` - print selected files (NOTE: ignores exclude_dirs)
#USAGE   - `mise run test:mutants src/cli/error.rs` - explicit target positional (skips the default)
#USAGE   - `mise run test:mutants --run-mutant-id <id>` - re-run one mutant to verify a kill
#USAGE   Every mutarust flag is declared. The task refuses only its owned flags
#USAGE   (--config, --test-flags, --exec/--no-exec, --features) and unknown or
#USAGE   misplaced flag-like tokens in target position — exit 2, `mutants:` message.
#USAGE   Inspect modes (--list-mutators/--list-files/--print-ast) run WITHOUT
#USAGE   --config or task flags (mutarust rejects them there).
#USAGE   Exit codes: THIS TASK exits 1 for usage-validation failures (conflicts,
#USAGE   exclusive, requires, bad values — `mise ERROR` message) and 2 for
#USAGE   scope/target-scan errors (bad -m/-f, refused/unknown flags — `mutants:`
#USAGE   message). mutarust directly: 0 pass · 1 tool error · 2 bash completion
#USAGE   (not a run) · 3 config/parse error · 4 quality gate (min_msi,
#USAGE   min_covered_msi, or --fail-on-escaped; --run-mutant-id bypasses gates).
#USAGE   A zero-mutant scope with --min-msi exits 4 (score is 0); only --git-diff
#USAGE   auto-pairs --ignore-msi-with-no-mutations. Retry 4s with narrower scope.
#USAGE   Migration from the previous engine: --exclude → config exclude_dirs /
#USAGE   -f / -m; --check → none (drop); --iterate → none (baseline +
#USAGE   --fail-on-escaped); -L/--all-logs → --verbose / --debug (the old -v/-V
#USAGE   were caught/unviable printers; mutarust -V is --version!); --json →
#USAGE   report.json (on via this task's config); --jobs → --workers N;
#USAGE   --no-config → never pass (task owns --config).
#USAGE   Alias: `mutants` remains a supported alias (`mise run mutants`) —
#USAGE   same task, hidden from `mise tasks`.
#USAGE   """
#USAGE }
```

Notes: multi-selector `conflicts` uses node-args form only (space-separated attributes are a silent no-op). `exclusive`/`requires` stay attributes. Inventory reconciled against `docs/refs/mutarust/docs/cli.md` during spec→plan conversion: mutarust has **no** `--package`/`--workspace` flags (they are cargo concepts passed through `--test-flags`/`--test-recursive`); they are intentionally absent here.

Spec deviations recorded at conversion (report again in Task 7 Step 4):

- **`--timeout ⊥ --exec-timeout` (new row beyond spec A3):** cli.md:237–238 says `--timeout` is an **alias** of `--exec-timeout`, and the old cascade refused the pair (declared `--timeout` + passthrough `--exec-timeout`, lines 176–181). Declaring the conflict preserves today's behavior; tightening needs no sign-off (only loosening does, per spec A3).
- **`--silent ⊥ --no-silent` (new row beyond spec A3):** cli.md:65–66 says the pair "cannot be used together"; without it both reach mutarust → rc 3. Declared bidirectionally so the pair exits 1 like every other known-invalid combination (tightening — no sign-off needed); matrix covers both orders.
- **Inspect conflict selectors are single-line:** spec A3 asked for block form ≤80 cols, but KDL node arguments cannot wrap across lines (unverified otherwise); the 34-selector lines are parsed and validated by Step 10's help render + Step 11's matrix.
- **`--no-git-diff` added to both inspect conflict lists:** spec A3's "negate spellings reject in either spelling" rule, applied to the inspect row.
- **Spec A5's `build_static_target_flags`/`build_mutarust_args`/`run_inspect_mode`/`run_count_mode` do not exist in the file** — the plan follows the actual structure (`main` → `build_inspect_args` | `build_static_flags` + `build_target_flags` + `build_gate_flags`).
- **`--git-diff-base` requires needs a script backstop (beyond spec A3's parse-time `requires`):** mise satisfies `requires` with a declared `default` *and* with any explicit value (`--no-git-diff`). Fix: no `default` on `--git-diff` (absence → rc 1) + `reject_bad_targets` guard (explicit `--no-git-diff` + base → rc 2, old cascade message). Both paths matrix-verified.
- **Spec A4's "declared flags after `--`" claim is void:** mise strips `--` before usage parsing, so `-- --workers 4` parses as `--workers 4` → rc 0 (bounded run), not rc 2. Undeclared post-`--` tokens still exit 2 via the scan; with *additional* separators a declared flag (double case) or a literal `--` (triple case) can land in the scan too → rc 2. Matrix cases cover all three edges; spec text left as-is (correction recorded here and in the spec addendum).
- **Two refusal-message rewordings (review nit):** spec A2 says the `--test-flags` refusal keeps the existing tailored text, but "drop it from passthrough" became "drop it from the command line" (passthrough no longer exists); spec A4's generic shape `flag-like token in target list: …` is implemented as `unknown or misplaced flag: … (declared flags go before --; …)`. Same intent, clearer text; spec left as-is.

- [x] **Step 3: Script — header, arrays, parse + scan (replaces lines 70–143 and `parse_passthrough_tokens`/`passthrough_has_token`/`has_fixed_timeout`)**

Replace the array declarations and the first three functions with:

```bash
declare -a mutarust_args=()
declare -a targets=()

# `--profile mutants` accepted in Task 10 P2 (see
# quality_gates_mise_adoption.md §11).
TEST_FLAGS="--features test-utils --all-targets --profile mutants"
TIMEOUT_COEFFICIENT=5
DEFAULT_TARGET="./src..."

########################################
# Parses variadic positional targets into an array.
# Globals:
#   usage_targets
#   targets
# Arguments:
#   None
# Outputs:
#   None
# Returns:
#   0 on success.
########################################
parse_targets() {
  # usage_targets arrives shell-quoted; mise documents eval-into-array as the
  # pattern for variadic args — intentional (mise_tasks 06_arguments.md).
  eval "targets=(${usage_targets:-})"
  return 0
}

########################################
# Rejects target-list tokens the task owns or does not know. mise's variadic
# positional absorbs unknown flag-like tokens (verified), so this scan is the
# strict-unknown-flag backstop. Refusals keep the tailored messages from the
# retired cascades; everything else flag-like gets the generic diagnostic.
# Also backstops the --git-diff-base requires: mise's `requires` is satisfied
# by any explicit value (e.g. `--no-git-diff`) and by a declared default, so
# the parse-time check alone does not cover every path (verified).
# Globals:
#   targets
#   usage_git_diff
#   usage_git_diff_base
# Arguments:
#   None
# Outputs:
#   Diagnostic on STDERR for a rejected token.
# Returns:
#   Exits 2 on a refused/unknown token or --git-diff-base without
#   --git-diff; 0 otherwise.
########################################
reject_bad_targets() {
  local t
  if [[ -n "${usage_git_diff_base:-}" &&
    "${usage_git_diff:-false}" != "true" ]]; then
    echo "mutants: --git-diff-base requires the --git-diff flag" >&2
    exit 2
  fi
  for t in "${targets[@]+"${targets[@]}"}"; do
    case "${t}" in
      --config | --config=*)
        echo "mutants: task owns --config; policy lives in mutarust.yml" >&2
        exit 2
        ;;
      --test-flags | --test-flags=*)
        echo "mutants: task owns --test-flags; drop it from the command line" >&2
        exit 2
        ;;
      --exec | --no-exec)
        echo "mutants: ${t} conflicts with task-owned --test-flags" >&2
        exit 2
        ;;
      --features | --features=*)
        echo "mutants: task owns --features via TEST_FLAGS; drop it from the command line" >&2
        exit 2
        ;;
      -*)
        echo "mutants: unknown or misplaced flag: ${t} (declared flags go before --; see 'mise run test:mutants --help')" >&2
        exit 2
        ;;
    esac
  done
  return 0
}
```

- [x] **Step 4: Delete `reject_conflicting_flags`, `passthrough_has_token`, `passthrough_has_positional`, `reject_declared_in_inspect`, `has_fixed_timeout` and their docblocks entirely** (approx. current lines 98–143, 145–257, 286–309, 311–357).

- [x] **Step 5: Rewrite `is_inspect_mode` (current lines 259–284)**

```bash
########################################
# True when a declared list/inspect flag was given. mutarust rejects all
# configuration and mutation options there (verified live on 0.1.10: even
# --verbose and loggers are refused), so main() bypasses every task-owned
# flag; the flag-level `conflicts` lists enforce that at parse time.
# Globals:
#   usage_list_mutators
#   usage_list_files
#   usage_print_ast
# Arguments:
#   None
# Outputs:
#   None
# Returns:
#   0 in a list/inspect mode, 1 otherwise.
########################################
is_inspect_mode() {
  [[ "${usage_list_mutators:-false}" == "true" ||
    "${usage_list_files:-false}" == "true" ||
    "${usage_print_ast:-false}" == "true" ]]
}
```

- [x] **Step 6: Rewrite `build_inspect_args` (current lines 359–394)**

```bash
########################################
# Assembles arguments for list/inspect modes. Task-owned flags are omitted
# (mutarust forbids them there; parse-time conflicts reject run flags), so
# only the mode flag, the -f/-m scope (or user targets, else DEFAULT_TARGET)
# is emitted. --list-mutators takes no target — `exclusive` already refused
# any flag or target alongside it, so the branch just emits the flag.
# Globals:
#   usage_list_mutators
#   usage_list_files
#   usage_print_ast
#   usage_file
#   usage_mod
#   targets
#   DEFAULT_TARGET
#   mutarust_args
# Arguments:
#   None
# Outputs:
#   None
# Returns:
#   0 on success.
########################################
build_inspect_args() {
  if [[ "${usage_list_mutators:-false}" == "true" ]]; then
    mutarust_args+=(--list-mutators)
    return 0
  fi
  if [[ "${usage_list_files:-false}" == "true" ]]; then
    mutarust_args+=(--list-files)
  else
    mutarust_args+=(--print-ast)
  fi
  if [[ -n "${usage_file:-}" || -n "${usage_mod:-}" ]]; then
    build_target_flags
  elif [[ "${#targets[@]}" -eq 0 ]]; then
    mutarust_args+=("${DEFAULT_TARGET}")
  else
    mutarust_args+=("${targets[@]}")
  fi
  return 0
}
```

- [x] **Step 7: Rewrite `build_static_flags` (current lines 396–426)**

```bash
########################################
# Appends policy and logging flags, then the Cargo test controls — omitted
# entirely under --dry-run (mutarust: dry-run ⊥ test/timeout controls; the
# flag-level conflicts guarantee no fixed/coefficient pair can arrive).
# Globals:
#   usage_dry_run
#   usage_timeout
#   usage_timeout_coefficient
#   usage_exec_timeout
#   TEST_FLAGS
#   TIMEOUT_COEFFICIENT
#   mutarust_args
# Arguments:
#   None
# Outputs:
#   None
# Returns:
#   0 on success.
########################################
build_static_flags() {
  mutarust_args+=(--config mutarust.yml --logger-agentic-json)
  if [[ "${usage_dry_run:-false}" != "true" ]]; then
    mutarust_args+=(--test-flags "${TEST_FLAGS}")
    if [[ -n "${usage_timeout:-}" ]]; then
      mutarust_args+=(--timeout "${usage_timeout}")
    elif [[ -n "${usage_timeout_coefficient:-}" ]]; then
      mutarust_args+=(--timeout-coefficient "${usage_timeout_coefficient}")
    elif [[ -z "${usage_exec_timeout:-}" ]]; then
      # --exec-timeout present → mutarust runs with its own fixed default;
      # emitting the task's adaptive coefficient would pair with it and be
      # rejected (cli.md:246). This preserves the old has_fixed_timeout guard.
      mutarust_args+=(--timeout-coefficient "${TIMEOUT_COEFFICIENT}")
    fi
    if [[ -n "${usage_exec_timeout:-}" ]]; then
      mutarust_args+=(--exec-timeout "${usage_exec_timeout}")
    fi
  fi
  return 0
}
```

- [x] **Step 8: `build_gate_flags` — add `--git-diff-base` inside the `--git-diff` branch (current lines 491–505)**

Also add `usage_git_diff_base` to the function docblock's Globals list (the branch now reads it).

```bash
build_gate_flags() {
  [[ -n "${usage_min_msi:-}" ]] && mutarust_args+=(--min-msi "${usage_min_msi}")
  [[ "${usage_fail_on_escaped:-false}" == "true" ]] &&
    mutarust_args+=(--fail-on-escaped)
  [[ "${usage_update_baseline:-false}" == "true" ]] &&
    mutarust_args+=(--update-baseline)
  if [[ "${usage_git_diff:-false}" == "true" ]]; then
    mutarust_args+=(--git-diff-lines)
    if [[ -n "${usage_git_diff_base:-}" ]]; then
      mutarust_args+=(--git-diff-base "${usage_git_diff_base}")
    fi
    if [[ -n "${usage_min_msi:-}" ]]; then
      mutarust_args+=(--ignore-msi-with-no-mutations)
    fi
  fi
  [[ "${usage_dry_run:-false}" == "true" ]] && mutarust_args+=(--dry-run)
  return 0
}
```

- [x] **Step 9: Add `build_declared_flags` (new function, place after `build_gate_flags`) and update `build_target_flags` + `main`**

```bash
########################################
# Emits declared run/display/input flags the user supplied. Task-owned flags
# (--config, --test-flags, --exec/--no-exec, --features) never reach here —
# reject_bad_targets refuses them. `--logger-agentic-json` is statically on
# (build_static_flags) and deliberately not re-emitted. Auto-paired flags
# (--git-diff-lines, --ignore-msi-with-no-mutations) are skipped when the
# gate builder already emits them, so no flag appears twice.
# Globals:
#   usage_*
#   mutarust_args
# Arguments:
#   None
# Outputs:
#   None
# Returns:
#   0 on success.
########################################
build_declared_flags() {
  [[ -n "${usage_workers:-}" ]] && mutarust_args+=(--workers "${usage_workers}")
  [[ -n "${usage_match:-}" ]] && mutarust_args+=(--match "${usage_match}")
  [[ -n "${usage_output_statuses:-}" ]] &&
    mutarust_args+=(--output-statuses "${usage_output_statuses}")
  [[ -n "${usage_baseline:-}" ]] && mutarust_args+=(--baseline "${usage_baseline}")
  [[ -n "${usage_blacklist:-}" ]] && mutarust_args+=(--blacklist "${usage_blacklist}")
  [[ -n "${usage_run_mutant_id:-}" ]] &&
    mutarust_args+=(--run-mutant-id "${usage_run_mutant_id}")
  [[ -n "${usage_min_covered_msi:-}" ]] &&
    mutarust_args+=(--min-covered-msi "${usage_min_covered_msi}")
  [[ "${usage_coverage:-false}" == "true" ]] && mutarust_args+=(--coverage)
  [[ "${usage_per_test:-false}" == "true" ]] && mutarust_args+=(--per-test)
  [[ "${usage_test_recursive:-false}" == "true" ]] && mutarust_args+=(--test-recursive)
  [[ "${usage_do_not_remove_tmp_folder:-false}" == "true" ]] &&
    mutarust_args+=(--do-not-remove-tmp-folder)
  [[ "${usage_verbose:-false}" == "true" ]] && mutarust_args+=(--verbose)
  [[ "${usage_debug:-false}" == "true" ]] && mutarust_args+=(--debug)
  [[ "${usage_quiet:-false}" == "true" ]] && mutarust_args+=(--quiet)
  [[ "${usage_no_diffs:-false}" == "true" ]] && mutarust_args+=(--no-diffs)
  [[ "${usage_silent:-false}" == "true" ]] && mutarust_args+=(--silent)
  [[ "${usage_no_silent:-false}" == "true" ]] && mutarust_args+=(--no-silent)
  [[ "${usage_html_output:-false}" == "true" ]] && mutarust_args+=(--html-output)
  [[ "${usage_logger_github:-false}" == "true" ]] && mutarust_args+=(--logger-github)
  [[ "${usage_logger_gitlab:-false}" == "true" ]] && mutarust_args+=(--logger-gitlab)
  [[ "${usage_logger_summary_json:-false}" == "true" ]] &&
    mutarust_args+=(--logger-summary-json)
  if [[ "${usage_git_diff_lines:-false}" == "true" &&
    "${usage_git_diff:-false}" != "true" ]]; then
    mutarust_args+=(--git-diff-lines)
  fi
  if [[ "${usage_ignore_msi_with_no_mutations:-false}" == "true" ]] &&
    ! { [[ "${usage_git_diff:-false}" == "true" && -n "${usage_min_msi:-}" ]]; }; then
    mutarust_args+=(--ignore-msi-with-no-mutations)
  fi
  return 0
}
```

In `build_target_flags` (current lines 449–472) change only the final condition and the surrounding comment mention of passthrough:

```bash
  if [[ -z "${usage_file:-}" && -z "${usage_mod:-}" &&
    "${#targets[@]}" -eq 0 ]]; then
    mutarust_args+=("${DEFAULT_TARGET}")
  fi
```

Update that function's docblock: replace "a user-supplied positional target" wording references to `passthrough` with `targets`, and drop the `passthrough` entry from Globals.

Rewrite `main` (current lines 519–534):

```bash
main() {
  parse_targets
  reject_bad_targets
  if is_inspect_mode; then
    build_inspect_args
  else
    build_static_flags
    build_target_flags
    build_gate_flags
    build_declared_flags
    if [[ "${#targets[@]}" -gt 0 ]]; then
      mutarust_args+=("${targets[@]}")
    fi
  fi

  mutarust "${mutarust_args[@]+"${mutarust_args[@]}"}"
}
```

Also fix docblocks that still list `passthrough` as a global (`build_inspect_args` done in Step 6; `build_target_flags`, `main`) and the comment in `parse_targets` header (done in Step 3). Keep `# shellcheck disable=SC2154` (line 67) — `usage_*` are injected by mise.

- [x] **Step 10: Syntax + help render**

```bash
bash -n .mise/tasks/mutants/_default && echo SYNTAX_OK
out=$(mise run test:mutants --help 2>&1); echo "rc=$?"
printf '%s\n' "$out" | grep -q "exits 1 for usage-validation" && echo HELP_OK
printf '%s\n' "$out" | grep -q -- "--workers 4" && echo EXAMPLES_OK
```

Expected: `SYNTAX_OK`, `rc=0`, `HELP_OK`, `EXAMPLES_OK`.

- [x] **Step 11: Parse-level matrix (validation cases are instant; scan cases use `--skip-deps`)**

```bash
declare -a fails=()
count=0
check() { # $1 expected rc, rest = mise run args
  local want=$1; shift
  local out rc
  count=$((count + 1))
  out=$(mise run --skip-deps "$@" 2>&1); rc=$?
  if [[ "$rc" != "$want" ]]; then
    fails+=("$* → want $want got $rc :: $(printf '%s' "$out" | grep -m1 -E 'ERROR|mutants:' | cut -c1-90)")
  fi
}
# Bounds: every non-inspect case carries `-f src/lib.rs --match __zz_no_match__`
# so a MISSING declaration degrades to a fast zero-mutant run (or a fast
# mutarust arg error) instead of a full-project mutation run. Inspect cases
# fail fast on their own (inspect output only). Never remove the bounds.
# usage validation → rc 1 (mise ERROR)
check 1 test:mutants --dry-run --update-baseline -f src/lib.rs --match __zz_no_match__
check 1 test:mutants --dry-run --timeout 5 -f src/lib.rs --match __zz_no_match__
check 1 test:mutants --dry-run --workers 2 -f src/lib.rs --match __zz_no_match__
check 1 test:mutants --dry-run --timeout-coefficient 3 -f src/lib.rs --match __zz_no_match__
check 1 test:mutants --dry-run --coverage -f src/lib.rs --match __zz_no_match__
check 1 test:mutants --dry-run --per-test -f src/lib.rs --match __zz_no_match__
check 1 test:mutants --dry-run --test-recursive -f src/lib.rs --match __zz_no_match__
check 1 test:mutants --dry-run --exec-timeout 30 -f src/lib.rs --match __zz_no_match__
check 1 test:mutants --dry-run --do-not-remove-tmp-folder -f src/lib.rs --match __zz_no_match__
check 1 test:mutants --timeout-coefficient 3 --timeout 5 -f src/lib.rs --match __zz_no_match__
check 1 test:mutants --timeout-coefficient 3 --exec-timeout 30 -f src/lib.rs --match __zz_no_match__
check 1 test:mutants --timeout 5 --exec-timeout 30 -f src/lib.rs --match __zz_no_match__
check 1 test:mutants --update-baseline --run-mutant-id deadbeef -f src/lib.rs --match __zz_no_match__
check 1 test:mutants --coverage --per-test -f src/lib.rs --match __zz_no_match__
check 1 test:mutants --silent --no-silent -f src/lib.rs --match __zz_no_match__
check 1 test:mutants --no-silent --silent -f src/lib.rs --match __zz_no_match__
# requires fires only because --git-diff has no default (a default would
# satisfy `requires` — see ground rules); message = `mise ERROR`
check 1 test:mutants --git-diff-base origin/main -f src/lib.rs --match __zz_no_match__
check 1 test:mutants --dry-run --workers=4 -f src/lib.rs --match __zz_no_match__
check 1 test:mutants --list-mutators --verbose
check 1 test:mutants --list-mutators ./src...
check 1 test:mutants --list-mutators -m index
check 1 test:mutants --list-files --min-msi 50
check 1 test:mutants --list-files --verbose
check 1 test:mutants --print-ast --dry-run
# target scan → rc 2 (bounds are inert here: the scan exits before mutarust)
check 2 test:mutants --config mutarust.yml -f src/lib.rs --match __zz_no_match__
check 2 test:mutants -- --config mutarust.yml -f src/lib.rs --match __zz_no_match__
check 2 test:mutants --test-flags "--features x" -f src/lib.rs --match __zz_no_match__
check 2 test:mutants --exec -f src/lib.rs --match __zz_no_match__
check 2 test:mutants --features x -f src/lib.rs --match __zz_no_match__
check 2 test:mutants --bogus -f src/lib.rs --match __zz_no_match__
check 2 test:mutants -x -f src/lib.rs --match __zz_no_match__
# a second `--` still lands following declared flags in scan range
check 2 test:mutants -- -- --dry-run -f src/lib.rs --match __zz_no_match__
# a third `--` survives into the scan itself (generic arm names the token)
check 2 test:mutants -- -- -- --dry-run -f src/lib.rs --match __zz_no_match__
# happy parse path → rc 0 (inspect is the only fast rc-0 path; dry-run
# happy paths are Task 2's Steps 1/1b). `-- --workers 4` parses as
# `--workers 4`: mise strips `--` before usage parsing, so a DECLARED flag
# after `--` is just the flag (bounded run → 0); undeclared post-`--`
# tokens still exit 2 via the scan (covered above).
check 0 test:mutants --list-files src/lib.rs
check 0 test:mutants -- --workers 4 -f src/lib.rs --match __zz_no_match__
# equals-form value parsing on the happy path (spec §Verification 3)
check 0 test:mutants --workers=4 -f src/lib.rs --match __zz_no_match__
# sibling inspect flags pair legally (excluded from both conflict lists);
# list-files wins, mutarust accepts the pair
check 0 test:mutants --list-files --print-ast -f src/lib.rs
# alias regression: the legacy `mutants` name resolves to this same task
# (kept via `#MISE aliases`, hidden from `mise tasks`)
check 0 mutants --list-mutators
# summary
if [[ ${#fails[@]} -eq 0 ]]; then echo "MATRIX: ALL PASS ($count cases)"; else
  printf 'MATRIX FAILURES:\n'; printf '  %s\n' "${fails[@]}"; fi
# any failed-pair declaration could have let a run touch the baseline:
if [[ -n "$(git status --porcelain mutarust-baseline.json)" ]]; then
  git checkout -- mutarust-baseline.json; echo BASELINE_RESTORED
fi
```

Expected: `MATRIX: ALL PASS (38 cases)`, then either no output or `BASELINE_RESTORED` from the baseline guard. If any `rc 1`/`rc 2` case returns `0`: run it again with `mise run --force --skip-deps …`; if it still returns 0, stop and report (mise usage-validation bug/setting drift — do not paper over with script code). If `mutarust-baseline.json` was rewritten, the guard restores it — never commit a matrix-touched baseline.

- [x] **Step 12: `hk` on the edited file + commit**

```bash
hk fix --safe --no-stage --unstaged 2>/dev/null || true
git diff --stat
git add .mise/tasks/mutants/_default
git commit -m "refactor(mutants): strict declared usage surface"
```

Expected: commit lands; hooks green; no other files staged.

---

### Task 2: `mutants` happy paths (cargo-scoped, bounded)

**Files:** none (verification only; fix `_default` fallout if a step fails)

- [x] **Step 1: Dry-run count (bounded scope)**

```bash
nice -n 10 mise run --skip-deps mutants -m strsim --dry-run
```

Expected: rc 0, mutant counts printed (≈6 mutants), `report.json` written. Timeout 900000 ms.

- [x] **Step 1b: dry-run happy paths — `-f`, `--`-targets, bare (spec verification 3)**

```bash
out=$(nice -n 10 mise run --skip-deps mutants --dry-run -f src/lib.rs 2>&1); echo "rc=$?"
out=$(nice -n 10 mise run --skip-deps mutants --dry-run -- src/cli/error.rs 2>&1); echo "rc=$?"
out=$(nice -n 10 mise run --skip-deps mutants --dry-run 2>&1); echo "rc=$?"
```

Expected: `rc=0` ×3 — single-file scope, explicit `--`-target positional, then the full-project count (the last is the slowest matrix-adjacent step; count-only, no tests). Timeout 900000 ms.

- [x] **Step 2: Scored gate against the committed baseline**

```bash
out=$(nice -n 10 mise run --skip-deps mutants -m strsim --fail-on-escaped 2>&1); echo "rc=$?"; printf '%s\n' "$out" | tail -5
```

Expected: rc 0 (baseline absorbs current escapes; `-m strsim` is Part I Task 11's proven bounded scope). Timeout 900000 ms.

- [x] **Step 3: git-diff empty-Rust-diff run**

```bash
out=$(nice -n 10 mise run --skip-deps mutants --git-diff 2>&1); echo "rc=$?"; printf '%s\n' "$out" | grep -E 'total|score|mutant' | tail -3
```

Expected: rc 0 — the branch changes touch no `src/` lines, so mutarust selects zero mutants (fast; no gate present).

- [x] **Step 4: Real inspect run**

```bash
out=$(mise run --skip-deps mutants --list-mutators 2>&1); echo "rc=$?"; printf '%s\n' "$out" | grep -c 'statement/'
```

Expected: rc 0, count = 4 (`statement/*` — mutarust 0.1.10 has only 4 statement mutators; 33 names total. Verified: raw `mutarust --list-mutators` also returns 4, so this is taxonomy, not task fallout. Original `> 10` expectation was a plan-text error, corrected after the Task 2 run).

- [x] **Step 5: Commit any fallout fixes (or skip when clean)**

```bash
git status --porcelain
```

Expected: empty (verification-only). If any earlier step forced `_default` fixes, commit them as `fix(mutants): <what>`.

---

### Task 3: `clean:reports` registry

**Files:**
- Modify: `.mise/tasks/clean/reports` (lines 4–9 long_help; lines 25–33 registry)

- [x] **Step 1: Add the five mutarust files to `get_known_report_registry`**

```bash
get_known_report_registry() {
  known_reports=(
    lcov.info
    tarpaulin-report.html
    mutants.out
    mutants.out.old
    mutants-report.md
    report.json
    mutarust-report.html
    mutarust-agentic.json
    mutarust-summary.json
    mutarust-gitlab.json
  )
}
```

Deliberately absent: `mutarust-baseline.json` (committed policy artifact).

- [x] **Step 2: Update the `--dry-run` long_help (lines 6–8)**

```bash
#USAGE   Preview mode: prints what report files (`lcov.info`, `tarpaulin-report.html`, `mutants.out/`, `mutants-report.md`, `report.json`, `mutarust-report.html`, `mutarust-agentic.json`, `mutarust-summary.json`, `mutarust-gitlab.json`) exist and would be deleted without removing them.
```

- [x] **Step 3: Verify listing (Task 2's runs regenerated the mutarust reports)**

```bash
bash -n .mise/tasks/clean/reports && echo SYNTAX_OK
out=$(mise run clean:reports --dry-run 2>&1); echo "rc=$?"; printf '%s\n' "$out" | grep -cE 'mutarust|report\.json'
```

Expected: `SYNTAX_OK`, `rc=0`, count ≥ 3 — at least `report.json` + `mutarust-report.html` + `mutarust-agentic.json` regenerated by Task 2 (`mutarust-summary.json`/`mutarust-gitlab.json` appear only when their logger flags are passed, so they may be absent). Never run `clean:reports` without `--dry-run` in this session.

- [x] **Step 4: Commit**

```bash
git add .mise/tasks/clean/reports
git commit -m "feat(clean): register mutarust report files"
```

---

### Task 4: `mess` task help + workflow text

**Files:**
- Modify: `mise.toml` lines 225–238 (`[tasks.mess]`)

- [x] **Step 1: Confirm baseline advisory contract (red for the help, green for behavior)**

```bash
out=$(mise run mess 2>&1); echo "rc=$?"          # expect 2 (findings)
out=$(mise run mess -- --ignore-violations-on-exit >/dev/null 2>&1); echo "rc=$?"  # expect 0
out=$(mise run mess --help 2>&1); printf '%s\n' "$out" | head -3   # current one-line help
```

Expected: `2`, `0`, then the current minimal usage. Preserve both rc values after the rewrite.

- [x] **Step 2: Replace the `usage = '…'` line (mise.toml line 228) with a multiline TOML basic-string spec**

```toml
usage = """
arg "[args]" var=#true {
  help "Extra arguments forwarded to messrust (e.g. --strict, --only Rule, --reportfile f.json)"
  long_help \"""
  Advisory static analysis with messrust — manual, never wired into verify/hk/CI.

  Workflow:
  1. Run after big refactors or before opening a PR: `mise run mess`
  2. Exit 0 = clean; exit 2 = findings (the normal advisory signal); exit 1 = tool error
  3. To triage machine-readably: `mise run mess -- --reportfile /tmp/mess.json --ignore-violations-on-exit` (exit 0, report kept)
  4. Tune policy in `messrust.xml` (rule refs, thresholds like reportLevel); history in docs/refs/quality_gates_messrust.md (§7–§9)
  5. Act: fix, or record why a finding stays; re-run

  Flag cheatsheet (forwarded verbatim after `--`, full reference docs/refs/messrust/docs/usage.md):
  --only Rule, --enable/--disable ruleset, --exclude path, --strict,
  --reportfile FILE, --ignore-tests (task always passes it),
  --ignore-violations-on-exit / --ignore-errors-on-exit (scripting escapes),
  --minimumpriority/--maximumpriority N, --suffixes ext, --color, --verbose
  \"""
}
"""
```

If mise rejects the multiline spec (`invalid usage spec` warning or `--help` failing), fall back to keeping the original one-line `usage` and moving the workflow text into Task 5's doc section only — then note the fallback in the commit message.

- [x] **Step 3: Verify help renders, behavior unchanged, forwarding intact**

```bash
out=$(mise run mess --help 2>&1); echo "rc=$?"; printf '%s\n' "$out" | grep -q "Workflow" && echo HELP_OK
out=$(mise run mess 2>&1); echo "rc=$?"                          # expect 2
out=$(mise run mess -- --ignore-violations-on-exit >/dev/null 2>&1); echo "rc=$?"   # expect 0
```

Expected: `rc=0`, `HELP_OK`, `2`, `0`.

- [x] **Step 4: Commit**

```bash
git add mise.toml
git commit -m "feat(mess): workflow help for the advisory task"
```

---

### Task 5: `quality_gates_messrust.md` — Workflow section

**Files:**
- Modify: `docs/refs/quality_gates_messrust.md` (append after §9, line 1182; file currently ends inside §9's location list)

- [x] **Step 1: Append the new section**

````markdown
---

## 10. Workflow

Advisory contract (unchanged by this section): `mise run mess` is manual and
never wired into `verify`, `hk`, or CI. Exit codes: `0` clean · `2` findings
(normal signal) · `1` tool error. Upstream CI treats exit 2 as failure; this
repo treats it as a to-triage list.

1. **Run** — `mise run mess` after refactors or before a PR (task always adds
   `--ignore-tests`; output is `text` on stdout).
2. **Read** — findings print as `file:line<tab>Rule<tab>message`. The §9 table
   is the triage history; counts > 25 per rule warrant an exclusion decision,
   `CyclomaticComplexity`/`NPathComplexity` counts warrant threshold retunes.
3. **Act** — fix the finding, or keep it with a recorded reason (threshold
   rationale lives in `messrust.xml`; decision history in §7/§9).
4. **Re-run** — until exit 0 or the remaining set is accepted signal.

Machine-readable triage:

```bash
mise run mess -- --reportfile /tmp/mess.json --ignore-violations-on-exit  # exit 0
```

Flag reference: `docs/refs/messrust/docs/usage.md` (all 16 flags; the task
forwards everything verbatim after `--`). Policy file: `messrust.xml`.
````

- [x] **Step 2: Sanity-check heading sequence and trailing whitespace**

```bash
grep -n '^## ' docs/refs/quality_gates_messrust.md | tail -3
```

Expected: `## 9. First advisory run (2026-09-27)` then `## 10. Workflow` as the last heading.

- [x] **Step 3: Commit**

```bash
git add docs/refs/quality_gates_messrust.md
git commit -m "docs(messrust): add day-to-day workflow section"
```

---

### Task 6: `mutation_testing.md` audit

**Files:**
- Modify: `docs/refs/mutation_testing.md` (six edits)

- [x] **Step 1: §2 config paragraph (lines 41–43)**

Before:
```markdown
- `mutarust.yml` — policy (exclusions, mutators, outputs, score gate).
  mutarust has **no config auto-discovery**: mutation runs always pass
  `--config mutarust.yml`; list/inspect passthrough runs (`--list-mutators`,
  `--list-files`, `--print-ast`) run without it.
```
After:
```markdown
- `mutarust.yml` — policy (exclusions, mutators, outputs, score gate).
  mutarust has **no config auto-discovery**: mutation runs always pass
  `--config mutarust.yml`; inspect runs (`--list-mutators`, `--list-files`,
  `--print-ast` — declared task flags, no `--` needed) run without it.
```

- [x] **Step 2: §3 flag table (lines 59 and 70)**

Line 59 before: `Declared flags (see \`mise run mutants --help\` for the full contract):`
Line 59 after: `Selected declared flags (the task declares mutarust's full surface minus its owned flags — see \`mise run mutants --help\` for the complete contract):`

Line 70 before: `| \`[args]\` | passthrough after \`--\` (e.g. \`--list-mutators\`, \`--workers 4\`) |`
Line 70 after: `| \`[targets]\` | positional target paths (flags are declared above; never passed after \`--\`) |`

- [x] **Step 3: rejection + exit-code prose (lines 75–79)**

Before:
```markdown
Conflicting flag combinations (e.g. `--update-baseline --dry-run`) are
rejected at the task level with exit 2 and a `mutants:` message.

**Exit codes:** task usage errors exit `2`. mutarust itself: `0` pass ·
```
After:
```markdown
Conflicting flag combinations (e.g. `--update-baseline --dry-run`) are
rejected by usage validation with exit 1 and a `mise ERROR` message; refused
(`--config`, `--test-flags`, `--exec`/`--no-exec`, `--features`) or unknown
flag-like tokens in target position are rejected by the task's target scan
with exit 2 and a `mutants:` message.

**Exit codes:** task argument-validation errors exit `1`, task
scope/target-scan errors exit `2`. mutarust itself: `0` pass ·
```

- [x] **Step 4: example commands (lines 92 and 118)**

Line 92 before: `mise run mutants -- --workers 4      # cap parallel workers`
Line 92 after: `mise run mutants --workers 4        # cap parallel workers`

Line 118 before: `mise run mutants -- --run-mutant-id <id>  # re-run just that mutant → killed?`
Line 118 after: `mise run mutants --run-mutant-id <id>  # re-run just that mutant → killed?`

- [x] **Step 5: Confirm no other stale forms remain in live docs**

```bash
grep -rn 'run mutants -- ' docs/refs/ || echo "no stale passthrough forms"
grep -rn 'exit 2' docs/refs/mutation_testing.md
```

Expected: first grep → `no stale passthrough forms` (scope is `docs/refs/` only — historical plan files under `docs/superpowers/` legitimately quote the old forms and are out of scope); second shows only the line updated in Step 3 (plus any mutarust-internal `exit 2` mentions, which stay). `docs/refs/quality_gates_mise_adoption.md` lines 198/418/577/627/736 are a historical research log (they quote the pre-rename `mutarust` entrypoint and completed recommendations) — **leave them untouched**; this deviates from the spec's tentative "736 might need update" note because context reading showed it records a completed recommendation, not current behavior.

- [x] **Step 6: Commit**

```bash
git add docs/refs/mutation_testing.md
git commit -m "docs: align mutation testing guide with strict mutants args"
```

---

### Task 7: Final gates

- [x] **Step 1: Full verify gate**

```bash
mise run verify
```

Expected: rc 0 (fmt, check, lint, test — ~2904 tests + 58 doctests; the pre-existing `dead_code` warning in `src/index/inlinks.rs` is not ours). Timeout 900000 ms.

- [x] **Step 2: hk pass over the tree**

```bash
hk check --safe --skip-step gitleaks --format json 2>/dev/null | jq -r '.status'
```

Expected: `passed` (key is `status`, not `overall`). `--skip-step gitleaks` is required: `--safe` refuses that step's unknown effect (pre-existing; rationale in the merged **Ground rules (standing, non-negotiable)** → **Commits** bullet). Without the skip, status is `failed` with `--safe refused to run: gitleaks.check: effect is unknown`. If not `passed`: `hk fix --safe --no-stage --unstaged`, review diff, re-run.

- [x] **Step 3: Tree state + summary**

```bash
git status --porcelain; git log --oneline -7
```

Expected: clean tree; the five task commits (Tasks 1, 3, 4, 5, 6 — plus any Task 2 fallout fix) visible on top of `01933d0d`.

- [x] **Step 4: Report back** — outcomes of matrix, happy paths, verify/hk, plus any spec deviations (e.g. mess multiline-spec fallback) for the user's review.

---

## Spec coverage map

| Spec section | Plan location |
| --- | --- |
| A1 declared surface (+ cli.md reconciliation note on `--package`/`--workspace`) | Task 1 Step 2 |
| A2 refusal set + tailored messages | Task 1 Step 3 (`reject_bad_targets`), long_help |
| A3 conflict matrix (dry-run ×9, timeout pairs incl. the `--timeout`⊥`--exec-timeout` extension, update-baseline pair, coverage⊥per-test, silent⊥no-silent both orders, git-diff-base requires, exclusive, inspect ×34 selectors incl. `--no-git-diff`) | Task 1 Step 2, notes, matrix Step 11 |
| A4 target scan (`-`-prefixed → exit 2, tailored vs generic, `--` dropped) + git-diff-base requires backstop; spec's declared-flag-after-`--` claim void (see deviation notes) | Task 1 Step 3, matrix Step 11 |
| A5 deletions (`reject_conflicting_flags`, `reject_declared_in_inspect`, `passthrough_has_token`, `passthrough_has_positional`, `has_fixed_timeout`, passthrough forwarding; spec A5's other function names don't exist in the file — see Step 2 notes) | Task 1 Steps 3–9 |
| A6 exit-code contract 1/2 + help rewrite | Task 1 Step 2 long_help, Step 10, matrix Step 11 |
| B registry + long_help | Task 3 |
| C mess help/long_help, passthrough kept, no automation | Task 4 (Workflow §: Task 5) |
| Docs audit table | Task 6 (adoption doc = leave, with rationale) |
| Verification matrix | Task 1 Step 11 + Task 2 + Task 7 |
| Risks: over-restrictive inspect conflicts (loosen only with cli.md citation + user sign-off), version drift (2026.9.15 pinned), enforcement determinism (`--force` escalation in matrix step) | Task 1 Step 11 escalation note; Ground rules |
| Post-implementation corrections (2026-09-29): task renamed `test:mutants` with `mutants` kept as a hidden file-task alias; every quoted invocation above predates the rename; plan matrix now runs 38 cases (37 renamed + one alias regression case) | Part III Task 3 (plan matrix + runnable lines) + Verification 4; spec addendum row appended in Part III Task 4 Step 2 |

---

# Part III — 2026-09-29: rename mutants → test:mutants

**Goal:** Move `.mise/tasks/mutants/_default` to `.mise/tasks/test/mutants` so the task is `test:mutants`, keep `mutants` as a hidden file-task alias, and repoint every live reference (task self-references, two refs docs, the living matrix plan, spec addenda).

**Architecture:** One `git mv`, one header line (`#MISE aliases=["mutants"]` — probe-verified, see Context), one bulk `sed` class (invocations `mise run mutants` → `mise run test:mutants`) plus three surgical doc edits, and mechanical updates to Part II (the 09-28 plan)'s runnable lines (37 matrix checks + step commands) while its `Line N before/after:` records and path prose stay historical. A 38th matrix case proves the alias keeps resolving.

**Tech Stack:** mise file tasks (`#MISE`/`#USAGE` headers), BSD sed/rg, the extracted bash matrix from Part II (the 09-28 plan), hk gates.

---

## Context

Design dated 2026-09-29; status approved (design brainstorm + user approval of delta, 2026-09-29).

The `mutants` task lives at `.mise/tasks/mutants/_default`. The directory form existed to host the `mutants:report` sibling; that report task was deleted at the mutarust cutover (`ffb725d6`), so the directory now earns nothing. A `test:` family already exists (`test`, `test:doc`, `test:unit`), and mutation testing belongs to it. Rename the invocation to `test:mutants`; keep `mutants` working as an alias.

Verified during brainstorm:

- `[task_templates.mutants]` (mise.toml:315) is what `#MISE extends="mutants"` targets — a template name, independent of the task name. The header's `extends` needs no change, and the rename removes the current task/template name ambiguity.
- mise 2026.9.15 supports `#MISE aliases=["…"]` on file tasks: alias invocations forward all arguments (including flag-like ones, e.g. `--dry-run`) transparently, and the alias stays hidden from `mise tasks` (no second help surface). A toml wrapper task was tried as the alternative and errored; the file-task alias is strictly better here.
- Ancestor task dirs do not leak: the main checkout shipped `.mise/tasks/mutants/report`, yet from the (now-retired) nested worktree `mise tasks` listed no `mutants:report`. Verification can assert `mutants:report`'s absence directly.

---

## Decisions

1. **Move**: `git mv .mise/tasks/mutants/_default .mise/tasks/test/mutants`; the `mutants/` directory disappears, `test/` gains a file, and the task is named `test:mutants` from its path. `depends=["test"]` and the rest of the header stay byte-identical except the two additions in 2 and 3.
2. **Alias**: add `#MISE aliases=["mutants"]` to the header, and one sentence in the `long_help` migration section documenting the alias (`mutants` remains a supported alias). Without the sentence the alias is undiscoverable because `mise tasks` hides it.
3. **In-file self-references (12 sites)**: the 9 help examples (`mise run mutants …` → `mise run test:mutants …`), the refusal message (`see 'mise run mutants --help'` → `test:mutants`), and 2 comments. After the edit the file contains exactly one `run mutants` occurrence — the intentional alias sentence.
4. **Live docs**:
   - `docs/refs/mutation_testing.md` — 15 invocations → `test:mutants`, plus the current-behavior mention at :30 (`tool-agnostic mutants task` and its path). The deleted-`report` mention at :130 stays historical.
   - `docs/refs/quality_gates_mutarust.md` — :570 invocation, :239 relative link (text + href), :308 current-state path. Lines citing the deleted `mutants/report` task (including its historical `mutants:report` mentions) stay untouched.
   - Stale-path repairs flagged by review — current-state prose only, not snapshots: `quality_gates_mutarust.md:514` (sibling reference), `quality_gates_messrust.md:279` (link text + href) and `:285` (pattern reference), all `mutants/_default` → `test/mutants`. The "sources consulted" snapshots keep the pre-rename paths as recorded history (excluded line numbers per Task 4 Step 1).
   - Plan `2026-09-28-quality-gates-task-ux.md` (now Part II of this document) — **runnable lines only**: the 37 matrix `check N mutants …` lines become `check N test:mutants …`, plus one new alias case (Task 3 Step 3), plus `mise run mutants …` in step commands and expected outputs. The `Line N before/after:` edit records and `.mise/tasks/mutants/_default` path prose stay as recorded history.
   - Plan `2026-09-27-quality-gates-messrust-mutarust.md` (now Part I of this document) — untouched (completed historical record).
   - Spec `2026-09-28-quality-gates-task-ux-design.md` — one new addendum row recording the rename and the alias; the approved body keeps its quoted pre-rename invocations; the row notes the plan matrix now runs 38 cases.

---

## Ground rules (era 3)

All era-3 rules are carried verbatim in the merged **Ground rules (standing, non-negotiable)** section at the top of this document.

---

## Execution

### Task 1: Move the file + self-references + alias

> **Completed 2026-09-29** — commit `c38d75b6`. Steps 1–6 re-verified in place after the worktree → main-checkout move (counts 12/1, help `10`/`1`, refusal rc=2, mode 755). Step 8 rewritten below for the new checkout.

**Files:**
- Move: `.mise/tasks/mutants/_default` → `.mise/tasks/test/mutants`

- [x] **Step 1: Move the file**

```bash
cd /Users/jack/Documents/41_personal/traces-pkm/.worktrees/quality-gates
git mv .mise/tasks/mutants/_default .mise/tasks/test/mutants
rmdir .mise/tasks/mutants
test -f .mise/tasks/test/mutants && echo MOVED
test ! -d .mise/tasks/mutants && echo DIR_GONE
```

Expected: `MOVED` then `DIR_GONE`.

- [x] **Step 2: Add the alias header line**

The header currently is:

```bash
#!/bin/bash
#MISE description="Run mutation testing (engine: `mutarust`)"
#MISE sources=["@group:rust"]
#MISE extends="mutants"
#MISE depends=["test"]
```

Insert `#MISE aliases=["mutants"]` as the new line 6 (directly after `depends`), then:

```bash
rg -o '^#MISE aliases=\["mutants"\]$' .mise/tasks/test/mutants | wc -l   # 1
```

Expected: `1`. (`extends="mutants"` is untouched — see Context.)

- [x] **Step 3: Replace all 12 invocations in the file**

```bash
sed -i '' 's/mise run mutants/mise run test:mutants/g' .mise/tasks/test/mutants
rg -o 'mise run test:mutants' .mise/tasks/test/mutants | wc -l   # 12
rg -o 'mise run mutants' .mise/tasks/test/mutants | wc -l        # 0
```

Expected: `12` then `0` (the 12 sites are broken down in Decision 3).

- [x] **Step 4: Add the alias sentence to `long_help`**

In the `#USAGE   """` long_help block, the current tail is:

```bash
#USAGE   report.json (on via this task's config); --jobs → --workers N;
#USAGE   --no-config → never pass (task owns --config).
#USAGE   """
```

Change it to:

```bash
#USAGE   report.json (on via this task's config); --jobs → --workers N;
#USAGE   --no-config → never pass (task owns --config).
#USAGE   Alias: `mutants` remains a supported alias (`mise run mutants`) —
#USAGE   same task, hidden from `mise tasks`.
#USAGE   """
```

Then:

```bash
rg -o 'mise run test:mutants' .mise/tasks/test/mutants | wc -l   # 12 (unchanged)
rg -o 'mise run mutants' .mise/tasks/test/mutants | wc -l        # 1 (the alias sentence)
```

Expected: `12` then `1`. These two numbers are the file's permanent invariants (Verification 5).

- [x] **Step 5: Syntax + help render**

```bash
bash -n .mise/tasks/test/mutants && echo SYNTAX_OK
out=$(mise run --skip-deps test:mutants --help 2>&1); echo "help_rc=$?"
printf '%s' "$out" | grep -o 'test:mutants' | wc -l      # 9 (the examples)
printf '%s' "$out" | grep -o 'mise run mutants' | wc -l  # 1 (the alias sentence)
```

Expected: `SYNTAX_OK`, `help_rc=0`, `10`, `1`. The `10` = the auto-generated
usage line `Usage: test:mutants …` (1) + the 9 example mentions (an earlier
draft said 9 — it forgot the usage line; `grep -o 'test:mutants' help` was
verified to be exactly those 10 lines).

- [x] **Step 6: Refusal message probe**

```bash
out=$(mise run --skip-deps test:mutants --bogus 2>&1); rc=$?
echo "rc=$rc"; printf '%s\n' "$out" | grep -m1 'unknown or misplaced'
```

Expected: `rc=2` and the line
`mutants: unknown or misplaced flag: --bogus (declared flags go before --; see 'mise run test:mutants --help')`.

- [x] **Step 7: hk + commit**

```bash
hk fix --safe --no-stage --unstaged 2>/dev/null || true
git add -A .mise/tasks
git status --porcelain
git commit -m "refactor(tasks): rename mutants task to test:mutants"
```

Expected: commit lands (rename + edits), hooks green, no other files staged, tree otherwise clean.

- [x] **Step 8: Alias probes — in place (guarded socket move)**

**Why in place now:** the worktree retired mid-plan (Incidents I1); this
checkout IS the repo root on `quality-gates`, so the old `.mise/tasks/mutants/`
path is gone and nothing can shadow the alias. The bounded alias run invokes
mutarust, which trips over `.codegraph/daemon.sock` — the socket is therefore
moved aside for the duration of the probe under an `EXIT` trap that restores it
no matter what (Ground rules: socket policy).

```bash
cd /Users/jack/Documents/41_personal/traces-pkm
SOCK="$PWD/.codegraph/daemon.sock"; BAK=/var/folders/9w/3qn47_qj3m9b27gkxwr5_k9m0000gn/T/opencode/sock-probe.bak
[ -e "$SOCK" ] && mv "$SOCK" "$BAK"
trap 'mv -f "$BAK" "$SOCK" 2>/dev/null; echo SOCKET_RESTORED' EXIT
out=$(mise run --skip-deps mutants -f src/lib.rs --match __zz_no_match__ 2>&1); rc=$?
echo "alias_rc=$rc"                                   # 0 — resolves via #MISE aliases → test:mutants
printf '%s\n' "$out" | grep -c 'test:mutants' || true # >0 — ran the NEW task
trap - EXIT; [ -e "$BAK" ] && mv -f "$BAK" "$SOCK"
mise tasks | grep -c '^mutants'                       # 0 — alias hidden from listing
mise tasks | grep -c 'mutants:report'                 # 0 — no report task
test -e "$SOCK" && echo SOCK_IN_PLACE
```

Expected: `alias_rc=0`, a nonzero `test:mutants` hit (the new task's
`[test:mutants] $ …/.mise/tasks/test/mutants` line), `0`, `0`,
`SOCK_IN_PLACE`. (Listing probes need no guard — they don't run mutarust.)

---

### Task 2: Live docs

> **Completed 2026-09-29** — commit `89f036e8` (counts 15→0/1→0, paths 1/3 verified).

**Files:**
- Modify: `docs/refs/mutation_testing.md` (:30 + 15 invocations)
- Modify: `docs/refs/quality_gates_mutarust.md` (:570, :239, :308)

- [x] **Step 1: mutation_testing.md**

```bash
rg -o 'mise run mutants' docs/refs/mutation_testing.md | wc -l   # pre-check: 15
```

Expected: `15` (verified pre-plan: 15 occurrences across 15 lines); then:

```bash
sed -i '' 's/mise run mutants/mise run test:mutants/g' docs/refs/mutation_testing.md
rg -o 'mise run mutants' docs/refs/mutation_testing.md | wc -l   # 0
```

Now the :30 line, which currently reads:

```markdown
   **tool-agnostic `mutants` task** (`.mise/tasks/mutants/_default`) — the
   entrypoint name predates the engine and outlives it.
```

Change it to:

```markdown
   **tool-agnostic `test:mutants` task** (`.mise/tasks/test/mutants`) — the
   entrypoint name predates the engine and outlives it.
```

Then:

```bash
rg -o '\.mise/tasks/test/mutants' docs/refs/mutation_testing.md | wc -l   # 1
```

Expected: `0`, then `1`. (Line :130's deleted-`report` mention stays historical.)

- [x] **Step 2: quality_gates_mutarust.md**

```bash
rg -o 'mise run mutants' docs/refs/quality_gates_mutarust.md | wc -l   # 1
sed -i '' 's/mise run mutants/mise run test:mutants/g' docs/refs/quality_gates_mutarust.md
```

Link at :239 currently:

```markdown
([.mise/tasks/mutants/_default](../../.mise/tasks/mutants/_default)),
```

Change to:

```markdown
([.mise/tasks/test/mutants](../../.mise/tasks/test/mutants)),
```

Path at :308 (inside the "Where mutation currently lives" paragraph) currently starts:

```markdown
  `.mise/tasks/mutants/_default` (extends `[task_templates.mutants]`,
```

Change the path only (leave the rest of the paragraph alone):

```markdown
  `.mise/tasks/test/mutants` (extends `[task_templates.mutants]`,
```

Then:

```bash
rg -o 'mise run mutants' docs/refs/quality_gates_mutarust.md | wc -l   # 0
rg -o '\.mise/tasks/test/mutants' docs/refs/quality_gates_mutarust.md | wc -l   # 3 (link text + href + :308)
```

Expected: `0`, then `3`. Lines citing the deleted `mutants/report` task and `mutants:report` history stay untouched.

- [x] **Step 3: hk + commit**

```bash
hk fix --safe --no-stage --unstaged 2>/dev/null || true
git add docs/refs/mutation_testing.md docs/refs/quality_gates_mutarust.md
git commit -m "docs: point live guides at test:mutants"
```

Expected: commit lands; hooks green.

---

### Task 3: The living 09-28 plan (now Part II) (matrix + runnable lines) + matrix run

> *(Post-consolidation note, 2026-09-29): `$p` and the other paths in this task's commands target the former `2026-09-28-quality-gates-task-ux.md`, now Part II of this document. The rename seds were scoped to that file — do **not** re-run them against the consolidated file (they would also match Part I's historical `mise run mutants` invocations). Matrix extraction (`declare -a fails` anchor) still works with `p=docs/superpowers/plans/2026-09-27-quality-gates.md`; the Step 5 `#USAGE flag "-f --file <file>"` anchor must instead be changed to the unique `` ^```kdl `` fence, because Part I's script snapshot contains an earlier copy of that line.)*

**Files:**
- Modify: `docs/superpowers/plans/2026-09-28-quality-gates-task-ux.md` (now Part II of this document)

- [x] **Step 1: Replace runnable invocations; revert the `Line N` records**

```bash
p=docs/superpowers/plans/2026-09-28-quality-gates-task-ux.md
sed -i '' 's/mise run mutants/mise run test:mutants/g' "$p"
sed -i '' -e '/^Line [0-9]* before:/s/mise run test:mutants/mise run mutants/' \
          -e '/^Line [0-9]* after:/s/mise run test:mutants/mise run mutants/' "$p"
rg -o 'mise run test:mutants' "$p" | wc -l   # 12
rg -o 'mise run mutants' "$p" | wc -l        # 6 (the historical Line N records only)
```

Expected: `12` then `6` — the 12 = ground-rules gotcha (:20), the 9 Step 2
block examples, the Step 3 block refusal message, and the Step 10 help probe;
the 6 docs hits = Part II Task 6's `Line 59/92/118 before/after:` edit records —
those quotes are history and must keep the pre-rename name.

- [x] **Step 2: Rename the 37 matrix checks**

```bash
sed -i '' -E 's/^(check [0-9]+) mutants /\1 test:mutants /' "$p"
rg -o 'check [0-9]+ test:mutants ' "$p" | wc -l   # 37
rg -o 'check [0-9]+ mutants ' "$p" | wc -l         # 0
```

Expected: `37` then `0`.

- [x] **Step 3: Insert matrix case 38 (alias regression)**

Immediately before the `# summary` line at the end of the matrix block, the current text is:

```bash
check 0 test:mutants --list-files --print-ast -f src/lib.rs
# summary
```

Change it to:

```bash
check 0 test:mutants --list-files --print-ast -f src/lib.rs
# alias regression: the legacy `mutants` name resolves to this same task
# (kept via `#MISE aliases`, hidden from `mise tasks`)
check 0 mutants --list-mutators
# summary
```

- [x] **Step 4: Bump the expected count**

```bash
sed -i '' 's/MATRIX: ALL PASS (37 cases)/MATRIX: ALL PASS (38 cases)/' "$p"
rg -o 'MATRIX: ALL PASS (38 cases)' "$p" | wc -l   # 1
```

Expected: `1`.

- [x] **Step 5: Sync the alias sentence into the plan's Step 2 block + verify byte-identity**

The Step 2 block mirrors the task file's `#USAGE` section. Apply the same tail edit as Task 1 Step 4 inside the plan's Step 2 block (the unique context is):

```bash
#USAGE   report.json (on via this task's config); --jobs → --workers N;
#USAGE   --no-config → never pass (task owns --config).
#USAGE   """
```

becomes:

```bash
#USAGE   report.json (on via this task's config); --jobs → --workers N;
#USAGE   --no-config → never pass (task owns --config).
#USAGE   Alias: `mutants` remains a supported alias (`mise run mutants`) —
#USAGE   same task, hidden from `mise tasks`.
#USAGE   """
```

(There is exactly one occurrence of this tail in the plan.) Then verify block↔file identity:

```bash
hit=$(rg -n '#USAGE flag "-f --file <file>"' "$p" | head -1 | cut -d: -f1)
first=$(awk -v h="$hit" 'NR<h&&/^```/{l=NR} END{print l}' "$p")
end=$(awk -v s="$first" 'NR>s&&/^```$/{print NR; exit}' "$p")
sed -n "$((first+1)),$((end-1))p" "$p" > /var/folders/9w/3qn47_qj3m9b27gkxwr5_k9m0000gn/T/opencode/plan_s2.sh
fu=$(rg -n '^#USAGE flag "-f --file' .mise/tasks/test/mutants | cut -d: -f1)
lu=$(rg -n '^#USAGE' .mise/tasks/test/mutants | tail -1 | cut -d: -f1)
diff /var/folders/9w/3qn47_qj3m9b27gkxwr5_k9m0000gn/T/opencode/plan_s2.sh <(sed -n "${fu},${lu}p" .mise/tasks/test/mutants) && echo STEP2_IDENTICAL
```

Expected: `STEP2_IDENTICAL`.

- [x] **Step 6: Extract the matrix and run it (38 cases) — clone as runner**

**Why a clone (runner only):** the matrix invokes mutarust ~38× and the main
checkout's live `.codegraph/daemon.sock` makes every mutarust run abort (Ground
rules: socket policy). The clone has no `.codegraph`, so the suite runs clean
without touching the socket — unlike the old worktree rationale, shadowing is
NOT a factor anymore (this checkout is the repo root; alias proven in place by
Task 1 Step 8). Extraction happens in the main checkout (the edited plan lives
here); execution `cd`s to the clone. matrix.sh is self-contained, so the
uncommitted Task 3 plan edits don't need to be in the clone — but the clone
must include Tasks 1+2 (commit `89f036e8`). Expect one cold cargo build on the
first cargo-touching case; subsequent cases reuse the clone's `target/`.

```bash
cd /Users/jack/Documents/41_personal/traces-pkm
p=docs/superpowers/plans/2026-09-28-quality-gates-task-ux.md
start=$(rg -n 'declare -a fails=\(\)' "$p" | cut -d: -f1)
rel=$(sed -n "$start,$((start+95))p" "$p" | grep -n '^```$' | head -1 | cut -d: -f1)
end=$((start+rel-2))
sed -n "${start},${end}p" "$p" > /var/folders/9w/3qn47_qj3m9b27gkxwr5_k9m0000gn/T/opencode/matrix.sh
bash -n /var/folders/9w/3qn47_qj3m9b27gkxwr5_k9m0000gn/T/opencode/matrix.sh
clone=/var/folders/9w/3qn47_qj3m9b27gkxwr5_k9m0000gn/T/opencode/rename-matrix-clone
rm -rf "$clone"
git clone -q /Users/jack/Documents/41_personal/traces-pkm "$clone"
git -C "$clone" rev-parse --abbrev-ref HEAD           # quality-gates (must include T1+T2 commits)
cd "$clone" && bash /var/folders/9w/3qn47_qj3m9b27gkxwr5_k9m0000gn/T/opencode/matrix.sh
git status --porcelain mutarust-baseline.json         # empty — clone baseline untouched
git -C /Users/jack/Documents/41_personal/traces-pkm status --porcelain mutarust-baseline.json  # empty
cd /Users/jack/Documents/41_personal/traces-pkm
```

Expected: `MATRIX: ALL PASS (38 cases)` (plus possibly `BASELINE_RESTORED`
from the guard), empty baseline status in both checkouts, back in the main
checkout. (If the clone HEAD predates the rename commits — a stale clone — the
run will mass-fail on `test:mutants` unknown-task errors; delete and re-clone.)

- [x] **Step 7: hk + commit**

```bash
hk fix --safe --no-stage --unstaged 2>/dev/null || true
git add docs/superpowers/plans/2026-09-28-quality-gates-task-ux.md
git commit -m "docs(plan): track rename, add alias matrix case"
```

Expected: commit lands; hooks green.

---

### Task 4: Stale-path repairs + spec addenda

**Files:**
- Modify: `docs/refs/quality_gates_mutarust.md` (:514) and `docs/refs/quality_gates_messrust.md` (:279, :285) — reviewer-flagged current-state stale paths (sibling prose, broken relative link, pattern reference)
- Modify: `docs/superpowers/specs/2026-09-28-quality-gates-task-ux-design.md` (addendum table)
- Modify: `docs/superpowers/specs/2026-09-29-mutants-task-rename-design.md` (Decision 4 scope note + Verification 5 widened — folded into this document)

- [x] **Step 1: Stale-path repairs (current-state prose only)**

```bash
# mutarust.md:514 — sibling reference
sed -i '' 's|sibling of `mutants/_default`|sibling of `test/mutants`|' docs/refs/quality_gates_mutarust.md
# messrust.md:279 — link text + href
sed -i '' 's|\[`.mise/tasks/mutants/_default`\](../../.mise/tasks/mutants/_default)|[`.mise/tasks/test/mutants`](../../.mise/tasks/test/mutants)|' docs/refs/quality_gates_messrust.md
# messrust.md:285 — pattern reference
sed -i '' 's|pattern matches `.mise/tasks/mutants/_default`|pattern matches `.mise/tasks/test/mutants`|' docs/refs/quality_gates_messrust.md
rg -n 'mutants/_default' docs/refs/quality_gates_mutarust.md docs/refs/quality_gates_messrust.md
```

Expected remaining hits (all snapshots, untouched by design): `mutarust.md:418`,
`:431`, `messrust.md:427`. Zero hits at `:514`/`:279`/`:285`.

- [x] **Step 2: Append the addendum row**

In the `## Post-implementation corrections (2026-09-29)` table of the 09-28 spec, after the last row (the `A5 keep/delete function names` row), append exactly one new row:

```markdown
| Whole spec (approved body + earlier addendum rows) | Renamed 2026-09-29: the task is now `test:mutants` with `mutants` kept as a file-task alias (hidden from `mise tasks`); every quoted invocation above predates the rename; the plan matrix now runs 38 cases (37 renamed + one alias regression case). See `docs/superpowers/plans/2026-09-27-quality-gates.md` |
```

(This document's Decision 4 scope note and Verification 5 widening for these
repairs are already applied — uncommitted until Step 3.)

- [x] **Step 3: hk + commit**

```bash
hk fix --safe --no-stage --unstaged 2>/dev/null || true
git add docs/refs/quality_gates_mutarust.md docs/refs/quality_gates_messrust.md \
        docs/superpowers/specs/2026-09-28-quality-gates-task-ux-design.md \
        docs/superpowers/specs/2026-09-29-mutants-task-rename-design.md
git commit -m "docs: repair stale task paths, record rename addenda"
```

Expected: commit lands; hooks green.

---

### Task 5: Final gates

**Files:** none (verification only)

- [x] **Step 1: Stale-name audit over the live set**

```bash
rg -o 'mise run mutants' .mise/tasks/test/mutants | wc -l                            # 1 (alias sentence)
rg -o 'mise run test:mutants' .mise/tasks/test/mutants | wc -l                        # 12
rg -o 'mise run mutants' docs/refs/mutation_testing.md docs/refs/quality_gates_mutarust.md docs/refs/quality_gates_messrust.md | wc -l   # 0
rg -n 'mutants/_default' docs/refs/quality_gates_mutarust.md docs/refs/quality_gates_messrust.md | cut -d: -f1,2   # only 418, 431, 427
```

Expected: `1`, `12`, `0`, and line numbers `418`/`431`/`427` only (the
"sources consulted" snapshots — excluded by design; historical docs like
adoption, Part I (the 2026-09-27 plan), approved spec bodies, and `.scratch/` also
excluded by design).

- [x] **Step 2: Merge `main` into `quality-gates` (before verify)**

Gates must run on the merged result (branch is 67/76 apart; both sides touched
`.gitignore`, `messrust.md`, `mutarust.md`, `mise.toml`, `mise.lock`,
`.mise/tasks/clean/reports`). Merge — never rebase (repo history uses merge
commits). Resolve conflicts in favor of *both* semantics (union of ignore
entries; keep our rename + main's line edits).

```bash
git merge --no-ff main -m "chore(merge): merge main into quality-gates for rename gates"
# resolve any conflicts, then:
git status --porcelain | grep -v '^??'   # empty
rg -o 'mise run test:mutants' .mise/tasks/test/mutants | wc -l   # 12 (rename survived)
test -f .mise/tasks/test/mutants && test ! -e .mise/tasks/mutants && echo RENAME_SURVIVED
```

Expected: merge lands, rename intact, working tree clean (user's three
untracked plan files are fine).

- [x] **Step 3: Completion gate**

```bash
mise run verify; echo "verify_rc=$?"
```

Expected: `verify_rc=0` (fmt → check/lint/test; docs+task-file changes only,
so this is the standard warm run; verify does not invoke mutarust, so the
socket policy does not apply).

- [x] **Step 4: hk gate**

```bash
hk check --safe --skip-step gitleaks --format json 2>/dev/null | jq -r '.status'
```

Expected: `passed`.

- [x] **Step 5: Tick the plan + final commit**

```bash
sed -i '' 's/^- \[ \] \*\*Step/- [x] **Step/' docs/superpowers/plans/2026-09-29-mutants-task-rename.md
git add docs/superpowers/plans/2026-09-29-mutants-task-rename.md
git commit -m "docs(plan): mark rename plan completed"
git status --porcelain; git log --oneline -8
```

Expected: tree clean (aside from the user's three untracked plan files);
commits in order: T1 `c38d75b6`, T2 `89f036e8`, plan amendments `554db5bf`,
T3 `b522f387`, T4 `123e4204`, merge `8413178f`, this tick `b82d7796`; final
report to the user (Phase 5: switch the directory back to `main` after the
branch merges, file the mutarust upstream issue, clean up temp clones).

---

## Verification

1. `bash -n .mise/tasks/test/mutants`; `mise run --skip-deps test:mutants
   --help` → rc 0, examples show `test:mutants`, alias sentence present,
   refusal-style exit-code text intact.
2. Refusal probe: `mise run --skip-deps test:mutants --bogus` → rc 2 and the
   message names `mise run test:mutants --help`.
3. Alias probes: `mise run --skip-deps mutants -f src/lib.rs --match
   __zz_no_match__` → rc 0 (alias resolves; bounds keep it inert);
   `mise tasks | grep -c '^mutants'` → 0 (alias hidden);
   `mutants:report` still absent from `mise tasks`.
4. Extract and run the plan matrix → `MATRIX: ALL PASS (38 cases)` — the 37
   renamed checks plus new case `check 0 mutants --list-mutators` (alias
   regression proof); baseline guard behavior unchanged (merged Ground rules at
   the top of this document: matrix never commits a baseline).
5. Stale-name grep over the live set (`mutation_testing.md`,
   `quality_gates_mutarust.md`, `quality_gates_messrust.md`) → zero
   `run mutants`, zero current-state `mutants/_default`; the task file's
   `run mutants` count is 1 (the alias sentence). Snapshot exclusions and
   historical globs per Task 5 Step 1 (sources-consulted snapshots keep their
   pre-rename paths as recorded history).
6. `mise run verify` green (2988 tests, 58 doctests); `hk check --safe
   --skip-step gitleaks --format json` → `passed`; tree clean; conventional
   commits per Ground rules.

---

## Incidents (log — forward-fix only, never amend)

- **I1 — worktree retired mid-plan (2026-09-29).** The nested worktree
  `.worktrees/quality-gates` shadowed `mutants` via main's ancestor task and
  produced unexplained state churn (staged rename split across commits;
  `.mise/tasks/mutants/_default` reappearing with no hooks; `.mise/tasks/test/mutants`
  missing from disk with a clean status — cause never found). User directed
  the move to the main checkout; worktree deregistered and its leftover dir
  removed. All subsequent work happens in `/Users/jack/Documents/41_personal/traces-pkm`
  on `quality-gates`.
- **I2 — `.codegraph/daemon.sock` vs mutarust (pre-existing).** mutarust's
  workspace copier aborts on the live socket (`could not copy unsupported
  workspace entry`) and consults no gitignore (root `.gitignore` entry
  probe-verified useless — residue reverted). Mitigated per Ground rules
  socket policy; durable fix = upstream issue against
  `quality-gates/mutarust` (skip non-regular entries when copying), Phase 5 —
  https://github.com/quality-gates/mutarust/issues/205.
- **I3 — hk's stash step clears MERGE_HEAD (probable F3 root cause,
  2026-09-29).** During the Task 5 merge commit, hk's pre-commit stash
  (which stashes untracked files too — it grabbed the user's 3 untracked
  plan files on every prior commit) ran `git stash` while a merge was in
  progress; `git stash` drops `MERGE_HEAD`, so `git commit` then failed with
  `fatal: could not open '.git/MERGE_HEAD'` even though all conflicts were
  resolved and staged. Fix: temporarily move untracked files out of the
  repo, rewrite `MERGE_HEAD` (`git rev-parse main > .git/MERGE_HEAD`),
  commit — merge landed with both parents (`8413178f`), files restored.
  **Rule for future merges: stage everything + clear untracked before the
  merge commit, or expect the same failure.** This stash/restore cycle is
  also the most likely explanation for the worktree file anomalies (F3):
  every commit stashed and re-created working-tree files around probes.

---

## Out of scope (non-goals)

- No toml wrapper task (rejected: errored in probe, reintroduces the name).
- No rename of `[task_templates.mutants]` (internal, independent of task name).
- No CI/hk changes — neither references the task.
- Historical records stay untouched: `quality_gates_mise_adoption.md`,
  `2026-09-27-quality-gates-messrust-mutarust.md` (now Part I of this document —
  merged verbatim as a historical record), the approved body of
  `2026-09-28-quality-gates-task-ux-design.md`, `.scratch/` issues.
- The main checkout keeps its own task files until this branch merges —
  expected, nothing to do.
