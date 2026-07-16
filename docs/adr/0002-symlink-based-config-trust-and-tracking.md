---
number: 2
title: Symlink-Based Config Trust and Tracking
status: accepted
date: 2026-07-07
---

# Symlink-Based Config Trust and Tracking

## Context and Problem Statement

Templates can invoke custom Rust functions (including interactive prompts and file includes) during rendering. This means instantiating an untrusted template is equivalent to executing arbitrary code — the template can read files, prompt for input, and write output. A trust mechanism is needed to prevent accidental execution of templates from unknown or modified sources.

**Scope note, so this doesn't read as a template-specific mechanism:** the above is *why* trust exists, not *what* it's scoped to. There is no separate template-trust concept anywhere in this codebase, and no plan to add one. What's actually trusted is the **workspace** — a project's root directory, the same directory `.traces/config.toml` lives under — plus that config file's content, via a companion hash (see the issue-04 update below). A project's template directory is *configured by* its config file (`directory = "..."` in `.traces/config.toml`); trusting the workspace is what authorizes any template rendering that config points at. `ConfigTrust` (this ADR's decision) is the only trust mechanism this codebase has or needs: config loading and template rendering are both just consumers of the same one workspace-level trust check, not independently-trusted subjects.

Minijinja itself provides no sandboxing for custom functions, so trust must be managed at the directory level: a workspace is either trusted (safe to load its config and run its templates) or untrusted (the tool warns/refuses before either).

The trust state must persist across sessions, handle directory moves/renames gracefully, and be trivially auditable.

Separately but relatedly, the tool benefits from *tracking* which config files it has loaded across projects (a distinct concern from trust — see mise's `TRACKED_CONFIGS` vs `TRUSTED_CONFIGS`). Trust answers "is this workspace safe to load config and run templates from?"; tracking answers "which config files has traces seen, anywhere?" Both use the same hash-keyed symlink store shape, so this ADR covers where both live.

## Considered Options

* **Symlink-based tracking** — Trust recorded as symlinks keyed by directory path hash (mise pattern)
* **Config file list** — Trusted directories listed in a TOML/JSON file (~/.config/traces/trusted.toml)
* **Checksum-based** — Trust based on content hash of the workspace directory, verified on each run
* **No trust at all** — Run any template anywhere without restriction

## Decision Outcome

Use mise's symlink-based tracking pattern for **both** stores, kept in separate directories under the XDG **state** dir (following mise, which places `TRACKED_CONFIGS` and `TRUSTED_CONFIGS` under its state dir, not its data dir):

- **Trust store** — `~/.local/state/traces/trusted-configs/`. The `traces trust` command records trust by creating a symlink named by the SHA-256 hash of the directory's canonical path, pointing back to the directory. `traces trust` (run from within or targeting a directory) creates this symlink. Config loading — and, by extension, any template rendering that config's settings drive — checks whether the workspace's project root has a corresponding symlink before proceeding; if not, it errors with a suggestion to run `traces trust`.
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

The trust check is enforced in the config-loading path: before a local config candidate is read, the tool checks for a symlink at `~/.local/state/traces/trusted-configs/<hash>` keyed by its project root — template rendering never checks trust on its own, it only ever runs against config that has already loaded successfully, so gating config loading is sufficient to gate templates too. Unit tests verify that trusted workspaces pass, untrusted workspaces are rejected with the correct error, and `traces trust` creates the symlink correctly. Tracking is confirmed separately: loading a config writes a symlink into `tracked-configs/`, and the cross-project list reflects it. Both stores are injectable (test point at a temp state dir).

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

Pattern follows mise's `src/config/tracking.rs` design. mise's `Tracker` operates over multiple hash-keyed symlink directories (`TRACKED_CONFIGS`, `TRACKED_STUBS`, `TRUSTED_CONFIGS`) via a shared `track_in`/`list_all_in`/`clean_in` core parameterised by the store root — traces mirrors this with one component that serves both the trusted and tracked stores, differing only by root. mise's `Config::get_tracked_config_files()` shows the cross-project consumer: it lists tracked configs and loads those outside the current hierarchy. Store roots resolve via `dirs::state_dir()`. The `traces trust` CLI subcommand (issue 05) wraps the trust store directly: `traces trust [PATH]` records trust for the given (or current) directory without an interactive confirmation step, `traces trust list` lists trusted workspaces, `traces trust clean` prunes dangling entries. Tracking has no CLI of its own for MVP (it is written on config load and read by future cross-project commands).

## Update (issue 04): trust anchor and re-verification, informed by mise and direnv

Config-loading trust (an issue-04 use of this ADR's store, beyond the original template-directory framing above) was initially implemented checking each candidate config file's parent directory. Review found two problems: for a local project this parent is `.traces/`, an incidental discovery detail rather than the project a user would recognize as "the thing I'm trusting"; for the global config it's the entire, single, shared `~/.config/traces/` folder, so trusting it once over-grants to everything ever placed there.

Researching mise's actual implementation (`config_file/mod.rs`) and direnv's `.envrc` trust resolved this:

- **mise's default (non-paranoid) trust is directory-level, anchored at the project root** — `config_root()` collapses every config file a project might have (`.mise.toml`, `.mise/conf.d/*.toml`, task files) to one trust decision at the root, not at whichever file happens to be read. mise's **paranoid mode** adds file-content-hash re-verification on top of that same root-anchored entry, for callers wanting stronger guarantees.
- **mise never hash-gates its own global config** — `~/.config/mise/config.toml` is unconditionally auto-trusted, on the reasoning that only the user can write to their own `$HOME`.
- **direnv trusts at file granularity with a combined path+content hash** (`sha256(abs_path + file_content)`), because its unit of trust is a single standalone script, not a project with multiple related files.

**Decision:** local config trust is anchored at the project root (`candidate.root()`, matching mise's default), with a companion BLAKE3 content hash of the config file itself for re-verification on edit (matching mise's paranoid mode, layered onto the same root-anchored entry rather than a second directory-trust decision). Global config trust is skipped entirely — always considered trusted, matching mise's own carve-out — which is what actually resolves the over-granting problem, not a switch to file-level hashing.

Content-hash re-verification only has somewhere sensible to attach for a single named file (the config file) — hashing a whole directory's content was already rejected above as the "Checksum-based" option, for the same cost-and-fragility reasons. A workspace with no config file yet is trusted at the root only, without a companion hash; see the issue-05 update below for why that's the correct behavior for that case, not a gap owed to future work.

A pure path-hash entry, once created, never expires — an edit to an already-trusted config file was previously accepted forever. The content-hash companion closes that gap for local config specifically: a mismatch between the file's current hash and the one recorded at trust time is surfaced as a distinct "stale" result, not silently treated as still-trusted.

## Update (issue 05 triage): one trust store, not two

> *This was generated by AI during triage.*

An earlier draft of the "Update (issue 04)" section above left "template-directory trust... still owed to issue 05" as an explicitly open, unresolved thread — implying a second, simpler directory-only trust concept alongside `ConfigTrust`. Re-triaging issue 05 resolved this in favor of **a single store**, not two (that draft language has since been corrected above, not just superseded here).

`traces trust <path>` (the CLI command this thread was deferred to) derives `config_file = <path>/.traces/config.toml` and calls the same `ConfigTrust::trust` issue 04 built. The only change needed: `ConfigTrust::trust` now tolerates a workspace with no `config_file` yet — trusting the root only, skipping the content-hash companion, rather than erroring. This covers "trust a workspace before it has a config file" (before `traces init` has run) without a second store, a second CLI code path, or a second mental model for what "trusted" means.

Considered and rejected: a second, directory-only trust store matching an earlier template-directory framing literally. Rejected because it would require two physically separate stores answering what a user experiences as the same question ("is this workspace trusted?"), doubling the clean/list surface for no behavioral gain — the companion-optional `ConfigTrust` already covers both the config-loaded case (companion present, re-verified on edit) and the pre-config case (no companion, root-only) with one code path.

Consequence: a root trusted before a config file existed is `Stale` (not `Trusted`) once a config file later appears there, until the user re-runs `traces trust` — the existing "missing companion fails toward re-verification" behavior, extended to a case it wasn't originally designed for but turns out to cover correctly without modification.

`ConfigTrust::trust` takes a `TrustTarget` (`Directory(root)` or `ConfigFile { root, config_file }`) rather than two positional paths, so callers state which of the two cases above applies instead of `trust` inferring it from whether `config_file` happens to exist on disk at call time.
