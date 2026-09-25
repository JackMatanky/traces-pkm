# 08: Docs & decision records

**What to build:** Every behavioral decision this effort made becomes durable documentation instead of tribal knowledge: the glossary states the calendar owner, the duration regime, the interface facts, and the strictness rules; the null-ordering divergence is an ADR, not a code comment; every intentional difference from Dataview/Templater lives in one divergence register; there's one source of truth for the default date format; and all touched public items meet the rust-doc completion bar.

**Blocked by:** 05, 06, 07 (glossary describes final behavior including query functions, template dialects, and remaining parity; 03/04 transitive).

**Status:** ready-for-agent

Skills: `rust-doc`, `writing-for-agents`, `domain-modeling`. Rules: rust-doc completion checklist (one-sentence summaries, real `# Errors` variants, intra-doc links); ADRs via the `adrs` MCP tools start `proposed`; CONTEXT.md entries must not use glossary *Avoid* terms as concept names. Design record: `../review.md` §7 (a–g), §9 (S13); spec stories 35–42.

- [ ] CONTEXT.md clauses (a)–(g): funnel claim incl. deserialization, calendar owner, duration regime (test-backed by ticket 03's pinning test; clause text written here), date interface facts (four-digit year, `YYYY-MM`, precision layer, naive = local → UTC), single `*Avoid*` rule, registry ownership without caveat, strictness-by-seam + ISO Monday + DST policy
- [ ] ADR: null ordering — records **both** halves: null never satisfies an ordering (supersedes the code comment; divergence from Dataview's null-greatest) *and* nulls compare equal to each other / unsortable against values (current sort behavior, pinned by test) — status `proposed`
- [ ] Divergence register records: UTC instants, space-separated datetimes, magnitude-vs-calendar identity, explicit `NoteFieldType` rank, canonical duration Eq, null ordering, ISO Monday weeks, local-naive + DST earliest/gap, strictness-by-seam, strftime dialect
- [ ] Config default-format constant removed; callers use the crate's `DEFAULT_DATETIME_FORMAT`, with the differing-value caveat noted
- [ ] Every public item touched by tickets 01–07 meets rust-doc completion (errors documented, links resolve); `mise run doc --all-features` clean with `-D warnings`
- [ ] `mise run verify` green
