# 01: Span position foundation — ByteOffset u32, ByteTracker move, span type decision

**Source:** .scratch/md-pkm-lsp/issues/11-source-span-and-position-model.md (resolved decision)
**What to build:** The shared position primitives that every later span-bearing work depends on: `ByteOffset` narrows from `usize` to `u32` (with `From<u32>`, `TryFrom<usize>` and an overflow error) across all existing call sites; `ByteTracker` relocates to `src/position.rs` and widens to crate-level visibility with zero behaviour change for its current parser caller; and a recorded decision settles the span representation shape — a `ByteSpan` newtype wrapping `Range<ByteOffset>` vs a bare `Range<ByteOffset>` — noting that ticket 19 already writes `ByteSpan(Range<ByteOffset>)` for frontmatter field spans while ticket 11 writes bare `Range` for AST spans, so one consistent shape must be chosen and recorded for ticket 04 and the deferred frontmatter-span work to follow.
**Blocked by:** None (can start immediately)
**Status:** ready-for-agent
- [ ] `ByteOffset` is `u32`-backed with explicit conversions and an overflow error; all existing call sites updated; tests and lint pass
- [ ] `ByteTracker` lives in `src/position.rs` at crate visibility; parser behaviour unchanged
- [ ] Span-type decision (newtype vs bare range) recorded in the ticket as resolved, with rationale, consistent with md-pkm-lsp ticket 19's `ByteSpan` mention
- [ ] `byte_to_utf16_cu` explicitly NOT added (LSP-wire concern, deferred)
