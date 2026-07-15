# `traces trust` CLI (default, path, list, clean)

Status: ready-for-agent

## Parent

`.scratch/config-service/spec.md`

## What to build

The `traces trust` command surface, wired through clap to the trust store from issue 04. Issue 04 only built `ConfigTrust::trust()`/`is_trusted()` (check/create/reject) — `list`/`clean` **do not exist yet** on `ConfigTrust`/`ConfigService` and are this issue's responsibility to add, mirroring `ConfigTracker::list_all()`/`clean()` and `ConfigService::list_tracked()`/`clean_tracked_store()`. Nor does a `src/cli/` module or a `clap` dependency exist yet — this issue is also the first CLI scaffolding, not just wiring onto an existing surface.

- `traces trust` — trust `cwd`
- `traces trust <path>` — trust the given path
- `traces trust list` — list all trusted directories (read the trust store entries)
- `traces trust clean` — remove stale entries whose target directory no longer exists, including orphaned content-hash companions

## Resolved design (from triage grilling — see Comments)

`ConfigService::trust(root, config_file)` takes two paths because issue 04's local-config trust needs a config file to content-hash. The CLI only ever has one user-supplied path. Resolution:

- **Deriving `config_file`:** the CLI computes `config_file = <path>/.traces/config.toml` (or `cwd/.traces/config.toml` for bare `traces trust`).
- **Tolerate a missing config file.** `traces trust <path>` before `.traces/config.toml` exists (i.e. before `traces init`) is a valid, intended flow — not an error. When the derived `config_file` doesn't exist, `ConfigTrust::trust()` must record the root only (via the store, no content-hash companion) instead of propagating `TrustError::Hash`. **This changes `ConfigTrust::trust()`'s behavior from issue 04**, not just the CLI layer: add a companion-optional path through `ConfigTrust::trust()` (or a `trust_root_only()` variant) that skips the `hash_file_contents`/companion-write steps when the config file is absent.
- **Known consequence, not a bug:** after a companion-less trust, a later `init` + `build()` will see a missing companion and report `Stale` (existing `is_trusted` behavior: missing companion always fails toward re-verification). The user re-runs `traces trust` once after `init` to bind the content hash. Document this in `--help`/error text; do not "fix" `is_trusted` to special-case this — it's the same fail-safe behavior issue 04 already relies on for legacy entries.

## Acceptance criteria

- [ ] `ConfigTrust` gains `list_all() -> Result<Vec<PathBuf>, StoreError>` and `clean() -> Result<usize, StoreError>`, delegating to the shared `ConfigFileStore` for the base entries
- [ ] `ConfigTrust::clean()` also removes each pruned entry's orphaned `<entry>.hash` companion — cannot be a bare delegation to `ConfigFileStore::clean()`, which has no concept of companions. Add whatever `ConfigFileStore` primitive is needed to know which entries were/would be removed (mirrors how `entry_path` was added in issue 04 for the same companion-file need)
- [ ] `ConfigService` gains `list_trusted()`/`clean_trusted_store()` (or equivalent names matching `list_tracked`/`clean_tracked_store`'s convention), delegating to `ConfigTrust`
- [ ] `ConfigTrust::trust()` (and thus `ConfigService::trust()`) accepts a `config_file` that does not exist: records the root only, skips the content-hash companion, does not error
- [ ] `traces trust` with no args trusts `cwd` (config file derived as `cwd/.traces/config.toml`)
- [ ] `traces trust <path>` trusts the given directory (config file derived as `<path>/.traces/config.toml`), whether or not that file exists yet
- [ ] `traces trust list` prints all currently trusted directories (roots only, no staleness column — consistent with `list_tracked`)
- [ ] `traces trust clean` removes dangling/stale trust entries (and their companions) and reports what was removed
- [ ] `src/cli/` module created, with its own error type (miette-wrapping `TrustError`/`StoreError` with `help` text, per `src/config/mod.rs`'s stated plan for a future CLI layer) — no other module currently owns CLI errors
- [ ] `clap` added to `Cargo.toml`; `src/main.rs` wires the `Cli`/`Commands` root and calls into `src/cli/`
- [ ] Integration tests verify each subcommand's effect on the trust store (temp store), including: trust-before-init (no companion) not erroring, and clean removing both a stale root entry and its companion

## Rust guidance

Relevant skills: `domain-cli`, `m05-type-driven`, `m06-error-handling`.

- **clap shape (domain-cli):** model `trust` as a subcommand whose payload is an optional positional path *plus* nested actions. `list` and `clean` are subcommands of `trust`, while bare `traces trust` and `traces trust <path>` share the default action. Use `#[derive(Subcommand)]` with an enum; give `list`/`clean` their own variants and a default `Trust { path: Option<PathBuf> }` variant. Verify clap can disambiguate `trust <path>` from `trust list` (reserve `list`/`clean` as keywords).
- **stdout vs stderr (domain-cli):** `trust list` output is data → `println!` to stdout (pipeable). Errors and the `clean` removal report go to stderr via `eprintln!`. **Note:** this workspace's `Cargo.toml` denies `clippy::print_stdout` (only `print_stderr` is allowed) — `trust list`'s `println!` call site needs an explicit `#[allow(clippy::print_stdout, reason = "...")]`, matching the existing `print_stderr = "allow"` precedent, or `cargo clippy -- -D warnings` fails.
- **Thin CLI layer (m05):** the command handlers should be thin adapters over the trust store API — parse args, derive `config_file` from the positional path, call `trust()`/`is_trusted()`/`list_trusted()`/`clean_trusted_store()`, format output. No trust logic in the CLI layer beyond the path derivation above.
- **`clean` semantics (m06):** report which entries were removed; a store with no stale entries is success with an empty report, not an error.

## Blocked by

- `.scratch/config-service/issues/04-trust-store.md` (implemented — see its Status/Comments; the check/create/reject core this issue builds on is done)

## Comments

> *This was generated by AI during triage.*

Re-triaged: issue 04 (its blocker) turned out to already be fully implemented despite a stale `Status: ready-for-agent`, but its own Comments flagged that `list_all`/`clean` on `ConfigTrust` were deliberately deferred to this issue and never added. Grilling also surfaced a real signature mismatch — `ConfigService::trust(root, config_file)` needs a config file to hash, but the CLI only ever has one user path — and ADR 0002's "Update (issue 04)" section, which explicitly left "template-directory trust... still owed to issue 05" as an open thread. Resolved: one trust store (not two), with `ConfigTrust::trust()` tolerating a missing config file by recording the root only. See ADR 0002's amendment for the closed-out rationale. Brief above rewritten to reflect this; kept `ready-for-agent` rather than downgrading, since the design is now fully resolved.
