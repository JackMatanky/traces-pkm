# Completion architecture: context detection & dispatch

Type: grilling
Blocked by: 15, 16, 19, 21, 22

## Question

Once the per-feature completion decisions (tickets 15 links, 16 tags, 19 frontmatter/inline-fields, 21 query, 22 template) are each settled, decide the *unifying* completion architecture:

- Context-detection strategy: given a cursor position, how does the LSP decide which of "inside a wikilink target", "inside a tag", "inside frontmatter YAML", "inside an inline field value", "inside a query-DSL string inside a template call", "inside a template helper-namespace call" applies — a single dispatcher walking outward from cursor position through (span-aware, per ticket 11) AST context, or per-context independent trigger-character registration (LSP `completionProvider.triggerCharacters`), or both layered.
- Trigger-character set: consult `docs/refs/lsp_spec.md`'s Completion Request section for how `triggerCharacters`/`triggerKind` work, and enumerate the full set Traces needs (`[`, `#`, `:`, `.`, etc.) checking for collisions with rumdl's own registered link-completion trigger characters (`(`, `#`, `/`, `.`, `-` per the rumdl research, ticket 03) — decide whether Traces needs to coordinate/document which characters it expects rumdl's `enableLinkCompletions` to be turned off for.
- `CompletionItem` resolution strategy: cheap initial list (`textDocument/completion`) vs expensive detail filled lazily (`completionItem/resolve`) —ergonomically relevant for e.g. showing full Schema Field Definition docs only on resolve, not in the initial list.
- Snippet support (`insertTextFormat: Snippet`) for structured insertions (e.g. completing a wikilink with cursor left inside `[[|]]`).
