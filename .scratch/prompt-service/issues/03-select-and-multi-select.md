# Add select + multi_select to trait and both providers

Status: done

## Parent

`.scratch/prompt-service/spec.md`

## What to build

Round out the list-based prompts. Add `select(label, items)` and `multi_select(label, items)` to `PromptProvider`, implemented in both `TerminalPromptProvider` (via `inquire`, with non-TTY fallback) and `PresetPromptProvider` (configured responses). `select` is the renamed former `suggester` — use `select` everywhere.

With this slice the trait is complete and every downstream consumer (ConfigService `init`, TemplateService functions) has the full prompt surface available.

## Acceptance criteria

- [x] `select` returns the user's chosen item; `multi_select` returns the chosen set — resolved to **index-based** selection: `select(label, &[String]) -> usize` and `multi_select(label, &[String]) -> Vec<usize>` (the caller recovers the item by indexing). Both object-safe (concrete param/return types, no method generics). See the ADR-3 follow-up below for why the return is a position, not the string.
- [x] Both implemented in `TerminalPromptProvider` (inquire `Select`/`MultiSelect` via `raw_prompt()`) and `PresetPromptProvider` (queued indices via `.with_select` / `.with_multi_select`).
- [x] Non-TTY `select`/`multi_select` return sensible defaults without calling `inquire` — `select` returns index `0` (guarded), `multi_select` returns an empty `Vec`; both early-return before constructing any inquire prompt.
- [x] Tests verify `PresetPromptProvider` returns configured selections and the non-TTY fallback for the terminal provider — 10 selection tests (33 total passing), including object-recovery-by-index and duplicate-label disambiguation.

## Implementation notes

Branch `feat/prompt-select-multiselect` (worktree `.worktrees/prompt-select-multiselect`, base `main` @ `97c115d`). File: `src/prompt.rs` (extended). Committed on the feature branch.

- **Empty-list contract (m06):** added a `PromptError::EmptyOptions` variant. `select` on an empty slice returns `Err(EmptyOptions)` (guarded via a `let [first, ..] = items else` slice pattern) in **both** providers — never indexes `[0]`, never panics. The guard runs *before* the TTY check, so the terminal provider errors on empty input regardless of TTY state (covered by `terminal_select_on_empty_items_errors`, which needs no TTY skip). `multi_select` on an empty slice yields an empty `Vec`, not an error.
- **Object-safety (m04):** methods take `&[String]`, return `String` / `Vec<String>` — no method generics, `&self` receiver — so `&dyn PromptProvider` still holds (the existing `usable_as_dyn_*` tests exercise the widened trait).
- **Non-TTY fallback:** `select` → first item (only reachable when non-empty, since the empty guard precedes it); `multi_select` → empty selection. Both branches early-`return Ok(...)` before touching `inquire`.
- **No new inquire mapping:** `inquire::Select`/`MultiSelect` fail with the same `InquireError` already mapped by the issue-02 `From` impl, so `?` on `.prompt()` reuses it (`OperationCanceled`/`Interrupted` → `Interrupted`). The only clone of the `items` slice (`items.to_vec()`) happens on the TTY branch, which inquire requires to own its option list.
- **Fake API:** `PresetPromptProvider` gains `selects: Mutex<VecDeque<String>>` + `multi_selects: Mutex<VecDeque<Vec<String>>>` and builders `.with_select(S: Into<String>)` / `.with_multi_select(IntoIterator<Item = Into<String>>)`. Empty queue → same non-TTY fallback as the terminal provider (first item / empty vec), keeping fake and real consistent.
### Follow-up: index-based selection (ADR-3)

The list-based prior art (Obsidian Templater's `suggester`/`multi_suggester`) selects by a display **label** but returns the chosen **object**, not the label string (e.g. pick a file by basename, get the whole file). A `&[String] -> String` seam cannot express this, and an object-safe `&dyn` trait cannot return a generic `T` (object-safety forbids generic methods). Resolution (see `docs/adr/0003-*.md`): **`select`/`multi_select` return the chosen position(s)**, and the caller recovers the value by indexing — the same call for both string menus and object selection.

- **Trait, two selection methods:** `select(label, &[String]) -> usize` and `multi_select(label, &[String]) -> Vec<usize>`. Both return positions; the caller indexes back into the array it owns (labels for a string menu, objects for the object case). The index — not a returned string — disambiguates duplicate labels, which a value-returning select cannot. No separate string-returning overload: one unambiguous primitive.
- **No generic helper.** The consumer is TemplateService, which works in `minijinja::Value` (dynamically typed) — it inspects the template's input array (strings vs objects) and, in both cases, calls `select` and indexes back into the input. No generic `T` API is needed or added. (A `select_object<T>` free helper over this primitive can be added later if a non-`Value` Rust caller needs it.)
- **Our vocabulary, not Templater's:** methods are `select`/`multi_select` — no `suggester`, no `_index` suffix.
- **Both providers:** `TerminalPromptProvider` reads indices via inquire's `raw_prompt()` -> `ListOption.index` (relative to the original list); non-TTY fallback `0` (guarded) / empty `Vec`. `PresetPromptProvider` queues indices via builders `.with_select(usize)` / `.with_multi_select(IntoIterator<Item = usize>)`, same empty-queue fallback.
- **Empty-list contract:** `select` on empty `items` -> `EmptyOptions` (guard before the TTY check); `multi_select` on empty -> empty `Vec`.
- **No `items[idx]` indexing in lib code:** the providers never index the slice (terminal returns inquire's index; preset returns a queued/`0` index), staying clear of the repo's `-D clippy::indexing_slicing`. Tests that recover an object by returned index use `.get(idx)`.

- **Verification:** `cargo nextest run` → **33 passing** (23 baseline + 10 selection). `cargo clippy --all-targets -- -D warnings` clean for `prompt.rs`; the only failures are pre-existing in `src/config.rs` (present on base `HEAD`, other merged work — not introduced here). `cargo +nightly fmt` applied. `RUSTDOCFLAGS="-D warnings" cargo doc --no-deps` clean. GitNexus `detect_changes` → low risk, no affected processes (additive; no consumers wired yet).

### Design history

Arrived at the final shape by exploring the option space rather than committing to the first idea:

1. Rejected a `PromptSelectItems` enum — can't return an object without a generic-in-`T` (breaks `&dyn`) or `Box<dyn Any>` (worse than an index).
2. Prototyped and validated a generic `suggest<T>` helper (compiled + passed, including duplicate-label disambiguation) but removed it as premature since TemplateService dispatches over `minijinja::Value`, not generic `T`.
3. Briefly carried *both* a string-returning `select`/`multi_select` and an index-returning `select_index`/`multi_select_indices` pair (committed as `ba971bf`), then collapsed to **index-only** — the string pair was redundant once the object path proved indices are the primitive, and two selection methods with different return types invite the ambiguous one to be used by mistake.

Landed on: a single index-returning `select`/`multi_select` primitive on the trait; the consumer indexes back into its own array.

## Rust guidance

Relevant skills: `m04-zero-cost`, `m06-error-handling`.

- **Keep the trait object-safe (m04):** `select`/`multi_select` take `&[String]` and return `String` / `Vec<String>` — concrete types, no generics on the method — so `&dyn PromptProvider` still holds.
- **Empty-list edge case (m06):** decide the contract for an empty `items` slice explicitly. `select` on zero items cannot return an item — return an `Err` variant (e.g. `EmptyOptions`) rather than panicking or indexing `[0]`. `multi_select` on an empty slice returns an empty `Vec`.
- **Non-TTY fallback:** for `select`, returning the first item is only valid when the list is non-empty — guard it. For `multi_select`, the non-TTY default is an empty selection.
- **`inquire::Select`/`MultiSelect`** return the chosen value(s); map their error the same way as issue 02 (`Interrupted` for cancellation). Avoid cloning the whole `items` slice if `inquire` can borrow it.

## Blocked by

- `.scratch/prompt-service/issues/01-provider-trait-and-fake.md`
- `.scratch/prompt-service/issues/02-terminal-provider.md`
