# Spec: Test Architecture Consolidation (`integration-e2e-suite`)

Status: ready-for-agent

## Parent

Evidence base: `.scratch/integration-e2e-suite/test-arch-audit.md` (Revisions 1–4, adjudicated) and
`.scratch/integration-e2e-suite/research/{A..H}-*.md`. The audit is the citation source; this spec
carries the decisions, not the evidence trail.

## Problem Statement

As a maintainer of `traces-pkm`, I cannot trust what the test suite claims to prove. The suite is
green (3,127 nextest tests + 76 doctests in ~11 s), but several tests' doc comments assert
boundaries they never execute: a test claiming to confirm an edit reached the persisted store
passes even if the command under test did nothing; E2E tests claiming cross-process persistence
only check that a file exists, with no second product process ever reading it; a "golden path"
test claims eight spawns and proves less than it says. Separately, plain `cargo test` runs zero
integration tests and exits 0 — silently green — while the documented `--feature default` command
hard-errors. Real contracts sit untested: no test anywhere asserts a numeric exit code, `traces
init` is never executed through dispatch or spawned as a process, config TOML has no behavioral
coverage from the test suite, and the E2E harness inherits ambient environment variables (and on
Windows, the runner's real config home), so hermeticity is assumed rather than enforced. At the
same time roughly 2% of the suite is provably redundant, fixtures silently perform the behavior
under test, and no mechanical check enforces the layer discipline the architecture documents
promise.

## Solution

From the maintainer's perspective: a test architecture whose every claim matches what it executes.

- **Honesty first:** assertions verify their own doc claims; every doc comment describes only
  what the test actually runs; silent green is bounded precisely — explicit
  `cargo test --test integration` without the test feature **hard-errors**, bare `cargo test`
  still **skips** that target (documented Cargo behavior), so CI must assert **per-target
  non-empty** results (lib, integration, e2e) plus an exact doctest count.
- **A hard E2E invariant:** every test under the E2E suite exercises product behavior only
  through the spawned binary, in an environment-scrubbed sandbox; a mechanical check in lint/CI
  keeps it that way.
- **Gap tests at the right seam:** process-boundary contracts (exit codes 0/1/2 and never 101,
  with 130 conditional on a portable interrupt mechanism; argv/usage, stream split, overwrite
  refusal, corruption self-heal, non-TTY determinism, cross-process state divergence), config-TOML-to-behavior and config failure stages through
  spawned processes, `@class` source expansion through the template composition root, refresh and
  path-set visibility, store-backed query parity, and early reproduction of the config-change
  staleness question.
- **Consolidation behind evidence:** deletions of duplicated tests only after the new tests land,
  gated by a recomputed duplication analysis and a behavior-to-surviving-test ledger — not by
  counting tests.
- **A clear facade and fixture vocabulary:** one `testing` facade module for the test-only
  surface **whose membership is frozen at today's export set** (a path relocation, never an
  opportunity to add), shared pure arrangement with each copy owned at the layer that can
  actually reach it, fixtures that declare their scenario instead of inferring it from directory
  existence.
- **A visibility freeze (see its own section):** no ticket in this spec adds a single `pub`,
  method, or re-export — not even cfg-gated — verified mechanically against a baseline.
- **Feature-separated runner phases** (unit / integration / E2E-on-shipped-config / doctests)
  aggregated by the canonical test task, with CI covering them.
- **Pruning-first naming:** capability-based file trees instantiated after deletions, test names
  aligned with glossary vocabulary.

## User Stories

1. As a maintainer, I want a test's doc comment to describe only what the test executes, so that
   I can read a test name and trust the claim.
2. As a maintainer, I want the store-edit assertion test to verify through a non-refreshing load,
   so that a no-op command under test fails the suite.
3. As a maintainer, I want doc comments that overstate their tests rewritten, so that the
   documentation and the assertions agree everywhere.
4. As a maintainer, I want an explicit `[[test]]` feature requirement on the integration suite, so
   that requesting it without the test feature fails loudly instead of running nothing.
5. As a maintainer, I want CI to assert each expected suite ran non-empty (lib, integration,
   e2e) with an exact doctest count, so that a target silently skipped by feature configuration
   is detected (bare `cargo test` skipping the gated target is Cargo behavior we document, not
   pretend to prevent).
6. As an agent implementing a ticket, I want every E2E test to go through the spawned binary, so
   that the behavior I'm guarding is the shipped behavior.
7. As a maintainer, I want the in-process `init` assertions relocated to an in-crate component
   test, so that E2E means what it says and the preset/custom-path dialog coverage survives.
8. As a maintainer, I want a genuinely spawned default-path `traces init` test asserting exit
   code and stderr, so that the initialization command is exercised end to end for the first time.
9. As a maintainer, I want the E2E working-directory guard deleted once no test mutates cwd in
   process, so that the harness contains no unused synchronization machinery.
10. As a maintainer, I want a cheap mechanical layer check (lint task + CI step) asserting E2E
    tests import nothing from the crate and integration tests spawn nothing, so that layer
    discipline doesn't depend on human vigilance.
11. As a maintainer, I want clippy to disallow in-test cwd mutation outright, so that the
    invariant is enforced at the compiler level.
12. As a maintainer, I want numeric exit-code assertions (0 success, 1 domain failure, 2 argv
    failure) and never-101 at the process level, with process-level 130 added **only if** a
    portable dialog-interrupt mechanism (PTY) is approved, so that the exit contract is pinned
    without a fragile signal-based approximation, and the in-crate
    `Cli::run → Aborted(Interrupted)` mapping link tested regardless (today only `Cancelled` is).
13. As a maintainer, I want representative paths asserted to never exit 101, so that a panic
    artifact can't silently become a de facto contract.
14. As a maintainer, I want a generic process-contract test file covering unknown subcommand,
    missing required flag, invalid value, `--help`, and `--version`, so that clap's
    unreachable-in-crate exit-2 path is observed where it actually runs.
15. As a maintainer, I want assertions for exact stdout/stderr content and `stdout == ""` on
    mutating commands, so that the stream-split contract is enforced, not just exit status.
16. As a maintainer, I want config TOML (task tag filters, schema class field, template
    directory) driving observable behavior through spawned processes, so that the disk form of
    configuration is covered without widening the production API.
17. As a maintainer, I want config failure coverage split by pipeline stage — with process-level
    tests only for stages that prove a distinct process-level failure possibility (discovery,
    stale trust, parse-after-trust, field-key, plus the existing never-trusted leg) and the
    remaining stages covered in-crate — so that E2E buys unique boundaries instead of
    re-rendering error variants already unit-tested.
18. As a maintainer, I want an explicit fixture that can write raw invalid TOML, so that
    malformed-config scenarios are declarable rather than hand-rolled.
19. As a maintainer, I want `@class` source expansion tested through the template composition
    root plus a paired CLI test, so that schema-driven selection is covered from both external
    entry points.
20. As a maintainer, I want the `schema_field_resolution` integration file deleted only in the
    same change that lands its replacement coverage, so that reachability testing transfers
    instead of vanishing.
21. As a maintainer, I want a query-visible refresh test asserting through the public query API
    (not store report counts), so that incremental indexing correctness is observed where users
    see it.
22. As a maintainer, I want note edits between two spawned runs asserted, so that
    edit-then-query across process boundaries is proven.
23. As a maintainer, I want deletes and renames between spawns asserted (ghost rows gone,
    links re-resolved), so that the path-set recompute branch has its own guard.
24. As a maintainer, I want cross-process persistence proven with a scenario where reload
    provably differs from rebuild, so that "the store was read" is demonstrated rather than
    inferred from a file's existence.
25. As a maintainer, I want a store-backed query parity differential with a pinned
    persist-first arrangement, so that in-memory and refreshed-store results are proven
    identical for a representative selector set, including cold-path inlinks.
26. As a maintainer, I want corruption self-heal tested (garbage written to the index file,
    next query exits 0 with correct output and a valid file again), so that index corruption
    cannot panic or wedge the CLI unnoticed.
27. As a maintainer, I want a spawned template test on null stdin exercising the real dialog
    provider deterministically (no hang), so that non-TTY behavior and CI safety are guarded.
28. As a maintainer, I want cross-render freshness and template-output-reenters-index tests, so
    that the two named-in-code regressions gain their missing guards.
29. As a maintainer, I want the sandbox environment scrubbed by default (state, config home
    including Windows, ceiling/ignore vars, pinned timezone) with tests opting in explicitly, so
    that hermeticity is enforced rather than assumed.
30. As a maintainer, I want sandbox accessors for state/config paths, so that env-isolation
    assertions become writable at all.
31. As a maintainer, I want a global-config-via-child-env test, so that the global config leg
    has process-level coverage.
32. As a maintainer, I want the config-change staleness question reproduced early with a traced,
    deterministic arrangement (including the mandatory re-trust step), so that we decide intended
    behavior before writing more config-sensitive tests.
33. As a maintainer, I want `refresh()` fail-open covered by an in-crate unit test where the
    seam exists, so that the documented fail-open behavior is guarded at its only inducible
    point.
34. As a maintainer, I want the EPIPE/exit-101 contract explicitly decided (ticket or recorded
    decision), so that it stops being an ambient unknown.
35. As a maintainer, I want duplicated tests deleted only after a recomputed duplication analysis
    of the post-new-tests inventory, so that consolidation never masks a freshly filled gap.
36. As a maintainer, I want every deletion recorded in a behavior-to-layer-to-defect-class-to-
    surviving-test ledger, so that removals are auditable without counting tests.
37. As a maintainer, I want the verified duplicate integration tests and the test-only-constructor
    schema file removed, so that each observable has one owner at the layer that uniquely proves
    it.
38. As a maintainer, I want the config trust/lifecycle integration tests eliminated with their
    unit and E2E twins confirmed, so that the budget moves to config-behavior coverage.
39. As a maintainer, I want the collapsed E2E command tests (query commands, completions,
    duplicate diagnostics) merged, so that E2E time buys unique boundaries only.
40. As a maintainer, I want the ~74 dead tempdir fixture parameters removed, so that fixtures
    stop creating directories nobody reads.
41. As a maintainer, I want the near-vacuous "tracked clean" assertion strengthened to a
    structured message check, so that the test fails on regressions instead of trivia.
42. As a maintainer, I want a single `testing` facade module as the only test-only export
    surface, so that the architectural status of every exported name is unambiguous.
43. As a contributor, I want integration tests to import from one facade namespace, so that
    renames are mechanical and the public production API stays clean.
44. As a contributor, I want each fixture literal deduplicated only where a copy is actually
    reachable from the other copies' layer (crate-side copies collapse to one internal constant;
    the E2E copy stays local since E2E imports nothing), so that sharing never becomes a
    backdoor for new exports.
45. As a contributor, I want fixtures that declare their scenario (a config fixture with tasks,
    schemas, templates sections, including raw-TOML writing) instead of inferring behavior from
    directory existence, so that test intent is readable at the call site.
46. As a contributor, I want arrange and trust split into separate fixture steps, so that a test
    that never consults trust doesn't silently depend on it.
47. As a contributor, I want test-body `.expect` on behavior-under-test converted to assertions,
    so that failures read as contract violations, not fixture panics.
48. As a maintainer, I want the default-constructed config service (which points at real OS
    state dirs and has zero callers) **removed outright**, so that no test can touch host state
    by accident — gating it behind the test feature is explicitly not acceptable, since every
    test target compiles the crate with that feature.
49. As a maintainer, I want runner phases separated by feature configuration (unit on production
    config, integration on the test feature, E2E on the shipped config, doctests separately)
    aggregated by the canonical test task, so that the shipped binary is what E2E spawns.
50. As a maintainer, I want the watch task to run the canonical suite, so that local iteration
    exercises the same tests CI does.
51. As a maintainer, I want the broken documented feature flag fixed or the promise removed, so
    that every documented command works when pasted.
52. As a maintainer, I want nextest timeouts and slow-test visibility enabled, so that a hang or
    a creeping slow test is reported instead of absorbed by defaults.
53. As a maintainer, I want CI to run the default-feature configuration and non-Linux E2E once
    the sandbox Windows fix lands, so that platform branches are tested, not just compiled.
54. As a maintainer, I want CI's test job split by suite, so that a layer failure is legible in
    the checks list.
55. As a maintainer, I want the misnamed test task and the silently-dropping module filter
    documented or fixed, so that local commands do what their names say.
56. As a maintainer, I want a coverage/mutation decision recorded (gate or advisory) plus
    targeted mutation runs against the modules the new seams protect, so that the investment is
    either enforced or explicitly accepted.
57. As a maintainer, I want capability-based E2E file names instantiated after pruning (query,
    template, trust, tracked, completions, init, index, config, plus one generic process-contract
    file), so that file layout reflects observable capabilities instead of test lineage.
58. As a maintainer, I want integration file names to name the seam whose failure needs at least
    two production components, so that a filename tells you why the layer exists.
59. As a maintainer, I want test names aligned with glossary vocabulary and free of marked-avoid
    terms, so that reading tests teaches the domain language.
60. As a maintainer, I want the golden-path test's delete-vs-rename decision deferred until its
    replacement coverage exists, so that we never trade a certified cross-process test for
    nothing.
61. As an agent implementing a ticket, I want each ticket self-contained with its evidence
    citation and defect class, so that I can work it fresh after a context clear.
62. As a maintainer, I want the audit's evidence standard applied forward to every new test
    (what it executes vs what it asserts, named defect class), so that the original defect shape
    cannot recur.
63. As a maintainer, I want a mechanically verified visibility freeze — public-api and
    `pub`-token baselines diffed on every ticket — so that test work can never quietly expand
    the crate's surface and cascade that expansion into rustdoc, API commitments, and further
    test coupling.
64. As a maintainer, I want every state-transition test to observe through a non-mutating path,
    so that no assertion can independently repair the state transition it claims to verify.
65. As a maintainer, I want the E2E environment defined as an explicit policy (sandbox-owned
    variables, a platform passthrough allowlist, opt-in overrides, and an assertion that state
    lands inside the sandbox), so that `env_clear()` is a contract rather than a blanket wipe
    that might strip runtime prerequisites.

## Implementation Decisions

**Ordering (revised — structural prerequisites land before the tests that consume them):**
honesty fixes first — everything after builds on true claims; then the **prerequisite slice of
the harness work**: the facade namespace relocation (5.1), `tests/common` wiring (5.2), and the
explicit `ConfigFixture` with raw-TOML writing (5.3), plus a standing assertion-style convention
for tests written from here on. These are independent of pruning — the facade's export set is
frozen at today's names regardless of what gets deleted — so pulling them forward removes the
churn of writing new tests against helpers scheduled for replacement. Then the E2E spawn-only
invariant (it changes where new E2E tests may live); then gap tests, **written directly into
their final capability files** (only relocation of *legacy* tests waits for pruning); then
deletions validated against the enlarged suite; then the remaining fixture/harness cleanup
(5.4–5.8); runner/CI largely parallel; naming last, because file trees are instantiated from
whatever survives pruning.

**Honesty**
- The store-edit test asserts via a non-refreshing load; its same-shape sibling is fixed or its
  doc scoped to what it runs.
- All overstated doc comments (E2E persistence, golden path, harness parallelism sentence, stale
  cross-reference, fixture doc divergence) rewritten to match executed behavior.
- Integration suite gains an explicit feature requirement in the manifest (repo precedent: benches
  already do this) — explicit selection without the feature is a hard error; `compile_error!` was
  rejected because it breaks featureless builds and IDE analysis. Bare `cargo test` still skips
  the gated target (Cargo: "the target will be skipped"), which is why CI asserts **per-target
  non-empty** counts (lib, integration, e2e) **and an exact doctest count**, so neither a skipped
  target nor doctests silently lost during the facade re-path can pass unnoticed.

**E2E invariant**
- Every E2E test executes product behavior only through the spawned binary — no in-process
  command runs, no exceptions.
- In-process-only dialog assertions (preset queue, custom path) relocate to an in-crate component
  test; a spawned default-path init test (null stdin) replaces the in-process init step; the
  golden-path first step becomes a spawn.
- The E2E cwd guard is deleted (children already set their own cwd); cwd mutation is disallowed
  by lint and greped by a layer-enforcement task in CI. This supersedes the earlier mutex
  proposal: remove the mutator rather than lock it.

**Gap tests (route decisions)**
- Exit codes pinned to 0/1/2/130 as the contract, with **process-level assertions for 0, 1,
  clap-2 and never-101 only**. Process-level 130 is **conditional**: the dialog-interrupt chain
  requires a real TTY (`TerminalDialogProvider` short-circuits to defaults on non-TTY stdin, and
  `PresetDialogProvider` has no interrupt path), so SIGINT-to-child would prove signal death, not
  the app-mapped code — it needs an approved PTY mechanism, otherwise 130 coverage stays in-crate
  plus one new unit for the untested `Cli::run → Aborted(Interrupted)` link (only `Cancelled` is
  covered today). 101 asserted only as "never" (no exit-101 path exists in the binary; release
  aborts on panic → 134). The harness run result gains a numeric code accessor, a failure
  predicate, and exact-stdout/stderr assertions — harness-side only, zero `src/` changes.
- Config-TOML-to-behavior goes through spawned processes (production composition path; visibility
  is irrelevant when spawning). **Do not widen the production API** — exporting internal config
  types for tests was explicitly rejected; the once-proposed narrow test-only adapter fallback
  is withdrawn — Visibility freeze rule 1 covers such a branch with an in-crate unit or a spawn.
- Config failures split by pipeline stage, but process-level tests cover **only stages with a
  distinct process-level failure possibility** — four new representatives: discovery (zero
  process coverage today), stale trust (the only genuinely cross-process trust state),
  parse-after-trust (pins the trust-hashes-without-parsing arrangement at process level), and
  field-key (asserted nowhere today), alongside the existing never-trusted leg. MissingBaseline
  and merge/validation stay in-crate (unit-owned); global-parse folds into the global-config
  ticket. Each process test asserts diagnostic code + exit status + arrangement — not
  re-rendered error variants. The key arrangement fact: trust verification happens before
  parsing and hashes without parsing, so a malformed config must be trusted first to reach the
  parse stage — the naive arrangement fails at trust instead.
- `@class` expansion tests through the template service composition root (the only external path
  attaching the class expander) plus a paired CLI test of `--from '@…'`; a direct query-service
  test was proven vacuous and rejected.
- Refresh family: query-visible delta at integration (observed via the non-mutating path, see
  Testing Decisions); edit-between-spawns and delete/rename path-set at E2E; cross-process
  persistence requires a **declared divergence precondition**, collapsed to **one canonical
  recipe** — same-size + restored-mtime content edit (portable) — with the `chmod` recipe as a
  documented `#[cfg(unix)]` alternative (root-sensitive, skipped when privileged). The
  config-staleness recipe is **not** an assertion anywhere (it would pin a potential product
  defect; it stays the T3b.1 investigation). Plain successive-invocation content assertions are
  forbidden from claiming "the store was read": reload and rebuild emit identical stdout/exit
  (refresh deltas go to a tracing subscriber that does not exist in the binary, `traces index`
  always rebuilds, and corruption wipe-and-recreates), so a would-be always-rebuild regression
  passes every non-divergent test. Persist→load *fidelity* stays at the integration layer,
  which already owns it.
- Store-backed query parity: same representative selector set through in-memory run vs
  sync-and-run, structurally compared, with a pinned persist-first arrangement so the from-disk
  fresh branch is guaranteed; it covers store legs only (config-load and class-expander legs
  belong to their own tickets).
- Environment policy by explicit table, not a blanket wipe (harness contract, zero `src/`
  changes):
  - **sandbox-owned** (set to sandbox paths/values): `TRACES_STATE_DIR`, `XDG_CONFIG_HOME`,
    `APPDATA`, `LOCALAPPDATA`, `HOME`, `USERPROFILE`, `TZ=UTC`;
  - **passthrough allowlist** (runtime prerequisites a wiped process still needs): `PATH`,
    `SystemRoot`, `TEMP`, `TMP` (Windows entries cfg-gated);
  - **opt-in** (tests that study them set them explicitly): `TRACES_CEILING_DIRS`,
    `TRACES_IGNORED_DIRS`;
  - **assertion:** state and config files provably land inside the sandbox's accessor paths —
    `env_clear()` alone is not proof of hermeticity. The non-Linux E2E CI job is gated on this
    table being implemented.
- Staleness reproduction promoted to an early investigation ticket with the traced arrangement
  (index under config A, change the config, re-trust — a stale config blocks all loads until
  re-trusted — then query untouched notes); determine intended behavior first, then keep the
  regression at the narrowest level that captures config-to-index invalidation.
- `refresh()` fail-open is an in-crate unit (not inducible from outside); EPIPE exit behavior is
  an open decision, not a ticket until the contract is chosen.

**Consolidation**
- Deletions run only after gap tests, and only after re-running the duplication analysis against
  the post-gap inventory (explicitly re-evaluating the persistence roundtrip, the E2E dispatch
  persistence tests, and the task-filter file against their new rivals).
- Every deletion carries a ledger entry: behavior, former layer, defect class, surviving test.
  Test-count deltas are bookkeeping, not a gate.
- Confirmed removals: the byte-equivalent config pair (keep one, plus at most one trust-store
  smoke for the public untrust surface), the schema file whose five tests all use a test-only
  constructor with source twins (lands with its replacement template-class coverage — the ledger
  must note that the replacement proves schema-driven class expansion, so field-inheritance
  residue is covered by source-local twins, not by that replacement), the config
  trust/lifecycle twins (unit and E2E coverage confirmed), duplicate unit/integration twins, E2E
  collapses, dead tempdirs. The golden-path test is explicitly NOT deleted yet — decision
  deferred to after its replacement coverage lands.
- Task-classification disposition default is **delete-as-redundant at the recompute step** (all
  three tests' assertions are unit-twin'd and the disk-config gap moves to E2E), with a ledger
  entry — **not** a fold into `index_query`. Folding is permitted only if the E2E config test
  fails to land *and* a surviving composition test keeps a failure boundary its units don't
  cover; cohesion beats file count. (This supersedes the fold-default in the audit's §12.)
- The config-TOML literal is **four copies, not three** (two in the crate's test-support code,
  one in an in-crate CLI test fixture, one in the E2E sandbox): crate-side copies collapse into
  one internal constant in the private test-support module (zero exports — private modules don't
  surface even though they're re-exported selectively at the root); the E2E copy stays local
  because E2E imports nothing from the crate. `tests/common` is scoped to what **both external
  test roots** actually duplicate — the benches hold zero copies (not a consumer), so no
  cross-layer miracle is required.

**Facade & harness**
- One `#[cfg(any(test, feature = "test-utils"))] pub mod testing` module becomes the sole
  test-only export surface. **This relocation is a path change only:** the identifier set
  exported at the root today (45 names by direct enumeration at spec time — the audit's "44" is
  off by one; re-derive at T0) must be exactly the set `testing` exports afterwards — zero
  additions, zero deletions. No root-level `pub use testing::…` shim may exist or be added, no
  glob re-exports, `mod test_support` stays private, and root aliases for in-crate use are
  `pub(crate)` and unconditional (never `pub`, never cfg-split). The paired aliases that exist
  today cover only ~8 names — do not manufacture new root paths to silence a missed consumer;
  re-path the consumer. Benches and doctests re-path with the integration files. Because the set
  is frozen, this ticket is independent of pruning and therefore lands **before** the gap tests.
- Pure arrangement shared only where sharing is real: `tests/common` (included by both external
  test roots, imports nothing from the crate) holds what those two roots duplicate; crate-side
  fixture literals collapse to one internal constant inside the private test-support module —
  zero new exports. Behavior constructors remain layer-local: integration trusts through the
  facade; E2E trusts only by spawning the CLI (already true).
- Fixtures become explicit: a config fixture with declared sections and raw-TOML writing for
  invalid cases (scheduled **before** the config gap tests that need it); arrange and trust
  split; the directory-existence-flipping accessor renamed **in place** (never add-new-keep-old
  — that would add a method to an exported type); the doc-vs-behavior divergence in the
  trusted-project helper fixed. Any fixture capability that would have to live *on an exported
  type* is a surface addition: default answer is a helper in `tests/common` or a spawn; a facade
  addition requires an itemized export-set update reviewed as a spec amendment, never as an
  implementation convenience. The OS-state-dir default constructor (`impl Default for
  ConfigService`, zero callers) is **removed outright**, not cfg-gated — gating behind
  `test-utils` would not stop any test, since every test target compiles with that feature.
- Assertion style: behavior-under-test `.expect` becomes assertions as a **standing convention
  for tests written from this spec onward** (not just a cleanup pass); the blanket expect allow
  is vestigial (already permitted by lint config) and removed as noise, not as a fix. Narrowing
  the lint config itself so `expect` fails outside fixture modules is a separate decision — not
  adopted here.

**Runner & CI**
- Four phases separated by feature configuration — unit/component on the production
  configuration, integration on the test feature, E2E on the shipped (feature-off) binary, and
  **doctests pinned explicitly to `--all-features`** (never default: test-support doctests are
  cfg'd to the feature and a default run would silently drop ~5 of them — a doctest-count
  assertion in CI catches exactly this), aggregated by the canonical test task. Rationale is the
  guarantee, not current behavior (no cfg branch changes behavior today).
- Watch task → canonical runner; broken feature-flag promise fixed; nextest timeouts +
  slow-test visibility; CI gains default-feature run and non-Linux E2E (gated on the sandbox
  Windows fix) and a split test job; module-filter and task-name docs corrected; coverage/
  mutation recorded as gate-or-advisory with targeted post-redesign runs (note: the mutation
  tool excludes the CLI module, so CLI-covered seams need process-level fault injection).

**Naming**
- Organizing rule: unit files name components; integration files name the seam whose failure
  needs ≥2 production components; E2E files name externally observable capabilities. Integration
  and E2E need not share filenames.
- Trees derived after pruning: integration ~5 files (persistence, refresh, query composition
  canaries, template render, optionally one config file if config behavior lands there —
  default is E2E; `task_classification` absent by default per the delete-as-redundant
  disposition, retained only under the T7.4 clause); E2E capability files as listed in the user
  stories plus one generic process-contract file — which must not be named with meta-words or
  grouped by error-renderer. New gap tests are written directly into these capability names;
  only legacy tests are relocated at the naming step.
- Test renames align with glossary vocabulary (avoid marked terms; use trust status, index
  store, refresh, task terminology as defined per-module glossaries).

## Visibility freeze (hard constraint)

The single greatest failure mode of a test-architecture effort is that implementing it quietly
widens the crate's visibility — one `pub` here, one adapter there — and the expansion cascades
into rustdoc, API commitments, and deeper test coupling. This spec permits exactly **one**
surface change: the facade relocation below, which is a path change with a byte-identical
membership. Everything else is frozen.

**Rules — every ticket, no exceptions:**

1. **No new `pub`, anywhere, including `#[cfg(feature = "test-utils")] pub`.** Gating a new
   `pub` behind the test feature does not make it acceptable — cfg-gating is not a mitigation,
   it is a member of the exported set. The "narrow test-only adapter" fallback for config tests
   is therefore **withdrawn**: if a branch is unreachable from existing exported names, cover it
   with an in-crate `#[cfg(test)]` unit (precedent: in-crate tests already call `load`) or a
   spawn — never a new export.
2. **No widening of `pub(crate)` → `pub`**, no new methods on exported types (renames happen
   **in place**), no new re-exports, no second public module path, no glob re-exports
   (`pub use …::*` appears nowhere in `src/` today — keep it that way).
3. **Facade relocation rules:** `testing` exports exactly today's root-export set; `mod
   test_support` stays private; no root shims; root aliases `pub(crate)` and unconditional;
   facade keeps `#[cfg(any(test, feature = "test-utils"))]` so reachability is preserved
   exactly. A seam integration "needs" beyond the frozen set is satisfied by `tests/common`, an
   in-crate unit, or a spawn — a spec amendment, not an implementation choice.
4. **Harness growth is test-crate-internal:** new `Run`/`Sandbox` methods, env scrub, accessors,
   `tests/common` — all live in the test targets; zero `src/` changes. `tests/common` imports
   nothing from the crate.
5. **Surface reductions are allowed:** removing `impl Default for ConfigService` (zero callers),
   deleting tests, and removing fixture helpers are shrink-only and need no special approval.

**Verification (bootstrapped at T0, enforced per ticket):**

- `cargo public-api` is **already a locked repo tool** (`mise.toml:51`, nightly toolchain pinned)
  — commit two baselines at T0: default features and `--features test-utils`, plus a sorted
  textual snapshot of all `pub`/`pub(crate)` declaration tokens in `src/` (catches
  `pub(crate)`→`pub` widening and new methods that the api diff can miss).
- Per ticket: production baseline diff must be **empty**; test-utils baseline *additions* only
  in the facade ticket — shrink-only removals and in-place renames (rules 2/5) change the
  baseline in the ticket that performs them, each with an itemized old→new justification;
  textual token diff empty; `rg 'pub use .*\*' src/` → 0.
- Compile-fail probes (rustdoc `compile_fail` doctests on `src/lib.rs`, no new dependency)
  assert the config internals (`ConfigLoadError`, `ConfigBuilder`, `SchemasConfig`,
  `FrontmatterConfig`) remain unnameable from outside, each paired with a positive-control
  doctest so a rename can't silently satisfy the probe. An existing tripwire also stands:
  `#[expect(private_interfaces, …)]` on the config error mapping — a widening that makes those
  types reachable fails `mise run lint` via unfulfilled-lint-expectation.
- Wire the whole check set into a mise task and both `hk` local-quality and CI, or it enforces
  nothing.

## Testing Decisions

**What makes a good test here (the audit's evidence standard, applied forward):**
- A test must assert exactly what its name and doc claim — every new test carries an "executes
  vs asserts" note and names the **defect class** it prevents (e.g. argv-parse regression,
  stale-secondary-index, trust-before-parse confusion, cross-process persistence).
- Tests observe external behavior at the highest seam that can uniquely prove it: process
  contracts at the spawned binary, cross-component behavior through public APIs with a real
  filesystem, private contracts in-crate. No test asserts implementation detail that a sibling
  layer already observes; vertical redundancy survives only when it proves a new failure boundary
  (exit code, stream split, cross-process state).
- Arrangements must reach the branch under test: pinned persist-first for the parity differential,
  trust-then-corrupt for parse failures, re-trust after config edits, divergence precondition for
  cross-process reads.
- **Non-mutating observation invariant:** a test asserting that a *prior* action took effect
  must observe through a path that cannot itself perform that action — for integration,
  `IndexerService::load()` + `QueryService::run` (both already exported; used today by
  `index_persistence_roundtrip`); `sync_and_run` and spawned refreshers are permitted only when
  the refresh/synchronization **is** the act under test. (A test that refreshes before asserting
  a prior refresh is exactly the self-validating defect this spec exists to remove.)
- **Divergence precondition for read claims:** any assertion whose claim is "the persisted store
  was read" (as opposed to "the observable behavior was correct") must name its declared
  divergence scenario; no plain-content assertion may make that claim. E2E never proves cache
  reuse as an implementation detail — it proves correct observable behavior, with divergence
  reserved for the one cross-process-read canary.

**Which modules/layers are tested (by seam):**
- *In-crate:* config fail-open unit; relocated dialog-preset component test; fixed store-edit
  assertion; existing exhaustive semantics unchanged (~98% already correctly placed).
- *Integration (via the `testing` facade's **frozen** export set — no ticket in this spec may
  add to it):* query-visible refresh delta observed via `load()`, `@class` through the
  template composition root (requires **zero new exports** — `TemplateService`,
  `render_to_file`, `DryRun`, `PresetDialogProvider`, `write_schema` are all exported today, and
  the expander attaches internally), store-backed parity differential, existing
  persistence/template coverage; shrinks by the confirmed duplicate deletions.
- *E2E (spawned binary, scrubbed env):* exit codes and argv contract, stream contracts, config
  stages and config-driven behavior, overwrite refusal, corruption self-heal, non-TTY stdin,
  edit/delete/rename between spawns, cross-process divergence, global config via child env,
  spawned init, template output re-enters index.
- *Harness itself:* env-isolation assertions (child observed the scrubbed variables), layer-
  enforcement grep task, non-empty-suite CI assertion.

**Prior art in this codebase:**
- The four certified cross-process tests (untrust observing trust across processes, tracked
  list, golden path) are the model for spawned-state assertions.
- The custom-markers E2E test (one exact-stdout contract concatenating what would otherwise be
  four unit tests) is the named model for stdout contracts.
- Nextest process-per-test isolation and the `Sandbox` helper are the existing arrangement
  machinery; bench `required-features` entries are the manifest precedent for the silent-green
  guard; the repository's local markdown tracker (this spec's sibling `issues/`) is the delivery
  mechanism, one PR-sized ticket per file with blocking edges.

## Out of Scope

- **Production behavior changes.** The config-change staleness question and the EPIPE/exit
  contract are *decided, not fixed*, in this spec: reproduce/decide first; any product fix
  becomes a separate spec. No production code path is modified except where a fix is required to
  make an assertion truthful (the two broken test assertions and the fixture doc divergence).
- **Widening any surface for tests** — production *or* `test-utils`-gated: adding a
  `pub`/`pub use`, widening `pub(crate)`→`pub`, adding a method to an exported type, adding a
  re-export, adding a second public module path, a glob re-export, or a cfg-gated adapter — is
  explicitly rejected (see Visibility freeze). Routes go through spawning, the template
  composition root, `tests/common/`, or in-crate `#[cfg(test)]` units. The only sanctioned
  surface change in this spec is the facade relocation, whose output set must equal today's set
  exactly.
- **New dependencies**, including a PTY crate for process-level exit-130 — without explicit
  approval, 130 stays in-crate-only per the conditional in the gap-test decisions.
- **Coverage/mutation enforcement in CI** — a recorded gate-or-advisory decision plus targeted
  local runs only; no CI gate added here.
- **Deleting the golden-path test** — deferred until replacement coverage exists (ticket-level
  decision, likely a rename).
- **A dedicated testing-strategy ADR** — not created by this spec; the audit remains the record.
  (No ADR currently covers testing; if one is wanted, that is a follow-up.)
- **Windows/macOS CI execution beyond what the sandbox fix unlocks** — the fix is in scope (it
  is an isolation contract), the OS matrix expansion rides on it in the runner phase.
- **Performance/benchmark suites, doctest content rewrites beyond re-pathing, and any test count
  targets.**

## Further Notes

- **Dependency edges (for ticket generation):** honesty → **prerequisites (facade namespace,
  `tests/common`, `ConfigFixture`, assertion-style convention)** → E2E invariant → gap tests →
  (gap ∥ investigation) → recompute → pruning → remaining harness cleanup ∥ naming; runner/CI
  depends on the env-policy table for its OS-matrix item but is otherwise parallel. Naming
  strictly after pruning; the schema-file deletion strictly with its replacement coverage.
- **Evidence citations live in the audit**, not here: each ticket should pull its
  `path::test_name` citation and defect-class note from the corresponding audit section
  (recommendations §14, plan §15, gap detail §10) — **except where this spec supersedes the
  audit.** Superseded audit details (do not implement the audit's version): §12's
  "fold task-classification into `index_query`" (now delete-as-redundant by default), §14's
  three divergence recipes (now one canonical + one unix alternative; staleness is
  investigation-only), the "triplicated" config literal (four copies), the "44 names" export
  count (45 — re-derive at T0), the eight-stage process table (now four representatives), and
  any wording that process-level exit-130 is unconditional (it's conditional on a PTY
  mechanism).
- **Adjudicated history matters:** three external critiques were folded in (Revisions 2–4).
  Decisions marked rejected in the audit's §16/§16b/§16c should not be re-proposed by
  implementers without new evidence — notably: cwd mutex (replaced by removal), direct
  query-service `@class` test (vacuous), plain content cross-process assertions (indistinguishable
  from rebuild), `compile_error!` guard, test-count gates, and pinning exit 101.
- **Suite at HEAD of audit:** 3,127 nextest + 76 doctests green in ~11 s; consolidation removes
  roughly 2% of the suite while gap tests add ~12–15 stronger ones. The ledger, not the count, is
  the acceptance criterion.
