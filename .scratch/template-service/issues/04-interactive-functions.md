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

- **Storing the provider in UiOps (m02/m03):** `UiOps` holds `Arc<dyn DialogProvider>`. The `Object::get_value` implementation returns callable `Value`s (via `Value::from_function(...)`) for each method name. Each callable captures a clone of the `Arc<dyn DialogProvider>` — object safety pays off here exactly as before. `Rc` would fail `Send` bounds on the callable.
- **Object safety pays off (m04):** `Arc<dyn DialogProvider>` only works because the trait is object-safe. No change from prior decision.
- **Error propagation into minijinja (m06):** same as before — prompt failures surface as `minijinja::Error` so `render` returns `Err` cleanly. Decide the `PromptError` → `ErrorKind` mapping at impl time.
- **Argument arity:** `ui.text_input` has a 1-arg and 2-arg form — accept `Option<String>` for the default.
- **Blocking render model in MCP mode:** unchanged — synchronous render, `PresetDialogProvider` avoids blocking.

## Design considerations

The interface the prompt module exposes is being explored. Three options, simplest first:

| Option | Structure | Pros | Cons |
|--------|-----------|------|------|
| **A: Monolithic `DialogProvider` trait** | Single trait with 4 methods (`text`, `confirm`, `select`, `multi_select`). UiOps holds `Arc<dyn DialogProvider>`, each callable returned by `get_value` captures the same `Arc`. | One `impl` block per concrete type. One `Arc` allocation. Zero refactoring cost. | Each callable carries vtable entries for all 4 methods when it only needs 1. |
| **B: Split traits + bundling struct** | 4 traits (`TextInputProvider`, `ConfirmProvider`, `SelectProvider`, `MultiSelectProvider`). UiOps holds per-capability `Arc<dyn SubTrait>`. | Each callable narrows to exactly one method. | 4 `impl` blocks per concrete type. 4 `Arc` allocations. Extra files. |
| **C: Split traits with blanket supertrait** | Same sub-traits as B, plus `trait DialogProvider: TextInputProvider + ConfirmProvider + SelectProvider + MultiSelectProvider {}`. | Old `Arc<dyn DialogProvider>` consumers keep working. | Trait upcasting is unstable; practical outcome identical to B. |

**Decision**: Option A (monolithic `DialogProvider` trait as already built in `src/dialog/mod.rs`). No change from prior decision — the vtable overhead is negligible.

## Blocked by

- `.scratch/template-service/issues/01-render-pipeline-tracer.md`
- `.scratch/dialog/issues/03-select-and-multi-select.md`

## Comments

- **From issue 03 (dry-run) implementation** (`.worktrees/dry-run`, branch `agent/dry-run`): issue 03's AC4 ("interactive functions return defaults during dry-run, no hang, no TTY required") is blocked on this issue and was left honestly unimplemented — `TemplateEngine`/`TemplateService` don't wire in any `DialogProvider` today (only the `file` namespace is registered, in `src/template/engine.rs`).

  When this issue lands, its design needs to say what provider `TemplateService` uses for a real render vs. a `WriteMode::DryRun` one. The current draft above (line 18) only says *"In tests and MCP mode a `PresetDialogProvider` supplies deterministic responses"* — it doesn't mention dry-run. That gap matters concretely: `TerminalDialogProvider`'s existing non-TTY fallback (`src/dialog/terminal.rs`) only kicks in when stdin genuinely isn't a TTY, so it will **not** force defaults when `--dry-run` runs from an actual interactive terminal — which is exactly what issue 03 requires ("dry-run must not depend on a terminal").

  Two ways to close it: (a) `TemplateService::new` constructs the engine with a `PresetDialogProvider`-equivalent whenever `mode` is `WriteMode::DryRun`, mirroring how `WriteMode` already drives `TemplateWriter::write` (`src/template/writer.rs`); or (b) thread an explicit provider-selection parameter through similarly. Worth adding as an explicit acceptance criterion here — "dry-run selects a defaults-only provider regardless of TTY state" — rather than assuming the non-TTY fallback alone covers it.
