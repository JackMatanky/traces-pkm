# TerminalPromptProvider (inquire + TTY fallback) for text, confirm

Status: ready-for-agent

## Parent

`.scratch/prompt-service/PRD.md`

## What to build

The real terminal implementation. `TerminalPromptProvider` implements `PromptProvider` via `inquire` for `text` and `confirm`. Before prompting it checks `is_terminal()` on stdin; in non-TTY contexts it returns the supplied default without ever calling `inquire`, so templates and `init` render/run without hanging in scripts, dry-run, or CI.

## Acceptance criteria

- [ ] `TerminalPromptProvider` implements `text` and `confirm` via `inquire`
- [ ] Non-TTY stdin returns the provided default (or a sensible empty/false fallback) without invoking `inquire`
- [ ] Uses the `is-terminal` crate for TTY detection
- [ ] Tests cover the non-TTY fallback path (real TTY paths are not automatable in CI)

## Rust guidance

Relevant skills: `m06-error-handling`, `m11-ecosystem`, `m13-domain-error`.

- **TTY gate (m13):** the non-TTY branch is a deliberate *fallback*, not an error — return the default `Ok(...)`, never `Err`. Only real `inquire` failures (I/O, user interrupt) map to the trait's error type.
- **inquire error mapping (m06):** convert `inquire::InquireError` into the `PromptProvider` error enum via a `From` impl or `map_err`, so the seam stays crate-agnostic and callers never see `inquire` types. Treat `OperationCanceled`/`OperationInterrupted` as a distinct `Interrupted` variant so a Ctrl-C during a prompt exits cleanly rather than looking like a bug.
- **Detection (m11):** use `is-terminal`'s `IsTerminal` trait on `std::io::stdin()`. Check it once per call before constructing the `inquire` prompt; do not construct the prompt and rely on it to fail.
- **No interior TTY state:** compute TTY-ness on demand; don't cache it in the struct — stdin redirection can differ between calls in tests.

## Blocked by

- `.scratch/prompt-service/issues/01-provider-trait-and-fake.md`
