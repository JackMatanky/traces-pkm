# Design: rename the `mutants` mise task to `test:mutants` (alias kept)

Date: 2026-09-29
Status: approved (design brainstorm + user approval of delta, 2026-09-29)

## Context

The `mutants` task lives at `.mise/tasks/mutants/_default`. The directory form
existed to host the `mutants:report` sibling; that report task was deleted at
the mutarust cutover (`ffb725d6`), so the directory now earns nothing. A
`test:` family already exists (`test`, `test:doc`, `test:unit`), and mutation
testing belongs to it. Rename the invocation to `test:mutants`; keep `mutants`
working as an alias.

Verified during brainstorm:

- `[task_templates.mutants]` (mise.toml:315) is what `#MISE extends="mutants"`
  targets — a template name, independent of the task name. The header's
  `extends` needs no change, and the rename removes the current task/template
  name ambiguity.
- mise 2026.9.15 supports `#MISE aliases=["…"]` on file tasks: alias
  invocations forward all arguments (including flag-like ones, e.g.
  `--dry-run`) transparently, and the alias stays hidden from `mise tasks`
  (no second help surface). A toml wrapper task was tried as the alternative
  and errored; the file-task alias is strictly better here.
- Ancestor task dirs do not leak: the main checkout still ships
  `.mise/tasks/mutants/report`, yet from this worktree `mise tasks` lists no
  `mutants:report`. Verification can assert `mutants:report`'s absence
  directly.

## Non-goals

- No toml wrapper task (rejected: errored in probe, reintroduces the name).
- No rename of `[task_templates.mutants]` (internal, independent of task name).
- No CI/hk changes — neither references the task.
- Historical records stay untouched: `quality_gates_mise_adoption.md`,
  `2026-09-27-quality-gates-messrust-mutarust.md`, the approved body of
  `2026-09-28-quality-gates-task-ux-design.md`, `.scratch/` issues.
- The main checkout keeps its own task files until this branch merges —
  expected, nothing to do.

## Decision

1. **Move**: `git mv .mise/tasks/mutants/_default .mise/tasks/test/mutants`;
   the `mutants/` directory disappears, `test/` gains a file, and the task is
   named `test:mutants` from its path. `depends=["test"]` and the rest of the
   header stay byte-identical except the two additions in 2 and 3.
2. **Alias**: add `#MISE aliases=["mutants"]` to the header, and one sentence
   in the `long_help` migration section documenting the alias (`mutants`
   remains a supported alias). Without the sentence the alias is undiscoverable
   because `mise tasks` hides it.
3. **In-file self-references (12 sites)**: the 9 help examples
   (`mise run mutants …` → `mise run test:mutants …`), the refusal message
   (`see 'mise run mutants --help'` → `test:mutants`), and 2 comments.
   After the edit the file contains exactly one `run mutants` occurrence —
   the intentional alias sentence.
4. **Live docs**:
   - `docs/refs/mutation_testing.md` — 15 invocations → `test:mutants`, plus
     the current-behavior mention at :30 (`tool-agnostic mutants task` and
     its path). The deleted-`report` mention at :130 stays historical.
   - `docs/refs/quality_gates_mutarust.md` — :570 invocation, :239 relative
     link (text + href), :308 current-state path. Lines citing the deleted
     `mutants/report` task (including its historical `mutants:report`
     mentions) stay untouched.
   - Stale-path repairs flagged by review — current-state prose only, not
     snapshots: `quality_gates_mutarust.md:514` (sibling reference),
     `quality_gates_messrust.md:279` (link text + href) and `:285`
     (pattern reference), all `mutants/_default` → `test/mutants`.
     The "sources consulted" snapshots (`mutarust.md:418/:431`,
     `messrust.md:427`) keep the pre-rename paths as recorded history.
   - Plan `2026-09-28-quality-gates-task-ux.md` — **runnable lines only**: the
     37 matrix `check N mutants …` lines become `check N test:mutants …`, plus
     one new alias case (below), plus `mise run mutants …` in step commands
     and expected outputs. The `Line N before/after:` edit records and
     `.mise/tasks/mutants/_default` path prose stay as recorded history.
   - Plan `2026-09-27-quality-gates-messrust-mutarust.md` — untouched
     (completed historical record).
   - Spec `2026-09-28-quality-gates-task-ux-design.md` — one new addendum row
     recording the rename and the alias; the approved body keeps its quoted
     pre-rename invocations; the row notes the plan matrix now runs 38 cases.

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
   regression proof). Baseline guard behavior unchanged; never commit a
   matrix-touched baseline.
5. Stale-name grep over the live set (`mutation_testing.md`,
   `quality_gates_mutarust.md`, `quality_gates_messrust.md`) → zero
   `run mutants`, zero current-state `mutants/_default`; the task file counts
   1 (the alias sentence). Snapshot exclusions: `mutarust.md:418/:431`,
   `messrust.md:427`. Historical globs excluded as in previous audits.
6. `mise run verify` green; `hk check --safe --skip-step gitleaks --format
   json` → `passed`; tree clean; conventional commits (no `--no-verify`).
