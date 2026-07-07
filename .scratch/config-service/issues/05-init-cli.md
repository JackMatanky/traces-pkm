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

## Blocked by

- `.scratch/config-service/issues/01-config-discovery-and-merge.md`
- `.scratch/prompt-service/issues/03-select-and-multi-select.md`
