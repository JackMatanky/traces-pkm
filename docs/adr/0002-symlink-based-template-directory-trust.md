---
number: 2
title: Symlink-Based Template Directory Trust
status: proposed
date: 2026-07-07
---

# Symlink-Based Template Directory Trust

## Context and Problem Statement

Templates can invoke custom Rust functions (including interactive prompts and file includes) during rendering. This means instantiating an untrusted template is equivalent to executing arbitrary code — the template can read files, prompt for input, and write output. A trust mechanism is needed to prevent accidental execution of templates from unknown or modified sources.

Minijinja itself provides no sandboxing for custom functions, so trust must be managed at the directory level: a directory is either trusted (safe to run templates from) or untrusted (the tool warns/refuses before execution).

The trust state must persist across sessions, handle directory moves/renames gracefully, and be trivially auditable.

## Considered Options

* **Symlink-based tracking** — Trust recorded as symlinks keyed by directory path hash (mise pattern)
* **Config file list** — Trusted directories listed in a TOML/JSON file (~/.config/traces/trusted.toml)
* **Checksum-based** — Trust based on content hash of template directory, verified on each run
* **No trust at all** — Run any template anywhere without restriction

## Decision Outcome

Use mise's symlink-based tracking pattern. The `traces trust` command records trust by creating a symlink in `~/.local/share/traces/trusted/` named by the SHA-256 hash of the directory's canonical path, pointing back to the directory. The `traces trust` command (run from within or targeting a directory) creates this symlink. `traces run` checks whether the template's resolved directory has a corresponding symlink; if not, it errors via miette with a suggestion to run `traces trust`.

### Consequences

Good, because:
- Symlinks are trivially auditable — list `~/.local/share/traces/trusted/` to see all trusted dirs
- The hash-based filename survives directory moves (trust entry becomes stale, tool warns and suggests re-trust)
- No config file parsing needed at the trust-check hot path — just a file existence check
- Cleanup (`traces trust --clean`) removes dangling symlinks to deleted directories

Bad, because:
- Symlinks don't work on all platforms equally (Windows needs a plain file fallback, matching mise's approach)
- Trust is path-based, not content-based — renaming a directory invalidates trust
- The hash-keyed naming makes manual inspection of the trust store slightly opaque (though `traces trust list` solves this)

### Confirmation

The trust check is enforced in the template instantiation path: before a template from a resolved directory is rendered, the tool checks for a symlink at `~/.local/share/traces/trusted/<hash>`. Unit tests verify that trusted directories pass, untrusted directories error with the correct miette diagnostic, and `traces trust` creates the symlink correctly.

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

Pattern follows mise's `src/config/tracking.rs` design. The `Tracker` struct provides `track()`, `list_all()`, and `clean()` methods operating on a hash-keyed symlink directory. The `traces trust` CLI subcommand wraps this with interactive prompts (inquire) when run without arguments.
