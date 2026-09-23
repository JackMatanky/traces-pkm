# 02: Frontmatter parser swap to noyalib

**Source:** .scratch/md-pkm-lsp/issues/19-frontmatter-and-inline-field-intelligence.md (resolved decision) + .scratch/md-pkm-lsp/issues/11-source-span-and-position-model.md §6
**What to build:** Frontmatter YAML is parsed through `noyalib` (via its serde_yaml compatibility layer) with `YamlVersion::V1_1` enabled for Dataview compatibility, replacing the current `serde_yaml` call, with existing valid-YAML behaviour preserved and the YAML-1.1 semantic differences covered by regression tests. The current silent-empty-frontmatter behaviour on malformed YAML stays or improves (structured parse failure surfaced to the caller) — state which in the ticket. Note: md-pkm-lsp ticket 19 lists ticket-11 changes as its prerequisite, but that gate governs the spans half, which is out of scope here; this swap is orthogonal and may run in parallel with 01.
**Blocked by:** None (can start immediately)
**Status:** ready-for-agent
- [ ] Frontmatter parsing routes through noyalib with V1_1; the frontmatter path no longer calls `serde_yaml`
- [ ] All existing frontmatter tests pass unchanged (valid-YAML behaviour preserved)
- [ ] Regression tests cover YAML-1.1 semantics differences relevant to real vault content
- [ ] Malformed-YAML behaviour explicitly stated (silent-empty kept vs structured error surfaced) and tested
- [ ] Tests and lint pass
