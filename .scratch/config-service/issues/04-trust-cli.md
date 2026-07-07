# `traces trust` CLI (default, path, list, clean)

Status: ready-for-agent

## Parent

`.scratch/config-service/PRD.md`

## What to build

The `traces trust` command surface, wired through clap to the trust store from issue 03:

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

## Blocked by

- `.scratch/config-service/issues/03-trust-store.md`
