# File operations: note rename/move/delete cascading link updates

Type: grilling
Blocked by: 13, 26

## Question

Once rename mechanics (ticket 26) and the LSP persistence/caching strategy (ticket 13) are settled, decide file-operation handling per `docs/refs/lsp_spec.md`'s Workspace File Operations section (`workspace/willRenameFiles`, `workspace/didRenameFiles`, `workspace/willDeleteFiles`, `workspace/didDeleteFiles`, and the corresponding client capability registration):

- On note rename/move (client-initiated, e.g. via the editor's file explorer): does Traces respond to `willRenameFiles` with a `WorkspaceEdit` that rewrites every inbound wikilink/Markdown-link target across the workspace (using the existing Inlink graph, `src/index/inlinks.rs`, as the starting point for "which files need edits"), applied atomically alongside the rename?
- On delete: does `willDeleteFiles` produce warnings/diagnostics for now-broken inbound links (there's nothing to rewrite them *to*), or a code action offering bulk clean-up?
- Server-capability registration: `didCreateFiles`/`didRenameFiles`/`didDeleteFiles` registration filters (glob patterns) scoped to Markdown files.
- Interaction with the index: does a file-operation-triggered `IndexerService::refresh` need to run before or after computing the cascading `WorkspaceEdit` (ordering matters: the edit must be computed against the *pre-rename* Inlink graph, then applied, then the index refreshed to reflect the new state).
