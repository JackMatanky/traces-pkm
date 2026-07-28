# 02 — Markdown Note Metadata Extraction

**What to build:** Markdown Notes add Note Metadata to the FileIndex using markdown structure from `pulldown-cmark` events. The first complete vertical slice should index YAML metadata blocks, basic tasks, lists, wikilinks/outlinks, and the parser state needed to exclude code regions from later Inline Field extraction.

**Blocked by:** 01 — FileIndex Baseline

**Status:** ready-for-agent

- [ ] Markdown Notes store Note Metadata in addition to their File Record.
- [ ] YAML metadata blocks are extracted from markdown Notes.
- [ ] Basic task markers are extracted with completion status.
- [ ] List and list item structure is extracted enough to support later task/list queries.
- [ ] Wikilinks or markdown links are indexed as outlinks.
- [ ] Parser tests use `pulldown-cmark` events, including code block and inline code events.
