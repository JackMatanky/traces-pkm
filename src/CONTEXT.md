# Core

Shared types and entry points for Traces. Covers `lib.rs` (module declarations,
public re-exports) and `main.rs` (process exit code mapping), plus the loose
root-level files.

## Language

### User

The human operating the CLI tool. In MCP mode, the AI agent acts on the user's behalf.
*Avoid*: Client, operator

### Directory Tree

The shared traversal vocabulary behind the FileIndex scan, Schema registry load,
config subtree discovery, and Template Directory listing:
`dirtree::children(dir)` reads a directory's immediate entries;
`dirtree::descendants(root)` walks a whole tree (`skipping` prunes subtrees).
Both yield **DirNodes** and classified **Dir Tree Errors** — `MissingRoot`,
`RootInaccessible`, `NodeInaccessible` — whose degrade-or-fail policy each
caller states explicitly in its match arms. *Avoid*: walker, walk adapter,
DirEntry
