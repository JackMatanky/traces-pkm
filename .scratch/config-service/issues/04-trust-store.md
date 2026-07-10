# Trust store: check + create + untrusted rejection

Status: ready-for-agent

## Parent

`.scratch/config-service/PRD.md`

## What to build

The trust mechanism, following the mise `config/tracking.rs` pattern (see ADR 0002). A directory is trusted by creating a symlink (plain file on Windows) in the **trusted** store, named by the SHA-256 hash of the canonical directory path, pointing back at the directory. `ConfigService` exposes `is_trusted(dir)` and `trust(dir)`. An untrusted directory check yields a miette error that includes the path and suggests running `traces trust`.

The trusted store lives at `~/.local/state/traces/trusted-configs/` (XDG **state** dir via `dirs::state_dir()`), a sibling of the tracking store from issue 03. **This is a change from the earlier DATA-dir location** — both stores now live under the state dir, matching mise's `TRUSTED_CONFIGS`/`TRACKED_CONFIGS` split. Reuse the shared hash-keyed-symlink component built in issue 03 (the trusted and tracked stores differ only by root directory and meaning); do not duplicate the symlink/hash/clean logic.

This is the programmatic trust API; the `traces trust` CLI is issue 05.

## Acceptance criteria

- [ ] `trust(dir)` creates the hashed symlink/file in the XDG **state** trusted store (via `dirs::state_dir()`)
- [ ] `is_trusted(dir)` returns true only when a valid trust entry exists for the canonical path
- [ ] Untrusted rejection error includes the path and a `traces trust` suggestion (miette)
- [ ] Canonicalization ensures the same directory hashes consistently regardless of relative path
- [ ] Trust logic reuses the shared symlink-store component from issue 03 rather than reimplementing hashing/symlink/clean
- [ ] Tests verify trust creation, positive/negative checks, and rejection error — using temp dirs and a temp trusted store

## Rust guidance

Relevant skills: `m11-ecosystem`, `m06-error-handling`, `m13-domain-error`, `m01-ownership`.

- **Canonicalization first (m01):** hash the **canonicalized** path (`std::fs::canonicalize`), not the raw input, so `./x`, `x/`, and symlinked aliases all map to the same trust entry. Canonicalize before hashing in both `trust()` and `is_trusted()` — a mismatch here silently breaks trust checks. (This is the same canonicalize-then-hash rule as the tracking store; it lives in the shared component.)
- **Hashing (m11):** use `sha2::Sha256` over the canonical path bytes, hex-encode for the filename. Follow ADR 0002's symlink scheme — **note ADR 0002 is still `proposed`; confirm it is accepted before this lands.**
- **Cross-platform (m11):** symlink on Unix (`std::os::unix::fs::symlink`), plain file on Windows (`#[cfg(windows)]`). Gate with `cfg`, keep the public API identical. This lives in the shared component (issue 03), not duplicated here.
- **Store path (m11):** resolve the state dir via `dirs::state_dir()` (fall back to `dirs::data_dir()` where a platform lacks a distinct state dir), not a hard-coded path. Make the store root injectable (parameter or field) so tests point at a temp dir instead of the real user store.
- **Rejection error (m13):** the untrusted-directory error is **user-facing and actionable** — a `thiserror` variant rendered through miette with a `help` note suggesting `traces trust`. Include the offending path. Distinguish "not trusted" (expected, actionable) from "trust store I/O failed" (internal).

## Blocked by

- `.scratch/config-service/issues/03-config-tracking-store.md` (shared symlink-store component)
