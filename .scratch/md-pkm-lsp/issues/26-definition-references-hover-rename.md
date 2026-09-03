# Definition, references, hover, rename & workspace-edit behavior

Type: grilling
Blocked by: 11, 15, 16, 19

## Question

Once link/reference (15), tag (16), and frontmatter/inline-field (19) semantics are settled, and the span model (11) is in place, decide the shared mechanics every entity kind's navigation/edit capability rides on:

- A single internal "resolvable entity" abstraction (if warranted — per the "introduce abstractions only for real boundaries" preference, decide whether links/tags/headings/fields genuinely share enough resolution/edit shape to warrant one, or whether four independent, simpler implementations are actually less code) covering: locate-at-position, list-references, compute-rename-edit.
- `WorkspaceEdit` construction for rename: does Traces use `documentChanges` (versioned, LSP 3.16+ preferred form) or the plain `changes` map — check `docs/refs/lsp_spec.md`'s WorkspaceEdit section; decide `TextDocumentEdit` version-check policy given multiple files may be touched by a single rename (e.g. renaming a tag across 40 notes).
- Hover content: `MarkupContent` kind (`plaintext` vs `markdown`) and what's shown per entity kind (resolved path + first heading for a link target; resolved File Class fields for a schema-bound note; etc.), negotiated against client `hoverProvider`/`markdown` capability.
- Prepare-rename (`textDocument/prepareRename`) validity checks — e.g. refusing to offer rename when the cursor isn't actually on a renamable entity, or when the target is outside any confined root.
