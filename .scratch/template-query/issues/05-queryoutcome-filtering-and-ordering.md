# 05 — QueryOutcome Filtering and Ordering

**What to build:** QueryOutcome supports enough query transformations for Template and CLI callers to narrow, order, group, and flatten indexed Notes.

**Blocked by:** 04 — Fresh Query Source Selection

**Status:** ready-for-agent

- [ ] QueryOutcome can filter Notes with `where` or `filter` expressions.
- [ ] QueryOutcome can sort Notes by indexed fields.
- [ ] QueryOutcome can limit the number of returned rows.
- [ ] QueryOutcome can group Notes by indexed fields.
- [ ] QueryOutcome can flatten nested list-like metadata.
- [ ] Invalid transformation inputs produce clear errors rather than panics.
