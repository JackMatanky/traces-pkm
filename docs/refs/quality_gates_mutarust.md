# mutarust — Research Note

Research into [`quality-gates/mutarust`](https://github.com/quality-gates/mutarust)
for possible use in this repository, compared against the existing
`cargo-mutants` setup (see [mutation_testing.md](mutation_testing.md)).

Primary sources checked on 2026-09-25: the repo README, `docs/` directory
(`install.md`, `quickstart.md`, `cli.md`, `config.md`, `ci.md`,
`selective-ci.md`, `parity.md`, `glossary.md`), `Cargo.toml`, GitHub repository
and release APIs, the crates.io crate API, and — for the comparison —
`sourcefrog/cargo-mutants` book pages. No secondary blog sources were used.

---

## 1. What mutarust is

- **Purpose:** a local CLI for **mutation testing** in Rust. It mutates Rust
  production source, runs the Cargo test suite against each mutant, and reports
  kills, escapes, and scores — "small source changes that should fail the suite
  and do not" ([README](https://github.com/quality-gates/mutarust)).
- So the name is literal, not incidental: it does what `cargo-mutants` does,
  as a standalone binary rather than a cargo subcommand.
- **Name origin:** not stated anywhere in the primary sources. The closest
  documented relationship is that mutarust is a Rust implementation carrying
  explicit feature parity with **Mutago** (a Go mutation tester) at
  Mutago v2.7.7 — mutator names, config field set, CLI flags, and baseline
  format are all Mutago-compatible
  ([docs/parity.md](https://github.com/quality-gates/mutarust/blob/main/docs/parity.md),
  [docs/config.md](https://github.com/quality-gates/mutarust/blob/main/docs/config.md)).
  Any etymology beyond that is unverified.
- **Maturity** (from primary APIs):
  - License: MIT ([README](https://github.com/quality-gates/mutarust),
    [GitHub API](https://api.github.com/repos/quality-gates/mutarust)).
  - Version: `0.1.9`, `rust-version = 1.85`, edition 2024, single binary target
    `mutarust` ([Cargo.toml](https://github.com/quality-gates/mutarust/blob/main/Cargo.toml)).
  - Project age: repo created 2026-08-01, first crates.io release 0.1.0 on
    2026-08-03, latest 0.1.9 on 2026-09-20 — 10 releases in ~7 weeks, roughly
    weekly cadence ([crates.io API](https://crates.io/api/v1/crates/mutarust),
    [GitHub releases API](https://api.github.com/repos/quality-gates/mutarust/releases)).
  - Adoption: **0 stars, 0 forks**, ~**201 total crates.io downloads**,
    13 open issues + 4 open PRs, still actively pushed (last push 2026-09-25)
    ([GitHub API](https://api.github.com/repos/quality-gates/mutarust),
    [repo page](https://github.com/quality-gates/mutarust),
    [crates.io API](https://crates.io/api/v1/crates/mutarust)).
  - Maintainership: a single publisher (`jonbaldie`) under the `quality-gates`
    org; each release note documents CI verification (Quality, Messrust,
    Security, Mutation, Docs workflows) plus a clean-registry install check
    ([release v0.1.9](https://github.com/quality-gates/mutarust/releases/tag/v0.1.9)).
  - Verdict: **actively maintained but very young and essentially unadopted
    (0.x, single maintainer).**

---

## 2. How it works technically

### 2.1 Inputs and execution flow

- Invocation: `mutarust [OPTIONS] [TARGET]...`, where a target is a source
  file, directory, Cargo package name, or workspace
  ([docs/cli.md](https://github.com/quality-gates/mutarust/blob/main/docs/cli.md)).
- Flow: (1) run the clean test suite first and abort if it fails; (2) parse
  selected production sources and plan mutants; (3) run each mutant **in a new
  temporary copy of the Cargo workspace**; test failure kills, success lets it
  escape, compile failure skips, test-command/timeout failure errors
  ([docs/cli.md](https://github.com/quality-gates/mutarust/blob/main/docs/cli.md)).
- It never mutates items inside `#[cfg(test)]` modules — those are test source
  ([docs/cli.md](https://github.com/quality-gates/mutarust/blob/main/docs/cli.md)).
- Summary states: killed, escaped, errored, not-covered, skipped, total.
  Score = (killed + errored + skipped) / total mutants — note this **counts
  errored and skipped toward the score**, which differs from the textbook
  MSI definition used in [mutation_testing.md](mutation_testing.md)
  ([docs/cli.md](https://github.com/quality-gates/mutarust/blob/main/docs/cli.md)).

### 2.2 Mutant generation

- Parsing/mutation is AST-based: the crate depends on `syn` 3,
  `proc-macro2` (with `span-locations`), and `rustc_lexer`
  ([Cargo.toml](https://github.com/quality-gates/mutarust/blob/main/Cargo.toml));
  `--print-ast` dumps the parsed syntax tree
  ([docs/cli.md](https://github.com/quality-gates/mutarust/blob/main/docs/cli.md)).
- **33 built-in mutators** with stable Mutago names across exactly ten groups:
  `arithmetic/`, `branch/`, `composite/`, `concurrency/`, `conditional/`,
  `expression/`, `loop/`, `numbers/`, `select/`, `statement/`
  (`value/` in the earlier draft is a Mutago *fixture directory*, not a
  mutator group) `[corrected in §9 review]`
  ([docs/parity.md](https://github.com/quality-gates/mutarust/blob/main/docs/parity.md),
  [docs/mutators.md](https://github.com/quality-gates/mutarust/blob/main/docs/mutators.md)).
  Go-only operators with no Rust form are simply absent.
- Custom mutators are documented separately
  ([docs/custom-mutators.md](https://github.com/quality-gates/mutarust/blob/main/docs/custom-mutators.md))
  (not examined in depth here).

### 2.3 Parallelism and incremental build reuse

- Runs mutants in parallel: default up to one worker per logical CPU,
  capped by `--workers`; each worker keeps **one isolated workspace copy and
  reuses its Cargo target directory across mutants**, so later mutants rebuild
  only the changed crate; each Cargo build is additionally capped with `-j` so
  worker count × jobs ≈ CPU count
  ([docs/cli.md](https://github.com/quality-gates/mutarust/blob/main/docs/cli.md),
  confirmed in [release v0.1.2](https://github.com/quality-gates/mutarust/releases/tag/v0.1.2)).
  Shared dependency artifacts are symlinked read-only rather than duplicated
  ([release v0.1.4](https://github.com/quality-gates/mutarust/releases/tag/v0.1.4)).
- **Timeouts:** fixed **60 s per Cargo test run by default** (`--exec-timeout`
  / `--timeout`), or adaptive via `--timeout-coefficient` = **longest single
  clean-test duration** × factor (min 1 s) — not the whole clean-suite
  duration; the factor also sizes the clean-suite and coverage runs as
  factor × 60 s before the clean duration is known `[corrected in §9 review]`
  ([docs/cli.md](https://github.com/quality-gates/mutarust/blob/main/docs/cli.md)).
- **No cross-run incremental cache of caught mutants.** The equivalent of
  "skip what already passed" is the *baseline* mechanism: `--update-baseline`
  writes accepted escapes to `mutarust-baseline.json`, `--fail-on-escaped`
  fails (exit 4) only on new escapes; `--blacklist` skips mutants by checksum
  ([docs/cli.md](https://github.com/quality-gates/mutarust/blob/main/docs/cli.md)).
  There is no `--iterate`-style re-run.

### 2.4 Test runner

- Default test command is plain **`cargo test`** (source: `src/execution.rs`
  references `cargo test` throughout; no alternative runner option exists in
  the CLI reference).
- **`nextest` is mentioned nowhere in the repository** — grep of the full
  source tree and docs returned zero hits (re-confirmed against the vendored
  copy: zero matches for `nextest`/`iterate` under
  `docs/refs/mutarust/`). The only way to use nextest is the
  custom `--exec COMMAND` escape hatch (exit 0 = kill, 1 = escape, 2 = skip)
  — but `--exec` runs on **one worker** (serial mutant execution) and
  mutators needing Cargo type-proof (`expression/context-nil`,
  `statement/return`, `statement/defer-remove`, `expression/recover-clear`,
  `select/*`) are marked `Skipped` under a custom command
  `[corrected in §9 review]`
  ([docs/cli.md](https://github.com/quality-gates/mutarust/blob/main/docs/cli.md)).
- `--test-recursive` passes `--workspace` to Cargo; `--test-flags` adds
  shell-quoted args (e.g. `--features test-utils`) to every compile/test
  command ([docs/cli.md](https://github.com/quality-gates/mutarust/blob/main/docs/cli.md)).

### 2.5 Coverage integration

- `--coverage` collects line coverage **via `cargo-llvm-cov`** (needs
  `cargo install cargo-llvm-cov` + `rustup component add llvm-tools-preview`).
  Uncovered mutants get state `not covered` and are not run; a separate
  covered-code score results; `--min-covered-msi` gates on it
  ([docs/cli.md](https://github.com/quality-gates/mutarust/blob/main/docs/cli.md)).
- `--per-test` collects one LLVM coverage report per Cargo test and runs only
  tests touching the mutated line — but **per-test collection is sequential**
  ([docs/cli.md](https://github.com/quality-gates/mutarust/blob/main/docs/cli.md)).
- This is a *different coverage engine* from this repo's `cargo-tarpaulin`
  stack ([crap_metric.md](crap_metric.md)); llvm-cov would be an additional
  tool in `mise.toml`.

### 2.6 Configuration

- YAML file, **only loaded when `--config FILE` is passed — no automatic
  discovery** (no `mutarust.yml` search up the tree)
  ([docs/config.md](https://github.com/quality-gates/mutarust/blob/main/docs/config.md)).
- Fields (all optional): `skip_without_test`, `skip_with_cfg`, `json_output`,
  `html_output`, `silent_mode`, `min_msi`, `min_covered_msi`, `exclude_dirs`,
  `disable_mutators`, `enable_mutators`, `ignore_source_lines`
  ([docs/config.md](https://github.com/quality-gates/mutarust/blob/main/docs/config.md)).
  JSON Schema published at
  [`schema/mutarust.schema.json`](https://github.com/quality-gates/mutarust/blob/main/schema/mutarust.schema.json);
  starter file [`mutarust.yml.example`](https://github.com/quality-gates/mutarust/blob/main/mutarust.yml.example).
- CLI flags override config; unknown fields/invalid values fail fast with the
  config filename in the diagnostic ([docs/config.md](https://github.com/quality-gates/mutarust/blob/main/docs/config.md)).
- Source scoping: `exclude_dirs` (path prefixes), `ignore_source_lines`
  (line regexes), `--match REGEXP` (function names), plus in-source
  annotations `// mutator-disable-func|next-line|regexp`
  ([docs/config.md](https://github.com/quality-gates/mutarust/blob/main/docs/config.md)).
  There is **no glob-based file exclusion and no "exclude by impl-block
  pattern" equivalent** to this repo's `exclude_globs`/`exclude_re` in
  `.cargo/mutants.toml`.

### 2.7 Output formats and exit codes

- Outputs: per-mutant stdout lines (with unified diffs for escapes),
  `report.json` (full), `mutarust-report.html`,
  `mutarust-summary.json`, `mutarust-agentic.json`, GitHub Actions
  `::warning` annotations (`--logger-github`), GitLab Code Quality
  `mutarust-gitlab.json`
  ([docs/cli.md](https://github.com/quality-gates/mutarust/blob/main/docs/cli.md),
  [docs/config.md](https://github.com/quality-gates/mutarust/blob/main/docs/config.md)).
- Exit codes: `0` gates passed; `1` command error; `2` bash completion;
  `3` source/config/annotation error; **`4` quality-gate failure**
  (`--min-msi` / `--min-covered-msi` / `--fail-on-escaped`)
  ([docs/ci.md](https://github.com/quality-gates/mutarust/blob/main/docs/ci.md),
  [docs/parity.md](https://github.com/quality-gates/mutarust/blob/main/docs/parity.md)).
- Reports are written to the **current working directory** when enabled —
  would need `.gitignore` entries here.

### 2.8 CI integration

- Documented GitHub Actions flow: install `cargo-llvm-cov` + mutarust, run
  `mutarust --coverage --min-msi 75 --min-covered-msi 80 --logger-github .`;
  PR mode uses `--git-diff-lines --git-diff-base origin/main
  --ignore-msi-with-no-mutations`; legacy adoption uses
  `--update-baseline` then `--fail-on-escaped`
  ([docs/ci.md](https://github.com/quality-gates/mutarust/blob/main/docs/ci.md)).
- Recommended thresholds: baseline only (legacy), 60/75 (active),
  75/80 (stable library), 90/95 (high assurance)
  ([docs/ci.md](https://github.com/quality-gates/mutarust/blob/main/docs/ci.md)).
- The project runs itself through path-based selective CI (mutation job runs
  only for `**/*.rs`, `Cargo.toml`, `Cargo.lock`, `mutation.yml` diffs)
  ([docs/selective-ci.md](https://github.com/quality-gates/mutarust/blob/main/docs/selective-ci.md)).

---

## 3. Overlap with the existing stack (vs `cargo-mutants`)

Both tools do the same *job* (inject mutants, run tests, report escapes) on
the same kind of codebase, so at the top level they are **substitutes, not
complements** — running both would double mutation cost for overlapping
signal. The differences are in gating ergonomics:

| Dimension | `cargo-mutants` (27.1.0, current) | `mutarust` (0.1.9) |
| --- | --- | --- |
| Invocation | `cargo mutants` subcommand | standalone `mutarust` binary |
| Config | `.cargo/mutants.toml`, auto-discovered; `deny_unknown_fields` ([mutation_testing.md](mutation_testing.md)) | `mutarust.yml`, **only via `--config`** ([config.md](https://github.com/quality-gates/mutarust/blob/main/docs/config.md)) |
| Mechanism | parse with `syn`, patch a scratch copy of the tree reused across mutants ([how-it-works.md](https://github.com/sourcefrog/cargo-mutants/blob/main/book/src/how-it-works.md)) | parse with `syn`/`proc-macro2`, mutate in a per-worker temp workspace copy with target-dir reuse ([cli.md](https://github.com/quality-gates/mutarust/blob/main/docs/cli.md), [Cargo.toml](https://github.com/quality-gates/mutarust/blob/main/Cargo.toml)) — same broad strategy |
| Test runner | `test_tool = "nextest"` supported and **used by this repo** ([.cargo/mutants.toml](../../.cargo/mutants.toml), [nextest.md](https://github.com/sourcefrog/cargo-mutants/blob/main/book/src/nextest.md)) | plain `cargo test` only; nextest only via `--exec` ([cli.md](https://github.com/quality-gates/mutarust/blob/main/docs/cli.md)) |
| Score gate | **No score concept at all** — the book and source contain no mutation-score/MSI threshold; CI fails via exit codes and `mutants.out` contents ([exit-codes.md](https://github.com/sourcefrog/cargo-mutants/blob/main/book/src/exit-codes.md)) | Native `--min-msi` / `--min-covered-msi` thresholds, exit 4 ([cli.md](https://github.com/quality-gates/mutarust/blob/main/docs/cli.md)) |
| Coverage awareness | none | `--coverage` (llvm-cov) → `not covered` state + covered-code score ([cli.md](https://github.com/quality-gates/mutarust/blob/main/docs/cli.md)) |
| Changed-code scoping | `--in-diff` ([in-diff.md](https://github.com/sourcefrog/cargo-mutants/blob/main/book/src/in-diff.md)) | `--git-diff-lines` / `--git-diff-base` ([cli.md](https://github.com/quality-gates/mutarust/blob/main/docs/cli.md)) — equivalent capability |
| Accepted escapes | **No accepted-escape mechanism** — `--baseline` only controls running/skipping the *clean* test suite before mutation ([baseline.md](https://github.com/sourcefrog/cargo-mutants/blob/main/book/src/baseline.md)); survivors stay in `mutants.out/missed.txt` for manual triage | JSON baseline + `--fail-on-escaped` + `--blacklist` checksums, Mutago-compatible ([cli.md](https://github.com/quality-gates/mutarust/blob/main/docs/cli.md)) — richer adoption story |
| Incremental re-run | `--iterate` re-tests only prior escapes ([iterate.md](https://github.com/sourcefrog/cargo-mutants/blob/main/book/src/iterate.md)) | **none** (baseline only) |
| Exclusions | `exclude_globs`, `exclude_re`, `--file/--mod` ([.cargo/mutants.toml](../../.cargo/mutants.toml)) | `exclude_dirs`, `ignore_source_lines`, `--match`, source annotations ([config.md](https://github.com/quality-gates/mutarust/blob/main/docs/config.md)) — no glob-by-file or impl-pattern equivalent |
| Timeouts | adaptive: `timeout_multiplier = 5`, `minimum_test_timeout = 30` ([mutation_testing.md](mutation_testing.md)) | fixed 60 s default, or `--timeout-coefficient` ([cli.md](https://github.com/quality-gates/mutarust/blob/main/docs/cli.md)) |
| Reports | `mutants.out/` files (`.mise/tasks/mutants/report` consumes `missed.txt`), GitHub annotations auto-detected via `$GITHUB_ACTION` ([ci.md](https://github.com/sourcefrog/cargo-mutants/blob/main/book/src/ci.md)) | `report.json`, HTML, summary/agentic JSON, GitHub/GitLab loggers ([cli.md](https://github.com/quality-gates/mutarust/blob/main/docs/cli.md)) |
| Maturity | v27.1.0, established, 201-era tool with large ecosystem | v0.1.9, 7 weeks old, 0 stars / ~201 downloads |

**What mutarust adds that this stack lacks today:** an explicit MSI threshold
gate, coverage-aware scoring, an accepted-escape baseline with new-escape-only
failing, and richer machine-readable reports (agentic/summary JSON).

**What mutarust drops that this stack relies on:** nextest as the mutation
test runner, `--iterate`, glob/regex-based exclusions, the existing
`mutants.out/missed.txt` → `mutants-report.md` pipeline
(`.mise/tasks/mutants/report`, deleted in ffb725d6), the
module-scoped task UX with `-m/-f/--check` flags
([.mise/tasks/test/mutants](../../.mise/tasks/test/mutants)),
and `--profile mutants` / `--cap-lints` build tuning.

**Direct comparison on this workspace:** cannot be run — this is a
docs-only assessment; neither tool was installed or executed. From docs alone
the runtime class is the same (one isolated build + full test run per mutant,
parallel workers with target-dir reuse), so a full mutarust run should land in
the same order of magnitude as the existing `cargo mutants` run.
`--coverage` adds an llvm-cov instrumented build pass
first; `--per-test` trades parallelism for fewer test executions
([cli.md](https://github.com/quality-gates/mutarust/blob/main/docs/cli.md)).
No benchmark numbers exist in either project's docs — treat this as an
estimate, not a measurement. Likewise, the "no score concept" claim for
cargo-mutants comes from grepping `book/src` and `src` at 27.1.0 (zero
matches for "score"), not from an exhaustive feature review.

---

## 4. Installation via mise

The crate **is published to crates.io** as `mutarust` with binary `mutarust`
([crates.io](https://crates.io/crates/mutarust),
[Cargo.toml](https://github.com/quality-gates/mutarust/blob/main/Cargo.toml)),
so the standard mise `cargo:` backend applies
([mise cargo backend docs](https://mise.jdx.dev/dev-tools/backends/cargo.html)):

```toml
# [tools] in mise.toml — matches the repo's existing conventions
"cargo:mutarust" = "latest"
```

Pinned alternative (recommended for a 0.x tool with weekly releases):

```toml
"cargo:mutarust" = "0.1.9"
```

(Version selection is `mise use cargo:mutarust@0.1.9`; the repo's
`lockfile = true` in [mise.toml](../../mise.toml) already records resolved
versions in `mise.lock`, so even `"latest"` is reproducible across machines
after first install.)

Caveats:

- **Rust ≥ 1.85 required** (`rust-version = "1.85"`, edition 2024) — the
  repo's `nightly-2026-09-09` toolchain satisfies this; no
  `install_env.RUSTUP_TOOLCHAIN` override is needed, unlike `cargo:ryl` /
  `cargo:adrs` in [mise.toml](../../mise.toml)
  ([docs/install.md](https://github.com/quality-gates/mutarust/blob/main/docs/install.md)).
- The repo has `cargo-binstall = "latest"`, and mise's cargo backend prefers
  binstall. mutarust publishes **no prebuilt release assets for current
  versions** (only v0.1.4 has darwin amd64/arm64 tarballs
  — [releases API](https://api.github.com/repos/quality-gates/mutarust/releases)),
  so binstall falls back to a source build; expect a couple of minutes for a
  ~16 kLOC crate plus `syn`.
- **`github:quality-gates/mutarust` is not viable**: the `github:` backend
  needs release assets, and recent releases have none.
- The custom URL form used for `rust-docs-mcp`
  (`"cargo:https://github.com/..."`) is unnecessary — the crate is on
  crates.io.
- Optional extra if `--coverage` is wanted:
  `"cargo:cargo-llvm-cov" = "latest"` (llvm-cov, not tarpaulin)
  ([docs/cli.md](https://github.com/quality-gates/mutarust/blob/main/docs/cli.md)).

---

## 5. How it would slot into this repo's gates

- **Where mutation currently lives:** file task
  `.mise/tasks/test/mutants` (extends `[task_templates.mutants]`,
  `timeout = "1h"`, cache off, depends on `test`, post-runs
  `mutants:report`). `hk.pkl` contains **no mutation step** (grep: zero
  matches), and `verify` = `fmt` → `check`/`lint`/`test` — mutation is
  deliberately outside `verify` ([mise.toml](../../mise.toml)).
- **Proposed slotting (if adopted):** a sibling file task
  `.mise/tasks/mutarust/_default` under the "Quality, Coverage & Mutation"
  section of `mise.toml`, extending `task_templates.mutants` (1 h timeout,
  cache off) so both tools share the same budget and never-skip semantics.
  Do **not** hang it off `verify` or the `hk` gate — it is far slower than
  fmt/check/lint/test, exactly why `mutants` is already excluded.
- **Config plumbing required:** repo-root `mutarust.yml` passed explicitly
  with `--config mutarust.yml` (no auto-discovery), plus `.gitignore` entries
  for `report.json` / `mutarust-*.json` / `mutarust-report.html` /
  `mutarust-baseline.json`.
- **Command shape** for this single-package workspace
  ([Cargo.toml](../../Cargo.toml)):

  ```bash
  mutarust --config mutarust.yml --test-flags "--features test-utils" \
    --min-msi 60 .
  # CI / PR mode:
  mutarust --config mutarust.yml --git-diff-lines --git-diff-base origin/main \
    --ignore-msi-with-no-mutations --min-msi 60 --logger-github .
  ```

- **Runtime/CI viability:** the existing mutants template already allows 1 h
  locally; mutarust is the same cost class (§3). This repo's CI
  ([.github/workflows/ci.yml](../../.github/workflows/ci.yml)) currently has
  **no mutation job at all** (cargo-mutants runs are local-only), so adopting
  mutarust would not make CI slower unless a mutation job is added
  deliberately — and `--git-diff-lines` is the documented way to keep that
  cheap ([docs/ci.md](https://github.com/quality-gates/mutarust/blob/main/docs/ci.md)).
- **Replace or coexist?** They are redundant in purpose. Running both
  doubles wall-clock for largely overlapping escape signal. Coexistence only
  makes sense as a short A/B pilot, not steady state.

---

## 6. Risks, caveats, recommendation

Risks:

1. **Immaturity/adoption:** 0.x, ~7 weeks old, 0 stars, ~201 downloads, one
   maintainer ([§1](#1-what-mutarust-is)). Breaking changes between 0.1.x
   releases are plausible (0.1.1 already absorbed a `syn` 2→3 migration).
   Pin the version; keep `lockfile = true`.
2. **Score-definition drift:** its MSI counts *errored* and *skipped* mutants
   as non-escapes ([docs/cli.md](https://github.com/quality-gates/mutarust/blob/main/docs/cli.md)),
   so thresholds will not be comparable with cargo-mutants' output or the
   definition in [mutation_testing.md](mutation_testing.md).
3. **No nextest:** the repo standardized on `test_tool = "nextest"` for
   mutation runs; mutarust always uses `cargo test` unless `--exec` is
   hand-written — a real feedback-speed regression.
4. **Fixed 60 s test timeout** by default could mis-classify mutants as
   errored on a slow full-suite run unless `--timeout-coefficient` is set
   ([docs/cli.md](https://github.com/quality-gates/mutarust/blob/main/docs/cli.md)).
5. **Config not auto-discovered** — easy to forget `--config` and silently run
   with no policy ([docs/config.md](https://github.com/quality-gates/mutarust/blob/main/docs/config.md)).
6. **Coverage tool split:** `--coverage` pulls in `cargo-llvm-cov` alongside
   the existing `cargo-tarpaulin` — two coverage engines in one repo.
7. **Exclusion gaps:** `.cargo/mutants.toml`'s `exclude_re` (skip `impl
   Debug/Display/Serialize…` boilerplate) has no direct mutarust equivalent;
   porting would need `ignore_source_lines` regexes or in-source annotations.

Recommendation: **adopt later (pilot), not now.**

Reasoning: mutarust solves a problem this repo already solves with a deeply
integrated `cargo-mutants` setup (bespoke file task with module/file filters,
report generation, task template, documented config). Its genuinely new value
— a numeric MSI gate, coverage-aware scoring, and baseline-based adoption of
accepted escapes — is not currently a stated requirement here, and its gaps
(nextest, `--iterate`, glob exclusions) collide with choices this repo has
already made. Given the tool's age and zero adoption, wiring it into gates
today would add a fragile dependency for marginal signal.

Revisit if one of these becomes true: (a) a hard numeric mutation-score
threshold is wanted in CI (cargo-mutants cannot express one), (b) surviving
mutants must be baselined so only *new* escapes fail a gate, or (c)
cargo-mutants itself stalls. A cheap trial at that point:

```toml
"cargo:mutarust" = "0.1.9"   # pin; bump deliberately
```

```bash
mise exec -- mutarust --git-diff-lines --min-msi 60 --logger-github .
```

---

## 7. Primary sources

- Repo README — https://github.com/quality-gates/mutarust
- CLI reference — https://github.com/quality-gates/mutarust/blob/main/docs/cli.md
- Configuration — https://github.com/quality-gates/mutarust/blob/main/docs/config.md
- CI guide — https://github.com/quality-gates/mutarust/blob/main/docs/ci.md
- Selective CI — https://github.com/quality-gates/mutarust/blob/main/docs/selective-ci.md
- Install / quick start — https://github.com/quality-gates/mutarust/blob/main/docs/install.md ,
  .../docs/quickstart.md
- Mutago parity table — https://github.com/quality-gates/mutarust/blob/main/docs/parity.md
- Mutators / glossary — .../docs/mutators.md , .../docs/glossary.md
- Manifest — https://github.com/quality-gates/mutarust/blob/main/Cargo.toml
- Releases — https://api.github.com/repos/quality-gates/mutarust/releases
- Repo metadata — https://api.github.com/repos/quality-gates/mutarust
- Crate registry — https://crates.io/api/v1/crates/mutarust
- cargo-mutants (comparison): https://github.com/sourcefrog/cargo-mutants
  `book/src/{how-it-works,exit-codes,in-diff,baseline,iterate,nextest,ci}.md`
- mise cargo backend — https://mise.jdx.dev/dev-tools/backends/cargo.html
- Local: `docs/refs/mutation_testing.md`, `.cargo/mutants.toml`, `mise.toml`,
  `.mise/tasks/mutants/_default`, `.mise/tasks/mutants/report`, `hk.pkl`,
  `.github/workflows/ci.yml`

---

## 8. Configuration & day-to-day usage (follow-up)

Follow-up research on *how to configure and leverage* mutarust locally (not
in CI), coexisting with the cargo-mutants setup. Primary sources: upstream
[docs/cli.md](https://github.com/quality-gates/mutarust/blob/main/docs/cli.md),
[docs/config.md](https://github.com/quality-gates/mutarust/blob/main/docs/config.md),
[mutarust.yml.example](https://github.com/quality-gates/mutarust/blob/main/mutarust.yml.example)
(all read in full for this section), plus this repo's
`.cargo/mutants.toml`, `.mise/tasks/mutants/_default`, `mise.toml`.

### 8.1 Config surface (verified from config.md + cli.md)

- **Loading:** `mutarust.yml` is read **only** when `--config FILE` is
  passed; relative paths resolve from the CWD; unknown fields / bad types /
  invalid scores / bad regexes / unknown mutator names fail fast naming the
  file. CLI flags override config (`--min-msi`, `--silent`/`--no-silent`,
  `--enable` replace; `--disable` *adds* to `disable_mutators`).
- **YAML fields (all optional, exact set):** `skip_without_test`,
  `skip_with_cfg`, `json_output` (→ `report.json`), `html_output` (→
  `mutarust-report.html`), `silent_mode`, `min_msi`, `min_covered_msi`
  *(typo `min_covered_ms` in the earlier draft* `[corrected in §9 review]`*)*,
  `exclude_dirs` (path prefixes relative to workspace root), `disable_mutators`
  / `enable_mutators` (exact name, `group/*`, or `*`), `ignore_source_lines`
  (regexes matched against whole lines; a mutation whose changed range touches
  a matching line is not created).
- **Command-only (not YAML):** baseline trio `--update-baseline` /
  `--baseline FILE` / `--fail-on-escaped` (exit 4 only on escapes not in the
  Mutago-compatible `mutarust-baseline.json`), `--blacklist FILE`
  (checksum skips), `--match REGEXP` (function names), score gates
  `--min-msi` / `--min-covered-msi`, scoping `--git-diff-lines` +
  `--git-diff-base REF`, execution `--workers`, `--timeout`/`--exec-timeout`
  (fixed 60 s default) or `--timeout-coefficient` (**longest single
  clean-test** duration × factor, min 1 s — not the suite duration;
  **mutually exclusive with `--timeout`**) `[corrected in §9 review]`, `--test-flags "..."`,
  `--test-recursive` (passes `--workspace`), `--coverage`/`--per-test`
  (llvm-cov), inspection `--dry-run` (count only, no tests), `--no-exec`,
  `--list-files`, `--print-ast`, `--list-mutators`, loggers
  `--logger-summary-json`/`--logger-agentic-json`/`--logger-github`.
- **Exit codes:** 0 pass, 1 command error, 2 bash completion, 3
  source/config/annotation error, **4 quality-gate failure**
  (`min_msi`/`min_covered_msi`/`--fail-on-escaped`).
- **Source selection:** targets may be files, directories, package names, or
  the workspace; recursive discovery excludes tests/benches/examples/fixtures/
  vendor/generated/`build.rs`/`*_test.rs` — broad targets already skip test
  source, and `#[cfg(test)]` items are never mutated.
- **Reports** land in the **current working directory** → need `.gitignore`
  entries here.

### 8.2 Porting `.cargo/mutants.toml` → `mutarust.yml` (mapping table)

| `.cargo/mutants.toml` | mutarust equivalent | Verdict |
| --- | --- | --- |
| `exclude_globs = ["src/cli/**"]` | `exclude_dirs: ["src/cli"]` | ✓ maps (path prefix) |
| `exclude_globs = ["src/main.rs", "src/**/error.rs"]` | none — `exclude_dirs` is directory-only; `ignore_source_lines` is line-level, not file-level | ✗ **gap**: main.rs/error.rs mutants will run unless explicit file targets are passed on the CLI or in-source `// mutator-disable-func` annotations are added per function |
| `exclude_re = ["fn main", "impl.*Debug", ...]` | nearest: `ignore_source_lines` regexes or in-source `// mutator-disable-regexp` annotations | ~ partial: line-regex only skips mutations *on* matching lines, so `impl.*Debug` headers (no expressions) skip nothing — bodies still mutate. Real port = `// mutator-disable-func` annotations in those impls, or accept the extra mutants |
| `test_tool = "nextest"` | `--exec "cargo nextest run ..."` (exit 0=kill/1=escape/2=skip) | ~ compiles, but **serial** (`--exec` uses one worker) and type-proof mutators become `Skipped` `[corrected in §9 review]`; default `cargo test` otherwise (§2.4) |
| `features = ["test-utils"]` + `additional_cargo_test_args = ["--all-targets"]` | `--test-flags "--features test-utils --all-targets"` | ✓ maps |
| `timeout_multiplier = 5`, `minimum_test_timeout = 30` | `--timeout-coefficient 5` (× longest clean test, floor 1 s) | ✓ maps (closest semantic) |
| `--baseline run` / `--iterate` (cargo-mutants) | baseline *file* + `--fail-on-escaped`; **no `--iterate`** | ~ different mechanism (§3) |
| `--profile mutants`, `--cap-lints true` | none (not exposed) | ✗ no equivalent; each worker reuses its own target dir instead |

### 8.3 Proposed artifacts (local-only pilot, no CI, no hk gate)

1. **`mise.toml` `[tools]`** (from §4):

   ```toml
   "cargo:mutarust" = "0.1.9"
   ```

   then `mise lock`. (`cargo:cargo-llvm-cov` deliberately omitted — coverage
   mode is a later opt-in, and it would be a second coverage engine beside
   tarpaulin.)

2. **Repo policy:** `mutarust.yml` at repo root (per-tool config convention).
   Initial content — **no score gate yet** (baseline-first adoption):

   ```yaml
   # mutarust.yml — used only via `--config mutarust.yml` (no auto-discovery)
   skip_without_test: false
   skip_with_cfg: false
   json_output: true          # report.json for triage
   html_output: false
   silent_mode: false
   # min_msi: 60              # enable after first full run (docs "active" tier)
   exclude_dirs:
     - src/cli                # port of exclude_globs "src/cli/**"
   disable_mutators: []
   enable_mutators: []
   ignore_source_lines: []    # add regexes only after seeing escape noise
   ```

3. **Task:** `.mise/tasks/mutarust/_default` — sibling of `mutants/_default`,
   **extends the same `[task_templates.mutants]`** (1 h timeout, cache off,
   `outputs = []`) so both mutation tools share budget and never-skip
   semantics. Names don't collide (`mutants` vs `mutarust`). Mirrors the
   cargo-mutants task's UX:

   ```bash
   #!/bin/bash
   #MISE description="Run mutation testing with `mutarust` (pilot)"
   #MISE sources=["@group:rust"]
   #MISE extends="mutants"
   #MISE depends=["test"]
   #USAGE flag "-f --file <file>" help="Mutate one file only (e.g. -f src/config.rs)"
   #USAGE flag "-m --mod <module>" {
   #USAGE   help "Mutate one module only (target src/<module>)"
   #USAGE }
   #USAGE complete "module" run="sed -nE 's/^(pub )?mod ([a-z_]+);$/\\2/p' src/lib.rs"
   #USAGE flag "--min-msi <n>" help="Fail (exit 4) if total mutation score < n"
   #USAGE flag "--update-baseline" help="Accept current escapes into mutarust-baseline.json"
   #USAGE flag "--fail-on-escaped" help="Fail (exit 4) only on escapes not in baseline"
   #USAGE flag "--git-diff" negate="--no-git-diff" default=#false help="Mutate only lines changed vs origin/HEAD"
   #USAGE flag "--dry-run" help="Count mutants without running tests"
   #USAGE arg "[args]" var=#true help="Extra arguments forwarded to mutarust"
   ```

   Body: `mutarust --config mutarust.yml --test-flags "--features test-utils
   --all-targets" --timeout-coefficient 5` + `-f`→file / `-m`→`src/<mod>`
   target / flags / passthrough. **No `depends_post` report task** — unlike
   cargo-mutants' `mutants.out/missed.txt` pipeline, mutarust writes
   `report.json` itself (`json_output: true`).

4. **`.gitignore` additions:** `report.json`, `mutarust-report.html`,
   `mutarust-summary.json`, `mutarust-agentic.json`, `mutarust-gitlab.json`.
   **`mutarust-baseline.json` should be committed** once created — it is the
   accepted-escape policy, analogous to `.cargo/mutants.toml`.

5. **Not proposed:** `hk.pkl` step, `verify`/`check` inclusion, CI job —
   matching the cargo-mutants precedent (mutation is already outside
   `verify`/hk) and the current "not part of any CI" decision.

### 8.4 Day-to-day leverage

- **Cost control first:** `mise run mutarust --dry-run` (count only) →
  scoped runs `-m <module>` / `-f <file>` → `--git-diff-lines` for
  changed-code-only runs (the cheap local default).
- **Baseline workflow (the key differentiator vs cargo-mutants):**
  1. Full scoped run; triage escapes in `report.json`.
  2. Fix real misses; for accepted survivors run `--update-baseline`
     (commits `mutarust-baseline.json`).
  3. Thereafter run with `--fail-on-escaped` — only *new* escapes fail
     (exit 4). This is the incremental adoption story cargo-mutants lacks.
- **Score gate later:** set `min_msi` in `mutarust.yml` once a full run
  establishes the current score (docs tiers: 60 active / 75 stable). Remember
  its score counts errored+skipped as non-escapes (§2.1) — not comparable
  with cargo-mutants numbers.
- **A/B pilot:** run `mise run mutarust -m <mod>` against
  `mise run test:mutants -m <mod>` on the same module; compare escape sets,
  wall-clock, and report ergonomics before deciding coexistence vs
  replacement (§3: steady-state double-running is not the goal).
- **nextest:** start with default `cargo test`. Do **not** hand-roll
  `--exec "cargo nextest run --features test-utils --all-targets ..."` for
  speed: `--exec` forces a **single worker** (serial mutants) and marks all
  Cargo type-proof mutators `Skipped`, so it is a parallelism *and*
  coverage regression `[corrected in §9 review]`. If plain `cargo test`
  feedback bites, tune with `--timeout-coefficient`/`--workers` instead
  (verify the exit-code contract on first use of any `--exec`).
- **Runtime:** same cost class as cargo-mutants (§3) — minutes to tens of
  minutes scoped, up to the shared 1 h template for full runs; *unmeasured on
  this codebase* (no benchmarks in either project's docs).

---

## 9. Full capability inventory (vendored docs review)

Review of the **vendored snapshot** at `docs/refs/mutarust/` — every file read
in full: `README.md`, `mutarust.yml.example`, all 14 `docs/*.md` (incl.
`json-outputs.md`, `mutators.md`, `custom-mutators.md`, `glossary.md`,
`parity.md`, `release.md`, `index.md`), and all five `schema/*.json`. The
vendored copy is authoritative and **post-dates §1's 2026-09-25 survey**
(mtime 2026-09-26; `json-outputs.md` shows `metadata.version: "0.2.0"` and
`index.md` links a 2026-09-26 report that is *not* included in the snapshot —
a broken link inside the vendored tree). Treat §1's version/adoption numbers
as a 2026-09-25 snapshot of upstream APIs; capability claims here follow the
vendored docs. Links below resolve inside the vendored copy
(`docs/refs/mutarust/…`); upstream equivalents follow the URL style of §1–§8.

**Corrections made in place above** (each tagged `[corrected in §9 review]`):
§2.2 (mutator groups — `value/` is a fixture dir, not a group; the ten real
groups are arithmetic, branch, composite, concurrency, conditional,
expression, loop, numbers, select, statement), §2.3 + §8.1
(`--timeout-coefficient` multiplies the **longest single clean test**, not
the suite duration; the factor also sizes clean-suite/coverage runs as
factor × 60 s until the clean duration is known), §2.4 + §8.2 + §8.4
(`--exec` runs on **one worker** and drops Cargo type-proof mutators to
`Skipped` — the earlier "works but hand-rolled" framing for nextest was too
generous), §8.1 (`min_covered_ms` typo → `min_covered_msi`).

### 9.1 Claims verified as-is

Confirmed against the vendored text: score = (killed + errored + skipped) /
total, zero when no mutants ([cli.md](mutarust/docs/cli.md)); no config
auto-discovery, `--config` only, CLI > YAML with `--disable` *adding* to
`disable_mutators` ([config.md](mutarust/docs/config.md)); 33 mutators with
Mutago names ([parity.md](mutarust/docs/parity.md)); exit codes 0/1/2/3/4;
reports written to CWD; `#[cfg(test)]` items never mutated; recursive
discovery excludes tests/benches/examples/fixtures/vendor/generated/
`build.rs`/`*_test.rs`; coverage via cargo-llvm-cov with sequential
`--per-test` collection; no `--iterate` and **zero occurrences of `nextest`
or `iterate` anywhere in the vendored tree**; recommended threshold tiers
60/75, 75/80, 90/95 ([ci.md](mutarust/docs/ci.md)).

### 9.2 Mutators and mutator selection

**(a) What it does.** All 33 operators with per-operator semantics are in
[mutators.md](mutarust/docs/mutators.md); names/selection rules in
[config.md](mutarust/docs/config.md). Selection patterns: exact name
(`conditional/bool-literal`), group wildcard (`conditional/*`), or `*`;
`enable_mutators` is an initial allowlist, `disable_mutators` a denylist;
CLI `--enable` *replaces* the config allowlist, `--disable` *appends* to the
denylist; `--list-mutators` prints the registry. Selection is validated
against the built-in list (unknown pattern → config error naming the file).
Notable operator behaviors: `statement/remove` only touches semicolon-terminated
expressions; `expression/error-guard` only inside `if` conditions;
`conditional/bool-literal` never touches conditions/returns/macros; number
mutators reject non-decimal literals, array lengths, overflow; every edited
file is re-parsed (invalid Rust is never planned) and equal mutations are
deduplicated before running. `mutatorStats[]` in `report.json` plus the
terminal's sorted per-mutator table report yield *per operator*
([json-outputs.md](mutarust/docs/json-outputs.md)).

**(b) Value here.** Grounded against `src/`:
- **Zero-yield groups for this repo:** no `tokio::select!`/`tokio::spawn`/
  `thread::spawn` anywhere in `src/` (grep: 0), so `select/case-remove`,
  `select/default-remove` and the Tokio arms of `concurrency/goroutine-remove`
  plan nothing → `disable_mutators: ["select/*"]` reduces planning noise at no
  signal cost (the thread arms of `concurrency/goroutine-remove` may still fire).
- **High-yield groups:** `expression/error-guard` has ~24 `if …​.is_err()/is_some()`
  candidates; `conditional/bool-literal` ~69 `= true/false` initializers;
  `statement/defer-remove` has real `drop(...)` sites (`src/index/store.rs`,
  `src/query/results.rs`, `src/dirtree.rs`, …); `composite/field-clear` fits
  the many `Default`-derived config/schema structs (`src/config/model.rs`,
  `src/schema/*`).
- **Mutator tuning for known-weak areas:** after the first full run, use
  `mutatorStats` to find the weakest operator, then raise signal by *adding*
  operators in a scoped run (`--enable "conditional/*"` on one module) rather
  than mutating everything everywhere — a bounded, repeatable workflow.

**(c) Humans vs agents.** Humans: per-mutator table + `--output-statuses`
make "which operator class is my suite bad at" a one-command read. Agents:
`mutatorStats` is a stable array keyed by Mutago name → direct diff between
runs; `--list-mutators` output is a fixed enumeration usable as a vocabulary
for config validation. Footgun: the published config schema's name pattern
`^[a-z0-9-]+(/[a-z0-9-]+)*(/\*)?$` **rejects underscores**, yet two built-ins
contain them (`arithmetic/assign_invert`, `loop/range_break`) — a
schema-validating editor will flag correct config (§9.13).

### 9.3 Source scoping and inspection commands

**(a) What it does** ([cli.md](mutarust/docs/cli.md),
[config.md](mutarust/docs/config.md)):
- `--list-files [TARGET]...` — dry listing of selected production sources;
  absolute, sorted, deduped. No target ⇒ CWD **recursively**; a
  directory/package/workspace target selects its **direct** source files
  (Cargo target data for packages — declared lib/bin paths incl. outside
  `src`); trailing `...` (`./src...`) selects nested files. Excludes
  `_test.rs`, `build.rs`, hidden, fixture/vendor/generated during recursive
  discovery — but **an explicit file target can select test/fixture source**.
  Targets needing inactive Cargo features are not selected.
- `--print-ast` — dump parsed syntax trees (same target rules; no tests, no
  workspace change).
- `--dry-run` — count selected mutants only, no writes, no tests; cannot be
  combined with workers/timeout/test controls/`--no-exec`.
- `--no-exec` — materialize each mutant into a temp *mutation area* and print
  its path (kept for inspection); `--do-not-remove-tmp-folder` keeps areas
  from normal runs (disk-heavy).
- `--match REGEXP` — function-name filter, **unanchored** unless you add
  `^`/`$`.
- Config scope: `exclude_dirs` (path prefixes; **does not change
  `--list-files` output**), `ignore_source_lines` (line regexes; a mutation
  whose changed range touches a matching line is never created).
- In-source annotations: `// mutator-disable-func [names]` (line above the
  fn, or above its first doc comment/attribute), `// mutator-disable-next-line`,
  `// mutator-disable-regexp <expr> [names]` (expression = first space-delimited
  token, no spaces inside); empty list or `*` = all mutators; invalid
  annotation → **exit 3 with path and line**.
- `skip_without_test: true` skips files with no `#[cfg(test)]` item;
  `skip_with_cfg: true` skips mutations inside non-test `#[cfg(...)]`-gated
  items (crate-level `#![cfg]` excludes the file).

**(b) Value here.** `exclude_dirs: ["src/cli"]` ports the existing
`exclude_globs` ([.cargo/mutants.toml](../../.cargo/mutants.toml)), but
`src/main.rs` and `src/**/error.rs` have **no file-level exclusion** —
annotate or pass explicit targets (§8.2). `skip_without_test` is near-useless
here (only 9 of 124 `src/*.rs` files lack `#[cfg(test)]`). `skip_with_cfg` is
**ambiguous for this repo**: dozens of items are gated
`#[cfg(any(test, feature = "test-utils"))]` (`src/config/model.rs`,
`src/note/parser/input.rs`, …) — the docs never say how `any(test, …)` is
classified; if treated as non-test cfg, enabling it would *suppress* active
test-support mutants while `--features test-utils` is on. Do not enable
without a `--dry-run` A/B. `mutarust --list-files …` is the cheap way to
verify scope before a run — and the docs leave one ambiguity: whether
`mutarust .` (explicit `.` target) is recursive or direct-files-only.
Always confirm with `--list-files` before trusting a bare `.` target.

**(c) Humans:** `--dry-run`/`--list-files`/`--print-ast` are the exploration
triad; annotations are per-site and self-documenting in source. **Agents:**
`--list-files` returns a deterministic sorted path list (great for building
`-m`/`-f` arguments or verifying exclusion config); `--dry-run` gives a
stable integer count for "how big is this run" questions without running
tests; annotation errors come back as exit 3 with file+line (actionable).

### 9.4 Baseline, blacklist, and escape gating

**(a) What it does** ([cli.md](mutarust/docs/cli.md),
[glossary.md](mutarust/docs/glossary.md)):
- Baseline file (Mutago-compatible): `{"version": 1, "mutants": [{id, file,
  mutator, line}]}`, default `mutarust-baseline.json`, `--baseline FILE`.
  `--update-baseline` writes the *full current escaped set* and **exits 0
  before normal output, score gates, and reports** (no report files are
  written on a baseline update); incompatible with `--dry-run`, `--no-exec`,
  `--run-mutant-id`. Missing baseline ⇒ accepts nothing.
- `--fail-on-escaped` ⇒ exit 4 only for escaped **stable IDs not in the
  baseline**; known escapes stay visible in normal output.
- `--blacklist FILE` (repeatable): lines of 32-char lowercase hex
  *checksums*; matching mutants are **never run** (cheaper than baseline).
  Checksum = hash of changed source lines (+ replaced byte range when needed)
  ⇒ survives unrelated edits elsewhere; printed as a `Blacklist checksum:`
  line under `ID:` and stored in `report.json`/HTML. **A stable ID is not a
  checksum.** Stable ID itself = source name + mutator name + mutation diff
  (byte range for repeated sites) — stable across runs, changes iff the
  mutation changes.

**(b) Value here.** Two acceptance granularities the cargo-mutants setup
lacks: *baseline* = "these survivors are policy, only new escapes fail"
(the adoption story in §8.4); *blacklist* = "this exact mutant is a known
false positive, don't burn a build on it". For a repo whose
`mutants:report` pipeline currently re-lists the same survivors every run
(`.mise/tasks/mutants/report`, deleted in ffb725d6),
`--fail-on-escaped` + committed baseline converts the report task from
"here is the same list again" to "here is what changed".

**(c) Humans:** commit `mutarust-baseline.json` like a lockfile; triage from
the still-visible escape lines. **Agents:** baseline diff (git diff of the
JSON) *is* the delta of newly-introduced weak tests — an ideal bounded task
("kill the mutants that appeared since HEAD"); checksums give a stable
per-mutant accept token independent of line numbers, so an agent can
blacklist one mutant without re-running everything.

### 9.5 Single-mutant rerun (`--run-mutant-id`)

**(a)** Runs exactly one stable mutant ID; prints only that mutant's
evidence — **no summary, no score gates**; sets `metadata.oneMutant: true`
in `report.json` ([cli.md](mutarust/docs/cli.md),
[json-outputs.md](mutarust/docs/json-outputs.md)). Incompatible with
`--update-baseline`.

**(b/c)** This completes the agent loop the prior note only listed: full run
→ `--logger-agentic-json` → agent writes a candidate test → re-run
**only that mutant** to verify the kill (exit/result line = proof), instead
of re-running the module. High agent value, modest human value (humans use
scoped `-m`/`-f` runs instead). It was mentioned in §8.1's flag list but
its semantics were never documented until now.

### 9.6 Git changed-line mode

**(a)** `--git-diff-lines` compares `--git-diff-base REF` (default: default
branch of `origin/HEAD`, else current branch) with the **working tree**:
committed + staged + unstaged tracked changes count; added Rust files and
changed lines in renamed files are selected; **untracked files and deleted
lines are not**; merge-base comparison when one exists (else base ref
directly); any Git error ⇒ hard stop (never widens scope); zero changed
mutable lines ⇒ success with zero mutants and no test run (pair with
`--ignore-msi-with-no-mutations` so `--min-msi` doesn't fail the empty run)
([cli.md](mutarust/docs/cli.md), [ci.md](mutarust/docs/ci.md)).

**(b/c)** The cheap local default for this repo (§8.4) — one flag replaces
hand-scoping to `-m <mod>`. Caveats discovered: untracked new files are
invisible to it (a brand-new `src/foo.rs` gets **no** mutants until
committed/staged), so agent workflows should stage before running. Humans:
instant "did *my* change hold the line". Agents: deterministic scope + exit
0 on empty change = safe to run in a pre-commit/pre-push gate without
false failures.

### 9.7 Coverage modes

**(a)** `--coverage`: llvm-cov LCOV pass **in private temp dirs** (never the
user's target dir); uncovered changed lines ⇒ `not covered` state, mutant not
run; summary gains covered-code score; `--min-covered-msi` gates it; **a
positive covered gate without `--coverage` ⇒ exit 4**; `--per-test` collects
one report per Cargo test and runs only tests covering the mutated line (works
*without* `--coverage`; sequential collection; no mapping ⇒ full suite);
`--coverage`/`--per-test` cannot combine with `--exec`, `--dry-run`,
`--no-exec` ([cli.md](mutarust/docs/cli.md)).

**(b/c)** Two effects here: (1) uncovered code stops dragging `min_msi`
(matters in a repo with deliberately excluded areas); (2) `notCovered[]` in
`report.json` is a *test-gap map* — arguably more actionable than escapes for
this codebase (which already measures line coverage with tarpaulin,
[crap_metric.md](crap_metric.md)). Cost: a second coverage engine
(`cargo:cargo-llvm-cov` in `mise.toml`). Humans: score honesty. Agents:
`notCoveredCount`/`notCovered[]` = "where to add the first tests" without
running mutants at all.

### 9.8 Timeouts, workers, and execution controls

**(a)** Fixed 60 s/Cargo-test default (`--exec-timeout`/`--timeout`);
`--timeout-coefficient F` = ceil(**longest single clean test** × F), min 1 s
— and before the clean duration is known, the clean-suite and coverage runs
each use F × 60 s; mutually exclusive with `--timeout`, `--exec`, `--no-exec`.
Workers: default ≤ 1/logical CPU, `--workers N` caps, never more than the
mutant count; each worker keeps one temp workspace + reuses its target dir;
each Cargo build gets `-j` so workers × jobs ≈ CPUs; **results print in plan
order regardless of parallelism**; `--exec` forces one worker. `--test-flags
"..."` is shell-quoted and applied to every compile/test command (conflicts
with `--exec`/`--no-exec`); `--test-recursive` ⇒ `--workspace` (or
`TEST_RECURSIVE=true` under `--exec`). Live progress line only when stderr is
a TTY and none of `--verbose/--debug/--silent/--no-exec/--dry-run` is set;
piped output has no control characters ([cli.md](mutarust/docs/cli.md)).

**(b/c)** `--timeout-coefficient 5` remains the right port of
`timeout_multiplier = 5` (§8.2) but its floor semantics differ from
cargo-mutants' `minimum_test_timeout = 30` (floor here: 1 s) — for a repo
whose full suite may run tens of seconds, coefficient-derived timeouts can be
*tighter* than the old 30 s floor; verify with one run. Plan-order output +
TTY-only progress makes stdout safe to parse even under parallelism — good for
the mise task. Incompatibility matrix (dry-run ⊥ workers/timeouts/test
controls; no-exec ⊥ timeout/Cargo controls; coverage ⊥ exec/dry-run/no-exec;
test-flags ⊥ exec/no-exec) means a mis-combined task script fails loudly at
argument validation rather than silently ignoring flags — desirable for
`#USAGE`-flagged mise tasks.

### 9.9 Custom `--exec` contract (nextest viability, restated precisely)

**(a)** `--exec COMMAND` parses shell-quoted, runs in the copied workspace,
inherits env, and receives `MUTATE_ORIGINAL`, `MUTATE_CHANGED`,
`MUTATE_PACKAGE`, `MUTATE_TIMEOUT`, `TEST_RECURSIVE`, `MUTATE_VERBOSE`,
`MUTATE_DEBUG`. Exit 0 = killed, 1 = escaped, 2 = skipped, anything else =
errored; timeout/interrupt kills the command and children; validated before
mutation starts. **One worker only.** Under a custom command, mutators whose
replacement needs Cargo type-proof are handled conservatively:
`expression/recover-clear`, `statement/defer-remove`, `select/case-remove`,
`select/default-remove` are always `Skipped`; `expression/context-nil` and
`statement/return` run only where a local parameter proves the type, else
`Skipped` ([cli.md](mutarust/docs/cli.md), [mutators.md](mutarust/docs/mutators.md)).

**(b/c)** For this repo: `--exec "cargo nextest run --features test-utils
--all-targets ..."` is legal but buys **serial execution + silently fewer
mutants** (§2.4/§8.2/§8.4 corrected). Recommendation stands: keep default
`cargo test`. If an agent ever uses `--exec`, it must know skipped-type-proof
mutants appear as `skippedCount` and are *scored as successes* — so `--exec`
runs overstate MSI. Low human/agent value today; documented for completeness.

### 9.10 Terminal output controls (human triage surface)

`--quiet` (escapes + summary only); `--output-statuses LETTERS` with exactly
the set of `k e s n x` (killed/escaped/skipped/not-covered/errored — e.g.
`--output-statuses ex`); `--no-diffs` hides escape diffs; `--silent` /
`--no-silent` (mutually exclusive; CLI beats `silent_mode`); `--verbose`
(file, line, worker count before each mutant); `--debug` (adds mutator name +
test command; **never prints env values** — deliberate secret hygiene);
per-mutant lines carry `ID:` and (when blacklisted-eligible) `Blacklist
checksum:`; escapes carry unified diffs; final block = counts, score, sorted
per-mutator table ([cli.md](mutarust/docs/cli.md)). §8.1's command-only list
omitted `--logger-gitlab`, `--html-output`, `--quiet`, `--output-statuses`,
`--no-diffs`, `--verbose`, `--debug`, `--do-not-remove-tmp-folder` — all
present here. **Humans:** `--quiet`/`--output-statuses ex` is the interactive
triage default. **Agents:** default per-mutant lines are stable, grep-able,
and control-char-free when piped; `--silent` + JSON loggers is the parse-not-
read mode.

### 9.11 Machine-readable outputs — exact shapes ([json-outputs.md](mutarust/docs/json-outputs.md), `schema/`)

All scores in JSON are **ratios 0–1** while `--min-msi` takes **0–100
integers**; paths are repository-relative with `/`; reports write only after
a **completed** run; a baseline update writes nothing; files land in **CWD**.

| File | Enable | Schema-required keys | Notes |
| --- | --- | --- | --- |
| `report.json` | `json_output: true` | `metadata{version,hasCoverage,oneMutant}`, `stats`, `escaped[]`, `killed[]`, `errored[]` | Optional-if-empty: `skipped[]`, `notCovered[]`, `generated[]` (dry-run/no-exec results), `mutatorStats[]`. Mutant = `{id, blacklistChecksum, mutator{mutatorName, originalFilePath, originalStartLine}, diff, processOutput?}`; IDs/checksums `^[0-9a-f]{32}$`; `additionalProperties: false`; `stats` `$ref`s `summary.schema.json` (ship both files when validating) |
| `mutarust-summary.json` | `--logger-summary-json` | all 8: `totalMutantsCount, killedCount, notCoveredCount, escapedCount, errorCount, skippedCount, msi, coveredCodeMsi` | Fixed compact shape; ratios — "badges and dashboards" |
| `mutarust-agentic.json` | `--logger-agentic-json` | `generated_at` (RFC 3339), `msi`, `escaped_count`, `reminder`, `mutants[]` | Per mutant: `id, file, line, mutator, description, kill_hint, diff, context_start_line, context_lines[], test_files[]` — all always present (empty list / 0 when unknown). `reminder` embeds write-the-test guidance ("don't assert on the mutant; ask what a caller would observe") |
| `mutarust-gitlab.json` | `--logger-gitlab` | array of `{type:"issue", check_name, description, severity:"minor", fingerprint, location{path, lines{begin}}}` | Empty array on clean run |
| GitHub annotations | `--logger-github` | stdout only, no file | `::warning file=…,line=…,title=Mutant escaped (mutator)::message`, percent-escaped, 1-based lines |
| `mutarust-report.html` | `html_output`/`--html-output` | — | Self-contained (inlined CSS/JS); empty run writes "No escaped mutants." |

**Design target for the mise task:** `--logger-agentic-json
--logger-summary-json` + exit code. `mutarust-agentic.json` is purpose-built
for this repo's agent audience — it is the machine-readable successor of
`.mise/tasks/mutants/report`'s "Instructions for Next Agent Session"
prose: stable IDs, unified diff, nearby context lines, **nearby test files**,
and a kill hint per escape. The summary file is the gate/monitoring shape.
`report.json` (generic filename, CWD) is the triage shape — and the one most
likely to collide with other tools; gitignore it (§8.3).

### 9.12 Exit codes — consolidated contract

| Code | Condition |
| --- | --- |
| 0 | run completed and all gates pass; also empty `--git-diff-lines` result (with `--ignore-msi-with-no-mutations`); also `--update-baseline` (writes and exits before gates) |
| 1 | command error — the generic failure class (Git repo/base resolution failures "stop with an error" per cli.md; the docs don't tag their code explicitly) |
| 2 | bash completion mode (`scripts/mutarust-bash_completion.sh` sets `GO_FLAGS_COMPLETION=1` — a Go leftover) |
| 3 | source parse / config / annotation error (path + line) |
| 4 | quality gate: score **strictly below** `min_msi` (equality passes), covered score below `min_covered_msi`, positive covered gate without `--coverage`, or new escape under `--fail-on-escaped` |

([cli.md](mutarust/docs/cli.md), [parity.md](mutarust/docs/parity.md),
[ci.md](mutarust/docs/ci.md)). This is the most agent-friendly property in the
whole tool: **four is the only failure class worth retrying with a narrower
scope**; 1/3 mean fix inputs, 2 is not a run.

### 9.13 Config schema vs docs — drift found

`schema/mutarust.schema.json` matches [config.md](mutarust/docs/config.md)'s
eleven fields exactly (`additionalProperties: false`; booleans default
`false`; scores integer 0–100; list defaults `[]`) and
[mutarust.yml.example](mutarust/mutarust.yml.example) uses the same set.
Drift/footguns:
1. **Schema mutator-name pattern forbids `_`** but built-ins
   `arithmetic/assign_invert` and `loop/range_break` contain it — schema-based
   validation rejects valid `enable_mutators`/`disable_mutators` entries
   (docs say runtime validates names against the built-in list, so the CLI
   likely accepts them; the *schema* is wrong).
2. **`mutarust.yml.example` sets `min_msi: 80` and `min_covered_msi: 80`** —
   above ci.md's own "stable library" total tier (75), and the covered gate
   forces `--coverage` or the run exits 4. Copying the example verbatim is a
   trap (§8.3's proposed config deliberately starts gate-less).
3. **Stray copy:** `docs/refs/messrust/mutarust.yml.example` exists and is
   **identical except `min_msi: 75`** (vendored copy: 80) — a cross-vendored
   inconsistency, no other differences (cheap `diff` checked).

### 9.14 CI/automation extras not covered earlier

- `--logger-github` / `--logger-gitlab` formats (§9.11) — the prior note
  listed the flags but not their shapes.
- Upstream ships optional `githooks/`: `pre-commit` mirrors fast
  format/Clippy/messrust; **`pre-push` runs changed-line self-mutation
  against `origin/main`** ([ci.md](mutarust/docs/ci.md)) — direct precedent
  for a local `--git-diff-lines` gate in *this* repo's `hk` setup (still not
  proposed in §8.3, but it is the documented upstream pattern).
- Selective CI: path-derived `*_or_unknown` flags, `run_mutation` PR
  detection, **`ci-full` label** to force the heavy suite; mutation scope on
  main = full approved production scope, PRs always `--git-diff-lines`
  ([selective-ci.md](mutarust/docs/selective-ci.md)). Relevant only if this
  repo ever adds a mutation CI job (§5: none exists today).
- `docs/release.md`/`docs/index.md` are maintainer-facing (self-mutation in
  upstream CI, package/install smoke) — no capability for this repo.

### 9.15 Custom mutators (library API)

[custom-mutators.md](mutarust/docs/custom-mutators.md): `mutarust` is also a
library — public `Mutator` trait (`name()` lower-case slash path, `mutations
(&str) -> Vec<Mutation>`), `Mutation::new(range, replacement)`,
`RegistryBuilder::with_builtins().register(…)`, `run_mutation_tests(…)` with
`run.mutation_score()`. You build a replacement binary. **Value here:** niche
but real — PKM-specific operators no generic tool has (e.g. flip a date-format
verb in `src/template/engine/date.rs`, or a query-grammar token in
`src/query/grammar/`) could target historically weak parsing code. Cost: a
custom binary to build/pin, and upstream's own guidance says prove operators
on a fixture crate before CI. Low priority; noted as an escape hatch neither
cargo-mutants nor the prior note offers.

### 9.16 Grounding in this workspace

- **Module surface** (`src/lib.rs` mod list): config, date, delimiter,
  dialog, dirs, dirtree, duration, env_vars, field, file,
  file_class_expander, file_tracker, hash, index, lexer, note, path,
  position, query, schema, strsim, tag, task, template + `pub mod cli`.
- **Current exclusions remove ~8 k lines** from mutation: `src/cli/**`
  (7,784 lines incl. `cli/error.rs` 1,783 and `cli/mod.rs` 1,598),
  `src/main.rs`, `src/**/error.rs` — a mutarust port must reproduce this via
  `exclude_dirs: ["src/cli"]` + something for the two loose files (§8.2 gap).
- **`exclude_re` port gap is large:** 44 `src/` files contain
  `impl.*(Debug|Display|Default|PartialEq|From|Serialize|Deserialize|Visitor)`
  hits that cargo-mutants skips wholesale; mutarust would mutate their bodies
  unless annotated — expect a **noisier first run** than the cargo-mutants
  baseline, mostly in derive-heavy `src/schema/`, `src/config/`, `src/note/`.
- **Highest mutation-cost files** (biggest payoff for `-m`/`-f`/`--git-diff-
  lines` scoping): `index/store.rs` (2,539), `config/service.rs` (2,111),
  `query/results.rs` (2,037), `schema/fields/select.rs` (2,016), `field.rs`
  (1,969), `template/engine/query.rs` (1,964), `note/parser/list.rs` (1,868).
- **Test-feature footgun (critical):** `tests/integration.rs` begins
  `#![cfg(feature = "test-utils")]` — a mutarust run **without**
  `--test-flags "--features test-utils"` compiles the whole integration suite
  to nothing, silently passes, and reports inflated kills from unit tests
  alone. The flag in §8.3's command is mandatory, not optional; benches
  (`required-features = ["test-utils"]`) additionally justify `--all-targets`.
- **`.gitignore`** today knows `/mutants.out/` and `/mutants-report.md` only
  ([.gitignore](../../.gitignore)) — §8.3's mutarust entries remain required.

### 9.17 Capability → opportunity summary

| Capability | Improvement for traces-pkm | Human | Agent |
| --- | --- | --- | --- |
| `mutarust-agentic.json` (+schemas) | replaces prose `mutants-report.md` with structured kill tasks (id/diff/context/test_files/kill_hint) | ★★ read one file | ★★★ parse, plan, verify |
| `--run-mutant-id` | verify a newly written test killed exactly that mutant | ★ | ★★★ closed loop |
| baseline + `--fail-on-escaped` | report becomes a delta, not a re-listing; commit baseline as policy | ★★★ | ★★★ bounded new-escapes task |
| `--git-diff-lines` (+ `--ignore-msi-with-no-mutations`) | cheap changed-code gate, local or future CI | ★★★ | ★★★ safe default (stage first) |
| `--min-msi` / summary JSON | first numeric mutation gate this repo can express (cargo-mutants can't) | ★★ | ★★★ exit 4 + `msi` ratio |
| `--logger-github` / `--gitlab` | only if a mutation CI job is ever added | ★★ | ★★ |
| per-mutator stats + `enable/disable_mutators` | drop zero-yield `select/*`; target weak operator classes per module | ★★ | ★★★ run-diff by operator |
| `--list-files` / `--dry-run` | verify scope & cost before spending minutes | ★★ | ★★★ deterministic integers/paths |
| output controls (`--quiet`, `--output-statuses ex`, `--no-diffs`) | interactive triage | ★★★ | ★ (use JSON instead) |
| `--coverage` / `notCovered[]` | coverage-gap map without running mutants; needs llvm-cov beside tarpaulin | ★★ | ★★★ |
| `--timeout-coefficient`, `--workers` | port of timeout policy (mind the 1 s floor vs old 30 s) | ★ | ★★ |
| annotations `mutator-disable-*` | replace `exclude_re` boilerplate skips, in-source and reviewable | ★★ | ★★ (exit 3 is actionable) |
| custom `--exec` / custom mutators | mostly dead ends here (serial + skipped type-proof; niche operators) | ★ | ★ |

### 9.18 Gaps and footguns the docs never state plainly

1. No config auto-discovery **and** no per-target config: forgetting
   `--config` silently runs zero policy (§2.6/§8.1).
2. `report.json` is a **generic name written to CWD** — collision-prone;
   gitignore all five report files (§8.3).
3. JSON scores are 0–1 ratios, gates are 0–100 integers — mixed units across
   one tool's own interface.
4. `mutatorStats.killed` **includes errored** mutants (Mutago convention) —
   per-operator tables are not directly comparable to the headline
   killed/errored split ([json-outputs.md](mutarust/docs/json-outputs.md)).
5. `exclude_dirs` does not affect `--list-files` — scope previews and actual
   runs can disagree on purpose ([config.md](mutarust/docs/config.md)).
6. Explicit file targets can select **test source**; broad targets cannot —
   target syntax changes semantics ([cli.md](mutarust/docs/cli.md)).
7. Untracked files are invisible to `--git-diff-lines` (§9.6).
8. Example config's covered gate requires `--coverage` (§9.13).
9. Schema mutator pattern rejects `_` (§9.13).
10. Directory-target recursion is ambiguous (`.` vs `./…` vs no target);
    verify with `--list-files` (§9.3).
11. `--match` is unanchored — `"parse"` matches `reparse`, `parse_all`, …
12. Baseline update reports *no* results and skips all gates — an agent
    reading exit 0 must not infer "full run completed with reports" (§9.4).
13. The vendored snapshot itself has one broken internal link
    (`index.md` → `exploratory-testing/2026-09-26-core-journeys.md`, not
    vendored) and its `report.json` example says version `0.2.0` while §1
    recorded `0.1.9` on 2026-09-25 — the snapshot is newer than the prior
    survey; re-verify version-sensitive claims against crates.io before
    pinning.
