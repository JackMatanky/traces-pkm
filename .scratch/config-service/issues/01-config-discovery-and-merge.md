# Config discovery, load, and local/global merge

Status: ready-for-agent

## Parent

`.scratch/config-service/PRD.md`

## What to build

The `ConfigService` core. Walk up from `cwd` looking for a local `.traces/config.toml`, fall back to the global `~/.config/traces/config.toml` (XDG via `dirs`), parse both as TOML, and merge with local overlaid on top of global. Expose the resolved config (the `[templates]` table: `directory`, `output_dir`) via read-only accessors. Invalid TOML surfaces a miette diagnostic with parse context.

This is the tracer bullet for configuration: given a filesystem layout, `ConfigService` returns the correct merged config.

## Acceptance criteria

- [ ] `ConfigService` struct loads local and/or global config and exposes the merged `[templates]` settings
- [ ] Upward walk from `cwd` finds `.traces/config.toml`; global fallback resolved via `dirs`
- [ ] Local overrides global on merge; either file may be absent
- [ ] `output_dir` defaults to `cwd` when unset
- [ ] Invalid TOML produces a miette error with context
- [ ] Tests cover: local only, global only, both (merge priority), neither, and invalid TOML — using per-test temp dirs

## Blocked by

None - can start immediately
