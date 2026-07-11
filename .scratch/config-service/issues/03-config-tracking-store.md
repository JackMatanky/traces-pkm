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
- [ ] Loading a config through `ConfigService` records that config's canonical path in the tracking store
- [ ] Tracking is idempotent — loading the same config twice yields exactly one entry
- [ ] The tracking list API returns the canonical paths of all live tracked configs; entries whose target was deleted are omitted (or removed by the clean API)
- [ ] The clean API removes dangling tracking entries and the store root resolves via the state-dir accessor (not a hard-coded path)
- [ ] The `Tracked` builder stage performs tracking rather than being a no-op, and a tracking-store write failure does **not** fail config loading (best-effort: warn/log and continue)
- [ ] Trust (issue 04) and tracking share one symlink-store component — the hashing/symlink/clean logic exists once, parameterized by root
- [ ] Tests cover: record creates an entry, idempotent re-record, list reflects live entries, clean prunes stale entries, tracking write failure is non-fatal — all against a temp store root

**Out of scope:**
- The **trust** semantics (`is_trusted`/`trust`/untrusted rejection) — issue 04, which consumes the shared component built here.
- Any `traces` CLI surface for tracking — there is no `traces track` command for MVP; tracking is written on load and read by future cross-project commands.
- Using tracked paths as a discovery cache to skip the upward walk — discovery is unchanged; the tracking store is a cross-project registry, not a discovery accelerator.
- Removing or renaming the `Trusted` builder stage — leave the trust stage placeholder for issue 04.
- Moving the trust store to the state dir — that path change is applied in issue 04 (ADR 0002 already reflects it).

## Rust guidance

Relevant skills: `m11-ecosystem`, `m01-ownership`, `m06-error-handling`, `m05-type-driven`, `m12-lifecycle`.

- **Shared component, parameterized by root (m05/m11):** mirror mise's `track_in`/`list_all_in`/`clean_in` — one implementation taking the store root, not two copies. Trust (issue 04) constructs it with the trusted-configs root, tracking with the tracked-configs root. This is the seam that keeps hashing/symlink/clean defined once.
- **Canonicalize then hash (m01):** `std::fs::canonicalize` before `sha2::Sha256`, hex-encode for the filename — identical rule to trust, which is why it belongs in the shared component. A canonicalization mismatch silently splits entries.
- **Cross-platform (m11):** Unix `std::os::unix::fs::symlink`, Windows plain file via `#[cfg(windows)]`; list resolves symlinks (Unix) or reads the file's stored path (Windows). Keep the public API identical across platforms.
- **State dir (m11):** resolve via the directories crate's state-dir accessor, with a data-dir fallback where absent. Make the root injectable (field/param) so tests use a temp dir — same hook style as issue 01's `with_global_config_path`.
- **Best-effort tracking (m06/m12):** tracking is a side effect of loading, not a precondition. A store write error is `warn!`-and-continue, never a `ConfigService` error variant. Reserve error propagation for the config parse/merge path; do not let bookkeeping I/O break config loading. The `Tracked` stage does this work as the config candidates pass through it during build.

## Blocked by

None — can start immediately. (Unblocks issue 04, which reuses the shared symlink-store component.)

## Comments

Implemented on branch `feat/config-tracking-store` (worktree `.worktrees/config-tracking-store`).

**File layout:**
- `src/config/tracker.rs` — `ConfigTracker`, the tracking store behavior.
- `src/config/paths.rs` — `TRACKED_CONFIGS` / `TRUSTED_CONFIGS` store-root constants and their state-dir resolution.
- `src/config/builder.rs` — `Discovered → Tracked` transition now calls `ConfigTracker::track` per candidate.

**Deviations from this issue's "Key interfaces" section** (design decisions made during implementation review, after comparing against mise's actual `src/config/tracking.rs`, `src/dirs.rs`, and `src/config/config_file/mod.rs` rather than the pattern as originally described):

- **No generic shared "symlink-store component."** The issue asks for one component "reused by the trust store in issue 04." mise's own trust code does *not* reuse its `Tracker` — trust has its own hashing scheme (human-readable filenames, not a bare hash) and extra concerns (`.hash` content-verification files, `.monorepo` markers, an in-memory trust cache) that a generic component wouldn't anticipate. Building shared infrastructure for a consumer whose real requirements aren't known yet is premature. Instead: `ConfigTracker` (`tracker.rs`) is a namespace struct (a ZST, mirrors mise's `Tracker {}`) with `track`/`list_all`/`clean` public methods hard-wired to `paths::TRACKED_CONFIGS`, and private `track_in`/`list_all_in`/`clean_in` methods taking an explicit directory — this is the seam issue 04 can call with `paths::TRUSTED_CONFIGS` *if* trust's actual requirements turn out to fit the same shape. That's an issue-04 decision, not one this issue should force.
- **No tracking API on `ConfigService`.** The issue asks for "a tracking API on or beside `ConfigService`." Built and then removed: `track_config`/`tracked_configs`/`clean_tracked_configs` had zero callers anywhere in the crate (not even tests) once the automatic write path (`ConfigBuilder::track()`) existed. mise's `Config` doesn't expose `Tracker::track`/`clean` either — only a list-style consumer (`get_tracked_config_files`), which exists because there's a genuine feature consuming it (loading configs from outside the discovery hierarchy). `ConfigTracker::list_all`/`clean` are `pub(crate)` and ready to be wired up wherever that consumer eventually lands — likely still not `ConfigService`, whose stated job is discover/build, not store administration.
- **Store root is a preset constant, not an injected field.** `paths::TRACKED_CONFIGS`/`TRUSTED_CONFIGS` are `LazyLock<PathBuf>` statics (matching mise's `dirs.rs` `TRACKED_CONFIGS`/`TRUSTED_CONFIGS` shape exactly), referenced directly by `ConfigTracker`'s public methods — not threaded through `ConfigBuilder`/`ConfigService` as a field or constructor parameter. Test isolation (never touching the real `~/.local/state/traces/`) is handled by `paths::state_root()` redirecting to the OS temp dir under `#[cfg(test)]`, mirroring mise's own `#[cfg(test)] HOME` redirection in `src/env.rs`, rather than dependency-injecting the root through the call chain.
- **`TrackerError` has 2 variants (`Canonicalize`, `Io`), not one per failing operation.** Matches this crate's own precedent for lean error enums (`ConfigBuilderError`: 1 variant, `DiscoveryError`: 2) rather than a variant per `std::fs` call site.
- **Logging uses the `tracing` crate** (added as a dependency) — `tracing::warn!` with structured fields (`path`, `error`), not `eprintln!`. No subscriber is installed in library code (nothing in `main.rs` yet to observe it); the library only emits through the facade.

**Testing:** 14 tests in `tracker::tests` exercise `track_in`/`list_all_in`/`clean_in` directly against an explicit temp directory (never `paths::TRACKED_CONFIGS`) — entry creation, idempotent re-tracking, list omitting stale entries, clean pruning stale entries and reporting the count, and a canonicalize failure on a missing target. `paths::tests` has one test confirming `TRACKED_CONFIGS`/`TRUSTED_CONFIGS` are distinct siblings under the same parent. `track()`'s non-`Result` signature on `ConfigBuilder` is itself the guarantee that a tracking failure can't fail config loading — there's no error variant to propagate.

**For issue 04:** `paths::TRUSTED_CONFIGS` already exists (currently `#[allow(dead_code)]`, unconsumed). `ConfigTracker`'s private `track_in`/`list_all_in`/`clean_in` are available to call with that root if trust's actual requirements fit the same canonicalize-then-hash-then-symlink shape — but don't assume they will; check mise's real trust implementation (`config_file/mod.rs`'s `trust`/`is_trusted`/`trust_path`/`hashed_path_filename`) first, since it diverges from `Tracker` in naming scheme and has concerns (content-hash verification, monorepo markers, in-memory cache) tracking has no reason to carry. The `Trusted` builder stage remains the pre-existing no-op placeholder, unchanged.
