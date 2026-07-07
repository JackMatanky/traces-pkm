# Trust store: check + create + untrusted rejection

Status: ready-for-agent

## Parent

`.scratch/config-service/PRD.md`

## What to build

The trust mechanism, following the mise `config/tracking.rs` pattern (see ADR 0002). A directory is trusted by creating a symlink (plain file on Windows) in `~/.local/share/traces/trusted/`, named by the SHA-256 hash of the canonical directory path, pointing back at the directory. `ConfigService` exposes `is_trusted(dir)` and `trust(dir)`. An untrusted directory check yields a miette error that includes the path and suggests running `traces trust`.

This is the programmatic trust API; the `traces trust` CLI is issue 04.

## Acceptance criteria

- [ ] `trust(dir)` creates the hashed symlink/file in the XDG data trust store (via `dirs`)
- [ ] `is_trusted(dir)` returns true only when a valid trust entry exists for the canonical path
- [ ] Untrusted rejection error includes the path and a `traces trust` suggestion (miette)
- [ ] Canonicalization ensures the same directory hashes consistently regardless of relative path
- [ ] Tests verify trust creation, positive/negative checks, and rejection error — using temp dirs and a temp trust store

## Blocked by

None - can start immediately
