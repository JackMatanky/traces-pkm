# Research: filesystem-watching in Rust, and whether Traces should own one beyond the LSP's needs

Type: research
Blocked by:
Status: resolved

## Question

Ticket [Analysis host architecture & integration with IndexerService/QueryService/SchemaService](10-analysis-host-and-index-integration.md)'s Q2 already settled that `workspace/didChangeWatchedFiles` (client-side OS watching, server registers glob patterns) is the primary mechanism for triggering an LSP-mode index refresh — no server-side filesystem-watcher crate is *required* for that. The user confirmed this but asked for research into filesystem watching more broadly: could Traces owning its own OS-level filesystem watcher (independent of any LSP client) benefit parts of the codebase beyond the LSP session?

Investigate:
- The `notify` crate (and any actively-maintained alternatives, e.g. `notify-debouncer-full`, `watchexec`'s internals) via `rust-docs-mcp` — current API, maturity, platform coverage, debouncing story.
- What capability gap a Traces-owned watcher would fill that `didChangeWatchedFiles` cannot: e.g. a `traces watch`/live-reindex CLI mode with no LSP client involved at all (every CLI invocation today is a fresh short-lived process, per the map's grounded baseline facts — a watch mode would be a first for Traces), a future MCP server (ticket 36) whose clients aren't editors and may not offer any file-watching capability, or watching files outside the client's declared workspace folders.
- Whether any of the studied precedent tools (Markdown Oxide, Marksman, zk, rust-analyzer) bundle their own watcher independent of/alongside LSP-provided watching, and why — cross-check the existing research files ([markdown-oxide.md](../research/markdown-oxide.md), [marksman.md](../research/marksman.md), [zk.md](../research/zk.md), [rust-analyzer-precedent.md](../research/rust-analyzer-precedent.md)) before re-deriving; note zk is SQLite-backed with incremental diff-indexing synced to unsaved buffers — check whether that read on watching already covers this.
- A concrete recommendation: is a Traces-owned watcher worth adopting now (and if so, is it in *this* ticket's scope or a distinct future capability/ticket), or does it not earn its keep until a concrete non-LSP consumer (CLI watch mode, MCP server) is actually being built?

## Answer

**Do not adopt a Traces-owned filesystem watcher for the LSP host.** `notify` (8.2.0) + `notify-debouncer-full` (0.7.0) is the mature, standard Rust choice if/when one is needed, but it doesn't earn its keep for ticket 10: relying on the LSP client's `didChangeWatchedFiles`/`didChange` is simpler and avoids real complexity (race conditions between unsaved-buffer state and disk state, editor-specific atomic-save event noise, background-thread lifecycle) for a capability the editor already provides. Zero precedent among Markdown Oxide, Marksman, zk, or rust-analyzer for a server-bundled watcher running alongside LSP-provided watching — all trust the editor as sync boundary. Real future capability gaps identified for later, non-LSP consumers: a `traces watch` CLI mode (today every CLI invocation is a fresh short-lived process, confirmed via `src/cli/`, no daemon/watch mode exists), and the future MCP server (ticket 36) if its clients turn out not to offer file-watching capability the way editors do. Neither is in scope for ticket 10 — revisit as its own ticket if/when either is actually being built. [Full findings](../research/filesystem-watching-beyond-lsp.md).
