# Add select + multi_select to trait and both providers

Status: ready-for-agent

## Parent

`.scratch/prompt-service/PRD.md`

## What to build

Round out the list-based prompts. Add `select(label, items)` and `multi_select(label, items)` to `PromptProvider`, implemented in both `TerminalPromptProvider` (via `inquire`, with non-TTY fallback) and `NoPromptProvider` (configured responses). `select` is the renamed former `suggester` — use `select` everywhere.

With this slice the trait is complete and every downstream consumer (ConfigService `init`, TemplateService functions) has the full prompt surface available.

## Acceptance criteria

- [ ] `select` returns one chosen item from the list; `multi_select` returns a `Vec` of chosen items
- [ ] Both implemented in `TerminalPromptProvider` (inquire) and `NoPromptProvider` (fake)
- [ ] Non-TTY `select`/`multi_select` return sensible defaults without calling `inquire` (e.g. first item / empty selection)
- [ ] Tests verify `NoPromptProvider` returns configured selections and the non-TTY fallback for the terminal provider

## Blocked by

- `.scratch/prompt-service/issues/01-provider-trait-and-fake.md`
- `.scratch/prompt-service/issues/02-terminal-provider.md`
