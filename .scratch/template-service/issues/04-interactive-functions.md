# Interactive template functions via PromptProvider

Status: ready-for-agent

## Parent

`.scratch/template-service/spec.md`

## What to build

Register the interactive custom functions on the minijinja `Environment`, each delegating to the interactive-provider API the service holds:

- `prompt_text(label)` / `prompt_text(label, default)`
- `select(label, items)`
- `confirm(label)`
- `multi_select(label, items)`

TemplateService stays ignorant of TTY state — the provider handles detection and fallback. In tests and MCP mode a `NoPromptProvider` supplies deterministic responses.

The provider API is in flux (see [Design considerations](#design-considerations)). Whichever option is chosen, TemplateService receives a single bundling object and destructures it internally — the closure-registration logic is the same either way.

## Acceptance criteria

- [ ] `prompt_text`, `select`, `confirm`, `multi_select` callable from templates and delegate to the interactive provider
- [ ] `prompt_text` supports the optional default argument
- [ ] With `NoPromptProvider`, rendering is deterministic (no TTY required)
- [ ] Tests render templates exercising each function and assert the output

## Rust guidance

Relevant skills: `m03-mutability`, `m02-resource`, `m04-zero-cost`, `m06-error-handling`.

- **Sharing the provider into closures (m02/m03):** each interactive custom function closure needs to call the provider. Closures are `Fn` with shared access and must be `'static` for minijinja, so capture as **`Rc<dyn Provider>`** (single-threaded) or **`Arc<dyn Provider>`** (if Send+Sync needed) and clone into each closure. Prefer `Rc` over threading a borrow — lifetimes won't allow a plain `&dyn` into a `'static` closure.
- **Object safety pays off (m04):** this is exactly why PromptService issue 01 kept the trait object-safe — `Rc<dyn Provider>` / `Arc<dyn Provider>` only works if the trait is object-safe. If a method took generics, it would break here.
- **Error propagation into minijinja (m06):** a prompt failure inside a function must surface as a minijinja `Error` (map the provider's error into `minijinja::Error` with `ErrorKind::InvalidOperation` or similar), so `render` returns `Err` cleanly rather than panicking. Never `unwrap` a prompt result inside a closure.
- **Argument arity:** `prompt_text` has a 1-arg and 2-arg form — minijinja supports optional trailing args; accept `Option<String>` for the default rather than registering two functions.

## Design considerations

The interface the prompt module exposes is being explored. Three options, simplest first:

| Option | Structure | Pros | Cons |
|--------|-----------|------|------|
| **A: Monolithic `PromptProvider` trait** | Single trait with 4 methods (`text`, `confirm`, `select`, `multi_select`). TemplateService holds `Arc<dyn PromptProvider>`, each closure captures the same `Arc`. | One `impl` block per concrete type. One `Arc` allocation. Zero refactoring cost. | Each closure carries vtable entries for all 4 methods when it only needs 1. |
| **B: Split traits + bundling struct** | 4 traits (`TextInputProvider`, `ConfirmProvider`, `SelectProvider`, `MultiSelectProvider`). `InteractiveBackend` struct holds `Arc<dyn T>` for each. TemplateService takes `InteractiveBackend`, each closure captures its own `Arc<dyn SubTrait>`. | Each closure narrow to exactly one method. Self-documenting per-capability traits. Module file structure clearer. | 4 `impl` blocks per concrete type. 4 `Arc` allocations per backend. Extra files. |
| **C: Split traits with blanket supertrait** | Same sub-traits as B, plus `trait PromptProvider: TextInputProvider + ConfirmProvider + SelectProvider + MultiSelectProvider {}` with blanket impl. | Old consumers (`Arc<dyn PromptProvider>`) keep working. New closures can narrow per sub-trait. | Still need `InteractiveBackend` or similar to produce `Arc<dyn SubTrait>` from `Arc<dyn PromptProvider>` (trait upcasting is unstable). Practical outcome identical to B. |

**Recommendation**: Option B with `InteractiveBackend` bundling struct. The module structure would be:

```
src/
├── interact.rs          // module doc + traits + re-exports (2018 idiom, no mod.rs)
└── interact/
    ├── error.rs          // PromptError
    ├── terminal.rs       // TerminalPromptProvider impls
    └── preset.rs         // PresetPromptProvider impls
```

TemplateService receives one `InteractiveBackend` parameter and doesn't know about the split. Each closure captures `Arc<dyn TextInputProvider>` (or whichever) and calls a single method.

**To decide at impl time**: naming of the sub-traits and bundling struct. The grill session converged on `TextInputProvider`, `ConfirmProvider`, `SelectProvider`, `MultiSelectProvider` for the sub-traits and `InteractiveBackend` for the bundling struct — but these are provisional.

## Blocked by

- `.scratch/template-service/issues/01-render-pipeline-tracer.md`
- `.scratch/prompt-service/issues/03-select-and-multi-select.md`
