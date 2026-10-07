# Testing Architecture Audit — `traces-pkm`

**Scope:** adversarial, evidence-driven audit of the unit / component /
integration / E2E test architecture. Read-only: no production code or tests were
modified. Evidence runs (cargo/nextest/mise) were executed by the lead during
verification.
**Evidence sub-reports:** `.scratch/integration-e2e-suite/research/{A..H}-*.md`
(9 subagent reports, all read and cross-checked; contradictions resolved by the
lead's own re-verification).
**Date:** 2026-10-06 · **HEAD:** `7699b4cf`

---

## 1. Executive summary

**Verdict: the architecture is sound; the defects are over-claiming and gaps, not structure.**

> **Revision 2 (§16):** an external critique was adjudicated by four review
> agents. Adopted: E2E spawn-only invariant, pruning-first file trees, `pub mod
> testing` facade, feature-separated runner, Windows `APPDATA` gap,
> layer-enforcement checks, `tests/common`. Softened: test-utils runtime risk
> (nil today), `golden_path` deletion (preconditions unmet), `expect_used`
> remedy. §14/§15 are revised accordingly — "consolidation, not restructuring"
> now applies to *tests* but not to the *facade/runner* (which do warrant
> restructuring).

- The suite is **green and fast**: `mise run test` → **3,127 nextest tests + 76
  doctests in ~11 s**.
- Layering (in-crate unit → in-crate component → `tests/integration` via
  `test-utils` facade → `tests/e2e` via spawned binary) is deliberate,
  documented in module docs, and mostly well kept. Roughly **98 % of in-crate
  tests are correctly placed** (D1: ~10 actionable of ~452; D2 similar).
- The three highest-value defects are all of the same shape — **a test's doc
  claims a boundary it does not execute**:
  1. `src/cli/mod.rs::query_workflows::table_reflects_a_note_edit…` asserts via
     a *fresh* `refresh_store()` (`src/index/service.rs:226`) after `Cli::run` —
     the assertion passes even if `Cli::run` did nothing (verified by the lead
     this session).
  2. `tests/e2e/dispatch.rs:17/:38` claim cross-process persistence; the only
     observable is harness `is_file()` (`dispatch.rs:24,:45`) — **no second
     product process ever reads `index.redb`**, and `:38`'s "would catch a
     `refresh()`→`build()` refactor" is unprovable (both persist a file).
  3. `tests/e2e/golden_path.rs:37-43` claims "eight spawns" (it performs six)
     and that it proves "`index`'s output is what `list` reads" — unprovable,
     because `list` cold-rebuilds when no store exists
     (`src/cli/mod.rs:303-334`).
- **The single largest boundary gap:** `traces init` is never executed through
  `Cli::run`'s dispatch arm (`src/cli/mod.rs:187`) nor spawned as a process —
  E2E calls `Init.run(&provider)` in-process (`tests/e2e/init.rs:34,45,58`).
- **No test anywhere asserts a numeric exit code.** Clap's exit 2 and the 130
  path are unobservable from the suite.
- **Operationally dangerous:** plain `cargo test` runs **0 integration tests and
  exits 0** (`tests/integration.rs:5` cfg-strip, no `[[test]]` guard) —
  verified: `cargo test --test integration` → `running 0 tests`, rc 0; `cargo
  nextest run --test integration` → rc 4.
- Waste is concentrated: ~10 E2E + ~3 integration tests (≈2 % of the suite).
  Consolidation, not restructuring.

---

## 2. Method & evidence standard

- **Claim bar:** every finding cites `path::test_name` + the traced call path +
  what the test **executes** vs what it **asserts**. Doc comments, filenames,
  ADRs and glossaries were treated as leads only.
- **Process:** Phase 0 inventory (lead) → Phase 1 nine parallel subagents (A–H,
  static inspection only, no cargo — lock contention) → Phase 2 lead
  verification (re-read of all nine reports, independent re-verification of
  every high-impact claim, plus live evidence runs) → Phase 3 report.
- **Evidence runs performed by the lead (all at HEAD `7699b4cf`):**

  | Command | Result |
  |---|---|
  | `mise run test` | 3,127 run / 3,127 passed (11.3 s) + 76 doctests passed |
  | `cargo nextest list --all-features -E 'binary(integration)'` | **30** |
  | `cargo nextest list --all-features -E 'binary(e2e)'` | **27** |
  | `cargo nextest list --all-features` | 3,127 total (lib+bin 3,070) |
  | `cargo test --test integration` (default features) | `running 0 tests`, **rc 0 (silent green)** |
  | `cargo nextest run --test integration` (default features) | **rc 4 `NO_TESTS_RUN`** |
  | `cargo tree -p traces-pkm --features default` | hard error: no such feature |

- **Known limitations:** no coverage tooling or mutation run was executed (both
  are local-only, §13-F8); per-test execution-vs-assertion details for the 3,070
  in-crate tests rest on the subagent reports (A–H), of which the high-impact
  items were spot-verified by the lead.

---

## 3. Canonical inventory (verified)

| Target | Source | Feature-gated | Tests |
|---|---|---|---|
| `traces_pkm` (lib) | `src/**` `#[cfg(test)]` | no (`cfg(test)` opens test-utils paths) | 3,058 of 3,070 |
| `traces-pkm` (bin) | `src/main.rs` `mod tests` | no | 4 (exit-code mapping) |
| `integration` | `tests/integration.rs` + `tests/integration/*.rs` (6 files) | **YES** — `#![cfg(feature = "test-utils")]` (`tests/integration.rs:5`) | **30** |
| `e2e` | `tests/e2e.rs` + `tests/e2e/*.rs` (6 files) | **no gate** | **27** (20 in `dispatch.rs`) |
| doctests | `src/**/*.rs` | n/a | 76 |

- In `src/`: **2,230 `#[test]` + 177 `#[rstest]` attributes = 2,407 attrs →
  3,070 nextest tests** (rstest case expansion). An earlier "~2,410" figure in
  the Phase-0 notes was this attribute sum.
- **31 of 57 external tests never spawn the binary**: all 30 integration tests
  (zero `std::process`/`CARGO_BIN` references) plus `tests/e2e/init.rs` (A §2).
- `Cargo.toml` has **no `[[test]]` sections** (only 12 `[[bench]]` with
  `required-features`) — so nothing but the in-file `#![cfg]` can empty the
  integration suite (G §2.1).
- Composition of `tests/integration`: `index_query` 11,
  `schema_field_resolution` 5, `template_render` 5, `task_tag_filters` 3,
  `index_persistence_roundtrip` 4, `config_lifecycle` 2.

---

## 4. How tests actually compile and run (tooling)

- **Canonical entry:** `mise run test` (alias `t`) = `cargo nextest run -p traces-pkm
  --all-features` → `cargo test --workspace --all-features --doc`
  (`_default:127-134`). Doctests are a separate phase because nextest cannot run
  them.
- **Feature matrix:** single feature `test-utils`, **no `default` feature**
  (`Cargo.toml:182-183`) ⇒ `--all-features ≡ --features test-utils`; `--features
  default` is a hard error (verified).
- **nextest config is 11 lines** (`.config/nextest.toml`): only `status-level =
  "fail"` and `success-output = "never"`. No timeouts, no retries, no
  partitions, no test-groups ⇒ built-in defaults: one shared `num-cpus` pool for
  all four binaries, `slow-timeout` 60 s *without termination*, no
  `global-timeout`, and `status-level=fail` **suppresses even slow-test
  markers** (G §5).
- **CI** (`.github/workflows/ci.yml`): exactly one job runs tests (`test`,
  ubuntu-only, `mise run test`). macOS/Windows only *compile* tests (`cargo
  check --all-targets`). No default-feature test run, no coverage, no mutation,
  no sharding, MSRV job never tests (G §6).
- **Hooks:** pre-commit runs **no tests** (`hk.pkl:124-131`); pre-push runs the
  full suite. `mise run check` and CI hygiene explicitly `--skip-step
  cargo-test`.
- **Coverage/mutation:** `coverage:*` uses `llvm-cov nextest` (⇒ doctests
  excluded from coverage); `test:mutants` (mutarust, `src/cli` excluded,
  `min_msi` commented out) are local-only and advisory (G §7, F8).
- **`mise watch` = `cargo watch -x check -x test`** (`mise.toml:140`) = *bare*
  `cargo test`: default features (0 integration tests) **and** threaded libtest
  (E2E cwd race live) — the wrong
  command on both counts (G F1).

---

## 5. Integration suite (`tests/integration/`) — findings

**What it is:** library-boundary tests through the `test-utils` pub surface
(`TestProject`, `fixture_service`, `QueryService`, `TemplateService`,
`SchemaService`), no process spawning (`tests/integration.rs:1-5`).

1. **The suite's stated differentiator is mostly wasted.** Its constraint ("pub surface only") is
   real, but in-crate unit twins reach the same functions from the other side of the `pub(crate)`
   wall. The only genuinely different ingredient is *filesystem/DB through pub APIs* — of 30 tests,
   only **4** (`index_persistence_roundtrip` #5/#6, `template_render` #26/#27) depend on it (B §1.5).
2. **Byte-equivalent duplicate pair:** `config_lifecycle.rs::trust_then_untrust_round_trips…`
   vs `::test_project_manages_trust_and_untrust_lifecycle` — same observable (config exists,
   `untrust → 1`), same calls underneath (`TestProject::trusted` → `create_trusted_project`,
   `TestProject::untrust` → `service.untrust`). The second also duplicates
   `src/lib.rs:918::manages_trust_and_untrust_lifecycle`. **Verified by the lead** (file read).
3. **`schema_field_resolution.rs` — 5 tests through a non-production constructor.** All call
   `SchemaService::new` (`src/schema/service.rs:62`), which is `expect(dead_code)` in production
   builds and only wraps `load_verbose` (`:78`), the path production uses via
   `TemplateService::new` (`src/template/engine.rs:131`). **Lead's nuance:** because `new` delegates
   straight to `load_verbose`, the tests are not testing *wrong behavior* — but the file's uniqueness
   claim ("only test through `SchemaService::new` + `get` + `field`") is circular (B §1.4). Also:
   zero `assert*` calls (early-return `Result` style) and hard-coded `".traces/schemas"` instead of
   `DEFAULT_SCHEMAS_DIR`.
4. **Seam mixing:** `index_query.rs` alone spans ≥5 seams (query modes, sort, diagnostics,
   frontmatter inheritance, reuse) — the worst offender; `template_render.rs` is the only file with
   a coherent single boundary (B §2).
5. **Every `Config` in this suite is fixture-computed.** `TestProject::config()` returns
   `Config::test_default` (+`with_templates()` if the dir exists) — it never goes through
   discovery + merge + trust gate (B G-B4, E C2).
6. **Duplication vs units is extensive but cheap:** the twin table (B §1.3) maps ~14 of 30 tests to
   exact in-crate twins (e.g. `sorts_pages_descending_by_text_field` = literally the same fixture as
   `src/query/sort.rs::sort::orders_descending_when_requested`).

## 6. E2E suite (`tests/e2e/`) — findings

**What it uniquely can prove** (process-boundary contract matrix, C §6):

| Contract only the process boundary can prove | State |
|---|---|
| **Numeric exit codes** (0 / 1 / clap-2 / 130) | **NONE** — only `is_success()` booleans (`support.rs:71-74`). `Cli::parse()` exit-2 paths and `main`'s 130 never observed. |
| stdout/stderr split | Strong for `list`/`task` diagnostics; **missing**: `init`'s `initialised traces in …`, `index`'s `indexed N file(s)`, `trust`'s `trusted {root}`, completions stderr-empty. |
| Cross-process persistence (A writes, B reads) | **4 genuine tests** (`untrust.rs:14`, `tracked.rs:19,:40`, `golden_path.rs:45`); index persistence is **not** one of them — harness `is_file()` only. |
| argv parse failure (usage + exit 2) | **NONE** at process level (only in-crate `try_parse_from`). |
| env isolation honored (`TRACES_STATE_DIR`/`XDG_CONFIG_HOME`) | **NONE** — `Sandbox` exposes no accessor for state/config dirs (`support.rs:97-132`), so the assertion cannot even be written today. |
| `traces init` as a process | **NEVER SPAWNED** (in-process `Init.run`, `init.rs:34,45,58`). |
| `trust list` / `trust --show` / `trust clean` / `template --list` / `completions --list-templates` stdout | **ZERO assertions anywhere** (F G3) — while `tracked list` *is* asserted, proving the harness supports it. |

Other findings:

- **`dispatch.rs` is a catch-all by runner heritage, not architecture** (C §2, H
  §4.2): its module doc claims `trust` coverage it does not have (trust appears
  only as fixture at `:18,:39,:79,:105`); `trust_and_diagnostics` mixes index
  persistence with error rendering. Sibling files (`template_write.rs`,
  `tracked.rs`, `untrust.rs`) were already split *by behavior* — the suite
  itself uses the better taxonomy elsewhere. (Note: production `Commands::run`
  (`src/cli/mod.rs:186-197`) is an exhaustive match — there is no code
  catch-all.)
- **~10 of the 20 `dispatch.rs` tests re-derive in-process content at process
  cost** (F D12): keep 6 genuinely-new-boundary + 4 thin-but-defensible (Miette
  rendering), collapse `query_commands` from 9 to 3 (channel split /
  exact-stdout bytes / unique fallthrough).
- **Fixtures silently perform the behavior under test** (C §4):
  `Sandbox::trusted()` spawns `traces trust` in **24 of 27** tests (failure
  attribution: "fixture setup failed"); all 3 completions tests use it though
  `Completions --shell` never loads config (`src/cli/completions.rs:45-53`);
  `write_config()` writes exactly what `init` would, so no dispatched test
  depends on real init output; `PresetDialogProvider` fakes the dialog so
  `TerminalDialogProvider` (`src/cli/mod.rs:215-217`) is never exercised
  end-to-end.
- **cwd race is real only under threaded libtest**: `CwdGuard::enter` is
  unlocked (`support.rs:243-256`); `init`/`golden_path` race inside the one
  `e2e` binary under `cargo test`, neutralized by nextest's process-per-test
  under the canonical runner. Worst-case interleave could write `.traces/`
  **into the repo checkout** (A §1). The doc's cross-binary worry is *spurious*
  (separate processes); `support.rs:41-45`'s sentence is garbled and
  self-contradicting (C §3).

## 7. In-crate component & unit layers — findings

- **9 of 45 `src/cli/mod.rs` tests drive `Cli::try_parse_from(..).run(...)`
  in-process**, none capture stdout — they are dispatch/diagnostic tests, not
  E2E duplicates (D1 §1a, **accepted partially**). Two
  (`template_query_ops_render_identically…`, `derived_inlinks_are_queryable…`)
  don't touch `Cli::run` at all — cross-component tests sitting in a CLI file.
- **`table_reflects_a_note_edit…` (D1 §3.2) — assertion doesn't verify its own
  claim.** **Confirmed by lead**: after the second `Cli::run`, the test builds
  its own store via `IndexerService::for_tests(&project).refresh_store()`
  (`src/cli/mod.rs:1282-1284`) — which re-reads and re-syncs from disk itself. A
  no-op `Cli::run` still passes. Fix: assert via `load()` (no refresh). The
  sibling `indexing_then_page_and_task_queries…` (`:1159`) has the same shape —
  its `Cli::run` results are `CommandOutcome::Completed` only; content
  assertions run against independently built indexes (`:1179-1194`).
- **The in-crate cwd locking discipline holds** (D1 §1b, **rejected** the
  contrary claim): every cwd-mutating test takes `CWD_TEST_LOCK`
  (`src/cli/cwd.rs:81-85`); `set_current_dir` occurs only inside the guard. The
  guard is `pub(crate)` and unreachable from `tests/` — hence the external
  harness has no lock at all (§8).
- **Assertion-gap smoke tests:**
  `list/table/task::run::succeeds_for_a_trusted_project_root` etc. assert only
  `Ok` (D1 §3.6) — DELETE/REPLACE candidates.
- **Dead fixture pattern:** `rows_for_files(_temp, …)` ignores its `_temp`
  parameter (`src/query/mod.rs:110-115`, verified) — callers create tempdirs
  nobody uses: **45 in `results.rs`, 22 in `grammar/filter.rs`, 7 in `sort.rs`**
  (D2 §2.1 REPLACE).
- **Exact duplicate across the crate wall:**
  `src/schema/service.rs::descendants::returns_a_
  transitive_descendant_through_an_intermediate_schema` (`:825`) ≡
  `tests/integration/schema_field_resolution.rs::descendants_of_returns_transitive_extenders`
  (`:103`) — same files, same chain, same assertion (verified by lead). DELETE
  the unit copy (or the integration file, per §10-D4).

## 8. Harness, fixtures & isolation — findings

1. **Behavior-hidden helpers (the Act is in arrangement):**
   - `create_trusted_project` (`src/lib.rs:592`): mkdir + config write + **trust
     write** in one — and its doc (`:581-583`) claims it creates
     `root/templates`; **code never does** (verified: `:596-611`), while
     `Sandbox::write_config` does (`support.rs:141`) ⇒ fixture divergence (C4).
   - `Sandbox::trusted` runs the CLI under test (E §7) — acceptable but rename
     (`trusted_via_cli`).
   - `TestProject::config()` *looks* like an accessor but selects bypass
     semantics (no discovery,
     no trust) and flips config based on directory existence (`lib.rs:319`).
2. **Hidden couplings** (E §6): 24 integration sites call `TestProject::trusted` yet never consult
   the trust store (trust is decorative there — a trust change breaks unrelated tests; a weakened
   `trusted()` would *not* be caught); config TOML literal hard-coded in 3 places
   (`lib.rs:271,:602`, `support.rs:145`); `write_template` requires `write_config` first.
3. **Environment isolation gaps:**
   - `Sandbox::command` sets `TRACES_STATE_DIR` + `XDG_CONFIG_HOME` only
     (`support.rs:187-194`, verified). **`TRACES_CEILING_DIRS` /
     `TRACES_IGNORED_DIRS` are inherited, not overridden**
     (`src/env_vars.rs:5,23`) — a shell exporting them changes indexing in every
     E2E child *and* in integration tests (they're `LazyLock` statics read
     during `build`).
   - **Windows config home is not isolated (Revision 2, missed in R1):** the
     Windows resolver uses `APPDATA` (`src/dirs.rs:114-117`), consulted at
     `src/config/discovery.rs:416` — the sandbox's `XDG_CONFIG_HOME` is ignored,
     so a Windows E2E child reads the runner's real
     `%APPDATA%\traces\config.toml` while `support.rs:13-15,:95-96` claim
     hermeticity. Latent: CI runs E2E on ubuntu only
     (`.github/workflows/ci.yml:61`). State side *is* hermetic everywhere
     (`src/dirs.rs:161-162`). Fix: sandbox must set the platform config-home
     vars.
   - If the `TRACES_STATE_DIR` plumbing broke, state would silently fall back to
     the real host and all 24 `trusted()` tests stay green — nothing asserts the
     child saw the var.
   - `TzGuard` is unreachable from `tests/` (`#[cfg(test)] pub(crate)`); no `TZ`
     pin in Sandbox.
4. **Zero shared fixture code between integration and E2E** — they independently re-implement
   config TOML, write helpers, dir constants (E §8). Rightly so for *behavior* constructors; share
   only pure arrangement (path constants, config const, safe-path join — E2E currently has no path
   guard at all).
5. **`impl Default for ConfigService` is `pub` under `test-utils`** and points at the *real* OS
   state dirs (`src/config/service.rs:578`) — gate or remove (E #8).

## 9. Duplication analysis (F, cross-checked)

Standard: *same layer, same boundary, same observable* = duplication. Vertical
redundancy that proves a new failure boundary (exit code, stream split,
cross-process state) = keep.

| ID | Duplication | Disposition |
|---|---|---|
| D1 | `config_lifecycle.rs` two tests byte-equivalent (**lead-verified**) | delete the `TestProject`-based one |
| D2 | integration `persist_then_load_recovers_the_same_file_count_and_paths` ⊂ unit roundtrip (unit asserts *more*) | delete integration (keep `preserves_query_rows_and_list_metadata_across_cold_reload` — unique: re-query after source deleted) |
| D3 | `tasks`-vs-`lists` differential proven twice at integration layer | keep, merge assertions |
| D4 | `renders_file_sourced_select_field…` byte-identical content at integration + E2E | keep integration; repoint E2E at `--output` or drop |
| D5/D6 | E2E checkbox-render & sorting re-prove units at 4th layer | keep 1 E2E stdout representative, drop sorting E2E |
| D7 | same `did you mean list.completed?` stderr twice in E2E (`:217` vs `:442`) | delete the `--column` twin |
| D8 | E2E `task_preserves_custom_markers…` = 4 units concatenated | **keep — this is the model** (one exact-stdout contract) |
| D9 | 3 E2E completions tests = 3 unit marker tests | keep 1 (zsh `starts_with`), drop 2 |
| D10 | trust/untrust asserted at 4 layers; integration is the weakest | drop one/both integration twins |
| D11 | never-trusted vs untrusted-then-relapsed E2E | **keep both** (distinct boundaries) |
| D12 | `dispatch.rs`: 10 of 20 tests only re-derive in-process content | collapse `query_commands` 9 → 3 |

**Net waste: ~10 E2E + ~3 integration tests ≈ <2 % of the suite.**

## 10. Coverage gaps (ranked by defect-class uniqueness × production exposure)

**HIGH**

- **G1 — `Commands::Init` arm (`src/cli/mod.rs:187`) never executed at any
  layer.** Parse test stops at `matches!`; E2E calls `Init.run` directly.
  Cheapest fix: one component test
  `Cli::try_parse_from(["traces","init"]).run(&service, PresetDialogProvider)`.
  (F G1, C #1.)
- **G-B1 — production query cold path untested from `tests/`:** `service.load` →
  `refresh_query` → `refresh_store` → `run_from_store` → `with_class_expander`:
  **zero hits in `tests/`** — even `sync_and_run`, the `pub` stand-in built for
  this, is never called by an integration test.
- **G-B4 — `ConfigService::load` never reached from `tests/`:** no test proves
  *config TOML → behavior*. All integration `Config`s are fixture-computed ⇒
  `[tasks] tag_filters`, `[schemas] class_field` (all 15
  `QueryService::new("class")` sites hard-coded), `[templates] directory` have
  no `tests/` coverage of their disk form. Related **F G6**: tag filters are
  always injected in code (`TaskConfig::from_tags`), never read from
  `.traces/config.toml`. **Executability caveat (Revision 2):** `load` is
  `pub(crate)` (`src/config/service.rs:190`); spawned e2e children *do* reach it
  via `cli::run → load_config`, so the gap means "never **asserted** on config
  TOML → behavior from `tests/`", not "never executed". Do not widen the
  production API — see §14 P1.7 for the three ranked routes.
- **G-B2 — `@Class` source expansion:** `rg "from\('@'" tests/` → zero; all 27
  integration `SourceSelector`s are `All`. Schema-diagnostics warnings also
  untested outward.
- **G2 — Inlinks: 46 unit tests, zero `tests/` coverage** of a public query
  field (`rg -rin "inlink" tests/` → no matches; the sole wikilink fixture
  `[[todo]]` at `index_query.rs:148` is unasserted).
- **No numeric exit-code assertion anywhere** (C matrix row 1) + **argv failure
  contract (usage + exit 2) has zero process coverage**.

**MEDIUM**

- **G-B3/G4 — incremental refresh:** the only "edit between two invocations"
  proof is the flawed in-crate test (§7); no integration test does
  build→persist→edit→refresh→assert-changed; no E2E edits a note between two
  spawned runs.
- **G3 — five stdout-producing commands with zero stdout assertion** (`trust
  list/--show/clean`, `template --list`, `completions --list-templates`).
- **G7 — index-corruption recovery has no process-level contract** (does `list`
  exit non-zero or silently rebuild? nobody asserts).
- **Env isolation unverifiable** (§8.3) — assertion impossible without a
  `Sandbox` accessor.
- **G-B5/G-B6** — typed-value filter/sort after cold reload; row-level fail-open
  (unit-only, `pub(super)` seam).

**OPERATIONAL**

- **Silent 0-test green** for bare `cargo test` (verified); `mise run test
  --feature default` documented but broken (verified); `mise watch` runs the
  wrong suite (§4).
- **E2E tests a `test-utils` build** (Revision 2): `mise run test` →
  `--all-features`, so the spawned binary has `test-utils` on. All 63 cfg sites
  are visibility/lint-canary/test-only-ctor — **zero behavior-affecting
  branches**, so runtime is identical to production today; the defect is the
  *guarantee* (black-box suite must test the shipped configuration), and that
  the feature-off configuration is only ever compiled by the docs job.
- **No mechanical layer enforcement** (Revision 2): nothing in `hk.pkl`,
  `.mise/tasks/`, `clippy.toml`, or CI greps `tests/` for layer violations — the
  in-process-init regression proves conventions don't hold themselves (§14
  P0.5).

## 11. Doc-comment & claim accuracy audit

Claims that exceed what the test executes (all lead-verified unless noted):

| Claim | Reality |
|---|---|
| `src/cli/mod.rs:1278-1284` "confirm the edit reached the persisted store — exactly what the CLI just queried" | assertion path itself re-syncs from disk; a no-op `Cli::run` passes |
| `dispatch.rs:10-15,31-36` "proves … survives process exit" | only harness `is_file()`; no second `traces` process reads the store |
| `dispatch.rs:31-36` "would catch a future refactor swapping `refresh()` for `build()`" | unprovable — both persist a file |
| `golden_path.rs:37` "eight process spawns" | six `run(...)` calls + one in-process `Init` |
| `golden_path.rs:38-43` "proves `index`'s output is what `list` reads" | `list` cold-rebuilds (`refresh_query` cold path) — not established |
| `golden_path.rs:41-43` "confirms `init` alone doesn't establish trust" | no command runs before the `trust` step |
| `dispatch.rs:1-2` file covers `trust` | zero trust-command assertions in the file |
| `dispatch.rs:51-54` "the right exit code" | asserts `!is_success()` only |
| `src/lib.rs:581-583` `create_trusted_project` "creating that directory" (`root/templates`) | never created (`:596-611`) |
| `src/cli/mod.rs:1278` "same `run_from_store` seam table uses" | true, but the refresh invalidates the claim it decorates |
| `support.rs:41-45` cargo-test parallelism sentence | garbled; cross-binary half of the race worry is impossible |
| `support.rs:37` references `src/cwd.rs` | actual path `src/cli/cwd.rs` |
| `index_persistence_roundtrip.rs:2,21` "promoted from `src/index/mod.rs::persist_then_load…`" | test doesn't exist anywhere |
| `.mise/tasks/test/_default:38` `--test init_cli` | no such target; `mise.toml:64` → `.cargo/mutants.toml` doesn't exist |

## 12. Naming & taxonomy (H)

- **Organizing rule (Revision 2, sharpened):**
  > Source-local tests own exhaustive semantics and private component contracts.
  > Integration tests own cross-component seams. E2E tests own only contracts
  > introduced by the executable/process boundary.
  > 
  > **Unit files name components. Integration files name the seam** whose
  > failure requires ≥2 production components to be correct.
  >
  > **E2E files name externally observable capabilities.**
  > Integration and E2E need not share filenames — their reasons for existing
  > differ.
- **Trees are derived AFTER pruning**, not before: classify →
  unique-failure-boundary check → delete/merge → identify gaps → add missing
  cases → only then name files. (The audit plan already sequences this: T4
  deletions → T7 renames; T7 depends on T4. §12 trees below are therefore the
  *shape*, instantiated at T7 from whatever survives T4 — the R1 note that §12's
  trees "keep everything" is expected: H was produced pre-pruning.)
- **Integration target shape (post-pruning, ~5-6 files):** `index_persistence` ·
  `index_refresh` (fs mutation → delta → redb → query-visible — a genuine seam,
  not benchmark naming) · `index_query` (only high-value composition canaries:
  real fs → query, cold `sync_and_run`, inlinks crossing the boundary, typed
  data after reload) · `task_classification` · `template_render` · + **one**
  config file *if and only if* the config-TOML→behavior test (§14 P1.7) lands
  here rather than in E2E.
- **E2E target shape:** capability files (`query`, `template`, `trust`,
  `tracked`, `completions`, `init`) + `support.rs` + a *generic
  process-contract* file (exit 0/1/2, usage, stream split). Feature-specific
  diagnostics stay with their capability file; the generic file must not be
  named `cli.rs` (meta-word, violates the rule below) — candidates:
  `process_contract.rs` or keep `dispatch.rs` with its scope narrowed to
  argv/exit only. **No `cli_diagnostics.rs` catch-all** (Revision 2: grouping by
  "rendered by Miette" is the same runner-heritage mistake as `dispatch.rs`; H's
  proposal rejected).
- **`workspace_workflow`: rename not endorsed; `golden_path` → delete *after*
  replacement preconditions exist** (Revision 2): it is 1 of the 4 certified
  cross-process tests (§6), and the two replacements the critique assumes —
  cross-process index-read and edit-between-two-spawns — do not exist and are
  now scheduled as §14 P1.10/12. Until those land, keep (rename at T7).
- **`schema_field_resolution.rs` → DELETE outright (Revision 2, stronger than
  R1):** all 5 tests use the test-only ctor (`SchemaService::new`), source twins
  exist for all 5, and its only residue (pub-visibility of 4 methods) is a
  visibility test, not an integration seam. Schema's outward seams belong to
  `template_render` + the new `@class` test. R1's "delete 1 dup + rename"
  disposition superseded.
- **`config_trust.rs` → ELIMINATE (Revision 2, resolving §9 D10's hedge):** both
  tests only assert config-file-exists + `untrust→1` (one is a verified dup);
  units cover the store (`src/config/service.rs:830,903,1075`) and E2E proves
  cross-process trust observation (`tests/e2e/untrust.rs:14`, `dispatch.rs:56`,
  plus `Sandbox::trusted` in 24 tests). Spend that budget on
  config-TOML→behavior instead. (Caveat: test 1 is the only external caller of
  the `pub fn untrust` — keep a single trust-store smoke assertion somewhere if
  `untrust` is part of the intended pub surface.)
- **Vocabulary violations to rename:** `vault` and `checkbox line` are explicit
  glossary `*Avoid*` (`classifies_multi_note_vault_lifecycle_…`,
  `task_prints_a_checkbox_line_per_task`); `roundtrip`/`lifecycle` are
  index-glossary words being used for config; `dispatch` is scoped in the
  glossary to `Commands::run` only.

## 13. Tooling & CI findings (G)

| ID | Finding | Fix |
|---|---|---|
| F1 | `mise watch` = bare `cargo test` ⇒ 0 integration tests + cwd race | `-x 'nextest run --all-features'` |
| F2 | documented `--feature default` hard-errors (verified) | emit no feature flags for `default`, or drop the promise |
| F3 | plain `cargo test` silently green with 0 integration tests (verified) | `[[test]] name="integration" required-features=["test-utils"]` (silent skip — still not loud) *or* `compile_error!` in `tests/integration.rs` (loud) *or* command-level guard + CI assertion |
| F4 | no timeouts anywhere; `status-level=fail` hides slow markers | `slow-timeout = {period=60s, terminate-after=4}` + `global-timeout`; consider `status-level="slow"` |
| F5 | CI never runs default-feature config or non-Linux tests (`src/dirs.rs:101-160` mac/win branches untested) | add `nextest run --no-default-features` step + `--test e2e` on OS matrix |
| F6 | layered suites exist on disk but not in the runner (one pool, one job) | split CI test job: `--test e2e` vs rest |
| F7 | `test:unit` name lies (runs e2e too); `-m <mod>` silently drops integration+e2e | rename/describe; add `binary(/^(integration\|e2e)$/)` or document |
| F8 | coverage/mutation local-only; `min_msi` commented out; doctests excluded from coverage | decide: gate or document as advisory |

---

## 14. Prioritized recommendations (Revision 2)

**P0 — correctness & invariants (small, immediate)**

1. **FIX** `table_reflects_a_note_edit…`: assert via `IndexerService::…load()`, not
   `refresh_store()` (D1 §3.2). Same-shape check for `indexing_then_page_and_task_queries…`.
2. **REWRITE the overclaiming doc comments** per §11 (dispatch.rs ×3, golden_path ×3, support.rs ×2,
   `create_trusted_project`, stale `index_persistence_roundtrip` pointer).
3. **Guard against silent 0-test runs:** `compile_error!`/`[[test]]` + a CI assertion that the
   canonical run is non-empty (F3).
4. **E2E INVARIANT — compiled binary only, no exceptions (critique #1):** every test under
   `tests/e2e/**` executes product behavior only through the spawned `traces-pkm` binary.
   - Move the 3 in-process `Init.run` calls (`init.rs:34,45,58`) out:
     preset/custom-path assertions become a **component test in `src/cli/`**
     (spawning can't reproduce them — `TerminalDialogProvider` ignores stdin on
     non-TTY, `src/dialog/terminal.rs:51-53`; only the default path is spawnable
     without a PTY). A genuine default-path `traces init` E2E test is then
     possible (stdin NUL/`</dev/null`-style, assert exit 0/1 + `initialised
     traces in …` stderr).
   - Drop `golden_path.rs:53`'s in-process step the same way.
   - **Delete `CwdGuard` from `tests/e2e/`** (`support.rs:234-256`) once no test
     cds in-process — children already use `.current_dir()` (`support.rs:190`).
     This **supersedes R1's P3#16/T4.3 mutex proposal**: remove the mutator
     instead of locking it.
5. **Layer-enforcement checks (critique #12, missed by R1):** a cheap `rg`-based mise task + CI
   step enforcing: `tests/e2e/**` has zero `use traces_pkm::` and zero `std::process` bypasses
   outside `support.rs`; `tests/integration/**` has zero `std::process::Command` /
   `CARGO_BIN_EXE`; no `env::set_current_dir` anywhere (`clippy.toml` disallow-methods too).

**P1 — close HIGH gaps (new tests, highest defect-class value)**

6. **Numeric exit-code assertions** for the mapped paths (0/1; clap 2 via a bad-argv spawn; 130 if
   reachable) — extend `Run` with `code()`; this also serves the critique's "generic process
   contract" file (bad argv → exit 2 + usage; domain failure → exit 1 + stderr; success → 0).
7. **Config TOML → behavior test (critique #4 — route decision required first):**
   `ConfigService::load` is `pub(crate)` (`src/config/service.rs:190`); `ConfigBuilder`,
   `ConfigLoadError` are also `pub(crate)`. Ranked routes — **do not widen the production API:**
   a. **Preferred:** E2E — spawn `traces` in a sandbox whose `.traces/config.toml` carries
   `[tasks] tag_filters` / `[schemas] class_field` / `[templates] directory` and assert the
   behavior difference (config loading is already on the production composition path there).
  . b. Fallback: narrow explicitly test-only adapter under `test-utils` exposing *only* the
   loaded-and-resolved outcome, not error/typestate internals.
   c. Rejected: exporting `load`/`ConfigLoadError` for tests.
8. **Inlinks integration test** through `list("inlinks")` (G2).
9. **`@class` source expansion integration test** with a schema dir (G-B2).
10. **Incremental-refresh pair:** integration test (edit → refresh → assert delta) + **E2E edit
    note between two spawned `list` runs** (G-B3/G4) — the second E2E is also a *precondition* for
    any future `golden_path` deletion.
11. **Trust/template/completions stdout assertions** at process level using the existing `Sandbox`
    fixture (G3) + one `trust --all` subtree E2E (G8).
12. **Cross-process index-read E2E (new — critique #11 precondition):** first spawned process
    writes, second spawned process *reads and asserts content* (not harness `is_file()`). Without
    this, the certified cross-process set (§6) shrinks if `golden_path` is later deleted.
13. **Env isolation as a harness contract:** `Sandbox` accessors (`state_dir()`,
    `config_home()`); assert the child sees `TRACES_STATE_DIR`; override `TRACES_CEILING_DIRS` /
    `TRACES_IGNORED_DIRS`; **set platform config-home vars — `APPDATA` on Windows** (critique #7,
    §8.3); `TZ=UTC`.
14. **`sync_and_run` integration test** — the pub stand-in for the CLI cold path is itself unused
    (G-B1).

**P2 — consolidation (delete/merge, §9)**

15. D1, D2, D4, D6, D7-pair, D9, D12-collapse; DELETE the `schema::descendants` unit dup and the
    D1 `src/lib.rs:918` fixture dup; REPLACE the ~74 dead tempdirs (drop `_temp` param).
16. **DELETE `schema_field_resolution.rs` entirely** (§12 — stronger than R1's dup-only deletion;
    its 5 tests have source twins through a test-only ctor).
17. **ELIMINATE integration `config_trust`/`config_lifecycle`** (§12 — resolve D10's hedge; keep at
    most one `pub fn untrust` smoke assertion if that API is intended).
18. Fix `tracked.rs:49` `stderr.contains('1')` → assert on a structured message.

**P3 — harness & facade redesign (critique #5, #13, #14, #15)**

19. **`pub mod testing` facade (critique #5, missed by R1):** replace the flat root-pub export
    block (`src/lib.rs:88-162`, 44 names) with `#[cfg(feature = "test-utils")] pub mod testing { … }`
    so integration tests import `traces_pkm::testing::{TestProject, QueryService, …}` and the
    architectural status of the surface is unambiguous. Keep paired root `pub(crate)` aliases for
    in-crate use; re-path ~5 doctests, 12 bench modules, 6 integration files. Expose **only the
    seams integration deliberately needs**, not automatic internal exports.
20. **`tests/common/` for pure arrangement (critique #13):** config TOML literal (currently
    triplicated: `lib.rs:271,:602`, `support.rs:145`), dir constants, safe-path join — shared via
    `mod common;` in both roots (feature-independent literals only; `integration.rs` is gated,
    `e2e.rs` is not). **Behavior constructors stay layer-local** (`TestProject` in facade,
    `Sandbox` in e2e): integration trusts via facade, e2e trusts only by spawning `traces trust`
    (already true — zero facade trust in `tests/e2e`).
21. **Explicit fixtures (critique #14):** `write_minimal_config(root)` / a small
    `ConfigFixture{tasks,schemas,templates}.write()` builder; a test must *declare* "this scenario
    has task filters" instead of `TestProject::config()` flipping behavior on directory existence
    (`src/lib.rs:317-324`) or `Sandbox::write_config` silently creating `templates/`
    (`support.rs:141-147`).
22. Split `create_trusted_project` (arrange vs trust); fix its `templates/` doc/behavior divergence;
    rename `TestProject::config`; gate `impl Default for ConfigService`.
23. **Scope `expect_used` properly (critique #15, corrected):** the blanket allow
    (`tests/integration.rs:6-11`) is *vestigial* — `clippy.toml:86-87` already allows
    expect/unwrap in all tests, so removing the attribute changes nothing (the critique's remedy
    is a no-op). The real fix is **style**: convert test-body `.expect` on behavior-under-test
    (e.g. `index_persistence_roundtrip.rs:32,65,210`) to `Result`-returning tests or assertions;
    keep `expect` only in fixture construction. R1 never ticketed this despite B §4.4.1.
24. Decouple integration fixtures from trust where trust is never consulted; drop
    `Sandbox::trusted()` from the 3 completions tests.

**P4 — runner & CI (§13, critique #6):**

25. **Feature-separated phases:** unit/component on the production configuration; integration with
    `--features test-utils`; **E2E without `test-utils`** (spawned binary = shipped config);
    doctests appropriate to default. `mise run test` becomes an aggregate over the phases. Rationale
    is the *guarantee*, not current behavior (zero behavior-affecting cfg branches today).
26. F1 (`mise watch` → nextest), F2 (`--feature default`), F4 (nextest timeouts + status-level),
    F5 (default-feature run + non-Linux E2E — now *blocked on* the Windows `APPDATA` fix in P1.13),
    F6 (split CI test job), F7 (`-m`/`test:unit` docs), F8 (coverage/mutation decision).

**P5 — naming/taxonomy (§12):** pruning-first trees, glossary renames, `golden_path` disposition
evaluated only after P1.10/12 land.

---

## 15. Implementation plan (Revision 2)

Ordering rationale: honesty fixes first (everything after builds on true
claims), then the E2E invariant (it changes *where* new E2E tests can live, so
gap tests come after it), then gap tests, then deletions (validated against the
enlarged suite), then facade/harness restructuring (touches every integration
file — do once), runner/CI parallelizable, naming last. Prune **before** naming:
file trees (§12) are instantiated at T7 from the surviving inventory.

| Phase | Tickets | Depends on | Verification gate |
|---|---|---|---|
| **T1 — honesty** | 1.1 FIX `table_reflects…` assertion (use `load()`) · 1.2 FIX `indexing_then…` or scope its doc · 1.3 rewrite §11 doc claims (14 sites) · 1.4 silent-0-test guard + CI non-empty assertion | — | `mise run test`; bare `cargo test` now fails/skips loudly |
| **T2 — E2E invariant** | 2.1 move preset/custom-path `Init` assertions to in-crate component test · 2.2 spawn default-path `traces init` E2E (exit code + stderr) · 2.3 remove in-process `Init` from `golden_path` step 1 · 2.4 delete `CwdGuard` from `tests/e2e/` · 2.5 layer-enforcement `rg` task + CI step · 2.6 clippy disallow `env::set_current_dir` | T1 | `rg 'use traces_pkm' tests/e2e` → 0 (outside removed files); `mise run test` |
| **T3 — gap tests** | 3.1 numeric exit codes + `Run::code()` + generic process-contract file · 3.2 config-TOML→behavior via **E2E route** (per P1.7a) · 3.3 inlinks integration · 3.4 `@class` expansion integration · 3.5 refresh delta integration + edit-between-spawns E2E · 3.6 trust/template/completions stdout E2E (+`trust --all`) · 3.7 **cross-process index-read E2E** (P1.12) · 3.8 Sandbox accessors + env contract incl. **Windows `APPDATA`** · 3.9 `sync_and_run` integration | T2 (E2E must be spawn-only first) | `mise run test`; each ticket names its defect class |
| **T4 — pruning** | 4.1 delete D1/D2/`schema::descendants`/`lib.rs:918` dups · 4.2 **DELETE `schema_field_resolution.rs`** (all 5) · 4.3 **ELIMINATE `config_lifecycle.rs`** (keep ≤1 `untrust` smoke elsewhere) · 4.4 E2E collapses (query_commands 9→3, completions 3→1, D6/D7) · 4.5 dead-tempdir REPLACE (74) · 4.6 fix `tracked clean` assertion · 4.7 **do NOT delete `golden_path` yet** — evaluate after 3.5+3.7 land | T3 (deletions can't mask new gaps) | `mise run test`; test-count delta reviewed (expect −25…−40) |
| **T5 — facade & harness** | 5.1 `pub mod testing` facade (re-path doctests/benches/integration) · 5.2 `tests/common/` pure arrangement (TOML literal, dir constants, safe-path join) · 5.3 explicit fixtures (`write_minimal_config`/`ConfigFixture`) · 5.4 split `create_trusted_project` + templates parity · 5.5 rename `TestProject::config` · 5.6 decouple trust fixtures + completions trim · 5.7 expect-style pass (test-body `.expect` → assertions) · 5.8 gate `Default for ConfigService` · 5.9 `TZ=UTC` | T4 (renames/deletions settle what the facade must expose) | `mise run test` + `mise run lint` + `mise run check` |
| **T6 — runner & CI** | 6.1 feature-separated phases (unit default / integration `test-utils` / **E2E no test-utils** / doctests) with `mise run test` as aggregate · 6.2 `mise watch` → nextest · 6.3 `--feature default` fix · 6.4 nextest timeouts + status-level · 6.5 CI: default-feature run, non-Linux e2e (after `APPDATA` fix), split test job · 6.6 `-m`/`test:unit` docs · 6.7 stale-pointer cleanup · 6.8 coverage/mutation decision | 3.8 (Windows fix before OS-matrix e2e) | CI green on a branch; each phase's command re-verified |
| **T7 — naming** | 7.1 instantiate §12 post-pruning trees (split `dispatch.rs` → capability files + process-contract file) · 7.2 glossary renames (`vault`, `checkbox line`, `roundtrip`/`lifecycle`, `golden_path`) · 7.3 `golden_path` delete-vs-rename decision (needs 3.5+3.7 evidence) | T4 (prune first) | `mise run test`; `rg` for old names returns 0 |

Each ticket is one PR-sized change with its evidence citation from this report; T3 tickets each
carry a "defect class prevented" note (the audit's evidence standard applied forward).

---

## 16. External critique adjudication (Revision 2)

Four review agents checked an external critique of R1 against the code. Verdicts:

**Adopted (critique correct, R1 missed or got wrong):**

| # | Critique point | R1 status | Evidence |
|---|---|---|---|
| 1 | E2E spawn-only invariant; kill in-process init + `CwdGuard` | Diagnosis right, **remedy wrong** (R1 proposed a cwd mutex) | `init.rs:34,45,58`; children already `.current_dir()` (`support.rs:190`) |
| 4 | `ConfigService::load` is `pub(crate)` → R1's recommendation unexecutable as written | R1 dropped B's caveat | `src/config/service.rs:190`; routed to E2E (P1.7) |
| 5 | `pub mod testing` facade instead of flat root-pub exports | **Missed** | 44 names at `src/lib.rs:88-162`; feasible (paired `pub(crate)` aliases stay) |
| 6 | E2E runs a `test-utils` build | Facts present, **not framed as boundary defect** | `.mise/tasks/test/unit:53`; 0 behavior-affecting cfg branches |
| 7 | Windows `APPDATA` isolation gap | **Missed** (C §7 endorsed the false hermeticity claim) | `src/dirs.rs:114-117` vs `support.rs:191` |
| 8 | Delete `schema_field_resolution.rs` entirely | R1 kept+renamed | 5 test-only-ctor tests, all with source twins |
| 9 | Eliminate integration `config_trust` | R1 hedged (D10 "one/both") | units `src/config/service.rs:830,903,1075`; E2E `untrust.rs:14` |
| 12 | Mechanical layer enforcement | **Missed** | no check in `hk.pkl`/mise/clippy/CI |
| 13/14 | `tests/common` + explicit fixtures | Substantially covered (§8.4/E §8); mechanism new | TOML literal ×3 |
| 15 | `expect_used` allow should go | Issue existed (B §4.4.1), **never ticketed** | — |
| 2 | Trees before pruning | **Partially wrong**: R1's T6 already depended on T3 (now T7←T4) | §15 R1; only §12 doc ambiguity |
| 11 | Delete `golden_path` | **Preconditions unmet** — adopted *conditional* (T4.7/T7.3), replacements scheduled (P1.10/12) | cross-process index-read + edit-between-spawns don't exist |

**Softened / corrected in critique:**

- **test-utils "not production config"** overstated: all 63 cfg sites are
  visibility/lint-canary/ test-only-ctor — **zero runtime behavior change**; the
  defect is the *guarantee* (P4.25), not today's behavior.
- **`expect_used` remedy is a no-op:** `clippy.toml:86-87` already allows
  expect/unwrap in all tests; the fix is assertion *style* (P3.23), and the
  blanket attribute is vestigial.
- **`cli_diagnostics` not a bare Miette grab-bag** (glossary/ADR 0004-grounded)
  — but R1 adopts the split anyway for the reason §6 itself gives: generic
  process contract separate, feature diagnostics with capabilities; critique's
  `cli.rs` name rejected as a meta-word (§12).
- Critique factual errors: `tests/e2e/trusted.rs` doesn't exist; the 5-file
  integration tree is the critic's own synthesis (H said 6, B said 8); e2e
  *does* import the crate (`init.rs:12`, `golden_path.rs:15` — which T2
  removes); "trust only by spawning" already holds.

---

## 17. Appendix — verification ledger

**Lead-verified this session (independent re-read or live run):**

| Claim | Verdict | Evidence |
|---|---|---|
| Suite green: counts & feature behavior | **CONFIRMED** | `mise run test` 3,127+76; nextest list 30/27/3,070; `cargo test --test integration` rc0 w/ 0 tests; nextest rc4; `--features default` error |
| `table_reflects…` assertion passes if `Cli::run` is a no-op | **CONFIRMED** | `src/cli/mod.rs:1282-1301` calls `refresh_store()` itself; `src/index/service.rs:226-245` re-syncs |
| `config_lifecycle.rs` byte-equivalent pair | **CONFIRMED** | both files read; `TestProject::trusted`→`create_trusted_project`, `untrust`→same call |
| `create_trusted_project` doc promises `templates/` it doesn't create | **CONFIRMED** | `src/lib.rs:581-583` vs `:596-611` |
| `tracked clean` assertion near-vacuous (`contains('1')`) | **CONFIRMED** | `tests/e2e/tracked.rs:49` |
| `Sandbox::command` doesn't override `TRACES_CEILING_DIRS`/`TRACES_IGNORED_DIRS` | **CONFIRMED** | `support.rs:187-194` vs `src/env_vars.rs:5,23` |
| No numeric exit-code assertion in `tests/` | **CONFIRMED** | `rg 'code()|status.code' tests/` → only `ExitStatus` plumbing + diagnostic-*code* string test |
| `traces init` never spawned; in-process in E2E | **CONFIRMED** | `init.rs:34,45,58`; `rg 'run(&\["init"\])' tests/` → none |
| `SchemaService::new` "dead in production" | **CONFIRMED with nuance** | `src/schema/service.rs:49-66`: `expect(dead_code)` wrapper over production `load_verbose` (`:78`, called at `src/template/engine.rs:131`) — tests exercise production logic through a test-only seam, not divergent code. A's "production pub API" label is wrong; B's circularity critique stands |
| `dispatch.rs:17/:38` persistence claims | **CONFIRMED with nuance** | harness `is_file()` after child exit proves durable *write*, but no second product process reads it; `:38`'s refresh-vs-build claim unprovable; `golden_path`'s stronger claim also unprovable (cold-path rebuild) |
| `schema::descendants` unit test = integration twin | **CONFIRMED** | `src/schema/service.rs:825` ≡ `tests/integration/schema_field_resolution.rs:103` |
| Dead tempdir pattern | **CONFIRMED** (mechanism; counts from D2) | `src/query/mod.rs:110-115` `rows_for_files(_temp, …)` ignores param |
| `Commands::run` catch-all | **REJECTED as code claim / CONFIRMED as taxonomy** | match is exhaustive (`src/cli/mod.rs:186-197`); the "catch-all" is `tests/e2e/dispatch.rs` grouping by runner heritage (C §2) |
| In-crate cwd mutation without `CWD_TEST_LOCK` | **REJECTED** | D1 §1b: `set_current_dir` only inside guard (`src/cli/cwd.rs:81-85,114`); discipline holds |
| Parent-cwd race (E2E) | **CONFIRMED, bounded** | unlocked `CwdGuard` (`support.rs:243-256`); live only under threaded libtest (`mise watch`); neutralized by nextest process-per-test |
| Fixture-hidden trust behavior | **CONFIRMED** | `Sandbox::trusted()` runs `traces trust` in 24/27 E2E (`support.rs:215-225`); 24 integration sites use `TestProject::trusted` yet never consult trust (E C1); trust stdout contract asserted nowhere except `golden_path` gate |
| Test-facade exposure | **CONFIRMED** | 63 `test-utils`/`cfg(test)` sites (G §2.2); behavior-hidden helpers (E §7, B §4.2) |

**Sub-reports:** `A-inventory` (57-test table, spawn/global-state census) ·
`B-integration` (30-test verdict table, G-B1..B8) · `C-e2e` (contract matrix,
fixture acts, ranked findings) · `D1-src-cli-config` (critical-claim verdicts,
~452-test classification) · `D2-src-core` (REPLACE/DELETE/KEEP actions) ·
`E-harness` (couplings, hidden Acts, 10 recommendations) · `F-dup-gap`
(behavior×layer matrix, D1–D12, G1–G8) · `G-tooling` (build/run/CI, F1–F8) ·
`H-naming` (glossary synthesis, proposed trees).
