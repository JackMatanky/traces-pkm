# Research: filesystem-watching in Rust, and whether Traces should own one beyond the LSP's needs

Type: research
Blocked by:
Status: completed

## Question

Ticket 10's Q2 already settled that `workspace/didChangeWatchedFiles` (client-side OS watching, server registers glob patterns) is the primary mechanism for triggering an LSP-mode index refresh — no server-side filesystem-watcher crate is *required* for that. The user confirmed this but asked for research into filesystem watching more broadly: could Traces owning its own OS-level filesystem watcher (independent of any LSP client) benefit parts of the codebase beyond the LSP session?

Investigate:
- The `notify` crate (and any actively-maintained alternatives, e.g. `notify-debouncer-full`, `watchexec`'s internals) via `rust-docs-mcp` — current API, maturity, platform coverage, debouncing story.
- What capability gap a Traces-owned watcher would fill that `didChangeWatchedFiles` cannot: e.g. a `traces watch`/live-reindex CLI mode with no LSP client involved at all (every CLI invocation today is a fresh short-lived process, per the map's grounded baseline facts — a watch mode would be a first for Traces), a future MCP server (ticket 36) whose clients aren't editors and may not offer any file-watching capability, or watching files outside the client's declared workspace folders.
- Whether any of the studied precedent tools (Markdown Oxide, Marksman, zk, rust-analyzer) bundle their own watcher independent of/alongside LSP-provided watching, and why — cross-check the existing research files before re-deriving.
- A concrete recommendation: is a Traces-owned watcher worth adopting now (and if so, is it in *this* ticket's scope or a distinct future capability/ticket), or does it not earn its keep until a concrete non-LSP consumer (CLI watch mode, MCP server) is actually being built?

## Answer

### 1. Ecosystem (`notify` and `notify-debouncer-full`)
The Rust ecosystem has a clear, mature standard for filesystem watching:
- **`notify`**: Provides cross-platform filesystem notification. It abstracts over native OS APIs (`fsevent` on macOS, `kqueue` on BSD/macOS, `inotify` on Linux, and `ReadDirectoryChangesW` on Windows) and falls back to a polling watcher (`PollWatcher`) where native events aren't available. It is mature, actively maintained, and handles the low-level event streams.
- **`notify-debouncer-full`**: Because OS-level file events are notoriously noisy (e.g., saving a file often triggers a sequence of `Create`, `Write`, `Remove`, `Rename` events depending on the editor's "safe write" behavior), `notify` alone is usually insufficient for application logic. `notify-debouncer-full` sits on top of `notify` to coalesce these rapid, noisy OS events into single semantic events. It includes a `FileIdCache` to correctly track files across renames and atomic-save swaps.

### 2. Capability Gap Filled
A Traces-owned, independent filesystem watcher would fill specific gaps that an LSP's `didChangeWatchedFiles` cannot:
- **CLI Daemon / Watch Mode**: The Traces CLI (`src/cli/`) currently only runs one-shot commands (`index`, `list`, `table`). A `traces watch` command would allow users to run a long-lived process in the terminal that incrementally reindexes the project as they edit files in an editor that isn't connected to the LSP (e.g., standard Vim, a basic text editor, or Obsidian without the plugin).
- **MCP Server (Ticket 36)**: Model Context Protocol (MCP) clients (like Claude Desktop) are not guaranteed to have rich, workspace-wide file watching capabilities like LSP editors (VS Code, Neovim) do. An MCP server might need to watch the `.traces` directory or the project root itself to keep its internal index fresh and notify the client of updated resources.
- **Out-of-Workspace Files**: If Traces eventually needs to watch a global config file (e.g., `~/.config/traces/`) or an external referenced vault that the editor doesn't consider part of the active workspace, `didChangeWatchedFiles` cannot help.

### 3. Precedent from Other Tools
Cross-checking the research on other PKM tools confirms that bundling a custom file watcher *inside* an LSP is generally avoided:
- **Markdown Oxide**: Relies exclusively on the LSP client (`did_change_watched_files` for disk changes, `did_change` for buffer changes). It has no independent watcher. In fact, on `did_change_watched_files`, it just drops the whole vault and rebuilds it from disk.
- **Marksman**: Operates on an eager scan at startup and then trusts the LSP client's `didChange`, `didOpen`, and `didClose` events to mutate the in-memory state. No independent OS watcher is mentioned.
- **zk**: Uses an embedded SQLite database. It syncs state by manually diffing a filesystem `Walk` against the database's `IndexedPaths()`, and leverages the LSP `TextDocumentSyncKindIncremental` for unsaved buffer changes. It does not actively watch the filesystem in the background outside of explicit triggers.
- **rust-analyzer**: Uses a VFS (Virtual File System). Within an LSP context, the VFS is the *sole sync boundary* and is fed entirely by the editor's notifications. `rust-analyzer` explicitly treats the editor as the source of truth to avoid race conditions between unsaved buffers and disk reads.

### 4. Concrete Recommendation
**Do not adopt a Traces-owned filesystem watcher for the LSP host (Ticket 10).**

The lazy, correct path (YAGNI) is to rely entirely on the LSP client (`didChangeWatchedFiles` and `didChange`) for the LSP server. Duplicating file-watching server-side introduces severe complexities: race conditions between the editor's unsaved buffer state and the disk state, handling editor-specific atomic save maneuvers, and managing background thread lifecycles, all for a capability the editor *already provides*. 

**When to adopt it:**
A `notify`-based watcher does not earn its keep until a concrete, **non-LSP consumer** is actually being built. It should remain strictly out of scope for Ticket 10. 

It should only be introduced in a distinct future ticket when building:
1. A `traces watch` CLI mode.
2. The MCP Server (Ticket 36), *if* it is determined that MCP clients cannot adequately watch the filesystem themselves. 

**Summary**: Defer `notify` and `notify-debouncer-full`. Stick to `didChangeWatchedFiles` for the Analysis Host.
