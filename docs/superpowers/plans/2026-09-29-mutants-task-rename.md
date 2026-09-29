# Mutants Task Rename (`mutants` → `test:mutants`) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Move `.mise/tasks/mutants/_default` to `.mise/tasks/test/mutants` so the task is `test:mutants`, keep `mutants` as a hidden file-task alias, and repoint every live reference (task self-references, two refs docs, the living matrix plan, spec addenda).

**Architecture:** One `git mv`, one header line (`#MISE aliases=["mutants"]` — probe-verified on mise 2026.9.15: transparent arg forwarding, hidden from `mise tasks`), one bulk `sed` class (invocations `mise run mutants` → `mise run test:mutants`) plus three surgical doc edits, and mechanical updates to the 09-28 plan's runnable lines (37 matrix checks + step commands) while its `Line N before/after:` records and path prose stay historical. A 38th matrix case proves the alias keeps resolving.

**Tech Stack:** mise file tasks (`#MISE`/`#USAGE` headers), BSD sed/rg, the extracted bash matrix from the 09-28 plan, hk gates.

**Spec:** `docs/superpowers/specs/2026-09-29-mutants-task-rename-design.md` (approved 2026-09-29).

---

## Ground rules (non-negotiable)

- **Worktree:** `/Users/jack/Documents/41_personal/traces-pkm/.worktrees/quality-gates` (branch `quality-gates`). Never touch the main checkout.
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

---

### Task 1: Move the file + self-references + alias

**Files:**
- Move: `.mise/tasks/mutants/_default` → `.mise/tasks/test/mutants`

- [ ] **Step 1: Move the file**

```bash
cd /Users/jack/Documents/41_personal/traces-pkm/.worktrees/quality-gates
git mv .mise/tasks/mutants/_default .mise/tasks/test/mutants
rmdir .mise/tasks/mutants
test -f .mise/tasks/test/mutants && echo MOVED
test ! -d .mise/tasks/mutants && echo DIR_GONE
```

Expected: `MOVED` then `DIR_GONE`.

- [ ] **Step 2: Add the alias header line**

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

- [ ] **Step 3: Replace all 12 invocations in the file**

```bash
sed -i '' 's/mise run mutants/mise run test:mutants/g' .mise/tasks/test/mutants
rg -o 'mise run test:mutants' .mise/tasks/test/mutants | wc -l   # 12
rg -o 'mise run mutants' .mise/tasks/test/mutants | wc -l        # 0
```

Expected: `12` then `0`. The 12 = 9 help examples + the refusal message (`see 'mise run mutants --help'`) + 2 comments.

- [ ] **Step 4: Add the alias sentence to `long_help`**

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

- [ ] **Step 5: Syntax + help render**

```bash
bash -n .mise/tasks/test/mutants && echo SYNTAX_OK
out=$(mise run --skip-deps test:mutants --help 2>&1); echo "help_rc=$?"
printf '%s' "$out" | grep -o 'test:mutants' | wc -l      # 9 (the examples)
printf '%s' "$out" | grep -o 'mise run mutants' | wc -l  # 1 (the alias sentence)
```

Expected: `SYNTAX_OK`, `help_rc=0`, `9`, `1`.

- [ ] **Step 6: Refusal message probe**

```bash
out=$(mise run --skip-deps test:mutants --bogus 2>&1); rc=$?
echo "rc=$rc"; printf '%s\n' "$out" | grep -m1 'unknown or misplaced'
```

Expected: `rc=2` and the line
`mutants: unknown or misplaced flag: --bogus (declared flags go before --; see 'mise run test:mutants --help')`.

- [ ] **Step 7: Alias probes**

```bash
out=$(mise run --skip-deps mutants -f src/lib.rs --match __zz_no_match__ 2>&1); rc=$?
echo "alias_rc=$rc"
mise tasks | grep -c '^mutants'
mise tasks | grep -c 'mutants:report'
```

Expected: `alias_rc=0` (alias resolves; bounds keep the run inert), then `0` (alias hidden from the listing), then `0` (ancestor checkout's old `report` does not leak in).

- [ ] **Step 8: hk + commit**

```bash
hk fix --safe --no-stage --unstaged 2>/dev/null || true
git add -A .mise/tasks
git status --porcelain
git commit -m "refactor(tasks): rename mutants task to test:mutants"
```

Expected: commit lands (rename + edits), hooks green, no other files staged, tree otherwise clean.

---

### Task 2: Live docs

**Files:**
- Modify: `docs/refs/mutation_testing.md` (:30 + 15 invocations)
- Modify: `docs/refs/quality_gates_mutarust.md` (:570, :239, :308)

- [ ] **Step 1: mutation_testing.md**

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

- [ ] **Step 2: quality_gates_mutarust.md**

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

- [ ] **Step 3: hk + commit**

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

- [ ] **Step 1: Replace runnable invocations; revert the `Line N` records**

```bash
p=docs/superpowers/plans/2026-09-28-quality-gates-task-ux.md
sed -i '' 's/mise run mutants/mise run test:mutants/g' "$p"
sed -i '' -e '/^Line [0-9]* before:/s/mise run test:mutants/mise run mutants/' \
          -e '/^Line [0-9]* after:/s/mise run test:mutants/mise run mutants/' "$p"
rg -o 'mise run test:mutants' "$p" | wc -l   # 12
rg -o 'mise run mutants' "$p" | wc -l        # 6 (the historical Line N records only)
```

Expected: `12` then `6`. The 12 = ground-rules gotcha (:20), the 9 Step 2 block examples, the Step 3 block refusal message, and the Step 10 help probe. The 6 = the Task 6 `Line 59/92/118 before/after:` edit records — those quotes are history and must keep the pre-rename name.

- [ ] **Step 2: Rename the 37 matrix checks**

```bash
sed -i '' -E 's/^(check [0-9]+) mutants /\1 test:mutants /' "$p"
rg -o 'check [0-9]+ test:mutants ' "$p" | wc -l   # 37
rg -o 'check [0-9]+ mutants ' "$p" | wc -l         # 0
```

Expected: `37` then `0`.

- [ ] **Step 3: Insert matrix case 38 (alias regression)**

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

- [ ] **Step 4: Bump the expected count**

```bash
sed -i '' 's/MATRIX: ALL PASS (37 cases)/MATRIX: ALL PASS (38 cases)/' "$p"
rg -o 'MATRIX: ALL PASS (38 cases)' "$p" | wc -l   # 1
```

Expected: `1`.

- [ ] **Step 5: Sync the alias sentence into the plan's Step 2 block + verify byte-identity**

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

- [ ] **Step 6: Extract and run the matrix (38 cases)**

```bash
start=$(rg -n 'declare -a fails=\(\)' "$p" | cut -d: -f1)
rel=$(sed -n "$start,$((start+95))p" "$p" | grep -n '^```$' | head -1 | cut -d: -f1)
end=$((start+rel-2))
sed -n "${start},${end}p" "$p" > /var/folders/9w/3qn47_qj3m9b27gkxwr5_k9m0000gn/T/opencode/matrix.sh
bash -n /var/folders/9w/3qn47_qj3m9b27gkxwr5_k9m0000gn/T/opencode/matrix.sh
bash /var/folders/9w/3qn47_qj3m9b27gkxwr5_k9m0000gn/T/opencode/matrix.sh
git status --porcelain mutarust-baseline.json
```

Expected: `MATRIX: ALL PASS (38 cases)` (plus possibly `BASELINE_RESTORED` from the guard), and empty `git status` for the baseline.

- [ ] **Step 7: hk + commit**

```bash
hk fix --safe --no-stage --unstaged 2>/dev/null || true
git add docs/superpowers/plans/2026-09-28-quality-gates-task-ux.md
git commit -m "docs(plan): track rename, add alias matrix case"
```

Expected: commit lands; hooks green.

---

### Task 4: Spec addenda

**Files:**
- Modify: `docs/superpowers/specs/2026-09-28-quality-gates-task-ux-design.md` (addendum table)
- Modify: `docs/superpowers/specs/2026-09-29-mutants-task-rename-design.md` (Decision 4 was refined pre-plan — no further edit needed here)

- [ ] **Step 1: Append the addendum row**

In the `## Post-implementation corrections (2026-09-29)` table of the 09-28 spec, after the last row (the `A5 keep/delete function names` row), append exactly one new row:

```markdown
| Whole spec (approved body + earlier addendum rows) | Renamed 2026-09-29: the task is now `test:mutants` with `mutants` kept as a file-task alias (hidden from `mise tasks`); every quoted invocation above predates the rename; the plan matrix now runs 38 cases (37 renamed + one alias regression case). See `2026-09-29-mutants-task-rename-design.md` |
```

- [ ] **Step 2: hk + commit**

```bash
hk fix --safe --no-stage --unstaged 2>/dev/null || true
git add docs/superpowers/specs/2026-09-28-quality-gates-task-ux-design.md docs/superpowers/specs/2026-09-29-mutants-task-rename-design.md
git commit -m "docs(spec): record task rename in addenda"
```

Expected: commit lands; hooks green.

---

### Task 5: Final gates

**Files:** none (verification only)

- [ ] **Step 1: Stale-name audit over the live set**

```bash
rg -o 'mise run mutants' .mise/tasks/test/mutants | wc -l                            # 1 (alias sentence)
rg -o 'mise run test:mutants' .mise/tasks/test/mutants | wc -l                        # 12
rg -o 'mise run mutants' docs/refs/mutation_testing.md docs/refs/quality_gates_mutarust.md | wc -l   # 0
```

Expected: `1`, `12`, `0`. (Historical docs — adoption, 2026-09-27 plan, approved spec bodies, `.scratch/` — are excluded by design.)

- [ ] **Step 2: Completion gate**

```bash
mise run verify; echo "verify_rc=$?"
```

Expected: `verify_rc=0` (fmt → check/lint/test; docs+task-file changes only, so this is the standard warm run).

- [ ] **Step 3: hk gate**

```bash
hk check --safe --skip-step gitleaks --format json 2>/dev/null | jq -r '.status'
```

Expected: `passed`.

- [ ] **Step 4: Tick the plan + final commit**

```bash
sed -i '' 's/^- \[ \] \*\*Step/- [x] **Step/' docs/superpowers/plans/2026-09-29-mutants-task-rename.md
git add docs/superpowers/plans/2026-09-29-mutants-task-rename.md
git commit -m "docs(plan): mark rename plan completed"
git status --porcelain; git log --oneline -6
```

Expected: tree clean; six commits total across the plan (T1–T5, with T4 possibly folding into the flow); final report to the user.
