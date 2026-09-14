---
number: 3
title: Index-based selection for label-vs-value prompts
status: accepted
date: 2026-07-08
links:
  - target: 6
    kind: relatesto
  - target: 7
    kind: relatesto
  - target: 1
    kind: relatesto
---

# Index-based selection for label-vs-value prompts

## Context and Problem Statement

Both config scaffolding and template rendering need list selection where the
displayed **label** differs from the **value** the caller wants back. The
motivating prior art is Obsidian Templater's `suggester`/`multi_suggester`: two
parallel arrays — display `text_items` and value `items: T[]` — returning the
chosen value(s) as the original object `T` (e.g. pick a file by `.basename`,
get the whole file object). Users select from labels but consume the full
object.

`DialogProvider` is an object-safe trait consumed as `&dyn DialogProvider`
(ADR-1: the minijinja custom-function closures require it). Object-safety
forbids a generic *method*, so a trait method cannot take `&[T]` or return `T`
for arbitrary `T`. The label-vs-value requirement therefore **cannot** be a
trait method — the question is where the `T` handling goes instead.

## Decision Drivers

- Preserve object-safety: `&dyn DialogProvider` must keep working for the
  minijinja closures (ADR-1).
- Support label != value, including recovering a non-string object.
- Handle duplicate display labels correctly (two items may render the same
  label but be distinct values).
- Keep the trait minimal and dependency-light (PRD: "one call, one response").
- Don't add surface with no consumer (YAGNI).

## Considered Options

- **A. Flat `select(&[String]) -> String` only** — no separate object path.
- **B. Input/output enum** — `select` takes `&[String]` or `&[(label, value)]`
  and returns `String`.
- **C. Generic free helper** — `suggest<T>(&dyn DialogProvider, items,
  to_label) -> &T`, generic living off the trait.
- **D. Index-returning trait methods** — `select_index -> usize`; the consumer
  recovers the object by index.

## Decision Outcome

Chosen option: **D, index-returning trait methods**, because it is the only
option that satisfies object-safety, label != value, *and* duplicate-label
correctness. There is exactly **one** selection primitive — no separate
string-returning convenience pair:

- `select(label, &[String]) -> DialogResult<usize>`
- `multi_select(label, &[String]) -> DialogResult<Vec<usize>>`

The methods take display labels and return the chosen **position(s)**. The
caller recovers the entry — a plain string or a richer object it holds in a
parallel list — by indexing with the result. The seam communicates only the
user's *choice*; recovering the value is the caller's job in every case.
`DialogResult<T>` is a `Result<T, DialogError>` alias, allowing error
propagation for empty-list errors, user cancellation, and I/O failures.

The primary consumer, `TemplateService`, inspects a template's input array at
runtime (minijinja values are dynamically typed): for a plain array of strings
it indexes back into that array; for an array of objects it maps each element
to a display label, calls `select`, and recovers the chosen object by the same
index. Because the value collapses to `minijinja::Value` there, no generic is
needed — the dispatch is ordinary value handling, and it is the same call
(`select`) either way.

The index (not a returned label string) is what disambiguates duplicate labels:
a value-returning select cannot tell two identical labels apart, but an index
can. This is also why we did **not** keep a string-returning convenience
overload — two selection methods with different return types invite the
ambiguous one to be used by mistake.

Empty-list contract: `select` on empty `items` returns
`DialogError::EmptySelectionInput` (guard runs before the TTY check); `multi_select`
on empty yields an empty `Vec`. Non-TTY fallback: index `0` (guarded) / empty
selection. `TerminalDialogProvider` reads indices via inquire's `raw_prompt()`
-> `ListOption.index`.

Naming: we use our own vocabulary (`select` / `multi_select`), not Templater's
`suggester`.

### Consequences

- Object-safety preserved; `&dyn DialogProvider` still works for minijinja
  closures.
- No value `T` enters the trait — no generic methods, no enum ceremony, no
  generic free helper to maintain until a non-`Value` Rust caller actually
  needs one.
- One selection concept, not two: every caller gets a position and indexes back
  into the array it already owns. The string-menu case (`select` over labels)
  and the object case use the identical call — the only difference is what the
  caller indexes into.
- Duplicate labels are handled correctly because selection returns a position,
  not a matched string.
- Trade-off: even a plain string menu returns an index the caller must resolve
  (`items[idx]`) rather than the string directly. Accepted in exchange for a
  single, unambiguous selection primitive.
- The caller must keep display labels and values positionally aligned — the
  same contract Templater itself uses.
- If a non-`Value` Rust caller later needs ergonomic object selection, a
  generic `select_object<T>` / `multi_select_objects<T>` free helper can be
  added over this primitive without touching the trait.
- An `is_interactive()` method with a default impl was added to the trait
  (additive, non-breaking).

### Confirmation

Enforced by unit tests in `src/dialog/mod.rs`, `src/dialog/preset.rs`, and
`src/dialog/terminal.rs`:

- `select_index_recovers_the_object_by_position` — maps objects to labels,
  selects by index, recovers the object; asserts `value != label`.
- `select_index_disambiguates_duplicate_labels` — two objects share a label;
  asserts the correct one (by position) is recovered.
- `select_index_on_empty_items_errors` / `multi_select_indices_*` — the
  empty-list and non-TTY-fallback contracts.

Object-safety is guarded by `provider_is_send_and_sync`, which asserts
`Arc<dyn DialogProvider>` — this fails to compile if a generic method is added
to the trait.

## More Information

- Relates to ADR-1 (minijinja lazy interactive custom functions), which
  establishes the `&dyn DialogProvider` object-safety requirement this decision
  must respect.
- Implemented on branch `feat/prompt-select-multiselect`
  (`.scratch/prompt-service/issues/03-select-and-multi-select.md`).
- SelectOptions value recovery lives in `src/template/engine/ui.rs:140-200`.
- Revisit if a non-`Value` Rust consumer needs ergonomic object selection — at
  that point add the option-C helper over the option-D primitive.
