# 01: Span position foundation — ByteOffset u32, ByteTracker move, span type decision

**Source:** .scratch/md-pkm-lsp/issues/11-source-span-and-position-model.md (resolved decision)
**What to build:** The shared position primitives that every later span-bearing work depends on: `ByteOffset` narrows from `usize` to `u32` (with `From<u32>`, `TryFrom<usize>` and an overflow error) across all existing call sites; `ByteTracker` relocates to `src/position.rs` and widens to crate-level visibility with zero behaviour change for its current parser caller; and a recorded decision settles the span representation shape — a `ByteSpan` newtype wrapping `Range<ByteOffset>` vs a bare `Range<ByteOffset>` — noting that ticket 19 already writes `ByteSpan(Range<ByteOffset>)` for frontmatter field spans while ticket 11 writes bare `Range` for AST spans, so one consistent shape must be chosen and recorded for ticket 04 and the deferred frontmatter-span work to follow.
**Blocked by:** None (can start immediately)
**Status:** resolved

**Acceptance criteria:**
- [x] `ByteOffset` is `u32`-backed with `From<u32>`, `TryFrom<usize>`, `ByteOffsetError`, and `ByteOffset::MAX`
- [x] `ByteOffset::from(0u32)` and `ByteOffset::try_from(0usize)` succeed; `ByteOffset::try_from` of a `usize` value greater than `u32::MAX` returns `ByteOffsetError`
- [x] `ByteOffset::try_from` of `u32::MAX` widened to `usize` succeeds and equals `ByteOffset::MAX`
- [x] No `From<usize> for ByteOffset` impl remains; all existing call sites updated to the explicit fallible/infallible paths
- [x] Parser pulldown-cmark event boundary saturates via `unwrap_or(ByteOffset::MAX)` (no panic on oversized input)
- [x] No persisted encode path depends on `ByteOffset`'s old width (spans stay transient, never persisted)
- [x] `ByteTracker` lives in `src/position.rs` at `pub(crate)` visibility with its tests moved alongside it, and is constructible from outside the note module
- [x] Parser behaviour unchanged: full existing test suite passes without assertion edits (only import/conversion-mechanics edits allowed); edge cases preserved (empty source → line 1, offset beyond source length → last line, empty lines counted)
- [x] Span-type decision (newtype vs bare range) recorded in this ticket under `## Answer`, with rationale and explicit guidance for ticket 04 and md-pkm-lsp ticket 19's `ByteSpan` mention
- [x] `byte_to_utf16_cu` explicitly NOT added (LSP-wire concern, deferred)
- [x] Project lint and test tasks pass (`mise run verify`)

## Answer

**Decision (2026-09-27): spans are a bare `Range<ByteOffset>` (`std::ops::Range<ByteOffset>`). No wrapper newtype. A `ByteSpan` name, if wanted for readability, may exist only as a type alias — never a tuple struct or newtype.**

**Rationale:**

1. The codebase's newtype discipline (`SourceLine` vs `ByteOffset`) exists to stop *different kinds of positions* being mixed. Every span endpoint is already the same kind of value — `ByteOffset` — so the element type supplies the distinctness; a wrapper over `Range<ByteOffset>` adds no new type-level protection against the errors that discipline guards against (mixing line numbers, offsets, and ranges of either is still caught).
2. A wrapper encodes no invariant worth its cost: it could not guarantee `start <= end` without validation, and carries no other data. Nothing would be made impossible by the type that is possible today.
3. Ergonomics: `.start`/`.end` and the full `Range`/iterator API come free; a newtype needs `Deref`, accessor methods, or trait impls at every use site.
4. Fewest edits to already-written downstream text (the tiebreaker the Agent Brief asked for): md-pkm-lsp ticket 11 (resolved) and md-pkm-lsp tickets 16/17/20 — plus `.scratch/md-pkm-lsp/research/20-designs.md` (`pub(crate) type ByteSpan = std::ops::Range<ByteOffset>;`, explicitly "chosen over wrapper/tuple/newtype alternatives … zero call-site breakage") — already specify the bare range. Note-parse ticket 04 is neutral: it defers entirely to this ticket ("the span type decided in ticket 01", AC "Span type matches ticket 01's recorded decision"). Only md-pkm-lsp ticket 19's `ByteSpan(Range<ByteOffset>)` write-up and `.scratch/md-pkm-lsp/map.md` say newtype; the weight of written text says bare range, and the alias form reconciles the name.

**Guidance for downstream work:**

- **Ticket 04 (AST spans):** declare span fields directly as `Range<ByteOffset>`. Do not introduce a `ByteSpan` newtype. This ticket deliberately introduces **no span type at all** (the first `Range<ByteOffset>` field lands with ticket 04's first consumer, per the Agent Brief's "agent's call" — chose first-consumer).
- **md-pkm-lsp ticket 19 (frontmatter field spans):** its `ByteSpan(Range<ByteOffset>)` notation is superseded by this decision — use bare `Range<ByteOffset>` for frontmatter field spans too, so frontmatter and AST spans share one shape. If the name reads better at those call sites, `type ByteSpan = Range<ByteOffset>` is acceptable; a tuple struct is not.
- Consistency rule: every future span is `Range<ByteOffset>`; any `ByteSpan` identifier in old ticket prose means that alias, not a type.

## Comments

> *This was generated by AI during triage.*

**2026-09-27 — implementation note (post-review):** AC 5's saturation (`unwrap_or(ByteOffset::MAX)`) means that on input ≥ 4 GiB the parser clamps every later pulldown-cmark event to `ByteOffset::MAX`, so `ByteTracker::line_at` reports the same line for everything past the cap — silently misattributed line numbers rather than a panic. This is exactly what AC 5 and the Agent Brief prescribe (no panic on oversized input); flagging for **ticket 04**: if oversized-input correctness ever matters, prefer a parse-time size guard over per-event saturation. Follow-up: the mechanism now lives in `ByteOffset::saturating_from(usize)` (implemented as exactly `try_from(...).unwrap_or(ByteOffset::MAX)`), which the parser boundary calls — done so the saturation behavior is unit-testable (`saturates_oversized_offset_to_max`) without a >4 GiB fixture.

## Implementation

**Landed:** branch `01-span-position-foundation` (base `b80e3ccf`) — `fa75ba3b` (feature) + `94f929f0` (review fixes). `mise run verify` green at `94f929f0`: fmt, check, lint (`--workspace --all-targets --all-features`), 2876 tests, 58 doctests.

**`src/position.rs`** — module doc rewritten as shared conversion infrastructure:

- `pub(crate) struct ByteOffset(u32)`; derives `Copy, Clone, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd, Deserialize, Serialize`. Serde derives kept deliberately — grep audit confirms no persisted encode path references `ByteOffset` outside `position.rs`, `note/parser.rs`, `lib.rs`, so the width change cannot alter stored formats (AC 6).
- API: `const fn new(u32)`; `ByteOffset::MAX`; `saturating_from(usize)` (body exactly `Self::try_from(offset).unwrap_or(Self::MAX)`); infallible `From<u32>`; `TryFrom<usize, Error = ByteOffsetError>` with a `# Errors` section; infallible widening `From<ByteOffset> for u32`/`for usize` (usize path documented as saturating only on <32-bit targets).
- `pub struct ByteOffsetError` — unit struct, thiserror, `byte offset exceeds u32 range`, styled after `SourceLineError` (`Copy` first in derive order per the canonical ordering discipline).
- `ByteTracker` relocated from `src/note/parser/line.rs` (file deleted, 130 lines), zero API/behaviour change: `pub(crate)`, `new(&str)` / `line_at(ByteOffset) -> SourceLine`, `line_starts: Box<[usize]>`; five tests moved verbatim; re-exported at crate level.

**`src/note/parser.rs`:**

- `mod line` removed; imports now `crate::{ByteOffset, ByteTracker, FieldKey, Tag, TaskStatusMap}`; module doc six → five submodules, `mod@line` bullet replaced by a `crate::ByteTracker` sentence.
- Boundary: `ByteOffset::saturating_from(range.start)` replacing infallible `ByteOffset::from(range.start)` (`From<usize>`). AC 5's literal `unwrap_or(ByteOffset::MAX)` now lives inside `saturating_from` — extracted post-review so saturation is unit-testable without a >4 GiB fixture (see Comments).
- `parse_markdown` docs state the 4 GiB clamp (offsets saturate at `u32::MAX`, line numbers clamp rather than fail).
- Zero parser test edits — AC 7's "no assertion edits" holds.

**`src/lib.rs`:** `pub(crate) use position::{ByteOffset, ByteTracker};` (was `position::ByteOffset` only).

**Tests — 15, Structure A per the rust-unit-testing naming spec:** `mod tests` → `mod source_line` (3, legacy names kept), `mod byte_offset` (6), `mod byte_tracker` (6). Coverage: boundary round-trip table `round_trips_u32_boundary_values` (0, 128, `u32::MAX` — folds the old conversions + constructs pair), `narrows_from_usize_within_u32_range` (0 + mid-range), `returns_max_when_narrowing_widened_u32_max` (AC 3), `returns_byte_offset_error_when_usize_exceeds_u32_max` + `displays_exceeds_u32_range_message` (split), `saturates_oversized_offset_to_max` (AC 5 mechanism), `resolves_offset_past_trailing_newline_to_final_line` (tracker edge). Moved tracker tests keep verbatim names and use `SourceLine::MIN`.

**Review passes:** two-axis code-review (standards + spec), then dedicated `rust-unit-testing` and `rust-doc` reviews; all hard findings fixed — derive order, Structure A split, verb-first renames/splits, `# Errors` on `TryFrom`, stale "tracking strategy local" module clause, softened 4 GiB claim, `dictionary.txt` additions (`UTF`, `representable`). Accepted caveat recorded in Comments: >4 GiB input silently misattributes line numbers — flagged for ticket 04.

**Downstream:** `Status: resolved` here is ticket 04's unblock condition (`Blocked by: 01`); span shape for 04 and md-pkm-lsp 19 is bare `Range<ByteOffset>` per `## Answer`.

## Agent Brief

**Category:** enhancement
**Summary:** Narrow `ByteOffset` from `usize` to `u32` with fallible narrowing conversions, relocate `ByteTracker` to the shared position module at crate visibility, and record the span-representation decision (newtype vs bare range).

**Current behavior:**
`ByteOffset` is a newtype over `usize` with infallible `From<usize>` construction, living in the shared position module alongside `SourceLine`. `ByteTracker` — a precomputing byte-offset→line-number converter — lives inside the note parser's line submodule at `pub(super)` visibility, even though it depends only on the shared position vocabulary (`ByteOffset`, `SourceLine`) and its logic (newline counting, binary search over line starts) is domain-general. No span-representation type or decision exists anywhere in the codebase yet, while two downstream tickets assume conflicting shapes: the frontmatter-intelligence ticket writes `ByteSpan(Range<ByteOffset>)`, the AST-spans ticket expects whatever this ticket decides.

**Desired behavior:**
1. `ByteOffset` is backed by `u32` (4 GiB/file cap, consistent with `SourceLine(NonZeroU32)`'s width philosophy):
   - `From<u32>` for infallible construction.
   - `TryFrom<usize>` returning a dedicated `ByteOffsetError` on overflow (values above `u32::MAX`).
   - A `ByteOffset::MAX` associated constant for saturation at overflow-prone boundaries.
   - The note parser's pulldown-cmark event boundary (the main `usize` producer) saturates via `unwrap_or(ByteOffset::MAX)` rather than panicking.
   - The previously-infallible `From<usize>` impl is removed; every construction site is updated to the explicit fallible or infallible path. Widening back out (`ByteOffset` → `u32`/`usize`) stays infallible.
   - If `ByteOffset`'s serde representation is exercised anywhere, its width change must not silently alter a persisted format — spans are transient by standing decision and must never be persisted; verify no persisted encode path depends on the old width.
2. `ByteTracker` moves into the shared position module, widened from `pub(super)` to `pub(crate)`, with its tests moving alongside it. The note parser imports it from its new home. Zero behavior change: identical line-number results for identical inputs (edge cases preserved: empty source → line 1, offset beyond source length → last line, empty lines counted).
3. The position module's doc comment updates to reflect that it now houses shared conversion infrastructure, not only type vocabulary.
4. The span-shape question is settled and recorded **in this ticket** under an `## Answer` heading with rationale: `ByteSpan` newtype wrapping `Range<ByteOffset>` vs bare `Range<ByteOffset>`. One shape must be chosen such that ticket 04 (AST spans) and md-pkm-lsp ticket 19 (frontmatter field spans, which already specifies `ByteSpan`) can both follow it consistently. Consider: type-level distinctness from other ranges (the codebase's established newtype discipline — `SourceLine` vs `ByteOffset`), ergonomic access at call sites, and which choice requires the fewest edits to already-written downstream ticket text.

**Key interfaces:**
- `ByteOffset`: `usize` → `u32` backing; `From<u32>` replaces `From<usize>`; add `TryFrom<usize, Error = ByteOffsetError>` and `ByteOffset::MAX`; keep infallible widening out.
- `ByteOffsetError`: new overflow error type, styled after the existing `SourceLineError` (thiserror, `Eq`, `Copy` where sensible).
- `ByteTracker`: visibility `pub(super)` → `pub(crate)`; module home moves from the note parser's line submodule to the shared position module; API (`new(&str)`, `line_at(ByteOffset) -> SourceLine`) unchanged.
- Note parser: construction site at the pulldown-cmark offset-iter boundary adopts saturating conversion; `line_tracker` field type import path updates.

**Out of scope:**
- `byte_to_utf16_cu` — LSP-wire concern, explicitly deferred (md-pkm-lsp ticket 19 owns adding it when `ByteTracker` gains a consumer needing UTF-16 conversion)
- Adding span fields to any AST type (`Link`, `Tag`, inline fields, `Heading`) — that is ticket 04, which is blocked on this one
- Frontmatter span scanning, ropey integration, LSP position conversion — md-pkm-lsp tickets 19/14/11 follow-on work
- Creating the span newtype itself: this ticket decides and records the shape; whether the type is introduced here or with its first consumer in ticket 04 is the agent's call, noted in the Answer either way
