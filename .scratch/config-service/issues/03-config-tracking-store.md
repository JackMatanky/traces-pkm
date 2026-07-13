# Config tracking store + `Tracked` builder stage

Status: ready-for-agent

## Parent

`.scratch/config-service/spec.md`

## What to build

A **tracking store** that records which config files `ConfigService` has loaded, so cross-project operations can list every config traces has ever seen from anywhere — plus the shared hash-keyed-symlink component that both this store and the trust store (issue 04) are built on.

This is the *bookkeeping* half of the mise `config/tracking.rs` pattern (see ADR 0002), distinct from *trust*: tracking answers "which config files has traces loaded, across all projects?", trust answers "is this directory safe to run templates from?". They share a symlink-store shape but live in separate directories under the XDG **state** dir.

Tracking is **not** on the discovery hot path — discovery is the existing upward `cwd` walk from issue 01, which is unchanged. Tracking is written as a side effect of loading a config and read by future cross-project commands.

## Agent Brief

**Category:** enhancement
**Summary:** Add a config tracking store (records loaded config paths across projects) and the shared symlink-store component that trust reuses; make the existing no-op `Tracked` builder stage actually track.

**Current behavior:**
`ConfigService` discovers config via an upward walk from `cwd` and a global fallback (issue 01, implemented). The config build pipeline is a typestate builder with a `Tracked` stage, but that stage is an explicit **no-op pass-through** ("reserved for future trust validation") — nothing is recorded when a config loads. There is no record of configs loaded outside the current directory hierarchy, so no command can enumerate "every config traces has used."

**Desired behavior:**
- When `ConfigService` loads a config file, its canonicalized path is recorded in a persistent tracking store as a hash-keyed symlink (plain file on Windows) pointing back at the file. Re-tracking an already-tracked path is idempotent.
- A programmatic API lists all currently tracked config paths (resolving each symlink back to its target) and prunes stale entries whose target no longer exists.
- The tracking store lives under the XDG **state** dir (sibling to the trust store), resolved via the directories crate's state-dir accessor, never hard-coded.
- A single shared component provides the hash-keyed-symlink store behavior (record / list / clean, canonicalize-then-hash, cross-platform symlink-or-file), parameterized by store root and reused by the trust store in issue 04. Trust and tracking differ only by root directory and meaning.
- The `Tracked` builder stage stops being a no-op: transitioning through it records each candidate config path into the tracking store. Failure to write a tracking entry must not fail config loading — tracking is best-effort bookkeeping, so a store I/O error is logged/warned, not propagated as a config error.

**Key interfaces:**
- A symlink-store component (call it what fits the codebase) with a `root: PathBuf` and methods to: record a path, list all live targets, and clean stale entries. Canonicalizes before hashing (SHA-256, hex filename). Uses a Unix symlink / Windows plain-file helper gated by `cfg`. Store root is injectable so tests point at a temp dir.
- A tracking API on or beside `ConfigService` — record a loaded config, list tracked configs, clean the tracking store — thin over the shared component with the tracked-store root.
- The existing `ConfigBuilder` typestate: the `Discovered → Tracked` transition (or whichever stage owns it) now performs tracking as a side effect. Keep the public build chain shape the same; only the stage's behavior changes.
- Store-root resolution helper returning the state-dir-based tracked-configs and trusted-configs paths, with a data-dir fallback where a platform lacks a distinct state dir.

**Acceptance criteria:**
- [x] Loading a config through `ConfigService` records that config's canonical path in the tracking store
- [x] Tracking is idempotent — loading the same config twice yields exactly one entry
- [x] The tracking list API returns the canonical paths of all live tracked configs; entries whose target was deleted are omitted (or removed by the clean API)
- [x] The clean API removes dangling tracking entries and the store root resolves via the state-dir accessor (not a hard-coded path)
- [x] The `Tracked` builder stage performs tracking rather than being a no-op, and a tracking-store write failure does **not** fail config loading (best-effort: warn/log and continue)
- [x] Trust (issue 04) and tracking share one symlink-store component — the hashing/symlink/clean logic exists once, parameterized by root
- [x] Tests cover: record creates an entry, idempotent re-record, list reflects live entries, clean prunes stale entries, tracking write failure is non-fatal — all against a temp store root

All checked after the adversarial review session below — the last two (tracking-write-failure-is-non-fatal, and tracking through the full `ConfigService` pipeline end-to-end) were implemented but untested until that review; see "Review session follow-up" in Comments.

**Out of scope:**
- The **trust** semantics (`is_trusted`/`trust`/untrusted rejection) — issue 04, which consumes the shared component built here.
- Any `traces` CLI surface for tracking — there is no `traces track` command for MVP; tracking is written on load and read by future cross-project commands.
- Using tracked paths as a discovery cache to skip the upward walk — discovery is unchanged; the tracking store is a cross-project registry, not a discovery accelerator.
- Removing or renaming the `Trusted` builder stage — leave the trust stage placeholder for issue 04.
- Moving the trust store to the state dir — that path change is applied in issue 04 (ADR 0002 already reflects it).

## Rust guidance

Relevant skills: `m11-ecosystem`, `m01-ownership`, `m06-error-handling`, `m05-type-driven`, `m12-lifecycle`.

- **Shared component, parameterized by root (m05/m11):** keep one implementation taking the store root, not two copies. Trust (issue 04) can call it with the trusted-configs root if its requirements fit; tracking calls it with the tracked-configs root. This is the seam that keeps hashing/symlink/clean defined once without copying mise's extra `TRACKED_STUBS` split.
- **Canonicalize then hash (m01):** `std::fs::canonicalize` before `sha2::Sha256`, hex-encode for the filename — identical rule to trust, which is why it belongs in the shared component. A canonicalization mismatch silently splits entries.
- **Cross-platform (m11):** Unix `std::os::unix::fs::symlink`, Windows plain file via `#[cfg(windows)]`; list resolves symlinks (Unix) or reads the file's stored path (Windows). Keep the public API identical across platforms.
- **State dir (m11):** resolve via the directories crate's state-dir accessor, with a data-dir fallback where absent. Make the root injectable (field/param) so tests use a temp dir — same hook style as issue 01's `with_global_config_path`.
- **Best-effort tracking (m06/m12):** tracking is a side effect of loading, not a precondition. A store write error is `warn!`-and-continue, never a `ConfigService` error variant. Reserve error propagation for the config parse/merge path; do not let bookkeeping I/O break config loading. The `Tracked` stage does this work as the config candidates pass through it during build.

## Blocked by

None — can start immediately. (Unblocks issue 04, which reuses the shared symlink-store component.)

## Comments

Implemented on branch `feat/config-tracking-store` (worktree `.worktrees/config-tracking-store`).

**File layout:**
- `src/config/store.rs` — `ConfigFileStore`, the hash-keyed symlink/file store behavior and OS-specific storage details.
- `src/config/tracker.rs` — `ConfigTracker`, the tracked-config adapter over `paths::TRACKED_CONFIGS`.
- `src/config/dirs.rs` — `CONFIG_HOME`, `TRACKED_CONFIGS` / `TRUSTED_CONFIGS` store-root constants and platform path resolution.
- `src/config/builder.rs` — `Discovered → Tracked` transition now calls `ConfigTracker::track` per candidate.

**Deviations from this issue's "Key interfaces" section** (design decisions made during implementation review, after comparing against mise's actual `src/config/tracking.rs`, `src/dirs.rs`, and `src/config/config_file/mod.rs` rather than the pattern as originally described):

- **Shared store mechanism, tracking-specific adapter.** The issue asks for one component "reused by the trust store in issue 04." `ConfigFileStore` (`store.rs`) owns the shared hash-keyed symlink/file mechanics and takes an explicit store root. `ConfigTracker` (`tracker.rs`) is the tracked-config adapter over `dirs::TRACKED_CONFIGS`. Issue 04 can call `ConfigFileStore` with `dirs::TRUSTED_CONFIGS` *if* trust's actual requirements fit the same shape. That's still an issue-04 decision, since mise's real trust code has extra concerns (`.hash` content-verification files, `.monorepo` markers, an in-memory trust cache).
- **No tracking API on `ConfigService`.** *(Superseded — see "Review session follow-up" below. `ConfigService::list_tracked`/`clean_tracked_store` were added during review, reopening the question this bullet answers. Also: `ConfigTracker::list_all`/`clean` were `pub(super)` at the time this was written, not `pub(crate)` as stated here — a self-report error caught during review.)*
- **Store root is a preset constant at the production call site, not an injected field.** *(Superseded — see "Review session follow-up" below. The root is now injectable, which is what made the two gaps below fixable.)*
- **`StoreError` has 2 variants (`Canonicalize`, `Io`), not one per failing operation.** Matches this crate's own precedent for lean error enums (`ConfigBuilderError`: 1 variant, `DiscoveryError`: 2) rather than a variant per `std::fs` call site.
- **Logging uses the `tracing` crate** (added as a dependency) — `tracing::warn!` with structured fields (`path`, `error`), not `eprintln!`. No subscriber is installed in library code (nothing in `main.rs` yet to observe it); the library only emits through the facade.

**Testing:** 9 tests in `store::tests` exercise `record`/`list_all`/`clean` directly against explicit temp directories (never `paths::TRACKED_CONFIGS`) — entry creation, idempotent re-recording, list omitting stale entries, clean pruning stale entries and reporting the count, a canonicalize failure on a missing target, and an I/O failure when the store root is occupied by a file. `paths::tests` has one test confirming `TRACKED_CONFIGS`/`TRUSTED_CONFIGS` are distinct siblings under the same parent. `track()`'s non-`Result` signature on `ConfigBuilder` is itself the guarantee that a tracking failure can't fail config loading — there's no error variant to propagate.

**For issue 04:** `dirs::TRUSTED_CONFIGS` already exists (currently `#[allow(dead_code)]`, unconsumed). `ConfigFileStore` can be called with that root if trust's actual requirements fit the same canonicalize-then-hash-then-symlink shape — but don't assume they will; check mise's real trust implementation (`config_file/mod.rs`'s `trust`/`is_trusted`/`trust_path`/`hashed_path_filename`) first, since it diverges from `Tracker` in naming scheme and has concerns (content-hash verification, monorepo markers, in-memory cache) tracking has no reason to carry. The `Trusted` builder stage remains the pre-existing no-op placeholder, unchanged.

**Implementation commits on `feat/config-tracking-store`:**
- `f9c79ee feat(config): add config tracking store` — initial tracking-store implementation.
- `84311e8 refactor(config): simplify tracking store API` — narrowed the store API to static root-parameterized calls instead of storing a root field.
- `56a8ec2 refactor(config): extract config file store` — split the reusable hash-keyed store mechanics into `ConfigFileStore`.
- `1aca687 docs(config): clarify config pipeline comments` — updated config pipeline comments after tracking/trust separation.
- `c335aa4 refactor(config): minimize config visibility` — made config internals private where possible.
- `9ef6a7e test(config): narrow test lint allowances` — first test cleanup pass.
- `3e364bd test(config): simplify config test assertions` — final test cleanup pass using `pretty_assertions` and normal test-local `expect(...)` setup failures.
- `0ce389e refactor(config): make tracking store root injectable and type-safe` — see "Review session follow-up" below.
- `3c7712b refactor(config): replace dirs crate with own path resolution` — see "Path resolution refactor" below.

**Final test style cleanup:** Added `pretty_assertions` as a dev-dependency and imported it in config test modules that compare values. Removed the earlier function-level `#[allow(clippy::panic_in_result_fn)]` annotations, the custom `must(...)` test helper, explicit `panic!` calls, and `std::process::abort()` branches. The final tests use ordinary `#[cfg(test)] mod tests`, `use super::*`, descriptive test names, normal `assert!` / `assert_eq!`, `matches!` for error variants, and test-local `expect(...)` for fallible fixture setup. This matches the existing `clippy.toml` policy (`allow-unwrap-in-tests = true`, `allow-expect-in-tests = true`) while keeping production lint strictness unchanged.

**Verification after cleanup:** `cargo clippy --workspace --all-targets -- -D warnings` passed, `cargo test --workspace` passed with 64 tests, and `cargo doc --workspace --no-deps` passed. A targeted scan of `src/config` found no `fn must`, explicit `panic!`, `std::process::abort`, `panic_in_result_fn`, or `#![allow(...)]` test workarounds. GitNexus change detection reported low risk with no affected execution flows.

**Review session follow-up (`0ce389e refactor(config): make tracking store root injectable and type-safe`):**

An adversarial review of the implementation above found the two "Superseded" deviations were hiding a real gap: because the tracking store root was a process-global `LazyLock<PathBuf>` static with no injection point, no test could force a write failure through the actual `Tracked` builder stage and observe the "does not fail config loading" behavior end-to-end. `store::tests` covered `ConfigFileStore::record` failing in isolation; nothing covered the swallow-and-continue *policy* that acceptance criterion 5 and the "Tests cover" bullet both require. `paths::state_root()`'s `#[cfg(test)]` redirect to a single shared OS-temp-dir location also meant every test exercising `.track()` wrote real, permanently-dangling symlinks into that shared directory across every `cargo test` run (confirmed empirically: 240 → 248 entries per run, 100% dangling) — a real, if minor, test-hygiene bug, not just a theoretical one.

Changes:
- **`ConfigFileStore` now holds `root: PathBuf` as instance state** instead of taking it as a parameter on every call (`record`/`list_all`/`clean` are now `&self` methods).
- **`ConfigFileStore::new` accepts only `dirs::StateDirRoot`**, a newtype whose constructor is private to `dirs.rs`. The only two values that can ever exist are `dirs::TRACKED_CONFIGS` and `dirs::TRUSTED_CONFIGS` — a production caller cannot construct a store pointed at an arbitrary or typo'd directory (a raw `&str`/`PathBuf` parameter was considered and rejected during review specifically because a typo'd directory name would compile silently and `clean()` would then delete files in the wrong place).
- **`ConfigFileStore::at(root: PathBuf)` / `ConfigTracker::at(root: PathBuf)`** (`#[cfg(test)]`-only) inject an arbitrary root, giving each test its own isolated `tempfile::tempdir()` instead of sharing the global test-state directory. This is the fix for the dangling-symlink accumulation above — verified empirically post-fix (248 → 248 across a full test run, zero new writes to the shared location).
- **Tracing for the best-effort policy moved from `ConfigBuilder::track()` into `ConfigTracker::track`**, which now has a non-`Result` signature. The swallow-and-warn behavior is owned by the tracking adapter (the type that already owns "what does a store failure mean for tracking"), not the builder's pipeline-orchestration stage.
- **`ConfigService::list_tracked`/`clean_tracked_store` added**, giving `ConfigTracker::list_all`/`clean` a real caller (removing their `#[allow(dead_code)]`) and a way for tests to observe tracking/idempotence/cleaning through the actual `ConfigService` pipeline rather than only at the `ConfigFileStore` unit level.
- **New tests:** `tracker.rs` — swallow-on-write-failure doesn't panic, a valid target round-trips. `service.rs` — record-through-the-pipeline + idempotence through repeated `build()` calls, `clean_tracked_store` pruning a deleted config's entry, and `build()` succeeding despite a broken tracking-store root (the direct test for AC5). Total: 69 tests (up from 64), all against isolated temp roots.

**Considered and rejected during review:** giving `ConfigFileStore` itself `track()`/`is_trusted()`-style domain-specific methods instead of separate adapter types. Rejected because tracking is explicitly best-effort (swallow errors) while trust will almost certainly need to propagate them (a security-relevant gate isn't bookkeeping), and because mise's real trust implementation carries concerns (`.hash` content-verification files, monorepo markers, an in-memory cache) tracking has no reason to share — putting both domains' methods on one type would mean every `ConfigFileStore` instance exposes methods that are meaningless for its own root (e.g. `.is_trusted()` callable, and silently wrong, on the tracked-configs instance). The "no tracking API on `ConfigService`" question from the original implementation was also reopened and re-decided the other way, specifically to get the pipeline-level test coverage above — whether tracking-store administration belongs on `ConfigService` long-term (vs. a narrower accessor, once issue 04 needs the same thing for trust) is intentionally left open for a later holistic pass across both stores' public surfaces.

**Verification after follow-up:** `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` (69 tests), `cargo doc --workspace --no-deps`, and `cargo fmt --check` all pass clean. GitNexus change detection reported low risk (its symbol index doesn't resolve this module in this repo — verification relied on grep-based caller tracing and the cargo commands above instead).

**Path resolution refactor (`3c7712b refactor(config): replace dirs crate with own path resolution`):**

The `paths.rs` module was replaced by `dirs.rs` with self-contained platform path resolution, removing the `dirs` crate dependency. Motivations: the external `dirs` crate resolved `state_dir()` and `data_dir()` but our only need was `CONFIG_HOME` (global config parent) and a state parent dir — each was a one-liner with stdlib and `#[cfg]`. The module name `paths` was chosen in issue 03 to avoid shadowing the then-existent external `dirs` crate; after removing that crate the shadow concern disappears, so the module was renamed to match what it actually holds: directory constants.

Changes:
- **`paths.rs` → `dirs.rs`**, all imports updated across `mod.rs`, `store.rs`, `tracker.rs`, `discovery.rs`.
- **Removed `dirs` crate from `Cargo.toml`** and its transitive tree from `Cargo.lock` (`dirs-sys`, `option-ext`, `libredox`, `redox_users`, `getrandom 0.2`).
- **Platform-specific `#[cfg]` blocks** for three targets:
  - `unix` (excluding macOS) — `$XDG_CONFIG_HOME` / `$HOME/.config`, `$XDG_STATE_HOME` / `$HOME/.local/state`.
  - `macos` — `$XDG_CONFIG_HOME` or `~/Library/Application Support`; `$XDG_STATE_HOME` or `~/Library/Application Support` (XDG first, then macOS native).
  - `windows` — `%APPDATA%`, `%LOCALAPPDATA%`, `%USERPROFILE%` via `non_empty_var` helper.
- **No separate `linux` block** — covered by the general unix block, matching issue 03's cross-platform concern without Linux-specific fns like the `dirs` crate's.
- **Mise-style `HOME` pattern**: `#[cfg(test)]` points to `CARGO_MANIFEST_DIR/test`; `#[cfg(not(test))]` reads env vars. All other statics (`CONFIG_HOME`, `STATE_HOME`, `TRACES_STATE_DIR`) compile unconditionally — no dead code. Follows mise's `env.rs` convention where only `HOME` has test-mode overrides.
- **`CONFIG_HOME` renamed from `XDG_CONFIG_HOME`** — matches its actual role (a cross-platform config parent dir, not exclusively XDG).
- **`discovery.rs`** updated to use `dirs::CONFIG_HOME.join(GLOBAL_CONFIG_FILE)` instead of the external `dirs::config_dir()` call.
- **`#[allow(dead_code)]` removed** from everything except `TRUSTED_CONFIGS` (reserved for issue 04). `non_empty_var` and `var_path` helpers are always reachable since all statics compile in both `#[cfg(test)]` and `#[cfg(not(test))]` modes.
- **`APP_NAME` constant** (`"traces"`) used by `TRACES_STATE_DIR = STATE_HOME.join(APP_NAME)`. No longer dead code — same reasoning.
- **`OsString` import** cleaned up: no more `#[allow(unused_imports)]` since `non_empty_var` always references it.

**Verification:** `cargo clippy --workspace --all-targets -- -D warnings` passed (zero allows), `cargo test --workspace` passed (69 tests, unchanged count).
