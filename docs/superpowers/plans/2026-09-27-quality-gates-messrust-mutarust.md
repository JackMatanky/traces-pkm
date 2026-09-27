# Quality Gates: messrust + mutarust Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

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

- [ ] **Step 1: Confirm exactly these files are pending**

Run: `git status --porcelain`
Expected: the three `docs/refs/quality_gates_*` paths (two modified, one untracked) plus `?? docs/superpowers/plans/`. Nothing else.

- [ ] **Step 2: Commit**

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

- [ ] **Step 1: Confirm the tool is absent (red)**

Run: `command -v messrust || echo "messrust: not installed"`
Expected: `messrust: not installed`

- [ ] **Step 2: Add the tools line**

In `mise.toml`, in the `# -- Testing & Quality --` block, after the line `"cargo:cargo-mutants" = "latest"` (line 30), insert:

```toml
"cargo:https://github.com/quality-gates/messrust" = { version = "tag:v0.1.15" }
```

- [ ] **Step 3: Lock**

Run: `mise lock`
Expected: exit 0; `git diff --stat mise.lock` shows a change; `rg 'messrust' mise.lock` prints an entry.
Fallback: if lock refuses the fresh pin (24 h release-age), change to `tag:v0.1.14`, re-run `mise lock`, note the substitution.

- [ ] **Step 4: Install (source build, ~1–3 min)**

Run: `mise install`
Expected: exit 0.

- [ ] **Step 5: Verify (green)**

Run: `messrust --version; echo "rc=$?"`
Expected: rc=0 and output containing `0.1.15` (exact format unverified; any nonzero rc → stop and read install output).

- [ ] **Step 6: Commit**

```bash
git add mise.toml mise.lock
git commit -m "chore: install messrust via mise"
```

---

### Task 2: `messrust.xml` policy (fail-first)

**Files:**
- Create: `messrust.xml` (repo root)

- [ ] **Step 1: Run against the missing policy file (red)**

Run: `messrust src text messrust.xml --ignore-tests; echo "rc=$?"`
Expected: **rc=1** (error beats findings) with an error about the ruleset (an unloadable policy is a configuration error; rc=2 is impossible here because rc=2 requires *findings*, which need a loaded policy). The message naming `messrust.xml` is likely but undocumented — rc=1 alone is the pass criterion. If in doubt add `--verbose` (documented as "ruleset load diagnostics").

- [ ] **Step 2: Write the policy**

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

- [ ] **Step 3: Run the policy (green)**

Run: `messrust src text messrust.xml --ignore-tests; echo "rc=$?"`
Expected: rc=2 with findings listed (the research predicts codesize findings on this codebase), **or** rc=0 if the policy is already quiet. rc=1 = config error → add `--verbose` (`messrust src text messrust.xml --ignore-tests --verbose`) and fix the XML (most likely a rule name).

- [ ] **Step 4: Machine triage output (format `json`, not `text`)**

Run: `messrust src json messrust.xml --ignore-tests --reportfile /tmp/messrust.json --ignore-violations-on-exit; echo "rc=$?"`
Expected: rc=0; `/tmp/messrust.json` exists and is non-empty.
**Note:** the FORMAT positional (2nd) selects report content — `--reportfile` only redirects it to a file (and empties stdout). The `.json` extension does *not* imply JSON (`text … --reportfile x.json` writes text). Structured fields documented: `path`, `line`, `rule`, `priority`, `message`, `context`, `suppression`.

- [ ] **Step 5: Count findings per rule**

Run: `grep -oE '"rule"[[:space:]]*:[[:space:]]*"[^"]+"' /tmp/messrust.json | sort | uniq -c | sort -rn`
Expected: one line per rule with findings. If the grep is empty, inspect the real shape first (`jq 'keys' /tmp/messrust.json`, then drill into the findings array) and adapt — then continue with the counts.

- [ ] **Step 6: Commit**

```bash
git add messrust.xml
git commit -m "chore: add messrust policy file"
```

---

### Task 3: `[tasks.mess]` in mise.toml

**Files:**
- Modify: `mise.toml` (after `[tasks.crap]`, line ~222)

- [ ] **Step 1: Task absent (red)**

Run: `mise tasks | grep -w mess; echo "rc=$?"`
Expected: rc=1 (no match).

- [ ] **Step 2: Add the task**

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

- [ ] **Step 3: Task present (green)**

Run: `mise tasks | grep -w mess`
Expected: a line `mess  Static mess detection with ...`.

- [ ] **Step 4: Run through the task**

Run: `mise run mess; echo "rc=$?"`
Expected: same findings as Task 2 Step 3; rc=2 (mise may wrap it — any nonzero other than a mise usage error is acceptable here; findings must be on stdout).

- [ ] **Step 5: Passthrough works**

Run: `mise run mess -- --ignore-violations-on-exit --reportfile /tmp/messrust.json; echo "rc=$?"`
Expected: rc=0; `/tmp/messrust.json` refreshed (text format — this file is only a smoke artifact; use Task 2 Step 4's `json` invocation for real triage).

- [ ] **Step 6: Lint surface still healthy**

Run: `mise run lint`
Expected: exit 0 (no Rust changes; confirms mise/hk plumbing still healthy after the mise.toml edit).

- [ ] **Step 7: Commit**

```bash
git add mise.toml
git commit -m "feat: add mise mess task"
```

---

### Task 4: First mess triage + tune

**Files:**
- Modify: `messrust.xml` (only if a threshold below trips)
- Modify: `docs/refs/quality_gates_messrust.md` (append §9)

- [ ] **Step 1: Get per-rule counts**

Run: `messrust src json messrust.xml --ignore-tests --reportfile /tmp/messrust.json --ignore-violations-on-exit && grep -oE '"rule"[[:space:]]*:[[:space:]]*"[^"]+"' /tmp/messrust.json | sort | uniq -c | sort -rn | tee /tmp/mess-rule-counts.txt`
Expected: file of `count rule` lines. (Format `json` is required — see Task 2 Step 4.)

- [ ] **Step 2: Apply the decision table**

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

- [ ] **Step 3: Record the observations**

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

- [ ] **Step 4: Verify still runnable**

Run: `mise run mess -- --ignore-violations-on-exit; echo "rc=$?"`
Expected: rc=0; no config errors.

- [ ] **Step 5: Commit**

```bash
git add messrust.xml docs/refs/quality_gates_messrust.md
git commit -m "chore: triage first messrust run and tune policy"
```

---

### Task 5: Install mutarust via mise

**Files:**
- Modify: `mise.toml` (Testing & Quality block)
- Modify: `mise.lock`

- [ ] **Step 1: Absent (red)**

Run: `command -v mutarust || echo "mutarust: not installed"`
Expected: `mutarust: not installed`

- [ ] **Step 2: Add tools line**

After the messrust line inserted in Task 1, insert:

```toml
"cargo:mutarust" = "0.1.10"
```

- [ ] **Step 3: Lock**

Run: `mise lock`
Expected: exit 0; `rg 'mutarust' mise.lock` prints an entry.
Fallback A: fresh-pin refusal → `"cargo:mutarust" = "0.1.9"`, re-lock, note it.
Fallback B (crate/version unpublished — vendored `docs/parity.md` says crates.io publication was still issue #28): switch to the git backend
`"cargo:https://github.com/quality-gates/mutarust" = { version = "tag:v0.1.10" }`
(or the latest existing tag if that ref 404s — check `git ls-remote --tags https://github.com/quality-gates/mutarust`), re-lock, note the substitution and the actual version.

- [ ] **Step 4: Install**

Run: `mise install`
Expected: exit 0. (cargo-binstall may report no prebuilt asset and fall back to source build — that is normal, not an error.)

- [ ] **Step 5: Verify (green) + doc-conformance spot check**

Run:
```bash
mutarust --version; echo "rc=$?"
mutarust --help | grep -cE -- '--logger-agentic-json|--git-diff-lines|--run-mutant-id'
```
Expected: rc=0; version output resolves the pin; grep count ≥ 3 (proves the installed build matches the vendored docs' flag surface — the docs were generated against 0.2.0, so this check matters). If flags are missing, STOP: the pin and the docs disagree; escalate before writing config/tasks.

- [ ] **Step 6: Commit**

```bash
git add mise.toml mise.lock
git commit -m "chore: install mutarust via mise"
```

---

### Task 6: `mutarust.yml` policy (fail-first)

**Files:**
- Create: `mutarust.yml` (repo root)

- [ ] **Step 1: Run without the config (red)**

Run: `mise exec -- mutarust --config mutarust.yml --dry-run; echo "rc=$?"`
Expected: nonzero rc (3 config error or 1 command error) with a message naming `mutarust.yml`.

- [ ] **Step 2: Write the policy**

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

- [ ] **Step 3: Run with the config (green)**

Run: `mise exec -- mutarust --config mutarust.yml --dry-run; echo "rc=$?"`
Expected: rc=0 and a mutant count printed.
**Note:** mutarust's no-target default is *undocumented* (docs only document a default for `--list-files`). If this errors about a missing/invalid target, rerun with an explicit `./src...`, record which form worked in Task 10's §11 record (row "default target"), and continue — the `mutants` task always supplies a target anyway.

- [ ] **Step 4: Validate schema-shape acceptance**

Run: `mise exec -- mutarust --config mutarust.yml --dry-run 2>&1 | head -5`
Expected: no config/schema error lines (unknown field or bad type would exit 3 naming the file).

- [ ] **Step 5: Commit**

```bash
git add mutarust.yml
git commit -m "chore: add mutarust.yml policy"
```

---

### Task 7: Replace `.mise/tasks/mutants/_default` with the mutarust implementation

The entrypoint keeps its stable name `mutants`; only the engine changes (locked decision 9). The legacy cargo-mutants script remains recoverable in git history (needed context for Task 9 is embedded below).

**Files:**
- Modify (replace): `.mise/tasks/mutants/_default` (keep executable bit)

- [ ] **Step 1: Confirm the legacy script is what's there today (red)**

Run: `head -6 .mise/tasks/mutants/_default; mise tasks | grep -w mutants`
Expected: line 2 is `#MISE description="Run parallel mutation testing with \`cargo-mutants\`"`; the `mutants` task (and `mutants:report`) are listed with the legacy description.
Also (background, no action): `bash -x .mise/tasks/mutants/_default --list 2>&1 | tail -3` dies at `build_filter_flags` under `set -e` — the legacy task is dead today, which is fine; we are replacing it, not fixing it.

- [ ] **Step 2: Replace the file content**

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
#USAGE   - `mise run mutants -- --run-mutant-id <id>` - re-run one mutant to verify a kill
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

# Optional cap-lints: see quality_gates_mise_adoption.md §10.2 row 2 —
# verify the Task 10 P3 A/B before enabling; it also affects depends=["test"]
# and invalidates build caches.
# #MISE env={ RUSTFLAGS = "--cap-lints=allow" }

# File description (Google style §4.1) — placed after the #MISE/#USAGE block
# because mise requires its headers at the top of the file.
# The `mutants` task: stable, tool-agnostic entrypoint for mutation testing.
# Engine: mutarust. All conditional builders end in `return 0` (set -e trap).

# shellcheck disable=SC2154  # usage_* are injected by mise
set -euo pipefail

declare -a mutarust_args=()
declare -a passthrough=()

# Edit here for the Task 10 P2 experiment (--profile mutants acceptance).
TEST_FLAGS="--features test-utils --all-targets"
TIMEOUT_COEFFICIENT=5
DEFAULT_TARGET="./src..."

########################################
# Parses variadic passthrough tokens into an array.
# Globals:
#   usage_args
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
# diagnostic instead.
# Globals:
#   usage_update_baseline
#   usage_dry_run
#   usage_git_diff
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
      --dry-run)
        echo "mutants: use the declared --dry-run flag, not passthrough" >&2
        exit 2
        ;;
      --timeout | --exec-timeout | --timeout-coefficient | --workers | \
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
# bare `mise run mutants` scoped to production code.
# Globals:
#   usage_file
#   usage_mod
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
  if [[ -z "${usage_file:-}" && -z "${usage_mod:-}" ]]; then
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
  build_static_flags
  build_target_flags
  build_gate_flags
  mutarust_args+=("${passthrough[@]+"${passthrough[@]}"}")

  mutarust "${mutarust_args[@]}"
}

main "$@"
```

- [ ] **Step 3: Keep executable + shellcheck**

Run: `chmod +x .mise/tasks/mutants/_default && shellcheck .mise/tasks/mutants/_default; echo "rc=$?"`
Expected: rc=0, no findings.

- [ ] **Step 4: Task visible with new description (green)**

Run: `mise tasks | grep -w '^mutants'`
Expected: `mutants` line now reads "Run mutation testing (engine: `mutarust`)" (the `mutants:report` line still exists — deleted at cutover).

- [ ] **Step 5: Help renders**

Run: `mise run mutants --help`
Expected: rc=0; usage block showing `-f`, `-m`, `--min-msi`, `--update-baseline`, `--fail-on-escaped`, `--git-diff`, `--dry-run`, `--timeout`, and the long_help (exit codes + migration cheatsheet).

- [ ] **Step 6: Passthrough probe (`--` forwarding on file tasks)**

Run: `mise run mutants -- --list-mutators | head -5; echo "rc=$?"`
Expected: rc=0 and mutator names — proves `--`-args land in `usage_args` for this file task. If instead you get a usage error, record it in §11 (Task 10) and stop: every later passthrough step needs rework first.

- [ ] **Step 7: Dry-run through the task (proves flag conditionality)**

Run: `mise run mutants --dry-run; echo "rc=$?"`
Expected: rc=0 with a count. If the static `--test-flags`/`--timeout-coefficient` were *not* omitted, mutarust would reject the line (rc 1/3) — rc=0 proves the condition works.

- [ ] **Step 8: Default-target scope**

Run: `mise run mutants -- --list-files > /tmp/mf.txt; echo "rc=$?"; wc -l < /tmp/mf.txt; grep -c '^src/cli' /tmp/mf.txt || true`
Expected: rc=0; count > 100 files; `src/cli` lines **will** appear — `exclude_dirs` intentionally does not affect `--list-files` (known quirk, §9.18.5); scope is enforced at run time.
Branch: if mutarust rejects `./src...` (rc 1/3 with a usage/target error), edit `DEFAULT_TARGET` to `"./src"`, re-run Step 7 (expect rc=0), then compare `mise run mutants --dry-run` counts before/after the change to confirm the fallback still selects the full production tree; record the outcome in Task 10's §11 record (`default target` row).

- [ ] **Step 9: `-m` mapping for dir and file modules**

Run: `mise run mutants -m index --dry-run; echo "rc=$?"` then `mise run mutants -m strsim --dry-run; echo "rc=$?"`
Expected: both rc=0 (`index` is a dir → `./src/index...`; `strsim` is `src/strsim.rs` → file target).

- [ ] **Step 10: Unknown module errors cleanly**

Run: `mise run mutants -m nosuchmod --dry-run; echo "rc=$?"`
Expected: rc=2, stderr `mutants: unknown module: nosuchmod`.

- [ ] **Step 11: Conflicting flags reject at task level**

Run: `mise run mutants --update-baseline --dry-run; echo "rc=$?"` then `mise run mutants --dry-run -- --workers 4; echo "rc=$?"`
Expected: both rc=2 with `mutants:` diagnostics (never reaching mutarust).

- [ ] **Step 12: Commit**

```bash
git add .mise/tasks/mutants/_default
git commit -m "feat(mutants): swap task engine from cargo-mutants to mutarust"
```

---

### Task 8: gitignore + smoke tests (report-on-exit-4 FIRST)

**Files:**
- Modify: `.gitignore` (after line 22 `/mutants-report.md`)

- [ ] **Step 1: Add mutarust artifacts to .gitignore**

After the `/mutants-report.md` line, insert:

```gitignore
/report.json
/mutarust-agentic.json
/mutarust-summary.json
/mutarust-gitlab.json
/mutarust-report.html
```

(`mutarust-baseline.json` is deliberately NOT ignored — it gets committed in Task 11.)

- [ ] **Step 2: Pick a small test module**

Run:
```bash
for m in strsim delimiter hash position dirs duration; do
  printf '%s: ' "$m"
  mise run mutants -m "$m" --dry-run 2>/dev/null | tail -1
done
```
Expected: one printed count per module. Choose the module with the **smallest count > 0**; call it `M` for later steps (it must work for both engines: raw cargo-mutants in Task 9 handles dir modules via `src/<M>/**/*.rs` and file modules via `src/<M>.rs`).

- [ ] **Step 3: CRITICAL — reports must exist on gate failure (exit 4)**

Run: `mise run mutants -m M --min-msi 99 -- --timeout 600 > /tmp/m4.txt 2>&1; echo "rc=$?"` (substitute the real module), then `tail -20 /tmp/m4.txt`.
Expected: rc=4 (or rc=0 if M scored 100 — check `jq '.stats.msi' report.json`), **and**:
`ls -la report.json mutarust-agentic.json mutarust-report.html` → **all three exist**, `jq '.escaped_count' mutarust-agentic.json` prints a number.
This run also proves timeout conditionality under *both* possible `--`-parse readings (declared `--timeout` is emitted from `usage_timeout`; if instead raw-passthrough, `has_fixed_timeout` suppresses the coefficient).
**Triage before escalating:**
- rc 1/3 or the run stopped *before* mutation (clean test suite failed — see tail of `/tmp/m4.txt`) → fix the build/test failure first; missing reports are then expected, not a design failure.
- Gate genuinely exited 4 but a report file is missing → **stop.** Record the failure in `docs/refs/quality_gates_mise_adoption.md` §8 checklist and escalate to the user — the agent triage design depends on reports surviving exit 4.

- [ ] **Step 4: Zero-mutant run still writes reports**

Run: `rm -f report.json && mise run mutants --git-diff; echo "rc=$?"; jq -c '.stats' report.json`
Expected: rc=0 (docs-only changes ⇒ zero mutable lines) **and** `report.json` exists again with zero-mutant stats. If the file was not rewritten on the empty run, note it in Task 10's §11 record (limitation: rely on stdout for empty runs).
**Reminder:** report.json is clobbered by *every* later run — never read it after an intervening `--dry-run`.

- [ ] **Step 5: Mutator inventory reachable**

Run: `mise run mutants -- --list-mutators | head -15`
Expected: rc=0, list of mutator names.

- [ ] **Step 6: Duplicate `--config` precedence**

Run: `printf 'silent_mode: false\n' > /tmp/other.yml && mise run mutants -m strsim --dry-run -- --config /tmp/other.yml; echo "rc=$?"; ls -la report.json 2>&1 | tail -1`
Expected: rc=0 and no config error — the run completes, proving a passthrough `--config` does not hard-fail the command line; `report.json` still exists (if the *losing* config were the task's own with `json_output: false`, reports would vanish — its presence indicates the task config won or both agree). Record "accepted"/"rejected" (+ report presence) in Task 10's §11 record. If mutarust rejects duplicate `--config` outright, document in the task's `long_help` that `--config` is task-owned.

- [ ] **Step 7: Commit**

```bash
git add .gitignore
git commit -m "chore: ignore mutarust report artifacts"
```

---

### Task 9: P1a — old-engine baseline (raw `cargo mutants`)

Task 7 replaced the legacy task script, so the old engine is measured via raw CLI — which is the honest baseline anyway, since the legacy task never ran past its `set -e` trap (Grounding facts). The flags below replicate what the legacy script assembled (captured from the pre-swap file; recoverable via `git log -p -- .mise/tasks/mutants/_default`). `.cargo/mutants.toml` still exists and is auto-applied (nextest, `test-utils` feature, excludes, `timeout_multiplier = 5`).

**Files:** none written outside gitignored `mutants.out/`

- [ ] **Step 1: Confirm both engines available**

Run: `mise exec -- cargo mutants --version; mutarust --version`
Expected: both rc=0.

- [ ] **Step 2: Run the old engine on module M**

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

- [ ] **Step 3: No commit**

Nothing tracked changes (`mutants.out/` is still gitignored). Proceed to Task 10.

---

### Task 10: Pilot A/B + experiments (record results)

**Files:**
- Modify: `.mise/tasks/mutants/_default` (only if P2 passes)
- Modify: `docs/refs/quality_gates_mise_adoption.md` (append §11)

Use the module `M` chosen in Task 8 Step 2; old-engine numbers come from Task 9.

- [ ] **Step 1 (P1): New tool on same scope**

Run: `/usr/bin/time -p mise run mutants -m M > /tmp/ab-new.txt 2>&1; echo "new_rc=$?"`
Expected: rc=0 (no gates yet). Record wall-clock from `time -p` and `jq -c '.stats' report.json` (escapedCount, msi, skippedCount) — read it **immediately**, before any other run clobbers it.

- [ ] **Step 2 (P2): `--profile mutants` in `--test-flags`**

Run:
```bash
mise exec -- mutarust --config mutarust.yml --logger-agentic-json \
  --test-flags "--features test-utils --all-targets --profile mutants" \
  --timeout-coefficient 5 "./src/M..."
```
Two outcomes:
- **Accepted** (rc=0 or 4, run completes): edit `TEST_FLAGS` in `.mise/tasks/mutants/_default` to `"--features test-utils --all-targets --profile mutants"`; `[profile.mutants]` in `Cargo.toml` is retained at cutover. Re-verify: `mise run mutants -m M --dry-run` → rc 0 (dry-run still omits it).
- **Rejected** (rc=1 or 3, argument error): record "profile does not port"; `[profile.mutants]` gets removed in Task 12.

- [ ] **Step 3 (P3): cap-lints A/B (highest-risk gap)**

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

- [ ] **Step 4 (P4): Record everything**

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
Duplicate `--config` passthrough: <accepted | rejected → long_help note;
  report.json present: yes/no>
Passthrough probe (`-- --list-mutators`): <worked | usage error → rework>
Any other surprises: <list or "none">
```

- [ ] **Step 5: Commit**

```bash
git add docs/refs/quality_gates_mise_adoption.md .mise/tasks/mutants/_default
git commit -m "chore: record quality-gate pilot results"
```
(The second path is staged even when P2 didn't change it — unstage if `git status` shows it unmodified.)

---

### Task 11: Stage 2 — full run + committed baseline

**Files:**
- Create: `mutarust-baseline.json` (commit)
- Modify: `mutarust.yml` (only if MSI ≥ 60)

- [ ] **Step 1: Full run**

Run: `mise run mutants > /tmp/full-run.txt 2>&1; echo "rc=$?"`
Expected: rc=0 (no score gates configured yet). Runtime up to the template's 1 h cap.
Branch: if mise kills it at the 1 h timeout, rerun unbounded (same flags, no task wrapper):
```bash
mise exec -- mutarust --config mutarust.yml --logger-agentic-json \
  --test-flags "<TEST_FLAGS value from the task>" --timeout-coefficient 5 "./src..."
```

- [ ] **Step 2: Record the score**

Run: `jq -c '.stats' report.json | tee /tmp/full-stats.json`
Expected: all counts + `msi` (ratio 0–1). Save the output — it goes into the §11-style record below and drives Step 4. Do this **before** any other mutarust invocation (reports are clobbered per run).

- [ ] **Step 3: Write the baseline**

Run: `mise run mutants --update-baseline; echo "rc=$?"` then `jq '.mutants | length' mutarust-baseline.json`
Expected: rc=0; length is the escaped count (likely > 0). Note: by design this writes **no report files** and exits before gates — do not mistake exit 0 for a completed scored run.

- [ ] **Step 4: Score gate decision**

Run: `jq '.stats.msi' report.json`
- If `msi >= 0.6`: in `mutarust.yml` replace the commented gate block with an active `min_msi: 60`.
- If `< 0.6`: leave it commented; change the comment to `# min_msi: 60  # blocked: measured MSI = <value> on 2026-09-27` and note in §11.

- [ ] **Step 5: Green gate with baseline (green)**

Run: `mise run mutants --fail-on-escaped > /tmp/gate.txt 2>&1; echo "rc=$?"; jq -c '.stats' report.json`
Expected: rc=0 (every escape is in the committed baseline). rc=4 here means `mutarust-baseline.json` is stale vs the run — re-run Step 3 once and retry.

- [ ] **Step 6: Commit**

```bash
git add mutarust-baseline.json mutarust.yml
git commit -m "chore: commit mutarust baseline"
```

---

### Task 12: Stage 3 — retire cargo-mutants

**Files:**
- Modify: `mise.toml` (remove tools line)
- Modify: `mise.lock` (regenerate)
- Delete: `.mise/tasks/mutants/report`, `.cargo/mutants.toml` (**keep** `.mise/tasks/mutants/_default` — it is the live task)
- Modify: `.gitignore` (remove 3 lines)
- Modify: `Cargo.toml` (conditional: remove `[profile.mutants]`)

- [ ] **Step 1: Remove the tools line + relock**

Delete `"cargo:cargo-mutants" = "latest"` from `mise.toml`, then:
Run: `mise lock && mise install && mise ls 2>/dev/null | grep -c cargo-mutants; echo "rc=$?"`
Expected: lock diff drops cargo-mutants; install exit 0; grep finds nothing (rc=1).

- [ ] **Step 2: Delete legacy report task, config, and stale artifacts**

Run: `rm .mise/tasks/mutants/report && rm .cargo/mutants.toml && rm -rf mutants.out mutants.out.old mutants-report.md`
Expected: report task + config gone, stale cargo-mutants run artifacts removed (they must NOT be re-ignored once Step 3 lands, or `git add -A` would sweep them in); **`.mise/tasks/mutants/_default` must still exist**; `git status --porcelain` shows only the intended deletions.

- [ ] **Step 3: Clean .gitignore**

Remove these three lines (20–22) from `.gitignore`:

```gitignore
/mutants.out/
/mutants.out.old/
/mutants-report.md
```

- [ ] **Step 4: `[profile.mutants]` decision**

- If Task 10 P2 **passed** (TEST_FLAGS contains `--profile mutants`): keep `Cargo.toml:264-267` unchanged.
- If P2 **failed**: remove from `Cargo.toml`:

```toml
[profile.mutants]
inherits = "test"
opt-level = 1
debug = false
```

- [ ] **Step 5: No dangling references in code/config**

Run:
```bash
rg -n 'cargo-mutants|mutants\.out|mutants-report|mutants\.toml' \
  --hidden -g '!.git' -g '!docs/refs/quality_gates_*' \
  -g '!docs/refs/mutation_testing.md' -g '!docs/superpowers/**' .
```
Expected: no output. (Allowed leftovers: the research docs, the plan itself, and `docs/refs/mutation_testing.md` — rewritten next task. The live `.mise/tasks/mutants/_default` intentionally contains none of these strings — its cheatsheet says "the previous engine", not `cargo-mutants`.) Also run `rg 'cargo-mutants' mise.lock` → no output.

- [ ] **Step 6: Entrypoints healthy**

Run: `mise run mutants -m strsim --dry-run; echo "mu_rc=$?"` then `mise run mutants:report 2>&1 | tail -2; echo "rep_rc=$?"`
Expected: `mu_rc=0` (the stable `mutants` entrypoint runs the mutarust engine); `mutants:report` errors with a task-not-found style message and nonzero rc.

- [ ] **Step 7: Commit**

```bash
git add -A mise.toml mise.lock .mise/tasks .cargo .gitignore Cargo.toml
git commit -m "chore: retire cargo-mutants in favor of mutarust"
```

---

### Task 13: Rewrite `docs/refs/mutation_testing.md`

**Files:**
- Rewrite: `docs/refs/mutation_testing.md` (replace all 190 lines)

- [ ] **Step 1: Replace the file**

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
- **errored**: the mutated code failed to compile.
- **skipped**: not run (no tests reference it, cfg-gated, or type-proof
  machinery declined it). Skipped mutants count toward the score — they are
  not evidence of test strength.
- **MSI**: mutation score = (killed + errored + skipped) / total, reported as
  a 0–1 ratio in JSON and accepted as 0–100 on `--min-msi`.

---

## 2. Tooling: `mutarust` (+ `cargo-nextest` for the test task)

1. **`mutarust`** — the mutation engine. Installed via `mise` in
   `mise.toml` `[tools]` (`"cargo:mutarust" = "0.1.10"`), wrapped by the
   **tool-agnostic `mutants` task** (`.mise/tasks/mutants/_default`) — the
   entrypoint name predates the engine and outlives it.
2. **`cargo-nextest`** — still used by the `mise run test` task for its
   fail-fast/parallel runner. Mutation runs themselves use plain
   `cargo test` (mutarust's default); the custom `--exec` nextest path is
   serial and silently skips type-proof mutators, so it is not used.

Config lives in two committed files (per-tool convention, like
`clippy.toml`/`deny.toml`):

- `mutarust.yml` — policy (exclusions, mutators, outputs, score gate).
  mutarust has **no config auto-discovery**: the task always passes
  `--config mutarust.yml`.
- `mutarust-baseline.json` — committed list of accepted escapes
  (Mutago-compatible). Treat it like a lockfile.

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
| `--git-diff` | scope to lines changed vs `origin/HEAD` |
| `--dry-run` | count only (omits `--test-flags`/timeout flags) |
| `--timeout <secs>` | fixed per-test timeout (suppresses `--timeout-coefficient`) |
| `[args]` | passthrough after `--` (e.g. `--list-mutators`, `--workers 4`) |

Static behavior of the task: `--config mutarust.yml
--logger-agentic-json --test-flags "--features test-utils --all-targets
--timeout-coefficient 5"`, `depends = ["test"]`, 1 h template timeout.
Conflicting flag combinations (e.g. `--update-baseline --dry-run`) are
rejected at the task level with exit 2 and a `mutants:` message.

**Exit codes:** task usage errors exit `2`. mutarust itself: `0` pass ·
`1` tool error · `2` bash completion (not a run) · `3` config/parse/
annotation error · `4` quality gate red (`min_msi`, `min_covered_msi`, or
`--fail-on-escaped`; `--run-mutant-id` bypasses gates). **Four is the only
failure worth retrying with a narrower scope.** Any zero-mutant scope with
`--min-msi` exits 4 (score 0); only `--git-diff` auto-pairs
`--ignore-msi-with-no-mutations`.

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
#    ...write a targeted assertion in the nearby test file...
mise run mutants -- --run-mutant-id <id>  # re-run just that mutant → killed?
# 5. Accept any remaining intentional escapes
mise run mutants --update-baseline
```

`mutarust-agentic.json` (always written by the task) is the structured
successor of the old `mutants-report.md` "Instructions for Next Agent
Session" block: per escape it carries the mutation diff, context lines,
nearby test files, and a kill hint. Human triage: stdout table,
`report.json`, and `mutarust-report.html`.

Report files (all gitignored, written to CWD after a completed run):
`report.json`, `mutarust-agentic.json`, `mutarust-report.html`. Every run —
including `--dry-run` — overwrites them: read stats right after the run you
care about.

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
- Replaces `cargo-mutants` (retired 2026-09-27); the historical
  cargo-mutants research remains in `docs/refs/quality_gates_*.md`.
````

- [ ] **Step 2: Sanity-scan the rewrite**

Run: `rg -n 'cargo mutants --|mutants\.out|--in-diff|--iterate' docs/refs/mutation_testing.md; echo "rc=$?"`
Expected: rc=1 (no stale cargo-mutants usage remains except the one historical mention of the retirement).

- [ ] **Step 3: Commit**

```bash
git add docs/refs/mutation_testing.md
git commit -m "docs: rewrite mutation testing guide for mutarust"
```

---

### Task 14: Final verification

- [ ] **Step 1: Full gate**

Run: `mise run verify`
Expected: exit 0 (`fmt` → `check`/`lint`/`test` all pass; no Rust sources changed, so this is a plumbing check).

- [ ] **Step 2: Both entrypoints from cold**

Run: `mise run mess -- --ignore-violations-on-exit; echo "mess_rc=$?"` then `mise run mutants -m strsim --dry-run; echo "mu_rc=$?"`
Expected: `mess_rc=0`, `mu_rc=0`.

- [ ] **Step 3: hk check on changed docs**

Run:
```bash
printf 'docs/refs/quality_gates_messrust.md\0docs/refs/quality_gates_mise_adoption.md\0docs/refs/mutation_testing.md\0docs/superpowers/plans/2026-09-27-quality-gates-messrust-mutarust.md\0' \
  | hk check --skip-step gitleaks --format json --files0-from -
```
Expected: `overall: passed` (gitleaks skipped because `--safe` refuses its unknown effect; run `gitleaks detect` manually if secrets are a concern for these files — they contain no secrets).

- [ ] **Step 4: Clean tree**

Run: `git status --porcelain`
Expected: empty. `git log --oneline -12` shows the plan's commits in order.

- [ ] **Step 5: Final commit (if any verification fix was needed)**

```bash
git add -A
git commit -m "chore: quality-gate verification fixes"
```
(Only if Step 1–4 produced changes; otherwise skip.)
