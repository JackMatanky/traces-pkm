# Interactive template functions via PromptProvider

Status: ready-for-agent

## Parent

`.scratch/template-service/PRD.md`

## What to build

Register the interactive custom functions on the minijinja `Environment`, each delegating to the `PromptProvider` the service holds:

- `prompt_text(label)` / `prompt_text(label, default)`
- `select(label, items)`
- `confirm(label)`
- `multi_select(label, items)`

TemplateService stays ignorant of TTY state — the PromptProvider handles detection and fallback. In tests and MCP mode a `NoPromptProvider` supplies deterministic responses.

## Acceptance criteria

- [ ] `prompt_text`, `select`, `confirm`, `multi_select` callable from templates and delegate to `PromptProvider`
- [ ] `prompt_text` supports the optional default argument
- [ ] With `NoPromptProvider`, rendering is deterministic (no TTY required)
- [ ] Tests render templates exercising each function and assert the output

## Blocked by

- `.scratch/template-service/issues/01-render-pipeline-tracer.md`
- `.scratch/prompt-service/issues/03-select-and-multi-select.md`
