# `traces init` CLI (interactive scaffold via DialogProvider)

Status: ready-for-agent

## Parent

`.scratch/config-service/spec.md`

## What to build

The `traces init` command. Using a `&dyn DialogProvider`, interactively ask for `[templates].directory` and `[templates].output_dir` (with sensible defaults), write `.traces/config.toml`, and create an empty `.traces/templates/` directory. In non-interactive contexts the DialogProvider returns defaults, so `init` still produces a usable config.

## Acceptance criteria

- [ ] `traces init` prompts for `directory` and `output_dir` via `DialogProvider` (with defaults)
- [ ] Writes a valid `.traces/config.toml` with the collected `[templates]` values
- [ ] Creates an empty `.traces/templates/` directory
- [ ] Runs non-interactively using DialogProvider defaults (usable in scripts/CI)
- [ ] Integration test (with `PresetDialogProvider`) verifies the created files and config contents in a temp dir

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

## Blocked by

_(Resolved — both dependencies are implemented/done.)_
