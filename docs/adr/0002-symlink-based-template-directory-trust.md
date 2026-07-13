---
number: 2
title: Symlink-Based Template Directory Trust
status: accepted
date: 2026-07-07
---

# Symlink-Based Template Directory Trust

## Context and Problem Statement

Templates can invoke custom Rust functions (including interactive prompts and file includes) during rendering. This means instantiating an untrusted template is equivalent to executing arbitrary code — the template can read files, prompt for input, and write output. A trust mechanism is needed to prevent accidental execution of templates from unknown or modified sources.

Minijinja itself provides no sandboxing for custom functions, so trust must be managed at the directory level: a directory is either trusted (safe to run templates from) or untrusted (the tool warns/refuses before execution).

The trust state must persist across sessions, handle directory moves/renames gracefully, and be trivially auditable.

Separately but relatedly, the tool benefits from *tracking* which config files it has loaded across projects (a distinct concern from trust — see mise's `TRACKED_CONFIGS` vs `TRUSTED_CONFIGS`). Trust answers "is this directory safe to run templates from?"; tracking answers "which config files has traces seen, anywhere?" Both use the same hash-keyed symlink store shape, so this ADR covers where both live.

## Considered Options

* **Symlink-based tracking** — Trust recorded as symlinks keyed by directory path hash (mise pattern)
* **Config file list** — Trusted directories listed in a TOML/JSON file (~/.config/traces/trusted.toml)
* **Checksum-based** — Trust based on content hash of template directory, verified on each run
* **No trust at all** — Run any template anywhere without restriction

## Decision Outcome

Use mise's symlink-based tracking pattern for **both** stores, kept in separate directories under the XDG **state** dir (following mise, which places `TRACKED_CONFIGS` and `TRUSTED_CONFIGS` under its state dir, not its data dir):

- **Trust store** — `~/.local/state/traces/trusted-configs/`. The `traces trust` command records trust by creating a symlink named by the SHA-256 hash of the directory's canonical path, pointing back to the directory. `traces trust` (run from within or targeting a directory) creates this symlink. Template instantiation checks whether the template's resolved directory has a corresponding symlink; if not, it errors via miette with a suggestion to run `traces trust`.
- **Tracking store** — `~/.local/state/traces/tracked-configs/`. Each time `ConfigService` loads a config file, its canonical path is recorded as a hashed symlink here. This is *not* on the discovery hot path (discovery is the upward cwd walk); the tracking store exists so cross-project operations can list/act on every config traces has ever loaded, from anywhere.

Both stores share the same hash-keyed-symlink shape and a common cross-platform helper, differing only in their root directory and their meaning.

Resolve the state dir via `dirs::state_dir()` (falling back to `dirs::data_dir()` where a platform lacks a distinct state dir), not a hard-coded path.

### Consequences

Good, because:
- Symlinks are trivially auditable — list `~/.local/state/traces/trusted-configs/` (or `tracked-configs/`) to see all entries
- The hash-based filename survives directory moves (trust entry becomes stale, tool warns and suggests re-trust)
- No config file parsing needed at the trust-check hot path — just a file existence check
- Cleanup (`traces trust clean`) removes dangling symlinks to deleted directories; the same clean logic applies to the tracking store
- Trust (a security decision) and tracking (bookkeeping) are physically separated, so listing trusted dirs never mixes in merely-seen configs — matching mise's `trusted-configs` vs `tracked-configs` split
- Using the state dir keeps this machine-local, regenerable bookkeeping out of the data dir (reserved for user content), matching mise

Bad, because:
- Symlinks don't work on all platforms equally (Windows needs a plain file fallback, matching mise's approach)
- Trust is path-based, not content-based — renaming a directory invalidates trust
- The hash-keyed naming makes manual inspection of either store slightly opaque (though `traces trust list` solves this for trust)
- Two stores instead of one means the cross-platform symlink helper and clean logic must be shared, not duplicated

### Confirmation

The trust check is enforced in the template instantiation path: before a template from a resolved directory is rendered, the tool checks for a symlink at `~/.local/state/traces/trusted-configs/<hash>`. Unit tests verify that trusted directories pass, untrusted directories error with the correct miette diagnostic, and `traces trust` creates the symlink correctly. Tracking is confirmed separately: loading a config writes a symlink into `tracked-configs/`, and the cross-project list reflects it. Both stores are injectable (test point at a temp state dir).

## Pros and Cons of the Options

### Symlink-based tracking (mise pattern)

* Good, because trivial to implement — file existence check on the hot path
* Good, because symlinks are self-documenting (point to the trusted directory)
* Good, because `clean()` is simply removing dangling symlinks
* Bad, because Windows requires a plain file fallback (symlinks are restricted)
* Bad, because renaming a directory breaks trust (path is the trust key)

### Config file list (TOML)

* Good, because human-readable — open the file and see all trusted dirs
* Good, because cross-platform without special-casing Windows
* Bad, because parsing overhead on every trust check (minor but real)
* Bad, because concurrent edits risk corruption (mitigated by atomic writes)
* Bad, because manual editing introduces syntax errors

### Checksum-based

* Good, because trust survives directory renames and moves
* Good, because content-based trust is more secure (modified dir automatically untrusted)
* Bad, because walking and checksumming a directory tree on every run is expensive
* Bad, because legitimate modifications (adding a template) break trust unexpectedly

### No trust at all

* Good, because zero implementation work
* Bad, because executing arbitrary templates from any location is a security risk
* Bad, because users have no way to audit which directories have been authorized

## More Information

Pattern follows mise's `src/config/tracking.rs` design. mise's `Tracker` operates over multiple hash-keyed symlink directories (`TRACKED_CONFIGS`, `TRACKED_STUBS`, `TRUSTED_CONFIGS`) via a shared `track_in`/`list_all_in`/`clean_in` core parameterised by the store root — traces mirrors this with one component that serves both the trusted and tracked stores, differing only by root. mise's `Config::get_tracked_config_files()` shows the cross-project consumer: it lists tracked configs and loads those outside the current hierarchy. Store roots resolve via `dirs::state_dir()`. The `traces trust` CLI subcommand wraps the trust store with interactive prompts (inquire) when run without arguments; tracking has no CLI of its own for MVP (it is written on config load and read by future cross-project commands).
