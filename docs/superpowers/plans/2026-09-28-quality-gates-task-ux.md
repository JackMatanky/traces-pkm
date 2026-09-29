# Quality-Gates Task UX Hardening Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the `mutants` task's hand-rolled conflict cascades with mise `#USAGE` parse-time constraints over a strict declared flag surface, register mutarust reports in `clean:reports`, and equip the advisory `mess` task with real help + a workflow doc.

**Architecture:** `.mise/tasks/mutants/_default` declares every mutarust flag (minus five task-owned refusals) with `conflicts`/`exclusive`/`requires` constraints; a small script-side target scan replaces the cascades (usage validation exits 1, scan exits 2). `clean/reports` gains the five mutarust root files. `mess` keeps free-form passthrough but gains `help`/`long_help`; `quality_gates_messrust.md` gains a Workflow section; `mutation_testing.md` is updated to match the new UX.

**Tech Stack:** mise task files (`#USAGE` KDL-lite spec), bash (`set -euo pipefail`), mutarust 0.1.10, messrust, markdown docs.

**Spec:** `docs/superpowers/specs/2026-09-28-quality-gates-task-ux-design.md` (approved, committed `01933d0d`).

---

## Ground rules (non-negotiable)

- **Worktree:** `/Users/jack/Documents/41_personal/traces-pkm/.worktrees/quality-gates` (branch `quality-gates`). Never touch the main checkout.
- **No multi-hour runs.** Every mutarust invocation is scoped (`-m strsim`, `--list-*`, `--git-diff` on a src-unchanged branch) and prefixed `nice -n 10`. Bash tool timeouts ≥ `900000` for cargo-touching commands.
- **Verification lives in the repo, not /tmp.** Usage-constraint enforcement was nondeterministic outside this project (settings drift); inside the worktree it was stable across repeat runs. All probes below run from the worktree root.
- **Pipeline gotchas:** never pipe `mise run mutants` into `head` (SIGPIPE); capture rc with `out=$(cmd 2>&1); rc=$?` — never `cmd | tail; echo $?`; zsh aborts unquoted `echo ===x===` (`=` expansion); no `timeout` binary on macOS.
- **Caching quirks (spike-verified, mise 2026.9.15):**
  - `mise run --force` bypasses task freshness/replay; `mise run --skip-deps` skips `depends=["test"]` (saves ~10 s per probe — use it for every parse-level case).
  - `conflicts="--a --b"` (space-separated in one attribute) is a **silent no-op**. Multi-selector conflicts MUST use node-args form: `conflicts "--a" "--b"`.
  - `exclusive`/`requires` must be **attributes** on the flag node (child form = invalid spec); `choices` is a child; unknown flags are absorbed by the variadic positional (rc 0) — that is exactly what the target scan catches.
- Commits: conventional (hk enforces), one per task, no `--no-verify`.

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

- [ ] **Step 1: Record baseline behavior (red)**

```bash
cd /Users/jack/Documents/41_personal/traces-pkm/.worktrees/quality-gates
out=$(mise run --skip-deps mutants --dry-run --update-baseline 2>&1); echo "rc=$?"; printf '%s\n' "$out" | grep -m1 mutants
out=$(mise run --skip-deps mutants --config mutarust.yml 2>&1); echo "rc=$?"; printf '%s\n' "$out" | grep -m1 mutants
```

Expected: `rc=2` both, with `mutants:` messages (`--update-baseline cannot be combined with --dry-run`; `task owns --config`). These two cases must diverge after the rewrite: first → rc 1 (mise usage), second → rc 2 (scan).

- [ ] **Step 2: Replace the entire `#USAGE` block (lines 6–54) with the following**

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
#USAGE flag "--git-diff" negate="--no-git-diff" default=#false {
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
#USAGE flag "--silent" help="No per-mutant output (overrides --output-statuses)"
#USAGE flag "--no-silent" help="Emit per-mutant output (task default; overrides a config silent_mode)"
#USAGE flag "--output-statuses <letters>" help="Show only chosen mutant states (k/e/s/c/x; overrides --quiet)"
#USAGE flag "--quiet" help="Counts and summary only (overridden by --output-statuses)"
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
#USAGE   - `mise run mutants -m index` - mutate module `index`
#USAGE   - `mise run mutants -m index --dry-run` - count only for one module
#USAGE   - `mise run mutants --min-msi 60 --fail-on-escaped` - gate on new escapes only
#USAGE   - `mise run mutants --git-diff` - only changed lines (tracked; stage new files first)
#USAGE   - `mise run mutants --workers 4` - cap parallel workers
#USAGE   - `mise run mutants --list-mutators` - list built-in mutators
#USAGE   - `mise run mutants --list-files` - print selected files (NOTE: ignores exclude_dirs)
#USAGE   - `mise run mutants src/cli/error.rs` - explicit target positional (skips the default)
#USAGE   - `mise run mutants --run-mutant-id <id>` - re-run one mutant to verify a kill
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
#USAGE   """
#USAGE }
```

Notes: multi-selector `conflicts` uses node-args form only (space-separated attributes are a silent no-op). `exclusive`/`requires` stay attributes. Inventory reconciled against `docs/refs/mutarust/docs/cli.md` during spec→plan conversion: mutarust has **no** `--package`/`--workspace` flags (they are cargo concepts passed through `--test-flags`/`--test-recursive`); they are intentionally absent here.

Spec deviations recorded at conversion (report again in Task 7 Step 4):

- **`--timeout ⊥ --exec-timeout` (new row beyond spec A3):** cli.md:237–238 says `--timeout` is an **alias** of `--exec-timeout`, and the old cascade refused the pair (declared `--timeout` + passthrough `--exec-timeout`, lines 176–181). Declaring the conflict preserves today's behavior; tightening needs no sign-off (only loosening does, per spec A3).
- **Inspect conflict selectors are single-line:** spec A3 asked for block form ≤80 cols, but KDL node arguments cannot wrap across lines (unverified otherwise); the 34-selector lines are parsed and validated by Step 10's help render + Step 11's matrix.
- **`--no-git-diff` added to both inspect conflict lists:** spec A3's "negate spellings reject in either spelling" rule, applied to the inspect row.
- **Spec A5's `build_static_target_flags`/`build_mutarust_args`/`run_inspect_mode`/`run_count_mode` do not exist in the file** — the plan follows the actual structure (`main` → `build_inspect_args` | `build_static_flags` + `build_target_flags` + `build_gate_flags`).

- [ ] **Step 3: Script — header, arrays, parse + scan (replaces lines 70–143 and `parse_passthrough_tokens`/`passthrough_has_token`/`has_fixed_timeout`)**

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
# Globals:
#   targets
# Arguments:
#   None
# Outputs:
#   Diagnostic on STDERR for a rejected token.
# Returns:
#   Exits 2 on a refused/unknown flag-like token; 0 otherwise.
########################################
reject_bad_targets() {
  local t
  for t in "${targets[@]+"${targets[@]}"}"; do
    case "${t}" in
      --)
        ;;
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
        echo "mutants: unknown or misplaced flag: ${t} (declared flags go before --; see 'mise run mutants --help')" >&2
        exit 2
        ;;
    esac
  done
  return 0
}
```

- [ ] **Step 4: Delete `reject_conflicting_flags`, `passthrough_has_token`, `passthrough_has_positional`, `reject_declared_in_inspect`, `has_fixed_timeout` and their docblocks entirely** (approx. current lines 98–143, 145–257, 286–309, 311–357).

- [ ] **Step 5: Rewrite `is_inspect_mode` (current lines 259–284)**

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

- [ ] **Step 6: Rewrite `build_inspect_args` (current lines 359–394)**

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

- [ ] **Step 7: Rewrite `build_static_flags` (current lines 396–426)**

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

- [ ] **Step 8: `build_gate_flags` — add `--git-diff-base` inside the `--git-diff` branch (current lines 491–505)**

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

- [ ] **Step 9: Add `build_declared_flags` (new function, place after `build_gate_flags`) and update `build_target_flags` + `main`**

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

- [ ] **Step 10: Syntax + help render**

```bash
bash -n .mise/tasks/mutants/_default && echo SYNTAX_OK
out=$(mise run mutants --help 2>&1); echo "rc=$?"
printf '%s\n' "$out" | grep -q "exits 1 for usage-validation" && echo HELP_OK
printf '%s\n' "$out" | grep -q -- "--workers 4" && echo EXAMPLES_OK
```

Expected: `SYNTAX_OK`, `rc=0`, `HELP_OK`, `EXAMPLES_OK`.

- [ ] **Step 11: Parse-level matrix (validation cases are instant; scan cases use `--skip-deps`)**

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
check 1 mutants --dry-run --update-baseline -f src/lib.rs --match __zz_no_match__
check 1 mutants --dry-run --timeout 5 -f src/lib.rs --match __zz_no_match__
check 1 mutants --dry-run --workers 2 -f src/lib.rs --match __zz_no_match__
check 1 mutants --dry-run --timeout-coefficient 3 -f src/lib.rs --match __zz_no_match__
check 1 mutants --dry-run --coverage -f src/lib.rs --match __zz_no_match__
check 1 mutants --dry-run --per-test -f src/lib.rs --match __zz_no_match__
check 1 mutants --dry-run --test-recursive -f src/lib.rs --match __zz_no_match__
check 1 mutants --dry-run --exec-timeout 30 -f src/lib.rs --match __zz_no_match__
check 1 mutants --dry-run --do-not-remove-tmp-folder -f src/lib.rs --match __zz_no_match__
check 1 mutants --timeout-coefficient 3 --timeout 5 -f src/lib.rs --match __zz_no_match__
check 1 mutants --timeout-coefficient 3 --exec-timeout 30 -f src/lib.rs --match __zz_no_match__
check 1 mutants --timeout 5 --exec-timeout 30 -f src/lib.rs --match __zz_no_match__
check 1 mutants --update-baseline --run-mutant-id deadbeef -f src/lib.rs --match __zz_no_match__
check 1 mutants --coverage --per-test -f src/lib.rs --match __zz_no_match__
check 1 mutants --git-diff-base origin/main -f src/lib.rs --match __zz_no_match__
check 1 mutants --dry-run --workers=4 -f src/lib.rs --match __zz_no_match__
check 1 mutants --list-mutators --verbose
check 1 mutants --list-mutators ./src...
check 1 mutants --list-mutators -m index
check 1 mutants --list-files --min-msi 50
check 1 mutants --list-files --verbose
check 1 mutants --print-ast --dry-run
# target scan → rc 2 (bounds are inert here: the scan exits before mutarust)
check 2 mutants --config mutarust.yml -f src/lib.rs --match __zz_no_match__
check 2 mutants -- --config mutarust.yml -f src/lib.rs --match __zz_no_match__
check 2 mutants --test-flags "--features x" -f src/lib.rs --match __zz_no_match__
check 2 mutants --exec -f src/lib.rs --match __zz_no_match__
check 2 mutants --features x -f src/lib.rs --match __zz_no_match__
check 2 mutants --bogus -f src/lib.rs --match __zz_no_match__
check 2 mutants -- --workers 4 -f src/lib.rs --match __zz_no_match__
check 2 mutants -x -f src/lib.rs --match __zz_no_match__
# happy parse path → rc 0 (inspect is the only fast rc-0 path; dry-run
# happy paths are Task 2's Steps 1/1b)
check 0 mutants --list-files src/lib.rs
# summary
if [[ ${#fails[@]} -eq 0 ]]; then echo "MATRIX: ALL PASS ($count cases)"; else
  printf 'MATRIX FAILURES:\n'; printf '  %s\n' "${fails[@]}"; fi
# any failed-pair declaration could have let a run touch the baseline:
if [[ -n "$(git status --porcelain mutarust-baseline.json)" ]]; then
  git checkout -- mutarust-baseline.json; echo BASELINE_RESTORED
fi
```

Expected: `MATRIX: ALL PASS (31 cases)`, then either no output or `BASELINE_RESTORED` from the baseline guard. If any `rc 1`/`rc 2` case returns `0`: run it again with `mise run --force --skip-deps …`; if it still returns 0, stop and report (mise usage-validation bug/setting drift — do not paper over with script code). If `mutarust-baseline.json` was rewritten, the guard restores it — never commit a matrix-touched baseline.

- [ ] **Step 12: `hk` on the edited file + commit**

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

- [ ] **Step 1: Dry-run count (bounded scope)**

```bash
nice -n 10 mise run --skip-deps mutants -m strsim --dry-run
```

Expected: rc 0, mutant counts printed (≈6 mutants), `report.json` written. Timeout 900000 ms.

- [ ] **Step 1b: dry-run happy paths — `-f`, `--`-targets, bare (spec verification 3)**

```bash
out=$(nice -n 10 mise run --skip-deps mutants --dry-run -f src/lib.rs 2>&1); echo "rc=$?"
out=$(nice -n 10 mise run --skip-deps mutants --dry-run -- src/cli/error.rs 2>&1); echo "rc=$?"
out=$(nice -n 10 mise run --skip-deps mutants --dry-run 2>&1); echo "rc=$?"
```

Expected: `rc=0` ×3 — single-file scope, explicit `--`-target positional, then the full-project count (the last is the slowest matrix-adjacent step; count-only, no tests). Timeout 900000 ms.

- [ ] **Step 2: Scored gate against the committed baseline**

```bash
out=$(nice -n 10 mise run --skip-deps mutants -m strsim --fail-on-escaped 2>&1); echo "rc=$?"; printf '%s\n' "$out" | tail -5
```

Expected: rc 0 (baseline absorbs current escapes; `-m strsim` is Task 11's proven bounded scope). Timeout 900000 ms.

- [ ] **Step 3: git-diff empty-Rust-diff run**

```bash
out=$(nice -n 10 mise run --skip-deps mutants --git-diff 2>&1); echo "rc=$?"; printf '%s\n' "$out" | grep -E 'total|score|mutant' | tail -3
```

Expected: rc 0 — the branch changes touch no `src/` lines, so mutarust selects zero mutants (fast; no gate present).

- [ ] **Step 4: Real inspect run**

```bash
out=$(mise run --skip-deps mutants --list-mutators 2>&1); echo "rc=$?"; printf '%s\n' "$out" | grep -c 'statement/'
```

Expected: rc 0, count > 10 (mutator names listed).

- [ ] **Step 5: Commit any fallout fixes (or skip when clean)**

```bash
git status --porcelain
```

Expected: empty (verification-only). If any earlier step forced `_default` fixes, commit them as `fix(mutants): <what>`.

---

### Task 3: `clean:reports` registry

**Files:**
- Modify: `.mise/tasks/clean/reports` (lines 4–9 long_help; lines 25–33 registry)

- [ ] **Step 1: Add the five mutarust files to `get_known_report_registry`**

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

- [ ] **Step 2: Update the `--dry-run` long_help (lines 6–8)**

```bash
#USAGE   Preview mode: prints what report files (`lcov.info`, `tarpaulin-report.html`, `mutants.out/`, `mutants-report.md`, `report.json`, `mutarust-report.html`, `mutarust-agentic.json`, `mutarust-summary.json`, `mutarust-gitlab.json`) exist and would be deleted without removing them.
```

- [ ] **Step 3: Verify listing (Task 2's runs regenerated the mutarust reports)**

```bash
bash -n .mise/tasks/clean/reports && echo SYNTAX_OK
out=$(mise run clean:reports --dry-run 2>&1); echo "rc=$?"; printf '%s\n' "$out" | grep -cE 'mutarust|report\.json'
```

Expected: `SYNTAX_OK`, `rc=0`, count ≥ 3 — at least `report.json` + `mutarust-report.html` + `mutarust-agentic.json` regenerated by Task 2 (`mutarust-summary.json`/`mutarust-gitlab.json` appear only when their logger flags are passed, so they may be absent). Never run `clean:reports` without `--dry-run` in this session.

- [ ] **Step 4: Commit**

```bash
git add .mise/tasks/clean/reports
git commit -m "feat(clean): register mutarust report files"
```

---

### Task 4: `mess` task help + workflow text

**Files:**
- Modify: `mise.toml` lines 225–238 (`[tasks.mess]`)

- [ ] **Step 1: Confirm baseline advisory contract (red for the help, green for behavior)**

```bash
out=$(mise run mess 2>&1); echo "rc=$?"          # expect 2 (findings)
out=$(mise run mess -- --ignore-violations-on-exit >/dev/null 2>&1); echo "rc=$?"  # expect 0
out=$(mise run mess --help 2>&1); printf '%s\n' "$out" | head -3   # current one-line help
```

Expected: `2`, `0`, then the current minimal usage. Preserve both rc values after the rewrite.

- [ ] **Step 2: Replace the `usage = '…'` line (mise.toml line 228) with a multiline TOML basic-string spec**

```toml
usage = 'arg "[args]" var=#true {
  help "Extra arguments forwarded to messrust (e.g. --strict, --only Rule, --reportfile f.json)"
  long_help """
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
  """
}'
```

If mise rejects the multiline spec (`invalid usage spec` warning or `--help` failing), fall back to keeping the original one-line `usage` and moving the workflow text into Task 5's doc section only — then note the fallback in the commit message.

- [ ] **Step 3: Verify help renders, behavior unchanged, forwarding intact**

```bash
out=$(mise run mess --help 2>&1); echo "rc=$?"; printf '%s\n' "$out" | grep -q "Workflow" && echo HELP_OK
out=$(mise run mess 2>&1); echo "rc=$?"                          # expect 2
out=$(mise run mess -- --ignore-violations-on-exit >/dev/null 2>&1); echo "rc=$?"   # expect 0
```

Expected: `rc=0`, `HELP_OK`, `2`, `0`.

- [ ] **Step 4: Commit**

```bash
git add mise.toml
git commit -m "feat(mess): workflow help for the advisory task"
```

---

### Task 5: `quality_gates_messrust.md` — Workflow section

**Files:**
- Modify: `docs/refs/quality_gates_messrust.md` (append after §9, line 1182; file currently ends inside §9's location list)

- [ ] **Step 1: Append the new section**

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

- [ ] **Step 2: Sanity-check heading sequence and trailing whitespace**

```bash
grep -n '^## ' docs/refs/quality_gates_messrust.md | tail -3
```

Expected: `## 9. First advisory run (2026-09-27)` then `## 10. Workflow` as the last heading.

- [ ] **Step 3: Commit**

```bash
git add docs/refs/quality_gates_messrust.md
git commit -m "docs(messrust): add day-to-day workflow section"
```

---

### Task 6: `mutation_testing.md` audit

**Files:**
- Modify: `docs/refs/mutation_testing.md` (six edits)

- [ ] **Step 1: §2 config paragraph (lines 41–43)**

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

- [ ] **Step 2: §3 flag table (lines 59 and 70)**

Line 59 before: `Declared flags (see \`mise run mutants --help\` for the full contract):`
Line 59 after: `Selected declared flags (the task declares mutarust's full surface minus its owned flags — see \`mise run mutants --help\` for the complete contract):`

Line 70 before: `| \`[args]\` | passthrough after \`--\` (e.g. \`--list-mutators\`, \`--workers 4\`) |`
Line 70 after: `| \`[targets]\` | positional target paths (flags are declared above; never passed after \`--\`) |`

- [ ] **Step 3: rejection + exit-code prose (lines 75–79)**

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

- [ ] **Step 4: example commands (lines 92 and 118)**

Line 92 before: `mise run mutants -- --workers 4      # cap parallel workers`
Line 92 after: `mise run mutants --workers 4        # cap parallel workers`

Line 118 before: `mise run mutants -- --run-mutant-id <id>  # re-run just that mutant → killed?`
Line 118 after: `mise run mutants --run-mutant-id <id>  # re-run just that mutant → killed?`

- [ ] **Step 5: Confirm no other stale forms remain in live docs**

```bash
grep -rn 'run mutants -- ' docs/refs/ || echo "no stale passthrough forms"
grep -rn 'exit 2' docs/refs/mutation_testing.md
```

Expected: first grep → `no stale passthrough forms` (scope is `docs/refs/` only — historical plan files under `docs/superpowers/` legitimately quote the old forms and are out of scope); second shows only the line updated in Step 3 (plus any mutarust-internal `exit 2` mentions, which stay). `docs/refs/quality_gates_mise_adoption.md` lines 198/418/577/627/736 are a historical research log (they quote the pre-rename `mutarust` entrypoint and completed recommendations) — **leave them untouched**; this deviates from the spec's tentative "736 might need update" note because context reading showed it records a completed recommendation, not current behavior.

- [ ] **Step 6: Commit**

```bash
git add docs/refs/mutation_testing.md
git commit -m "docs: align mutation testing guide with strict mutants args"
```

---

### Task 7: Final gates

- [ ] **Step 1: Full verify gate**

```bash
mise run verify
```

Expected: rc 0 (fmt, check, lint, test — ~2904 tests + 58 doctests; the pre-existing `dead_code` warning in `src/index/inlinks.rs` is not ours). Timeout 900000 ms.

- [ ] **Step 2: hk pass over the tree**

```bash
hk check --safe --format json 2>/dev/null | jq -r '.status'
```

Expected: `passed` (key is `status`, not `overall`). If not: `hk fix --safe --no-stage --unstaged`, review diff, re-run.

- [ ] **Step 3: Tree state + summary**

```bash
git status --porcelain; git log --oneline -7
```

Expected: clean tree; the five task commits (Tasks 1, 3, 4, 5, 6 — plus any Task 2 fallout fix) visible on top of `01933d0d`.

- [ ] **Step 4: Report back** — outcomes of matrix, happy paths, verify/hk, plus any spec deviations (e.g. mess multiline-spec fallback) for the user's review.

---

## Spec coverage map

| Spec section | Plan location |
| --- | --- |
| A1 declared surface (+ cli.md reconciliation note on `--package`/`--workspace`) | Task 1 Step 2 |
| A2 refusal set + tailored messages | Task 1 Step 3 (`reject_bad_targets`), long_help |
| A3 conflict matrix (dry-run ×9, timeout pairs incl. the `--timeout`⊥`--exec-timeout` extension, update-baseline pair, coverage⊥per-test, git-diff-base requires, exclusive, inspect ×34 selectors incl. `--no-git-diff`) | Task 1 Step 2, notes, matrix Step 11 |
| A4 target scan (`-`-prefixed → exit 2, tailored vs generic, `--` dropped) | Task 1 Step 3 |
| A5 deletions (`reject_conflicting_flags`, `reject_declared_in_inspect`, `passthrough_has_token`, `passthrough_has_positional`, `has_fixed_timeout`, passthrough forwarding; spec A5's other function names don't exist in the file — see Step 2 notes) | Task 1 Steps 3–9 |
| A6 exit-code contract 1/2 + help rewrite | Task 1 Step 2 long_help, Step 10, matrix Step 11 |
| B registry + long_help | Task 3 |
| C mess help/long_help, passthrough kept, no automation | Task 4 (Workflow §: Task 5) |
| Docs audit table | Task 6 (adoption doc = leave, with rationale) |
| Verification matrix | Task 1 Step 11 + Task 2 + Task 7 |
| Risks: over-restrictive inspect conflicts (loosen only with cli.md citation + user sign-off), version drift (2026.9.15 pinned), enforcement determinism (`--force` escalation in matrix step) | Task 1 Step 11 escalation note; Ground rules |
