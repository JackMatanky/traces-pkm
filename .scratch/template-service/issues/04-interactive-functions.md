# Interactive template functions via DialogProvider

Status: ready-for-agent

## Parent

`.scratch/template-service/spec.md`

## What to build

Register the interactive custom functions on the minijinja `Environment`, each delegating to the interactive-provider API the service holds:

- `prompt_text(label)` / `prompt_text(label, default)`
- `select(label, items)`
- `confirm(label)`
- `multi_select(label, items)`

TemplateService stays ignorant of TTY state — the provider handles detection and fallback. In tests and MCP mode a `PresetDialogProvider` supplies deterministic responses.

The provider API is in flux (see [Design considerations](#design-considerations)). Whichever option is chosen, TemplateService receives a single bundling object and destructures it internally — the closure-registration logic is the same either way.

## Acceptance criteria

- [ ] `prompt_text`, `select`, `confirm`, `multi_select` callable from templates and delegate to the interactive provider
- [ ] `prompt_text` supports the optional default argument
- [ ] With `PresetDialogProvider`, rendering is deterministic (no TTY required)
- [ ] Tests render templates exercising each function and assert the output

## Rust guidance

Relevant skills: `m03-mutability`, `m02-resource`, `m04-zero-cost`, `m06-error-handling`.

- **Sharing the provider into closures (m02/m03):** each interactive custom function closure needs to call the provider. The `Function` trait requires `Send + Sync + 'static` on all closures registered via `add_function`, so capture as **`Arc<dyn DialogProvider>`** and clone the `Arc` into each closure. `Rc` is not `Send` and will fail to compile. Lifetimes won't allow a plain `&dyn` into a `'static` closure, so `Arc` is the only option.
- **Object safety pays off (m04):** this is exactly why the dialog module kept the trait object-safe — `Rc<dyn DialogProvider>` / `Arc<dyn DialogProvider>` only works if the trait is object-safe. If a method took generics, it would break here.
- **Error propagation into minijinja (m06):** a prompt failure inside a function must surface as a minijinja `Error`, so `render` returns `Err` cleanly rather than panicking. Never `unwrap` a prompt result inside a closure. **To decide before implementation**: which `PromptError` variant maps to which `minijinja::ErrorKind` (e.g. does `Interrupted` become `InvalidOperation`? Should `EmptyOptions` be a distinct kind?). Document the chosen mapping at impl time.
- **Argument arity:** `prompt_text` has a 1-arg and 2-arg form — minijinja supports optional trailing args; accept `Option<String>` for the default rather than registering two functions.
- **Blocking render model in MCP mode:** Template rendering is synchronous — `prompt_text`, `select`, and `multi_select` block the calling thread. In MCP mode, `PresetDialogProvider` returns preset responses so no blocking occurs. However, if select/multi-select items are computed dynamically during rendering (e.g. `{% set items = get_list() %}` / `{{ select("pick", items) }}`), the agent supplies the preset index blind — it does not see the computed items. Presets are consumed in render order, so the index is a relative choice ("first", "second") resolved against the items array at call time. For MVP this is sufficient. Richer MCP interaction (agent previews items before choosing) would require a multi-pass render model and is deferred.

## Design considerations

The interface the prompt module exposes is being explored. Three options, simplest first:

| Option | Structure | Pros | Cons |
|--------|-----------|------|------|
| **A: Monolithic `DialogProvider` trait** | Single trait with 4 methods (`text`, `confirm`, `select`, `multi_select`). TemplateService holds `Arc<dyn DialogProvider>`, each closure captures the same `Arc`. | One `impl` block per concrete type. One `Arc` allocation. Zero refactoring cost. | Each closure carries vtable entries for all 4 methods when it only needs 1. |
| **B: Split traits + bundling struct** | 4 traits (`TextInputProvider`, `ConfirmProvider`, `SelectProvider`, `MultiSelectProvider`). `InteractiveBackend` struct holds `Arc<dyn T>` for each. TemplateService takes `InteractiveBackend`, each closure captures its own `Arc<dyn SubTrait>`. | Each closure narrow to exactly one method. Self-documenting per-capability traits. Module file structure clearer. | 4 `impl` blocks per concrete type. 4 `Arc` allocations per backend. Extra files. |
| **C: Split traits with blanket supertrait** | Same sub-traits as B, plus `trait DialogProvider: TextInputProvider + ConfirmProvider + SelectProvider + MultiSelectProvider {}` with blanket impl. | Old consumers (`Arc<dyn DialogProvider>`) keep working. New closures can narrow per sub-trait. | Still need `InteractiveBackend` or similar to produce `Arc<dyn SubTrait>` from `Arc<dyn DialogProvider>` (trait upcasting is unstable). Practical outcome identical to B. |

**Decision**: Option A (monolithic `DialogProvider` trait as already built in `src/dialog/mod.rs`). The trait is object-safe, `Send + Sync`, and sufficient for MVP. Each closure captures `Arc<dyn DialogProvider>` and calls the single method it needs — the vtable overhead of carrying all four method entries is negligible for four methods on a trait invoked once per template call. Trait-splitting (Options B/C) remains a future option if profiling ever shows it matters, but is deferred.

## Blocked by

- `.scratch/template-service/issues/01-render-pipeline-tracer.md`
- `.scratch/dialog/issues/03-select-and-multi-select.md`
