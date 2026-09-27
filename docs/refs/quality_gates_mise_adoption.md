# mise Adoption Design — messrust + mutarust, and the cargo-mutants migration

Decision-ready design for adopting
[messrust](https://github.com/quality-gates/messrust) and
[mutarust](https://github.com/quality-gates/mutarust) through mise, including a
staged migration off the current `cargo-mutants` setup. Companion to
[quality_gates_messrust.md](quality_gates_messrust.md) (§4 install, §7 proposed
artifacts), [quality_gates_mutarust.md](quality_gates_mutarust.md) (§4 install,
§8 proposed artifacts), and [mutation_testing.md](mutation_testing.md).

**Status: proposal.** Assembled 2026-09-26 from prior research notes, this
repo's files, the vendored mise docs under `docs/refs/mise_tasks/`, current
online mise/usage docs, and the crates.io / GitHub APIs. **Nothing was
installed, no task was executed, no config was edited.** Every behavior that
mise or the tools only *documents* is marked *unverified* where it matters, and
§8 lists what to run before trusting this design.

## 1. Scope

Five questions, one section each:

1. `[tools]` install lines, and lockfile / CI / cargo-binstall implications (§2)
2. TOML task vs file task per tool, full sketches, template reuse (§3)
3. Caching semantics and decisions (§4)
4. Staged migration from cargo-mutants (§5)
5. Human + agent optimization of the tasks (§6)

Out of scope (unchanged decisions): no `hk.pkl` step, no `verify`/`check`
inclusion, no CI job — mutation stays outside hk exactly as cargo-mutants is
today (`hk.pkl` has no mutation reference; `.mise/tasks/mutants/*` is invoked
by hand), and messrust stays advisory until its policy is quiet.

## 2. Q1 — Install lines, lockfile, CI, binstall

### 2.1 Recommended `[tools]` lines

Under the existing `# -- Testing & Quality --` group
([mise.toml:26-30](../../mise.toml)):

```toml
# -- Testing & Quality --
"github:nextest-rs/nextest" = "latest"
"cargo:cargo-tarpaulin" = "latest"
"cargo:cargo-crap" = "latest"
"cargo:cargo-mutants" = "latest"          # retired in stage 3 (§5.1)
"cargo:https://github.com/quality-gates/messrust" = { version = "tag:v0.1.15" }
"cargo:mutarust" = "0.1.10"
```

Then:

```sh
mise lock          # resolves without installing
mise install       # installs the recorded resolutions
```

### 2.2 Why this form per tool

**messrust — cargo git form, pinned tag.** Verified reasoning, mostly already
established in `quality_gates_messrust.md` §4, refreshed today:

- crates.io is stale (0.1.0, 12+ releases behind at research time) → `"cargo:messrust"` rejected.
- `github:quality-gates/messrust` ships **darwin-only** release assets — confirmed again today: [latest release](https://github.com/quality-gates/messrust/releases/tag/v0.1.15) carries only `messrust_0.1.15_darwin_{amd64,arm64}.tar.gz` + `checksums.txt`. The `github:` backend matches release assets, never source archives, so it cannot build for Linux ([github backend docs](https://mise.jdx.dev/dev-tools/backends/github.html)) → rejected as non-CI-viable.
- The cargo git form — `"cargo:https://github.com/owner/repo"` with `version = "tag:…"` — is documented at [cargo backend docs](https://mise.jdx.dev/dev-tools/backends/cargo.html) and is already proven in this repo: `"cargo:https://github.com/snowmead/rust-docs-mcp" = { version = "branch:main", crate = "rust-docs-mcp" }` ([mise.toml:41](../../mise.toml)). messrust is a single-crate repo (no workspace), so no `crate =` selector is needed.
- **Pin, don't float.** Upstream ships near-daily: the prior note pinned `tag:v0.1.12`; today's release is **v0.1.15** (published 2026-09-26). Exact tag pins are the only stable choice.

**mutarust — registry `cargo:` form, pinned version.**

- The crate is on crates.io as `mutarust` (binary `mutarust`, `rust-version = 1.85`, edition 2024 — nightly-2026-09-09 satisfies it). **Latest today is 0.1.10** (crates.io API, 2026-09-26); the prior note's `0.1.9` pin is one release behind. Pin `"0.1.10"`.
- Recent releases publish **no prebuilt assets** (only v0.1.4 did), so the cargo backend's binstall-first path (this repo installs `cargo-binstall`, [mise.toml:14](../../mise.toml)) finds nothing and falls back to `cargo install` (binstall's exit-94 fallback) — a source build either way. Same for messrust's git form, which **always** runs `cargo install --git` and never consults binstall.

### 2.3 Lockfile and CI implications

Verified from [mise.lock docs](https://mise.jdx.dev/dev-tools/mise-lock.html)
and this repo:

- `lockfile = true` is already set ([mise.tom:47](../../mise.toml)); `mise.lock` is committed (354 lines today, `[[tools."cargo:cargo-mutants"]] version = "27.1.0"` at mise.lock:107).
- `mise lock` resolves **without installing** and also picks up tools declared only in tasks/templates (it reads definitions without running them). `mise install --locked` is the strict CI form.
- **The cargo backend is exempt from URL/checksum locking** — listed explicitly among backends skipped by URL locking — so both new entries lock as **version-only** top-level pins, exactly like the existing cargo entries (e.g. rust-docs-mcp records `version = "branch:main"` + `specifiers`, no checksum; mise.lock:132-138). Consequence: a retagged/moved git ref would *not* be caught by checksum verification — the tag string itself is the pin. Acceptable here (upstream releases are marked immutable), but worth knowing.
- **Bumping:** `mise lock --bump <tool>` only re-resolves *fuzzy* selectors (`latest`, prefixes); **exactly pinned versions are left unchanged** (to rewrite a pin you edit `mise.toml` then `mise lock`, or use `mise upgrade --bump`). So for these two tools the bump ritual is: edit `tag:` / version in `mise.toml` → `mise lock` → review diff → `mise install` → run §8 checks.
- **CI:** [.github/workflows/ci.yml:19-21](../../.github/workflows/ci.yml) sets `MISE_EXPERIMENTAL=1`, `MISE_YES=1`, `MISE_AUTO_INSTALL=0`, and each job installs exactly the tools it needs (`mise install --locked cargo-binstall github:nextest-rs/nextest` at ci.yml:69, the hygiene list at ci.yml:85, …). Two consequences: (a) no CI change is needed for this proposal — neither tool is run in CI; (b) if a hygiene/mutation job is *ever* added, its `mise install --locked` line must name the tool explicitly, because auto-install is off.
- **Release-age default:** mise applies a 24 h `minimum_release_age` to fuzzy resolution; locked exact versions are exempt afterwards. Whether it also delays a freshly pinned cargo-git/crates version *before* the first lock is *unverified* (the docs tie it to backends with release timestamps). If `mise lock` refuses today's `tag:v0.1.15`, pin v0.1.14 or wait a day.

### 2.4 Version policy (proposal)

| Tool | Pin style | Bump ritual | Rationale |
| --- | --- | --- | --- |
| messrust | `version = "tag:vX.Y.Z"` | edit tag → `mise lock` | pre-1.0, daily releases, crates.io stale |
| mutarust | `"0.1.10"` (exact) | edit version → `mise lock` | pre-1.0, weekly releases; `latest` would float |
| cargo-mutants | `"latest"` (today) | `mise lock --bump cargo-mutants` | retired in stage 3 anyway |

## 3. Q2 — Task design: TOML vs file, templates, sketches

### 3.1 Decision criteria observed in this repo

- **TOML task in `mise.toml`** when the body is short and the surface is a plain passthrough: `check` (mise.toml:83), `fmt` (:132), `audit*` (:146-170), `release`/`doc` (:228, :244). They use `usage = 'arg "[args]" var=#true …'` + an `eval` one-liner.
- **File task under `.mise/tasks/`** when the body maps several flags to tool arguments and carries helper functions: `lint` (`.mise/tasks/lint`, functions + `load_fix_registry`), `test` (`.mise/tasks/test/_default`, `-m`/`--feature`/`--docs` logic), `mutants` (`.mise/tasks/mutants/_default`, five `build_*_flags` functions).
- File tasks carry metadata in `#MISE` headers and their spec in `#USAGE` comment lines (`.mise/tasks/mutants/_default:1-42`); `task.disable_spec_from_run_scripts = true` ([mise.toml:48](../../mise.toml)) only disables the *deprecated Tera* `flag()/arg()` functions inside `run` scripts — it does not affect `usage = …` on TOML tasks or `#USAGE` on file tasks (vendored `06_arguments.md:555-568`).

### 3.2 Can a file task extend a template? Yes — with constraints

Verified (online [templates docs](https://mise.jdx.dev/tasks/templates.html)
+ this repo):

- File tasks extend via a header line: `#MISE extends="name"` — documented
  explicitly, and proven in-repo twice: `.mise/tasks/mutants/_default:4`
  (`extends="mutants"`) and the bench task (`#MISE extends="bench"`,
  see the comment at mise.toml:272-275).
- **One extends target only.** A file task accepts a single target; the array
  form is silently dropped in mise 2026.9.12 (repo comment, mise.toml:287-291).
  So a task cannot inherit `gate:never-skip` *and* `mutants` at once — it must
  pick one template (or repeat fields locally, as bench does).
- A task with a `file` **never inherits `run`** — the script is the body.
- Merge rules that matter here: `timeout`, `sources`, `outputs`, `cache` are
  local-override-completely (unset local ⇒ inherit); `depends`/`depends_post`
  are local-override-completely and are **not** in any template we use;
  `usage` concatenates (template first, then task) *for TOML tasks* — whether a
  file task's `#USAGE` lines also concatenate with an inherited template spec is
  **unverified** (our templates define no `usage`, so it cannot bite us).
- **`#MISE` headers reject unknown fields** — e.g. `timeout` in a file-task
  header is warned about and dropped (mise.toml:272-275). Safety caps must live
  on the template. `[task_templates.mutants]` already provides `timeout = "1h"`
  (mise.toml:279-282).

### 3.3 Decision: `mess` — TOML task (sketch)

Placement: the *Quality, Coverage & Mutation* section, after `[tasks.crap]`
(mise.toml:217-222). Follows the `check`/`fmt` TOML-gate idiom: extends
`gate:never-skip`, **no `sources`** (this repo's TOML gates declare none; only
file gates and cacheable tasks do — an internal inconsistency that is not ours
to fix here).

```toml
[tasks.mess]
description = "Static mess detection with `messrust` (advisory)"
extends = "gate:never-skip"
usage = 'arg "[args]" var=#true help="Extra arguments forwarded to messrust (e.g. --strict, --only Rule, --reportfile f.json)"'
run = '''
#!/bin/bash
# usage_args is shell-quoted (var=#true arg); eval parses it into the extra
# array.
# shellcheck disable=SC2154
set -euo pipefail
declare -a extra
eval "extra=(${usage_args:-})"
messrust src text messrust.xml --ignore-tests "${extra[@]+"${extra[@]}"}"
'''
```

Decisions embedded here:

- **TOML, not file task:** three flags worth exposing live in *passthrough*
  anyway (`--only`/`--strict`/`--reportfile` change per triage session, per
  messrust §7.3); no flag→argument mapping logic, no helper functions → a file
  task would be ceremony.
- Positional invocation `messrust <paths> <format> <ruleset>` with the
  committed `messrust.xml` policy file at repo root
  (per-tool config convention: `clippy.toml`, `deny.toml`, `tarpaulin.toml`).
  `src` = comma-free single path; `text` = human format on stdout; `--ignore-tests`
  makes it a production-code gate (upstream docs/usage.md:78).
- **Advisory posture:** no `--strict`, no `--reportfile` by default — stdout +
  non-zero exit (2 = findings) is the contract; adopters add flags via
  passthrough (`mise run mess -- --reportfile messrust.json --ignore-violations-on-exit`).
- Rejected alternative: `extends = "mutants"` — wrong timeout (1 h for a
  parse-only tool) and semantically a *hygiene* gate, not a mutation run.

### 3.4 Decision: `mutarust` — file task (sketch)

New file `.mise/tasks/mutarust/_default` (task name `mutarust`), sibling of
`.mise/tasks/mutants/_default`, in the house style of that file and
`.mise/tasks/lint`:

```bash
#!/bin/bash
#MISE description="Run mutation testing with `mutarust`"
#MISE sources=["@group:rust"]
#MISE extends="mutants"
#MISE depends=["test"]
#USAGE flag "-f --file <file>" {
#USAGE   help "Mutate one file only (e.g. -f src/config.rs)"
#USAGE }
#USAGE flag "-m --mod <module>" {
#USAGE   help "Mutate one module only (target src/<module>/)"
#USAGE }
#USAGE // `complete "module"` completes the value after `-m`/`--mod`, not the flag name.
#USAGE complete "module" run="sed -nE 's/^(pub )?mod ([a-z_]+);$/\\2/p' src/lib.rs"
#USAGE flag "--min-msi <n>" help="Fail (exit 4) if total mutation score < n"
#USAGE flag "--update-baseline" help="Accept current escapes into mutarust-baseline.json"
#USAGE flag "--fail-on-escaped" help="Fail (exit 4) only on escapes absent from the baseline"
#USAGE flag "--git-diff" negate="--no-git-diff" default=#false help="Mutate only lines changed vs the default branch (origin/HEAD)"
#USAGE flag "--dry-run" help="Count mutants without writing areas or running tests"
#USAGE arg "[args]" var=#true {
#USAGE   help "Extra arguments forwarded to mutarust"
#USAGE   long_help """
#USAGE   Examples:
#USAGE   - `mise run mutarust -m index` - mutate module `index`
#USAGE   - `mise run mutarust -m index --dry-run` - count only for one module
#USAGE   - `mise run mutarust -- --list-mutators` - list built-in mutators
#USAGE   - `mise run mutarust -- --update-baseline` - accept current escapes
#USAGE   """
#USAGE }

# shellcheck disable=SC2154  # usage_* are injected by mise
set -euo pipefail

declare -a mutarust_args=()

########################################
# Appends policy, execution, and logging flags.
# `--dry-run` is mutually exclusive with timeout controls and Cargo test
# controls (upstream cli.md), so count-only mode omits them.
# Globals:
#   mutarust_args, usage_dry_run
########################################
build_static_flags() {
  mutarust_args+=(--config mutarust.yml --logger-agentic-json)
  if [[ "${usage_dry_run:-false}" != "true" ]]; then
    mutarust_args+=(
      --test-flags "--features test-utils --all-targets"
      --timeout-coefficient 5
    )
  fi
}

########################################
# Maps -f / -m to mutarust TARGET positionals.
# Globals:
#   mutarust_args, usage_file, usage_mod
########################################
build_target_flags() {
  [[ -n "${usage_file:-}" ]] && mutarust_args+=("${usage_file}")
  if [[ -n "${usage_mod:-}" ]]; then
    [[ -d "src/${usage_mod}" ]] || { echo "mutarust: unknown module: ${usage_mod}" >&2; exit 2; }
    mutarust_args+=("src/${usage_mod}")
  fi
}

########################################
# Appends gate and scoping flags.
# Globals:
#   mutarust_args, usage_min_msi, usage_fail_on_escaped,
#   usage_update_baseline, usage_git_diff, usage_dry_run
########################################
build_gate_flags() {
  [[ -n "${usage_min_msi:-}" ]] && mutarust_args+=(--min-msi "${usage_min_msi}")
  [[ "${usage_fail_on_escaped:-false}" == "true" ]] && mutarust_args+=(--fail-on-escaped)
  [[ "${usage_update_baseline:-false}" == "true" ]] && mutarust_args+=(--update-baseline)
  [[ "${usage_git_diff:-false}" == "true" ]] && mutarust_args+=(--git-diff-lines)
  [[ "${usage_dry_run:-false}" == "true" ]] && mutarust_args+=(--dry-run)
}

########################################
# Evaluates and parses variadic usage_args tokens into mutarust_args.
########################################
parse_passthrough_tokens() {
  eval "mutarust_args+=(${usage_args:-})"
}

main() {
  build_static_flags
  build_target_flags
  build_gate_flags
  parse_passthrough_tokens

  mutarust "${mutarust_args[@]}"
}

main "$@"
```

Decisions embedded here:

- **File task, not TOML:** five declared flags plus four conditional groups and
  a `-m` → directory mapping — the exact complexity band where this repo uses
  file tasks (`mutants`, `test`, `lint`).
- **`extends = "mutants"`** — shares the 1 h safety timeout, `outputs = []`,
  `cache = { enabled = false }` with cargo-mutants' task while both coexist
  (single-slot constraint §3.2 means the *never-skip payload* comes along with
  the timeout; there is no need for `gate:never-skip` on top).
- **`depends = ["test"]`** — mirrors `.mise/tasks/mutants/_default:5`
  (clean-suite-first habit; mutarust runs the clean suite itself, this is an
  early-failure belt-and-braces).
- **`--test-flags "--features test-utils --all-targets"`** — the ported form
  of `features` + `additional_cargo_test_args` (`.cargo/mutants.toml:5-6`).
- **`--timeout-coefficient 5`** — port of `timeout_multiplier = 5`
  (`.cargo/mutants.toml:31`). Note the floor changes: cargo-mutants
  `minimum_test_timeout = 30` s has no mutarust equivalent (its floor is 1 s ×
  longest clean test) — see risk R3.
- **`--dry-run` must be a declared flag, not passthrough:** because
  `--test-flags`/`--timeout-coefficient` are statically present, a raw
  passthrough `--dry-run` would make mutarust *reject its own command line*
  (mutual exclusions, cli.md:260-263). The body drops those static flags only
  when the declared flag is used. Documented in the task's `long_help`.
- **`--logger-agentic-json` always on** — replaces the report task (§5.5).
- **No `depends_post`** — unlike cargo-mutants' `mutants:report`, there is
  nothing to post-process (§5.5).
- Rejected: giving this task a `--exclude <glob>` flag to mirror cargo-mutants'
  `--exclude` — mutarust has no glob target; `-f`/`-m` cover the same daily
  use, and `exclude_dirs` lives in `mutarust.yml`.

### 3.5 Template choice and sharing during coexistence

| Task | Extends | Inherits | Why |
| --- | --- | --- | --- |
| `mess` (TOML) | `gate:never-skip` | `outputs = []`, cache off | every repo gate does this (check/fmt/audit/verify) |
| `mutarust` (file) | `mutants` | 1 h timeout + same never-skip payload | shared mutation budget; proven pattern |
| `mutants` (file, until stage 3) | `mutants` | unchanged | coexistence |
| `bench` (file) | `bench` | 20 m timeout, declares never-skip pair locally | untouched precedent for the single-slot rule |

Optional tidy-up at retirement (stage 3): rename
`[task_templates.mutants]` → `[task_templates.mutation]` and update the one
`#MISE extends=` line in the mutarust task. Low value; do it only if the old
name would confuse (`mise tasks info mutarust` prints the inherited fields either way).

## 4. Q3 — Caching decisions

### 4.1 Mechanics (verified)

From [mise.toml](../../mise.toml) and the vendored task docs:

- Project default: `[task_config.cache] enabled = true` (mise.toml:58-61) — but
  a task is only *freshness-skippable* if it declares output paths; **`outputs = []` is the explicit "no files" declaration that blocks the skip**, and `cache = { enabled = false }` blocks artifact-cache replay (repo comment, mise.toml:284-291; online templates docs, "Empty lists and file tasks").
- `global_inputs = ["mise.toml", "Cargo.lock", "clippy.toml"]` (mise.toml:56)
  are added to every task's inputs; `input_groups` `rust` =
  `["Cargo.toml", "src/**/*.rs", "tests/**/*.rs"]` (mise.toml:63-67).
- Per-run overrides exist: `mise run --task-cache off|read-only|…`,
  `--force` (bypass freshness), `--task-cache-explain-json` (show cache-key
  inputs) — [mise run CLI](https://mise.jdx.dev/cli/run.html).

### 4.2 Decisions

| Task | sources | outputs | cache | timeout | Verdict |
| --- | --- | --- | --- | --- | --- |
| `mess` | *(none)* | inherited `[]` | inherited off | none | **Never skip.** via `gate:never-skip`; parse-only runtime makes skipping pointless, and gates in this repo always run. |
| `mutarust` | `@group:rust` | inherited `[]` | inherited off | inherited `1h` | **Never skip + no artifact cache**, inherited from `[task_templates.mutants]`. |
| `mutants` (interim) | `@group:rust` | `[]` | off | `1h` | unchanged. |

Why `mutarust` must not be cached even though a successful mutation run looks
like "outputs for inputs":

1. Its real outputs (`report.json`, `mutarust-agentic.json`, …) are written to
   the CWD and are **not declared** — an artifact cache keyed on undeclared
   outputs would replay stale logs over fresh files, or worse, restore nothing
   while claiming a hit.
2. Its policy inputs are **not in any input group**: `mutarust.yml` and
   `mutarust-baseline.json` are neither `global_inputs` nor `@group:rust`.
   Editing the baseline would not invalidate a cached run. (Same argument for
   `messrust.xml` vs `mess` — if `mess` were ever made cacheable, its sources
   must become `["@group:rust", "messrust.xml"]`, mirroring how `coverage`
   declares `tarpaulin.toml` beside the group, mise.toml:181.)
3. Wall-clock timeouts and worker scheduling make runs inherently
   non-deterministic in *duration*, and a mutation result without the tool's
   own exit code (score gates) is not the deliverable — the exit code is.

Never-skip also sidesteps input-group incompleteness: with cache off and
`outputs = []`, a missing input declaration has no failure mode.

### 4.3 Considered and rejected

- **Cache `mess`** (deterministic parse, seconds): correct *in principle* only
  after adding `messrust.xml` to sources; rejected because every other gate
  here is `gate:never-skip`, and a gate that can be silently served from cache
  is a gate an agent can mistake for having run. Keep it uniform; revisit only
  if `mess` ever graduates into `hk` quality (then re-derive, since hk has its
  own freshness model).
- **Declared `outputs` for reports** (`outputs = ["report.json"]`): rejected —
  flags change which files exist (`--dry-run` writes none; `--json-out` renames
  them), so any declared list would be a lie some runs.
- **Freshness-skip `mutants`/`mutarust` via real outputs:** rejected outright;
  the shared template exists precisely to prevent it (mise.toml:279-294).

## 5. Q4 — Staged migration from cargo-mutants

### 5.1 Stages

| Stage | Goal | Touches | Gate on |
| --- | --- | --- | --- |
| **0 — messrust** (independent) | install + policy + advisory task | `[tools]` line, `mise.lock`, `messrust.xml`, `[tasks.mess]` | first run reviewed; policy quiet enough |
| **1 — coexistence pilot** | mutarust beside cargo-mutants, same codebase | `[tools]` line, `mise.lock`, `mutarust.yml`, `.mise/tasks/mutarust/_default`, `.gitignore` | A/B on 1–2 modules: same escapes found? runtime comparable? |
| **2 — baseline** | accept the score reality | `mutarust-baseline.json` (committed), `mutarust.yml` tweaks, optional `min_msi` | full scoped run triaged; `--fail-on-escaped` green |
| **3 — cutover** | retire cargo-mutants | remove tools line + lock entry, `.mise/tasks/mutants/*`, `.cargo/mutants.toml`, `[profile.mutants]`, `mutants.out` gitignore lines, rewrite `mutation_testing.md` §2-§6 | mutarust handles every daily workflow (`-m`, `-f`, `--dry-run`, baseline) |
| **4 — optional** (explicitly out of scope now) | CI job (`--git-diff-lines`) / hk quality step | `ci.yml`, `hk.pkl` | only if the team ever wants it |

Stages 0 and 1 are independent (messrust has nothing to do with mutation) and
can land in either order or in parallel.

### 5.2 Config port table: `.cargo/mutants.toml` → `mutarust.yml` / CLI

Carried from `quality_gates_mutarust.md` §8.2, with the task sketch's flags
wired in:

| `.cargo/mutants.toml` | mutarust equivalent | Verdict |
| --- | --- | --- |
| `test_tool = "nextest"` | `--exec "…"` (exit 0=kill/1=escape/2=skip, **serial**) | ~ see §5.6 — pilot on default `cargo test` first |
| `features = ["test-utils"]` + `additional_cargo_test_args = ["--all-targets"]` | `--test-flags "--features test-utils --all-targets"` (in task) | ✓ |
| `exclude_globs = ["src/cli/**/*.rs"]` | `exclude_dirs: ["src/cli"]` in `mutarust.yml` | ✓ |
| `exclude_globs = ["src/main.rs", "src/**/error.rs"]` | none — `exclude_dirs` is directory-only | ✗ gap: pass explicit file targets, add `// mutator-disable-func` annotations, or accept the mutants |
| `exclude_re = ["fn main", "impl.*Debug", …]` (10 patterns) | `ignore_source_lines` (line-regex) / in-source annotations | ~ partial: regexes on `impl.*Debug` header lines skip nothing (bodies still mutate); real port = per-fn annotations |
| `timeout_multiplier = 5`, `minimum_test_timeout = 30` | `--timeout-coefficient 5` (in task), floor ≈ 1 s × longest clean test | ✓ closest; floor semantics differ (R3) |
| cargo-mutants `--baseline run` / `--iterate` | baseline file + `--fail-on-escaped`; no `--iterate` | ~ different mechanism (arguably better: committed `mutarust-baseline.json`) |
| cargo-mutants `--profile mutants`, `--cap-lints true` | none | ✗ no equivalent |
| cargo-mutants `--check` (build-only) | none (`--no-exec` writes areas without tests but does not prove they compile; `--dry-run` only counts) | ✗ does not port |

### 5.3 Task UX port table: `mutants` USAGE flags → `mutarust`

| `mise run mutants …` | `mise run mutarust …` | Note |
| --- | --- | --- |
| `-f <file>` | `-f <file>` | same spelling, positional target instead of `--file` |
| `-m <mod>` (→ `src/<mod>/**/*.rs`) | `-m <mod>` (→ `src/<mod>/`) | completion snippet reused verbatim |
| `--exclude <glob>` | — | no glob targets (use `exclude_dirs` / explicit targets) |
| `--check` | — | no build-only mode (5.2) |
| `--iterate` (default on) | `--fail-on-escaped` + baseline | different mental model |
| `--timeout <secs>` | `--timeout <secs>` (passthrough; cannot combine with `--timeout-coefficient`) | conflict: static coefficient is in the task — to use fixed timeouts, the task would need the same conditional treatment as `--dry-run` (**not** sketched; passthrough `--timeout` + static coefficient = rejected command line, R6) |
| `--no-config` | `--config <other.yml>` | pass `-- --config /dev/null`-style only if ever needed |
| `-L --level` | passthrough (`-v`/log level flags per cli.md) | |
| `-v/--caught`, `-V/--unviable`, `--all-logs` | passthrough (`--output-statuses`, `--quiet`, `--silent`) | different vocabulary |
| `--json` | always-on: `report.json` (yaml `json_output: true`) + `mutarust-agentic.json` | richer |
| `-- --list` | `-- --dry-run` / `-- --list-mutators` / `-- --list-files` | |

### 5.4 What does not port (summary)

`--check`, `--iterate`, glob `--exclude`, `exclude_re` regex semantics,
`--profile`/`--cap-lints`, `minimum_test_timeout = 30` floor, nextest-as-default
runner (until/unless §5.6 lands), and the `mutants.out/` artifact tree.

### 5.5 Report-task replacement

Today: `depends_post = ["mutants:report"]` (`.mise/tasks/mutants/_default:6`)
runs a hidden task (`.mise/tasks/mutants/report`, `#MISE hide=true`) that
converts `mutants.out/missed.txt` into `mutants-report.md` with a hand-written
"Instructions for Next Agent Session" block.

Replacement (mutarust writes its own):

- `json_output: true` in `mutarust.yml` → `report.json` (full report; schema in
  upstream `docs/json-outputs.md`).
- `--logger-agentic-json` (in the task) → `mutarust-agentic.json`, which is a
  strict superset of the retired markdown report: per-escaped-mutant `diff`,
  `context_lines`, `test_files`, `kill_hint`, plus a `reminder` about how to
  write kill tests, and a published JSON Schema
  ([schema/agentic.schema.json](https://github.com/quality-gates/mutarust/blob/main/schema/agentic.schema.json)).
- Optional: `mutarust-summary.json` (compact), `mutarust-report.html`
  (`html_output`), `mutarust-gitlab.json` / GitHub warnings for CI later.

Decision: **no replacement `depends_post` task.** The report is a side effect
of the run, not a derived artifact needing its own task. When stage 3 lands,
delete `.mise/tasks/mutants/report` and the `mutants-report.md` gitignore line.

### 5.6 `--exec` + nextest viability

Current setup runs cargo-mutants with `test_tool = "nextest"`
(`.cargo/mutants.toml:4`), i.e. cargo-mutants invokes nextest per mutant in
parallel. mutarust's `--exec` is **not** the same machine:

- **Exit-code contract is inverted** (cli.md:284-285): the `--exec` command
  must exit **0 = mutant killed (test failed), 1 = escaped (tests passed), 2 =
  skipped**, anything else = errored. A bare `cargo nextest run` exits 0 when
  tests pass and non-zero when they fail, so raw would classify *every* mutant
  backwards — it needs a small wrapper script (e.g.
  `scripts/mutarust-nextest.sh`) that flips statuses.
- **Serial execution:** `--exec` runs one worker — a large wall-clock
  regression vs cargo-mutants' parallel nextest (and vs mutarust's own default
  parallel `cargo test` workers).
- **Mutual exclusions:** `--exec` cannot be combined with `--test-flags` or
  `--timeout-coefficient` (cli.md:246-253, 260-263) — the two static flags in
  the task sketch would have to move *inside* the exec command, i.e. a
  different task profile entirely.

**Proposal:** pilot on mutarust's default runner (parallel `cargo test` with
`--test-flags`), collect wall-clock and result-state deltas on one module, and
treat "nextest via `--exec`" as its own follow-up decision (wrapper script +
task profile + benchmark) rather than part of this migration. *Everything in
this subsection is unexecuted — including whether mutarust's default is fast
enough here.*

### 5.7 Artifact ledger

**Added**

| Artifact | What |
| --- | --- |
| `mise.toml` `[tools]` | messrust git line + `cargo:mutarust` line (§2.1) |
| `mise.lock` | two version-only entries (`mise lock`) |
| `messrust.xml` (root) | repo policy = `rust` preset minus dupes/FP rules + `explicitness` (messrust §7.2 sketch) |
| `mutarust.yml` (root) | `json_output: true`, `exclude_dirs: [src/cli]`, no score gate yet (mutarust §8.2 sketch) |
| `mise.toml` `[tasks.mess]` | §3.3 |
| `.mise/tasks/mutarust/_default` | §3.4 |
| `.gitignore` | `/report.json`, `/mutarust-agentic.json`, `/mutarust-summary.json`, `/mutarust-report.html`, `/mutarust-gitlab.json` |
| `mutarust-baseline.json` | created at stage 2 and **committed** (accepted-escape policy, analogous to `.cargo/mutants.toml`) |
| this document | `docs/refs/quality_gates_mise_adoption.md` |

**Changed**

| Artifact | What |
| --- | --- |
| `docs/refs/mutation_testing.md` | stage 3: §2 tooling, §3 config, §5 usage/flags rewritten around mutarust; §4 CI stays "none" |
| `docs/refs/quality_gates_*.md` | optional back-link here from both notes |

**Retired (stage 3)**

| Artifact | Replaced by |
| --- | --- |
| `"cargo:cargo-mutants"` + lock entry (mise.toml:30, mise.lock:107) | `cargo:mutarust` |
| `.mise/tasks/mutants/_default`, `.mise/tasks/mutants/report` | `.mise/tasks/mutarust/_default` + self-written reports |
| `.cargo/mutants.toml` (all 32 lines) | `mutarust.yml` + task flags (§5.2) |
| `Cargo.toml:264 [profile.mutants]` | nothing (mutarust uses per-worker target dirs) |
| `.gitignore:20-22` (`/mutants.out/`, `/mutants.out.old/`, `/mutants-report.md`) | new mutarust report lines |
| `mutants` task name / `depends_post` report pipeline | `mutarust` task, no post step |
| (optional) template name `[task_templates.mutants]` | `[task_templates.mutation]` |

**Unchanged on purpose:** `github:nextest-rs/nextest` (the `test` task still
needs it), `hk.pkl`, `verify`, `ci.yml`, `[task_templates.bench]`,
`gate:never-skip`.

## 6. Q5 — Human + agent task design

### 6.1 Help and discovery

- **Specs are the interface.** `mise run mess --help` and
  `mise run mutarust --help` render the `usage`/`#USAGE` specs (flags,
  long_help, examples) — note mise flags come *before* the task name, task
  flags after (`mise run mutarust --dry-run` reaches the task;
  `mise run --dry-run mutarust` is mise's own dry-run, which just prints the
  plan — do not confuse the two).
- **Shell completions** come free from the same specs, including the dynamic
  `complete "module"` (sed over `src/lib.rs`) shared by `-m` on both mutation
  tasks.
- **Discovery for agents:** `mise tasks` (table, `--name-only` for piping),
  `mise tasks -J` (JSON), `mise tasks info mutarust` (— critically —
  shows the task **after** template inheritance: timeout, outputs, cache),
  `mise tasks validate --json`, `mise tasks deps mutarust`. Source of these:
  [mise tasks CLI](https://mise.jdx.dev/cli/tasks.html),
  [templates docs](https://mise.jdx.dev/tasks/templates.html).
- `mise generate task-docs` exists if machine-readable task docs are wanted
  (vendored `03_running_tasks.md:79`).

### 6.2 Exit-code contracts (put them in `long_help` too)

| Tool | 0 | 1 | 2 | 3 | 4 |
| --- | --- | --- | --- | --- | --- |
| messrust (usage.md:94-98) | clean | operational/config error | **findings** | — | — |
| mutarust (parity.md:110-121) | pass | generic error | bash completion | source/config/annotation error | **quality-gate failure** (`min_msi`, `--fail-on-escaped`) |

For agents the sharp edges are: messrust **2 = findings, not crash**; mutarust
**4 = gate red, 3 = your config/annotation is broken, 1 = real failure**, and
mutarust's completion mode exits 2 (never triggered by these tasks, but it
means "2" is overloaded across the two tools). Scripts should branch on the
tool, not on `!= 0`.

### 6.3 Output handling and machine triage

- **Human/streaming:** default prefix mode is fine; `mise run --raw mutarust …`
  switches to direct stdio (no line-prefixing, **no redactions**, forces
  `--jobs=1`) — right for interactive tailing and for feeding raw tool output
  to a parser; `-o interleave` is the lighter-weight alternative. Whether
  `--raw` also changes task-cache interaction is *unverified* — irrelevant
  here, since both tasks are `cache = { enabled = false }` (§4).
- **Machine triage:** prefer files over scraped stdout — `report.json`,
  `mutarust-agentic.json` (escaped mutants with diffs + kill hints),
  `mutarust-summary.json`; messrust via `--reportfile messrust.json`.
  This is the standing rule for this repo: structured artifacts, not log
  parsing.
- **Scoped runs are the default cost control:** `mise run mutarust --dry-run`
  → `-m <mod>` / `-f <file>` → `--git-diff` for changed-lines-only runs;
  `mise run mess -- --only CyclomaticComplexity` to bisect noise.
- **Coexistence during stages 1–2:** both mutation tasks share the `mutants`
  template, so both carry the 1 h timeout; running both in one session
  (`mise run mutarust ::: mutants`) is possible but doubles cost — pilot with
  A/B *sequentially* instead.

### 6.4 Footguns to document (in task `long_help` / docs)

1. **Declare flags, don't passthrough the exclusivity-sensitive ones**
   (`--dry-run`, later `--timeout`) — the task omits conflicting static flags
   only when it sees its own flag (§3.4). Passthrough `--dry-run` → mutarust
   rejects the command line (exit 1/3 class, unverified which).
2. **`--` before unrecognized extras** (`mise run mutarust -- --list-mutators`)
   is the established idiom (`.mise/tasks/mutants/_default:36-41`) — whether
   the usage parser *hard-rejects* undeclared flags without `--` is
   *unverified*; assume yes.
3. **`mise run -n/--dry-run`** (mise's) prints the execution plan and runs
   nothing — never use it as a synonym for mutarust's `--dry-run`.
4. **`mise run --force`** only bypasses freshness; it does not bypass
   `cache = { enabled = false }` semantics — these gates already always run.
5. **Not hidden:** neither new task sets `hide=true` (only `mutants:report`
   does) — both must appear in `mise tasks`.

## 7. Risks and open questions (ranked)

| # | Risk | Severity | Mitigation / decision needed |
| --- | --- | --- | --- |
| R1 | **Nothing executed.** mutarust 0.1.10 was published *today*; messrust v0.1.15 too. All behavior above is docs-derived. | high | §8 checklist before trusting any of it; pin, then `mise install` and smoke-test |
| R2 | **`--exec` nextest port** — inverted exits, serial execution, flag exclusions (§5.6) | high if pursued | don't pursue in this migration; wrapper + benchmark as separate decision |
| R3 | **Timeout semantics shift**: 30 s floor (cargo-mutants) → ~1 s × longest clean test × 5 (mutarust). Slow-but-innocent tests may now time out mutants (`Errored`) or the reverse. | medium | compare result-state counts on the A/B module; tune coefficient |
| R4 | **`exclude_re` has no true port** — the 10 `impl.*Debug/Display/…` patterns will start producing mutants → lower score, more noise (§5.2) | medium | decide: in-source `// mutator-disable-func` annotations (touches source) or accept noise and let baseline absorb it |
| R5 | **`exclude_globs` gap** (`src/main.rs`, `src/**/error.rs`) — `exclude_dirs` is directory-only | medium | explicit targets, annotations, or accept; decide at stage 1 |
| R6 | **Static `--timeout-coefficient` blocks passthrough `--timeout`** (mutual exclusion, cli.md:246-253) — same class as the `--dry-run` issue but *not* handled in the sketch | low/medium | either extend the conditional-flag pattern or document "fixed timeouts unsupported by the task" |
| R7 | **Version-only lock entries** for cargo backend: git tags/crates versions have no checksum in `mise.lock`; a retag is not detected (§2.3) | low | accept (upstream releases immutable) or verify tags out-of-band on bump |
| R8 | **Usage-parser strictness** (undeclared flags without `--`) and **file-task + inherited-template `usage` concat** are unverified | low | our templates define no `usage`; verify `--help` output in §8 |
| R9 | **messrust churn** (12+ releases in weeks; policy rules still moving, open FP bugs #160/#183/#184) | low | pinned tag; keep `mess` advisory until quiet (already the plan) |
| R10 | **`minimum_release_age` 24 h default** may briefly refuse brand-new pins | low | wait or pin the previous version (§2.3) |
| R11 | Double mutation cost during coexistence (two tools, both 1 h) | low | run them in separate sessions; stage 1 is time-boxed |

## 8. Verification checklist (before any of this is believed)

Nothing below has been run. In implementation order:

```sh
# install & lock
mise lock                                # expect two new version-only entries
mise install                             # installs everything in [tools], incl. the two new pins
mise which messrust && messrust --version
mise which mutarust && mutarust --version

# task wiring
mise tasks | grep -E 'mess|mutarust'     # both listed, not hidden
mise tasks info mess                     # extends gate:never-skip visible
mise tasks info mutarust                 # timeout 1h, outputs [], cache off inherited
mise run mess --help                     # usage spec renders; unknown-flag behavior noted
mise run mutarust --help

# smoke (behavioral)
mise run mess                            # expect exit 2 while findings exist
mise run mess -- --reportfile /tmp/m.json --ignore-violations-on-exit   # expect 0
mise run mutarust --dry-run              # must NOT be rejected despite static flags
mise run mutarust -m <some-module>       # exit 0/4, report.json + mutarust-agentic.json written
mise run mutarust -- --list-mutators

# exit-code propagation (docs say task status is the tool's; confirm)
mise run mess; echo $?                   # expect 2 with findings

# migration gates
mise run mutants -m <same-module>        # A/B: same escapes, comparable runtime (cargo-mutants via its task)
mise run mutarust --update-baseline      # stage 2, after triage
```

If any check contradicts a claim above, correct this document — it is a
proposal, not a record of observed behavior.

## 9. Primary sources

- This repo: `mise.toml` (tools/settings/task_config/templates lines cited
  inline), `mise.lock`, `.mise/tasks/{lint,test/_default,mutants/_default,mutants/report}`,
  `.cargo/mutants.toml`, `.gitignore`, `Cargo.toml` (`[profile.mutants]`),
  `hk.pkl`, `.github/workflows/ci.yml`, `docs/refs/mutation_testing.md`,
  `docs/refs/quality_gates_messrust.md` (§4, §7),
  `docs/refs/quality_gates_mutarust.md` (§4, §8).
- Upstream tool docs (vendored): `docs/refs/messrust/docs/usage.md`,
  `docs/refs/mutarust/docs/{cli.md,config.md,json-outputs.md,parity.md}`.
- mise docs — vendored under `docs/refs/mise_tasks/` (03 running tasks,
  04 TOML tasks, 05 file tasks, 06 arguments, 09 templates) and, checked live
  2026-09-26, online:
  [cargo backend](https://mise.jdx.dev/dev-tools/backends/cargo.html),
  [github backend](https://mise.jdx.dev/dev-tools/backends/github.html),
  [mise.lock](https://mise.jdx.dev/dev-tools/mise-lock.html),
  [task templates](https://mise.jdx.dev/tasks/templates.html),
  [mise run](https://mise.jdx.dev/cli/run.html),
  [mise tasks](https://mise.jdx.dev/cli/tasks.html).
- usage spec: [scripts](https://usage.jdx.dev/cli/scripts),
  [flagset](https://usage.jdx.dev/spec/reference/flagset) (flagsets/`use`/`include`
  exist in usage's KDL spec; whether mise's `#USAGE` comment form accepts them
  is unverified — noted only as a possible dedup of the shared `-f`/`-m`
  declarations during coexistence).
- Registries: crates.io API for `mutarust` (max 0.1.10, 2026-09-26) and
  `messrust`; GitHub releases API for
  [messrust v0.1.15](https://github.com/quality-gates/messrust/releases/tag/v0.1.15)
  (darwin-only assets confirmed).

---

## 10. Deep dive — full capability parity for `.mise/tasks/mutants/` (plan revision)

§5 mapped `.cargo/mutants.toml` but treated the task layer too lightly. This
section enumerates **everything** `.mise/tasks/mutants/_default` and
`.mise/tasks/mutants/report` actually do — every declared flag, every static
behavior, the hidden report contract — and revises the migration design so no
task capability is silently lost. Sources: both task files (read in full),
`Cargo.toml:264-267` (`[profile.mutants]`), `.gitignore:20-22`,
`docs/refs/mutation_testing.md`, repo-wide grep for task/report consumers, and
the vendored mutarust docs (`cli.md`, `config.md`, `json-outputs.md`).

### 10.1 Full surface of today's tasks

**`_default` (177 lines)** — four layers:

| Layer | What it does |
| --- | --- |
| Header | `sources=@group:rust`, `extends="mutants"` (1 h, `outputs=[]`, cache off), `depends=["test"]`, `depends_post=["mutants:report"]` |
| Static flags (built by `build_static_flags`) | `--baseline run` (clean-suite first), `--cap-lints true`, `--build-timeout-multiplier 3`, `--profile mutants` |
| Declared UX (14 flags + passthrough) | `-f/--file`, `-m/--mod` (+ dynamic `complete`), `--exclude <glob>`, `--check`, `--iterate` (**default on**), `--timeout`, `--no-config`, `-L/--level`, `-v/--caught`, `-V/--unviable`, `--all-logs`, `--json`, variadic `arg [args]` with long_help examples |
| Execution | assembles args → `cargo mutants "${mutants_args[@]}"`, `set -euo pipefail`, shellcheck-guarded `eval` of `usage_args` |

**`report` (41 lines, `hide=true`)** — a *post-run* contract:
- `-i` (default `mutants.out/missed.txt`) → `-o` (default `mutants-report.md`).
- Writes a Markdown file with an **"Instructions for Next Agent Session"**
  block (read list → write kill tests → verify with `cargo mutants --file`),
  plus an explicit **empty-state template** when no survivors exist.
- Runs via `depends_post` after every `_default` invocation.

**Ecosystem around them:** `"cargo:cargo-mutants" = "latest"` (mise.toml:30),
`[task_templates.mutants]` (mise.toml:279), `[profile.mutants]`
(`inherits = "test"`, `opt-level = 1`, `debug = false` — Cargo.toml:264-267),
`.gitignore:20-22` (`/mutants.out/`, `/mutants.out.old/`, `/mutants-report.md`),
and the doc `docs/refs/mutation_testing.md` (whose §3.2 already shows a stale
`[tasks.mutants]` TOML snippet — the real task is the file task — plus §5
usage examples and §7 parallelism guidance).

**Consumer scan (repo-wide):** nothing invokes `mutants`/`mutants:report`
outside the task tree — no hk step, no CI job, no AGENTS/CLAUDE/CONTEXT
reference, no `.scratch` issue (the `.scratch` "mutation" hits are the
task-system's *data-model* mutation operations — unrelated). The only readers
of `mutants-report.md` are `.gitignore` and `mutation_testing.md`. **The tasks
are a purely manual entrypoint** — which means migration risk is low but also
that nothing forces the rewrite of `mutation_testing.md` to happen; it must be
an explicit stage-3 task.

### 10.2 Capability parity matrix (every task feature → mutarust)

| # | `_default` capability | mutarust equivalent | Verdict / mitigation |
| --- | --- | --- | --- |
| 1 | `--baseline run` (clean suite first) | mutarust **always** runs the clean suite and aborts on failure | ✓ free (unconditional — no way to skip, none needed) |
| 2 | `--cap-lints true` | **no flag, no RUSTFLAGS control** (grep: zero hits in vendored docs) | ✗ **highest-risk gap.** This repo denies warning groups in `[lints]`; cargo-mutants' own DESIGN.md explains mutants that ignore params trip `unused_*` lints and would fail the build for the wrong reason. Without cap-lints those mutants become compile-fails → mutarust counts them **Skipped, which inflates the score** (score = killed+errored+skipped / total). Mitigation: A/B experiment — set `env = { RUSTFLAGS = "--cap-lints=allow" }` on the mise task (mutarust shells out to Cargo and should inherit env; unverified, and it also affects `depends=["test"]` + invalidates build caches). Default: **do not enable until verified** |
| 3 | `--build-timeout-multiplier 3` | no build timeout at all (only `--exec-timeout` for *test* runs) | ✗ gap; a hung compiler hangs the run until the template's 1 h kill. Accept (local-only) + document |
| 4 | `--profile mutants` (`opt-level=1`, `debug=false`) | no `--profile` flag, **but** `--test-flags` is appended to "every Cargo compile **and test** command" (cli.md) | ~ **portable candidate**: add `--profile mutants` to the task's `--test-flags` string and **retain `[profile.mutants]`** (correcting §5.7, which retires it). Unverified — mutarust may reject unknown cargo args or already pass its own; check in §8. If it fails: mutants build at dev `opt-level = 0` → different compile/test perf, results unaffected |
| 5 | `-f/--file` | positional file target | ✓ same spelling (§5.3) |
| 6 | `-m/--mod` (+ `sed` completion over `src/lib.rs`) | positional dir target `src/<mod>` | ✓ completion snippet reused verbatim |
| 7 | `--exclude <glob>` (declared flag) | no glob targets; `exclude_dirs` is config-only, directory-only | ✗ **decision required**: (a) drop the flag (prior sketch) — daily use covered by `-f`/`-m`/`exclude_dirs`; (b) map repeated `--exclude` to explicit *negative* targets — impossible (targets select, never exclude). Recommendation: **drop**, but say so in `long_help` so muscle memory has an answer |
| 8 | `--check` (build-only: mutants compile, no tests) | nothing. `--dry-run` only counts; `--no-exec` writes areas **without compiling them** | ✗ real loss. Options: (a) drop the workflow (no repo consumer found — §10.1); (b) keep cargo-mutants installed *only* for `--check` (undermines cutover); (c) hand workflow: `--no-exec` + `cargo check` inside printed area paths (clunky, areas are in `$TMPDIR`). Recommendation: **drop**, re-evaluate only if a use appears; this is the one UX with no path forward |
| 9 | `--iterate` (**default on**) | none — baseline exempts escapes from `--fail-on-escaped` but **they still run**; only `--blacklist` skips execution (checksums) | ✗ philosophy change, not just a flag: cargo-mutants re-tests only prior escapes across runs; mutarust re-runs the full scope every time. **Do not** approximate `--iterate` by blacklisting *killed* mutants — that would silently stop detecting test-suite regressions (the tool's purpose). Runtime control becomes **scoping** (`-m`/`-f`/`--git-diff`) instead of incremental reuse. Measure in stage-1 A/B (R3-adjacent) |
| 10 | `--timeout <secs>` | `--timeout` exists but is **mutually exclusive** with the task's static `--timeout-coefficient` (cli.md) | ~ **fix the sketch**: apply the same conditional pattern already designed for `--dry-run` — emit `--timeout-coefficient 5` only when no `--timeout` token is present in `usage_args`. (§5.3/R6 listed this; §3.4 did not implement it — this is a concrete revision) |
| 11 | `--no-config` | config only arrives via `--config`, so "no config" = don't pass it — but the task always does | ~ passthrough `--config <other.yml>`; **duplicate `--config` behavior (task's + user's) unverified** → checklist item. `--config /dev/null` as bypass is *unverified* (empty-YAML parse) |
| 12 | `-L --level` (trace…error) | `--verbose` / `--debug` (two levels, different content) | ~ drop the flag; document mapping in `long_help` (passthrough `-v`/`--debug` still reach mutarust) |
| 13 | `-v/--caught`, `-V/--unviable` | mutarust prints **one line per mutant by default** — killed and skipped states already visible | ✓ obsolete-by-default; keep accepted as passthrough no-ops or drop — recommend drop + note `--output-statuses` for *filtering* (inverse direction) |
| 14 | `--all-logs` (dump all Cargo output) | `--debug` prints file/line/worker/mutator/test-command; per-mutant Cargo output on failure is part of result lines (unverified how much) | ~ partial; verify with a deliberately erroring mutant in §8 |
| 15 | `--json` | `report.json` always-on (`json_output: true`) + agentic/summary loggers | ✓ richer |
| 16 | `-- --list` (passthrough examples) | `--dry-run`, `--list-files`, `--list-mutators`, `--print-ast`, `--run-mutant-id` | ✓ more modes; update the `long_help` examples (§3.4 already does) |
| 17 | `depends=["test"]` | kept as-is | ✓ (belt-and-braces; mutarust re-checks anyway) |
| 18 | `depends_post=["mutants:report"]` | see §10.3 | replaced, not ported |
| 19 | `extends="mutants"` (1 h, never-skip) | same template in new task | ✓; optional rename to `mutation` at stage 3 (§3.5) |
| 20 | passthrough `--jobs 4` (documented in `mutation_testing.md:134`) | `--workers N` (mutarust) + internal `-j` cap so workers×jobs ≈ CPU | ~ different knob; `--jobs` passthrough would hit Cargo *twice-multiplied* — document `--workers` instead |

### 10.3 The `report` task — thorough replacement

**What is actually lost:** not the file, but three behaviors — (a) *always*
runs post-task (via `depends_post`), (b) an explicit **empty-state** artifact
("No surviving mutants / Next Action: None") so an agent reading the repo
post-run is never confused by a missing file, (c) a Markdown artifact at a
stable path with a prose contract for the next session.

**Replacement design:**

1. **Agents:** `mutarust-agentic.json` (always-on in the task) strictly
   supersedes the prose block — per-escape `diff`, `context_lines`,
   `test_files`, `kill_hint`, `reminder`, published schema. The old
   instructions ("add targeted assertions… verify with `cargo mutants --file`")
   become mutarust's own `reminder` + `--run-mutant-id ID` verification loop.
2. **Empty state:** an agentic/report file with `escaped_count: 0` and
   `mutants: []` (json-outputs.md documents the empty-run form) — equally
   unambiguous as the Markdown template; **verify the file is actually
   written on empty runs** (checklist §10.6).
3. **Humans:** stdout table by default; **enable `html_output: true` in
   `mutarust.yml`** (revises §8.2 sketch) so manual runs leave a browsable
   `mutarust-report.html`. Gitignore already lists it (§5.7).
4. **Markdown file: retire — do not replace.** Repo-wide consumer scan
   (§10.1) found only `.gitignore` and the doc. A thin `mutarust:report`
   renderer (jq → md) is justified only if a future PR-comment workflow wants
   Markdown; not now. **Delete `.mise/tasks/mutants/report` at stage 3 and
   drop its gitignore line**, as §5.7 says.
5. **Critical sequencing question (HIGH priority, unverified):** does
   mutarust write `report.json`/agentic output **when the run fails a gate
   (exit 4)**? `config.md:69` says "after a completed run"; `config.md:64`
   says the score policy is checked "after a normal mutation run" — order
   unspecified. If reports are skipped on exit 4, the artifact is missing
   precisely when agents most need it. §8 checklist must test this first;
   fallback = run ungated (`min_msi` unset) and gate in a wrapper, or file
   upstream. Also note `--update-baseline` writes **no** reports by design
   (cli.md:167) — an exit-0 run that produced no artifacts (§9.4 footgun).

### 10.4 Revisions to the §3.4 task sketch (delta list)

1. **Timeout conditionality:** build `--timeout-coefficient 5` only when
   `usage_args` contains no `--timeout` token (mirror the `--dry-run`
   condition); otherwise mutarust rejects the command line (R6 closed).
2. **`--test-flags` upgrade (pending experiment):** `"--features test-utils
   --all-targets --profile mutants"` — keeps `[profile.mutants]` alive and
   corrects the §5.7 retire list. Ship the task without it first; add after
   the §8 experiment passes.
3. **Drop flags** (with `long_help` answers): `--exclude`, `-L`, `-v`, `-V`
   → replaced by defaults/`--output-statuses`/`--debug`; add a **flag-migration
   cheatsheet** to `long_help` (old `mutants` flag → new answer) — the single
   biggest human/agent-continuity affordance during coexistence.
4. **Exit-code contract in `long_help`:** `0 pass / 1 tool error / 3
   config-annotation broken / 4 gate red` (+ caution: `2` means *bash
   completion* for mutarust but *findings* for messrust).
5. **`--workers` mention** in help (replaces `--jobs` muscle memory);
   note workers×`-j` coupling.
6. **`--git-diff` caveat in help:** untracked files invisible (§9 footgun).
7. **RUSTFLAGS/cap-lints:** *not* in the sketch by default — add a commented
   `# env = { RUSTFLAGS = "--cap-lints=allow" }` line with a "see
   quality_gates_mise_adoption §10.2 row 2 before enabling" comment.
8. **Do not add `depends_post`** — §10.3; the report task is gone, not
   renamed.

### 10.5 Corrected artifact ledger (supersedes §5.7 where they conflict)

- **Retired at stage 3:** tools line + lock entry, both `.mise/tasks/mutants/`
  files, `.cargo/mutants.toml`, gitignore `mutants-report.md` line.
- **`[profile.mutants]`: RETAIN if §10.4 row-2 experiment passes; retire only
  if it fails** (§5.7 said unconditional retire — corrected here).
- **`.gitignore` `/mutants.out*/` lines:** retire at stage 3 (mutarust areas
  live in `$TMPDIR`, removed automatically; `--do-not-remove-tmp-folder` is
  opt-in passthrough).
- **`docs/refs/mutation_testing.md` rewrite — now enumerated:** §2 tooling
  (nextest motivation weakens: mutation stops using it — say what still does),
  §3 config/task snippets (already stale — `[tasks.mutants]` TOML never
  matched reality), §4 CI sample (`--in-diff` → `--git-diff-lines`), §5 usage
  examples (line-for-line flag mapping from §5.3 + §10.2), §6 version notes →
  mutarust versions/schema pointers, §7 parallelism (`--jobs` → `--workers`;
  the "keep parallelism in mise.toml" claim matches no current file — drop or
  re-derive), new §: baseline workflow (`--update-baseline` → commit →
  `--fail-on-escaped`) which is the methodology section's centerpiece now.
- **Unchanged:** hk.pkl, verify, ci.yml, nextest (still used by `test`),
  `[task_templates.bench]`, worktree copies under `.worktrees/` (out of tree).

### 10.6 Open decision points & additions to the §8 checklist

**Decisions the implementer must get answered (in order):**

1. Cap-lints strategy (§10.2 row 2) — A/B on one module: state counts with
   and without `RUSTFLAGS=--cap-lints=allow`; watch `Skipped` inflation.
2. `--profile mutants` in `--test-flags` — accept or reject (row 4).
3. `--check` workflow: drop (recommended) vs retain cargo-mutants (row 8).
4. `--exclude`: drop (recommended) vs any accepted approximation (row 7).
5. Markdown report: retire (recommended) vs thin renderer (§10.3).
6. R4/R5 exclusion gaps + nextest `--exec` — unchanged from §5/§7.

**Checklist additions (§8):**

```sh
# report-on-gate-failure (do this FIRST — it constrains the whole design)
mise run mutarust --min-msi 99 -m <mod>; echo $?     # expect 4
ls -la report.json mutarust-agentic.json             # MUST exist despite exit 4
# empty-run artifacts
mise run mutarust -m <empty-or-fully-killed-mod>     # escaped_count 0 file still written?
# timeout conditionality
mise run mutarust -m <mod> -- --timeout 120          # must not be rejected
mise run mutarust --dry-run                           # still omits static flags
# profile / test-flags experiments
mise run mutarust -- --list-files                    # confirm scope; then with --profile mutants appended
# config precedence
mise run mutarust -- --config other.yml              # task's --config + passthrough: which wins?
# UX regressions
mise run mutarust -- --exclude 'src/*'               # confirm clean "unknown" error, document wording
```
