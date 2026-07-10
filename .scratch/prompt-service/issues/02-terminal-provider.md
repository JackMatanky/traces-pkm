# TerminalPromptProvider (inquire + TTY fallback) for text, confirm

Status: done

## Parent

`.scratch/prompt-service/spec.md`

## What to build

The real terminal implementation. `TerminalPromptProvider` implements `PromptProvider` via `inquire` for `text` and `confirm`. Before prompting it checks `is_terminal()` on stdin; in non-TTY contexts it returns the supplied default without ever calling `inquire`, so templates and `init` render/run without hanging in scripts, dry-run, or CI.

## Acceptance criteria

- [x] `TerminalPromptProvider` implements `text` and `confirm` via `inquire` — `src/prompt.rs`; `text` uses `inquire::Text`, `confirm` uses `inquire::Confirm`, each applying `with_default` only when a default is supplied.
- [x] Non-TTY stdin returns the provided default (or a sensible empty/false fallback) without invoking `inquire` — both methods early-`return Ok(...)` before constructing any prompt; empty-`String` fallback for `text`, `false` for `confirm`.
- [x] Uses the `is-terminal` crate for TTY detection — `stdin_is_tty()` calls `is_terminal::IsTerminal::is_terminal` on `std::io::stdin()`, once per call.
- [x] Tests cover the non-TTY fallback path (real TTY paths are not automatable in CI) — 5 tests (`terminal_*`) assert default/empty/false fallback and `&dyn` usage; each skips *visibly* (via `skip_if_tty`, which logs a notice) if stdin is a real TTY so a local run never blocks and never reports a silent green pass. 3 further tests cover the `InquireError -> PromptError` source-chain mapping.

## Rust guidance

Relevant skills: `m06-error-handling`, `m11-ecosystem`, `m13-domain-error`.

- **TTY gate (m13):** the non-TTY branch is a deliberate *fallback*, not an error — return the default `Ok(...)`, never `Err`. Only real `inquire` failures (I/O, user interrupt) map to the trait's error type.
- **inquire error mapping (m06):** convert `inquire::InquireError` into the `PromptProvider` error enum via a `From` impl or `map_err`, so the seam stays crate-agnostic and callers never see `inquire` types. Treat `OperationCanceled`/`OperationInterrupted` as a distinct `Interrupted` variant so a Ctrl-C during a prompt exits cleanly rather than looking like a bug.
- **Detection (m11):** use `is-terminal`'s `IsTerminal` trait on `std::io::stdin()`. Check it once per call before constructing the `inquire` prompt; do not construct the prompt and rely on it to fail.
- **No interior TTY state:** compute TTY-ness on demand; don't cache it in the struct — stdin redirection can differ between calls in tests.

## Blocked by

- `.scratch/prompt-service/issues/01-provider-trait-and-fake.md`

## Implementation notes

Branch `feat/terminal-provider` (worktree `.worktrees/prompt-terminal-provider`). File: `src/prompt.rs` (extended). Reflects the final state after an ordering pass, a best-practices review, and a rename (see Commit history below).

- **TTY gate (m13):** `TerminalPromptProvider::text`/`confirm` check `stdin_is_tty()` first. Non-TTY is a *fallback*, not an error — returns `Ok(default)` (empty `String` / `false` when no default), never `Err`. The `inquire` prompt is only constructed on the TTY branch, so scripts/dry-run/CI never hang.
- **inquire error mapping (m06):** `impl From<inquire::InquireError> for PromptError`. `OperationCanceled | OperationInterrupted | NotTTY -> Interrupted`; `IO -> Io`; `InvalidConfiguration | Custom -> Backend`. `inquire` types never cross the seam — the TTY branch uses `prompt.prompt()?` and the `?` converts via this impl. `NotTTY` is unreachable behind the TTY guard, so it folds into `Interrupted` rather than inventing a message.
- **`PromptError` shape (err-source-chain):** `Interrupted`; `Io(#[source] std::io::Error)`; `Backend(#[source] Box<dyn Error + Send + Sync>)`. `Backend` carries the backend error as a boxed **source** (not a stringified message), so miette/anyhow layers can walk the chain while `inquire`'s concrete type stays out of the public API. `Io` uses `#[source]` + a manual `From<std::io::Error>` (instead of `#[from]`) so the `Display` message doesn't duplicate the source. `?` ergonomics are unchanged. The old `#[allow(dead_code)]` on `Interrupted` is gone — it is now constructed by the mapping.
- **Detection (m11):** `stdin_is_tty()` uses `is-terminal`'s `IsTerminal` trait on `std::io::stdin()`, once per call before building the prompt. Do-not-construct-then-fail satisfied.
- **No interior TTY state:** `TerminalPromptProvider` is a unit struct (`#[derive(Copy, Clone, Debug, Default)]`, Copy-first per derive-ordering) — TTY-ness is computed on demand, never cached, so stdin redirection can differ between calls (tests rely on this).
- **Send + Sync:** the unit struct trivially satisfies the trait's `Send + Sync` supertrait; guarded by extending `provider_is_send_and_sync` to assert over `TerminalPromptProvider`.
- **Ordering discipline:** per `docs/refs/rust/best_practices_canonical/08_ordering_discipline.md`, the primary impl (`TerminalPromptProvider`) is ordered above the preset provider (`PresetPromptProvider`); free helpers (`stdin_is_tty`, `lock`) sit below the impl blocks that use them rather than interrupting a type's impl group.
- **Docs (err-doc-errors):** both trait `# Errors` sections list all three `PromptError` variants with intra-doc links. `RUSTDOCFLAGS="-D warnings" cargo doc --no-deps` is clean.
- **Tests:** the 5 `terminal_*` tests skip via `skip_if_tty(name)` — which `eprintln!`s a visible skip notice — instead of a silent `return`, so an interactive local run doesn't report a green pass with nothing asserted. Real TTY paths still can't be automated in CI; under `cargo test`/nextest (CI, mise) stdin is redirected so the fallback branch runs. Test module uses a single `use super::*;` (test-use-super). 3 source-chain tests assert `cancel -> Interrupted`, `IO` preserves `.source()`, and `Custom` preserves `.source()`.
- **Deps:** added `inquire = "0.9"`, `is-terminal = "0.4"`.
- **Verification:** `cargo nextest run` -> **17 passing** (9 from issue 01 + 5 fallback + 3 source-chain). `cargo clippy --all-targets -- -D warnings` clean for `prompt.rs` (only pre-existing package-metadata `cargo` lints on `Cargo.toml` remain — not introduced here). `rustfmt` (nightly) applied. GitNexus `detect_changes` -> low risk, no affected processes (additive; no consumers wired yet).

## Commit history (on `feat/terminal-provider`)

- `753ba99` feat: `TerminalPromptProvider` with non-TTY fallback + `InquireError` mapping.
- `6bae2c8` docs: this issue file marked done.
- `f814acf` refactor: apply ordering discipline (helpers below impls, Copy-first derives).
- `680f703` refactor: order `TerminalPromptProvider` above `NoPromptProvider`.
- `535cdbb` fix: preserve error source chain (`Backend` boxed `#[source]`), `NotTTY -> Interrupted`, complete `# Errors` docs, visible test skips, single `use super::*;`, 3 source-chain tests.
- `2c239de` test: tighten `Io` source-chain assertion, apply AAA spacing.
- rename: `NoPromptProvider` -> `PresetPromptProvider` (unintuitive "No" prefix; the type serves non-interactive/MCP mode, not just tests). spec + issue 03 updated to the new name.
