# 06 — QueryOps Template Namespace

**What to build:** Templates can use a QueryOps namespace to query the FileIndex through method chaining. QueryOutcome values can be iterated, indexed, counted, and passed into existing Interactive Functions.

**Blocked by:** 05 — QueryOutcome Filtering and Ordering

**Status:** ready-for-agent

- [ ] The minijinja environment exposes a `query` namespace Object.
- [ ] QueryOps methods return QueryOutcome Objects that support method chaining.
- [ ] Templates can iterate QueryOutcome values with `{% for %}`.
- [ ] Templates can index into QueryOutcome values by integer position.
- [ ] Templates can inspect QueryOutcome length.
- [ ] QueryOutcome values can be passed to `ui.select` and `ui.multi_select` with `attribute=` labels.
- [ ] Query method failures surface as render errors with useful template context.
