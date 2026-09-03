# Multi-root / multi-workspace behavior vs the single Project Root model

Type: grilling
Blocked by: 10

## Question

Traces' existing model is a single Project Root (`.traces/`-anchored, discovered by walking up from cwd — `src/config/discovery.rs`) with a two-tier Configuration Scope (global defaults overridden by local project config, verified by Trust Verification against a companion hash before loading). LSP's `workspace/workspaceFolders` supports multiple simultaneous roots in one client session. Decide, once the analysis-host design (ticket 10) is settled:

- Whether the LSP server runs one analysis host per workspace folder (N independent `FileIndex`/`SchemaService`/etc. instances, each with its own Trust Verification and Config Scope) or requires/assumes exactly one workspace folder and errors/degrades otherwise.
- How Trust Verification interacts with LSP's initialization flow: the CLI today refuses to load an Untrusted/Stale local config outright — decide the LSP-mode equivalent (block initialization for that folder? show a diagnostic/notification prompting the user to run `traces trust`? never auto-trust from the LSP path for security reasons?).
- `workspace/didChangeWorkspaceFolders` handling: adding/removing a root at runtime spins up/tears down that root's analysis host.
- Cross-root reference resolution: should a wikilink/tag/query in one workspace folder ever resolve against another open workspace folder, or does each folder get a fully independent analysis host with no cross-root visibility (matching the existing single-Project-Root model's isolation)? Decide this on its own merits — e.g. whether real PKM usage patterns (a personal vault plus a shared team vault open together) would benefit from cross-root resolution — rather than defaulting to isolation because that's the smaller change from today's single-root CLI.
