# messrust — PHPMD-style Mess Detector for Rust

Research into [`quality-gates/messrust`](https://github.com/quality-gates/messrust)
as a candidate quality gate for this repo: what it is, how it runs, how it
differs from the existing stack (cargo-mutants, clippy, cargo-crap), and how it
would be installed via mise.

---

## 1. What messrust is

**Purpose.** `messrust` is a static "mess detector" for Rust — a local CLI that
catches maintainability problems "before they calcify: oversized functions and
types, tangled dependencies, dead private code, muddy naming, and other mess
that reviews keep rediscovering." It "parses Rust source, never builds or runs
your project, and needs no project dependencies installed"
([README](https://github.com/quality-gates/messrust/blob/main/README.md)).
The package self-describes as "A PHPMD-style mess detector for Rust source
code" ([Cargo.toml](https://github.com/quality-gates/messrust/blob/main/Cargo.toml)).

**Name origin.** The repository never states an explicit etymology. The only
first-party statement tying the name to a lineage is the `description` field
"PHPMD-style mess detector for Rust source code" plus the GitHub repo
description "Mess detector for Rust." ([README](https://github.com/quality-gates/messrust/blob/main/README.md),
[repo metadata](https://api.github.com/repos/quality-gates/messrust)). The name
reads as *mess* (from that phrase) + *rust*; anything beyond that is inference
and is not claimed here.

**What it measures/checks.** Rule inventory read directly from the shipped
rulesets ([rulesets/](https://github.com/quality-gates/messrust/tree/main/rulesets)):

| Ruleset | Rules |
| --- | --- |
| `codesize` | CyclomaticComplexity, NPathComplexity, ExcessiveClassComplexity, ExcessiveClassLength, ExcessiveMethodLength, ExcessiveParameterList, ExcessivePublicCount, TooManyFields, TooManyMethods, TooManyPublicMethods |
| `naming` | LongVariable, ShortVariable, LongClassName, ShortClassName, ShortMethodName, ConstantNamingConventions, BooleanGetMethodName |
| `unusedcode` | UnusedLocalVariable, UnusedFormalParameter, UnusedPrivateField, UnusedPrivateMethod |
| `cleancode` | BooleanArgumentFlag, ElseExpression, IfStatementAssignment, StaticAccess, DuplicatedArrayKey |
| `design` | ExitExpression, EmptyCatchBlock, CouplingBetweenObjects, LackOfCohesionOfMethods, GlobalVariable, GotoStatement, CountInLoopExpression, DevelopmentCodeFragment |
| `controversial` | CamelCaseClassName/MethodName/ParameterName/PropertyName/VariableName |
| `explicitness` | ImplicitInput, ImplicitOutput |

Policies compose: `rust` is the recommended low-noise default (it pulls in
`codesize`, `naming`, `unusedcode`, `cleancode`, `design`, `controversial` with
several exclusions), `opinionated` adds the checks `rust` deliberately omits
([rulesets/rust.xml](https://github.com/quality-gates/messrust/blob/main/rulesets/rust.xml),
[rulesets/opinionated.xml](https://github.com/quality-gates/messrust/blob/main/rulesets/opinionated.xml)).

**Maturity** (queried 2026-09-25):

- Version `0.1.12` ([Cargo.toml](https://github.com/quality-gates/messrust/blob/main/Cargo.toml)),
  13 GitHub releases, v0.1.12 published 2026-09-23
  ([releases API](https://api.github.com/repos/quality-gates/messrust/releases)).
- Created 2026-07-31, last push 2026-09-25 — very active: 257 commits, releases
  roughly every 1–2 days during September 2026
  ([repo metadata](https://api.github.com/repos/quality-gates/messrust),
  [commits](https://api.github.com/repos/quality-gates/messrust/commits)).
- License: MIT ([README badge / LICENSE](https://github.com/quality-gates/messrust/blob/main/LICENSE),
  [repo metadata](https://api.github.com/repos/quality-gates/messrust)).
- Traction: **0 stars, 0 forks**, 12 open issues, single visible author
  (`jonbaldie` in release notes) — effectively a one-person project under an
  org (`quality-gates`) created for it
  ([repo metadata](https://api.github.com/repos/quality-gates/messrust)).
- crates.io: only **0.1.0** published (2026-08-01), 306 total downloads — the
  registry copy is 12 releases behind GitHub
  ([crates.io API](https://crates.io/api/v1/crates/messrust/versions)).
- Self-gating: the project runs messrust on its own source in CI and treats
  exit 2 as failure ([README](https://github.com/quality-gates/messrust/blob/main/README.md),
  [ci.yml](https://github.com/quality-gates/messrust/blob/main/.github/workflows/ci.yml)).

---

## 2. How it works technically

**Inputs and invocation.** One positional command shape
([docs/usage.md](https://github.com/quality-gates/messrust/blob/main/docs/usage.md)):

```console
messrust <path[,path...]> <format> <ruleset[,ruleset...]> [options]
```

Example: `messrust src text rust --ignore-tests`. Paths are comma-separated
files/directories, walked recursively for `.rs` suffixes; `.git`, `target`, and
`node_modules` are skipped. **There is no default ruleset** — `rust`, a
component name, or an XML path must be passed explicitly. Discovery, analysis,
and reporting are pure source parsing: dependencies are `syn`, `proc-macro2`,
`walkdir`, `roxmltree`, `serde`/`serde_json`, `regex`, `libc`
([Cargo.toml](https://github.com/quality-gates/messrust/blob/main/Cargo.toml)) —
no rustc, no build, no project dependencies required
([README](https://github.com/quality-gates/messrust/blob/main/README.md)).

**Config file.** There is **no project config file** (no `.messrust.toml` or
equivalent is documented in [docs/usage.md](https://github.com/quality-gates/messrust/blob/main/docs/usage.md)
or present in the repo tree — searched). Policy is supplied per invocation as
built-in ruleset names and/or a path to an XML ruleset using the PMD ruleset
schema (`xmlns="http://pmd.sf.net/ruleset/1.0.0"`), with `<rule ref=...>`,
`<exclude name=...>`, `<priority>` and `<property>` overrides; nested references
resolve relative to the current file
([docs/usage.md](https://github.com/quality-gates/messrust/blob/main/docs/usage.md),
[rulesets/rust.xml](https://github.com/quality-gates/messrust/blob/main/rulesets/rust.xml)).
Practical consequence: a team policy file can live in the repo and be passed by
path (`messrust src text path/to/team-policy.xml --ignore-tests`).

**Output formats.** `text`, `ansi`, `json`, `xml`, `html`, `github` (Actions
line annotations), `gitlab` (Code Quality), `checkstyle`, `sarif`
([docs/usage.md](https://github.com/quality-gates/messrust/blob/main/docs/usage.md)).
`--reportfile <path>` writes to a file and leaves stdout empty; structured
formats carry path/line/rule/priority/message/context/suppression fields and
sort by file and line.

**Exit codes** ([docs/usage.md](https://github.com/quality-gates/messrust/blob/main/docs/usage.md)):

| Code | Meaning |
| ---: | --- |
| 0 | Clean (or findings/errors explicitly ignored) |
| 1 | Tool/config/discovery/source-processing error (takes precedence) |
| 2 | Findings present, no non-ignored processing error |

Adoption helpers: `--ignore-violations-on-exit` returns 0 while still reporting
findings (useful while triaging); `--strict` keeps suppressed findings visible.

**Suppression.** In-source `// messrust-disable-next-line Rule` and
`messrust-disable` / `messrust-enable` regions, rule names case-insensitive
([README](https://github.com/quality-gates/messrust/blob/main/README.md)).

**Test exclusion semantics.** `--ignore-tests` skips test files/dirs,
`#[cfg(test)]` modules, and test-only methods — and with it, test code does not
inflate a production type's size/coupling/cohesion metrics
([docs/usage.md](https://github.com/quality-gates/messrust/blob/main/docs/usage.md)).

**CI integration.** The README shows GitHub Actions (`cargo install messrust
--locked` + `messrust src github rust --ignore-tests`) and GitLab Code Quality
(`--reportfile gl-code-quality-report.json`) snippets; the project's own
[ci.yml](https://github.com/quality-gates/messrust/blob/main/.github/workflows/ci.yml)
builds a release binary then runs self-analysis on every push/PR, failing the
job on exit 2.

---

## 3. Overlap with the existing stack

Existing stack: strict clippy (`[lints.clippy]` in
[Cargo.toml](../../Cargo.toml) —
`complexity`/`correctness`/`perf`/`suspicious` denied, `pedantic`/`style` warn,
plus ~30 individually denied lints), `cargo-crap` + `cargo-llvm-cov`,
`cargo-mutants`, `cargo-deny`/`cargo-audit`/`gitleaks`, `nextest`, all behind
mise tasks and `hk` gates ([mise.toml](../../mise.toml),
[hk.pkl](../../hk.pkl)).

**Verdict: complementary, with a redundant tail.** messrust occupies a slot
nothing in the current stack fills — *type/aggregate-level structural mess* —
while several of its rules duplicate lints this repo already denies.

### 3.1 Genuine gaps (messrust-only)

- **Type-level size/structure**: `ExcessiveClassLength`,
  `ExcessiveClassComplexity`, `TooManyMethods`, `TooManyPublicMethods`,
  `ExcessivePublicCount`, `TooManyFields`, `ExcessiveClassLength` — clippy has
  no aggregate-per-type lints (this repo's denials are function- or
  item-level: `too_many_lines`, `fn_params_excessive_bools`,
  `struct_excessive_bools` — Cargo.toml:183/193/195). No verified clippy
  equivalent exists for "this struct has 40 methods".
- **Coupling & cohesion**: `CouplingBetweenObjects`,
  `LackOfCohesionOfMethods` — no clippy or cargo-crap equivalent (CRAP is
  per-function complexity×coverage, not a dependency-graph metric; formula in
  [docs/refs/crap_metric.md](crap_metric.md)).
- **Identifier length limits**: `LongVariable` (max 35 in `rust`),
  `LongClassName`, `ShortMethodName`. Clippy only has *minimum* length
  (`min_ident_chars` is deliberately `allow` in this repo, Cargo.toml:231) —
  no max-length lint.
- **`explicitness` ruleset** (`ImplicitInput`/`ImplicitOutput`): flags
  functions with hidden inputs/outputs — no clippy equivalent
  ([docs/explicitness.md](https://github.com/quality-gates/messrust/blob/main/docs/explicitness.md)).
- **`DevelopmentCodeFragment`** (leftover dev markers in source comments) —
  this repo denies the `todo!` *macro* (Cargo.toml:194) but does not scan
  comment fragments.

### 3.2 Redundant with what this repo already gates

| messrust rule | Already covered here by |
| --- | --- |
| `ExcessiveMethodLength` | `clippy::too_many_lines = deny` (Cargo.toml:195) |
| `ExcessiveParameterList` | function-arity checks in the denied `complexity` clippy group (Cargo.toml:167) — exact member lint not independently verified |
| `BooleanArgumentFlag` | `fn_params_excessive_bools`/`struct_excessive_bools = deny` (Cargo.toml:183/193) — partial |
| `ElseExpression` | `redundant_else` + `else_if_without_else = warn` under `-D warnings` (Cargo.toml:205/215) |
| `ExitExpression` | `clippy::exit = deny` (Cargo.toml:181) |
| `UnusedLocalVariable`, `UnusedPrivateField`, `UnusedPrivateMethod` | rustc `unused_variables` / `dead_code`, hardened by `-D warnings` in the lint task |
| `controversial` CamelCase* rules | rustc `non_snake_case` / `non_camel_case_types` (warn by default, denied via `-D warnings`) |
| `CyclomaticComplexity` / `NPathComplexity` | partially — `clippy::cognitive_complexity = warn` with `cognitive-complexity-threshold = 25` (Cargo.toml:204, [docs/refs/clippy_lint_config.md](clippy_lint_config.md)); different metric (cognitive vs cyclomatic), so not a strict duplicate |
| `GotoStatement` | dead weight in Rust (no `goto` in the language) — a PHPMD/PMD legacy rule |

### 3.3 vs cargo-mutants and cargo-crap

- **cargo-mutants** ([docs/refs/mutation_testing.md](mutation_testing.md)):
  different axis entirely — mutants measure *test suite effectiveness*,
  messrust measures *static structure*. Zero overlap; both can coexist.
- **cargo-crap** ([docs/refs/crap_metric.md](crap_metric.md)): shares the
  word "complexity", but CRAP = cyclomatic² × (1−coverage)³ + cyclomatic,
  per function, requiring coverage runs; messrust is coverage-free structural
  thresholds. Complementary: CRAP ranks risky-under-change functions,
  messrust flags structural mess types CRAP cannot see (coupling, cohesion,
  type size). Neither subsumes the other.

---

## 4. Installation path via mise

**Verified facts** (all primary):

- Crate name = binary name = package name = `messrust`
  ([Cargo.toml](https://github.com/quality-gates/messrust/blob/main/Cargo.toml),
  `[[bin]] name = "messrust"`).
- Published on crates.io but only at **0.1.0** (2026-08-01), 12 releases behind
  GitHub's v0.1.12
  ([crates.io versions API](https://crates.io/api/v1/crates/messrust/versions)).
- GitHub releases carry **darwin-only** assets — `messrust_<v>_darwin_amd64.tar.gz`,
  `messrust_<v>_darwin_arm64.tar.gz`, `checksums.txt` — explicitly by design:
  "The immutable GitHub release contains only these assets" built on "Native
  Intel and Apple Silicon runners"
  ([docs/homebrew-release.md](https://github.com/quality-gates/messrust/blob/main/docs/homebrew-release.md),
  verified across all 13 releases via the
  [releases API](https://api.github.com/repos/quality-gates/messrust/releases)).
  **No Linux or Windows binaries exist.**
- Repo layout is a single binary crate at the root (no workspace), so the
  cargo git-install form needs no `crate =` selector
  ([Cargo.toml](https://github.com/quality-gates/messrust/blob/main/Cargo.toml)).

**Candidate `[tools]` lines:**

| Line | Result | Verdict |
| --- | --- | --- |
| `"cargo:messrust" = "latest"` | Installs crates.io **0.1.0** — misses 12 releases of rules/format fixes | ✗ Stale |
| `"github:quality-gates/messrust" = "latest"` | Prebuilt v0.1.12, no compile — but **fails on Linux/Windows** (no assets); plausible on this Mac, unverified (asset autodetection against `*_darwin_arm64.tar.gz` is mise behavior I did not execute) | ✗ Not CI-viable |
| `"cargo:https://github.com/quality-gates/messrust" = { version = "tag:v0.1.12" }` | `cargo install --git --tag v0.1.12` — current source, works on any OS with a Rust toolchain; mirrors the repo's existing rust-docs-mcp pattern ([mise.toml:41](../../mise.toml)) | ✓ **Recommended** |

**Recommended `mise.toml` line:**

```toml
"cargo:https://github.com/quality-gates/messrust" = { version = "tag:v0.1.12" }
```

(mise cargo-backend git syntax documented at
[mise.jdx.dev/dev-tools/backends/cargo.html](https://mise.jdx.dev/dev-tools/backends/cargo.html);
`tag:` is one of `tag:`/`branch:`/`rev:`. `locked` (use the crate's committed
`Cargo.lock`) defaults to on. The `github:` backend behavior cited above is
from [mise.jdx.dev/dev-tools/backends/github.html](https://mise.jdx.dev/dev-tools/backends/github.html).)

**Caveats:**

- **Source build**: compiles `syn` + deps from scratch at install time
  (~1 min class; not timed here). Requires Rust — satisfied by the repo's
  mise-managed nightly (`mise.toml` `rust = nightly-2026-09-09`); the crate is
  `edition = "2021"`, CI builds it on stable, so a nightly install should work
  but was **not executed** in this research.
- **`install_env`**: none expected (no build-script/native-dep requirements
  beyond the `libc` crate, which the tool's own CI exercises only on Linux for
  source builds). Windows compatibility of the binary is **unverified** —
  upstream never ships Windows artifacts.
- **Pin, don't float**: upstream is pre-1.0 with near-daily releases; `"latest"`
  or `branch:` would move constantly. `mise.toml` has `lockfile = true`
  ([mise.toml](../../mise.toml)), so record the entry and run `mise lock`.
- Alternative if only local macOS use matters: the Homebrew tap
  `brew install quality-gates/tap/messrust`
  ([README](https://github.com/quality-gates/messrust/blob/main/README.md)) —
  but that bypasses mise, which this repo forbids as an install mechanism.

---

## 5. How it would slot into this repo's gates

**Runtime cost.** Parse-only over ~80k lines of `src/` (plus `tests/` if not
`--ignore-tests`): no compile, no test execution, no dependency resolution
([docs/usage.md](https://github.com/quality-gates/messrust/blob/main/docs/usage.md)).
Upstream publishes **no benchmark** — treat "seconds" as an informed guess, not
a verified figure. Structurally it is orders of magnitude cheaper than
`cargo-mutants` and comparable in class to clippy-without-codegen.

**1h mutants template**: unrelated — `[task_templates.mutants]` with
`timeout = "1h"` only binds tasks that `extends = "mutants"`
([mise.toml:279](../../mise.toml), [`.mise/tasks/mutants/_default`](../../.mise/tasks/mutants/_default)).
A messrust task inherits nothing from it; give it its own short timeout or none.

**Integration options, cheapest first:**

1. **Advisory mise task** (recommended first step) — a file task
   `.mise/tasks/mess/_default` (pattern matches `.mise/tasks/mutants/_default`)
   or a TOML task:

   ```toml
   [tasks.mess]
   description = "Static mess detection with messrust"
   sources = ["@group:rust"]
   run = "messrust src text rust --ignore-tests"
   ```

   `sources = ["@group:rust"]` plugs into the existing freshness cache
   ([mise.toml:58-67](../../mise.toml)).

2. **hk gate** — add a step to the `quality` mapping in
   [hk.pkl:107](../../hk.pkl) alongside `cargo-clippy`:

   ```pkl
   ["messrust"] = new Step {
       glob = List("**/*.rs", "**/Cargo.toml")
       check = "mise run mess"
   }
   ```

   That puts it in the `quality` group used by both `pre-push` and the
   `check` hook ([hk.pkl:133-163](../../hk.pkl)) — which means it also runs
   inside `mise run check` and therefore `mise run verify`
   ([mise.toml:83-104, 260-270](../../mise.toml)). Exit 2 fails the gate,
   exactly like the other steps.

3. **CI** — [.github/workflows/ci.yml](../../.github/workflows/ci.yml)
   sets `MISE_AUTO_INSTALL: "0"` and installs tools explicitly per job with
   `mise install --locked <tools>` (lines 69, 85, 108, 125). Two consequences:
   - `messrust` must be added to the `[tools]` entry **and** to the relevant
     job's `mise install --locked …` list, plus a `mise lock` commit.
   - The `hygiene` job runs on **ubuntu-latest** (line 75): only the `cargo:`
     git form works there — another reason the `github:` backend is excluded.
     Whether upstream's self-analysis job translates to this codebase (their
     default `rust` ruleset on our source) can only be known by running it.

**Relation to the existing `verify` chain.** `verify` = `fmt` → parallel
`check`/`lint`/`test`. Adding messrust via the hk `quality` group lands it in
`check`, i.e. automatically in `verify` — no change to the verify task itself.
If it proves noisy at first, keep it out of hk and run it as a standalone
`mise run mess` until the policy is tuned, or run it with
`--ignore-violations-on-exit` during adoption
([docs/usage.md](https://github.com/quality-gates/messrust/blob/main/docs/usage.md)).

---

## 6. Risks, caveats, recommendation

**Risks**

1. **Pre-1.0 churn**: 0.1.0 → 0.1.12 in ~8 weeks, with breaking-ish activity
   (report pipeline rewrite landed the day this research ran:
   "Deepen report rendering into typed format pipeline", 2026-09-25
   [commits](https://api.github.com/repos/quality-gates/messrust/commits)).
   Rules and formats may change under you; pin the tag.
2. **Known false positives still open** (repo's own issue tracker,
   [issues](https://api.github.com/repos/quality-gates/messrust/issues)):
   #184 raw identifiers misflagged by controversial rules, #183 format-macro
   interpolation misflagged as unused code, #160 `ExcessivePublicCount`/
   `TooManyPublicMethods` counting `pub(crate)` as public, #185 anonymous
   constants flagged, #100 report rows dropped in html/sarif/gitlab/checkstyle
   on processing errors. A hard gate on a codebase full of `pub(crate)` and
   raw-ish identifiers would be premature.
3. **Distribution gaps**: crates.io stale at 0.1.0; GitHub releases are
   darwin-only by design — so the two "obvious" mise install forms are
   respectively stale and non-portable (§4).
4. **Bus factor / trust**: 0 stars, 0 forks, single maintainer, org created
   days before the repo. The tool reads your entire `src/` — but it does run
   locally with no network and no build (README), so supply-chain exposure is
   limited to the install step (mitigated by `mise.lock` checksums on the git
   source and `--locked` cargo builds).
5. **Noise-vs-clippy tension**: `controversial` and parts of `naming`/
   `unusedcode` restate rustc lints this repo already denies — running the
   full `rust` ruleset duplicates signal and wastes review attention; a
   trimmed repo XML policy (drop the redundant rules, keep `codesize` +
   coupling/cohesion + explicitness) would be the real configuration work.
6. **Upstream self-gate ≠ our gate**: upstream's clean run on *their* source
   says nothing about findings on traces-pkm's ~80k lines.

**Recommendation: adopt later (trial now, gate after tuning).**

Reasoning: the tool covers a real gap — type-level mess, coupling/cohesion,
long-name limits, explicitness — that clippy, cargo-crap, and cargo-mutants
structurally cannot detect (§3), costs almost nothing to run (parse-only,
no build), and slots cleanly into existing mise/hk/CI wiring (§5). But three
things argue against turning it on as a hard gate today: pre-1.0 churn with
documented false-positive bugs that touch idioms this repo uses daily
(`pub(crate)`, macro-heavy code), a significant redundant tail that needs a
custom ruleset before it says anything clippy doesn't, and an immature
distribution story (stale crates.io, darwin-only binaries) that forces the
slower `cargo:`-git install path anyway.

Staged path:

1. Install via `"cargo:https://github.com/quality-gates/messrust" = { version = "tag:v0.1.12" }`,
   `mise lock`, no gate changes.
2. Run `messrust src text rust --ignore-tests`, review findings; write a repo
   XML policy that drops the §3.2-redundant rules and keeps codesize,
   coupling/cohesion, and explicitness.
3. Add the advisory `mise run mess` task; only then add the hk `quality` step
   and the CI install line.
4. Re-evaluate for a hard gate once upstream leaves 0.1.x or the open
   false-positive issues (#160, #183, #184) are closed.

---

## Primary sources checked

Queried 2026-09-25:

- README, `Cargo.toml`, `LICENSE`, `docs/usage.md`, `docs/homebrew-release.md`,
  `docs/{codesize,naming,unusedcode,cleancode,design,controversial,explicitness}.md`,
  `rulesets/{rust,opinionated,codesize,...}.xml`, `src/` tree,
  `.github/workflows/{ci,release,mutation}.yml` —
  https://github.com/quality-gates/messrust
- GitHub API: repo metadata, commits, releases, tags, issues —
  https://api.github.com/repos/quality-gates/messrust
- crates.io API (crate + versions): https://crates.io/api/v1/crates/messrust
- mise backend docs (cargo git syntax, github asset autodetection):
  https://mise.jdx.dev/dev-tools/backends/cargo.html,
  https://mise.jdx.dev/dev-tools/backends/github.html
- This repo (integration surface): `mise.toml`, `hk.pkl`, `Cargo.toml`
  `[lints]`, `.github/workflows/ci.yml`, `.mise/tasks/`, `docs/refs/mutation_testing.md`,
  `docs/refs/crap_metric.md`, `docs/refs/clippy_lint_config.md`

Not verifiable from primary sources (stated as such in text above): messrust's
runtime on this codebase, Windows binary compatibility, whether mise's `github:`
asset autodetection accepts the `*_darwin_arm64.tar.gz` naming, and any etymology
of the name beyond the "PHPMD-style mess detector" description.
