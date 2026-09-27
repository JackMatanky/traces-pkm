# 08: Docs & decision records

**What to build:** Every behavioral decision this effort made becomes durable documentation instead of tribal knowledge: the glossary states the calendar owner, the duration regime, the interface facts, and the strictness rules; the null-ordering divergence is an ADR, not a code comment; every intentional difference from Dataview/Templater lives in one divergence register; there's one source of truth for the default date format; and all touched public items meet the rust-doc completion bar.

**Blocked by:** 05, 06, 07 (glossary describes final behavior including query functions, template dialects, and remaining parity; 03/04 transitive).

**Status:** ready-for-agent

Skills: `rust-doc`, `writing-for-agents`, `domain-modeling`. Rules: rust-doc completion checklist (one-sentence summaries, real `# Errors` variants, intra-doc links); ADRs via the `adrs` MCP tools start `proposed`; CONTEXT.md entries must not use glossary *Avoid* terms as concept names. Design record: `../review.md` §7 (a–g), §9 (S13); spec stories 35–42.

- [ ] CONTEXT.md clauses (a)–(g): funnel claim incl. deserialization, calendar owner, duration regime; calendar frame = local wall clock; day/month/year calendar, sub-hour exact (test-backed by ticket 03's pinning test; clause text written here), date interface facts (four-digit year, `YYYY-MM`, precision layer, naive = local → UTC), single `*Avoid*` rule, registry ownership without caveat, strictness-by-seam + ISO Monday + DST policy (resolver cited to Temporal 'compatible'/RFC 5545)
- [ ] ADR: null ordering — records **both** halves: null never satisfies an ordering (supersedes the code comment; divergence from Dataview's null-greatest) *and* nulls compare equal to each other / unsortable against values (current sort behavior, pinned by test) — status `proposed`
- [ ] Divergence register records: UTC instants, space-separated datetimes, magnitude-vs-calendar identity, explicit `NoteFieldType` rank, canonical duration Eq, null ordering, ISO Monday weeks, local-naive + DST earliest/gap, strictness-by-seam, strftime dialect, DST overlap resolves earlier (ecosystem consensus: Temporal/jiff/BigQuery; PostgreSQL resolves later — outlier), first day of week pinned ISO Monday (elsewhere CLDR locale data), null ordering unique among engines (they disagree anyway: PG nulls-largest, Trino/DuckDB always-last, SQLite first), day/month/year calendar vs sub-hour exact (PG `1 day` ≠ `24 hours`), wasm `Local` caveat (chrono #1701) documented out-of-scope
- [ ] Config default-format constant removed; callers use the crate's `DEFAULT_DATETIME_FORMAT`, with the differing-value caveat noted
- [ ] Every public item touched by tickets 01–07 meets rust-doc completion (errors documented, links resolve); `mise run doc --all-features` clean with `-D warnings`
- [ ] Docs cite the prior-art research base: ../research/sql-temporal-conventions.md, ../research/general-temporal-libraries.md, ../research/rust-temporal-ecosystem.md (linked from spec and review)
- [ ] `mise run verify` green
