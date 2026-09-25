# 07: Remaining parity — shorthands, parse_with, file.day

**What to build:** The last parity odds and ends land: bucketing shorthands (`sow`/`eow`/`soy`/`eoy`) and `weekday(n)` (ISO Monday) work in templates; ISO-8601 duration offsets (`P1M`, `P-1M`) are accepted on date shorthands like `date.now` by adapter translation, not a second grammar; `reference`/`reference_format`-style parsing goes through one `parse_with(text, fmt)` with a `Result` contract; notes expose `file.day` for day-bucketed views.

**Blocked by:** 04 (shorthands delegate to the calendar owner; `parse_with` builds on the recognition interface).

**Status:** ready-for-agent

Skills: `rust-integration-testing`, `rust-skills`. Rules: edge strategies named per workflow; `conv-tryfrom-fallible`; adapter translation (ISO-8601 offsets) at the shorthand seam, not in the grammar. Design record: `../review.md` §3 (B11, B12, B26/B30; T1, T2, T3), §9 (S9+S11+S12); spec stories 6, 10–11.

- [ ] `sow`/`eow`/`soy`/`eoy` available (B11 — Dataview's query-side bucketing functions, L14960); start of week is ISO Monday
- [ ] `weekday(n)` works with ISO Monday convention (T3)
- [ ] ISO-8601 `P1M`/`P-1M` accepted on date shorthands via adapter translation; `P-1M` internal-sign handling deliberately not ported (T1)
- [ ] `parse_with(text, fmt)` returns `Result` with a documented contract — one implementation serving query `reference` and future LSP needs (B12/T2); no lenient guessing
- [ ] The `reference`/`reference_format` consumer is wired end-to-end through its seam — B12/T2 demonstrated by a user-facing case, not just the library function
- [ ] `file.day` computed at index time, exposed to queries (B26/B30)
- [ ] Integration cases cover each workflow and edge failure (bad format string, missing date field, out-of-range week number) or state an out-of-scope reason
- [ ] `mise run verify` green
