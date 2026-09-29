# Mutants Task Rename (`mutants` → `test:mutants`) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Move `.mise/tasks/mutants/_default` to `.mise/tasks/test/mutants` so the task is `test:mutants`, keep `mutants` as a hidden file-task alias, and repoint every live reference (task self-references, two refs docs, the living matrix plan, spec addenda).

**Architecture:** One `git mv`, one header line (`#MISE aliases=["mutants"]` — probe-verified on mise 2026.9.15: transparent arg forwarding, hidden from `mise tasks`), one bulk `sed` class (invocations `mise run mutants` → `mise run test:mutants`) plus three surgical doc edits, and mechanical updates to the 09-28 plan's runnable lines (37 matrix checks + step commands) while its `Line N before/after:` records and path prose stay historical. A 38th matrix case proves the alias keeps resolving.

**Tech Stack:** mise file tasks (`#MISE`/`#USAGE` headers), BSD sed/rg, the extracted bash matrix from the 09-28 plan, hk gates.

**Spec:** `docs/superpowers/specs/2026-09-29-mutants-task-rename-design.md` (approved 2026-09-29).

---

## Ground rules (non-negotiable)

- **Checkout:** the main directory `/Users/jack/Documents/41_personal/traces-pkm`, currently on branch `quality-gates` (user-approved 2026-09-29: the nested worktree was retired mid-plan — see Incidents). All editing happens here. This IS the repo root, so the old `.mise/tasks/mutants/` path is gone and no ancestor can shadow the alias. Branch `main` stays untouched until Task 5's merge step.
- **Clone = runner only:** logic is proven in place; a clean clone under `/var/folders/9w/3qn47_qj3m9b27gkxwr5_k9m0000gn/T/opencode/` exists solely to execute suites that must not see local runtime state (the matrix) — never to author edits.
- **Socket policy:** `.codegraph/daemon.sock` (live codegraph daemon, pid from `.codegraph/daemon.pid`) makes mutarust abort with `could not copy unsupported workspace entry` — gitignore does not protect it (probe-verified: root `.gitignore` entry changes nothing). Single bounded probes in the main dir: move the socket aside under a `trap … EXIT` guard that restores it in all cases. Long suites: use the clone — never leave the socket moved for minutes. `mise run verify` does not invoke mutarust and runs in place.
- **Anomaly protocol:** if on-disk state contradicts git (file missing/extra/restored), STOP — capture `git status --porcelain`, `git log --oneline -3`, `ls -la <path>`, `git reflog -5`; log it under Incidents; reconcile with forward-fix commits only — never amend or re-stage history to "correct" an anomaly.
- **Capture rc safely:** `out=$(cmd 2>&1); rc=$?` — never `cmd | tail; echo $?`; never pipe `mise run …` into `head` (SIGPIPE). No `timeout` binary on macOS.
- **All probes use `--skip-deps`** (skips `depends=["test"]`, ~10 s saved per probe). The one alias run (`-f src/lib.rs --match __zz_no_match__`) is bounded by spec and finishes in seconds.
- **Matrix never commits a baseline:** if the guard prints `BASELINE_RESTORED`, accept it; `mutarust-baseline.json` must be unmodified in every commit.
- **Commits:** conventional via hk, one per task, no `--no-verify`. Final hk gate is `hk check --safe --skip-step gitleaks --format json` (gitleaks is pre-existing-unknown-effect; run it manually only if secrets are a concern).
- **Counts:** `rg -c` prints nothing on zero matches — use `rg -o PATTERN FILE | wc -l` for counts that must equal 0.

---

## File structure

| File | Change |
| --- | --- |
| `.mise/tasks/mutants/_default` → `.mise/tasks/test/mutants` | Task 1 — move, `aliases` header line, 12 invocation replacements, alias sentence in `long_help` |
| `docs/refs/mutation_testing.md` | Task 2 — 15 invocations + name/path at :30 |
| `docs/refs/quality_gates_mutarust.md` | Task 2 — :570 invocation, :239 link, :308 path |
| `docs/superpowers/plans/2026-09-28-quality-gates-task-ux.md` | Task 3 — 37 matrix checks, 12 step-command invocations (6 `Line N` records reverted), alias case 38, Expected 38, Step 2 block sync |
| `docs/superpowers/specs/2026-09-28-quality-gates-task-ux-design.md`, `docs/superpowers/specs/2026-09-29-mutants-task-rename-design.md` | Task 4 — addendum row; Decision 4 already refined pre-plan |
| `docs/refs/quality_gates_mutarust.md` (:514), `docs/refs/quality_gates_messrust.md` (:279, :285) | Task 4 — reviewer-flagged current-state stale paths (sibling prose, broken relative link, pattern reference); snapshot lines (:418/:431, :427) stay historical |

---

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

Expected: `1`. (`extends="mutants"` is untouched — it targets `[task_templates.mutants]`, a template name independent of the task name.)

- [x] **Step 3: Replace all 12 invocations in the file**

```bash
sed -i '' 's/mise run mutants/mise run test:mutants/g' .mise/tasks/test/mutants
rg -o 'mise run test:mutants' .mise/tasks/test/mutants | wc -l   # 12
rg -o 'mise run mutants' .mise/tasks/test/mutants | wc -l        # 0
```

Expected: `12` then `0`. The 12 = 9 help examples + the refusal message (`see 'mise run mutants --help'`) + 2 comments.

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

Expected: `12` then `1`. These two numbers are the file's permanent invariants (spec Verification 5).

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

### Task 3: The living 09-28 plan (matrix + runnable lines) + matrix run

**Files:**
- Modify: `docs/superpowers/plans/2026-09-28-quality-gates-task-ux.md`

- [x] **Step 1: Replace runnable invocations; revert the `Line N` records**

```bash
p=docs/superpowers/plans/2026-09-28-quality-gates-task-ux.md
sed -i '' 's/mise run mutants/mise run test:mutants/g' "$p"
sed -i '' -e '/^Line [0-9]* before:/s/mise run test:mutants/mise run mutants/' \
          -e '/^Line [0-9]* after:/s/mise run test:mutants/mise run mutants/' "$p"
rg -o 'mise run test:mutants' "$p" | wc -l   # 12
rg -o 'mise run mutants' "$p" | wc -l        # 6 (the historical Line N records only)
```

Expected: `12` then `6`. The 12 = ground-rules gotcha (:20), the 9 Step 2 block examples, the Step 3 block refusal message, and the Step 10 help probe. The 6 = the Task 6 `Line 59/92/118 before/after:` edit records — those quotes are history and must keep the pre-rename name.

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
- Modify: `docs/refs/quality_gates_mutarust.md` (:514) and `docs/refs/quality_gates_messrust.md` (:279, :285) — reviewer-flagged current-state stale paths
- Modify: `docs/superpowers/specs/2026-09-28-quality-gates-task-ux-design.md` (addendum table)
- Modify: `docs/superpowers/specs/2026-09-29-mutants-task-rename-design.md` (Decision 4 scope note + Verification 5 widened)

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
| Whole spec (approved body + earlier addendum rows) | Renamed 2026-09-29: the task is now `test:mutants` with `mutants` kept as a file-task alias (hidden from `mise tasks`); every quoted invocation above predates the rename; the plan matrix now runs 38 cases (37 renamed + one alias regression case). See `2026-09-29-mutants-task-rename-design.md` |
```

(The rename spec's Decision 4 / Verification 5 scope note for these repairs
is already applied on disk — uncommitted until Step 3.)

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
adoption, the 2026-09-27 plan, approved spec bodies, and `.scratch/` also
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
commits in order: T1 `c38d75b6`, T2 `89f036e8`, plan amendments, T3, T4,
merge, this tick; final report to the user (Phase 5: switch the directory back
to `main` after the branch merges, file the mutarust upstream issue, clean up
temp clones).

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
  `quality-gates/mutarust` (skip non-regular entries when copying), Phase 5.
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
