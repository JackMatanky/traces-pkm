# 07: Remaining parity — shorthands, parse_with, file.day

**What to build:** The last parity odds and ends land: bucketing helpers (`sow`/`eow`/`soy`/`eoy` and start/end-of-month) arrive on both relevant seams — as query `FilterFunction` registry entries (spec story 4's start/end-of-week/month/year helpers) and as template shorthands — and `weekday(n)` (ISO Monday) works in templates; ISO-8601 duration offsets (`P1M`, `P-1M`) are accepted on date shorthands like `date.now` by adapter translation, not a second grammar; `reference`/`reference_format`-style parsing goes through one `parse_with(text, fmt)` with a `Result` contract; notes expose `file.day` for day-bucketed views.

**Blocked by:** 04, 05, 06 (spec parity ordering 05 → 06 → 07; shorthands delegate to the calendar owner — 03, reached via 04; `parse_with` builds on the recognition interface).

**Status:** ready-for-agent

Skills: `rust-integration-testing`, `rust-skills`. Rules: edge strategies named per workflow; `conv-tryfrom-fallible`; adapter translation (ISO-8601 offsets) at the shorthand seam, not in the grammar. Design record: `../review.md` §3 (B11, B12, B26/B30; T1, T2, T3), §9 (S9+S11+S12); spec stories 4, 6, 10–11, 34.

- [ ] Bucketing helpers available as query `FilterFunction` registry entries: `sow`/`eow`/`soy`/`eoy` plus start/end-of-month (B11 — Dataview's query-side bucketing functions, L14960; spec story 4 requires start/end-of-week/month/year); start of week is ISO Monday
- [ ] The same bucketing helpers exposed as template shorthands (`sow`/`eow`/`soy`/`eoy` plus start/end-of-month) — ticket 07 owns both seams per spec story 4 (05 defers them here)
- [ ] `weekday(n)` works with ISO Monday convention (T3)
- [ ] Seam 3 rule: shorthands and `weekday(n)` are tested through the template seam only — never by invoking the engine adapter directly
- [ ] Local-clock display through the template seam: `now`/`today` render the local wall clock while storage stays UTC (spec Seam 3, story 34)
- [ ] ISO-8601 `P1M`/`P-1M` accepted on date shorthands via adapter translation; `P-1M` internal-sign handling deliberately not ported (T1)
- [ ] `parse_with(text, fmt)` returns `Result` with a documented contract — one implementation serving query `reference` and future LSP needs (B12/T2); no lenient guessing
- [ ] The `reference`/`reference_format` consumer is wired end-to-end through its seam — B12/T2 demonstrated by a user-facing case, not just the library function
- [ ] `file.day` computed at index time, exposed to queries (B26/B30)
- [ ] Integration cases cover each workflow and edge failure (bad format string, missing date field, out-of-range week number) or state an out-of-scope reason
- [ ] `sow`/`eow`/`soy`/`eoy` and `weekday(n)` are built from chrono primitives (`NaiveDate::iso_week()` / `from_isoywd_opt` / weekday accessors) — no epoch-day division for weeks (Dataview `.week` footgun)
- [ ] `mise run verify` green
