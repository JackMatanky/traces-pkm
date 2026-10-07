# Audit E — Harness, Fixtures & Isolation

Read-only static inspection (Read/Grep/Glob only; no cargo/nextest/mise runs).
Scope: `src/lib.rs::test_support`, `tests/e2e/support.rs`, `src/cli/cwd.rs`,
`tests/integration/*.rs` helpers, environment isolation, integration↔e2e fixture
sharing.

Evidence convention: `file:line` = observed code. "claims" = doc/comment text,
verified against the code separately.

---

## 1. Symbol table — `src/lib.rs::test_support` (mod at lib.rs:181-969)

Visibility legend: **cfg(test,test-utils)** = `#[cfg(any(test, feature = "test-utils"))] pub`
(re-export lib.rs:156-162); **cfg(test)** = `#[cfg(test)]` only (lib.rs:154-155).
"Arr" = pure arrangement (dirs/files/in-memory values). "Beh" = performs product
behavior (trust write, index build/persist, config construction that bypasses or
drives product rules).

| Symbol | Location | Visibility | Arr / Beh | Users | Recommendation |
|---|---|---|---|---|---|
| `DEFAULT_TEMPLATES_DIR` | lib.rs:202 | cfg(test,test-utils) | Arr (const) | `TestProject::write_template`:406, free `write_template`:650 | **Remain**; export unconditionally (plain `&str`) so e2e stops hardcoding `templates` / `.traces/schemas` (support.rs:141,162) |
| `DEFAULT_SCHEMAS_DIR` | lib.rs:205 | cfg(test,test-utils) | Arr | `write_schema`:425,675 | Remain (as above) |
| `DEFAULT_SCHEMA_VALUES_DIR` | lib.rs:208 | cfg(test,test-utils) | Arr | `write_schema_value`:443 | Remain |
| `TestProject` (struct) | lib.rs:223-227 | cfg(test,test-utils) | Arr (root + `ConfigService`) | integration: `index_query`(11×), `template_render`(5×), `task_tag_filters`(3×), `index_persistence_roundtrip`(4×), `schema_field_resolution`(`empty` 5×), `config_lifecycle`:44; in-crate cli tests (`index.rs:94…`, `list.rs:515`, `table.rs:534`, `task.rs:819`, `template.rs:289`, `completions.rs:212`, `trust.rs:308`, `untrust.rs:135`, `cli/mod.rs:581…`); `template/engine/query.rs:1664`; `benches/template_render.rs:68` | **Remain** — the one good integration-level fixture |
| `TestProject::empty` | lib.rs:244-252 | cfg(test,test-utils) | Arr (mkdir only) | `schema_field_resolution.rs` 5×; `cli/trust.rs:308,330,370,391,427`; `cli/untrust.rs:135,157`; `cli/mod.rs:1540`; lib tests:889,929 | Remain |
| `TestProject::untrusted` | lib.rs:261-279 | cfg(test,test-utils) | Arr (writes hardcoded `[templates] directory = "templates"` TOML) | `cli/index.rs:143`, `cli/table.rs:550`, `cli/task.rs:830`, `cli/template.rs:369`, `cli/completions.rs:225`, lib test:920 | **Split**: body duplicates `create_trusted_project`'s write-config half; extract `write_minimal_config(root)` and have both constructors call it |
| `TestProject::trusted` | lib.rs:289-297 | cfg(test,test-utils) | **Arr+Beh**: calls `create_trusted_project` → `service.trust(...)` (lib.rs:608-610), panics on trust failure (lib.rs:610 `.expect`) | ~24 integration sites + ~20 in-crate cli sites + `benches/template_render.rs:68` | **Split** (config-write vs `.trust()`); see §6 coupling C1 |
| `TestProject::root/service` | lib.rs:302-311 | cfg(test,test-utils) | Arr (accessors) | everywhere | Remain |
| `TestProject::config` | lib.rs:317-324 | cfg(test,test-utils) | **Beh-bypass**: `Config::test_default` (model.rs:189-201) skips discovery *and* trust entirely; auto-enables templates iff `templates/` exists (lib.rs:319) | `task_tag_filters.rs:35,156`, `template_render.rs:29`, all `project.indexer()` callers (indexer:452) | **Rename** → `config_without_discovery()`/`local_test_config()`; doc must say trust is not consulted (currently silent) |
| `TestProject::trust` | lib.rs:334-341 | cfg(test,test-utils) | Beh (writes isolated trust store) | `cli/mod.rs:1551`, lib test:921 | Remain (explicit Act, name says so) |
| `TestProject::untrust` | lib.rs:351-355 | cfg(test,test-utils) | Beh | `config_lifecycle.rs:47`, lib test:922 | Remain |
| `write_file/write_note/write_template/write_schema/write_schema_value` | lib.rs:364-446 | cfg(test,test-utils) | Arr (safe-path writes) | all integration tests; in-crate tests | Remain; **dedupe** with free fns (below) — `write_note` is already a delegation (390) but `write_template`/`write_schema` re-implement 643-682 |
| `TestProject::indexer` | lib.rs:451-453 | cfg(test,test-utils) | Arr (constructs `IndexerService` from bypassed config) | `task_tag_filters.rs:93`, `index_persistence_roundtrip.rs:64,209` | Remain |
| `TestProject::build_index` | lib.rs:462-464 | cfg(test,test-utils) | **Beh** (runs product `indexer.build()`) but placed as Act by callers | `index_query.rs:22,50,79…`, `task_tag_filters.rs:93`, lib tests:962 | Remain (name states the Act); keep in Act position, not buried in arrangement |
| `TestProject::build_and_persist` | lib.rs:473-478 | cfg(test,test-utils) | **Beh** (build + `persist`) used as the test's Act | `index_persistence_roundtrip.rs:30,63,177,315`, lib test:904 | Remain, but see §4 — Act hidden behind fixture method name |
| `parse_note` | lib.rs:492-494 | cfg(test,test-utils) | Arr (pure; `MarkdownParserInput::for_test` input.rs:57-64, no I/O) | unit tests across `note`/`query`, doctests:485 | Remain — good abstraction |
| `parse_note_str` | lib.rs:507-509 | cfg(test,test-utils) | Arr (pure) | unit tests | Remain |
| `build_test_index` | lib.rs:523-525 | cfg(test,test-utils) | Arr (pure, zero disk — `WorkspaceIndex::for_test` entry.rs:53-76 only parses strings) | `query/mod.rs:115`, `query/service.rs:621`, `query/sort.rs:503`, `query/grammar/filter.rs:493`, lib test:950 | Remain — good abstraction (doc claim "zero disk I/O" lib.rs:512 **verified true**) |
| `parse_tag` | lib.rs:543-545 | cfg(test,test-utils) | Arr (pure) | fixture data in unit tests | Remain |
| `resolve_safe_path` | lib.rs:557-571 | cfg(test,test-utils) | Arr (pure assert + join); self-tested lib.rs:842-866 | `TestProject::write_file`:369, free writes:627,648,673 | Remain; **Sandbox should adopt it** (see §2) |
| `fixture_service` | lib.rs:577-579 | cfg(test,test-utils) | Arr (`ConfigService::at` explicit roots, service.rs:174-179 — never OS dirs) | `TestProject::project_service`:232, `config_lifecycle.rs:27`, `cli/trust.rs`, `cli/tracked.rs`, `cli/completions.rs`, lib test:875 | Remain — key isolation primitive |
| `create_trusted_project` | lib.rs:592-612 | cfg(test,test-utils) | **Arr+Beh combined** (mkdir + config write + `service.trust`); panics on any step | `TestProject::trusted`:292, `config_lifecycle.rs:32` | **Split** into `write_minimal_config` + trust step; **fix doc**: doc (581-583) claims it points at `root/templates` *"creating that directory"* — code (596-611) never creates `templates/`. `Sandbox::write_config` *does* create it (support.rs:141) → fixture divergence (§6 C4) |
| free `write_note` | lib.rs:622-633 | cfg(test,test-utils) | Arr | `template/engine/query.rs:706` + many | Remain |
| free `write_template` | lib.rs:643-657 | cfg(test,test-utils) | Arr | unit tests | Remain (dedupe with method) |
| free `write_schema` | lib.rs:667-682 | cfg(test,test-utils) | Arr | `template/engine/schema.rs` ~40 call sites, `query.rs:1826…` | Remain |
| `TzGuard` + `TZ_LOCK` + `set_var/remove_var` | lib.rs:686,721-724,739-763,785-806 | **cfg(test)** only (`pub(crate)`, lib.rs:154-155) | Beh (process env mutation), correctly serialized by `TZ_LOCK` held to thread end (guard stored in thread_local lib.rs:757-762) | 64 call sites: `date.rs` (30), `template/engine/date.rs` (21), `query/grammar/filter.rs:770,817,812…`, `note/field.rs:994`, lib tests | Remain. **Gap**: not available under `feature="test-utils"` → integration/e2e cannot pin TZ at all |
| `tz_guard_tests` (lib.rs:808-833), `tests::{path_safety,service,project,memory}` (835-968) | lib.rs | cfg(test) | Self-tests of harness (+1 product test: `scan_skips_traces_dir`:958) | — | Keep self-tests (fixture has no other safety net); move `scan_skips_traces_dir` out of the harness module (it's an index behavior test) |

## 2. Symbol table — `tests/e2e/support.rs`

| Symbol | Location | Arr / Beh | Users | Recommendation |
|---|---|---|---|---|
| `TRACES_BIN` | support.rs:60 | Arr (`env!("CARGO_BIN_EXE_traces-pkm")`, compile-time) | `Sandbox::command`:188 | Remain |
| `Run` / `is_success` | support.rs:64-74 | Arr (captured output) | every e2e test | Remain |
| `plain` | support.rs:83-90 | Arr (pure string transform) | `dispatch.rs:92,117,230,456` | Remain — correctly documented as wrap-fragile-limited (support.rs:17-31) |
| `Sandbox` | support.rs:97-101 | Arr (3 TempDirs) | all e2e except in-process parts | Remain |
| `Sandbox::new/from_dirs/root` | support.rs:110-134 | Arr | `dispatch` (new:57), `golden_path:49` (from_dirs) | Remain |
| `write_config` | support.rs:138-148 | Arr (creates `.traces/` + `templates/` + TOML literal) | `trusted`:217, `dispatch.rs:58` | Remain; TOML literal duplicated at lib.rs:271,602 → single source |
| `write_note/write_schema/write_schema_value` | support.rs:152-174 | Arr — **but raw `root.join(rel)` with no path guard** (contrast `resolve_safe_path`) | all e2e | **Adopt `resolve_safe_path`** (needs test-utils feature or a local copy) |
| `write_template` | support.rs:177-180 | Arr — no `create_dir_all`; silently depends on `write_config` having created `templates/` | `dispatch.rs:482,511`, `template_write.rs:18,51` | Add `create_dir_all` (remove hidden order coupling) |
| `Sandbox::command` | support.rs:187-194 | Arr (child env `TRACES_STATE_DIR`+`XDG_CONFIG_HOME`, child `current_dir`) — parent never mutated (**verified**: no `env::set_var` anywhere in `tests/`) | all spawned tests | Remain; gaps: no `TZ` pin, parent's `TRACES_IGNORED_DIRS`/`TRACES_CEILING_DIRS` (env_vars.rs:5,23) and `HOME` inherited by children |
| `Sandbox::run` | support.rs:197-208 | Arr (spawn + capture) | all spawned tests | Remain |
| `Sandbox::trusted` | support.rs:215-225 | **Beh hidden in fixture**: runs product `traces trust` and asserts success | ~25 sites: `dispatch.rs` (20), `tracked.rs:20,41`, `untrust.rs:15`, `template_write.rs:17,32` | Keep, but **rename** (e.g. `trusted_via_cli()`) or keep name + it already documents the side effect (210-214); it is a 25-test coupling to `traces trust` exit status (§6 C3) |
| `CwdGuard` / `enter` / `Drop` | support.rs:234-256 | **Beh (process-global cwd mutation), NO lock** | `init.rs:29,42,55`, `golden_path.rs:52` | **Fix** — see §5 |

## 3. `src/cli/cwd.rs` — in-crate guard

- `CWD_TEST_LOCK` (cwd.rs:56) is `#[cfg(test)] static Mutex<()>`; `CwdGuard::enter` (80-90)
  acquires it *before* `set_current_dir`; `same_dir` (98-107) is the read-only variant;
  `Drop` (111-116) restores while the `_lock` field is still alive (field dropped after
  `drop()` body) → lock covers the restore too. Poison handled via `into_inner` (82,100).
- **Writer completeness: yes.** Grep of `set_current_dir` across the repo finds exactly 4
  sites: cwd.rs:85/114 (both inside the lock-holding guard) and support.rs:245/254 (no lock).
- **Reader completeness: spot-verified.** Production cwd reads go through `Cwd::new`
  (cwd.rs:32) called from `cli::current_dir` (mod.rs:226) → `load_config` (mod.rs:239),
  `resolve_trust_subjects` (mod.rs:499), `Init::run` (init.rs:43). Every test that reaches
  those paths examined holds a guard: `init.rs:186`, `mod.rs:1511` (explicit), `mod.rs:1525`,
  `trust.rs:310`, `untrust.rs:160`, plus `locked_cwd()` (cwd.rs:125-130) for cwd.rs's own
  read tests, and `same_dir()` used by `index/entry.rs:358`.
- **Not reachable outside the unit-test binary**: `#[cfg(test)]` means the lock is not even
  compiled when the lib is built as a dependency of `tests/e2e.rs`/`tests/integration.rs`.
  support.rs:38-39 states this correctly.
- **Not enforced by lint**: `clippy.toml:91` disallows `std::env::current_dir` (hence the
  `#[expect]` at support.rs:239-242) but **does not disallow `std::env::set_current_dir`** —
  a future writer anywhere in `src/` would not be flagged.

## 4. Integration local helpers (all file-private; no shared `tests/common`)

| Helper | Location | Nature | Note |
|---|---|---|---|
| `line_source` | index_persistence_roundtrip.rs:258-260 | Arr (constructs `SourceLine`) | trivial; fine |
| `assert_rows_match` | index_persistence_roundtrip.rs:264-298 | Assertion (structural + task fields) | good: reusable comparison, no hidden product call |
| `assert_query_task_statuses` | task_tag_filters.rs:216-244 | Assertion | fine |
| `assert_resolved_parent` / `assert_incomplete_parent` | task_tag_filters.rs:248,274 | Assertion | fine |
| `assert_config` / `table_str` | e2e/init.rs:65-84 | Assertion over TOML | fine |

All are pure assertions/arrangement — none hide an Act. The Act step is instead hidden in
fixture constructors (§6).

## 5. Isolation & parallel-safety analysis

### Process-global mutation inventory

| Mutation | Site | Serialized? | Reach |
|---|---|---|---|
| **Parent cwd (e2e binary)** | `CwdGuard::enter` support.rs:245, `Drop`:254; sites init.rs:29/42/55, golden_path.rs:52 | **NO lock** (support.rs:47: *"there is no lock"*) | `init` + `golden_path` are in the **same binary** (`tests/e2e.rs:8-11`) |
| **Parent cwd (unit-test binary)** | cwd.rs:85/114 | `CWD_TEST_LOCK` (cwd.rs:56) | lib `#[cfg(test)]` only |
| **`TZ` env (unit-test binary)** | lib.rs:746,755,770 (set), 789/805 (unsafe set/remove) | `TZ_LOCK` (lib.rs:686) held to thread end | lib `#[cfg(test)]` only; tests *not* taking the guard can read a swapped `TZ` (acknowledged lib.rs:717-719) |
| Child process env | support.rs:191-192 (`cmd.env`) | n/a (per-child) | safe; parent untouched (verified: zero `env::set_var` in `tests/`, only lib.rs:789/805 in src) |
| `dirs::*` LazyLock statics | dirs.rs:91,102,161,176,188 | first-use per process | each test binary resolves under its own env |
| Shared `<manifest>/test` HOME | dirs.rs:80-82 (`#[cfg(test)] HOME`) | shared path across unit tests | only a `Debug`-compare test constructs OS-backed stores (`service.rs:655-663`); **no test writes there today** |
| TempDirs | per-test `tempfile` everywhere | n/a | fine, except a deleted TempDir can sit under another test's cwd (cwd.rs:52-54 acknowledges) |

### Is E2E parallel-safe?

**Under the project's canonical runner — `cargo nextest` (`.config/nextest.toml`,
`.mise/tasks/test/unit` runs `cargo nextest run -p traces-pkm --all-features`) — yes:**
nextest is process-per-test, so `init` and `golden_path` get separate processes and the
parent-cwd race cannot occur; cross-binary hazards are also eliminated (each test = own
process).

**Under libtest (`cargo test`, thread-per-test) — no:**
- `init_scaffolds_…` (init.rs:26) and `init_trust_index_…` (golden_path.rs:45) run
  concurrently in one process (default `test-threads` = CPU count). Concrete failure modes:
  1. thread B's `Init.run` (init.rs:34) reads cwd (init.rs:43) that thread A just redirected
     → scaffolds into A's dir → `assert_config(preset.path())` (init.rs:37) fails;
  2. B captures A's tempdir as `original` (support.rs:244), A finishes and its `TempDir`
     drops, B's `Drop` runs `set_current_dir(&self.original).expect` (support.rs:254) on a
     deleted dir → **panic in the innocent test**;
  3. both tests' non-guarded sections (assertions at init.rs:37-49,61 run *outside* the
     guard) observe whatever cwd the other thread set.
- `mise watch` runs `cargo watch -x check -x test` (mise.toml:140) — **bare `cargo test`,
  no nextest** → the race is live in that workflow.

**Cross-binary:** cargo executes test binaries serially within one invocation, and no
`tests/integration/*.rs` or unit test mutates cwd outside its own guard, so there is no
cross-binary cwd hazard *today* — but there is also **no mechanism that could prevent one**
(both locks are per-process). `std::env::set_current_dir` is not in `clippy.toml`'s
disallowed list (clippy.toml:91), so nothing stops a future integration test from mutating
cwd unguarded.

**Module-doc verification (support.rs:33-49 vs code):**
- Claim "see `src/cwd.rs`" (line 37) → actual path is **`src/cli/cwd.rs`** (wrong).
- Claim the in-crate lock is unreachable from this binary (38-39) → **true** (cfg(test), private).
- Claim "within this one binary `init` and `golden_path` are the only two cwd-mutating tests
  and … default per-binary test parallelism still runs them concurrently … there is no lock …
  a known, accepted limitation" (41-49) → **true and matches code** (only 4 `CwdGuard::enter`
  sites exist, all listed above; support.rs:243-249 has no synchronization).
- Sentence at 41-45 is **garbled/self-contradicting** ("runs each integration test *binary*
  single-threaded … is false in general, but …").
- **Internal contradiction:** support.rs:7-9 claims the two env vars make "every test …
  parallel-safe with no shared mutable state" — false in the same file: cwd *is* shared
  mutable state (33-49) and `TZ`/`HOME`/`TRACES_*_DIRS` are inherited, not scrubbed.

### Environment isolation summary
- **Good:** `TRACES_STATE_DIR` + `XDG_CONFIG_HOME` set per child (support.rs:191-192);
  `ConfigService::at`/`fixture_service` give integration tests explicit store roots
  (service.rs:174-179); `#[cfg(test)] HOME` pins the unit binary to a nonexistent
  `<manifest>/test` (dirs.rs:80-82); zero `env::set_var` outside `TzGuard`.
- **Gaps:** (a) no TZ pinning for spawned children and no `TzGuard` under `test-utils` —
  today no test in `tests/` asserts zone-dependent output (grep: no `now`/`Local`/date-value
  asserts), so this is latent; (b) `impl Default for ConfigService` (service.rs:578-583) is
  public under `test-utils` and returns **OS-backed** stores (`ConfigService::new`:159-163) —
  an external test calling `default()` writes the developer's real state dir; (c) parent's
  `TRACES_IGNORED_DIRS`/`TRACES_CEILING_DIRS`/`TZ`/`HOME` leak into children;
  (d) `tests/integration.rs:5` `#![cfg(feature = "test-utils")]` + **no `[[test]]`
  `required-features`** in Cargo.toml → bare `cargo test` (incl. `mise watch`) compiles an
  **empty** integration binary and reports success with 0 tests (the default `mise test`
  task saves this by passing `--all-features`).

## 6. Hidden couplings (fixture constructor changes that would silently weaken/break many tests)

- **C1 — `TestProject::trusted` couples *every* integration test to trust semantics.**
  It panics if `service.trust` fails (lib.rs:608-610), and 24 integration sites call it —
  yet nearly all of them then use `project.config()`/`project.indexer()` which go through
  `Config::test_default` (model.rs:189-201) and **never consult the trust store**. So:
  (a) a trust-semantics change breaks unrelated index/query tests (false failures);
  (b) conversely, weakening `trusted()` would *not* be caught by those tests — trust is
  decorative there. The tests that genuinely read trust are the in-crate CLI tests that pass
  `project.service()` into `run(...)` (`cli/index.rs:98`, `cli/table.rs:544`,
  `cli/task.rs:824`, `cli/mod.rs:581`, …).
- **C2 — `TestProject::config()` silently flips template config when a test writes a
  template** (lib.rs:319 `is_dir()` check). Order of `write_template` vs `config()` changes
  the config under test with no explicit signal (works today only because
  `template_render.rs:25` writes before `:29` reads).
- **C3 — `Sandbox::trusted()` binds ~25 e2e tests to `traces trust`'s exit status/stderr**
  (support.rs:218-223). Failures are at least loudly labeled `"fixture setup: …"` — the one
  fixture that does this right; `TestProject::trusted`'s panic message ("trust project
  config", lib.rs:610) is not labeled as fixture setup.
- **C4 — fixture divergence:** `Sandbox::write_config` creates `templates/`
  (support.rs:141); `create_trusted_project` does **not**, despite its doc claiming it does
  (lib.rs:583 vs 596-611). CLI-level tests therefore start with a templates dir, integration
  tests without one, unless `write_template` runs first.
- **C5 — hardcoded config TOML in three places** (lib.rs:271, lib.rs:602,
  support.rs:145): a config-format change must be made in all three or one fixture tier
  silently diverges.
- **C6 — `Sandbox::write_template` requires `write_config` to have run** (no
  `create_dir_all`, support.rs:178) — hidden ordering requirement.

## 7. Arrangement vs behavior: which helpers hide the Act?

Worth **keeping as-is** (pure arrangement or explicitly-Act-named):
`parse_note`/`parse_note_str`/`parse_tag`/`build_test_index` (pure, zero I/O — claim at
lib.rs:512 verified against entry.rs:53-76), `resolve_safe_path`, `fixture_service`,
`DEFAULT_*`, `Sandbox::command`/`run`/`Run`/`plain`, `TestProject::{empty,write_*,indexer}`,
in-crate `CwdGuard`, `TzGuard`.

**Hidden Act (setup performs product behavior) — split or rename:**
1. `create_trusted_project` (lib.rs:592): one function = mkdir + config write + trust write.
   Split → `write_minimal_config(root)` (arr) + `service.trust(...)` (act). `TestProject`
   then derives `trusted`/`untrusted` from the same primitive instead of duplicating the
   write (lib.rs:261-279 vs 596-604).
2. `Sandbox::trusted` (support.rs:215): fixture runs the CLI under test. Acceptable, but
   rename to make the Act visible (`trusted_via_cli`) — dispatch.rs's explicit
   `run(&["trust"])` calls (golden_path:60) are the model for trust-under-test.
3. `TestProject::build_index` / `build_and_persist` (lib.rs:462/473): product index build
   + redb persist behind a fixture method. Names do state the Act, so low risk; recommend
   documenting them as Act helpers so they stay in Act position (they currently are).
4. `TestProject::config` is the most dangerous: it *looks* like an accessor but selects
   bypass semantics (no discovery, no trust) and mutates behavior on directory existence.
   Rename + doc.

**Lower-level / eliminate:**
- Free `write_note`/`write_template`/`write_schema` vs `TestProject::{write_note,
  write_template, write_schema}` — two parallel APIs implementing the same thing
  (lib.rs:385-446 vs 622-682). Keep one (methods delegate to free fns).
- `config_lifecycle.rs:42` `test_project_manages_trust_and_untrust_lifecycle` tests the
  **fixture** (`TestProject::untrust`) and duplicates `src/lib.rs:917`
  `manages_trust_and_untrust_lifecycle` — eliminate one (the integration value is only in
  the *public re-export* argument; keep the re-export check via `create_trusted_project`
  test at config_lifecycle.rs:24, drop the fixture-duplication test).

## 8. Do integration and E2E share fixture primitives?

**No — zero shared code.** Integration uses in-process `TestProject` (test-utils surface,
`tests/integration.rs:5`), e2e uses `Sandbox` (process spawn, no test-utils). They
independently re-implement: config TOML literal (lib.rs:271/602 vs support.rs:145),
`write_note`/`write_schema`/`write_schema_value`/`write_template` (lib.rs:385-446 vs
support.rs:152-180), schema/template dir constants (lib.rs:202-208 vs literals at
support.rs:141,162,170), trusted-fixture construction (lib.rs:289 vs support.rs:215).

**Should they?** Share only *pure arrangement* primitives — path constants, the config TOML
template, and safe-path joining (e2e currently lacks path guarding entirely). Do **not**
share the behavior constructors: `TestProject::trusted` writes the trust store in-process
while `Sandbox::trusted` must run the real binary — that divergence is the point of the e2e
tier. Concretely: export `DEFAULT_*` unconditionally (they're `&str`), put the config TOML
in one const, and give `Sandbox` a `resolve_safe_path`-style guard.

## 9. Prioritized recommendations

1. **Fix/serialize the e2e cwd race**: add a process-local `Mutex` to `tests/e2e/support.rs`
   mirroring `cwd.rs` (cheap; makes `cargo test` and `mise watch` safe), *and/or* restrict
   `tests/e2e` to nextest + document it; update support.rs:7-9 to stop claiming "no shared
   mutable state" and fix the `src/cwd.rs` → `src/cli/cwd.rs` reference (line 37) and the
   garbled sentence (41-45).
2. **Add `std::env::set_current_dir` to `clippy.toml` disallowed-methods** (only the *read*
   is disallowed today, clippy.toml:91) so new cwd writers must take the lock explicitly.
3. **Split `create_trusted_project`** into arrangement (config write) and behavior (trust);
   dedupe `TestProject::untrusted`; fix its "creating that directory" doc claim (lib.rs:583)
   or actually create `templates/` for Sandbox parity.
4. **De-couple integration fixtures from trust**: use `TestProject::empty()` + explicit
   config write (or a new `with_config()`) in tests that never load via `service()`
   (`index_query`, `task_tag_filters`, `template_render`, `index_persistence_roundtrip`,
   `schema_field_resolution`) → trust changes fail only trust tests.
5. **Rename `TestProject::config`** to advertise the discovery/trust bypass; doc the
   templates-dir auto-detection (lib.rs:317-324).
6. **Add `[[test]] name = "integration" required-features = ["test-utils"]`** (or
   `compile_error!` in `tests/integration.rs`) so bare `cargo test`/`mise watch` cannot
   silently run zero integration tests.
7. **Pin `TZ=UTC` in `Sandbox::command`** (support.rs:191) and/or feature-gate `TzGuard`
   for `test-utils`, so future date-rendering e2e assertions are host-independent.
8. **Gate or remove public `impl Default for ConfigService`** under `test-utils`
   (service.rs:578) — it silently points at the real OS state dirs.
9. **Harden e2e writes** with `resolve_safe_path`, and make `Sandbox::write_template`
   create its parent dir (support.rs:178).
10. **Deduplicate harness self-tests**: drop `config_lifecycle.rs:42` (duplicate of
    lib.rs:917); move `scan_skips_traces_dir` (lib.rs:958) out of the harness's own
    `tests::memory` module into the index tests where the behavior lives.
