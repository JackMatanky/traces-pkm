# Tags & hierarchical tags LSP intelligence

Type: grilling
Blocked by: 01, 02

## Question

Grounding: `Tag` (`src/note/model.rs:29`, `Vec<Tag>` on `Note`, no location tracking today) already models hierarchical sub-tags via prefix matching per `src/CONTEXT.md`'s Tag definition (`#projects/active`). Decide, informed by Markdown Oxide/Marksman tag handling (tickets 01, 02):

- Completion: tag-name completion triggered after `#`, including hierarchical-segment completion (typing `#projects/` completes existing children).
- Definition/references: does "go to definition" make sense for a tag (there's no single defining location), or is "find references" (all notes/occurrences with this tag or a descendant tag) the only applicable capability, with hierarchical query semantics (does `#projects` reference-search include `#projects/active` occurrences)?
- Rename: renaming a tag (workspace-wide edit across all occurrences) — does this need to understand hierarchy (renaming a parent segment should cascade to children, e.g. `#projects` → `#work` renames `#projects/active` → `#work/active`)?
- Symbols: are tags surfaced as workspace symbols?
- Interaction with frontmatter `tags:` list vs inline `#tag` body occurrences — are these the same semantic entity for completion/rename purposes?
