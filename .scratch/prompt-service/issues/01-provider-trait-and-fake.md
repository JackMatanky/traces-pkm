# PromptProvider trait + NoPromptProvider fake (text, confirm)

Status: done

## Parent

`.scratch/prompt-service/PRD.md`

## What to build

Establish the interactive-input seam. Define a `PromptProvider` trait with `text(label, default)` and `confirm(label, default)`, and ship a `NoPromptProvider` test fake that returns pre-configured responses (or the supplied defaults) with zero I/O. This is the tracer bullet that proves the abstraction works end-to-end — a consumer can hold a `&dyn PromptProvider`, call it, and get deterministic results in tests without a TTY.

`select`/`multi_select` come later (issue 03); the terminal implementation comes in issue 02.

## Acceptance criteria

- [x] `PromptProvider` trait defined with `text` and `confirm` methods returning `Result` — `src/prompt.rs` lines 38-53, both return `Result<_, PromptError>`.
- [x] `NoPromptProvider` implements the trait and returns configured responses (falling back to the provided default when no response is queued) — pops from a per-method queue, `unwrap_or_else` -> call-site default.
- [x] Trait is object-safe (`&dyn PromptProvider` usable by consumers) — `&self` receiver, no generic methods, no `Self` returns; proven by `usable_as_dyn_prompt_provider` test.
- [x] Unit tests verify `NoPromptProvider` returns exactly the configured responses and honors defaults — 8 tests: queued order, queue-then-fallback, default fallback, empty/`false` fallback, `&dyn` usage.
- [x] Lives in its own module/crate with no dependency beyond what the fake needs (no `inquire` yet) — `src/prompt.rs` module; only `thiserror` added.

## Rust guidance

Relevant skills: `m04-zero-cost`, `m06-error-handling`, `m05-type-driven`.

- **Dispatch (m04):** consumers hold one prompt implementation chosen at runtime (terminal vs fake), so this is a genuine dynamic-dispatch case — use `&dyn PromptProvider`, not generics. Keep the trait **object-safe**: no generic methods, no `Self` return, no `Self: Sized` bound. Take `&self` (not `&mut self`) so a shared reference can be passed to both ConfigService `init` and TemplateService functions.
- **Error type (m06):** this is a reusable component, not an app entry point, so return a **typed** error via `thiserror`, not `anyhow`. Give it a small enum (e.g. `Interrupted`, `Io`) so downstream miette layers can categorise. Do not `unwrap`/`expect` on prompt paths.
- **Method shape:** `default: Option<&str>` for `text`; `default: Option<bool>` for `confirm`. Prefer borrowed `&str`/`&[String]` params over owned to keep call sites cheap.
- **Fake determinism:** `NoPromptProvider` holds a queue of configured responses behind interior mutability if `&self` methods must pop (a `RefCell<VecDeque<_>>` is fine here — see `m03-mutability`); when the queue is empty, fall back to the supplied default rather than panicking.

## Blocked by

None - can start immediately

## Implementation notes

Branch `feat/prompt-service`. Files: `src/lib.rs` (new), `src/prompt.rs` (new).

- **Crate promoted to lib.** Added `src/lib.rs` exposing `pub mod prompt`. The seam is a reusable component that ConfigService + TemplateService will both depend on; as a `pub` lib API it isn't dead-code-flagged (a binary-only module would be, since `main` doesn't consume it yet). `main.rs` left as-is (its pre-existing `println!` is the only clippy error and predates this work).
- **Dispatch (m04):** object-safe trait, `&self` receiver, `&dyn PromptProvider`. Borrowed params (`&str`, `Option<&str>`, `Option<bool>`).
- **`Send + Sync` supertrait:** the trait requires `Send + Sync`. The primary consumer, TemplateService, captures the provider into custom-function closures registered on a minijinja `Environment`; minijinja's `Function` trait requires those closures (and everything they capture) to be `Send + Sync + 'static`. Without the bound, an `Arc<dyn PromptProvider>` could not be captured and the integration would fail to compile. A `provider_is_send_and_sync` test guards this. The stateless `TerminalPromptProvider` (issue 02) satisfies it trivially.
- **Error (m06):** `PromptError` typed enum via `thiserror` with `Interrupted` + `Io(#[from] std::io::Error)`. `Interrupted` is unused until issue 02's terminal impl, so it carries `#[allow(dead_code, reason = "...")]` — categories defined up front so downstream miette layers stay stable.
- **Fake determinism (m03):** `NoPromptProvider` uses `Mutex<VecDeque<_>>` per method so `&self` calls can pop *and* the fake stays `Sync` (required by the supertrait above — `RefCell` would make it `!Sync`). Builder API: `.with_text(...)` / `.with_confirm(...)`. Empty queue -> call-site default (`""` for text with no default, `false` for confirm with no default). A private `lock()` helper recovers a poisoned guard via `PoisonError::into_inner` so no `unwrap`/`expect` appears on any path (repo denies both).
- **Deps:** added `thiserror = "2"`. No `inquire`.
- **Verification:** `cargo test --lib` -> 9 passing. `cargo clippy --lib` clean under the repo's strict lints (only remaining warnings are pre-existing package-metadata cargo lints).
- **Not committed** — left staged for review on `feat/prompt-service`.
- **Worktree setup:** added `.worktrees/` to root `.gitignore` (committed on `main` as `chore: ignore .worktrees/`).
