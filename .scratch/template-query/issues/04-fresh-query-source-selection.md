# 04 — Fresh Query Source Selection

**What to build:** Query execution refreshes stale FileIndex entries before returning data and can produce page-level QueryOutcome values from all Notes, tag sources, and folder sources.

**Blocked by:** 03 — Dataview Inline Fields and Tags

**Status:** ready-for-agent

- [ ] Query execution refreshes stale entries when file freshness metadata changes.
- [ ] Query execution returns all markdown Notes when no source is specified.
- [ ] Query execution can select Notes by tag source.
- [ ] Query execution can select Notes by folder source.
- [ ] QueryOutcome exposes IndexRecord values with File Record and Note Metadata fields.
- [ ] Tests verify freshness through observable query results, not redb internals.
