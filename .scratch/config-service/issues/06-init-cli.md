# `traces init` CLI (interactive scaffold via PromptProvider)

Status: ready-for-agent

## Parent

`.scratch/config-service/PRD.md`

## What to build

The `traces init` command. Using a `&dyn PromptProvider`, interactively ask for `[templates].directory` and `[templates].output_dir` (with sensible defaults), write `.traces/config.toml`, and create an empty `.traces/templates/` directory. In non-interactive contexts the PromptProvider returns defaults, so `init` still produces a usable config.

## Acceptance criteria

- [ ] `traces init` prompts for `directory` and `output_dir` via `PromptProvider` (with defaults)
- [ ] Writes a valid `.traces/config.toml` with the collected `[templates]` values
- [ ] Creates an empty `.traces/templates/` directory
- [ ] Runs non-interactively using PromptProvider defaults (usable in scripts/CI)
- [ ] Integration test (with `NoPromptProvider`) verifies the created files and config contents in a temp dir

## Rust guidance

Relevant skills: `domain-cli`, `m04-zero-cost`, `m06-error-handling`, `m11-ecosystem`.

- **Inject the provider (m04):** the `init` handler takes `&dyn PromptProvider` as a parameter — do **not** construct a `TerminalPromptProvider` inside it. This is what lets the integration test pass a `NoPromptProvider` and run without a TTY. The binary wires the terminal provider at the top level.
- **Serialize, don't string-build (m11):** write `config.toml` by serializing the config struct with `toml::to_string` (serde), not by formatting TOML by hand — keeps it in lockstep with the parse side from issue 01.
- **Idempotency / safety (m06):** decide behavior when `.traces/` already exists — refuse with an actionable miette error rather than clobbering an existing config. Directory creation uses `std::fs::create_dir_all`; propagate I/O errors with `?`, don't `unwrap`.
- **Defaults flow through the provider:** supply sensible defaults to `prompt_text` (e.g. templates dir, output dir) so the non-interactive path still yields a valid config.

## Blocked by

- `.scratch/config-service/issues/01-config-discovery-and-merge.md`
- `.scratch/prompt-service/issues/03-select-and-multi-select.md`
