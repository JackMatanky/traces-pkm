# TerminalPromptProvider (inquire + TTY fallback) for text, confirm

Status: done

## Parent

`.scratch/prompt-service/PRD.md`

## What to build

The real terminal implementation. `TerminalPromptProvider` implements `PromptProvider` via `inquire` for `text` and `confirm`. Before prompting it checks `is_terminal()` on stdin; in non-TTY contexts it returns the supplied default without ever calling `inquire`, so templates and `init` render/run without hanging in scripts, dry-run, or CI.

## Acceptance criteria

- [x] `TerminalPromptProvider` implements `text` and `confirm` via `inquire` — `src/prompt.rs`; `text` uses `inquire::Text`, `confirm` uses `inquire::Confirm`, each applying `with_default` only when a default is supplied.
- [x] Non-TTY stdin returns the provided default (or a sensible empty/false fallback) without invoking `inquire` — both methods early-`return Ok(...)` before constructing any prompt; empty-`String` fallback for `text`, `false` for `confirm`.
- [x] Uses the `is-terminal` crate for TTY detection — `stdin_is_tty()` calls `is_terminal::IsTerminal::is_terminal` on `std::io::stdin()`, once per call.
- [x] Tests cover the non-TTY fallback path (real TTY paths are not automatable in CI) — 5 tests (`terminal_*`) assert default/empty/false fallback and `&dyn` usage; each self-skips if stdin is a real TTY so the suite never blocks.

## Rust guidance

Relevant skills: `m06-error-handling`, `m11-ecosystem`, `m13-domain-error`.

- **TTY gate (m13):** the non-TTY branch is a deliberate *fallback*, not an error — return the default `Ok(...)`, never `Err`. Only real `inquire` failures (I/O, user interrupt) map to the trait's error type.
- **inquire error mapping (m06):** convert `inquire::InquireError` into the `PromptProvider` error enum via a `From` impl or `map_err`, so the seam stays crate-agnostic and callers never see `inquire` types. Treat `OperationCanceled`/`OperationInterrupted` as a distinct `Interrupted` variant so a Ctrl-C during a prompt exits cleanly rather than looking like a bug.
- **Detection (m11):** use `is-terminal`'s `IsTerminal` trait on `std::io::stdin()`. Check it once per call before constructing the `inquire` prompt; do not construct the prompt and rely on it to fail.
- **No interior TTY state:** compute TTY-ness on demand; don't cache it in the struct — stdin redirection can differ between calls in tests.

## Blocked by

- `.scratch/prompt-service/issues/01-provider-trait-and-fake.md`

## Implementation notes

Branch `feat/terminal-provider` (worktree `.worktrees/prompt-terminal-provider`), committed `753ba99`. File: `src/prompt.rs` (extended).

- **TTY gate (m13):** `TerminalPromptProvider::text`/`confirm` check `stdin_is_tty()` first. Non-TTY is a *fallback*, not an error — returns `Ok(default)` (empty `String` / `false` when no default), never `Err`. The `inquire` prompt is only constructed on the TTY branch, so scripts/dry-run/CI never hang.
- **inquire error mapping (m06):** added `impl From<inquire::InquireError> for PromptError`. `OperationCanceled | OperationInterrupted -> Interrupted`; `IO -> Io`; `NotTTY`/`InvalidConfiguration`/`Custom -> Backend(String)`. `inquire` types never cross the seam — the TTY branch uses `prompt.prompt()?` and the `?` converts via this impl.
- **`PromptError` changes:** added a `Backend(String)` variant for non-interrupt, non-I/O `inquire` failures (config/custom-validator/NotTTY). Removed the `#[allow(dead_code)]` on `Interrupted` — it is now actually constructed by the mapping.
- **Detection (m11):** `stdin_is_tty()` uses `is-terminal`'s `IsTerminal` trait on `std::io::stdin()`, once per call before building the prompt. Do-not-construct-then-fail satisfied.
- **No interior TTY state:** `TerminalPromptProvider` is a unit struct (`Debug, Default, Clone, Copy`) — TTY-ness is computed on demand, never cached, so stdin redirection can differ between calls (tests rely on this).
- **Send + Sync:** the unit struct trivially satisfies the trait's `Send + Sync` supertrait; guarded by extending `provider_is_send_and_sync` to assert over `TerminalPromptProvider`.
- **Tests:** the 5 `terminal_*` tests each guard with `if stdin_is_tty() { return; }` — real TTY paths can't be automated in CI, and this stops the suite blocking on `inquire` if a dev runs it from an interactive shell. Under `cargo test`/nextest (CI, mise) stdin is redirected so the fallback branch runs.
- **Deps:** added `inquire = "0.9"`, `is-terminal = "0.4"`.
- **Verification:** `cargo nextest run` -> 14 passing (9 prior + 5 new). `cargo clippy --lib --all-targets` clean under the repo's strict lints (only pre-existing package-metadata cargo warnings remain). `rustfmt` (nightly) applied. GitNexus `detect_changes` -> low risk, no affected processes (additive; no consumers wired yet).
- **Committed** on `feat/terminal-provider` as `753ba99`.
