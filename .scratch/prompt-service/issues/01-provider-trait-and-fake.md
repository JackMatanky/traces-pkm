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

## Rust guidance

Relevant skills: `m04-zero-cost`, `m06-error-handling`, `m05-type-driven`.

- **Dispatch (m04):** consumers hold one prompt implementation chosen at runtime (terminal vs fake), so this is a genuine dynamic-dispatch case — use `&dyn PromptProvider`, not generics. Keep the trait **object-safe**: no generic methods, no `Self` return, no `Self: Sized` bound. Take `&self` (not `&mut self`) so a shared reference can be passed to both ConfigService `init` and TemplateService functions.
- **Error type (m06):** this is a reusable component, not an app entry point, so return a **typed** error via `thiserror`, not `anyhow`. Give it a small enum (e.g. `Interrupted`, `Io`) so downstream miette layers can categorise. Do not `unwrap`/`expect` on prompt paths.
- **Method shape:** `default: Option<&str>` for `text`; `default: Option<bool>` for `confirm`. Prefer borrowed `&str`/`&[String]` params over owned to keep call sites cheap.
- **Fake determinism:** `NoPromptProvider` holds a queue of configured responses behind interior mutability if `&self` methods must pop (a `RefCell<VecDeque<_>>` is fine here — see `m03-mutability`); when the queue is empty, fall back to the supplied default rather than panicking.

## Blocked by

None - can start immediately
