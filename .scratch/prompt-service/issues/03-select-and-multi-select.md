# Add select + multi_select to trait and both providers

Status: ready-for-agent

## Parent

`.scratch/prompt-service/PRD.md`

## What to build

Round out the list-based prompts. Add `select(label, items)` and `multi_select(label, items)` to `PromptProvider`, implemented in both `TerminalPromptProvider` (via `inquire`, with non-TTY fallback) and `NoPromptProvider` (configured responses). `select` is the renamed former `suggester` — use `select` everywhere.

With this slice the trait is complete and every downstream consumer (ConfigService `init`, TemplateService functions) has the full prompt surface available.

## Acceptance criteria

- [ ] `select` returns one chosen item from the list; `multi_select` returns a `Vec` of chosen items
- [ ] Both implemented in `TerminalPromptProvider` (inquire) and `NoPromptProvider` (fake)
- [ ] Non-TTY `select`/`multi_select` return sensible defaults without calling `inquire` (e.g. first item / empty selection)
- [ ] Tests verify `NoPromptProvider` returns configured selections and the non-TTY fallback for the terminal provider

## Rust guidance

Relevant skills: `m04-zero-cost`, `m06-error-handling`.

- **Keep the trait object-safe (m04):** `select`/`multi_select` take `&[String]` and return `String` / `Vec<String>` — concrete types, no generics on the method — so `&dyn PromptProvider` still holds.
- **Empty-list edge case (m06):** decide the contract for an empty `items` slice explicitly. `select` on zero items cannot return an item — return an `Err` variant (e.g. `EmptyOptions`) rather than panicking or indexing `[0]`. `multi_select` on an empty slice returns an empty `Vec`.
- **Non-TTY fallback:** for `select`, returning the first item is only valid when the list is non-empty — guard it. For `multi_select`, the non-TTY default is an empty selection.
- **`inquire::Select`/`MultiSelect`** return the chosen value(s); map their error the same way as issue 02 (`Interrupted` for cancellation). Avoid cloning the whole `items` slice if `inquire` can borrow it.

## Blocked by

- `.scratch/prompt-service/issues/01-provider-trait-and-fake.md`
- `.scratch/prompt-service/issues/02-terminal-provider.md`
