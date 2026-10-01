# 01: Span position foundation — BytePos u32, LineIndex move, span type decision

**Source:** .scratch/md-pkm-lsp/issues/11-source-span-and-position-model.md (resolved decision)
**What to build:** The shared position primitives that every later span-bearing work depends on: `BytePos` narrows from `usize` to `u32` (with `From<u32>`, `TryFrom<usize>` and an overflow error) across all existing call sites; `LineIndex` relocates to `src/position.rs` and widens to crate-level visibility with zero behaviour change for its current parser caller; and a recorded decision settles the span representation shape — a `ByteSpan` newtype wrapping `Range<BytePos>` vs a bare `Range<BytePos>` — noting that ticket 19 already writes `ByteSpan(Range<BytePos>)` for frontmatter field spans while ticket 11 writes bare `Range` for AST spans, so one consistent shape must be chosen and recorded for ticket 04 and the deferred frontmatter-span work to follow.
**Blocked by:** None (can start immediately)
**Status:** resolved

**Acceptance criteria:**
- [x] `BytePos` is `u32`-backed with `From<u32>`, `TryFrom<usize>`, `PositionError`, and `BytePos::MAX`
- [x] `BytePos::from(0u32)` and `BytePos::try_from(0usize)` succeed; `BytePos::try_from` of a `usize` value greater than `u32::MAX` returns `PositionError`
- [x] `BytePos::try_from` of `u32::MAX` widened to `usize` succeeds and equals `BytePos::MAX`
- [x] No `From<usize> for BytePos` impl remains; all existing call sites updated to the explicit fallible/infallible paths
- [x] Parser pulldown-cmark event boundary saturates via `unwrap_or(BytePos::MAX)` (no panic on oversized input)
- [x] No persisted encode path depends on `BytePos`'s old width (spans stay transient, never persisted)
- [x] `LineIndex` lives in `src/position.rs` at `pub(crate)` visibility with its tests moved alongside it, and is constructible from outside the note module
- [x] Parser behaviour unchanged: full existing test suite passes without assertion edits (only import/conversion-mechanics edits allowed); edge cases preserved (empty source → line 1, offset beyond source length → last line, empty lines counted)
- [x] Span-type decision (newtype vs bare range) recorded in this ticket under `## Answer`, with rationale and explicit guidance for ticket 04 and md-pkm-lsp ticket 19's `ByteSpan` mention
- [x] `byte_to_utf16_cu` explicitly NOT added (LSP-wire concern, deferred)
- [x] Project lint and test tasks pass (`mise run verify`)

## Answer

**Decision (2026-09-27): spans are a bare `Range<BytePos>` (`std::ops::Range<BytePos>`). No wrapper newtype. A `ByteSpan` name, if wanted for readability, may exist only as a type alias — never a tuple struct or newtype.**

**Rationale:**

1. The codebase's newtype discipline (`SourceLine` vs `BytePos`) exists to stop *different kinds of positions* being mixed. Every span endpoint is already the same kind of value — `BytePos` — so the element type supplies the distinctness; a wrapper over `Range<BytePos>` adds no new type-level protection against the errors that discipline guards against (mixing line numbers, offsets, and ranges of either is still caught).
2. A wrapper encodes no invariant worth its cost: it could not guarantee `start <= end` without validation, and carries no other data. Nothing would be made impossible by the type that is possible today.
3. Ergonomics: `.start`/`.end` and the full `Range`/iterator API come free; a newtype needs `Deref`, accessor methods, or trait impls at every use site.
4. Fewest edits to already-written downstream text (the tiebreaker the Agent Brief asked for): md-pkm-lsp ticket 11 (resolved) and md-pkm-lsp tickets 16/17/20 — plus `.scratch/md-pkm-lsp/research/20-designs.md` (`pub(crate) type ByteSpan = std::ops::Range<BytePos>;`, explicitly "chosen over wrapper/tuple/newtype alternatives … zero call-site breakage") — already specify the bare range. Note-parse ticket 04 is neutral: it defers entirely to this ticket ("the span type decided in ticket 01", AC "Span type matches ticket 01's recorded decision"). Only md-pkm-lsp ticket 19's `ByteSpan(Range<BytePos>)` write-up and `.scratch/md-pkm-lsp/map.md` say newtype; the weight of written text says bare range, and the alias form reconciles the name.

**Guidance for downstream work:**

- **Ticket 04 (AST spans):** declare span fields directly as `Range<BytePos>`. Do not introduce a `ByteSpan` newtype. This ticket deliberately introduces **no span type at all** (the first `Range<BytePos>` field lands with ticket 04's first consumer, per the Agent Brief's "agent's call" — chose first-consumer).
- **md-pkm-lsp ticket 19 (frontmatter field spans):** its `ByteSpan(Range<BytePos>)` notation is superseded by this decision — use bare `Range<BytePos>` for frontmatter field spans too, so frontmatter and AST spans share one shape. If the name reads better at those call sites, `type ByteSpan = Range<BytePos>` is acceptable; a tuple struct is not.
- Consistency rule: every future span is `Range<BytePos>`; any `ByteSpan` identifier in old ticket prose means that alias, not a type.

## Comments

> *This was generated by AI during triage.*

**2026-09-27 — implementation note (post-review):** AC 5's saturation (`unwrap_or(BytePos::MAX)`) means that on input ≥ 4 GiB the parser clamps every later pulldown-cmark event to `BytePos::MAX`, so `LineIndex::line_at` reports the same line for everything past the cap — silently misattributed line numbers rather than a panic. This is exactly what AC 5 and the Agent Brief prescribe (no panic on oversized input); flagging for **ticket 04**: if oversized-input correctness ever matters, prefer a parse-time size guard over per-event saturation. Follow-up: the mechanism now lives in `BytePos::saturating_from(usize)` (implemented as exactly `try_from(...).unwrap_or(BytePos::MAX)`), which the parser boundary calls — done so the saturation behavior is unit-testable (`saturates_oversized_offset_to_max`) without a >4 GiB fixture.

**2026-09-27 — review round 2 (rust-unit-testing, thorough):** suite audited against the source-derived case surface, then consolidated per table-driven guidance — six `LineIndex` tests with identical bodies became one 16-case rstest; `byte_pos` for-loop tables became rstest cases with scenario names; the duplicated `u32::MAX + 1` boilerplate became a shared fixture plus a `usize::MAX` case. Coverage gaps closed: `SourceLine`/`BytePos` serde round-trips and zero-reject deserialize (locks the AC-6 wire format to plain u32), `PositionError` display, `SourceLine::MIN`, a direct `BytePos::MAX` pin (previously only reachable transitively), ordering, saturating in-range identity, `TryFrom` at `usize::MAX`, and tracker cases for the exact newline byte, exact end-of-source, and CRLF sources. Net: 15 → 40 test instances, verify 2901 green. Only accepted gap: unused `Default` derive (see Implementation).

## Implementation

**Landed:** branch `01-span-position-foundation` (base `b80e3ccf`) — `fa75ba3b` (feature), `94f929f0` (review fixes), `3ff13e0c` (review round 2: rstest consolidation + gap closure). `mise run verify` green at `3ff13e0c`: fmt, check, lint (`--workspace --all-targets --all-features`), 2901 tests, 58 doctests.

**`src/position.rs`** — module doc rewritten as shared conversion infrastructure:

- `pub(crate) struct BytePos(u32)`; derives `Copy, Clone, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd, Deserialize, Serialize`. Serde derives kept deliberately — grep audit confirms no persisted encode path references `BytePos` outside `position.rs`, `note/parser.rs`, `lib.rs`, so the width change cannot alter stored formats (AC 6).
- API: `const fn new(u32)`; `BytePos::MAX`; `saturating_from(usize)` (body exactly `Self::try_from(offset).unwrap_or(Self::MAX)`); infallible `From<u32>`; `TryFrom<usize, Error = PositionError>` with a `# Errors` section; infallible widening `From<BytePos> for u32`/`for usize` (usize path documented as saturating only on <32-bit targets).
- `pub struct PositionError` — unit struct, thiserror, `byte offset exceeds u32 range`, styled after `PositionError` (`Copy` first in derive order per the canonical ordering discipline).
- `LineIndex` relocated from `src/note/parser/line.rs` (file deleted, 130 lines), zero API/behaviour change: `pub(crate)`, `new(&str)` / `line_at(BytePos) -> SourceLine`, `line_starts: Box<[usize]>`; five tests moved verbatim; re-exported at crate level.

**`src/note/parser.rs`:**

- `mod line` removed; imports now `crate::{BytePos, LineIndex, FieldKey, Tag, TaskStatusMap}`; module doc six → five submodules, `mod@line` bullet replaced by a `crate::LineIndex` sentence.
- Boundary: `BytePos::saturating_from(range.start)` replacing infallible `BytePos::from(range.start)` (`From<usize>`). AC 5's literal `unwrap_or(BytePos::MAX)` now lives inside `saturating_from` — extracted post-review so saturation is unit-testable without a >4 GiB fixture (see Comments).
- `parse_markdown` docs state the 4 GiB clamp (offsets saturate at `u32::MAX`, line numbers clamp rather than fail).
- Zero parser test edits — AC 7's "no assertion edits" holds.

**`src/lib.rs`:** `pub(crate) use position::{BytePos, LineIndex};` (was `position::BytePos` only).

**Tests — 40 instances across 16 test fns, Structure A + rstest tables (rust-unit-testing spec):**

- `mod source_line` (5): legacy `source_line_displays_as_its_numeric_value` (fixture hoisted out of the assert), `round_trips_u32_value_through_source_line` (renamed from the `and`-bundled `source_line_conversions_and_accessors`), `source_line_rejects_zero`, plus gap tests `defines_minimum_line_as_one` and `displays_source_line_error_message`.
- `mod serialization` (3): `round_trips_source_line_as_plain_u32`, `rejects_zero_when_deserializing`, `round_trips_byte_offset_as_plain_u32` — the last pins AC 6 (wire format stays plain u32 despite the width change).
- `mod byte_pos` (8 fns / 16 cases): `round_trips_u32_boundary_values` (zero / non_ascii_byte / u32_max), `defines_maximum_offset_as_u32_max` (pins the MAX const, previously only reachable transitively), `narrows_from_usize_within_u32_range` (zero / mid_range / u32_max — absorbs `returns_max_when_narrowing_widened_u32_max`), `returns_byte_offset_error_when_usize_exceeds_u32_max` (one_past_u32_max / usize_max — `usize::MAX` case kills the duplicated `+1` boilerplate, now a shared `offset_past_u32_max()` fixture), `keeps_in_range_offsets_unsaturated` (closes the saturating in-range identity gap), `saturates_oversized_offset_to_max`, `displays_exceeds_u32_range_message`, `orders_offsets_by_numeric_value`.
- `mod line_index` (1 fn × 16 cases): the six former tests collapsed into `resolves_expected_line_for_offset_in_source` (identical `(source, offset) → line` bodies — table-driven per naming spec) with scenario names preserving the old test identities, plus new cases: exact `\n` byte (belongs to the prior line), exact `source.len()` without trailing newline, and a CRLF pair (carriage return stays on its line, line starts after `\n`).
- Accepted (reported, no test): `BytePos::Default` derive has zero callers — decide removal vs a `defaults` test in ticket 04 rather than cementing unused API.

**Review passes:** two-axis code-review (standards + spec), then dedicated `rust-unit-testing` and `rust-doc` reviews; all hard findings fixed — derive order, Structure A split, verb-first renames/splits, `# Errors` on `TryFrom`, stale "tracking strategy local" module clause, softened 4 GiB claim, `dictionary.txt` additions (`UTF`, `representable`). Accepted caveat recorded in Comments: >4 GiB input silently misattributes line numbers — flagged for ticket 04.

**Downstream:** `Status: resolved` here is ticket 04's unblock condition (`Blocked by: 01`). *Correction (2026-10-01):* the span-shape guidance below (bare `Range<BytePos>` as the Answer's choice, "first field lands with ticket 04") predates the position refactor — the codebase landed a `ByteSpan` struct instead (`src/position.rs:86`, `f9d0103b`), and ticket 04 has since been rewritten as **Heading-only** with no spans at all; occurrence spans moved to md-pkm-lsp research ticket 40 and grilling ticket 41, and md-pkm-lsp 19's `ByteSpan` naming is unaffected. The two follow-ups above that aimed at ticket 04 (the >4 GiB line-attribution caveat, and the `BytePos::Default`-now-`BytePos::Default` zero-caller question) are therefore unassigned — standing caveats, not part of 04.

## Agent Brief

**Category:** enhancement
**Summary:** Narrow `BytePos` from `usize` to `u32` with fallible narrowing conversions, relocate `LineIndex` to the shared position module at crate visibility, and record the span-representation decision (newtype vs bare range).

**Current behavior:**
`BytePos` is a newtype over `usize` with infallible `From<usize>` construction, living in the shared position module alongside `SourceLine`. `LineIndex` — a precomputing byte-offset→line-number converter — lives inside the note parser's line submodule at `pub(super)` visibility, even though it depends only on the shared position vocabulary (`BytePos`, `SourceLine`) and its logic (newline counting, binary search over line starts) is domain-general. No span-representation type or decision exists anywhere in the codebase yet, while two downstream tickets assume conflicting shapes: the frontmatter-intelligence ticket writes `ByteSpan(Range<BytePos>)`, the AST-spans ticket expects whatever this ticket decides.

**Desired behavior:**
1. `BytePos` is backed by `u32` (4 GiB/file cap, consistent with `SourceLine(NonZeroU32)`'s width philosophy):
   - `From<u32>` for infallible construction.
   - `TryFrom<usize>` returning a dedicated `PositionError` on overflow (values above `u32::MAX`).
   - A `BytePos::MAX` associated constant for saturation at overflow-prone boundaries.
   - The note parser's pulldown-cmark event boundary (the main `usize` producer) saturates via `unwrap_or(BytePos::MAX)` rather than panicking.
   - The previously-infallible `From<usize>` impl is removed; every construction site is updated to the explicit fallible or infallible path. Widening back out (`BytePos` → `u32`/`usize`) stays infallible.
   - If `BytePos`'s serde representation is exercised anywhere, its width change must not silently alter a persisted format — spans are transient by standing decision and must never be persisted; verify no persisted encode path depends on the old width.
2. `LineIndex` moves into the shared position module, widened from `pub(super)` to `pub(crate)`, with its tests moving alongside it. The note parser imports it from its new home. Zero behavior change: identical line-number results for identical inputs (edge cases preserved: empty source → line 1, offset beyond source length → last line, empty lines counted).
3. The position module's doc comment updates to reflect that it now houses shared conversion infrastructure, not only type vocabulary.
4. The span-shape question is settled and recorded **in this ticket** under an `## Answer` heading with rationale: `ByteSpan` newtype wrapping `Range<BytePos>` vs bare `Range<BytePos>`. One shape must be chosen such that ticket 04 (AST spans) and md-pkm-lsp ticket 19 (frontmatter field spans, which already specifies `ByteSpan`) can both follow it consistently. Consider: type-level distinctness from other ranges (the codebase's established newtype discipline — `SourceLine` vs `BytePos`), ergonomic access at call sites, and which choice requires the fewest edits to already-written downstream ticket text.

**Key interfaces:**
- `BytePos`: `usize` → `u32` backing; `From<u32>` replaces `From<usize>`; add `TryFrom<usize, Error = PositionError>` and `BytePos::MAX`; keep infallible widening out.
- `PositionError`: new overflow error type, styled after the existing `PositionError` (thiserror, `Eq`, `Copy` where sensible).
- `LineIndex`: visibility `pub(super)` → `pub(crate)`; module home moves from the note parser's line submodule to the shared position module; API (`new(&str)`, `line_at(BytePos) -> SourceLine`) unchanged.
- Note parser: construction site at the pulldown-cmark offset-iter boundary adopts saturating conversion; `line_tracker` field type import path updates.

**Out of scope:**
- `byte_to_utf16_cu` — LSP-wire concern, explicitly deferred (md-pkm-lsp ticket 19 owns adding it when `LineIndex` gains a consumer needing UTF-16 conversion)
- Adding span fields to any AST type (`Link`, `Tag`, inline fields, `Heading`) — that is ticket 04, which is blocked on this one
- Frontmatter span scanning, ropey integration, LSP position conversion — md-pkm-lsp tickets 19/14/11 follow-on work
- Creating the span newtype itself: this ticket decides and records the shape; whether the type is introduced here or with its first consumer in ticket 04 is the agent's call, noted in the Answer either way
