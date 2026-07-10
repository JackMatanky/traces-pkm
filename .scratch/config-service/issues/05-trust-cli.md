# `traces trust` CLI (default, path, list, clean)

Status: ready-for-agent

## Parent

`.scratch/config-service/spec.md`

## What to build

The `traces trust` command surface, wired through clap to the trust store from issue 04:

- `traces trust` — trust `cwd`
- `traces trust <path>` — trust the given path
- `traces trust list` — list all trusted directories (read the trust store entries)
- `traces trust clean` — remove stale entries whose target directory no longer exists

## Acceptance criteria

- [ ] `traces trust` with no args trusts `cwd`
- [ ] `traces trust <path>` trusts the given directory
- [ ] `traces trust list` prints all currently trusted directories
- [ ] `traces trust clean` removes dangling/stale trust entries and reports what was removed
- [ ] Integration tests verify each subcommand's effect on the trust store (temp store)

## Rust guidance

Relevant skills: `domain-cli`, `m05-type-driven`, `m06-error-handling`.

- **clap shape (domain-cli):** model `trust` as a subcommand whose payload is an optional positional path *plus* nested actions. `list` and `clean` are subcommands of `trust`, while bare `traces trust` and `traces trust <path>` share the default action. Use `#[derive(Subcommand)]` with an enum; give `list`/`clean` their own variants and a default `Trust { path: Option<PathBuf> }` variant. Verify clap can disambiguate `trust <path>` from `trust list` (reserve `list`/`clean` as keywords).
- **stdout vs stderr (domain-cli):** `trust list` output is data → `println!` to stdout (pipeable). Errors and the `clean` removal report go to stderr via `eprintln!`. Exit non-zero on failure by returning `Result` from the handler.
- **Thin CLI layer (m05):** the command handlers should be thin adapters over the trust store API from issue 04 — parse args, call `trust()`/`is_trusted()`/list/clean, format output. No trust logic in the CLI layer.
- **`clean` semantics (m06):** report which entries were removed; a store with no stale entries is success with an empty report, not an error.

## Blocked by

- `.scratch/config-service/issues/04-trust-store.md`
