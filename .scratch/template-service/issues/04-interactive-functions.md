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

## Rust guidance

Relevant skills: `m03-mutability`, `m02-resource`, `m04-zero-cost`, `m06-error-handling`.

- **Sharing the provider into closures (m02/m03):** each interactive custom function closure needs to call the one `PromptProvider`. Closures are `Fn` with shared access and must be `'static` for minijinja, so capture the provider as **`Rc<dyn PromptProvider>`** (single-threaded) and clone the `Rc` into each closure. Prefer `Rc` over threading a borrow — lifetimes won't allow a plain `&dyn` into a `'static` closure.
- **Object safety pays off (m04):** this is exactly why PromptService issue 01 kept the trait object-safe — `Rc<dyn PromptProvider>` only works if the trait is object-safe. If a method there took generics, it would break here.
- **Error propagation into minijinja (m06):** a prompt failure inside a function must surface as a minijinja `Error` (map the `PromptProvider` error into `minijinja::Error` with `ErrorKind::InvalidOperation` or similar), so `render` returns `Err` cleanly rather than panicking. Never `unwrap` a prompt result inside a closure.
- **Argument arity:** `prompt_text` has a 1-arg and 2-arg form — minijinja supports optional trailing args; accept `Option<String>` for the default rather than registering two functions.

## Blocked by

- `.scratch/template-service/issues/01-render-pipeline-tracer.md`
- `.scratch/prompt-service/issues/03-select-and-multi-select.md`
