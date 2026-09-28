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
- **33 built-in mutators** with stable Mutago names across groups
  `arithmetic/`, `branch/`, `conditional/`, `expression/`, `loop/`,
  `numbers/`, `statement/`, `concurrency/`, `select/`, `value/`, etc.
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
  / `--timeout`), or adaptive via `--timeout-coefficient` (clean-suite
  duration × factor)
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
  source tree and docs returned zero hits. The only way to use nextest is the
  custom `--exec COMMAND` escape hatch (exit 0 = kill, 1 = escape, 2 = skip)
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
- This is the *same coverage engine* this repo's stack already uses
  ([crap_metric.md](crap_metric.md)); `cargo-llvm-cov` is declared in
  `mise.toml`, so `--coverage` adds no new tool.

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
([.mise/tasks/mutants/report](../../.mise/tasks/mutants/report)), the
module-scoped task UX with `-m/-f/--check` flags
([.mise/tasks/mutants/_default](../../.mise/tasks/mutants/_default)),
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
- No extra tool for `--coverage`: `"cargo:cargo-llvm-cov" = "latest"` is
  already declared in this repo's `mise.toml`
  ([docs/cli.md](https://github.com/quality-gates/mutarust/blob/main/docs/cli.md)).

---

## 5. How it would slot into this repo's gates

- **Where mutation currently lives:** file task
  `.mise/tasks/mutants/_default` (extends `[task_templates.mutants]`,
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
6. **Coverage tool split (resolved):** `--coverage` pulls in
   `cargo-llvm-cov`, the same engine this repo's coverage gates already use —
   no second coverage engine enters `mise.toml`
   ([crap_metric.md](crap_metric.md)).
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
