---
number: 2
title: Symlink-Based Config Trust and Tracking
status: accepted
date: 2026-07-07
---

# Symlink-Based Config Trust and Tracking

## Context and Problem Statement

Templates can invoke custom Rust functions (including interactive prompts and
file includes) during rendering. This means instantiating an untrusted template
is equivalent to executing arbitrary code — the template can read files, prompt
for input, and write output. A trust mechanism is needed to prevent accidental
execution of templates from unknown or modified sources.

**Scope note, so this doesn't read as a template-specific mechanism:** the above
is *why* trust exists, not *what* it's scoped to. There is no separate
template-trust concept anywhere in this codebase, and no plan to add one. What's
actually trusted is the **workspace** — a project's root directory, the same
directory `.traces/config.toml` lives under — plus that config file's content,
via a companion hash (see the issue-04 update below). A project's template
directory is *configured by* its config file (`directory = "..."` in
`.traces/config.toml`); trusting the workspace is what authorizes any template
rendering that config points at. `ConfigTrust` (this ADR's decision) is the only
trust mechanism this codebase has or needs: config loading and template
rendering are both just consumers of the same one workspace-level trust check,
not independently-trusted subjects.

Minijinja itself provides no sandboxing for custom functions, so trust must be
managed at the directory level: a workspace is either trusted (safe to load its
config and run its templates) or untrusted (the tool warns/refuses before
either).

The trust state must persist across sessions, handle directory moves/renames
gracefully, and be trivially auditable.

Separately but relatedly, the tool benefits from *tracking* which config files
it has loaded across projects (a distinct concern from trust — see mise's
`TRACKED_CONFIGS` vs `TRUSTED_CONFIGS`). Trust answers "is this workspace safe
to load config and run templates from?"; tracking answers "which config files
has traces seen, anywhere?" Both use the same hash-keyed symlink store shape, so
this ADR covers where both live.

## Considered Options

- **Symlink-based tracking** — Trust recorded as symlinks keyed by directory
  path hash (mise pattern)
- **Config file list** — Trusted directories listed in a TOML/JSON file
  (~/.config/traces/trusted.toml)
- **Checksum-based** — Trust based on content hash of the workspace directory,
  verified on each run
- **No trust at all** — Run any template anywhere without restriction

## Decision Outcome

Use mise's symlink-based tracking pattern for **both** stores, kept in separate
directories under the XDG **state** dir (following mise, which places
`TRACKED_CONFIGS` and `TRUSTED_CONFIGS` under its state dir, not its data dir):

- **Trust store** — `~/.local/state/traces/trusted-configs/`. The `traces trust`
  command records trust by creating a symlink named by the BLAKE3 hash of the
  directory's canonical path, pointing back to the directory. `traces trust`
  (run from within or targeting a directory) creates this symlink. Config
  loading — and, by extension, any template rendering that config's settings
  drive — checks whether the workspace's project root has a corresponding
  symlink before proceeding; if not, it errors with a suggestion to run `traces
  trust`.
- **Tracking store** — `~/.local/state/traces/tracked-configs/`. Each time
  `ConfigService` loads a config file, its canonical path is recorded as a
  hashed symlink here. This is *not* on the discovery hot path (discovery is the
  upward cwd walk); the tracking store exists so cross-project operations can
  list/act on every config traces has ever loaded, from anywhere.

Both stores share the same hash-keyed-symlink shape and a common cross-platform
helper, differing only in their root directory and their meaning.

Resolve the state dir via `dirs::state_dir()` (falling back to
`dirs::data_dir()` where a platform lacks a distinct state dir), not a
hard-coded path.

### Consequences

Good, because:

- Symlinks are trivially auditable — list
  `~/.local/state/traces/trusted-configs/` (or `tracked-configs/`) to see all
  entries
- The hash-based filename survives directory moves (trust entry becomes stale,
  tool warns and suggests re-trust)
- No config file parsing needed at the trust-check hot path — just a file
  existence check
- Cleanup (`traces trust clean`) removes dangling symlinks to deleted
  directories; the same clean logic applies to the tracking store
- Trust (a security decision) and tracking (bookkeeping) are physically
  separated, so listing trusted dirs never mixes in merely-seen configs —
  matching mise's `trusted-configs` vs `tracked-configs` split
- Using the state dir keeps this machine-local, regenerable bookkeeping out of
  the data dir (reserved for user content), matching mise

Bad, because:

- Symlinks don't work on all platforms equally (Windows needs a plain file
  fallback, matching mise's approach)
- Trust is path-based, not content-based — renaming a directory invalidates
  trust
- The hash-keyed naming makes manual inspection of either store slightly opaque
  (though `traces trust list` solves this for trust)
- Two stores instead of one means the cross-platform symlink helper and clean
  logic must be shared, not duplicated

### Confirmation

The trust check is enforced in the config-loading path: before a local config
candidate is read, the tool checks for a symlink at
`~/.local/state/traces/trusted-configs/<hash>` keyed by its project root —
template rendering never checks trust on its own, it only ever runs against
config that has already loaded successfully, so gating config loading is
sufficient to gate templates too. Unit tests verify that trusted workspaces
pass, untrusted workspaces are rejected with the correct error, and `traces
trust` creates the symlink correctly. Tracking is confirmed separately: loading
a config writes a symlink into `tracked-configs/`, and the cross-project list
reflects it. Both stores are injectable (test point at a temp state dir).

## More Information

Pattern follows mise's `src/config/tracking.rs` design. mise's `Tracker`
operates over multiple hash-keyed symlink directories (`TRACKED_CONFIGS`,
`TRACKED_STUBS`, `TRUSTED_CONFIGS`) via a shared
`track_in`/`list_all_in`/`clean_in` core parameterised by the store root —
traces mirrors this with one component that serves both the trusted and tracked
stores, differing only by root. mise's `Config::get_tracked_config_files()`
shows the cross-project consumer: it lists tracked configs and loads those
outside the current hierarchy. Store roots resolve via `dirs::state_dir()`. The
`traces trust` CLI subcommand (issue 05) wraps the trust store directly: `traces
trust [PATH]` records trust for the given (or current) directory without an
interactive confirmation step, `traces trust list` lists trusted workspaces,
`traces trust clean` prunes dangling entries. The `traces tracked` CLI (list,
clean) was delivered beyond MVP scope.

The trust model evolved through issues 04 and 05: initial implementation checked
config file parent directories; review found this should anchor at the project
root (matching mise's default) with a companion BLAKE3 content hash for
re-verification (matching mise's paranoid mode). Global config is unconditionally
auto-trusted (matching mise's carve-out). A second directory-only trust store was
considered and rejected in favor of a single store with an optional companion
hash.
