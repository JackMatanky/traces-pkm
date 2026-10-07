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

> **Revision 4 (§16c):** a third external critique was adjudicated by four
> review agents. Adopted: **never pin exit 101** (panic artifact — real contract
> is 0/1/clap-2/130, and 130 *is* reachable via dialog interrupt), `index` +
> `config` E2E capability homes, conditional `task_classification`, a T4.0
> duplication recompute step, stage-separated config-failure arrangements
> (trust-before-parse means E-8's naive arrangement would fail with
> `config_build_untrusted`), `store_backed_query_parity` rename + persist-first
> arrangement, sharpened staleness repro (re-trust is mandatory), test-count
> gate → behavior ledger. Rejected: "no observable cross-process divergence
> exists" (three recipes exist — config-change, same-size+mtime, `chmod 000`),
> the critique's own persistence-fallback set, "only one T4 dependency",
> "config errors are one generic scenario".

> **Revision 3 (§16b):** a second candidate-discovery pass (two codegraph-driven
> reviewers re-mapping the full public API and CLI surface) added 11 new test
> candidates, corrected 5 gaps the audit mis-stated (notably: a direct `@class`
> integration test would be *vacuous* — `with_class_expander` is `pub(crate)`;
> the inlinks gap is mostly a unit twin; clap's exit 2 is unreachable in-crate),
> and surfaced 2 potential product defects (config-change staleness, EPIPE).
> §10/§12/§14/§15 updated in place.

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
  this, is never called by an integration test. **Absorbed (R3, scoped R4):**
  closed by the store-backed parity differential, §14 P1.14 — but only its
  store legs; `ConfigService::load` and `with_class_expander` legs → P1.7a/P1.9.
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
  untested outward. **Route correction (Revision 3, lead-verified):** a direct
  `QueryService` integration test is **vacuous** — `with_class_expander` is
  `pub(crate)` (`src/query/service.rs:82`), `QueryService::new` leaves the
  expander `None` (`:75`), and `ClassExpansionMode` starts empty
  (`src/query/grammar/source.rs:326`), so `from('@X')` from `tests/` can never
  match. The only external composition roots that attach the expander are
  `TemplateService` (`src/template/engine/query.rs`) and the CLI dispatch
  (`src/cli/mod.rs`) → test via template render + E2E `--from '@…'`.
- **G2 — Inlinks: 46 unit tests, zero `tests/` coverage** of a public query
  field (`rg -rin "inlink" tests/` → no matches; the sole wikilink fixture
  `[[todo]]` at `index_query.rs:148` is unasserted).
  **Demoted (Revision 3, lead-verified):** `src/cli/mod.rs:1342::derived_inlinks_are_queryable…`
  already asserts both the `list("inlinks")` query string (`:1364`) and the
  identical template render — a plain integration `list("inlinks")` test would
  near-duplicate it. The real residue is **inlinks on the persisted/cold path**
  (`LINKS` reconstruction) → fold into G-B1's differential (§14 P1.14).
- **No numeric exit-code assertion anywhere** (C matrix row 1) + **argv failure
  contract (usage + exit 2) has zero process coverage**.

**MEDIUM**

- **G-B3/G4 — incremental refresh:** the only "edit between two invocations"
  proof is the flawed in-crate test (§7); no integration test does
  build→persist→edit→refresh→assert-changed; no E2E edits a note between two
  spawned runs. **Sharpened (Revision 3):** units assert store *report counts*
  (`src/index/service.rs:1075…1588`), never **query rows** across a refresh —
  the integration form must assert through the public query API. And the
  content-edit branch is only half the story: **delete/rename exercises the
  path-set full-recompute branch** (`src/index/service.rs:40-42`,
  `src/index/delta.rs:38-58`) — a distinct E2E candidate (E-6).
- **G3 — five stdout-producing commands with zero stdout assertion** (`trust
  list/--show/clean`, `template --list`, `completions --list-templates`).
  **Extended (Revision 3):** also the *mutating* commands — `trust`, `untrust`,
  `index`, `template -i … --no-input` are spawned but never stream-asserted;
  their negative contract (`stdout == ""`, status on stderr) has zero coverage.
  Assert exact formats, not `contains` (`trust list` = `path\tstate`,
  `src/cli/trust.rs:115`).
- **G7 — index-corruption recovery has no process-level contract** (does `list`
  exit non-zero or silently rebuild? nobody asserts). **Concrete (Revision 3):**
  no test ever mutates the redb file's bytes; the code notes a real-file
  fixture is missing (`src/index/store.rs:1818-1821`).
- **Env isolation unverifiable** (§8.3) — assertion impossible without a
  `Sandbox` accessor.
- **G-B5/G-B6** — typed-value filter/sort after cold reload; row-level fail-open
  (unit-only, `pub(super)` seam).

**NEW in Revision 3 (from the second candidate-discovery pass):**

- **E-1 — overwrite refusal never proven non-destructive at process level
  (HIGHEST blast radius).** `template -i X --no-input` over an existing file
  must exit 1 + `traces::cli::template::output_exists` (`src/cli/error.rs:565`)
  with the file byte-identical; `-o`/`-f` argv paths are never spawned (in-crate
  tests construct the `Template` struct directly, `src/cli/template.rs:464-505`;
  only the `-n`/`-o` conflict is argv-tested, `:769`).
- **E-2 — corrupt-index self-heal across processes:** `index` → overwrite
  `.traces/index.redb` with garbage → `list` must exit 0 with correct stdout and
  a valid file again. Catches exit-101 panic, permanent `index::failed`
  (`src/cli/error.rs:307`), silent empty results.
- **E-3 — clap-layer process contract:** `Cli::parse()` (`src/cli/mod.rs:217`)
  never returns — clap writes usage and `process::exit(2)` directly, while every
  in-crate test uses `try_parse_from` which *returns* the error. **Exit 2 is
  structurally unreachable in-crate.** Unknown subcommand / missing required
  flag / invalid value → exit 2 + usage; `--help`/`--version` → exit 0 +
  correct stdout. **Revision 4 correction:** pin **0, 1, clap-2, and 130** —
  130 is reachable via `DialogError::UserInterrupted` (`src/dialog/error.rs:93`
  → `src/cli/error.rs:230-238` → `main.rs:27-29`), no signal handler exists.
  Assert representative valid/error paths **never return 101**: it is an
  unwitnessed panic artifact (no `ExitCode::from(101)` anywhere; release
  `panic = "abort"` would give 134 anyway, `Cargo.toml:289`), revisited only if
  the EPIPE contract is decided.
- **E-4 — non-TTY stdin never proven deterministic:** every spawned template
  test passes `--no-input` (which swaps in `PresetDialogProvider`,
  `src/cli/template.rs:204-210`); the real `TerminalDialogProvider` is never
  exercised at process level, and its in-crate tests *skip* when stdin is a TTY
  (`src/dialog/terminal.rs:123-131`). Catches CI hangs and TTY-guard
  regressions.
- **E-8 — config error paths (R4 arrangement correction):** `ConfigService::build`
  verifies trust **before** parsing (`src/config/service.rs:238-256`, typestate
  `LocalConfigFile<Parsed>` only from `Trusted`, `src/config/file.rs:290-297`),
  and trust hashes **without parsing** (`service.rs:322-336`). So the naive
  arrangement (write malformed config, spawn untrusted) fails with
  `config_build_untrusted`, **not** a parse failure. Reachable stages, each with
  its own code (`src/cli/error.rs:540-555`):

  | Stage | Arrangement from spawned process | stderr code |
  |---|---|---|
  | discovery (no local config) | spawn in tree without `.traces/config.toml` | `config_discovery_failed` |
  | trust: never-trusted | write config, never `traces trust` | `config_build_untrusted` (`Untrusted`) |
  | trust: stale hash | `traces trust` → edit config → spawn | `config_build_untrusted` (`Stale`) |
  | trust: baseline missing | `traces trust` w/o config → create config → spawn | `config_build_untrusted` (`MissingBaseline`) |
  | parse local | write malformed → **`traces trust` first** → spawn | `config_build_config_file_failed` |
  | parse global | trusted valid local + malformed `$XDG_CONFIG_HOME/traces/config.toml` | `config_build_config_file_failed` |
  | field-key validation | trusted config, empty/whitespace key | `config_build_invalid_field_key` |
  | merge/validation | trusted config, bad `[tasks]` tag | `config_build_config_file_failed` |

  Process-level coverage today: only `config_build_untrusted`
  (`tests/e2e/dispatch.rs:65`, `untrust.rs:22`); no test asserts `Stale`,
  `config_build_config_file_failed`, or `config_discovery_failed` anywhere in
  `tests/`. Codes asserted in-process only (`src/cli/error.rs:806,:846`).
- **I-1 — store-backed query parity differential (absorbs G-B1's store legs;
  renamed from "cold/warm" in R4):** the same
  `SourceSelector`s (`#tag`, nested tags, paths, `(#a or notes/b.md)`, `not #t`)
  × modes through `QueryService::run` vs `sync_and_run`, assert structural
  equality (`QueryRow::eq`, `src/query/results.rs:422,:695`). **R4 fixes:**
  (a) pin the arrangement — persist first, then `sync_and_run` with an
  unchanged workspace, so the `RefreshState::Fresh` from-disk branch
  (`src/index/refresh.rs:277-301`) is guaranteed; otherwise the differential
  degenerates into querying rows written in the same call. (b) `sync_and_run`
  = `refresh_store()` + `run_from_store()` (`src/query/service.rs:178-185`) —
  it proves *in-memory vs refreshed-store* parity, hence the rename. (c) it
  does **not** absorb G-B1's `ConfigService::load` or `with_class_expander`
  legs (those → P1.7a / P1.9). The only unit twin
  is All-source-lists-only (`src/query/service.rs:1235`). Catches `SourceResolver`
  prefilter false-negatives (silent wrong answers,
  `src/query/service.rs:354-384`), stale `PATHS_BY_TAG`/`FILE_CLASS`, codec
  drift, and **cold-path inlinks** (the real residue of G2).
- **I-4 — cross-render freshness:** the regression is *named in code* with no
  guard — a cached field "would wrongly persist across independent renders"
  (`src/template/engine/query.rs:97-100`, `src/template/engine.rs:128`); all 5
  integration template tests are single-render.
- **I-5 — template output re-enters the index:** writer → fs → refresh → query
  feedback loop untested (write units test path resolution only; index units
  read pre-existing files).

**POTENTIAL PRODUCT DEFECTS surfaced (not just test gaps):**

- **Config-change staleness:** `RefreshPlan::collect` (`src/index/refresh.rs:277-295`,
  lead-read) fingerprints **only file metadata** — no config hash — and
  `is_fresh()` short-circuits on an empty file delta (`:299-301`).
  `IndexDimensions::for_class_field` is supplied **only on the Stale arm**
  (`src/index/service.rs:234-236`), so config changes never reach the store
  while the plan is Fresh. **R4 sharpened repro (R4, traced end-to-end,
  deterministically failing today):** config A → `traces trust` → `traces
  index` → edit `class_field` (or a tag key / task filter) → **`traces trust`
  again** (mandatory — a stale config blocks all loads with
  `config_build_untrusted`, `service.rs:232-233`) → query *untouched* notes.
  Symptom shape depends on selector: class-field changes give false
  **negatives** (stale rows masked by `is_match`'s current-field re-check,
  `src/query/grammar/source.rs:221-227`); tag/task-filter changes give false
  **positives** (persisted `note.tags()` trusted, `:217-219`; task parsing
  bakes config at index time, `src/index/service.rs:277-282`). **Untested at
  any layer.** Early reproduction ticket (T3b.1) → then narrowest regression.
- **`refresh()` fail-open has no test at any layer** (`src/index/service.rs:84-108`
  documents it) — but it is *not inducible from the public API* (`IndexStore::open`
  re-creates the file). Disposition: **in-crate unit**, not integration.
- **EPIPE/exit-101 on `traces list | head`:** no signal handling anywhere in
  `src/`; Rust's default `print!` panics on EPIPE → exit 101. **Open question,
  not a ticket** — decide the intended contract first.

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
  not benchmark naming) ·   `index_query` (only high-value composition canaries:
  real fs → query, store-backed `sync_and_run` parity differential, cold-path inlinks,
  typed data after reload) · `task_classification` **conditional (R4):** all 3
  current tests (`tests/integration/task_tag_filters.rs`) have unit twins (B
  rows 23–25; parser `src/note/parser/task.rs:717,910`, pipeline
  `src/query/service.rs:1168`) and the real gap — config-TOML→classification —
  moves to E2E (P1.7a); default disposition per D3/F-row-14 is **fold into
  `index_query`**, retain as a file only if a post-T3 test keeps a failure
  boundary not unit-twin'd · `template_render` · + **one**
  config file *if and only if* the config-TOML→behavior test (§14 P1.7) lands
  here rather than in E2E.
- **E2E target shape (R4: complete capability set):** capability files
  (`query`, `template`, `trust`, `tracked`, `completions`, `init`, **`index`**,
  **`config`**) + `support.rs` + a *generic process-contract* file (exit
  0/1/2/130, usage, stream split). **`index`** is a first-class subcommand
  (`src/cli/mod.rs:156-158`) with homes for: `dispatch.rs:17,:38`, E-2
  corruption self-heal, T3.5 refresh/index branches, `indexed N file(s)`
  stderr split, mutating-`index` `stdout == ""`. **`config`** homes: T3.2
  config-TOML→behavior + the E-8 stage table + T3.12 global config — distinct
  codes (`config_discovery_failed`, `config_build_untrusted` ×3 statuses,
  `config_build_config_file_failed`, `config_build_invalid_field_key`) justify
  a capability file rather than reuse of `trust`. Feature-specific
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
  now scheduled as §14 P1.10. Until those land, keep (rename at T7).
- **`schema_field_resolution.rs` → DELETE outright (Revision 2, stronger than
  R1):** all 5 tests use the test-only ctor (`SchemaService::new`), source twins
  exist for all 5, and its only residue (pub-visibility of 4 methods) is a
  visibility test, not an integration seam. Schema's outward seams belong to
  `template_render` + the new `@class` test. R1's "delete 1 dup + rename"
  disposition superseded. **Revision 3 caveat:** it is the only integration file
  exercising `SchemaService` reachability from outside — schedule the deletion
  *in the same ticket* that lands the TemplateService `@class` test (§14 P1.9),
  so reachability coverage transfers rather than vanishing.
- **`config_trust.rs` → ELIMINATE (Revision 2, resolving §9 D10's hedge):** both
  tests only assert config-file-exists + `untrust→1` (one is a verified dup);
  units cover the store (`src/config/service.rs:830,903,1075`) and E2E proves
  cross-process trust observation (`tests/e2e/untrust.rs:14`, `dispatch.rs:56`,
  plus `Sandbox::trusted` in 24 tests). Spend that budget on
  config-TOML→behavior instead. (Caveat: test 1 is the only external caller of
  the `pub fn untrust` — keep a single trust-store smoke assertion somewhere if
  `untrust` is part of the intended pub surface. **Revision 3 disposition:**
  confirmed — fold one ~5-line `untrust` assertion into another integration
  file rather than deleting the pub-surface check entirely.)
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
| F3 | plain `cargo test` silently green with 0 integration tests (verified) | **R4 pick:** `[[test]] name="integration" required-features=["test-utils"]` (repo precedent: all 12 benches, `Cargo.toml:43-98`) — hard-errors on explicit `cargo test --test integration`, still silently skips bare `cargo test`; **plus** CI non-empty assertion as the guard for bare runs. `compile_error!` rejected (breaks featureless `cargo test`/IDE builds) |
| F4 | no timeouts anywhere; `status-level=fail` hides slow markers | `slow-timeout = {period=60s, terminate-after=4}` + `global-timeout`; consider `status-level="slow"` |
| F5 | CI never runs default-feature config or non-Linux tests (`src/dirs.rs:101-160` mac/win branches untested) | add `nextest run --no-default-features` step + `--test e2e` on OS matrix |
| F6 | layered suites exist on disk but not in the runner (one pool, one job) | split CI test job: `--test e2e` vs rest |
| F7 | `test:unit` name lies (runs e2e too); `-m <mod>` silently drops integration+e2e | rename/describe; add `binary(/^(integration\|e2e)$/)` or document |
| F8 | coverage/mutation local-only; `min_msi` commented out; doctests excluded from coverage | decide: gate or document as advisory. **R4 addition:** after T2/T3, run *targeted* `test:mutants -m <module>` against the production modules the new E2E seams claim to protect; `src/cli` stays excluded (`mutarust.yml:21-22`), so E2E-covered CLI code gets no mutation signal — process-level fault injection where that matters |

---

## 14. Prioritized recommendations (Revision 4)

**P0 — correctness & invariants (small, immediate)**

1. **FIX** `table_reflects_a_note_edit…`: assert via `IndexerService::…load()`, not
   `refresh_store()` (D1 §3.2). Same-shape check for `indexing_then_page_and_task_queries…`.
2. **REWRITE the overclaiming doc comments** per §11 (dispatch.rs ×3, golden_path ×3, support.rs ×2,
   `create_trusted_project`, stale `index_persistence_roundtrip` pointer).
3. **Guard against silent 0-test runs (R4 mechanism):** `[[test]] name="integration"
   required-features=["test-utils"]` (repo precedent — all 12 benches already do
   this, `Cargo.toml:43-98`; hard-errors on explicit `cargo test --test integration`)
   **plus** a CI assertion that the canonical run is non-empty (F3). `compile_error!`
   rejected — only it is loud on bare `cargo test`, at the cost of breaking
   featureless `cargo test`/rust-analyzer.
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

6. **Numeric exit-code assertions — corrected (R4):** extend `Run` with
   `code()`, `is_failure()`, exact-stdout and stderr-predicate assertions
   (keep `sandbox.run(...)` calls direct in each test). Pin **0, 1, clap-2,
   and 130** — 130 is reachable via dialog interrupt
   (`src/dialog/error.rs:93` → `main.rs:27-29`); **assert representative
   valid/error paths never return 101** (panic sentinel, not a contract — no
   `ExitCode::from(101)` anywhere; release `panic = "abort"` → 134,
   `Cargo.toml:289`). Add the clap-layer contract file: unknown subcommand →
   exit 2 + usage on stderr; missing required flag (`table` w/o `--column`);
   invalid value (`completions --shell tcsh`); `--help`/`--version` → exit 0
   on stdout. Structural note: exit 2 is *unreachable in-crate* (`Cli::parse`
   vs `try_parse_from`). Serves the generic process-contract file (bad argv →
   2; domain failure → 1; success → 0; interrupt → 130).
7. **Config TOML → behavior (critique #4 + R3 additions + R4 stages):**
   `ConfigService::load` is `pub(crate)` (`src/config/service.rs:190`);
   `ConfigBuilder`, `ConfigLoadError` are also `pub(crate)`; `SchemasConfig` /
   `FrontmatterConfig` are unnameable from `tests/` (`mod config` private,
   `src/lib.rs:62`, exports at `:89`). **Do not widen the production API.**
   Ranked routes:
   a. **Preferred:** E2E — spawn `traces` in a sandbox whose `.traces/config.toml`
      carries `[tasks] tag_filters` / `[schemas] class_field` / `[templates]
      directory` and assert the behavior difference (config loading is on the
      production composition path there; visibility is irrelevant when spawning).
   b. Fallback: narrow explicitly test-only adapter under `test-utils` exposing
      *only* the loaded-and-resolved outcome, not error/typestate internals.
   c. Rejected: exporting `load`/`ConfigLoadError` for tests.
   **R4:** split by pipeline stage with per-stage arrangements (full table in
   §10 E-8) — discovery / trust (3 statuses) / parse local (trust the
   malformed bytes first — `traces trust` hashes without parsing) / parse
   global / field-key / merge. **Not one generic scenario**; `ConfigFixture`
   needs raw-TOML writing alongside its structured builder for the invalid
   cases. Note `config_build_untrusted` already has process coverage
   (`dispatch.rs:65`); the other five codes have none.
8. ~~Inlinks integration test~~ **Demoted (R3):** near-dup of
   `src/cli/mod.rs:1342` (lead-verified) — cold-path inlinks absorbed into #14.
9. **`@class` source expansion — route rewritten (R3, lead-verified):** the
   integration test must go through the **`TemplateService` composition root**
   (only external path attaching the expander): write schemas to default
   `.traces/schemas/`, render `query.from("@book*")` / `class(Book, children)`
   forms via `render_to_file(.., DryRun)`, assert transitive-`extends` rows +
   unknown-class degradation. A direct `QueryService` test would assert
   `0 == 0`. Pair with an E2E `traces list --from '@book*'` (separate CLI path,
   `src/cli/mod.rs:438-452`). **Gates the `schema_field_resolution` deletion
   (§12).**
10. **Refresh family (R3 consolidation of G-B3/G4 + cross-process):**
    - *Integration:* build → persist → edit tag/body → assert **query-visible**
      delta through the public query API (units only assert report counts).
    - *E2E content branch:* edit between two spawned `list` runs (precondition
      for `golden_path` deletion).
    - *E2E path-set branch (new):* delete + rename between spawns → ghost rows
      gone, inlinks re-resolved (`src/index/service.rs:40-42`) — distinct
      recompute path from content edits.
    - *Cross-process index-read (R4: divergence precondition):* vanilla
      content assertions on an unchanged workspace are **indistinguishable
      from a rebuild** (no verbose/log signal — tracing has no subscriber in
      the binary; `traces index` always rebuilds). The ticket must pick a
      scenario where reload ≠ rebuild: **(a)** `chmod 000` a source after
      indexing → spawned `list` exits 0 with rows (reads persisted) while
      `traces index` exits 1 (`FileMeta` unchanged → Fresh path never opens
      content); **(b)** the T3b.1 config-staleness repro (stale rows returned
      only if persisted). Deletes/renames/corruption do **not** qualify —
      they rebuild equivalently (critique's fallback rejected). Persist→load
      *fidelity* stays in `index_persistence` (integration), where
      `persist_then_load…` already owns the codec boundary.
11. **Stdout/stream contracts — expanded (R3):**
    - listing commands (G3): exact formats, not `contains` (`trust list` =
      `path\tstate`, `trust --show` = bare root);
    - `trust --all` + companion `untrust --all`, count asserted via follow-up
      spawned `trust list`;
    - **mutating commands' negative contract (new):** `stdout == ""` for
      `trust`, `untrust`, `index`, `template -i … --no-input`; status text on
      stderr.
12. **E-1 — overwrite refusal (NEW, highest blast radius):** seed an existing
    output file; `template -i X --no-input` → exit 1 + `output_exists` + file
    byte-identical; `-o` → exit 0 writes elsewhere; `-f` → replaced.
13. **Env isolation as a harness contract (R4: scrub by default):**
    `Sandbox::command` currently *inherits* the parent env with additive
    overrides (`support.rs:187-194`) — switch to `env_clear` + sandbox-managed
    `TRACES_STATE_DIR` / `XDG_CONFIG_HOME` / platform config-home / `TZ=UTC`;
    dedicated `config.rs` tests opt into `TRACES_CEILING_DIRS` /
    `TRACES_IGNORED_DIRS` explicitly when testing those contracts (§8.3).
    **Set platform config-home vars — `APPDATA` on Windows** (critique #7).
14. **I-1 — store-backed query parity differential (R4 rename of "cold/warm",
    rewrite of G-B1):** same `SourceSelector`s × modes through
    `QueryService::run` vs `sync_and_run`, assert `run == sync_and_run`
    (structural `QueryRow::eq`). Representative selector set, not a 50-case
    sweep. **Arrangement: persist first, then sync with an unchanged workspace**
    so the `RefreshState::Fresh` from-disk branch is guaranteed — otherwise
    the test queries rows it just wrote. Catches `SourceResolver`
    false-negatives, stale secondary indexes, codec drift, **and cold-path
    inlinks (#8)**. Does *not* cover `ConfigService::load` or the class
    expander legs (→ #7a, #9).

**P1 — MEDIUM additions (Revisions 3–4)**

15. **E-2 — corrupt-index self-heal E2E:** overwrite redb with garbage →
    `list` exits 0, correct stdout, file valid again (sharpens G7).
16. **E-4 — non-TTY stdin determinism:** spawned `template` without
    `--no-input` → real `TerminalDialogProvider` on null stdin: picker refusal
    exit 1, or default-path render exit 0; test terminates (no hang).
17. **E-5 / I-5 / I-4:** stdout-split already in #11; then template-output
    re-enters index (I-5), cross-render freshness (I-4 — named-in-code
    regression with no guard, `src/template/engine/query.rs:97-100`).
18. **E-7 — global config via child env:** config_home accessor + write global
    `[templates] directory` → spawned `template --list` sees it (gated on #13).
19. **Config-change staleness — early reproduction (R4 promoted from
    "investigate"):** run the §10 sharpened repro (config A → trust → index →
    edit `class_field`/tag key → **re-trust** → query untouched notes), assert
    the config-B expectation; determine intended behavior first, then keep the
    regression at the narrowest level capturing config → derived-index
    invalidation, with one E2E canary if process composition matters.
    **`refresh()` fail-open → in-crate unit** (not integration); **EPIPE →
    open question**, not a ticket.
20. **E-9 (optional):** `trust ../project` positional from outside the root —
    guards a trust-scope safety class (positional silently ignored → trusting
    cwd); low priority since durability is already implied by `Sandbox::trusted()`.

**P2 — consolidation (delete/merge, §9)**

> **R4:** the D1–D12 list below is precomputed *pre-T3*; applying it requires
> the T4.0 recompute step first — new T3 tests change the redundancy graph
> (e.g. a real corruption E2E may obsolete
> `index_persistence_roundtrip.rs:309`; new config E2Es may finish off
> `task_tag_filters.rs`).

21. D1, D2, D4, D6, D7-pair, D9, D12-collapse; DELETE the `schema::descendants` unit dup and the
    D1 `src/lib.rs:918` fixture dup; REPLACE the ~74 dead tempdirs (drop `_temp` param).
22. **DELETE `schema_field_resolution.rs` entirely** (§12 — stronger than R1's dup-only deletion;
    its 5 tests have source twins through a test-only ctor; **lands with P1.9** per §12 caveat).
23. **ELIMINATE integration `config_trust`/`config_lifecycle`** (§12 — resolve D10's hedge; keep at
    most one `pub fn untrust` smoke assertion if that API is intended).
24. Fix `tracked.rs:49` `stderr.contains('1')` → assert on a structured message.

**P3 — harness & facade redesign (critique #5, #13, #14, #15)**

25. **`pub mod testing` facade (critique #5, missed by R1):** replace the flat root-pub export
    block (`src/lib.rs:88-162`, 44 names) with `#[cfg(feature = "test-utils")] pub mod testing { … }`
    so integration tests import `traces_pkm::testing::{TestProject, QueryService, …}` and the
    architectural status of the surface is unambiguous. Keep paired root `pub(crate)` aliases for
    in-crate use; re-path ~5 doctests, 12 bench modules, 6 integration files. Expose **only the
    seams integration deliberately needs**, not automatic internal exports. (R3 note: this is also
    the natural place to expose `TemplateService` composition for the P1.9 test.)
26. **`tests/common/` for pure arrangement (critique #13):** config TOML literal (currently
    triplicated: `lib.rs:271,:602`, `support.rs:145`), dir constants, safe-path join — shared via
    `mod common;` in both roots (feature-independent literals only; `integration.rs` is gated,
    `e2e.rs` is not). **Behavior constructors stay layer-local** (`TestProject` in facade,
    `Sandbox` in e2e): integration trusts via facade, e2e trusts only by spawning `traces trust`
    (already true — zero facade trust in `tests/e2e`).
27. **Explicit fixtures (critique #14):** `write_minimal_config(root)` / a small
    `ConfigFixture{tasks,schemas,templates}.write()` builder; a test must *declare* "this scenario
    has task filters" instead of `TestProject::config()` flipping behavior on directory existence
    (`src/lib.rs:317-324`) or `Sandbox::write_config` silently creating `templates/`
    (`support.rs:141-147`).
28. Split `create_trusted_project` (arrange vs trust); fix its `templates/` doc/behavior divergence;
    rename `TestProject::config`; gate `impl Default for ConfigService`.
29. **Scope `expect_used` properly (critique #15, corrected):** the blanket allow
    (`tests/integration.rs:6-11`) is *vestigial* — `clippy.toml:86-87` already allows
    expect/unwrap in all tests, so removing the attribute changes nothing (the critique's remedy
    is a no-op). The real fix is **style**: convert test-body `.expect` on behavior-under-test
    (e.g. `index_persistence_roundtrip.rs:32,65,210`) to `Result`-returning tests or assertions;
    keep `expect` only in fixture construction. R1 never ticketed this despite B §4.4.1.
30. Decouple integration fixtures from trust where trust is never consulted; drop
    `Sandbox::trusted()` from the 3 completions tests.

**P4 — runner & CI (§13, critique #6):**

31. **Feature-separated phases:** unit/component on the production configuration; integration with
    `--features test-utils`; **E2E without `test-utils`** (spawned binary = shipped config);
    doctests appropriate to default. `mise run test` becomes an aggregate over the phases. Rationale
    is the *guarantee*, not current behavior (zero behavior-affecting cfg branches today).
32. F1 (`mise watch` → nextest), F2 (`--feature default`), F4 (nextest timeouts + status-level),
    F5 (default-feature run + non-Linux E2E — now *blocked on* the Windows `APPDATA` fix in P1.13),
    F6 (split CI test job), F7 (`-m`/`test:unit` docs), F8 (coverage/mutation decision +
    **R4 targeted post-redesign mutation runs** on the modules the new E2E seams protect —
    `src/cli` stays excluded, so CLI-covered seams need process-level fault injection).

**P5 — naming/taxonomy (§12):** pruning-first trees, glossary renames, `golden_path` disposition
evaluated only after P1.10 lands.

---

## 15. Implementation plan (Revision 4)

Ordering rationale: honesty fixes first (everything after builds on true
claims), then the E2E invariant (it changes *where* new E2E tests can live, so
gap tests come after it), then gap tests, then deletions (validated against the
enlarged suite), then facade/harness restructuring (touches every integration
file — do once), runner/CI parallelizable, naming last. Prune **before** naming:
file trees (§12) are instantiated at T7 from the surviving inventory.

| Phase | Tickets | Depends on | Verification gate |
|---|---|---|---|
| **T1 — honesty** | 1.1 FIX `table_reflects…` assertion (use `load()`) · 1.2 FIX `indexing_then…` or scope its doc · 1.3 rewrite §11 doc claims (14 sites) · 1.4 silent-0-test guard: `[[test]] required-features` + CI non-empty assertion (P0.3, R4) | — | `mise run test`; bare `cargo test` now fails/skips loudly |
| **T2 — E2E invariant** | 2.1 move preset/custom-path `Init` assertions to in-crate component test · 2.2 spawn default-path `traces init` E2E (exit code + stderr) · 2.3 remove in-process `Init` from `golden_path` step 1 · 2.4 delete `CwdGuard` from `tests/e2e/` · 2.5 layer-enforcement `rg` task + CI step · 2.6 clippy disallow `env::set_current_dir` | T1 | `rg 'use traces_pkm' tests/e2e` → 0 (outside removed files); `mise run test` |
| **T3 — gap tests** | 3.1 exit codes **0/1/clap-2/130 + never-101** + `Run::code()`/`is_failure()`/stdout-stderr predicates + process-contract file (P1.6, R4) · 3.2 config-TOML→behavior via **E2E route** (P1.7a) + **stage-split** config error paths w/ per-stage arrangements (E-8 table; R4) → `config.rs` · 3.3 **E-1 overwrite refusal** (P1.12) · 3.4 **TemplateService `@class` integration** + paired E2E `--from '@…'` (P1.9) · 3.5 refresh family: query-visible delta integration + edit-between-spawns + delete/rename path-set E2E + **cross-process index-read with divergence precondition** (P1.10, R4) → `index.rs` · 3.6 stdout/stream contracts: listings exact-format + `trust --all` + mutating-commands `stdout==""` (P1.11) · 3.7 Sandbox accessors + **env scrub-by-default** incl. **Windows `APPDATA`** (P1.13, R4) · 3.8 **store-backed query parity differential**, persist-first arrangement, incl. cold-path inlinks (P1.14, R4) · 3.9 E-2 corrupt-index self-heal · 3.10 E-4 non-TTY stdin · 3.11 I-4 cross-render + I-5 template-output-reenters-index · 3.12 E-7 global config via child env (after 3.7) | T2 (E2E must be spawn-only first) | `mise run test`; each ticket names its defect class |
| **T3b — investigation** | 3b.1 **config-change staleness — early reproduction (R4):** run the §10 sharpened repro (config A → trust → index → edit class_field/tag key → **re-trust** → query untouched notes); determine intended behavior, then narrowest regression · 3b.2 `refresh()` fail-open → in-crate unit · 3b.3 **EPIPE contract decision** (open question, may spawn a ticket) | parallel with T3 | written disposition in the audit or an issue |
| **T4 — pruning** | **4.0 R4: re-run §9/F's duplication analysis against the post-T3 inventory before applying anything below** — explicitly re-evaluate `index_persistence_roundtrip.rs:309` (vs new corruption E2E), `dispatch.rs:17/:38` (vs 3.5), the D2-keep test, and `task_tag_filters.rs` (vs 3.2) · 4.1 delete D1/D2/`schema::descendants`/`lib.rs:918` dups · 4.2 **DELETE `schema_field_resolution.rs`** (all 5 — **only after 3.4**, §12) · 4.3 **ELIMINATE `config_lifecycle.rs`/`config_trust.rs`** (fold one `untrust` smoke elsewhere, §12) · 4.4 E2E collapses (query_commands 9→3, completions 3→1, D6/D7) · 4.5 dead-tempdir REPLACE (74) · 4.6 fix `tracked clean` assertion · 4.7 **do NOT delete `golden_path` yet** — evaluate after 3.5 lands | T3 (deletions can't mask new gaps) | `mise run test`; **behavior→layer→defect-class→surviving-test ledger** — every deleted test marked redundant or replaced by a stronger boundary test (R4: test counts are bookkeeping, not a gate) |
| **T5 — facade & harness** | 5.1 `pub mod testing` facade (re-path doctests/benches/integration) · 5.2 `tests/common/` pure arrangement (TOML literal, dir constants, safe-path join) · 5.3 explicit fixtures (`write_minimal_config`/`ConfigFixture` **+ raw-TOML writing for invalid cases**, R4) · 5.4 split `create_trusted_project` + templates parity · 5.5 rename `TestProject::config` · 5.6 decouple trust fixtures + completions trim · 5.7 expect-style pass (test-body `.expect` → assertions) · 5.8 gate `Default for ConfigService` | T4 (renames/deletions settle what the facade must expose) | `mise run test` + `mise run lint` + `mise run check` |
| **T6 — runner & CI** | 6.1 feature-separated phases (unit default / integration `test-utils` / **E2E no test-utils** / doctests) with `mise run test` as aggregate · 6.2 `mise watch` → nextest · 6.3 `--feature default` fix · 6.4 nextest timeouts + status-level · 6.5 CI: default-feature run, non-Linux e2e (after `APPDATA` fix), split test job · 6.6 `-m`/`test:unit` docs · 6.7 stale-pointer cleanup · 6.8 coverage/mutation decision + **targeted post-redesign `test:mutants -m` runs** (R4, F8) | 3.7 (Windows fix before OS-matrix e2e) | CI green on a branch; each phase's command re-verified |
| **T7 — naming** | 7.1 instantiate §12 post-pruning trees (split `dispatch.rs` → capability files incl. **`index`/`config`** + process-contract file, R4) · 7.2 glossary renames (`vault`, `checkbox line`, `roundtrip`/`lifecycle`, `golden_path`) · 7.3 `golden_path` delete-vs-rename decision (needs 3.5 evidence) · 7.4 `task_classification` final disposition per §12 R4 clause (fold vs retain) | T4 (prune first) | `mise run test`; `rg` for old names returns 0 |

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
| 11 | Delete `golden_path` | **Preconditions unmet** — adopted *conditional* (T4.7/T7.3), replacements scheduled (P1.10) | cross-process index-read + edit-between-spawns don't exist |

**Softened / corrected in critique:**

- **test-utils "not production config"** overstated: all 63 cfg sites are
  visibility/lint-canary/ test-only-ctor — **zero runtime behavior change**; the
  defect is the *guarantee* (P4.31), not today's behavior.
- **`expect_used` remedy is a no-op:** `clippy.toml:86-87` already allows
  expect/unwrap in all tests; the fix is assertion *style* (P3.29), and the
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

## 16b. Revision 3 adjudication — second candidate-discovery pass

Two codegraph-driven reviewers re-mapped the full public API and CLI surface
for test candidates the audit missed. Findings were then critically reviewed by
the lead (spot verification via `codegraph_explore` + targeted greps) before
being folded into §10/§12/§14/§15.

**Corrections to the audit (adopted):**

| # | Audit said | Correction (verified) |
|---|---|---|
| 1 | G-B2 `@class` gap → integration test on `QueryService` | **Would be vacuous:** `with_class_expander` is `pub(crate)` (`src/query/service.rs:82`), `QueryService::new` leaves the expander `None`, `ClassExpansionMode` starts empty (`src/query/grammar/source.rs:326`) → `from('@X')` from `tests/` can never match. Route via `TemplateService` composition root + E2E `--from '@…'` (§14 P1.9) |
| 2 | G2 inlinks → standalone integration test | **Near-duplicate:** `src/cli/mod.rs:1342` unit already asserts `list("inlinks")` (`:1364`). Real residue = inlinks on the persisted/cold path → folded into the P1.14 parity differential |
| 3 | G-B1 route "use `sync_and_run`" as one test | Generalized into the parity differential (P1.14) — representative selector set. **R4:** renamed `store_backed_query_parity`, persist-first arrangement required |
| 4 | G-B4 rationale mixed API-width with executability | Sharpened: `mod config` is private (`src/lib.rs:62`), exports only 4 names (`:89`) → `SchemasConfig`/`FrontmatterConfig` unnameable from `tests/` regardless of `load`; E2E route (P1.7a) is correct precisely because spawning sidesteps visibility |
| 5 | No numeric exit codes | Added structural note: exit **2 is unreachable in-crate** — `Cli::parse` (`src/cli/mod.rs:217`) calls clap which `process::exit(2)`s directly; `try_parse_from` returns. Process-level only (E-3). ~~pin **101** too~~ — **R4 superseded:** never-101 is now a *negative* assertion, pin 0/1/2/130 |

**New candidates adopted:** E-1 overwrite refusal (highest blast radius — only
in-crate struct-level tests exist, `src/cli/template.rs:464-505`), E-2
corrupt-index self-heal (code admits the fixture gap, `src/index/store.rs:1818-1821`),
E-4 non-TTY stdin, E-5 mutating-command stdout split, E-6 delete/rename
path-set branch, E-7 global config, E-8 config error paths, E-9 (optional),
I-1 parity differential, I-4 cross-render freshness (regression *named in code*,
`src/template/engine/query.rs:97-100`), I-5 template-output re-enters index.

**Potential product defects (new, escalated to T3b):** config-change staleness
(`RefreshPlan::collect` fingerprints file metadata only,
`src/index/refresh.rs:277-295`; no config hash), `refresh()` fail-open not
inducible from the public API → in-crate unit, EPIPE/exit-101 contract →
open question first.

**Rejected after review:** template write-policy matrix as a separate ticket
(folded into E-1); `QuerySet::table/list` direct tests (pub(crate) seam);
"same-size/same-mtime index blind spot" (implementation timestamps are checked
in-process, no external evidence); `tmpl`/`completion` alias tests (meta-arg
layer, covered by clap contract); multi-`--where` as a gap (covered by parity
differential selector set); a plain `list("inlinks")` integration test (dup of
`src/cli/mod.rs:1342`).

---

## 16c. Revision 4 adjudication — third external critique

A third external critique (11 findings + 11 suggestions) was checked by four
review agents against the code. Verdicts:

**Adopted:**

| # | Critique point | Verdict | Evidence / disposition |
|---|---|---|---|
| P0 | Don't pin exit 101 | **CORRECT** | `main.rs:19-35` maps 0/130/1 only; no `ExitCode::from(101)` anywhere; release `panic = "abort"` → 134 (`Cargo.toml:289`). Audit was internally inconsistent (§6 said 0/1/2/130; §10 E-3/§14.6/T3.1/§16b#5 said "pin 101"). Fixed in all four sites: pin **0/1/clap-2/130** (130 reachable via `src/dialog/error.rs:93`), assert never-101 as a panic sentinel |
| P0 | E2E tree missing `index`/`config` homes | **CORRECT** | `traces index` first-class (`src/cli/mod.rs:156-158`); `dispatch.rs:17,:38`, E-2, T3.5, T3.2/T3.12 had no named file. §12 tree now lists both with explicit ticket mappings |
| P1 | `task_classification.rs` pre-committed | **CORRECT** | Reserved unconditionally; B rows 23–25 (all asserts unit-twin'd); TOML gap → E2E (T3.2); D3/F-row-14 survivor is `index_query`. §12 clause now conditional, default = fold; T7.4 settles it |
| P1 | Recompute deletions after T3 | **PARTIAL** | New T4.0 recompute ticket (re-evaluate `index_persistence_roundtrip.rs:309`, `dispatch.rs:17/:38`, D2-keep, `task_tag_filters.rs`). Critique's "only one acknowledged dependency" was wrong — T4 already had three gates (4.2, 4.7, phase gate) |
| P1 | Malformed-config arrangement | **PARTIAL — mechanism CORRECT** | Trust-before-parse confirmed (`src/config/service.rs:238-256`, typestate `file.rs:290-297`); trust hashes without parsing (`:322-336`); E-8's naive arrangement would fail with `config_build_untrusted`. §10 E-8 now carries an 8-stage arrangement table; §14.7 splits by stage; `ConfigFixture` gets raw-TOML writing (T5.3). Critique's "one generic scenario" was overstated — §14.7 already named three codes |
| P1 | `cold/warm` parity naming | **PARTIAL — rename adopted** | `sync_and_run` = `refresh_store` + `run_from_store` (`src/query/service.rs:178-185`) confirmed → renamed `store_backed_query_parity`; **more important**: persist-first arrangement now required (else the test queries rows it just wrote), and §10 I-1's "absorbs G-B1" narrowed to the store legs (`load`/expander → P1.7a/P1.9) |
| P1 | Sharper staleness repro | **PARTIAL** | All code claims verified (`is_fresh()` = `delta.is_empty()`; `class_field` only on Stale arm); repro traced, deterministically failing. Adopted as early ticket T3b.1 with the **mandatory re-trust step** and the false-negative (class-field) vs false-positive (tag/task) symptom split. Critique missed both |
| P2 | Test-count delta not a gate | **CORRECT** | T4 gate literally said "expect −25…−40, +12–15" → replaced with behavior→layer→defect-class→surviving-test ledger |
| P2 | Mutation underused | **PARTIAL** | Facts (mutarust, `src/cli` excluded, advisory, no CI) already in §4/F8. New: targeted post-redesign `test:mutants -m` runs (F8, T6.8) — with the caveat that mutarust never mutates `src/cli`, so CLI-covered seams need process-level fault injection |
| S9 | `required-features` vs `compile_error!` | **PARTIAL** | False dichotomy (audit listed both). Mechanic: `required-features` hard-errors on explicit `cargo test --test integration`, still silent on bare `cargo test`. Adopted as: `[[test]] required-features` (repo precedent — all 12 benches) + CI non-empty assertion; `compile_error!` rejected (breaks featureless builds) |
| S10 | `Run` API + env scrub | **PARTIAL** | `Run` has only `is_success()`; env inherited. Folded into P1.6 (`code`/`is_failure`/stdout-stderr predicates) and P1.13 (`env_clear` scrub-by-default, config tests opt in) — mechanism change to already-ticketed items |

**Rejected:**

- **"No observable cross-process divergence exists."** False — three recipes:
  config-change staleness (the critique's own P1 repro, doubling as the
  index-read proof), same-size+restored-mtime edit (`src/index/delta.rs:40-43`;
  in-code proof `src/index/service.rs:1360-1388` needs a 15 ms sleep to make
  mtime advance), and `chmod 000` source (`list` exits 0 from Fresh cached
  rows vs `index` exit 1 on `IndexError::NoteParse`).
- **The critique's own persistence-fallback** (cover edits/deletes/renames/
  corruption instead): those rebuild byte-equivalently — its alternative is
  weaker than the ticket it attacks. §14.10 now requires a divergence
  precondition instead.
- **"Config errors are one generic scenario."** §14.7 already separated three
  codes pre-critique; the genuine gap was the arrangements + two stages
  (field-key, merge) — both folded.
- **Note:** the critique re-litigated the same-size/mtime blind spot that
  §16b rejected for "no external evidence" — the R4 lead-verified recipe
  supplies that evidence, so §16b's rejection stands superseded by §14.10's
  divergence precondition, not reversed.

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
