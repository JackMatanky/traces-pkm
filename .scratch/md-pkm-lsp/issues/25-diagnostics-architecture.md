# Diagnostics architecture: sourcing, publishing, and rumdl non-overlap

Type: grilling
Blocked by: 03, 15, 20, 21

## Question

Once link/reference resolution (ticket 15), schema validation (ticket 20), and query diagnostics (ticket 21) are each settled, decide the *unifying* diagnostics architecture:

- Aggregation: how per-feature diagnostic sources (unresolved links, ambiguous headings, schema violations, malformed queries, malformed templates) get merged into one `textDocument/publishDiagnostics` payload per document, with what severity levels and diagnostic-code namespacing (so a client/user can distinguish a Traces diagnostic from a rumdl diagnostic on the same line — consult the rumdl boundary research, ticket 03, for its diagnostic-code convention as precedent to avoid collision).
- Push (`publishDiagnostics`) vs pull (`textDocument/diagnostic`, LSP 3.17+) model — check `docs/refs/lsp_spec.md` for the pull-diagnostics capability and decide which Traces implements (or both, gated by client capability negotiation, ticket 29).
- Debounce/recompute triggers: on every `didChange` keystroke vs `didSave` vs a settle-timer — bounded by the performance targets ticket (33) and the concurrency model (ticket 12).
- Workspace-wide vs open-document-only diagnostics: does an unresolved-link diagnostic fire for every note in the workspace on startup (expensive, but complete) or only for currently-open documents (cheap, but backlink-target notes that reference a broken link elsewhere stay silent until opened)?
- Explicit non-overlap with rumdl: confirm from ticket 03's findings which diagnostic categories rumdl already owns (its MD-rule lint catalogue) and ensure Traces' diagnostic set has zero overlap by design, documenting the split for users configuring both servers.
