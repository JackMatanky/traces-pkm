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
plus ~30 individually denied lints), `cargo-crap` + `cargo-tarpaulin`,
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
| `ExcessiveParameterList` | `clippy::too_many_arguments` in the denied `complexity` group (Cargo.toml) at `too-many-arguments-threshold = 5` (clippy.toml) — **verified** [confirmed in §8 review]; strictly stricter than messrust's ≥10 |
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

---

## 7. Configuration & day-to-day usage (follow-up)

Follow-up research on *how to configure and leverage* messrust locally (not in
CI). Primary sources: upstream
[docs/usage.md](https://github.com/quality-gates/messrust/blob/main/docs/usage.md),
[rulesets/rust.xml](https://github.com/quality-gates/messrust/blob/main/rulesets/rust.xml)
(read in full — the exact preset contents below come from that file), plus this
repo's `mise.toml`, `.mise/tasks/mutants/_default`, `.cargo/mutants.toml`.

### 7.1 Configuration mechanics (verified)

- **No config file.** Policy is per-invocation: built-in ruleset names and/or a
  path to a custom XML file, comma-separated:
  `messrust <paths> <format> <ruleset[,ruleset...]> [options]`
  ([docs/usage.md](https://github.com/quality-gates/messrust/blob/main/docs/usage.md)).
  A repo-committed XML passed by path *is* the supported team-policy mechanism.
- **XML policy composition** (PMD schema `xmlns="http://pmd.sf.net/ruleset/1.0.0"`):
  - `<rule ref="rust">` / `ref="codesize"` / `ref="rulesets/x.xml"` / relative
    file path — refs nest; **later refs override earlier priority/properties**.
  - `<exclude name="RuleName"/>` removes a rule from the referenced set.
  - `<priority>N</priority>` and `<properties><property name="maximum"
    value="50"/></properties>` override thresholds per rule.
- **Run-time filters** (after composition): `--only`/`--enable` (keep only
  named loaded rules — bisect noisy runs), `--disable` (drop rules without
  editing XML), `--minimumpriority`/`--maximumpriority`, `--exclude
  a,b` (path substrings), `--suffixes`.
- **Suppression:** in-source `// messrust-disable-next-line Rule` and
  `disable`/`enable` regions (case-insensitive); `--strict` keeps suppressed
  findings visible so exceptions stay auditable.
- **Adoption helper:** `--ignore-violations-on-exit` → exit 0 despite
  findings (report stays complete) — the triage-then-harden lever.
- **`--ignore-tests`** is mandatory for a production-code gate: skips test
  files, `#[cfg(test)]` modules/impls, and stops test code inflating a
  production type's size/coupling/cohesion metrics
  ([docs/usage.md](https://github.com/quality-gates/messrust/blob/main/docs/usage.md)).

### 7.2 The `rust` preset, and what this repo's policy should drop/keep

`rulesets/rust.xml` composes (verified from source): `codesize` (all 10
rules) + `naming` minus `ShortVariable`/`LongVariable` (re-included with
`maximum=35`) + `unusedcode` minus `UnusedFormalParameter` + `cleancode` minus
`BooleanArgumentFlag`/`ElseExpression`/`StaticAccess` + `design` minus
`CountInLoopExpression`/`ExitExpression` + `controversial` (all). `explicitness`
is opt-in, in neither preset.

Proposed repo policy (`messrust.xml`, root — matches per-tool config
convention of `clippy.toml`/`deny.toml`/`tarpaulin.toml`):

| Rule (in `rust` preset) | Decision | Reason |
| --- | --- | --- |
| `controversial` CamelCase×5 | **exclude** | dup of rustc `non_snake_case`/`non_camel_case_types` already failing under `-D warnings` (§3.2); also known-FP bug #184 (raw identifiers) |
| `UnusedLocalVariable`, `UnusedPrivateField`, `UnusedPrivateMethod` | **exclude** | dup of rustc `unused_variables`/`dead_code`; also FP bug #183 (format-macro interpolation) |
| `ExcessiveMethodLength` | **exclude** | dup of `clippy::too_many_lines = deny` |
| `GotoStatement` | **exclude** (cosmetic) [corrected in §8 review] | upstream documents it as a quiet compatibility rule that "never reports" ([docs/design.md](messrust/docs/design.md)) — exclusion changes nothing either way |
| `ExcessivePublicCount`, `TooManyPublicMethods` | **exclude for now** | FP bug #160 counts `pub(crate)` as public — this codebase uses `pub(crate)` heavily; re-enable when #160 closes |
| `codesize` rest (`ExcessiveClassLength`, `ExcessiveClassComplexity`, `TooManyMethods`, `TooManyFields`, `CyclomaticComplexity`, `NPathComplexity`) | **keep** | type-level aggregate checks clippy has no equivalent for; `CyclomaticComplexity`≠ `cognitive_complexity` (different metric) |
| `ExcessiveParameterList` | **exclude** [corrected in §8 review] | verified redundant: `clippy::too_many_arguments` sits in the denied `complexity` group (Cargo.toml) with `too-many-arguments-threshold = 5` (clippy.toml) — strictly stricter than messrust's ≥10 (§8.11) |
| `naming` `LongVariable`(35)/`LongClassName`/`ShortClassName`/`ShortMethodName`/`BooleanGetMethodName` | **keep** | max-length limits are a genuine gap (clippy only has min-length); `ShortMethodName` needs `exceptions = "at,of,or,eq"` for this codebase (§8.11) |
| `ConstantNamingConventions` | **exclude** [corrected in §8 review] | duplicates rustc `non_upper_case_globals` (warn-by-default, already an error under the lint task's `-D warnings`); upstream naming.md says the default matches that lint (§8.12) |
| `cleancode` `IfStatementAssignment` | **keep** | additive |
| `DuplicatedArrayKey` | **keep — but never fires** [corrected in §8 review] | only reports duplicate struct-literal fields, which rustc rejects (E0124) — zero signal on compilable code; keeping it is harmless, the old "additive" rationale was wrong (§8.12) |
| `design` `CouplingBetweenObjects`, `LackOfCohesionOfMethods`, `EmptyCatchBlock`, `GlobalVariable` | **keep** | the coupling/cohesion core gap (§3.1); `GlobalVariable` never fires here (`static mut` = 0 in src/) |
| `DevelopmentCodeFragment` | **exclude** [corrected in §8 review] | clippy already denies `print_stdout`/`dbg_macro`; `eprintln!` is deliberately `print_stderr = allow` and used as CLI output in 13 production sites; messrust ignores the repo's `#[expect(clippy::…, reason=…)]` justifications (5 more print sites) → ~18 findings of intentional output (§8.12) |
| `unusedcode` `UnusedFormalParameter` | **keep** | already excluded from `rust` preset — re-add only if wanted later |
| `explicitness` (`ImplicitInput`/`ImplicitOutput`) | **opt-in, on demand** [refined in §8 review] | additive gap, but noisy on fs/lock-heavy code — prefer leaving it out of the default XML and running `messrust src text messrust.xml,explicitness` for deep passes (§8.10); the sketch's inline ref still works mechanically |

Resulting policy sketch:

```xml
<?xml version="1.0" encoding="UTF-8"?>
<ruleset name="traces-pkm policy" xmlns="http://pmd.sf.net/ruleset/1.0.0">
  <description>traces-pkm: rust preset minus rules already denied by
  clippy/rustc, minus rules with open upstream FP bugs.</description>
  <rule ref="rust">
    <exclude name="CamelCaseClassName"/>
    <exclude name="CamelCaseMethodName"/>
    <exclude name="CamelCaseParameterName"/>
    <exclude name="CamelCasePropertyName"/>
    <exclude name="CamelCaseVariableName"/>
    <exclude name="UnusedLocalVariable"/>
    <exclude name="UnusedPrivateField"/>
    <exclude name="UnusedPrivateMethod"/>
    <exclude name="ExcessiveMethodLength"/>
    <exclude name="GotoStatement"/>
    <!-- pending upstream #160 (pub(crate) counted as public) -->
    <exclude name="ExcessivePublicCount"/>
    <exclude name="TooManyPublicMethods"/>
    <!-- [corrected in §8 review] redundant or noisy here (§8.12) -->
    <exclude name="ExcessiveParameterList"/>
    <exclude name="ConstantNamingConventions"/>
    <exclude name="DevelopmentCodeFragment"/>
  </rule>
  <!-- [refined in §8 review] prefer `messrust.xml,explicitness` on the CLI
       for deep passes instead of loading it here — see §8.10 -->
  <!-- <rule ref="explicitness"/> -->
</ruleset>
```

(Threshold overrides via per-rule properties — exact names in the §8.4 table —
reserved for after the first advisory run shows which codesize rules actually
fire.) **[corrected in §8 review: these are not all `maximum`]**

### 7.3 Proposed artifacts (local-only, no CI, no hk gate yet)

1. **`mise.toml` `[tools]`** (from §4, unchanged):

   ```toml
   "cargo:https://github/quality-gates/messrust" = { version = "tag:v0.1.12" }
   ```

   then `mise lock`.

   > **[corrected in §8 review]** The vendored docs postdate this tag; upstream
   > latest is **v0.1.15** (2026-09-26,
   > [releases API](https://api.github.com/repos/quality-gates/messrust/releases/latest)).
   > Re-verify the §8 flags (`--ignore-errors-on-exit`, `--verbose`) exist at
   > the pinned tag, or bump to `tag:v0.1.15` — see §8.1.

2. **Repo policy file:** `messrust.xml` at repo root (§7.2).

3. **Task** — a TOML task in the "Formatting & Linting" section, matching the
   repo's `arg "[args]" var=#true` passthrough idiom (no USAGE flags worth
   exposing beyond passthrough; flags like `--only`/`--strict` change per
   triage session):

   ```toml
   [tasks.mess]
   description = "Static mess detection with messrust (advisory)"
   extends = "gate:never-skip"   # parse-only → always cheap; never freshness-skip
   usage = 'arg "[args]" var=#true help="Extra arguments forwarded to messrust (e.g. --strict, --only Rule, --reportfile f.json)"'
   run = '''
   #!/bin/bash
   set -euo pipefail
   declare -a extra
   eval "extra=(${usage_args:-})"
   messrust src text messrust.xml --ignore-tests "${extra[@]+"${extra[@]}"}"
   '''
   ```

   `sources = ["@group:rust"]` is deliberately *not* the cache story here:
   `gate:never-skip` (`outputs = []`, `cache = false`, mise.toml:292) matches
   how every other always-run check in this repo is declared, and parse-only
   runtime makes skipping pointless.

4. **Not proposed (yet):** `hk.pkl` quality step, `verify` inclusion, CI
   install — all deferred until the policy is tuned and the run is quiet,
   per §5/§6. Explicitly out of scope per current decision ("useful even if
   not part of any CI").

### 7.4 Day-to-day leverage

- **First run / triage:** `mise run mess` (exit 2 with findings); re-run
  narrowing with `--only CyclomaticComplexity` or `--minimumpriority 2`;
  full machine triage via `--reportfile messrust.json --ignore-violations-on-exit`.
- **Tuning loop:** findings that are true-but-won't-fix → bump `<priority>`
  (then filter with `--minimumpriority`) or in-source
  `// messrust-disable-next-line Rule` with a reason; findings that are
   redundant-with-clippy → `<exclude>` in `messrust.xml`; threshold noise →
   a per-rule property — names are heterogeneous (`reportLevel` for
   `CyclomaticComplexity`, `minimum` for length/count rules, `maximum` only
   for `ExcessiveClassComplexity`/`LongVariable`/`LongClassName`/
   `CouplingBetweenObjects`/`LackOfCohesionOfMethods`, `maxfields`/`maxmethods`
   for `TooMany*`). **[corrected in §8 review — full table in §8.4]**
- **Cadence:** on-demand (`mise run mess`) after refactors of large modules;
  when quiet, promote to the `hk` `quality` group (§5 option 2) so `check` →
  `verify` picks it up — one-line change, no verify restructuring.
- **Runtime:** parse-only, no build — expect seconds; *unverified on this
  codebase* (no upstream benchmark exists).

---

## 8. Full capability inventory (vendored docs review)

Review of the vendored snapshot at
[docs/refs/messrust/](messrust/) — `README.md`, every file under `docs/`,
every file under `rulesets/` — verifying §1–§7 against primary text and going
deeper on mechanics the prior note only summarised. The vendored copy is the
authority; upstream URLs appear only where the snapshot points upstream or an
external check was needed (queried 2026-09-27). Grounding against this
codebase uses `Cargo.toml` `[lints.clippy]`, `clippy.toml`,
`docs/refs/clippy_lint_config.md`, and `src/`.

### 8.1 Snapshot provenance, version drift, and `mutarust.yml.example`

- **Version drift.** Vendored mtimes are 2026-09-26 (`usage.md` 2026-09-27) —
  the snapshot **postdates the `tag:v0.1.12` pin** in §4/§7.3 (v0.1.12 was
  published 2026-09-23). Upstream's latest release at query time is **v0.1.15**
  (2026-09-26)
  ([releases API](https://api.github.com/repos/quality-gates/messrust/releases/latest)).
  Before adopting, re-verify that every flag documented below exists at the
  pinned tag — notably `--ignore-errors-on-exit` and `--verbose` — or bump the
  pin to `tag:v0.1.15`. **[corrected in §8 review — see §7.3]**
- **`mutarust.yml.example` verdict: belongs to upstream messrust, but is not a
  messrust capability — ignore it.** It is byte-identical to the file upstream
  commits as `mutarust.yml` (verified by `diff` against
  [raw](https://raw.githubusercontent.com/quality-gates/messrust/main/mutarust.yml);
  upstream has no `mutarust.yml.example` — [contents API](https://api.github.com/repos/quality-gates/messrust/contents/)).
  It is the *messrust project's own* mutation-testing policy (`min_msi: 75`,
  `min_covered_msi: 80` — the exact thresholds its README documents in the
  Maintainers section), renamed `.example` during vendoring. It says nothing
  about messrust's analysis; this repo's mutarust material lives separately in
  [docs/refs/mutarust/](mutarust/) and
  [docs/refs/quality_gates_mutarust.md](quality_gates_mutarust.md).
  (It is *not* a copy of that directory's `mutarust.yml.example`, which differs
  — `min_msi: 80`.)
- **Missing from the snapshot** (linked by vendored docs, not vendored):
  first-party exploratory reports
  [2026-09-26-messrust](https://github.com/quality-gates/messrust/tree/main/docs/exploratory-testing)
  and `2026-09-23-explicitness` (referenced from
  [docs/refs/messrust/docs/usage.md](messrust/docs/usage.md) and
  [explicitness.md](messrust/docs/explicitness.md)) — useful for calibrating
  real-world finding volume; worth fetching if available upstream.

### 8.2 Complete rule inventory (41 live rules × 7 rulesets)

Read line-by-line from `rulesets/*.xml`; "Preset" = membership in `rust` /
`opinionated` ([rulesets/rust.xml](messrust/rulesets/rust.xml),
[opinionated.xml](messrust/rulesets/opinionated.xml)). Priorities matter for
the `--minimumpriority`/`--maximumpriority` filters (1 = highest).

**codesize** — all 10 rules in `rust`, all priority 3
([rulesets/codesize.xml](messrust/rulesets/codesize.xml),
[docs/codesize.md](messrust/docs/codesize.md)):

| Rule | Properties (default) | Fires |
| --- | --- | --- |
| `CyclomaticComplexity` | `reportLevel` (10), `showClassesComplexity` (true), `showMethodsComplexity` (true) | value ≥ `reportLevel` |
| `NPathComplexity` | `minimum` (200) | ≥ 200 |
| `ExcessiveMethodLength` | `minimum` (100), `ignore-whitespace` (false) | ≥ 100 lines of the full source span — comment/blank lines count unless `ignore-whitespace=true` |
| `ExcessiveClassLength` | `minimum` (1000), `ignore-whitespace` (false) | ≥ 1000; counts type span + spans of methods attached to the type (trait default bodies inside the span add nothing) |
| `ExcessiveParameterList` | `minimum` (10) | ≥ 10 typed parameters; skips `self` and `_: Type` wildcards |
| `ExcessivePublicCount` | `minimum` (45) | ≥ 45 public fields + methods; trait methods count public only if the trait is public |
| `TooManyFields` | `maxfields` (15) | **>** 15; struct/union fields only — enum variants are not fields |
| `TooManyMethods` | `maxmethods` (25), `ignorepattern` (`(^(set\|get\|is\|has\|with))i`) | **>** 25; inherent + trait-declared methods, *not* `impl Trait for Type` blocks; accessor-prefixed names skipped |
| `TooManyPublicMethods` | `maxmethods` (10), same `ignorepattern` | **>** 10 public methods |
| `ExcessiveClassComplexity` | `maximum` (50) | ≥ 50 WMC (sum of per-method cyclomatic complexities on one type) |

Note the **fire condition asymmetry**: the seven `minimum`/`reportLevel`/
`maximum` rules fire when the value *meets* the threshold; the three
`TooMany*` rules fire only when *strictly greater than* their maximum
([docs/codesize.md](messrust/docs/codesize.md)). Cyclomatic counting: starts
at 1, +1 per `if`/`if let`, `while`/`while let`, `for`, `loop`, non-wildcard
`match` arm, match guard, `&&`/`||`; a lone `_` arm is free; nested `fn` items
and closures roll their decision points into the *enclosing* function.

**naming** — `rust` includes 6 of 7 (all priority 3 except the two noted)
([rulesets/naming.xml](messrust/rulesets/naming.xml),
[docs/naming.md](messrust/docs/naming.md)):

| Rule | Preset | Properties (default) | Fires |
| --- | --- | --- | --- |
| `ShortClassName` | `rust` | `minimum` (3), `exceptions` | type name < 3 chars (struct/enum/trait/union) |
| `LongClassName` | `rust` | `maximum` (40), `subtract-prefixes`, `subtract-suffixes` | type name > 40 chars (only first matching prefix/suffix subtracted) |
| `ShortMethodName` | `rust` | `minimum` (3), `exceptions` | fn/method/trait-method name < 3 chars |
| `LongVariable` | `rust` (**`maximum` overridden to 35**) | `maximum` (20 base / **35 in `rust`**), `subtract-prefixes`, `subtract-suffixes` | field/param/local > limit; `for`/`while let` binders skipped, `if let`/`match` binders checked |
| `ConstantNamingConventions` | `rust`, priority **4** | `convention` (`upper`) | const/static/associated const not SCREAMING_SNAKE_CASE; `const _` ignored |
| `BooleanGetMethodName` | `rust`, priority **4** | `checkParameterizedMethods` (false) | `get*` name returning `bool`; default skips parameterized methods and free fns with any parameter |
| `ShortVariable` | **`opinionated`** | `minimum` (3), `exceptions` | field/param/local < 3 chars; `for`/`while let` binders skipped |

**unusedcode** — all priority 3, single-file analysis
([rulesets/unusedcode.xml](messrust/rulesets/unusedcode.xml),
[docs/unusedcode.md](messrust/docs/unusedcode.md)):

| Rule | Preset | Properties | Notes |
| --- | --- | --- | --- |
| `UnusedPrivateField` | `rust` | — | skips `pub`/`pub(crate)`/`pub(super)`/tuple fields and **fields on types deriving `Serialize`/`Deserialize`**; constructor write ≠ read |
| `UnusedLocalVariable` | `rust` | `exceptions` | `let`/`if let`/`while let`/`for`/`match` binders; `_`-prefixed names never fire |
| `UnusedPrivateMethod` | `rust` | — | private *inherent* methods only; same-file call forms `self.x()`/`Type::x()`/`Self::x()` recognized |
| `UnusedFormalParameter` | **`opinionated`** | — | params except `self`; abstract trait methods (no body) never fire |

**cleancode**
([rulesets/cleancode.xml](messrust/rulesets/cleancode.xml),
[docs/cleancode.md](messrust/docs/cleancode.md)):

| Rule | Preset | P | Properties | Fires on |
| --- | --- | ---: | --- | --- |
| `BooleanArgumentFlag` | `opinionated` | 1 | `exceptions` (enclosing type names), `ignorepattern` (fn name regex) | any bare `bool` param; `_`/underscore-prefixed skipped |
| `ElseExpression` | `opinionated` | 1 | — | final non-`if` branch; pure `else if` chains don't fire |
| `StaticAccess` | `opinionated` | 1 | `exceptions` (type names), `ignorepattern` | call through a PascalCase type path other than the enclosing type; `Self::…`, snake_case module paths, associated items without calls are exempt |
| `IfStatementAssignment` | `rust` | 1 | — | `=` (not `+=`) assignment inside an `if`/`while` condition block |
| `DuplicatedArrayKey` | `rust` | 2 | — | duplicate named field in a struct literal — which **rustc rejects anyway (E0124)** |

**design**
([rulesets/design.xml](messrust/rulesets/design.xml),
[docs/design.md](messrust/docs/design.md)):

| Rule | Preset | P | Properties (default) | Fires on |
| --- | --- | ---: | --- | --- |
| `ExitExpression` | `opinionated` | 1 | — | path ending in `exit`/`abort` |
| `CountInLoopExpression` | `opinionated` | 2 | — | `.len()`/`.capacity()` or fn path ending `len`/`capacity` in a `while` condition or `for` iterable |
| `DevelopmentCodeFragment` | `rust` | 2 | `unwanted-functions` (default `""`, documented as **additional** names — extends, not replaces, the built-ins) | calls to `println`/`println!`, `print`/`print!`, `eprintln`/`eprintln!`, `dbg`/`dbg!` |
| `EmptyCatchBlock` | `rust` | 2 | — | `if let Err(_) = … {}` or empty `Err(…)` match arm |
| `CouplingBetweenObjects` | `rust` | 2 | `maximum` (13) | ≥ 13 distinct non-builtin type names in one type's fields/params/returns; last path segment only (`Vec<Service>` → `Vec`); primitives/`str`/`Self` excluded; **per-type, not a module/crate dependency graph** |
| `GlobalVariable` | `rust` | 1 | `report-immutable` (false) | `static mut` actually assigned in the same file; immutable `static`/`const` never fire |
| `LackOfCohesionOfMethods` | `rust` | 3 | `maximum` (1) | LCOM4 > 1 (disconnected method groups); struct/enum/union only; trivial getters/setters excluded as nodes, stateless methods ignored |
| `GotoStatement` | `rust` | 1 | — | **nothing** — documented as an "IDENTITY / QUIET" compatibility rule that "never fabricates findings" |

**controversial** — all 5 in `rust`, all priority 1
([rulesets/controversial.xml](messrust/rulesets/controversial.xml),
[docs/controversial.md](messrust/docs/controversial.md)):
`CamelCaseClassName` (property `camelcase-abbreviations`, default `false` —
`HTTPClient` passes), `CamelCaseMethodName`, `CamelCasePropertyName`,
`CamelCaseParameterName`, `CamelCaseVariableName` (their `allow-underscore*`
properties are catalogue no-ops). Historical ids, real Rust checks: PascalCase
types, snake_case everything else. `self` and closure params skipped by the
parameter rule.

**explicitness** — 2 rules, priority 3, in **neither** preset
([rulesets/explicitness.xml](messrust/rulesets/explicitness.xml)):

| Rule | Properties (default) | Fires on |
| --- | --- | --- |
| `ImplicitInput` | `include-self` (false) | reads of shared statics; calls reading env/clock/FS/stdin/random |
| `ImplicitOutput` | `include-self` (false) | writes to shared statics; `&mut` parameters; print/log macros; calls changing FS/env/process (details in §8.10) |

Counts: `rust` = 32 rules, `opinionated` = 7, `explicitness` = 2 → 41 total.

### 8.3 Preset composition — §7.2's claim verified exact

Verified line-by-line against the XMLs: `rust` = all of `codesize` + `naming`
minus `ShortVariable`/`LongVariable` (with `LongVariable` re-included at
`maximum=35`) + `unusedcode` minus `UnusedFormalParameter` + `cleancode` minus
`BooleanArgumentFlag`/`ElseExpression`/`StaticAccess` + `design` minus
`CountInLoopExpression`/`ExitExpression` + all of `controversial`.
**`opinionated` is exactly the seven rules `rust` omits**
([rulesets/opinionated.xml](messrust/rulesets/opinionated.xml)) — so
`rust,opinionated` covers every component rule, and note `LongVariable` stays
at 35 in the combined run (opinionated does not restore the 20-char default).
`explicitness` is in neither, confirmed in
[docs/usage.md](messrust/docs/usage.md) ("Opt-in; not part of `rust` or
`opinionated`"). The per-component `.md` membership tables agree with the XMLs
in every row checked.

### 8.4 Threshold tuning — exact property names (corrects §7.4's blanket `maximum`)

§7.4 said "threshold noise → `<property name="maximum" .../>`" — **that name
works for only 5 rules.** The full map (defaults in §8.2):

| Property name | Rules using it |
| --- | --- |
| `reportLevel` | `CyclomaticComplexity` |
| `minimum` | `NPathComplexity`, `ExcessiveMethodLength`, `ExcessiveClassLength`, `ExcessiveParameterList`, `ExcessivePublicCount`, `ShortClassName`, `ShortVariable`, `ShortMethodName` |
| `maximum` | `ExcessiveClassComplexity`, `LongClassName`, `LongVariable`, `CouplingBetweenObjects`, `LackOfCohesionOfMethods` |
| `maxfields` / `maxmethods` | `TooManyFields` / `TooManyMethods`, `TooManyPublicMethods` |
| `ignore-whitespace` | `ExcessiveMethodLength`, `ExcessiveClassLength` |
| `exceptions` (allowlist) | `ShortClassName`, `ShortVariable`, `ShortMethodName`, `UnusedLocalVariable`, `BooleanArgumentFlag`, `StaticAccess` |
| `ignorepattern` (fn-name regex) | `BooleanArgumentFlag`, `StaticAccess`, `TooManyMethods`, `TooManyPublicMethods` |
| `subtract-prefixes` / `subtract-suffixes` | `LongClassName`, `LongVariable` |
| `unwanted-functions` | `DevelopmentCodeFragment` (extends the `println`/`print`/`eprintln`/`dbg` defaults) |
| `convention` / `checkParameterizedMethods` / `camelcase-abbreviations` / `report-immutable` / `include-self` / `showClassesComplexity` / `showMethodsComplexity` | as named in §8.2 |

Worked override example (from
[docs/codesize.md](messrust/docs/codesize.md) and
[docs/naming.md](messrust/docs/naming.md)):

```xml
<rule ref="codesize/CyclomaticComplexity">
  <properties><property name="reportLevel" value="12" /></properties>
</rule>
<rule ref="naming/ShortMethodName">
  <properties><property name="exceptions" value="at,of,or,eq" /></properties>
</rule>
```

Behaviour of a *misspelled* property name is undocumented — when a policy
"does not load as expected", `--verbose` prints ruleset load diagnostics
([docs/usage.md](messrust/docs/usage.md)).

### 8.5 XML composition and the runtime filter pipeline

- **The ruleset argument is a comma-separated mix** of built-in preset names,
  component names, single rule names, `rulesets/name.xml`, and XML file paths —
  e.g. `messrust src text rust,path/to/extra.xml --ignore-tests` or
  `messrust src text messrust.xml,explicitness`. (This means §7's
  `<rule ref="explicitness"/>` is *not the only way* to add a component.)
- **References nest**; a ref may name a built-in, one rule, a shipped
  `rulesets/*.xml`, or another XML file *relative to the current file*.
  **Later refs override earlier priority and property values** (document order
  = precedence); `<exclude name="..."/>` removes a rule from the referenced
  set ([docs/usage.md](messrust/docs/usage.md)).
- **Filters run after composition, in this order of intent:**
  `--only`/`--enable LIST` (keep only named rules *already in the loaded
  policy* — **cannot import an absent rule**, so `--only ImplicitOutput` on a
  policy without explicitness does nothing); `--disable LIST` (drop without
  editing XML); `--minimumpriority N` (keep priorities **≤ N**, 1 = highest);
  `--maximumpriority N` (keep **≥ N**); `--exclude LIST` (skip paths
  *containing* any listed substring — substring, so `--exclude index` also
  matches `…/indexing/…`); `--suffixes LIST` (**replaces** the default `.rs`
  list — omit `.rs` and you scan nothing).
- **Priority distribution makes the priority filters coarse** (defaults from
  the XMLs): priority 1 = all `controversial` + `IfStatementAssignment`,
  `GlobalVariable`, `GotoStatement` (the last two `rust`-preset p1 rules), plus
  opinionated `BooleanArgumentFlag`/`ElseExpression`/
  `StaticAccess`/`ExitExpression`; priority 2 =
  `DuplicatedArrayKey`, `DevelopmentCodeFragment`, `EmptyCatchBlock`,
  `CouplingBetweenObjects`, `CountInLoopExpression`; priority 3 = **all of
  `codesize`**, `LCOM4`, all `unusedcode`, `LongVariable`/`LongClassName`/
  `ShortClassName`/`ShortMethodName`, `explicitness`; priority 4 =
  `ConstantNamingConventions`, `BooleanGetMethodName`.
  Consequence: `--minimumpriority 2` shows only p1–2 and **hides every
  codesize finding** — for codesize bisection the right levers are
  `--only`/`--disable`, not priorities (§7.4's `--minimumpriority 2` triage
  tip is correct as "show the most severe first", not as "focus on
  codesize").

### 8.6 Suppression directives, `--strict`, and `#[expect]` non-interference

Exact semantics ([docs/usage.md](messrust/docs/usage.md),
[README](messrust/README.md)):

```rust
// messrust-disable-next-line CyclomaticComplexity,NPathComplexity
fn intentionally_dense() { /* ... */ }

// messrust-disable LongVariable
let deliberately_named_variable = value;
// messrust-enable LongVariable
```

- Names case-insensitive, comma **or space** separated;
  `disable-next-line` applies to the **following physical line only**;
  `disable` starts a region on the following line; `enable` closes **only the
  named rules**; **malformed directives are silently ignored** (a typo = the
  finding stays and you get no warning — footgun for agents grepping
  suppressions).
- Normal reports omit suppressed findings; `--strict` keeps them **marked
  suppressed** — this is the auditable-suppressions mechanism: a periodic
  `--strict --reportfile suppressions.json` run yields the full suppression
  inventory with rule + location + context.
- **messrust does not read rustc/clippy `#[expect]`/`#[allow]` attributes.**
  This repo's primary exception mechanism — `#[expect(clippy::print_stdout,
  reason = "…")]` at `src/cli/tracked.rs:45`, `src/cli/trust.rs:57`,
  `src/cli/template.rs:158` (and friends) — is invisible to messrust. Every
  such site will still be reported unless separately suppressed or excluded
  from the policy. This is the direct cause of the §8.12
  `DevelopmentCodeFragment` correction.

### 8.7 Exit codes, ignore-on-exit flags, failure modes (the agent contract)

([docs/usage.md](messrust/docs/usage.md)):

| Code | Meaning | Agent handling |
| ---: | --- | --- |
| 0 | Clean, or every relevant failure was explicitly ignored | proceed |
| 1 | Command/configuration/discovery/report-write/source-processing error — **takes precedence over findings** | **findings state is unknown**; read the report/stderr, never treat as "clean" |
| 2 | Findings present, no non-ignored processing error | quality failure; findings list is trustworthy |

- **`--ignore-errors-on-exit`** (missed by the prior note): returns 0 despite
  operational/processing errors; "report contents still include the errors".
  Never use in CI; an agent trusting exit 0 alone with this flag can miss a
  broken policy or unreadable files.
- **`--ignore-violations-on-exit`**: returns 0 despite findings; report stays
  complete. The adoption/triage lever (§7). Both ignore flags "change only the
  process status. They never remove rows from the report."
- **Footgun — empty run ≠ clean:** "With `--ignore-tests`, a run with no
  source files to scan reports a **discovery error** instead of returning a
  clean result." A scoped run (hk step over changed files that all live under
  `tests/`, or a path list fully removed by `--exclude`) exits **1**, not 0.
- **Partial analysis:** a malformed/unreadable file becomes a processing error
  while "other valid files still analyze" — but the process still exits 1,
  masking any findings (exit 2) behind it.
- Upstream's own CI treats exit 2 as the failure
  ([README](messrust/README.md)) — same convention proposed here.

### 8.8 Report formats and fields

Formats: `text`, `ansi`, `json`, `xml`, `html`, `github` (Actions line
annotations), `gitlab` (Code Quality), `checkstyle`, `sarif`
([docs/usage.md](messrust/docs/usage.md)). "Structured formats carry **stable
path, line, rule, priority, message, context, and suppression fields**.
Findings sort by file and source line." `--reportfile <path>` writes the
report to a file **and leaves stdout empty** (task wiring must read the file);
`--color` colorizes text output.

**Gap:** the vendored docs do not give exact JSON key names or SARIF
artifact structure — derive the schema from a first
`messrust src json rust --ignore-tests --reportfile messrust.json` run before
writing any consumer. (Known upstream issue #100: html/sarif/gitlab/checkstyle
drop report rows on processing errors — prefer `json` for agent triage,
already cited in §6.)

### 8.9 Scoping semantics: `--ignore-tests`, `--exclude`, `--suffixes`

- `--ignore-tests` skips test files/dirs, `#[cfg(test)]` modules, and
  `#[cfg(test)]` `impl` blocks/methods, and — crucially — "a method that only
  compiles with `test` on does not count toward the metrics of its type … same
  code size, complexity, coupling, and cohesion as in a production build"
  ([docs/usage.md](messrust/docs/usage.md)). Material here: **171 `#[cfg(test)]`
  blocks inside `src/`** plus `tests/e2e` and `tests/integration` — without
  the flag, every codesize/coupling/cohesion metric is inflated by test code.
  Mandatory for this repo's gate, as §7.1 already states.
- **Documented ambiguity (verify on first run):** usage.md says with
  `--ignore-tests` "the rules do not see `#[cfg(test)]` modules" **and also**
  "a read, call, or write in a test module still counts as a use of production
  code", while [docs/unusedcode.md](messrust/docs/unusedcode.md) lists as
  reading caveat #4 "A test may use the item when you run with
  `--ignore-tests`" (implying missed test usage). Whether an item used *only*
  by tests is reported unused under `--ignore-tests` is unclear from the
  docs — cheap to settle empirically with one fixture.
- `--exclude` = substring match (over-matching risk on names like `index`);
  `--suffixes` replaces the default `.rs` list (does not extend it).

### 8.10 `explicitness` in depth — verdict for this codebase

**Mechanics** ([docs/explicitness.md](messrust/docs/explicitness.md),
[rulesets/explicitness.xml](messrust/rulesets/explicitness.xml)):

- Definitions follow *Grokking Simplicity*: parameters = explicit inputs,
  return value = explicit output; everything else is implicit.
- **Shared static** (both rules): a `static mut`; a `static` whose type
  contains `Atomic*`, `Cell`, `RefCell`, `UnsafeCell`, `Mutex`, `RwLock`,
  `OnceCell`, or `OnceLock`; a `thread_local!` key. Plain immutable `static`
  and `const` are constants → never reported.
- `ImplicitInput` reports reads of shared statics and calls that read the
  environment, clock, file system, stdin, or random source.
  `ImplicitOutput` reports writes to shared statics, `&mut` parameters, print
  and log macros, calls that change FS/env/process. A "write" includes
  assignment, compound assignment, `&mut X`, and calls named `store`,
  `fetch_add`, `set`, `replace`, `take`, **`lock`**, `write`, `borrow_mut` —
  **`lock` counts as a write even when the code only reads the value** (a
  documented false-positive source on `Mutex`/`RwLock` code).
- `include-self=true` (stricter, opt-in): each read of `self` becomes an
  implicit input; `&mut self` receivers and `self` writes become implicit
  outputs. Default `false`.
- **Trait impls**: in `impl Trait for Type`, `&mut` params, the receiver, and
  `self` state are not reported; body effects (println, static writes) still
  are.
- Each distinct finding is reported **once per function, at its first line**.
- **Known limits (all documented):** single-file (a static declared elsewhere
  is invisible); calls matched by last two path segments (after
  `use std::env::var;`, a bare `var(..)` is **not** found); no effect
  propagation into callers; nested `fn` excluded but closures included in the
  enclosing function.

**Verdict for traces-pkm: valuable as an on-demand analysis lens, not as part
of the default policy.**

- *For this code:* it produces an **effect map** — every function touching
  shared state, the filesystem, the clock, or `&mut` parameters — which is
  exactly the structural information the current stack lacks. For a PKM tool,
  that map says where I/O and state boundaries actually are (vs. where they're
  supposed to be): candidates to push behind a trait or a pure core are
  visible at a glance, and `include-self=true` runs give method-level state
  churn for refactoring `field.rs`/`index/` types toward testable cores.
- *Against putting it in the default gate:* (1) this codebase is fs-centric —
  every config/index load function legitimately reads files and will be
  flagged forever, turning suppressions into routine noise; (2) `lock()`
  counts-as-write FPs apply to the ~18 `Mutex`/`RwLock`/`OnceLock`/`Atomic`
  sites in `src/`; (3) `&mut` params are ordinary Rust — the very reason
  upstream made the ruleset opt-in (≥12 free-fn `&mut` sites found here);
  (4) the documented blind spots (per-file, no effect propagation,
  two-segment call matching) mean the signal is a *map*, not a *score* — a map
  is most useful when summoned, not on every run.
- *Human vs agent:* humans get a boundary map and refactoring radar; agents
  get a machine-readable impurity list (json + `--reportfile`) that answers
  "what does this function secretly touch?" before a refactor — high agent
  value, moderate human value, poor gate value.
- **Recommended shape:** leave `<rule ref="explicitness"/>` **out** of the
  default `messrust.xml` and invoke it per-run — the ruleset argument mixes
  paths and built-ins (§8.5), so deep passes are
  `messrust src text messrust.xml,explicitness --ignore-tests` (plus
  `--reportfile … --ignore-violations-on-exit` for triage). The §7 sketch's
  inline ref still works mechanically; if kept, the default task would need
  `--disable ImplicitInput,ImplicitOutput` — the two-command split is
  simpler.

### 8.11 What messrust would surface here (grounded)

Structural facts from `src/` (~80k lines; `wc` + `grep`, 2026-09-27):

- **Type/file size (§3.1's core gap):** the default `ExcessiveClassLength`
  (≥1000) threshold sits below 14 files — `index/store.rs` (2539),
  `config/service.rs` (2111), `query/results.rs` (2037),
  `schema/fields/select.rs` (2016), `field.rs` (1969),
  `template/engine/query.rs` (1964), `note/parser/list.rs` (1868),
  `index/service.rs` (1804), `cli/error.rs` (1783), `schema/builder.rs`
  (1699), `template/engine/date.rs` (1652), `cli/mod.rs` (1598),
  `duration.rs` (1495), `note/parser.rs` (1421). Caveat: the rule counts
  *type span + attached inherent methods*, not whole files — but these files
  are each dominated by one service/type, so expect real hits.
- **Complexity:** `NPathComplexity` (≥200) and `CyclomaticComplexity`
  (≥10) have no clippy equivalent (clippy's `cognitive_complexity` = 25 is a
  different metric — §3.2 already noted this). Parser/query/template code
  (`note/parser/list.rs`, `query/grammar/source.rs`,
  `template/engine/query.rs`) is path-explosion territory: cyclomatic +1 per
  match arm means dispatch-heavy functions will fire often — start with
  `reportLevel` 12+ (upstream's own tuning example) rather than accepting the
  default 10.
- **Coupling:** `CouplingBetweenObjects` (≥13) measures **per-type fan-out**,
  not the index↔query↔config *module* coupling — messrust has no
  module/crate-level dependency metric (limitation vs. the expectation set in
  §1/§3). First candidates: `ConfigService` (`config/service.rs` wires
  tracked/trusted roots, templates, dialog, env) and the index store/service.
- **Cohesion:** `LackOfCohesionOfMethods` (>1) on the large types above —
  `field.rs` (1969-line `Field`) and `cli/error.rs` are prime split-candidates
  to examine first.
- **Naming:** no type name >40 chars exists (grep) → `LongClassName` quiet.
  `LongVariable` (>35) mostly quiet in prod code (long snake names found are
  test fn names, which no length rule covers and `--ignore-tests` skips).
  **`ShortMethodName` (<3) will fire on real code:** `fn at(` (×3:
  `src/config/service.rs:117`, `src/config/tracker.rs:94`,
  `src/file_tracker.rs:40`), `fn of(` (`src/path.rs:199`), `fn or(`
  (`src/query/grammar/source.rs:155`), and `fn eq(` in `PartialEq` impls
  (`src/note/lists.rs:610`+, `src/note/field.rs:436`, `src/duration.rs:516`)
  → set `<property name="exceptions" value="at,of,or,eq"/>` (§8.4) or accept
  suppressions.
- **Redundancy verified against the live lint config:**
  - `ExcessiveParameterList` (≥10): `clippy::too_many_arguments` is in the
    **denied `complexity` group** (verified in clippy source:
    [functions/mod.rs](https://github.com/rust-lang/rust-clippy/blob/master/clippy_lints/src/functions/mod.rs))
    at `too-many-arguments-threshold = 5` (`clippy.toml`) — strictly stricter.
    Resolves §3.2's "not independently verified" cell → exclude (§8.12).
  - `ExcessiveMethodLength` (≥100) vs `clippy::too_many_lines = deny` at 100:
    thresholds match but **counting bases differ** — clippy counts only lines
    containing code (blank/comment lines skipped,
    [too_many_lines.rs](https://github.com/rust-lang/rust-clippy/blob/master/clippy_lints/src/functions/too_many_lines.rs)),
    messrust counts the full span including comments unless
    `ignore-whitespace=true`. Near-dup with occasional divergence on
    comment-heavy functions — §7's exclude still right.
  - `DevelopmentCodeFragment` vs the print/dbg story: 5 `println!` + 13
    non-test `eprintln!` sites in `src/` (a 14th `eprintln!` is in
    `src/dialog/terminal.rs`'s test module, skipped by `--ignore-tests`), all
    deliberate CLI/diagnostic output — several are explicitly
    `#[expect(clippy::print_stdout, reason = "… output is data
    meant to be piped …")]`. `clippy::print_stdout` and `dbg_macro` are
    already `deny`; `print_stderr` is deliberately `allow` (Cargo.toml), so
    `eprintln!` is sanctioned here while messrust would flag all 13 →
    **exclude** (§8.12).
  - `unusedcode` rules vs rustc: `unused_variables`/`dead_code` are already
    errors under `-D warnings`, *and* messrust's single-file analysis cannot
    see a child module using a parent-module private item — an everyday shape
    in this workspace (`src/index/`, `src/note/`, `src/query/` are multi-file
    modules) → §7's exclude of the three `rust`-preset unused rules is
    correct for two independent reasons.
  - `ExcessivePublicCount`/`TooManyPublicMethods`: **588 `pub(crate)`
    occurrences** in `src/` → the pending-FP exclusion (upstream #160) is
    well-founded here.
  - `GlobalVariable`: **zero `static mut`** in `src/` → never fires; harmless
    to keep. `GotoStatement`: never fires by design (§8.2).
  - `DuplicatedArrayKey`: can only fire on code rustc rejects (E0124) →
    never fires on compilable code (§8.12).

### 8.12 Corrections applied to §7 (in place, marked `[corrected in §8 review]`)

1. **`DevelopmentCodeFragment`: keep → exclude.** The prior "design: keep"
   row was wrong for this repo: clippy already denies `print_stdout`/
   `dbg_macro`; `eprintln!` is explicitly allowed (`print_stderr = "allow"`)
   and used as deliberate CLI output in 13 production sites (plus 5
   `println!` sites carrying `#[expect]` justifications); and messrust
   ignores those attributes entirely (§8.6) — keeping the rule buys ~18
   findings of intentional output. (Upstream's own
   escape hatch for exactly this situation is "disable the rule" —
   [docs/design.md](messrust/docs/design.md).)
2. **`ExcessiveParameterList`: keep → exclude.** Verified redundant with
   `clippy::too_many_arguments` in the denied `complexity` group at
   threshold 5 (§8.11) — strictly stricter than messrust's ≥10. §3.2's
   unverified cell updated to match.
3. **`ConstantNamingConventions`: keep → exclude.** Duplicates rustc
   `non_upper_case_globals` (warn-by-default → error under this repo's
   `-D warnings`); upstream's own [docs/naming.md](messrust/docs/naming.md)
   says the default "matches … clippy's non_upper_case_globals". Zero
   incremental signal expected — cheap to re-include if a first run proves
   otherwise.
4. **`DuplicatedArrayKey`: rationale corrected** (stays loaded — harmless —
   but "additive" was wrong): it only reports duplicate struct-literal fields,
   which rustc rejects (E0124), so it never fires on code that compiles.
5. **`GotoStatement`: rationale corrected** (exclusion is cosmetic): it is a
   documented quiet compatibility rule that "never fabricates findings"
   ([docs/design.md](messrust/docs/design.md)), not merely a language dead
   weight — keeping or excluding changes nothing.
6. **`naming` keep row corrected**: added the missing `ShortClassName`, split
   out `ConstantNamingConventions` (item 3), and annotated `ShortMethodName`
   with the required `exceptions` for `at`/`of`/`or`/`eq` (§8.11).
7. **`explicitness` row refined**: from "keep (opt-in ref)" to on-demand
   invocation via `messrust.xml,explicitness` (§8.10); sketch line commented
   with the rationale.
8. **§7.3 pin note**: vendored docs postdate `tag:v0.1.12`; upstream latest is
   v0.1.15 (2026-09-26) — verify flags or bump (§8.1).
9. **§7.4 property-name claim fixed**: threshold overrides are not all
   `maximum` — see the table in §8.4 (`reportLevel`/`minimum`/`maxfields`/
   `maxmethods`/…).
10. **Sketch updated** accordingly (three new `<exclude>` entries, explicitness
    commented out); the sketch's structure (nested ref + excludes) was checked
    against [docs/usage.md](messrust/docs/usage.md) and is valid.

### 8.13 Capability → audience value (summary)

| Capability | Human value | Agent value |
| --- | --- | --- |
| `codesize` thresholds + `<property>` tuning | readable triage against fixed numbers; one-limit-at-a-time tuning workflow | machine-tunable policy; deterministic pass/fail per rule via `--only` |
| `CouplingBetweenObjects`/`LCOM4` | refactor radar for big types | structural metrics in JSON for "where to split" decisions |
| `NPath`/`Cyclomatic` (beyond clippy's cognitive-25) | catches test-hostile functions cognitive complexity misses | path-explosion hotspots for test-planning |
| `--only`/`--disable`/`--minimumpriority` | interactive narrowing | cheap bisection of noisy runs; scoped re-checks |
| Suppression directives + `--strict` | narrow waivers with prose reasons next to code | suppression inventory audit (`--strict --reportfile`) — auditable exceptions |
| Exit codes 0/1/2 + ignore-on-exit | adoption without breaking workflows | stable contract: 2 = findings, 1 = unknown state, 0 = clean; ignore flags never mutate report rows |
| json/sarif/checkstyle + `--reportfile` | html/checkstyle for review, github annotations in CI | path/line/rule/priority/message/context/suppression rows, sorted, stdout-free |
| `--ignore-tests` | honest production metrics | scoped/bisectable runs unaffected by the 171 `cfg(test)` blocks |
| `explicitness` (on-demand) | boundary/refactor map | impurity list — "what does this function secretly touch?" |

---

## 9. First advisory run (2026-09-27)

The counts block below is the **post-tuning re-run** (the pre-tuning
measurement had `CyclomaticComplexity` at 50 — see decisions). Produced by:

`messrust src json messrust.xml --ignore-tests --reportfile /tmp/messrust.json --ignore-violations-on-exit && grep -oE '"rule"[[:space:]]*:[[:space:]]*"[^"]+"' /tmp/messrust.json | sort | uniq -c | sort -rn`

(counts as measured, key excerpts):

```
  29 "rule": "LackOfCohesionOfMethods"
  26 "rule": "CyclomaticComplexity"
   9 "rule": "CouplingBetweenObjects"
   5 "rule": "ExcessiveClassComplexity"
   4 "rule": "NPathComplexity"
   2 "rule": "EmptyCatchBlock"
   1 "rule": "TooManyMethods"
```

Action thresholds applied (from the quality-gates triage plan): any rule with
count > 25 → consider exclusion (written reason required); `CyclomaticComplexity`
count > 15 → retune `reportLevel`; `NPathComplexity` count > 10 → retune
`minimum`.

Decision-table rows, count → action:

| Rule | Count | Action |
| --- | --- | --- |
| `CyclomaticComplexity` | 50 (> 15) | **Edit applied**: `codesize/CyclomaticComplexity` ref with `reportLevel=12` appended to `messrust.xml`; re-run → 26. |
| `NPathComplexity` | 4 (≤ 10) | No edit. |
| `ExcessiveClassLength` | 0 | No findings; discrepancy with §8.11 noted below. |
| `ShortMethodName` | 0 | No findings (exceptions list from Task 2 retained). |
| `LongVariable` / `LongClassName` | 0 / 0 | No findings. |
| `CouplingBetweenObjects` | 9 | Recorded below — kept as signal, no tuning (no confirmed false positive). |
| `LackOfCohesionOfMethods` | 29 | Recorded below — kept as signal, no tuning. |
| `ShortClassName` | 0 | No findings. |
| any other rule (> 25 to exclude) | `ExcessiveClassComplexity` 5, `EmptyCatchBlock` 2, `TooManyMethods` 1 | All ≤ 25 → no exclusions; recorded as signals. |

Decisions taken: `CyclomaticComplexity` threshold edit applied (`reportLevel`
10 → 12, upstream's own tuning example); no other threshold edits — policy
otherwise accepted as-is.

Notable findings kept as signals (esp. `ExcessiveClassLength` files): no
`ExcessiveClassLength` findings (rule did not fire), which contradicts §8.11's
prediction of "expect real hits" against 14 files over 1000 lines. The
discrepancy is unexplained and warrants a follow-up check — §8.11 already
noted the rule counts type span + attached inherent methods rather than whole
files, which may explain part of the gap, but that was not verified against
these results. Example locations for rules that did fire:

- `LackOfCohesionOfMethods`: src/cli/task.rs:28, src/config/file.rs:117,
  src/config/model.rs:54
- `CyclomaticComplexity`: src/cli/error.rs:259, src/cli/error.rs:341,
  src/cli/error.rs:413
- `CouplingBetweenObjects`: src/cli/error.rs:51, src/cli/mod.rs:149,
  src/index/service.rs:35
- `ExcessiveClassComplexity`: src/duration.rs:36, src/index/store.rs:135,
  src/note/field.rs:152
- `NPathComplexity`: src/query/grammar/field.rs:242,
  src/query/grammar/source.rs:425, src/schema/fields/select.rs:359
- `EmptyCatchBlock`: src/file_tracker.rs:179, src/file_tracker.rs:292
- `TooManyMethods`: src/index/store.rs:135
