# PromptProvider trait + NoPromptProvider fake (text, confirm)

Status: ready-for-agent

## Parent

`.scratch/prompt-service/PRD.md`

## What to build

Establish the interactive-input seam. Define a `PromptProvider` trait with `text(label, default)` and `confirm(label, default)`, and ship a `NoPromptProvider` test fake that returns pre-configured responses (or the supplied defaults) with zero I/O. This is the tracer bullet that proves the abstraction works end-to-end — a consumer can hold a `&dyn PromptProvider`, call it, and get deterministic results in tests without a TTY.

`select`/`multi_select` come later (issue 03); the terminal implementation comes in issue 02.

## Acceptance criteria

- [ ] `PromptProvider` trait defined with `text` and `confirm` methods returning `Result`
- [ ] `NoPromptProvider` implements the trait and returns configured responses (falling back to the provided default when no response is queued)
- [ ] Trait is object-safe (`&dyn PromptProvider` usable by consumers)
- [ ] Unit tests verify `NoPromptProvider` returns exactly the configured responses and honors defaults
- [ ] Lives in its own module/crate with no dependency beyond what the fake needs (no `inquire` yet)

## Blocked by

None - can start immediately
