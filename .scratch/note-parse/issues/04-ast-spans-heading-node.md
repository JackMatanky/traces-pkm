# 04: AST spans and Heading node

**Source:** .scratch/md-pkm-lsp/issues/11-source-span-and-position-model.md §1–§2 (+ §8 persistence constraint) (resolved decision)
**What to build:** The Note AST starts retaining positions: `Link`, `Tag`, and inline-field entries carry source ranges captured from the offset iter the parser already emits (start AND end, currently discarded), using the span type decided in ticket 01; and headings — currently not retained at all — become a flat `Heading` node carrying its level, enabling later heading-reference and symbol work. Spans stay transient: they are never persisted to the index store, so the encode path must exclude them. Explicitly out of scope: frontmatter span storage (deferred to a dedicated design session), `byte_to_utf16_cu`, and any consumer-facing feature (symbols, heading references) — this ticket only lands the data.
**Blocked by:** 01/Span position foundation — ByteOffset u32 and the span-type decision must land first
**Status:** ready-for-agent
- [ ] `Link`, `Tag`, and inline-field AST entries carry source ranges (start and end) populated during parse
- [ ] Flat `Heading` node with level retained in the AST output; existing behaviour for other nodes unchanged
- [ ] Span type matches ticket 01's recorded decision
- [ ] Persistence round-trip excludes spans (index store encode path unchanged / spans skipped), verified by test
- [ ] Parser tests and lint pass
