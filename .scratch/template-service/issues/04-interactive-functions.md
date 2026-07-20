# Interactive template functions via ui.* namespace (Object + DialogProvider)

Status: ready-for-agent

## Parent

`.scratch/template-service/spec.md`

## What to build

Register the interactive custom methods on the `ui` namespace object, each delegating to the interactive-provider API the service holds:

- `ui.text_input(label)` / `ui.text_input(label, default)`
- `ui.select(label, items)`
- `ui.confirm(label)`
- `ui.multi_select(label, items)`

TemplateService stays ignorant of TTY state — the provider handles detection and fallback. In tests and MCP mode a `PresetDialogProvider` supplies deterministic responses.

The `ui` namespace is a struct implementing `minijinja::value::Object`, registered via `env.add_global("ui", Value::from_object(...))`. It holds an `Arc<dyn DialogProvider>`. The `Object` trait's default `call_method` looks up each method name via `get_value` and dispatches to returned callable `Value`s, each created via `Value::from_function(...)`.

## Acceptance criteria

- [ ] `ui.text_input`, `ui.select`, `ui.confirm`, `ui.multi_select` callable from templates and delegate to the interactive provider
- [ ] `ui.text_input` supports the optional default argument
- [ ] With `PresetDialogProvider`, rendering is deterministic (no TTY required)
- [ ] Tests render templates exercising each method and assert the output

## Rust guidance

Relevant skills: `m03-mutability`, `m02-resource`, `m04-zero-cost`, `m06-error-handling`, custom.

- **Storing the provider in UiNamespace (m02/m03):** `UiNamespace` holds `Arc<dyn DialogProvider>`. The `Object::get_value` implementation returns callable `Value`s (via `Value::from_function(...)`) for each method name. Each callable captures a clone of the `Arc<dyn DialogProvider>` — object safety pays off here exactly as before. `Rc` would fail `Send` bounds on the callable.
- **Object safety pays off (m04):** `Arc<dyn DialogProvider>` only works because the trait is object-safe. No change from prior decision.
- **Error propagation into minijinja (m06):** same as before — prompt failures surface as `minijinja::Error` so `render` returns `Err` cleanly. Decide the `PromptError` → `ErrorKind` mapping at impl time.
- **Argument arity:** `ui.text_input` has a 1-arg and 2-arg form — accept `Option<String>` for the default.
- **Blocking render model in MCP mode:** unchanged — synchronous render, `PresetDialogProvider` avoids blocking.

## Design considerations

The interface the prompt module exposes is being explored. Three options, simplest first:

| Option | Structure | Pros | Cons |
|--------|-----------|------|------|
| **A: Monolithic `DialogProvider` trait** | Single trait with 4 methods (`text`, `confirm`, `select`, `multi_select`). UiNamespace holds `Arc<dyn DialogProvider>`, each callable returned by `get_value` captures the same `Arc`. | One `impl` block per concrete type. One `Arc` allocation. Zero refactoring cost. | Each callable carries vtable entries for all 4 methods when it only needs 1. |
| **B: Split traits + bundling struct** | 4 traits (`TextInputProvider`, `ConfirmProvider`, `SelectProvider`, `MultiSelectProvider`). UiNamespace holds per-capability `Arc<dyn SubTrait>`. | Each callable narrows to exactly one method. | 4 `impl` blocks per concrete type. 4 `Arc` allocations. Extra files. |
| **C: Split traits with blanket supertrait** | Same sub-traits as B, plus `trait DialogProvider: TextInputProvider + ConfirmProvider + SelectProvider + MultiSelectProvider {}`. | Old `Arc<dyn DialogProvider>` consumers keep working. | Trait upcasting is unstable; practical outcome identical to B. |

**Decision**: Option A (monolithic `DialogProvider` trait as already built in `src/dialog/mod.rs`). No change from prior decision — the vtable overhead is negligible.

## Blocked by

- `.scratch/template-service/issues/01-render-pipeline-tracer.md`
- `.scratch/dialog/issues/03-select-and-multi-select.md`
