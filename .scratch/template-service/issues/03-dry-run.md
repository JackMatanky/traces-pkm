# Dry-run mode (-n / --dry-run)

Status: ready-for-agent

## Parent

`.scratch/template-service/PRD.md`

## What to build

Add `-n` / `--dry-run`. In dry-run, render the template and print the result to stdout, skip the existence check and the file write entirely, and let interactive functions return their non-interactive defaults so the preview never hangs. This relies on the PromptProvider's non-TTY fallback; dry-run must not depend on a terminal.

## Acceptance criteria

- [ ] `-n` / `--dry-run` renders to stdout and writes nothing to disk
- [ ] Existence check / overwrite guard is skipped in dry-run
- [ ] Interactive functions return defaults during dry-run (no hang, no TTY required)
- [ ] Tests verify stdout output, absence of any written file, and default values from interactive functions

## Blocked by

- `.scratch/template-service/issues/01-render-pipeline-tracer.md`
