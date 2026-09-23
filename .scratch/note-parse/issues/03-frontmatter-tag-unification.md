# 03: Frontmatter tag unification with configured tag field

**Source:** .scratch/md-pkm-lsp/issues/16-tags-and-hierarchical-tags.md §5 (resolved decision; frontmatter↔body tag unification)
**What to build:** A note's frontmatter tag field is extracted into `Note.tags()` alongside body tags, so `#tag` source expressions, the `PATHS_BY_TAG`/`TAGS_BY_PATH` index axes, and task tag filters all see frontmatter tags (fixing the documented bug where frontmatter tags currently land only as a plain field and are ignored by tag filtering). Per the user's decision: `FrontmatterConfig` gains a field naming which frontmatter key the parser reads tags from, defaulting to `"tags"` — same pattern as the existing `title_name`/`aliases_name` config accessors. Reconcile with md-pkm-lsp ticket 16's broader key list (`tags`, `tag`, `keywords`) by stating in the ticket whether the configured key is the sole source or whether `tag`/`keywords` remain fallbacks; support list-array, scalar-string, and comma-separated values with a leading `#` trimmed. This ticket covers tag VALUES only — frontmatter tag spans are out of scope (deferred with the frontmatter-span design session).
**Blocked by:** None (can start immediately)
**Status:** ready-for-agent
- [ ] `FrontmatterConfig` has a tag-field name accessor defaulting to `"tags"`
- [ ] Parser extracts the configured frontmatter tag key (value forms: list, scalar, comma-separated; leading `#` trimmed) into `Note.tags()` together with body tags
- [ ] Key-precedence question vs ticket 16's `tags`/`tag`/`keywords` list resolved and stated in the ticket
- [ ] `#tag` queries and `PATHS_BY_TAG`/`TAGS_BY_PATH` axis behaviour covered by tests showing frontmatter-sourced tags match
- [ ] Existing tag/task-filter tests still pass; lint passes
