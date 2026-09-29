# Design: quality-gates task UX hardening (strict mutants args, clean registry, mess workflow)

Date: 2026-09-28
Status: approved (design brainstorm + user review "proceed", 2026-09-28)

## Context

Three follow-ups to the completed quality-gates track (plan
`2026-09-27-quality-gates-messrust-mutarust.md` (now Part I of `2026-09-27-quality-gates.md`, fully reviewed and ticked):

1. `.mise/tasks/mutants/_default` enforces its conflict matrix with two hand-rolled
   cascades (`reject_conflicting_flags`, `reject_declared_in_inspect`) that scan raw
   token lists with `case` arms. Every new mutarust flag needs cascade edits; the
   cascades and the free-form passthrough disagree about what a flag *is*. mise's
   `#USAGE` DSL supports parse-time constraints we do not use.
2. `clean:reports` still knows only cargo-mutants-era outputs; the five mutarust
   root reports are invisible to it.
3. `mess` is advisory and manual by decision; it has only a one-line usage string —
   no workflow guidance, no flag documentation.

## Non-goals

- No report foldering (mutarust has no output-dir option; deferred with the user).
- No mess automation, gating, or `reportfile` wiring (advisory stays advisory).
- `# min_msi: 60` stays commented; `mutarust-baseline.json` stays committed at
  the root; no `hk.pkl` step; no CI.
- No behavioral change to gate/dry-run/inspect/count logic beyond what strict
  parsing makes explicit.

## Spike findings (mise 2026.9.15, /tmp/usage-spike)

Verified live; these facts drive the design:

- `#USAGE group`, flag `conflicts="…"`, `exclusive=#true`, `requires="…"`, and
  `choices` are all enforced by mise **before the script runs**, with readable
  messages (e.g. `Invalid flag --wet: cannot be used with --dry in group mode`).
  Exit code for every usage-validation failure is **1** (not the task's current 2).
- `exclusive` also refuses positional targets ("must be given on its own, and
  <targets> was given too") — matches `--list-mutators`, which takes no target.
- `--` still separates flags from positionals, but **any positional argument
  (single or variadic) absorbs unknown flag-like tokens**: `task --config x`
  yields `targets=[--config x]` with rc 0. Strict unknown-flag rejection therefore
  needs a small script-side scan of the target list.
- `-f/--file` resolves to `usage_file`, flag names map to snake-case usage vars
  (`--list-mutators` → `usage_list_mutators`), equals form `--workers=4` parses.

## Design A — `mutants/_default`: strict curated surface

### A1 Declared flags

Rule: **declare every flag mutarust's cli.md documents, minus the refusal set
(A2), plus the task's own flags.** The inventory below was captured during
design; at implementation time it is re-derived from
`docs/refs/mutarust/docs/cli.md` and any difference is either added to the
USAGE block or explicitly refused with rationale recorded in A2. Value metavars
follow cli.md.

- Task-owned: `-f/--file <file>`, `-m/--mod <module>`, `--git-diff` with
  `negate="--no-git-diff"` (default `#false`).
- Existing mutarust declarations kept: `--min-msi <n>`, `--update-baseline`,
  `--fail-on-escaped`, `--dry-run`, `--timeout <secs>`.
- Added mutarust declarations: `--baseline <file>`, `--blacklist <file>`,
  `--coverage`, `--debug`, `--do-not-remove-tmp-folder`, `--exec-timeout <secs>`,
  `--git-diff-base <ref>`, `--git-diff-lines`, `--html-output`,
  `--ignore-msi-with-no-mutations`, `--list-files`, `--list-mutators`,
  `--logger-agentic-json`, `--logger-github`, `--logger-gitlab`,
  `--logger-summary-json`, `--match <pattern>`, `--min-covered-msi <n>`,
  `--no-diffs`, `--no-silent`, `--output-statuses <letters>`, `--package <pkg>`,
  `--per-test`, `--print-ast`, `--quiet`, `--run-mutant-id <id>`, `--silent`,
  `--test-recursive`, `--timeout-coefficient <n>`, `--verbose`, `--workers <n>`,
  `--workspace`.
- Positional: `arg "[targets]" var=#true` (renamed from `[args]`; usage var
  `usage_targets`, script's `eval` and comments updated). Short help states
  these are paths / package names; flags never belong there.

### A2 Refusal set (deliberately not declared)

Tokens in this set fall through to the target scan (A4) and get tailored
messages. Nothing else is refused.

| Flag(s) | Rationale | Message shape |
| --- | --- | --- |
| `--config` | task owns `--config mutarust.yml` (policy lives there) | existing tailored text |
| `--test-flags` | task owns test flags via `TEST_FLAGS`/`--features` | existing tailored text |
| `--exec`, `--no-exec` | rejected by the old cascade today (verified unviable with task-owned test hooks) | existing tailored text |
| `--features` | new tightening: already carried by `TEST_FLAGS`; double-declaration risks cargo conflicts | new tailored text citing `TEST_FLAGS` |

### A3 Conflict matrix (declarative)

| Declaration | Constraint |
| --- | --- |
| `--list-mutators` | `exclusive=#true` (no flags, no targets alongside) |
| `--list-files`, `--print-ast` | `conflicts` with **every other declared flag except `-f`/`-m` and the sibling inspect flag** (block form, one selector per line, ≤80 cols). Mirrors the verified claim that mutarust rejects config/mutation/display options in inspect modes — strict-by-default; loosening a selector requires a citable cli.md statement plus user sign-off. |
| `--dry-run` | `conflicts`: `--update-baseline --timeout --timeout-coefficient --workers --coverage --per-test --test-recursive --exec-timeout --do-not-remove-tmp-folder` (today's dry-run matrix, verbatim) |
| `--timeout-coefficient` | `conflicts`: `--timeout --exec-timeout` |
| `--update-baseline` | `conflicts`: `--run-mutant-id` (plus `--dry-run`, declared above) |
| `--coverage` | `conflicts`: `--per-test` (cli.md forbids the pair; today it fails opaquely in mutarust) |
| `--git-diff-base` | `requires="--git-diff"` |

`-f`/`-m` carry no conflicts (legal alongside target-taking inspect modes, as
today). Negate spellings (`--no-git-diff` vs `--git-diff`, `--no-silent` vs
`--silent`) reject in either spelling.

### A4 Target parsing and scan

After usage parsing: `eval` `usage_targets` (same quoting pattern as today),
then scan tokens:

1. Token matches `-*` and is in A2 → tailored message, `exit 2`.
2. Token matches `-*` otherwise → `flag-like token in target list: <tok>
   (declared flags go before --; unknown flags are rejected — see --help)`,
   `exit 2`. This covers both absorption of unknown pre-`--` flags and
   misplaced post-`--` legacy forms (`mise run mutants -- --workers 4`).
3. Keep existing validations: `-f` file existence, `-m` module discovery
   (exit 2, unchanged messages).

### A5 Script outcome

- **Delete**: `reject_conflicting_flags`, `reject_declared_in_inspect`,
  `passthrough_has_token`, the passthrough branch of `has_fixed_timeout`, and
  the token-scan in `is_inspect_mode` (replaced by `usage_list_*` /
  `usage_print_ast` checks).
- **Keep**: `build_static_target_flags`, `build_target_flags`,
  `build_gate_flags` (still auto-pairs `--ignore-msi-with-no-mutations` with
  `--git-diff --min-msi`), `build_inspect_args`, `build_mutarust_args`
  (minus refusals), `run_inspect_mode`, `run_count_mode`, `main` flow, all
  `#MISE extends="mutants"` templates, dynamic `-f`/`-m` handling, exit
  plumbing for mutarust codes (2/3/4/…) unchanged.

### A6 Exit codes and help text

- Usage-validation failures (conflicts/exclusive/requires/choices/unknown
  declared syntax) → **rc 1** from mise, with mise's own message.
- Script failures (A2/A4 scan, `-f`, `-m`) → **rc 2**, `mutants:` messages.
- Rewrite `long_help`: examples drop the `-- ` prefix
  (`mise run mutants --list-mutators`), the exit-code paragraph states the
  1/2 split above, the inspect-mode note no longer claims exit 2 for
  conflicts, migration lines (`--jobs → --workers N`, `--run-mutant-id`)
  stay.

## Design B — `clean:reports` registry

- Add to the registry: `report.json`, `mutarust-report.html`,
  `mutarust-agentic.json`, `mutarust-summary.json`, `mutarust-gitlab.json`.
- `mutarust-baseline.json` **excluded** (tracked artifact).
- Legacy cargo-mutants entries (`lcov.info`, `tarpaulin-report.html`,
  `mutants.out`, `mutants.out.old`, `mutants-report.md`) stay.
- Update the task's `--dry-run` long_help to list the new files.
- No path/folder logic anywhere.

## Design C — mess: manual but equipped

- `mise.toml` `[tasks.mess]`: replace the one-line `usage` with a proper
  `help` + `long_help`: when to run (post-refactor / pre-commit optional),
  exit codes 0/1/2 and the `--ignore-violations-on-exit` escape for
  scripting, and a cheatsheet of messrust's 16 flags (`--color --disable
  --enable --exclude --help --ignore-errors-on-exit --ignore-tests
  --ignore-violations-on-exit --maximumpriority --minimumpriority --only
  --reportfile --strict --suffixes --verbose --version`), with
  value/choice semantics quoted from `docs/refs/messrust/docs/usage.md` —
  never invented. The task keeps its free-form passthrough: strict
  declaration (Design A) is **not** applied to mess (advisory tool, no
  conflict matrix, full mess capability stays reachable).
- `docs/refs/quality_gates_messrust.md`: append a **Workflow** section (next
  sequential §): run → read findings → act (fix or justify) → re-run; where
  `messrust.xml` tuning lives (rule refs, `reportLevel`); the advisory
  contract (never wired into verify/hk/CI). Exit-2 semantics of messrust
  itself are unchanged.

## Docs audit

| Location | Disposition |
| --- | --- |
| `mutation_testing.md:42–43` | update: inspect modes are declared flags, no `--` needed |
| `mutation_testing.md` flag-table `[args]` row (line 70) | rename to `[targets]`, paths/packages not passthrough |
| `mutation_testing.md:92,118` examples | drop `-- ` (`--workers 4`, `--run-mutant-id <id>` direct) |
| `mutation_testing.md:76` "rejected … exit 2" | rewrite for the 1/2 split (A6) |
| `quality_gates_mise_adoption.md:198,418,577,627,736` | read each in context; update only where it asserts current task behavior (736 explicitly governs `long_help` examples → update); leave historical narrative intact |
| `quality_gates_mutarust.md` | no `-- ` forms found → untouched |
| Plan file / ADR-era text | historical → untouched |
| Upstream refs (`docs/refs/mutarust/**`, `docs/refs/messrust/**`) | untouched |

## Verification

Parse-level cases are free (no cargo). Full matrix:

1. Each A3 row: conflicting pair → rc 1; `--git-diff-base` alone → rc 1;
   `--list-mutators -m x` / `--list-mutators ./src` → rc 1;
   `--list-files ./src --min-msi 50` → rc 1.
2. A2/A4: `--config x` (pre- and post-`--`) → rc 2 tailored; `--test-flags`,
   `--exec`, `--features` → rc 2 tailored; `--bogus` → rc 2 generic;
   `-- --workers 4` (legacy form) → rc 2 generic.
3. Happy paths rc 0: bare `--dry-run`; `-m strsim --dry-run`;
   `--list-mutators`; `--list-files ./src/cli/error.rs`; `-f src/lib.rs
   --dry-run`; `--git-diff`; equals form `--workers=4`; `--` then path
   targets; scored `-m strsim --fail-on-escaped` against the committed
   baseline (bounded, `nice -n 10`).
4. `clean:reports --dry-run` lists mutarust files (step 3's dry-run
   regenerates `report.json` first if absent).
5. `mise run mess --help` renders the new help; workflow section reviewed
   against `docs/refs/messrust/docs/usage.md`.
6. `shellcheck`/hk clean on edited scripts; `mise run verify` green as the
   final gate.

## Risks

- **Over-restrictive inspect conflicts**: strict-by-default may forbid a pair
  mutarust actually allows; rule in A3 — loosen only with a cli.md citation
  and explicit user approval.
- **Absorbed-token edges**: equals/short forms are covered by the matrix;
  anything missed surfaces as an rc-2 scan hit, never a silent pass.
- **mise version drift**: spike results are pinned to 2026.9.15; a mise
  upgrade that changes usage semantics would surface in the parse-level
  matrix first.

---

## Post-implementation corrections (2026-09-29)

Append-only addendum recorded while implementing plan
`2026-09-28-quality-gates-task-ux.md` (now Part II of `2026-09-27-quality-gates.md`; all seven tasks landed, matrix
37/37, `mise run verify` green). The approved text above stands as
written; where execution disproved or amended a claim, the correction is
below. Full deviation record lives in the plan's deviation notes.

| Spec claim | Correction (as implemented) |
| --- | --- |
| A1 declares `--package <pkg>` and `--workspace` | mutarust has no such flags — cargo concepts forwarded via `--test-flags`/`--test-recursive`; both intentionally absent from the declared surface (reconciliation noted at spec→plan conversion) |
| A2 `--test-flags` keeps "existing tailored text" | Reworded with the passthrough concept retired: `mutants: task owns --test-flags; drop it from the command line` |
| A3 matrix rows | Two tightening rows added beyond this table (tightening needs no sign-off per A3): `--timeout`⊥`--exec-timeout` (cli.md:237–238 alias; preserves the old cascade) and `--silent`⊥`--no-silent` (cli.md:65–66 cannot-combine; both orders matrix-tested) |
| A3 `--git-diff-base` = `requires="--git-diff"` alone | Insufficient in mise 2026.9.15: a declared `default` satisfies `requires`, and *any* explicit value (including `--no-git-diff`) satisfies it. Implemented as no-`default` on `--git-diff` (absence → rc 1) plus a script backstop in `reject_bad_targets` (explicit `--no-git-diff` + base → rc 2, old cascade message) |
| A3 inspect conflicts in "block form, one selector per line, ≤80 cols" | KDL node arguments cannot wrap; implemented as single-line 34-selector lists, validated by the help render + matrix. `--no-git-diff` added to both inspect lists per the negate rule below A3 |
| A3 negate rule "reject in either spelling" for `--silent`/`--no-silent` | Held only at mutarust rc 3 before; now declared as a bidirectional `conflicts` pair (rc 1) |
| A4 scan case: "`-- --workers 4`" → rc 2 | Void: mise strips `--` before usage parsing, so a declared flag after `--` parses normally → rc 0 (bounded matrix case). Undeclared post-`--` tokens still exit 2 via the scan; a *second* `--` also leaves following tokens in scan range (matrix case) |
| A4 generic message `flag-like token in target list: <tok> …` | Implemented as `unknown or misplaced flag: <tok> (declared flags go before --; see 'mise run mutants --help')` — same intent, clearer text |
| A4 implies a bare `--` can appear in the token scan | One separator is stripped before the script runs (single-`--` cases, matrix-verified), but with extra separators a literal `--` can survive into `targets` (triple-`--` case: rc 2, `unknown or misplaced flag: --`). Removing the old silent `--) ;;` arm is behavior-compatible: same rc, the generic arm now names the token — both edges matrix-cased |
| A5 keep/delete function names (`build_static_target_flags`, `build_mutarust_args`, `run_inspect_mode`, `run_count_mode`, …) | Partially fictional — they do not exist in the file; implementation followed the actual structure (`main` → `build_inspect_args` \| `build_static_flags` + `build_target_flags` + `build_gate_flags` + `build_declared_flags`) |
| A1 short help states "these are paths / package names" | Help says paths only — package names are not a mutarust surface concept (row 1: `--package`/`--workspace` never existed; cargo packages move via `--test-flags`/`--test-recursive`) |
| A3 inspect conflicts: "every other declared flag except `-f`/`-m` and the sibling inspect flag" | Implemented literally, with two clarifications: `--list-mutators` is not in the two 34-selector lists — its `exclusive` refuses any companion anyway (same rc 1, matrix-cased); sibling inspect flags pair legally and both were probed (`--list-files --print-ast` → rc 0, list-files wins — now matrix-cased) |
| Whole spec (approved body + earlier addendum rows) | Renamed 2026-09-29: the task is now `test:mutants` with `mutants` kept as a file-task alias (hidden from `mise tasks`); every quoted invocation above predates the rename; the plan matrix now runs 38 cases (37 renamed + one alias regression case). See `docs/superpowers/plans/2026-09-27-quality-gates.md` (Part III; the former plan files were consolidated into it 2026-09-29) |
