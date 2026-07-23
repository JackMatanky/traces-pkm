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

## Items parameter handling (select / multi_select)

`items` accepts `Value` (not `Vec<String>`) so templates can pass either:

```jinja
{{ ui.select("pick", ["a", "b", "c"]) }}
{{ ui.select("pick", [{label: "US", value: 1}, {label: "GB", value: 44}]) }}
```

**As implemented, this went further than the algorithm originally
sketched here.** Instead of a hardcoded `"label"` key, `select`/
`multi_select` take two optional kwargs — deliberately named and
shaped to match minijinja's own `map`/`sort`/`groupby` filters, not
invented fresh:

- `attribute` (string, default `"label"`): a dot-separated path
  (e.g. `"address.city"`) walked via a locally reimplemented
  `get_path` (minijinja's own `Value::get_path` is `pub(crate)`,
  not importable) — numeric segments index by position via
  `Value::get_item_by_index`, other segments look up an attribute
  via `Value::get_attr`.
- `default` (any `Value`, optional): stringified and used as the
  label when an item's attribute is undefined. Without it, a missing
  attribute falls back to `item.to_string()` — this is what makes a
  plain `["a", "b", "c"]` array keep working with no `attribute=` at
  all, since a bare string has no `"label"` attribute.

```jinja
{{ ui.select("pick", items, attribute="name") }}
{{ ui.select("pick", items, attribute="address.city") }}
{{ ui.select("pick", items, attribute="name", default="Unnamed") }}
```

The closure (`src/template/ui_ops.rs::label_items`):
1. Extracts `attribute`/`default` from a trailing `Kwargs` parameter,
   then `kwargs.assert_all_used()` — an unknown third kwarg is a
   `minijinja::Error` (`ErrorKind::TooManyArguments`), not silently
   ignored.
2. Calls `items.try_iter()` to iterate, propagating errors via
   `minijinja::Error`.
3. For each item, walks the attribute path; if the resolved value is
   undefined, uses `default` (stringified) or `item.to_string()`;
   otherwise stringifies the resolved value directly (so a non-string
   attribute, e.g. a number, renders as itself, not the whole item).
4. Collects labels into `Vec<String>`, calls
   `provider.select(label, &labels)`.
5. Returns the original `Value` at the returned index (or `Vec<Value>`
   for `multi_select` — minijinja converts to a seq automatically).

Still one `Value::from_function` closure per method; no `Object`
override needed.

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

- **Implementation** (`.worktrees/interactive-functions`, branch `agent/interactive-functions`, commit `4746264`): `UiOps` (`src/template/ui_ops.rs`, new file) backs the `ui` namespace exactly as designed — Option A monolithic `DialogProvider`, `Arc<dyn DialogProvider>` cloned per method closure, `Object::get_value` + `Value::from_function`, no custom `call_method`. `DialogProvider` gained a `Debug` supertrait bound (`src/dialog/mod.rs`) because minijinja's own `Object` trait requires it and `UiOps` couldn't otherwise derive it; both concrete providers already derived `Debug`, so this cost nothing. `select`/`multi_select`'s label derivation is richer than originally sketched — see the rewritten "Items parameter handling" section above (`attribute=`/`default=` kwargs, dotted-path support), added after the user asked for `rust-docs-mcp` research into minijinja's own conventions before committing to a design; it stays backward compatible with the plain-array and hardcoded-`"label"` cases this issue specified.

  **Unfulfilled acceptance criterion — deliberate, not an oversight:** *"Dry-run mode selects a defaults-only provider (not `TerminalDialogProvider`) — resolves issue 03's known gap"* (line 152, Agent Brief's AC list only — the top-level AC list at line 22 never added this bullet in the first place, per the original "Known gap" comment below) is **not** implemented as worded. Discussed with the user mid-implementation: forcing empty defaults on every `--dry-run` makes the preview useless for any template whose output branches on a `ui.select`/`ui.confirm` answer — exactly the "conditional interaction" case ADR 0001 exists to support. Instead:
  - `WriteMode` now decides only whether output is written, never whether `ui.*` prompts (`TemplateService` builds one `TemplateEngine` at construction from whatever provider it's given, and uses it unconditionally for every render — no more per-render provider swap keyed on `WriteMode::DryRun`).
  - A new, independent `--no-input` flag (`src/cli/template.rs`) forces a defaults-only `PresetDialogProvider`, regardless of `--dry-run` or TTY state — this is what actually satisfies issue 03's original "no hang, no TTY required" need, just decoupled from `--dry-run` rather than fused to it. The flag name and semantics ("If `--no-input` is passed, don't prompt or do anything interactive") come directly from `docs/refs/cli_guide.md`'s Interactivity section, at the user's direction.
  - `TerminalDialogProvider`'s pre-existing non-TTY fallback (`src/dialog/terminal.rs`) still covers the common case — `--dry-run` from CI/scripts/pipes gets defaults automatically, with no flag needed — so `--no-input` is only strictly necessary for a scripted `--dry-run` run from an *actual* interactive terminal.
  - Net effect: `--dry-run` alone now previews with real answers when stdin is a real TTY; `--dry-run --no-input` (or `--no-input` alone, for a real write) reproduces the originally-specified forced-defaults behavior. If a defaults-only dry-run needs to stay the *default* (no flag), rather than opt-in via `--no-input`, that's a follow-up decision, not something this implementation reverts.
  - Verified: `template::service::tests::render_to_file::dry_run_still_uses_the_injected_provider_for_ui_calls` (real answers survive a dry-run preview), `cli::template::tests::run_no_input_ignores_the_injected_provider_and_uses_defaults` / `run_uses_the_injected_providers_queued_answer_by_default` (the flag's actual effect), plus a manual smoke test of the release binary confirming no terminal is touched under `--no-input`/non-TTY stdin.

  Every other acceptance criterion in both lists is met: all four methods callable and delegating; `text_input`'s optional default; deterministic rendering under `PresetDialogProvider`; `select`/`multi_select` accept `Value` items with label detection and `to_string()` fallback (now generalized via `attribute=`); tests render real template source through each method (`template::service::tests::render_to_file::ui_functions_render_and_delegate_to_the_provider` and siblings). 351 tests total, `cargo clippy --workspace -- -D warnings` clean, `hk check` clean.

  Incidental changes this required: `WriteFlags` (`src/cli/template.rs`) — `force`/`dry_run` flattened into their own `#[command(flatten)]` sub-struct once `--no-input` pushed `Template` to a third `bool` field, past this crate's `max-struct-bools = 2` clippy threshold. `TemplateLoader` is no longer a `TemplateService` field — it's consumed directly by `TemplateEngine::new` inside `TemplateService::new`, since engine construction reverted to happening once (not per-render) now that provider selection doesn't depend on `WriteMode`.

- **Adversarial re-review** (`.worktrees/interactive-functions`, same branch, on top of `b739649`): ran the `code-review` skill (Standards + Spec axes, parallel sub-agents) plus a dedicated `rust-skills` idiom pass and a `rust-unit-testing` coverage audit against the implementation above. One real defect found and fixed, plus four idiom/design/coverage improvements:
  - **Bug (fixed):** `get_path` (`src/template/ui_ops.rs`) called `Value::get_attr`/`Value::get_item_by_index` on an already-`Value::UNDEFINED` intermediate, which minijinja 2.21.0 itself errors on (`ErrorKind::UndefinedError`; confirmed by reading minijinja's actual source via rust-docs-mcp, not just its docs). Concretely: `attribute="address.city"` against an item with no `address` key at all didn't fall back to `default=` — it hard-failed the *entire* `select`/`multi_select` call with a raw `UndefinedError`, regardless of whether a `default=` was supplied. `get_path` now short-circuits to `Value::UNDEFINED` as soon as a segment resolves to undefined, matching how minijinja's own `map`/`groupby` filters degrade gracefully. Regression tests: `get_path_resolves_to_undefined_when_an_intermediate_segment_is_missing`, `label_items_falls_back_to_default_for_a_dotted_path_missing_an_intermediate_segment`; verified end-to-end with a release-binary smoke test rendering `ui.select(..., attribute="address.city", default="Unknown")` against a mixed item list.
  - **API-surface fix:** the `DialogProvider: std::fmt::Debug` supertrait bound added for the original implementation (see the "Implementation" comment above — that claim is now superseded) has been removed. It widened a public trait's contract for every implementor just so `UiOps` could `#[derive(Debug)]`; `UiOps` now hand-writes `impl Debug` (`f.debug_struct("UiOps").finish_non_exhaustive()`) instead, so `DialogProvider` is back to `Send + Sync` only.
  - **Idiom fixes:** `dialog_error` no longer duplicates the dialog error's message (it previously set both the minijinja `Error`'s `detail` *and* its `.with_source()` to the same text, which double-prints when a caller renders the full chain, per minijinja's own recommended chain-rendering pattern) — the detail is now a stable `"dialog provider failed"`, with the real message still available via `.source()`. `label_items` now pre-sizes its two output `Vec`s with `Vec::with_capacity(items.len().unwrap_or(0))` instead of growing from empty.
  - **Readability fixes:** `src/template/ui_ops.rs` reordered so `UiOps` and its impls read first (the public API), with helper consts/types/functions below, per this repo's ordering convention; `indexed()` renamed to `recover_indexed_value` (a verb phrase, not a bare adjective).
  - **Coverage gaps closed:** added tests for `ui.confirm`'s hardcoded `None`-default fallback, `recover_indexed_value`'s out-of-range-index error path (both `select` and `multi_select`, via a misbehaving preset index), `get_path`'s numeric-segment (`get_item_by_index`) branch, an unknown kwarg rejected through the *full* `select` closure call (not just at the `label_items` unit level), and a `Debug`-formatting sanity check for `UiOps`'s new hand-written impl.
  - Not changed: `UiOps::new` staying `const fn` (harmless either way — reviewers disagreed on whether it's meaningful, nobody found it wrong) and the already-documented, deliberate dry-run/`--no-input` provider-selection design (re-verified against current source, still accurate as described above).
  - Verified: 359 unit tests (was 353, +6), 1 integration test, 10 doctests, `cargo clippy --workspace -- -D warnings` clean, `hk check` clean except a pre-existing, unrelated `cargo fmt` drift in `src/config/store.rs` confirmed present on `main` itself (not touched by this issue's scope).

- **Acceptance-criteria audit** (`.worktrees/interactive-functions`, same branch, on top of `4b34fbd`): re-verified every item in both checklists individually against current source and tests — not against this file's own narrative claims — since the checklists themselves were left unchecked throughout (by design; see below) and earlier comments summarized status in prose rather than auditing line by line.
  - **Top-level checklist (line 24-27):** all four items met. Callable + delegates: `service.rs::ui_functions_render_and_delegate_to_the_provider` renders real template source (`{{ ui.text_input(...) }}|{{ ui.confirm(...) }}|{{ ui.select(...) }}|{{ ui.multi_select(...) }}`) and asserts output — covers "tests render templates exercising each method" too. `text_input` optional default: `text_input_accepts_an_optional_default`. Deterministic under `PresetDialogProvider`/no TTY: every test uses it; `--no-input` smoke-tested against the release binary.
  - **Agent Brief checklist (line 158-163):** same four items met, plus two Agent-Brief-only bullets:
    - *"`ui.select`/`ui.multi_select` accept `Value` items, detect `label`/`value` keyed items, fall back to `to_string()`"* — **met, generalized.** `ui_select_and_multi_select_render_keyed_items_and_fall_back_to_to_string` proves `{label, value}` items resolve `.value` after `select`, and a bare `[10, 20, 30]` falls back to `to_string()`. The implementation doesn't literally special-case a `"value"` key — it returns the whole original `Value` for any item shape, so `.value`, `.name`, or any other field works post-selection; the `attribute=` kwarg (default `"label"`) generalizes what the AC called the hardcoded `"label"` key. Wording's intent satisfied by a strictly more capable mechanism.
    - *"Dry-run mode selects a defaults-only provider... resolves issue 03's known gap"* (line 161) — **confirmed still unfulfilled, still deliberate.** Re-checked directly against current `TemplateService::new`/`render_to_file` (unconditional single provider regardless of `WriteMode`) and `dry_run_still_uses_the_injected_provider_for_ui_calls` (proves real answers survive a `--dry-run` preview). No regression from the adversarial-review fixes above — this is the one acceptance criterion, across both checklists, not met as literally worded. `--no-input` is the actual mechanism that satisfies the underlying need (issue 03's "no hang, no TTY required"), just decoupled from `--dry-run` — see the "Implementation" comment above for the full tradeoff discussion.
  - **Checklists intentionally left unchecked:** per explicit instruction carried through this issue's implementation sessions, the checkbox syntax in both "## Acceptance criteria" (line 22) and the Agent Brief's own list (line 157) was never edited — only narrative sections and this `## Comments` section were updated. `Status:` (line 3) was likewise left at `ready-for-agent`, the only value in this repo's canonical five (`docs/agents/triage-labels.md`) that doesn't imply completion; the tracker's label vocabulary has no "done"/"implemented" role, so this comment section is the source of truth for completion state instead.

## Agent Brief

> *This was generated by AI during triage.*

**Category:** enhancement
**Summary:** Register `ui.*` interactive functions (`text_input`, `select`, `confirm`, `multi_select`) on the minijinja `Environment` as a namespace object delegating to `Arc<dyn DialogProvider>`.

**Current behavior:**
`TemplateEngine` registers only the `file` namespace. No `DialogProvider` is wired in. Interactive functions called from templates produce a minijinja `UndefinedError`.

**Desired behavior:**
A `ui` namespace object (implementing `minijinja::value::Object`) is registered via `env.add_global("ui", ...)`, holding an `Arc<dyn DialogProvider>`. The four methods are callable from templates, each delegating to the corresponding `DialogProvider` method. `ui.text_input` supports an optional default argument (`Option<String>`). `ui.select` and `ui.multi_select` accept `Value` items (iterable, with label/value key detection). In dry-run mode, `TemplateService` selects a `PresetDialogProvider` (or equivalent) so defaults are returned without TTY involvement — see known gap below.

**Key interfaces:**
- `src/template/engine.rs` — register `ui` namespace; `TemplateEngine` needs `Arc<dyn DialogProvider>` at construction
- `src/template/service.rs` — wire provider selection: dry-run → defaults provider, else the one from CLI setup
- `src/dialog/mod.rs` — `DialogProvider` trait (monolithic Option A, 4 methods: `text`, `confirm`, `select`, `multi_select`)
- `src/dialog/preset.rs` — `PresetDialogProvider` for deterministic responses (already exists per spec)

**Acceptance criteria:**
- [ ] `ui.text_input`, `ui.select`, `ui.confirm`, `ui.multi_select` callable from templates and delegate to `DialogProvider`
- [ ] `ui.text_input` supports optional default argument
- [ ] `ui.select`/`ui.multi_select` accept `Value` items, detect `label`/`value` keyed items, fall back to `to_string()`
- [ ] Dry-run mode selects a defaults-only provider (not `TerminalDialogProvider`) — resolves issue 03's known gap
- [ ] With `PresetDialogProvider`, rendering is deterministic (no TTY required)
- [ ] Tests render templates exercising each method and assert output

**Known gap (dry-run):**
This issue's AC does not yet include a dry-run provider criterion. The implementor must choose between: (a) `TemplateService` builds a `PresetDialogProvider`-equivalent when `WriteMode == DryRun`, or (b) an explicit provider-selection parameter is threaded through. See prior comment from issue 03's implementation notes for full context.

**Out of scope:**
- Adding new `DialogProvider` methods beyond the four listed
- Multi-pass rendering or batch prompting (deferred per spec)
- MCP-mode variable injection (separate ticket needed per spec)
