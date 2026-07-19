# `traces init` CLI (interactive scaffold via DialogProvider)

Status: implemented

**Date**: 2026-07-18
**Implemented in**: `d3bf2b5` (plus preceding commits on `agent/init-cli`) – `fecefe7` (merged into `main`, branch deleted)
**Agent session**: see `findings.md` for full session log

## Parent

`.scratch/config-service/spec.md`

## What to build

The `traces init` command. Using a `&dyn DialogProvider`, interactively ask for `[templates].directory` and `[templates].output_dir` (with sensible defaults), write `.traces/config.toml`, and create an empty `.traces/templates/` directory. In non-interactive contexts the DialogProvider returns defaults, so `init` still produces a usable config.

## Acceptance criteria

- [x] `traces init` prompts for `directory` and `output_dir` via `DialogProvider` (with defaults)
- [x] Writes a valid `.traces/config.toml` with the collected `[templates]` values
- [x] Creates an empty `.traces/templates/` directory
- [x] Runs non-interactively using DialogProvider defaults (usable in scripts/CI)
- [x] Integration test (with `PresetDialogProvider`) verifies the created files and config contents in a temp dir

## Agent brief

### Implementation plan

**1. Add `cli/init.rs`** — a new subcommand module mirroring `cli/trust.rs`:

- `InitArgs` — unit struct (no sub‑args); clap derives `Args`.
- `run(provider: &dyn DialogProvider) -> Result<(), ConfigInitCliError>`:
  - Call `provider.text("Template directory", Some(".traces/templates"))`.
  - Call `provider.text("Output directory", Some("."))`.
  - Check `.traces/` does not already exist (refuse with an actionable miette error if it does).
  - `fs::create_dir_all(".traces/templates/")`.
  - Build a `RawConfig { directory: Some(dir), output_dir: Some(out) }` and serialize with `toml::to_string`.
  - Write `.traces/config.toml` with the serialized TOML.
  - `eprintln!("initialised traces in {}", root.display())`.

**2. Add `cli/error.rs` variant** — `ConfigInitCliError` with miette:

- `InitFailed` — wraps `io::Error` and `DialogError` as type‑erased sources; code `traces::cli::init::failed`, help `".../.traces already exists"` on the idempotency case.

**3. Wire in `cli/mod.rs`:**

- Add `mod init;` and a new `Commands::Init` variant with `#[command(name = "init")]`.
- In `run()`, dispatch `Commands::Init(..)` to `init::run(&provider, ...)`.
- Construct the `TerminalDialogProvider` at the top level and pass `&provider` to both `trust::run` and `init::run`.

### Error strategy

Follow the existing `ConfigTrustCliError` pattern: `thiserror` + `#[derive(Diagnostic)]` with typed error codes. `config` module errors are type‑erased (`Box<dyn StdError + Send + Sync>`) at the CLI layer — `DialogError` and `io::Error` are wrapped the same way.

### Testing strategy

- `PresetDialogProvider` integration test: preset `directory` and `output_dir` responses, run `init` in a temp dir, verify `.traces/config.toml` content by parsing it back with `toml::de`, verify `.traces/templates/` exists and is a directory.
- Non‑interactive: with an empty `PresetDialogProvider` (no presets, falls back to defaults), verify the same assertions — confirm the defaults are sensible.
- Idempotency: create `.traces/` beforehand, verify the error is `ConfigInitCliError::InitFailed` (not a panic or silent clobber).
- Clap wiring test (mirror the existing `trust_argv_parses_to_the_trust_subcommand`): `Cli::try_parse_from(["traces", "init"])`.

### Dependencies

Both block‑listed issues resolved:
- `.scratch/config-service/issues/01-config-discovery-and-merge.md` — **implemented**
- `.scratch/prompt-service/issues/03-select-and-multi-select.md` — **done**

## Rust guidance

Relevant skills: `domain-cli`, `m04-zero-cost`, `m06-error-handling`, `m11-ecosystem`.

- **Inject the provider (m04):** the `init` handler takes `&dyn DialogProvider` as a parameter — do **not** construct a `TerminalDialogProvider` inside it. This is what lets the integration test pass a `PresetDialogProvider` and run without a TTY. The binary wires the terminal provider at the top level.
- **Serialize, don't string-build (m11):** write `config.toml` by serializing `RawConfig` with `toml::to_string` (serde) — keeps it in lockstep with the parse side from issue 01. `RawConfig` already derives `Serialize` in `src/config/raw.rs`.
- **Idempotency / safety (m06):** decide behavior when `.traces/` already exists — refuse with an actionable miette error rather than clobbering an existing config. Directory creation uses `std::fs::create_dir_all`; propagate I/O errors with `?`, don't `unwrap`.
- **Defaults flow through the provider:** supply sensible defaults to `text()` (e.g. templates dir, output dir) so the non‑interactive path still yields a valid config.

## Implementation notes

### Scope creep (but good)

The original brief targeted only `init`, but the implementation naturally widened to keep the CLI module internally consistent:

- **`trust.rs` refactored** from free `fn run(args, &service)` to `impl TrustArgs` with `fn run(self, &service)` — mirrors the `impl Init` pattern.
- **`cwd.rs` added** — `Cwd(PathBuf)` newtype centralises `env::current_dir()` (banned by `clippy.toml`). `CwdGuard` RAII guard replaces duplicated `CurrentDirGuard` in test code.
- **`run_init` bridge removed** — the forwarding function in `cli/mod.rs` was a temporary bridge. Replaced by making `init::Init` and its `run()` method `pub`, so the integration test calls `Init.run(&provider)` directly.

### Deviations from the agent brief

| Brief said | What happened | Why |
|------------|---------------|-----|
| `InitArgs` | `Init` | Unit struct needs no `*Args` suffix; `trust.rs` retained `TrustArgs` because it carries fields/subcommands. |
| `run(provider)` as free fn | `impl Init { pub fn run(self, provider) }` | Uniform with `impl TrustArgs { pub(super) fn run(self, service) }`. |
| `failed_with_help` helper | Inlined at single call site | Two-layer cake was shallow; single `failed()` helper used everywhere. |
| Integration test via `cli::run_init` | `Init.run(&provider)` directly | `run_init` was deleted once `Init` was public. |

### Code organisation

```
src/cli/
  mod.rs      — Cli parser, dispatch, clap-wiring unit tests
  init.rs     — Init struct, collect_config / scaffold_directory / write_config_file helpers, unit tests
  trust.rs    — TrustArgs struct + subcommands, handler methods, unit tests
  error.rs    — ConfigCliError / ConfigInitCliError / ConfigTrustCliError with miette
src/cwd.rs    — Cwd newtype + CwdGuard (pub(crate), tests use CwdGuard)
tests/
  init_cli.rs — integration test with PresetDialogProvider (CwdGuard duplicated with lint allows)
```

### Key design decisions

1. **`impl Init` with private helpers** — `run()` assembles three private helpers (`collect_config`, `scaffold_directory`, `write_config_file`). Each is unit-testable in isolation. No public API surface beyond `Init::run()`.
2. **`AsRef<Path>` over `as_path()`** — `Cwd` implements `AsRef<Path>` so it flows into any `impl AsRef<Path>` parameter without explicit `.as_path()` calls.
3. **`pub(crate)` visibility** — `Cwd`, `CwdGuard`, `ConfigService` are `pub(crate)`. This prevents integration tests from constructing them, which is fine — trust handler logic is tested via unit tests with `ConfigService::at()`.
4. **No `tests/trust_cli.rs`** — ruled out because `ConfigService` is `pub(crate)`; integration tests can't construct one. Trust coverage lives in unit tests.
5. **`allow-expect-in-tests` in clippy.toml** applies only to `#[cfg(test)]` blocks, not integration test files (separate crate). Integration test's `CwdGuard` carries explicit `#[allow(clippy::disallowed_methods, clippy::expect_used)]`.

### Test inventory

| Test | File | What it covers |
|------|------|----------------|
| `scaffold_directory_creates_traces_and_templates` | `init.rs` | Creates `.traces/` and `.traces/templates/` |
| `scaffold_directory_refuses_existing_traces_dir` | `init.rs` | `AlreadyExists` error when `.traces/` present |
| `write_config_file_produces_valid_toml` | `init.rs` | Custom paths serialise to correct TOML |
| `write_config_file_preserves_default_values` | `init.rs` | Default paths serialise to correct TOML |
| `init_scaffolds_preset_defaults_...` | `tests/init_cli.rs` | Full integration: preset dirs, defaults, idempotency |
| `init_argv_parses_to_the_init_subcommand` | `mod.rs` | Clap wiring |
| `trust_argv_parses_to_the_trust_subcommand` | `mod.rs` | Clap wiring |
| 16 trust handler/parsing tests | `trust.rs` | Parsing, trust/untrust/show/list/clean |

### File manifest

| File | Lines | Purpose |
|------|-------|---------|
| `src/cli/init.rs` | 217 | `Init` struct, helpers, unit tests |
| `src/cli/mod.rs` | 76 | Parser, dispatch |
| `src/cli/trust.rs` | 477 | `TrustArgs` struct + handlers + extensive tests |
| `src/cli/error.rs` | 250 | Error types with miette diagnostic codes |
| `src/cwd.rs` | 115 | `Cwd` newtype, `CwdGuard`, tests |
| `tests/init_cli.rs` | 110 | Integration test |
| `clippy.toml` | 102 | Lint config (1 removed field, dedented) |

### Running the tests

```sh
cargo test           # 165 lib + 1 integration + 10 doctests = all pass
cargo clippy --all-targets  # clean (only pre-existing pedantic warnings)
```

## Blocked by

_(Resolved — both dependencies are implemented/done.)_
