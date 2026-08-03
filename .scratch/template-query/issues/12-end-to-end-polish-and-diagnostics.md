# 12 — End-to-End Polish and Diagnostics

**What to build:** Query failures surface clear diagnostics across Template and CLI usage, and the common workflows from the spec are covered end-to-end before review.

**Blocked by:** 07 — Template Markdown Renderers; 08 — CLI Page Query Commands; 09 — Task-Level Queries; 10 — Derived Inlinks; 11 — Obsidian Wikilink Ambiguity Resolution

**Status:** ready-for-agent

- [ ] Template query errors identify the failing Template context clearly.
- [ ] CLI query errors explain what failed and what the User can do next.
- [ ] End-to-end tests cover indexing, Template QueryOps usage, page CLI queries, task CLI queries, and inlink behavior.
- [ ] Common workflows from the spec are demonstrated through runnable tests or command examples.
- [ ] No implementation ticket leaves redb internals exposed to Template or CLI callers.
