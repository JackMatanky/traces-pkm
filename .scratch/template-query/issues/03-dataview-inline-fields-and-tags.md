# 03 — Dataview Inline Fields and Tags

**What to build:** Markdown Notes index Dataview-compatible Inline Fields and markdown tags from body text and list items. Inline Fields remain in the Note source, but they do not index inside fenced code blocks, indented code blocks, or inline code.

**Blocked by:** 02 — Markdown Note Metadata Extraction

**Status:** ready-for-agent

- [ ] `Key:: Value` body fields are indexed as Inline Fields.
- [ ] `[Key:: Value]` inline fields are indexed as visible-key Inline Fields.
- [ ] `(Key:: Value)` inline fields are indexed as hidden-key Inline Fields.
- [ ] Inline Fields in list items are indexed.
- [ ] Inline Fields inside fenced code blocks, indented code blocks, and inline code are ignored.
- [ ] Markdown tags are indexed for tag source queries.
