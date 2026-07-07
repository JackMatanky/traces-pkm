# TerminalPromptProvider (inquire + TTY fallback) for text, confirm

Status: ready-for-agent

## Parent

`.scratch/prompt-service/PRD.md`

## What to build

The real terminal implementation. `TerminalPromptProvider` implements `PromptProvider` via `inquire` for `text` and `confirm`. Before prompting it checks `is_terminal()` on stdin; in non-TTY contexts it returns the supplied default without ever calling `inquire`, so templates and `init` render/run without hanging in scripts, dry-run, or CI.

## Acceptance criteria

- [ ] `TerminalPromptProvider` implements `text` and `confirm` via `inquire`
- [ ] Non-TTY stdin returns the provided default (or a sensible empty/false fallback) without invoking `inquire`
- [ ] Uses the `is-terminal` crate for TTY detection
- [ ] Tests cover the non-TTY fallback path (real TTY paths are not automatable in CI)

## Blocked by

- `.scratch/prompt-service/issues/01-provider-trait-and-fake.md`
