# Config tracking store + `Tracked` builder stage

Status: ready-for-agent

## Parent

`.scratch/config-service/PRD.md`

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
