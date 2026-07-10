# Config discovery, load, and local/global merge

Status: implemented

## Parent

`.scratch/config-service/spec.md`

## What to build

The `ConfigService` core. Walk up from `cwd` looking for a local `.traces/config.toml`, fall back to the global `~/.config/traces/config.toml` (XDG via `dirs`), parse both as TOML, and merge with local overlaid on top of global. Expose the resolved config (the `[templates]` table: `directory`, `output_dir`) via read-only accessors. Invalid TOML surfaces a miette diagnostic with parse context.

This is the tracer bullet for configuration: given a filesystem layout, `ConfigService` returns the correct merged config.

## Acceptance criteria

- [x] `ConfigService` struct loads local and/or global config and exposes the merged `[templates]` settings
- [x] Upward walk from `cwd` finds `.traces/config.toml`; global fallback resolved via `dirs`
- [x] Local overrides global on merge; either file may be absent
- [x] `output_dir` defaults to `cwd` when unset
- [x] Invalid TOML produces a miette error with context
- [x] Tests cover: local only, global only, both (merge priority), neither, and invalid TOML — using per-test temp dirs

## Implementation notes

- Implemented in `src/config.rs` with `ConfigService` as the discovery/load/resolve boundary.
- `RawConfig` is the TOML DTO and `Config` is the resolved consumer-facing config.
- `DiscoveredConfig` is an immutable discovery result with private fields and read-only accessors for `config`, `root`, and primary `source`.
- `ConfigLayers` is the private merge staging type for optional global/project `DiscoveredConfig` values; there is no separate singular layer type.
- Global config defaults through `dirs::config_dir().join("traces/config.toml")`; tests use `ConfigService::with_global_config_path` to avoid depending on the host user's real config directory.
- Relative paths are resolved against the config file root before merge: project-relative for local config, global-config-directory-relative for global config.
- Verification run: `mise run test` passed 6/6 tests, `mise run clippy` passed, and GitNexus change detection reported low risk with no affected processes.

## Remaining gaps

- No unfulfilled functional acceptance criteria identified.
- Residual test gap: the default `dirs::config_dir()` path construction is implemented but not directly asserted in tests; the global fallback behavior is covered through the explicit test path hook.

## Rust guidance

Relevant skills: `m06-error-handling`, `m11-ecosystem`, `m12-lifecycle`, `m09-domain`.

- **Deserialization (m11):** parse TOML into `#[derive(Deserialize)]` structs via `serde` + `toml`. Make optional fields `Option<String>` and derive `Default` so an absent file collapses to an empty config. Do not hand-parse TOML.
- **Merge as data, not I/O (m09):** model merge as a pure function over two parsed `Config` values (local overlays global, field by field, `Some` wins). Keeping merge pure makes it unit-testable without touching the filesystem. `output_dir` defaulting to `cwd` is a resolution step *after* merge, not a stored value.
- **Error strategy (m06/m13):** ConfigService is a library boundary — return a **typed** `thiserror` error (`NotFound` is not an error here; a missing file is `Ok(default)`). Reserve error variants for *invalid* TOML and unreadable files. Surface parse failures through **miette** with the source span so the user sees where the TOML broke; carry `#[source]` from the underlying `toml::de::Error`.
- **Directory discovery (m11):** resolve the global path with the `dirs` crate (`config_dir()`), not hard-coded `~/.config`. The upward walk from `cwd` is plain `std::path` ancestor iteration — stop at the first `.traces/config.toml` found.
- **Lifecycle (m12):** load once into an owned `ConfigService` value and hand out read-only borrows; no `OnceLock`/global singleton for MVP — an explicit owner passed to consumers is simpler and more testable.

## Blocked by

None - can start immediately
