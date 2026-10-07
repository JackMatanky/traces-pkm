# Audit G — Tooling, Cargo & CI (how tests actually compile and run)

Read-only audit. No files outside this report were modified; no `cargo build/test/check`
was executed (other agents hold the lock). Where a claim could be verified without
building (cargo/nextest `--help`, `cargo tree`, `cargo metadata`), it is marked
**[verified]**; everything else is derived from config/source and marked **[derived]**.

---

## 1. Executive summary

| Question | Answer |
| --- | --- |
| Test binaries produced | **4 libtest harnesses** (`traces_pkm`, `traces-pkm`, `integration`, `e2e`) + **doctests** (separate, lib only) |
| Canonical command | `mise run test` (alias `t`) = `cargo nextest run -p traces-pkm --all-features` → then `cargo test --workspace --all-features --doc -- --quiet` |
| Is plain `cargo test` green with 0 integration tests? | **Yes** — `tests/integration.rs:5` cfg-strips the whole target when `test-utils` is off, and no `[[test]]` section gates it |
| Can E2E be selected alone? | **Yes, today**: `--test e2e` / `-E 'binary("e2e")'` / `cargo test --test e2e` (no feature gate on `tests/e2e.rs`) |
| Suite partitioning in CI | **None**: one `test` job, one nextest pool, no `--partition`, no test-groups, no timeouts |
| Broken documented flag | `--feature default` → `cargo … --features default` → **hard error** **[verified]** |

---

## 2. `Cargo.toml` — targets, features, lints, profiles

### 2.1 Target declarations (no `[[test]]` sections — confirmed)

- `[lib] bench = false` — `Cargo.toml:29-30`; `[[bin]] bench = false, name = "traces-pkm"` — `Cargo.toml:32-35`.
  Both are deliberate: they keep `cargo bench` from reaching libtest's harness with
  Criterion flags (`Cargo.toml:24-28` comment).
- **`grep -n "\[\[test\]\]"` → no matches.** The only `[[…]]` sections are the **12
  `[[bench]]` entries** at `Cargo.toml:40-98`, every one with
  `harness = false` **and** `required-features = ["test-utils"]`.
- Consequence: `tests/integration.rs` and `tests/e2e.rs` are **auto-discovered** and have
  **no `required-features`**, so Cargo always builds them — the *only* thing that can
  empty the integration suite is an in-file attribute (§3).
- `examples/` contains no `.rs` files (only a tracked `examples/.traces/schemas/*` data
  dir) → **no example targets**.

### 2.2 Feature matrix

```toml
[features]
test-utils = []        # Cargo.toml:182-183
```

There is **no `default` feature**. Therefore:

- `--all-features` ≡ `--features test-utils` ≡ the only non-empty configuration.
- `--no-default-features` ≡ plain (default) configuration — nothing to turn off.
- `--features default` is **not** a valid feature: `cargo tree --features default`
  → `error: the package 'traces-pkm' does not contain this feature: default`
  **[verified]**.

`test-utils` (and `cfg(test)`) gates **63 sites** in `src/`, the bulk being
`#[cfg(any(test, feature = "test-utils"))] pub use …` visibility widening in
`src/lib.rs:88-159` and the inverse `#[cfg(not(any(test, feature = "test-utils")))]`
re-exports at `src/lib.rs:90,114,116,146`. There are also
`#[cfg_attr(not(any(test, feature = "test-utils")), expect(dead_code, …))]`
canaries on test/bench-only introspection methods
(`src/index/inlinks.rs:67,136,152,167,182`, `src/schema/service.rs:53`).

**Why this matters:** those `not(any(test, feature))` branches are only compiled when the
crate is built *without* `test-utils`. Every compile/lint/test CI job passes
`--all-features` (§6), so in CI they are reached only by the `docs` job
(`cargo doc --no-deps`, no feature flags — `mise.toml:491`). **[derived]**

### 2.3 dev-dependencies (`Cargo.toml:166-177`)

`criterion` (+html_reports), `pretty_assertions`, `rstest`, `rustix` (+fs),
`stats_alloc`, `tempfile`, `tracing-subscriber` (+registry).
All are available to *every* test target (lib, bin, integration, e2e) and to benches.

### 2.4 Test-affecting lints and how `tests/` escapes them

Package-level denials in `[lints.clippy]` — all apply to test targets too, because
Cargo emits `[lints]` for every target of the package:

| Lint | Level | Cargo.toml line |
| --- | --- | --- |
| `arithmetic_side_effects` | deny | 200 |
| `disallowed_methods` | deny | 204 |
| `expect_used` | deny | 206 |
| `indexing_slicing` | deny | 208 |
| `panic` | deny | 212 |
| `print_stdout` | deny | 214 |
| `unwrap_used` | deny | 224 |
| `print_stderr` | **allow** | 254 |
| `unsafe_code` (rustc) | deny | 269 |

Escape hatches, in order of who does the work:

1. **clippy config, not source** — `clippy.toml:86-87`:
   `allow-unwrap-in-tests = true`, `allow-expect-in-tests = true`.
   Test targets are compiled with rustc's `--test` harness, so clippy treats them as
   test code. This is what lets **181 `.unwrap()`/`.expect(` call sites in `tests/`**
   survive `deny(unwrap_used)`/`deny(expect_used)`.
   *Evidence that this — not an attribute — is doing the job:* `tests/e2e.rs` has
   **zero** `#![allow(...)]` attributes yet uses `.expect(` heavily
   (`tests/e2e/support.rs:112-202`) and CI runs
   `cargo clippy --all-targets --all-features -- -D warnings`
   (`.github/workflows/ci.yml:57`). **[derived]**
2. **Crate-level allow, integration target only** — `tests/integration.rs:6-10`:
   `#![allow(clippy::expect_used, reason = "…fixture itself is broken…")]`.
   Belt-and-braces (or vestigial); `tests/e2e.rs` has no equivalent.
3. **Per-site `#[expect(...)]`** — the ones clippy's test exemption does *not* cover:
   - `tests/e2e/support.rs:239-242`: `#[expect(clippy::disallowed_methods, …)]` for
     `env::current_dir` (disallowed at `clippy.toml:90` because "global state → races").
   - `src/schema/fields/date.rs:75,96`: `#[expect(clippy::panic, …)]` inside `#[test]`
     fns — i.e. **`clippy::panic` still fires in test code**, so `deny(panic)` is *not*
     test-exempt.
4. **Nothing to escape for `print_stdout`**: `grep -rn "panic!\|println!\|print!(" tests/`
   → **0 matches**. `print_stderr` is allowed package-wide (`Cargo.toml:254`).

`grep -rn "allow(" tests/` → only `tests/integration.rs:6`. So `indexing_slicing`,
`arithmetic_side_effects` are *not* exempted for tests; `tests/` simply does not trip
them. **[derived]**

### 2.5 Profiles (`Cargo.toml:278-295`)

| Profile | Definition | Test impact |
| --- | --- | --- |
| `bench` | inherits `release`, opt 3, debug, thin LTO (`:278-283`) | measurement only |
| `release` | opt `z`, strip, LTO, `panic = "abort"`, cgu 1 (`:285-290`) | **not used by any test task**; `panic=abort` would change panic-based test behaviour if someone ran `cargo test --release` |
| `mutants` | inherits `test`, opt-level 1, debug off (`:292-295`) | used by `test:mutants` via `--profile mutants` (`.mise/tasks/test/mutants:127`) |
| `dev`/`test` | **not overridden** | plain test runs execute at opt-level 0 |

---

## 3. Which test binaries exist and what each contains

`Cargo.toml` has no `[[test]]` → auto-discovery from `tests/*.rs`:

| Binary | Source | Feature-gated? | Contents |
| --- | --- | --- | --- |
| `traces_pkm` (lib) | `src/**` `#[cfg(test)]` | no (`cfg(test)` also opens test-utils paths, `src/lib.rs:88`) | ~**2226** `#[test]` attrs across `src/` (incl. `src/cli/*`, which is `pub mod cli` in the **lib**, `src/lib.rs:61`) |
| `traces-pkm` (bin) | `src/main.rs:37` `mod tests` | no | **4** `#[test]` fns (exit-code mapping) |
| `integration` | `tests/integration.rs` + `tests/integration/*.rs` | **YES** — `#![cfg(feature = "test-utils")]`, `tests/integration.rs:5` | **30** `#[test]` across 6 files (`config_lifecycle`, `index_persistence_roundtrip`, `index_query` (11), `schema_field_resolution`, `task_tag_filters`, `template_render`) |
| `e2e` | `tests/e2e.rs` + `tests/e2e/*.rs` | **no feature gate** (`tests/e2e.rs:1-20` is only module docs + `mod` decls) | **27** `#[test]` (`dispatch` 20, `template_write` 2, `tracked` 2, `golden_path` 1, `init` 1, `untrust` 1) |
| doctests | `src/**/*.rs` fenced blocks (13 files) | n/a | run only by `cargo test --doc` |

### Default-feature build: what happens to `integration`?

`#![cfg(feature = "test-utils")]` is a **crate-root** attribute, so with the feature off
the cfg-expansion removes every `mod` item: `tests/integration/*.rs` is **never even
parsed/compiled**, and the harness binary links as an effectively empty crate.

- `cargo test` (libtest) → prints `running 0 tests`, **exit 0** — silent. **[derived]**
- `cargo nextest run` → that binary contributes 0 tests; the run still passes because
  other binaries have tests (nextest's `--no-tests` only fires when *nothing* runs:
  nextest default is `auto`, documented as *"defaulting to fail"* / *"0.9.85 The default
  is fail"* → exit code 4 `NO_TESTS_RUN`) **[verified from `cargo nextest run --help`]**.
- Selecting it explicitly on a default build — `cargo nextest run --test integration` —
  selects an empty target → 0 tests → **exit 4 (loud)**; the same selection under
  `cargo test --test integration` → **exit 0 (silent)**. **[derived]**

So: **a plain `cargo test` is green while running 0 integration tests**, and there is no
`[[test]] required-features` line that could make Cargo skip it loudly instead
(required-features would in fact make it skip *silently* too — the repo already
documents that behaviour for benches: `.mise/tasks/bench/_default:145-146`
"`cargo test` silently skips targets when [required-features are] unmet").

---

## 4. mise tasks — the real entry points

### 4.1 `.mise/tasks/test/*`

**`test` (`.mise/tasks/test/_default`, alias `t`)**

- `#USAGE flag "--feature <feature>" default="all"` (`:15-23`) — help claims
  "`all`: `--all-features`; `default`: passes no feature flags".
- `#USAGE flag "--docs" negate="--no-docs" default=#true` (`:24-32`) — doctests on by default.
- Flow (`main`, `:127-134`): `run_unit_phase` → `test:unit` (`:63-77`), then
  `run_doc_phase` → `test:doc` (`:109-114`) unless `--no-docs`.
- Extra args are forwarded through a literal `--` (`:72-74`), i.e.
  `mise run test --test e2e` → `mise run test:unit --feature all -- --test e2e`.

**`test:unit` (`.mise/tasks/test/unit`)** — *name is misleading: it runs
lib + bin + integration + e2e, i.e. everything nextest selects.*

- `resolve_package_feature_args` (`:51-57`): `all` → `-p traces-pkm --all-features`;
  **any other value → `-p traces-pkm --features "<value>"`**.
- `resolve_module_filter` (`:71-75`): `-m <mod>` → `-E "test(/^<mod>::/)"`.
- Executes `cargo nextest run "${unit_args[@]}"` (`:111`).

**`test:doc` (`.mise/tasks/test/doc`)** — `cargo test --workspace --all-features --doc -- --quiet`
(`:41-47`, `:81-83`). Same `--feature` mapping (`all` vs `--features <value>`).

**`test:mutants` (`.mise/tasks/test/mutants`)** — `#MISE depends=["test"]` (`:5`), so the
full suite runs *before* every mutation run; `TEST_FLAGS="--features test-utils
--all-targets --profile mutants"` (`:127`), `TIMEOUT_COEFFICIENT=5` (`:128`),
`DEFAULT_TARGET="./src..."` (`:129`), and it always passes `--config mutarust.yml
--logger-agentic-json` (`:288`).

### 4.2 **Bug: `--feature default` is broken**

Help text at `.mise/tasks/test/_default:20-21`, `test/unit:19-20`, `test/doc:10-11`
all promise "`default`: passes no feature flags". The implementation only special-cases
the literal `all`; everything else is forwarded as `--features <value>`
(`test/unit:51-57`, `test/doc:41-47`). With no `default` feature declared
(`Cargo.toml:182-183`), `mise run test --feature default` becomes
`cargo nextest run -p traces-pkm --features default` →
`error: the package 'traces-pkm' does not contain this feature: default` **[verified
via `cargo tree --features default`]**. The *documented* way to run the default-feature
suite therefore does not work; only `--feature all` and `--feature test-utils` do.

### 4.3 **`-m/--mod` silently drops integration + e2e**

`-m index` → `-E 'test(/^index::/)'` (`test/unit:71-75`). Unit tests are named
`index::<mod>::…` so they match; integration tests are named `index_query::…`,
`config_lifecycle::…` (crate-root `#[path] mod`, `tests/integration.rs:12-23`) and e2e
tests are `dispatch::…`, `init::…` — none match `^index::`. The run still exits 0 (unit
tests matched), so `mise test -m index` is a **unit-only** run whose help text only says
it "filters to tests under module `index`" (`test/_default:8-10`).

### 4.4 **Stale usage example**

`test/_default:38` and `test/unit:28` document `mise run test --test init_cli`; there is
no `init_cli` test target (targets are `integration` and `e2e`).

### 4.5 `mise.toml` tasks

| Task | Line | Command / effect |
| --- | --- | --- |
| `watch` | `mise.toml:140` | `cargo watch -x check -x test` → **bare `cargo test`** (default features ⇒ 0 integration tests; libtest ⇒ e2e cwd race, §5/§9) |
| `check` | `mise.toml:112-117` | `cargo check --workspace` (**no `--all-features`**) then `hk check` with `--skip-step cargo-test/clippy/doc/deny` → **runs no tests** |
| `verify` | `mise.toml:501-504` | `fmt` first, then `check` + `lint --no-fix` + `test` in parallel |
| `lint` | `.mise/tasks/lint:31` (`cargo_args=(--workspace --all-targets --all-features)`), `-D warnings` on by default (`:13-16`, applied `:110`) | clippy over all targets/features incl. `tests/` |
| `doc` | `mise.toml:478-492` | `cargo doc --no-deps`, `RUSTDOCFLAGS="-D warnings"`, **no feature flags** |
| `coverage:lcov` | `mise.toml:348` | `cargo llvm-cov nextest --workspace --all-features --lcov … --ignore-filename-regex "$COVERAGE_IGNORE_REGEX"` |
| `coverage:html` | `mise.toml:405-407` | same + `--html`, optional `-m` scope via sibling-exclusion (`:376-401`) |
| `COVERAGE_IGNORE_REGEX` | `mise.toml:67` | `src/main\.rs$\|src/cli($\|/)\|src/(.*/)?error\.rs$` (POSIX ERE; a bad regex silently disables **all** exclusions, `:63-66`) |
| `task_templates.mutants` | `mise.toml:513-516` | `timeout = "1h"`, `outputs = []`, `cache = { enabled = false }` |
| `task_templates.bench` | `mise.toml:510-511` | `timeout = "20m"` (file tasks can't declare timeout) |
| `gate:never-skip` | `mise.toml:526-528` | `outputs = []` + `cache = false`; `test`, `test:unit`, `test:doc`, `test:mutants`, `verify`, `lint`, `fmt` all extend it → **tests are never freshness-skipped** despite `task_config.cache.enabled = true` (`mise.toml:72-75`) |

Coverage notes: `coverage:*` uses the `llvm-cov nextest` subcommand, and **nextest does
not run doctests** — the separate `test:doc` phase exists precisely because of that
(`test/_default:24-32`). So doctests contribute nothing to coverage numbers. **[derived]**

---

## 5. `.config/nextest.toml` — the entire file is 11 lines

```toml
[profile.default]
status-level = "fail"     # .config/nextest.toml:8
success-output = "never"  # .config/nextest.toml:11
```

That is all. **No** `test-threads`, `retries`, `slow-timeout`, `global-timeout`,
`default-filter`, `test-groups`, `overrides`, or second profile. (The `NEXTEST_PROFILE=<profile>`
hint at `:5` references profiles that don't exist.)

Built-in defaults therefore apply **[verified via `cargo nextest help repo-config`]**:

| Setting | Effective value | Consequence |
| --- | --- | --- |
| `test-threads` | `num-cpus` | unit, integration and e2e share **one** pool, all binaries in parallel |
| `retries` | `0` | no flake tolerance |
| `slow-timeout` | `60s` period, **no termination** | a hung test is only *marked* slow — never killed |
| `status-level` | repo override `fail` | even the slow/leak markers are **suppressed** (default `pass` would show them) |
| `global-timeout` | unset | e2e hangs block until the GitHub job limit |
| `default-filter` | `all()` | no suite is excluded by default |
| process model | one process per test | cwd/env mutations are isolated (see §9) |

Nothing serializes or partitions suites: no `--partition` anywhere in the repo, no
`threads-required`, no `[profile.ci]`.

---

## 6. `.github/workflows/ci.yml`

| Job | Line | OS | Runs tests? | Feature flags |
| --- | --- | --- | --- | --- |
| `check` | `:27-43` | **ubuntu, macos, windows** (`:32`) | no — `cargo check --workspace --all-targets --all-features` (`:43`) | `--all-features` |
| `clippy` | `:45-57` | ubuntu | no — `cargo clippy --workspace --all-targets --all-features -- -D warnings` (`:57`) | `--all-features` |
| `test` | `:60-70` | **ubuntu only** (`:61`) | **yes** — installs nextest (`:69`) then `mise run test` (`:70`) | `--all-features` (task default) |
| `hygiene` | `:73-95` | ubuntu | no — `hk check --all --no-fail-fast … --skip-step cargo-test` (`:94`) | n/a |
| `secrets` | `:97-110` | ubuntu | no | n/a |
| `audit` | `:113-130` | ubuntu | no | n/a |
| `docs` | `:131-140` | ubuntu | no — `mise run doc` → `cargo doc --no-deps` (`mise.toml:491`) | **none** |
| `msrv` | `:142-164` | ubuntu | **no** — `cargo check --workspace --all-features` at rust-version 1.96 (`:163-164`) | `--all-features` |

Observations, all evidence-backed:

- **Exactly one job executes tests**, on one OS. macOS/Windows only compile the test
  targets (`--all-targets`), never run them.
- **No `--no-default-features` / `--features test-utils` job exists** — so the
  default-feature configuration (including the `cfg_attr(not(any(test, feature)),
  expect(dead_code))` canaries, §2.2) is compiled in CI only by the `docs` job.
- **MSRV never runs a test.**
- **No coverage job, no mutation job** — `coverage:*` and `test:mutants` are local-only.
- **No partitioning/sharding**: no `--partition`, no matrix split of the test job, no
  e2e step. e2e, integration and ~2230 unit tests all run inside the single
  `mise run test` invocation.
- e2e is separable *by name* today (`--test e2e`, `binary("e2e")`) but CI doesn't use it.

---

## 7. Mutation testing (`mutarust.yml` + JSON + task)

- Engine: `mutarust` 0.1.10 (`mise.toml:36`), config **only** read via
  `--config mutarust.yml` (`mutarust.yml:2-5`).
- Exclusions: `exclude_dirs: [src/cli]` (`mutarust.yml:21-22`) — a port of the old
  `.cargo/mutants.toml` `exclude_globs`; note `mise.toml:64` still says "keep in sync
  with .cargo/mutants.toml's exclude globs" but **`.cargo/` does not exist** (stale
  pointer). `src/main.rs` and the 8 `src/**/error.rs` files *are* mutated
  (`mutarust.yml:15-20`).
- `disable_mutators: ["select/*"]` (`:27-29`); `skip_without_test: false`,
  `skip_with_cfg: false` (`:30-35`); `enable_mutators: []` = all remaining on (`:49`).
- Score gate is **commented out** (`min_msi: 60`, `mutarust.yml:37-45`) → mutation is
  advisory; only `--fail-on-escaped`/`--min-msi` on the CLI can gate.
- Structure **[verified by parsing]**:
  - `mutarust-baseline.json` = `{version: 1, mutants: [ {id, file, mutator, line}×1 ]}`
    (single accepted escape in `src/strsim.rs:15`) — tracked in git.
  - `mutarust-agentic.json` = `{generated_at, msi, escaped_count, reminder, mutants[]}`;
    currently `msi 0.0`, `escaped_count 0` (a dry-run artifact) — gitignored
    (`.gitignore:21`) together with `/report.json` (`.gitignore:20`) and
    `mutarust-report.html` (`.gitignore:24`).
- Task: `depends = ["test"]` (`.mise/tasks/test/mutants:5`) + `timeout = "1h"`
  (`mise.toml:514`), `--test-flags "--features test-utils --all-targets --profile mutants"`
  (`:127`), adaptive `--timeout-coefficient 5` (`:128`, emitted at `:299`).
  `--all-targets` also selects the 12 `[[bench]]` targets (all have their
  `required-features` satisfied by `--features test-utils`); Criterion 0.8.2 detects a
  test-style invocation and runs in *test mode* (one pass, no measurement) —
  `criterion-0.8.2/src/lib.rs:959-963`.

---

## 8. hk / pre-commit

```
["cargo-test"] = new Step { check = "mise run test" }   # hk.pkl:112-115 (in `quality`, hk.pkl:107)
pre-commit: format + validate only                       # hk.pkl:124-131  → NO tests
pre-push:    validate + security + quality + cargo-sweep # hk.pkl:133-140  → tests run
hk check:    format + validate + security + quality      # hk.pkl:157-165  → tests run unless skipped
```

- **`pre-commit` runs no tests** (`hk.pkl:124-131`).
- `pre-push` runs the full `mise run test` (nextest + doctests) — `hk.pkl:112-115,133-140`.
- `mise run check` explicitly skips it: `--skip-step cargo-test` (`mise.toml:115`), and CI's
  hygiene job does the same (`ci.yml:94`). Net: in CI, the **only** test execution is the
  dedicated `test` job.

---

## 9. Platform-specific code vs. CI matrix

- `grep -rn "cfg(windows)|cfg(unix)|cfg(target_os" tests/` → **0 matches**. `tests/` has no
  platform-conditional tests at all; every e2e/integration test is expected to pass on all
  three OSes.
- `src/` does: `src/dirs.rs:101` (`target_os = "macos"`), `:114` (`windows`),
  `:137`/`:150`, plus `all(unix, not(macos))` state-home branches — i.e. **three distinct
  production config-resolution paths**, only one of which (Linux) ever runs a test in CI.
- `#[cfg(unix)]` test code also exists under `src/` (e.g. `src/template/loader.rs:541,560,644`,
  `src/template/writer.rs:480,909`, `src/cli/index.rs:159`) → compiled only on unix hosts.
- e2e is process-spawning and cwd-sensitive but **per-child-process isolated**:
  `Sandbox::command` sets `current_dir` only on the child (`tests/e2e/support.rs:187-194`)
  and `TRACES_BIN = env!("CARGO_BIN_EXE_traces-pkm")` (`:60`) is a Cargo-provided absolute
  path (`.exe` on Windows), so it is Windows-portable in principle.
- **The real cwd hazard is in-process**: `CwdGuard::enter` calls `env::set_current_dir`
  with **no lock** (`tests/e2e/support.rs:234-256`), used by 4 tests
  (`tests/e2e/init.rs:29,42,55`, `tests/e2e/golden_path.rs:52`). The harness itself
  documents this as a known limitation (`tests/e2e/support.rs:33-49`: "there is no lock —
  meaning true concurrent cwd mutation between them is a known, accepted limitation").
  The crate's own unit tests *are* serialized by `CWD_TEST_LOCK`
  (`src/cli/cwd.rs:56-66`), but that mutex is `pub(crate)` and unreachable from `tests/`.
- **Built-in defaults decide whether that race fires:**
  - `cargo nextest run` → **one process per test** → cwd mutations cannot collide. ✅
  - `cargo test` (libtest) → one process for the whole `e2e` binary, threads at
    `RUST_TEST_THREADS` default (= #CPUs); Cargo runs *integration test binaries*
    serially (Cargo Book → Cargo Targets → Integration tests: "cargo test will run them
    serially"), so cross-binary isolation exists, but **within** the `e2e` binary the 4
    `CwdGuard` tests race. ⚠️
  - `mise watch` runs exactly that unsafe path (`mise.toml:140`).

---

## 10. Command cookbook — what works today

`<all>` below means `--all-features` (the only feature set; equals `--features test-utils`).

### Via mise (canonical)

| Goal | Command | Notes |
| --- | --- | --- |
| Everything (4 binaries + doctests) | `mise run test` / `mise t` | nextest `<all>` then `cargo test --workspace --all-features --doc -- --quiet` (`test/_default:127-134`) |
| Everything, no doctests | `mise run test --no-docs` | `test/_default:24-32` |
| Unit only (lib + bin) | `mise run test:unit --lib --bins` | forwards to nextest with `<all>` (`test/unit:51-57,111`) |
| Integration only | `mise run test --test integration` | ✅ works; `--` passthrough (`test/_default:72-74`) |
| E2E only | `mise run test --test e2e` | ✅ works, no feature needed |
| Unit only (doctests too) | `mise run test --lib --bins` | doctests still run in phase 2 |
| Default features | `mise run test --feature default` | ❌ **broken** (§4.2) — use `--feature test-utils`/`all`, or raw cargo below |
| One module | `mise run test -m index` | unit-only in practice (§4.3) |
| Doctests only | `mise run test:doc` | `test/doc:81-83` |
| Coverage | `mise run coverage` / `coverage:lcov` | `cargo llvm-cov nextest --workspace --all-features` (`mise.toml:348,405`) |
| Mutation | `mise run test:mutants [-m <mod>]` | runs `test` first; 1h timeout |

### Raw cargo / nextest

| Goal | Command | Notes |
| --- | --- | --- |
| Everything | `cargo test --all-features` | libtest: lib + bin + `integration` + `e2e` + doctests (Cargo `test` default selection) |
| Everything via nextest | `cargo nextest run --all-features && cargo test --workspace --all-features --doc` | nextest cannot run doctests |
| Unit only | `cargo nextest run --lib --bins --all-features` | |
| Integration only | `cargo nextest run --test integration --all-features` | without `--all-features`: **exit 4 `NO_TESTS_RUN`** (nextest default) vs `cargo test --test integration` → exit 0 with "running 0 tests" |
| Integration only (name) | `cargo nextest run -E 'binary("integration")'` | `binary()` = target name for integration targets |
| E2E only | `cargo nextest run --test e2e` or `-E 'binary("e2e")'` | **[verified]** these flags exist in `cargo nextest run --help` |
| Integration **kind** | `cargo nextest run -E 'kind(test)'` | ⚠️ **selects BOTH `integration` and `e2e`** — both are `tests/` targets; layer is distinguishable only by binary name, not kind |
| Default features | `cargo nextest run --no-default-features` | runs unit+bin+e2e, `integration` empty |
| Sharding (unused) | `cargo nextest run --partition hash:1/2` | supported **[verified]**, not configured anywhere |

**Does *not* work / does not do what it says:**
`cargo nextest run` (bare) — skips doctests entirely; with default features compiles an
empty `integration` target. `mise run test --feature default` — cargo error. `mise test -m <mod>`
— silently unit-only. `mise run test --test init_cli` (documented) — no such target.

---

## 11. Findings & recommendations (evidence-justified only)

**F1 — `mise watch` runs the wrong test command (two defects).**
`cargo watch -x check -x test` (`mise.toml:140`) = bare `cargo test`:
(a) default features → `tests/integration.rs:5` empties the integration suite → **0
integration tests, exit 0**;
(b) libtest thread mode → the 4 `CwdGuard` tests race on process cwd, the exact
"known, accepted limitation" the harness documents (`tests/e2e/support.rs:33-49`).
→ Fix: `cargo watch -x check -x 'nextest run --all-features'` (nextest = one process per
test kills the race *and* runs the integration suite), or `-x 'test --all-features'` +
a lock in `CwdGuard`.

**F2 — documented `--feature default` is a hard error.**
Help promises "passes no feature flags" (`test/_default:20-21`, `test/unit:19-20`,
`test/doc:10-11`); implementation forwards `--features default` (`test/unit:51-57`,
`test/doc:41-47`) and the package has no such feature (`Cargo.toml:182-183`) →
verified error. → Fix: add a `default` branch emitting no feature flags (or drop the
promise from the help text).

**F3 — plain `cargo test` is silently green with zero integration tests.**
No `[[test]]` sections exist, so nothing but the in-file `#![cfg(...)]` can gate
`tests/integration`. Since required-features would also skip silently
(`bench/_default:145-146`), the loud fix is a **command-level guard**, not Cargo: keep
`mise run test` (`--all-features`, `test/_default:15-23`) as the only documented entry,
and/or add a CI assertion that the canonical run is non-empty. A default-feature CI run
(see F5) gives `nextest`'s `--no-tests` fail-by-default behaviour for explicit selection.

**F4 — no timeouts anywhere; slow-test visibility is actively suppressed.**
`.config/nextest.toml` configures only `status-level = "fail"` and
`success-output = "never"` (`:6-11`), so nextest's built-in defaults apply: 60s
`slow-timeout` **with no termination**, no `global-timeout`, retries 0 — and the repo's
`status-level = "fail"` hides even the slow/leak markers (default would be `pass`).
A hung e2e child (`tests/e2e/support.rs:197-208`, unbounded `Command::output()`) blocks
the single CI test job until GitHub kills it.
→ Fix: in `[profile.default]` add
`slow-timeout = { period = "60s", terminate-after = 4, grace-period = "10s" }` and a
`global-timeout` for the suite; consider `status-level = "slow"` so hangs are visible.

**F5 — CI never exercises the default-feature configuration or non-Linux test runs.**
Every compile/lint/test job passes `--all-features` (`ci.yml:43,57,70`), which flips all
63 `test-utils` cfg sites — including the `cfg_attr(not(any(test, feature)),
expect(dead_code))` canaries (`src/index/inlinks.rs:67` etc.) that are only *live* in a
feature-off build; only `docs` (`mise.toml:491`, no flags) compiles that configuration.
And although `check` builds on ubuntu/macos/windows (`ci.yml:32`), **tests run only on
ubuntu** (`ci.yml:61`), leaving `src/dirs.rs:101-160` mac/windows branches and every
`#[cfg(unix)]` test site unexercised on the other two OSes.
→ Fix: add one `cargo nextest run --no-default-features` step (cheap, covers the canary
config + a default-feature e2e against the shipped binary) and one
`cargo nextest run --test e2e` step to the OS matrix.

**F6 — layered suites exist in the file tree but not in the runner.**
`tests/integration` vs `tests/e2e` are cleanly separated (and the distinction is
*binary name only* — both are `kind(test)`), yet everything runs in one nextest pool in
one job with no `test-groups`, no `--partition`, no per-suite step. e2e (process spawns,
sandbox I/O) therefore competes for the same `num-cpus` slots as ~2230 unit tests, and
an e2e failure is indistinguishable from a unit failure in CI logs.
→ Fix (low-risk, uses flags already verified): split the CI `test` job into
`cargo nextest run --test e2e --all-features` (with its own timeout from F4) and
`mise run test --no-docs` + `mise run test:doc`. Nothing else needs to change —
selection by binary name works today.

**F7 — `test:unit`/`test` naming and `-m` filtering hide what actually runs.**
`test:unit` runs four binaries including e2e (`test/unit:2` description says only "unit
and integration"), and `-m <mod>` (`test/unit:71-75`) drops integration + e2e silently
while still exiting 0. Both make "I ran the tests" unreliable for layering decisions.
→ Fix: rename/extend the descriptions, and state in `-m`'s help that it is unit-only
(or add `binary(/^(integration|e2e)$)` to the filter when a module is given).

**F8 — coverage and mutation are local-only, and coverage excludes doctests.**
No CI job runs `coverage:*` (`mise.toml:348,405`) or `test:mutants`
(`timeout = "1h"`, `mise.toml:514`); `min_msi` is commented out (`mutarust.yml:44`), so
mutation output is advisory, and `coverage:*` goes through the `llvm-cov nextest`
subcommand, which does not run doctests (the reason `test:doc` exists —
`test/_default:24-32`).
→ Fix only if these are meant to be gates: a scheduled/manual CI job for
`coverage:lcov`, and an explicit decision on `min_msi` (`mutarust.yml:37-45` defers it
"under the no-multi-hour-runs constraint"). Otherwise document that they are advisory —
which `mutarust.yml:37-44` already half-does.

**Housekeeping (stale pointers found while citing):** `.mise/tasks/test/_default:38` and
`test/unit:28` reference a non-existent `--test init_cli` target; `mise.toml:64` points at
`.cargo/mutants.toml` which does not exist (superseded by `mutarust.yml`); `.config/nextest.toml:5`
advertises `[profile.<name>]` sections that aren't in the file; `tests/e2e/support.rs:37`
references `src/cwd.rs`, which is actually `src/cli/cwd.rs`.
