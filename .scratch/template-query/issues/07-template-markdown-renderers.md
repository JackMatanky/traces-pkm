# 07 — Template Markdown Renderers

**What to build:** Templates can render QueryOutcome values as markdown tables, markdown lists, markdown task lists, and counts through terminal methods and pipeline filters. Template loops remain the escape hatch for custom value transformations.

**Blocked by:** 06 — QueryOps Template Namespace, 09 — Task-Level Queries

**Status:** ready-for-agent

- [ ] QueryOutcome can render a markdown table from field path columns.
- [ ] QueryOutcome can render a markdown list from field path values.
- [ ] Task-level QueryOutcome values can render markdown task lists.
- [ ] QueryOutcome can render a count.
- [ ] Terminal renderers work as QueryOutcome methods.
- [ ] Terminal renderers work as pipeline filters.
- [ ] Tests show `{% for %}` loops can format transformed values that terminal renderers do not handle.
